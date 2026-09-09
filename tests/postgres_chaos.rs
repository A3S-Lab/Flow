//! FLOW-R6 real-provider chaos certification against PostgreSQL.
//!
//! These gates prove concurrent-writer, lease, hook-token, and dead-letter
//! invariants on a production-capable SQL store. They skip when
//! `A3S_FLOW_POSTGRES_URL` is unset so local default suites stay offline-safe.

#![cfg(feature = "postgres")]

use a3s_flow::{
    migrate_postgres_flow, FlowError, FlowEvent, FlowEventStore, FlowTask, FlowTaskQueue,
    PostgresEventStore, PostgresFlowTaskQueue, WorkflowSpec,
};
use a3s_orm::PostgresExecutor;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Barrier;
use uuid::Uuid;

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn schema_scoped_url(postgres_url: &str, schema: &str) -> String {
    let separator = if postgres_url.contains('?') { '&' } else { '?' };
    format!("{postgres_url}{separator}options=-csearch_path%3D{schema}")
}

fn unique_schema(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}_{nanos}")
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("cert.postgres.chaos", "1", "tests::postgres_chaos", "main")
}

async fn migrated_store(
    postgres_url: &str,
    schema: &str,
) -> (PostgresExecutor, Arc<PostgresEventStore>) {
    let base = PostgresExecutor::connect_no_tls(postgres_url, 8).unwrap();
    base.connection()
        .await
        .unwrap()
        .batch_execute(&format!("CREATE SCHEMA {schema}"))
        .await
        .unwrap();
    let scoped_url = schema_scoped_url(postgres_url, schema);
    let scoped = PostgresExecutor::connect_no_tls(&scoped_url, 8).unwrap();
    migrate_postgres_flow(&scoped).await.unwrap();
    let store = Arc::new(
        PostgresEventStore::from_executor_verified(scoped.clone())
            .await
            .unwrap(),
    );
    (base, store)
}

async fn drop_schema(base: PostgresExecutor, schema: &str) {
    base.connection()
        .await
        .unwrap()
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
}

#[tokio::test]
async fn postgres_chaos_concurrent_writers_have_one_winner_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres chaos concurrent writers; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let schema = unique_schema("chaos_writers");
    let (base, store) = migrated_store(&url, &schema).await;
    let run_id = format!("chaos-writers-{}", Uuid::new_v4());

    store
        .append_if_sequence(
            &run_id,
            0,
            FlowEvent::RunCreated {
                spec: spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let left_store = Arc::clone(&store);
    let right_store = Arc::clone(&store);
    let left_barrier = Arc::clone(&barrier);
    let right_barrier = Arc::clone(&barrier);
    let left_run = run_id.clone();
    let right_run = run_id.clone();

    let left = tokio::spawn(async move {
        left_barrier.wait().await;
        left_store
            .append_if_sequence(&left_run, 1, FlowEvent::RunStarted)
            .await
    });
    let right = tokio::spawn(async move {
        right_barrier.wait().await;
        right_store
            .append_if_sequence(&right_run, 1, FlowEvent::RunStarted)
            .await
    });

    let left_result = left.await.unwrap();
    let right_result = right.await.unwrap();
    let winners = [&left_result, &right_result]
        .iter()
        .filter(|result| result.is_ok())
        .count();
    let conflicts = [&left_result, &right_result]
        .iter()
        .filter(|result| {
            matches!(
                result,
                Err(FlowError::EventConflict {
                    expected_sequence: 1,
                    actual_sequence: 2,
                    ..
                })
            )
        })
        .count();
    assert_eq!(winners, 1, "exactly one concurrent writer must commit");
    assert_eq!(conflicts, 1, "the loser must observe EventConflict");
    assert_eq!(store.list(&run_id).await.unwrap().len(), 2);

    drop_schema(base, &schema).await;
}

#[tokio::test]
async fn postgres_chaos_hook_token_claim_is_exclusive_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres chaos hook claim; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let schema = unique_schema("chaos_hooks");
    let (base, store) = migrated_store(&url, &schema).await;
    let first = format!("hook-a-{}", Uuid::new_v4());
    let second = format!("hook-b-{}", Uuid::new_v4());
    for run_id in [&first, &second] {
        store
            .append_if_sequence(
                run_id,
                0,
                FlowEvent::RunCreated {
                    spec: spec(),
                    input: json!({}),
                },
            )
            .await
            .unwrap();
        store
            .append_if_sequence(run_id, 1, FlowEvent::RunStarted)
            .await
            .unwrap();
    }

    let barrier = Arc::new(Barrier::new(2));
    let left_store = Arc::clone(&store);
    let right_store = Arc::clone(&store);
    let left_barrier = Arc::clone(&barrier);
    let right_barrier = Arc::clone(&barrier);
    let left_run = first.clone();
    let right_run = second.clone();

    let left = tokio::spawn(async move {
        left_barrier.wait().await;
        left_store
            .append_hook_if_token_available(
                &left_run,
                2,
                "h1".into(),
                "shared-chaos-token".into(),
                json!({}),
            )
            .await
    });
    let right = tokio::spawn(async move {
        right_barrier.wait().await;
        right_store
            .append_hook_if_token_available(
                &right_run,
                2,
                "h2".into(),
                "shared-chaos-token".into(),
                json!({}),
            )
            .await
    });

    let left_result = left.await.unwrap();
    let right_result = right.await.unwrap();
    let winners = [&left_result, &right_result]
        .iter()
        .filter(|result| result.is_ok())
        .count();
    let conflicts = [&left_result, &right_result]
        .iter()
        .filter(|result| matches!(result, Err(FlowError::HookTokenConflict { .. })))
        .count();
    assert_eq!(winners, 1, "exactly one hook claim must succeed");
    assert_eq!(conflicts, 1, "the duplicate token must conflict");

    drop_schema(base, &schema).await;
}

