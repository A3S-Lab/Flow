#![cfg(feature = "sqlite")]

use a3s_flow::{
    CancellationRequest, FlowEvent, FlowEventStore, RetryPolicy, ScheduledWakeupKind,
    SqliteEventStore, WorkflowSpec,
};
use a3s_orm::{sql_query, Database, Migration, Migrator, SqliteDialect, SqliteExecutor};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

const LEGACY_EVENTS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_events (
    run_id TEXT NOT NULL,
    sequence BIGINT NOT NULL CHECK (sequence >= 1),
    event_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    event_json TEXT NOT NULL,
    PRIMARY KEY (run_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_flow_events_run_id_sequence
ON flow_events (run_id, sequence);
"#;

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.sqlite-scheduled-wakeups",
        "1",
        "tests::sqlite_scheduled_wakeups",
        "main",
    )
}

async fn create_run(store: &SqliteEventStore, run_id: &str) {
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
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
}

async fn insert_raw_event(
    executor: &SqliteExecutor,
    run_id: &str,
    sequence: i64,
    event: FlowEvent,
) {
    Database::new(SqliteDialect, executor.clone())
        .execute(
            sql_query::<()>(
                "INSERT INTO flow_events (run_id, sequence, event_id, timestamp, event_json) VALUES (",
            )
            .bind(run_id)
            .append(", ")
            .bind(sequence)
            .append(", ")
            .bind(Uuid::new_v4().to_string())
            .append(", ")
            .bind(Utc::now().to_rfc3339())
            .append(", ")
            .bind(serde_json::to_string(&event).unwrap())
            .append(")"),
        )
        .await
        .unwrap();
}

async fn scheduled_rows(executor: &SqliteExecutor) -> Vec<(String, i64, String, String, i64)> {
    Database::new(SqliteDialect, executor.clone())
        .fetch_all_as(sql_query::<(String, i64, String, String, i64)>(
            "SELECT run_id, wakeup_kind, subject_id, scheduled_at_key, created_sequence \
             FROM flow_scheduled_wakeups \
             ORDER BY scheduled_at_key, run_id, wakeup_kind, subject_id",
        ))
        .await
        .unwrap()
        .rows
}

#[tokio::test]
async fn sqlite_scheduled_wakeup_projection_preserves_nanosecond_boundaries() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-scheduled-lifecycle";
    create_run(&store, run_id).await;

    let wait_at = timestamp("2026-08-07T00:00:01.000000100Z");
    store
        .append(
            run_id,
            FlowEvent::WaitCreated {
                wait_id: "timer".into(),
                resume_at: wait_at,
            },
        )
        .await
        .unwrap();

    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(
        rows,
        vec![(
            run_id.into(),
            0,
            "timer".into(),
            "2026-08-07T00:00:01.000000100Z".into(),
            3,
        )]
    );
    assert!(store
        .list_due_wakeups(timestamp("2026-08-07T00:00:01.000000099Z"))
        .await
        .unwrap()
        .is_empty());
    let due = store.list_due_wakeups(wait_at).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].kind, ScheduledWakeupKind::Wait);
    assert_eq!(due[0].subject_id, "timer");

    let next = store.next_scheduled_wakeup().await.unwrap().unwrap();
    assert_eq!(next.scheduled_at, wait_at);
    store
        .append(
            run_id,
            FlowEvent::WaitCompleted {
                wait_id: "timer".into(),
            },
        )
        .await
        .unwrap();
    assert!(scheduled_rows(store.executor()).await.is_empty());

    let retry_at = timestamp("2026-08-07T00:00:02.000000200Z");
    store
        .append(
            run_id,
            FlowEvent::StepCreated {
                step_id: "flaky".into(),
                step_name: "flakyStep".into(),
                input: json!({}),
                retry: RetryPolicy::fixed(3, Duration::from_secs(1)),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::StepStarted {
                step_id: "flaky".into(),
                attempt: 1,
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::StepRetrying {
                step_id: "flaky".into(),
                attempt: 1,
                error: "retry later".into(),
                retry_after: Some(retry_at),
            },
        )
        .await
        .unwrap();

    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 2);
    assert_eq!(rows[0].2, "flaky");
    assert_eq!(rows[0].3, "2026-08-07T00:00:02.000000200Z");
    assert!(store
        .list_due_wakeups(timestamp("2026-08-07T00:00:02.000000199Z"))
        .await
        .unwrap()
        .is_empty());
    let due = store.list_due_wakeups(retry_at).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].kind, ScheduledWakeupKind::Retry);

    store
        .append(
            run_id,
            FlowEvent::StepStarted {
                step_id: "flaky".into(),
                attempt: 2,
            },
        )
        .await
        .unwrap();
    assert!(scheduled_rows(store.executor()).await.is_empty());

    store
        .append(
            run_id,
            FlowEvent::StepRetrying {
                step_id: "flaky".into(),
                attempt: 2,
                error: "retry immediately".into(),
                retry_after: None,
            },
        )
        .await
        .unwrap();
    assert!(scheduled_rows(store.executor()).await.is_empty());

    store
        .append(
            run_id,
            FlowEvent::RunCancellationRequested {
                request: CancellationRequest::new(Some("cleanup required".into())),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::WaitCreated {
                wait_id: "cleanup".into(),
                resume_at: timestamp("2026-08-07T00:00:03Z"),
            },
        )
        .await
        .unwrap();
    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "cleanup");
    assert_eq!(rows[0].3, "2026-08-07T00:00:03.000000000Z");
    store
        .append(
            run_id,
            FlowEvent::RunCancelled {
                reason: Some("cleanup complete".into()),
            },
        )
        .await
        .unwrap();
    assert!(scheduled_rows(store.executor()).await.is_empty());
}

