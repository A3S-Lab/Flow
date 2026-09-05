#![cfg(feature = "postgres")]

use a3s_flow::{
    CancellationRequest, FlowEvent, FlowEventStore, PostgresEventStore, RetryPolicy,
    RuntimeBuildId, ScheduledWakeupKind, WorkflowSpec,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
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

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.postgres-scheduled-wakeups",
        "1",
        "tests::postgres_scheduled_wakeups",
        "main",
    )
}

async fn create_run(store: &PostgresEventStore, run_id: &str) {
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

#[tokio::test]
async fn postgres_continue_as_new_closes_indexed_hooks_and_wakeups() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let store = PostgresEventStore::connect(&postgres_url).await.unwrap();
    let scope = Uuid::new_v4();
    let run_id = format!("postgres-continue-cleanup-{scope}");
    create_run(&store, &run_id).await;
    store
        .append(
            &run_id,
            FlowEvent::HookCreated {
                hook_id: "approval".into(),
                token: format!("postgres-continue-token-{scope}"),
                metadata: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append(
            &run_id,
            FlowEvent::WaitCreated {
                wait_id: "timer".into(),
                resume_at: timestamp("2200-08-07T00:00:01Z"),
            },
        )
        .await
        .unwrap();
    store
        .append(
            &run_id,
            FlowEvent::RunContinuedAsNew {
                successor_run_id: format!("postgres-continue-successor-{scope}"),
                input: json!({ "generation": 1 }),
            },
        )
        .await
        .unwrap();

    assert!(store
        .list_active_hooks()
        .await
        .unwrap()
        .into_iter()
        .all(|active| active.run_id != run_id));
    assert!(store
        .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
        .await
        .unwrap()
        .into_iter()
        .all(|wakeup| wakeup.run_id != run_id));
}

#[tokio::test]
async fn postgres_indexed_wakeups_include_the_persisted_runtime_build() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let store = PostgresEventStore::connect(&postgres_url).await.unwrap();
    let run_id = format!("postgres-build-routed-wakeup-{}", Uuid::new_v4());
    let build_id = RuntimeBuildId::new("worker-v2").unwrap();
    store
        .append_if_sequence(
            &run_id,
            0,
            FlowEvent::RunCreated {
                spec: spec().with_runtime_build(build_id.clone()),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store.append(&run_id, FlowEvent::RunStarted).await.unwrap();
    let wait_at = timestamp("2200-08-07T00:00:01Z");
    store
        .append(
            &run_id,
            FlowEvent::WaitCreated {
                wait_id: "timer".into(),
                resume_at: wait_at,
            },
        )
        .await
        .unwrap();

    let due = store
        .list_due_wakeups(wait_at)
        .await
        .unwrap()
        .into_iter()
        .find(|wakeup| wakeup.run_id == run_id)
        .unwrap();
    assert_eq!(due.runtime_build_id.as_ref(), Some(&build_id));

    store
        .append(
            &run_id,
            FlowEvent::WaitCompleted {
                wait_id: "timer".into(),
            },
        )
        .await
        .unwrap();
}

async fn insert_raw_event(
    executor: &PostgresExecutor,
    run_id: &str,
    sequence: i64,
    event: FlowEvent,
) {
    Database::new(PostgresDialect, executor.clone())
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

async fn scheduled_rows(
    store: &PostgresEventStore,
    run_id: &str,
) -> Vec<(String, i64, String, String, i64)> {
    Database::new(PostgresDialect, store.executor().clone())
        .fetch_all_as(
            sql_query::<(String, i64, String, String, i64)>(
                "SELECT run_id, wakeup_kind, subject_id, scheduled_at_key, created_sequence \
                 FROM flow_scheduled_wakeups WHERE run_id = ",
            )
            .bind(run_id)
            .append(" ORDER BY scheduled_at_key, wakeup_kind, subject_id"),
        )
        .await
        .unwrap()
        .rows
}

fn schema_scoped_url(postgres_url: &str, schema: &str) -> String {
    let separator = if postgres_url.contains('?') { '&' } else { '?' };
    format!("{postgres_url}{separator}options=-csearch_path%3D{schema}")
}

#[tokio::test]
async fn postgres_scheduled_wakeup_migration_backfills_legacy_history() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let schema = format!("flow_wakeup_upgrade_{}", Uuid::new_v4().simple());
    let base_executor = PostgresExecutor::connect_no_tls(&postgres_url, 2).unwrap();
    let base_client = base_executor.connection().await.unwrap();
    // The identifier is generated exclusively from a UUID and cannot contain
    // user-controlled SQL. PostgreSQL identifiers cannot be query parameters.
    base_client
        .batch_execute(&format!("CREATE SCHEMA {schema}"))
        .await
        .unwrap();
    drop(base_client);

    let scoped_url = schema_scoped_url(&postgres_url, &schema);
    let scoped_executor = PostgresExecutor::connect_no_tls(&scoped_url, 2).unwrap();
    scoped_executor
        .connection()
        .await
        .unwrap()
        .batch_execute(LEGACY_EVENTS_SQL)
        .await
        .unwrap();

    let wait_run = format!("postgres-upgrade-wait-{}", Uuid::new_v4());
    insert_raw_event(
        &scoped_executor,
        &wait_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&scoped_executor, &wait_run, 2, FlowEvent::RunStarted).await;
    let wait_at = timestamp("2200-08-07T01:00:00.123456789Z");
    insert_raw_event(
        &scoped_executor,
        &wait_run,
        3,
        FlowEvent::WaitCreated {
            wait_id: "legacy-wait".into(),
            resume_at: wait_at,
        },
    )
    .await;

    let retry_run = format!("postgres-upgrade-retry-{}", Uuid::new_v4());
    insert_raw_event(
        &scoped_executor,
        &retry_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&scoped_executor, &retry_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &scoped_executor,
        &retry_run,
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
        &scoped_executor,
        &retry_run,
        4,
        FlowEvent::StepStarted {
            step_id: "legacy-retry".into(),
            attempt: 1,
        },
    )
    .await;
    let retry_at = timestamp("2200-08-07T01:00:01.987654321Z");
    insert_raw_event(
        &scoped_executor,
        &retry_run,
        5,
        FlowEvent::StepRetrying {
            step_id: "legacy-retry".into(),
            attempt: 1,
            error: "retry after upgrade".into(),
            retry_after: Some(retry_at),
        },
    )
    .await;

    let continued_run = format!("postgres-upgrade-continued-{}", Uuid::new_v4());
    insert_raw_event(
        &scoped_executor,
        &continued_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&scoped_executor, &continued_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &scoped_executor,
        &continued_run,
        3,
        FlowEvent::HookCreated {
            hook_id: "legacy-hook".into(),
            token: format!("postgres-upgrade-continued-token-{}", Uuid::new_v4()),
            metadata: json!({}),
        },
    )
    .await;
    insert_raw_event(
        &scoped_executor,
        &continued_run,
        4,
        FlowEvent::WaitCreated {
            wait_id: "legacy-continued-wait".into(),
            resume_at: timestamp("2200-08-07T01:00:02Z"),
        },
    )
    .await;
    insert_raw_event(
        &scoped_executor,
        &continued_run,
        5,
        FlowEvent::RunContinuedAsNew {
            successor_run_id: format!("postgres-upgrade-successor-{}", Uuid::new_v4()),
            input: json!({ "generation": 1 }),
        },
    )
    .await;

    let store = PostgresEventStore::from_executor(scoped_executor.clone())
        .await
        .unwrap();
    let wait_rows = scheduled_rows(&store, &wait_run).await;
    let retry_rows = scheduled_rows(&store, &retry_run).await;
    let continued_rows = scheduled_rows(&store, &continued_run).await;
    let continued_hook_exists = store
        .list_active_hooks()
        .await
        .unwrap()
        .into_iter()
        .any(|active| active.run_id == continued_run);
    let due_before_wait = store
        .list_due_wakeups(timestamp("2200-08-07T01:00:00.123456788Z"))
        .await
        .unwrap();
    let due_at_wait = store.list_due_wakeups(wait_at).await.unwrap();

    drop(store);
    drop(scoped_executor);
    base_executor
        .connection()
        .await
        .unwrap()
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();

    assert_eq!(wait_rows.len(), 1);
    assert_eq!(wait_rows[0].1, 0);
    assert_eq!(wait_rows[0].2, "legacy-wait");
    assert_eq!(wait_rows[0].3, "2200-08-07T01:00:00.123456789Z");
    assert_eq!(retry_rows.len(), 1);
    assert_eq!(retry_rows[0].1, 2);
    assert_eq!(retry_rows[0].2, "legacy-retry");
    assert_eq!(retry_rows[0].3, "2200-08-07T01:00:01.987654321Z");
    assert!(continued_rows.is_empty());
    assert!(!continued_hook_exists);
    assert!(!due_before_wait
        .iter()
        .any(|wakeup| wakeup.run_id == wait_run));
    assert!(due_at_wait.iter().any(|wakeup| {
        wakeup.run_id == wait_run
            && wakeup.kind == ScheduledWakeupKind::Wait
            && wakeup.scheduled_at == wait_at
    }));
}

#[tokio::test]
async fn postgres_scheduled_wakeup_trigger_tracks_legacy_writers_and_nanoseconds() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let scope = Uuid::new_v4();
    let run_id = format!("postgres-scheduled-{scope}");
    let store = PostgresEventStore::connect(&postgres_url).await.unwrap();
    create_run(&store, &run_id).await;

    let wait_at = timestamp("2200-08-07T00:00:01.000000100Z");
    insert_raw_event(
        store.executor(),
        &run_id,
        3,
        FlowEvent::WaitCreated {
            wait_id: "legacy-wait".into(),
            resume_at: wait_at,
        },
    )
    .await;
    let rows = scheduled_rows(&store, &run_id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 0);
    assert_eq!(rows[0].2, "legacy-wait");
    assert_eq!(rows[0].3, "2200-08-07T00:00:01.000000100Z");
    assert_eq!(rows[0].4, 3);

    assert!(!store
        .list_due_wakeups(timestamp("2200-08-07T00:00:01.000000099Z"))
        .await
        .unwrap()
        .iter()
        .any(|wakeup| wakeup.run_id == run_id));
    let due = store
        .list_due_wakeups(wait_at)
        .await
        .unwrap()
        .into_iter()
        .find(|wakeup| wakeup.run_id == run_id)
        .unwrap();
    assert_eq!(due.kind, ScheduledWakeupKind::Wait);
    assert_eq!(due.subject_id, "legacy-wait");

    insert_raw_event(
        store.executor(),
        &run_id,
        4,
        FlowEvent::WaitCompleted {
            wait_id: "legacy-wait".into(),
        },
    )
    .await;
    assert!(scheduled_rows(&store, &run_id).await.is_empty());

    let retry_at = timestamp("2200-08-07T00:00:02.000000200Z");
    insert_raw_event(
        store.executor(),
        &run_id,
        5,
        FlowEvent::StepCreated {
            step_id: "legacy-retry".into(),
            step_name: "legacyRetry".into(),
            input: json!({}),
            retry: RetryPolicy::fixed(3, Duration::from_secs(1)),
        },
    )
    .await;
    insert_raw_event(
        store.executor(),
        &run_id,
        6,
        FlowEvent::StepStarted {
            step_id: "legacy-retry".into(),
            attempt: 1,
        },
    )
    .await;
    insert_raw_event(
        store.executor(),
        &run_id,
        7,
        FlowEvent::StepRetrying {
            step_id: "legacy-retry".into(),
            attempt: 1,
            error: "retry later".into(),
            retry_after: Some(retry_at),
        },
    )
    .await;
    let rows = scheduled_rows(&store, &run_id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 2);
    assert_eq!(rows[0].2, "legacy-retry");
    assert_eq!(rows[0].3, "2200-08-07T00:00:02.000000200Z");

    insert_raw_event(
        store.executor(),
        &run_id,
        8,
        FlowEvent::StepStarted {
            step_id: "legacy-retry".into(),
            attempt: 2,
        },
    )
    .await;
    assert!(scheduled_rows(&store, &run_id).await.is_empty());

    insert_raw_event(
        store.executor(),
        &run_id,
        9,
        FlowEvent::StepRetrying {
            step_id: "legacy-retry".into(),
            attempt: 2,
            error: "retry immediately".into(),
            retry_after: None,
        },
    )
    .await;
    assert!(scheduled_rows(&store, &run_id).await.is_empty());

    let cancelled_run = format!("postgres-scheduled-cancelled-{}", Uuid::new_v4());
    create_run(&store, &cancelled_run).await;
    insert_raw_event(
        store.executor(),
        &cancelled_run,
        3,
        FlowEvent::StepCreated {
            step_id: "cancelled-retry".into(),
            step_name: "cancelledRetry".into(),
            input: json!({}),
            retry: RetryPolicy::fixed(3, Duration::from_secs(1)),
        },
    )
    .await;
    insert_raw_event(
        store.executor(),
        &cancelled_run,
        4,
        FlowEvent::StepStarted {
            step_id: "cancelled-retry".into(),
            attempt: 1,
        },
    )
    .await;
    insert_raw_event(
        store.executor(),
        &cancelled_run,
        5,
        FlowEvent::StepRetrying {
            step_id: "cancelled-retry".into(),
            attempt: 1,
            error: "ambiguous sibling abort".into(),
            retry_after: Some(timestamp("2200-08-07T00:00:04Z")),
        },
    )
    .await;
    assert_eq!(scheduled_rows(&store, &cancelled_run).await.len(), 1);
    insert_raw_event(
        store.executor(),
        &cancelled_run,
        6,
        FlowEvent::StepCancelled {
            step_id: "cancelled-retry".into(),
            attempt: 1,
            reason: "batch sibling outcome is unknown".into(),
        },
    )
    .await;
    assert!(scheduled_rows(&store, &cancelled_run).await.is_empty());

    insert_raw_event(
        store.executor(),
        &run_id,
        10,
        FlowEvent::RunCancellationRequested {
            request: CancellationRequest::new(Some("cleanup required".into())),
        },
    )
    .await;
    insert_raw_event(
        store.executor(),
        &run_id,
        11,
        FlowEvent::WaitCreated {
            wait_id: "cleanup-wait".into(),
            resume_at: timestamp("2200-08-07T00:00:03.000000300Z"),
        },
    )
    .await;
    let rows = scheduled_rows(&store, &run_id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "cleanup-wait");
    insert_raw_event(
        store.executor(),
        &run_id,
        12,
        FlowEvent::RunCancelled {
            reason: Some("test cleanup".into()),
        },
    )
    .await;
    assert!(scheduled_rows(&store, &run_id).await.is_empty());
}