#[tokio::test]
async fn postgres_chaos_stale_lease_cannot_ack_after_heartbeat_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres chaos lease rotation; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let schema = unique_schema("chaos_lease");
    let (base, _store) = migrated_store(&url, &schema).await;
    let scoped_url = schema_scoped_url(&url, &schema);
    let queue_name = format!("chaos-lease-{}", Uuid::new_v4());
    let queue = PostgresFlowTaskQueue::connect_with_queue(&scoped_url, &queue_name)
        .await
        .unwrap();
    let task = FlowTask::DriveRun {
        run_id: format!("chaos-lease-run-{}", Uuid::new_v4()),
    };
    queue.enqueue(task).await.unwrap();
    let lease = queue.lease().await.unwrap().unwrap();
    let rotated = queue.heartbeat(&lease.lease_id).await.unwrap();
    assert_ne!(rotated, lease.lease_id);
    let stale = queue.ack(&lease.lease_id).await.unwrap_err();
    assert!(matches!(stale, FlowError::LeaseLost(_)));
    queue.ack(&rotated).await.unwrap();
    assert_eq!(queue.inflight_len().await.unwrap(), 0);

    drop_schema(base, &schema).await;
}

#[tokio::test]
async fn postgres_chaos_dead_letter_redrive_is_idempotent_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres chaos redrive; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let schema = unique_schema("chaos_redrive");
    let (base, _store) = migrated_store(&url, &schema).await;
    let scoped_url = schema_scoped_url(&url, &schema);
    let queue_name = format!("chaos-redrive-{}", Uuid::new_v4());
    let queue = PostgresFlowTaskQueue::connect_with_queue(&scoped_url, &queue_name)
        .await
        .unwrap();
    let task = FlowTask::DriveRun {
        run_id: format!("chaos-redrive-run-{}", Uuid::new_v4()),
    };
    queue.enqueue(task.clone()).await.unwrap();
    let lease = queue.lease().await.unwrap().unwrap();
    queue
        .dead_letter_inflight_older_than(
            Utc::now() + ChronoDuration::seconds(1),
            "postgres chaos redrive",
        )
        .await
        .unwrap();

    assert!(queue.redrive_dead_lettered(&lease.lease_id).await.unwrap());
    assert!(!queue.redrive_dead_lettered(&lease.lease_id).await.unwrap());
    assert_eq!(queue.dead_letter_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(queue.dequeue().await.unwrap(), Some(task));

    drop_schema(base, &schema).await;
}

#[tokio::test]
async fn postgres_chaos_competing_workers_lease_distinct_tasks_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres chaos competing workers; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let schema = unique_schema("chaos_compete");
    let (base, _store) = migrated_store(&url, &schema).await;
    let scoped_url = schema_scoped_url(&url, &schema);
    let queue_name = format!("chaos-compete-{}", Uuid::new_v4());
    let first = PostgresFlowTaskQueue::connect_with_queue(&scoped_url, &queue_name)
        .await
        .unwrap();
    let second = PostgresFlowTaskQueue::connect_with_queue(&scoped_url, &queue_name)
        .await
        .unwrap();
    let task_a = FlowTask::DriveRun {
        run_id: format!("compete-a-{}", Uuid::new_v4()),
    };
    let task_b = FlowTask::DriveRun {
        run_id: format!("compete-b-{}", Uuid::new_v4()),
    };
    first.enqueue(task_a.clone()).await.unwrap();
    first.enqueue(task_b.clone()).await.unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let left_queue = first.clone();
    let right_queue = second.clone();
    let left_barrier = Arc::clone(&barrier);
    let right_barrier = Arc::clone(&barrier);
    let left = tokio::spawn(async move {
        left_barrier.wait().await;
        left_queue.lease().await
    });
    let right = tokio::spawn(async move {
        right_barrier.wait().await;
        right_queue.lease().await
    });
    let left_lease = left.await.unwrap().unwrap().unwrap();
    let right_lease = right.await.unwrap().unwrap().unwrap();
    assert_ne!(left_lease.lease_id, right_lease.lease_id);
    assert_ne!(left_lease.task, right_lease.task);
    let leased = [left_lease.task, right_lease.task];
    assert!(leased.contains(&task_a) && leased.contains(&task_b));

    drop_schema(base, &schema).await;
}