#[tokio::test]
async fn sqlite_scheduled_wakeup_migration_backfills_and_tracks_legacy_writers() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("flow.db");
    let executor = SqliteExecutor::open(&database_path).await.unwrap();
    Migrator::new(executor.clone())
        .run([Migration::new(
            "a3s-flow-0001-events",
            "create Flow event history",
            LEGACY_EVENTS_SQL,
        )])
        .await
        .unwrap();

    let wait_run = "sqlite-wakeup-upgrade-wait";
    insert_raw_event(
        &executor,
        wait_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, wait_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        wait_run,
        3,
        FlowEvent::WaitCreated {
            wait_id: "legacy-wait".into(),
            resume_at: timestamp("2026-08-07T01:00:00.123456789Z"),
        },
    )
    .await;

    let completed_wait_run = "sqlite-wakeup-upgrade-completed-wait";
    insert_raw_event(
        &executor,
        completed_wait_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, completed_wait_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        completed_wait_run,
        3,
        FlowEvent::WaitCreated {
            wait_id: "already-completed".into(),
            resume_at: timestamp("2026-08-07T01:00:00.500000000Z"),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        completed_wait_run,
        4,
        FlowEvent::WaitCompleted {
            wait_id: "already-completed".into(),
        },
    )
    .await;

    let retry_run = "sqlite-wakeup-upgrade-retry";
    insert_raw_event(
        &executor,
        retry_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, retry_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        retry_run,
        3,
        FlowEvent::StepCreated {
            step_id: "legacy-retry".into(),
            step_name: "legacyRetry".into(),
            input: json!({}),
            retry: RetryPolicy::fixed(3, Duration::from_secs(1)),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        retry_run,
        4,
        FlowEvent::StepStarted {
            step_id: "legacy-retry".into(),
            attempt: 1,
        },
    )
    .await;
    insert_raw_event(
        &executor,
        retry_run,
        5,
        FlowEvent::StepRetrying {
            step_id: "legacy-retry".into(),
            attempt: 1,
            error: "legacy retry".into(),
            retry_after: Some(timestamp("2026-08-07T01:00:01.987654321Z")),
        },
    )
    .await;

    let cancelling_run = "sqlite-wakeup-upgrade-cancelling";
    insert_raw_event(
        &executor,
        cancelling_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, cancelling_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        cancelling_run,
        3,
        FlowEvent::WaitCreated {
            wait_id: "pre-cancellation".into(),
            resume_at: timestamp("2026-08-07T01:00:02Z"),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        cancelling_run,
        4,
        FlowEvent::RunCancellationRequested {
            request: CancellationRequest::new(Some("cleanup required".into())),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        cancelling_run,
        5,
        FlowEvent::WaitCreated {
            wait_id: "cleanup".into(),
            resume_at: timestamp("2026-08-07T01:00:03Z"),
        },
    )
    .await;
    drop(executor);

    let store = SqliteEventStore::connect(format!("sqlite://{}", database_path.display()))
        .await
        .unwrap();
    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().any(|row| {
        row.0 == wait_run
            && row.1 == 0
            && row.2 == "legacy-wait"
            && row.3 == "2026-08-07T01:00:00.123456789Z"
    }));
    assert!(rows.iter().any(|row| {
        row.0 == retry_run
            && row.1 == 2
            && row.2 == "legacy-retry"
            && row.3 == "2026-08-07T01:00:01.987654321Z"
    }));
    assert!(rows
        .iter()
        .any(|row| row.0 == cancelling_run && row.2 == "cleanup"));
    assert!(!rows.iter().any(|row| row.2 == "pre-cancellation"));
    assert!(!rows.iter().any(|row| row.2 == "already-completed"));

    insert_raw_event(
        store.executor(),
        wait_run,
        4,
        FlowEvent::WaitCompleted {
            wait_id: "legacy-wait".into(),
        },
    )
    .await;
    insert_raw_event(
        store.executor(),
        retry_run,
        6,
        FlowEvent::StepStarted {
            step_id: "legacy-retry".into(),
            attempt: 2,
        },
    )
    .await;
    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "cleanup");

    insert_raw_event(
        store.executor(),
        cancelling_run,
        6,
        FlowEvent::RunCancelled {
            reason: Some("cleanup complete".into()),
        },
    )
    .await;
    assert!(scheduled_rows(store.executor()).await.is_empty());
}
