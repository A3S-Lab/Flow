#![cfg(feature = "sqlite")]

use a3s_flow::{
    CancellationRequest, FlowError, FlowEvent, FlowEventStore, RetryPolicy, RuntimeBuildId,
    ScheduledWakeupKind, SelectArm, SelectMode, SqliteEventStore, WorkflowSpec,
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

#[tokio::test]
async fn sqlite_scope_cancel_drops_indexed_waits() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let now = timestamp("2300-01-01T00:00:00Z");

    let sibling = "sqlite-scope-cancel-sibling";
    create_run(&store, sibling).await;
    store
        .append(
            sibling,
            FlowEvent::WaitCreated {
                wait_id: "outside".into(),
                resume_at: timestamp("2200-08-07T00:00:01Z"),
            },
        )
        .await
        .unwrap();
    store
        .append(
            sibling,
            FlowEvent::ScopeOpened {
                scope_id: "outer".into(),
                parent_scope_id: None,
            },
        )
        .await
        .unwrap();
    store
        .append(
            sibling,
            FlowEvent::WaitCreated {
                wait_id: "inside".into(),
                resume_at: timestamp("2200-08-07T00:00:02Z"),
            },
        )
        .await
        .unwrap();
    store
        .append(
            sibling,
            FlowEvent::ScopeCancelled {
                scope_id: "outer".into(),
                reason: None,
            },
        )
        .await
        .unwrap();
    let subjects: Vec<_> = store
        .list_due_wakeups(now)
        .await
        .unwrap()
        .into_iter()
        .map(|wakeup| wakeup.subject_id)
        .collect();
    assert_eq!(
        subjects,
        vec!["outside".to_string()],
        "cancelling a scope must drop its waits and keep waits outside it"
    );

    let nested = "sqlite-scope-cancel-nested";
    create_run(&store, nested).await;
    store
        .append(
            nested,
            FlowEvent::ScopeOpened {
                scope_id: "parent".into(),
                parent_scope_id: None,
            },
        )
        .await
        .unwrap();
    store
        .append(
            nested,
            FlowEvent::ScopeOpened {
                scope_id: "child".into(),
                parent_scope_id: Some("parent".into()),
            },
        )
        .await
        .unwrap();
    store
        .append(
            nested,
            FlowEvent::WaitCreated {
                wait_id: "nested-wait".into(),
                resume_at: timestamp("2200-08-07T00:00:03Z"),
            },
        )
        .await
        .unwrap();
    store
        .append(
            nested,
            FlowEvent::SelectCreated {
                select_id: "nested-race".into(),
                arms: vec![
                    SelectArm::timer("nested-timer", timestamp("2200-08-07T00:00:04Z")),
                    SelectArm::timer("nested-later", timestamp("2200-08-07T00:00:05Z")),
                ],
                mode: SelectMode::Race,
            },
        )
        .await
        .unwrap();
    store
        .append(
            nested,
            FlowEvent::ScopeCancelled {
                scope_id: "parent".into(),
                reason: None,
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .list_due_wakeups(now)
            .await
            .unwrap()
            .iter()
            .all(|wakeup| wakeup.run_id != nested),
        "cancelling a parent scope must drop waits and select timers in child scopes"
    );
}

#[tokio::test]
async fn sqlite_scope_cancel_keeps_a_wait_owned_by_a_completed_child() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-scope-cancel-completed-child";
    create_run(&store, run_id).await;
    store
        .append(
            run_id,
            FlowEvent::ScopeOpened {
                scope_id: "parent".into(),
                parent_scope_id: None,
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ScopeOpened {
                scope_id: "child".into(),
                parent_scope_id: Some("parent".into()),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::WaitCreated {
                wait_id: "survives".into(),
                resume_at: timestamp("2200-08-07T00:00:01Z"),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ScopeCompleted {
                scope_id: "child".into(),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::WaitCreated {
                wait_id: "parent-wait".into(),
                resume_at: timestamp("2200-08-07T00:00:02Z"),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ScopeCancelled {
                scope_id: "parent".into(),
                reason: None,
            },
        )
        .await
        .unwrap();

    let subjects: Vec<_> = store
        .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
        .await
        .unwrap()
        .into_iter()
        .filter(|wakeup| wakeup.run_id == run_id)
        .map(|wakeup| wakeup.subject_id)
        .collect();
    assert_eq!(
        subjects,
        vec!["survives".to_string()],
        "a timer owned by a completed child must stay indexed when an ancestor is cancelled"
    );
}

#[tokio::test]
async fn sqlite_select_timer_arm_is_a_scheduled_wakeup() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-select-timer";
    create_run(&store, run_id).await;
    store
        .append(
            run_id,
            FlowEvent::SelectCreated {
                select_id: "race".into(),
                arms: vec![
                    SelectArm::timer("soon", timestamp("2200-08-07T00:00:01Z")),
                    SelectArm::timer("later", timestamp("2200-08-07T00:00:02Z")),
                ],
                mode: SelectMode::Race,
            },
        )
        .await
        .unwrap();

    let due = store
        .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
        .await
        .unwrap();
    let subjects: Vec<_> = due
        .iter()
        .map(|wakeup| wakeup.subject_id.as_str())
        .collect();
    assert!(
        subjects.contains(&"soon") && subjects.contains(&"later"),
        "select timer arms must be indexed wakeups, got {subjects:?}"
    );
    let next = store
        .next_scheduled_wakeup()
        .await
        .unwrap()
        .expect("the earlier select timer must be the next wakeup");
    assert_eq!(next.subject_id, "soon");
    assert_eq!(next.kind, ScheduledWakeupKind::Wait);

    store
        .append(
            run_id,
            FlowEvent::WaitCompleted {
                wait_id: "soon".into(),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::SelectCompleted {
                select_id: "race".into(),
                winning_arm_id: Some("soon".into()),
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
            .await
            .unwrap()
            .is_empty(),
        "a lost select timer must leave the wakeup index"
    );

    let signal_run = "sqlite-select-signal-timer";
    store
        .append_if_sequence(
            signal_run,
            0,
            FlowEvent::RunCreated {
                spec: spec().with_signal("approved"),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append(signal_run, FlowEvent::RunStarted)
        .await
        .unwrap();
    store
        .append(
            signal_run,
            FlowEvent::SelectCreated {
                select_id: "signal-race".into(),
                arms: vec![
                    SelectArm::timer("timer-arm", timestamp("2200-08-07T00:00:03Z")),
                    SelectArm::signal("signal-arm", "approved"),
                ],
                mode: SelectMode::Race,
            },
        )
        .await
        .unwrap();
    let signal_race = store
        .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(
        signal_race
            .iter()
            .map(|wakeup| wakeup.subject_id.as_str())
            .collect::<Vec<_>>(),
        vec!["timer-arm"]
    );
    store
        .append(
            signal_run,
            FlowEvent::SelectCompleted {
                select_id: "signal-race".into(),
                winning_arm_id: Some("signal-arm".into()),
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
            .await
            .unwrap()
            .is_empty(),
        "a signal winner must drop the losing timer arm"
    );
}

#[tokio::test]
async fn sqlite_continue_as_new_closes_indexed_hooks_and_wakeups() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-continue-as-new-cleanup";
    create_run(&store, run_id).await;
    store
        .append(
            run_id,
            FlowEvent::HookCreated {
                hook_id: "approval".into(),
                token: "sqlite-continue-token".into(),
                metadata: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::WaitCreated {
                wait_id: "timer".into(),
                resume_at: timestamp("2200-08-07T00:00:01Z"),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::RunContinuedAsNew {
                successor_run_id: "sqlite-continue-successor".into(),
                input: json!({ "generation": 1 }),
            },
        )
        .await
        .unwrap();

    assert!(store.list_active_hooks().await.unwrap().is_empty());
    assert!(store
        .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn sqlite_persists_and_rejects_an_unsupported_event_schema_version() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-event-schema-version";
    create_run(&store, run_id).await;

    let database = Database::new(SqliteDialect, store.executor().clone());
    let version = database
        .fetch_all_as(
            sql_query::<i64>("SELECT schema_version FROM flow_events WHERE run_id = ")
                .bind(run_id)
                .append(" ORDER BY sequence ASC LIMIT 1"),
        )
        .await
        .unwrap()
        .rows
        .into_iter()
        .next()
        .expect("the first event has a persisted schema version");
    assert_eq!(version, 1);

    database
        .execute(
            sql_query::<()>("UPDATE flow_events SET schema_version = 2 WHERE run_id = ")
                .bind(run_id),
        )
        .await
        .unwrap();
    let error = store.list(run_id).await.unwrap_err();
    assert!(matches!(
        error,
        FlowError::UnsupportedEventSchemaVersion {
            version: 2,
            supported: 1
        }
    ));
}

#[tokio::test]
async fn sqlite_store_advertises_production_execution_guarantees() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    assert!(store.capabilities().production_ready());
}

#[tokio::test]
async fn sqlite_indexed_wakeups_include_the_persisted_runtime_build() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-build-routed-wakeup";
    let build_id = RuntimeBuildId::new("worker-v2").unwrap();
    store
        .append_if_sequence(
            run_id,
            0,
            FlowEvent::RunCreated {
                spec: spec().with_runtime_build(build_id.clone()),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    let wait_at = timestamp("2026-08-07T00:00:01Z");
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

    let due = store.list_due_wakeups(wait_at).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].runtime_build_id.as_ref(), Some(&build_id));
    assert_eq!(
        store
            .next_scheduled_wakeup()
            .await
            .unwrap()
            .unwrap()
            .runtime_build_id
            .as_ref(),
        Some(&build_id)
    );
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
            FlowEvent::StepCreated {
                step_id: "cancelled-retry".into(),
                step_name: "cancelledRetry".into(),
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
                step_id: "cancelled-retry".into(),
                attempt: 1,
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::StepRetrying {
                step_id: "cancelled-retry".into(),
                attempt: 1,
                error: "ambiguous sibling abort".into(),
                retry_after: Some(retry_at),
            },
        )
        .await
        .unwrap();
    assert!(scheduled_rows(store.executor())
        .await
        .iter()
        .any(|row| { row.1 == 2 && row.2 == "cancelled-retry" }));
    store
        .append(
            run_id,
            FlowEvent::StepCancelled {
                step_id: "cancelled-retry".into(),
                attempt: 1,
                reason: "batch sibling outcome is unknown".into(),
            },
        )
        .await
        .unwrap();
    assert!(!scheduled_rows(store.executor())
        .await
        .iter()
        .any(|row| row.2 == "cancelled-retry"));

    store
        .append(
            run_id,
            FlowEvent::StepCreated {
                step_id: "instant".into(),
                step_name: "instantStep".into(),
                input: json!({}),
                retry: RetryPolicy::fixed(3, Duration::ZERO),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::StepStarted {
                step_id: "instant".into(),
                attempt: 1,
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::StepRetrying {
                step_id: "instant".into(),
                attempt: 1,
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

    let cancelled_retry_run = "sqlite-wakeup-upgrade-cancelled-retry";
    insert_raw_event(
        &executor,
        cancelled_retry_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, cancelled_retry_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        cancelled_retry_run,
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
        &executor,
        cancelled_retry_run,
        4,
        FlowEvent::StepStarted {
            step_id: "cancelled-retry".into(),
            attempt: 1,
        },
    )
    .await;
    insert_raw_event(
        &executor,
        cancelled_retry_run,
        5,
        FlowEvent::StepRetrying {
            step_id: "cancelled-retry".into(),
            attempt: 1,
            error: "ambiguous sibling abort".into(),
            retry_after: Some(timestamp("2026-08-07T01:00:01.000000000Z")),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        cancelled_retry_run,
        6,
        FlowEvent::StepCancelled {
            step_id: "cancelled-retry".into(),
            attempt: 1,
            reason: "batch sibling outcome is unknown".into(),
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

    let continued_run = "sqlite-wakeup-upgrade-continued";
    insert_raw_event(
        &executor,
        continued_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, continued_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        continued_run,
        3,
        FlowEvent::HookCreated {
            hook_id: "legacy-hook".into(),
            token: "sqlite-upgrade-continued-token".into(),
            metadata: json!({}),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        continued_run,
        4,
        FlowEvent::WaitCreated {
            wait_id: "legacy-continued-wait".into(),
            resume_at: timestamp("2026-08-07T01:00:04Z"),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        continued_run,
        5,
        FlowEvent::RunContinuedAsNew {
            successor_run_id: "sqlite-wakeup-upgrade-successor".into(),
            input: json!({ "generation": 1 }),
        },
    )
    .await;

    let activity_run = "sqlite-wakeup-upgrade-activity-retry";
    insert_raw_event(
        &executor,
        activity_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, activity_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        activity_run,
        3,
        FlowEvent::ActivityCreated {
            activity_id: "legacy-activity".into(),
            activity_name: "legacyActivity".into(),
            input: json!({}),
            retry: RetryPolicy::fixed(3, Duration::from_secs(1)),
            timeout_ms: None,
        },
    )
    .await;
    insert_raw_event(
        &executor,
        activity_run,
        4,
        FlowEvent::ActivityStarted {
            activity_id: "legacy-activity".into(),
            attempt: 1,
            attempt_id: "attempt-1".into(),
            idempotency_key: "idem-1".into(),
            fencing_token: "fence-1".into(),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        activity_run,
        5,
        FlowEvent::ActivityRetrying {
            activity_id: "legacy-activity".into(),
            attempt: 1,
            attempt_id: "attempt-1".into(),
            fencing_token: "fence-1".into(),
            error: "legacy activity retry".into(),
            retry_after: Some(timestamp("2026-08-07T01:00:05.123456789Z")),
        },
    )
    .await;

    let resumed_activity_run = "sqlite-wakeup-upgrade-resumed-activity";
    insert_raw_event(
        &executor,
        resumed_activity_run,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, resumed_activity_run, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        resumed_activity_run,
        3,
        FlowEvent::ActivityRetrying {
            activity_id: "already-resumed".into(),
            attempt: 1,
            attempt_id: "attempt-1".into(),
            fencing_token: "fence-1".into(),
            error: "already resumed".into(),
            retry_after: Some(timestamp("2026-08-07T01:00:06Z")),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        resumed_activity_run,
        4,
        FlowEvent::ActivityStarted {
            activity_id: "already-resumed".into(),
            attempt: 2,
            attempt_id: "attempt-2".into(),
            idempotency_key: "idem-2".into(),
            fencing_token: "fence-2".into(),
        },
    )
    .await;
    drop(executor);

    let store = SqliteEventStore::connect(format!("sqlite://{}", database_path.display()))
        .await
        .unwrap();
    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(rows.len(), 4);
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
    assert!(rows.iter().any(|row| {
        row.0 == activity_run
            && row.1 == 1
            && row.2 == "legacy-activity"
            && row.3 == "2026-08-07T01:00:05.123456789Z"
    }));
    assert!(!rows.iter().any(|row| row.0 == cancelled_retry_run));
    assert!(!rows.iter().any(|row| row.0 == resumed_activity_run));
    assert!(rows
        .iter()
        .any(|row| row.0 == cancelling_run && row.2 == "cleanup"));
    assert!(!rows.iter().any(|row| row.2 == "pre-cancellation"));
    assert!(!rows.iter().any(|row| row.2 == "already-completed"));
    assert!(!rows.iter().any(|row| row.0 == continued_run));
    assert!(store
        .list_active_hooks()
        .await
        .unwrap()
        .into_iter()
        .all(|active| active.run_id != continued_run));

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
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .any(|row| row.0 == cancelling_run && row.2 == "cleanup"));
    assert!(rows.iter().any(|row| row.2 == "legacy-activity"));

    insert_raw_event(
        store.executor(),
        cancelling_run,
        6,
        FlowEvent::RunCancelled {
            reason: Some("cleanup complete".into()),
        },
    )
    .await;
    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "legacy-activity");
    insert_raw_event(
        store.executor(),
        activity_run,
        6,
        FlowEvent::ActivityStarted {
            activity_id: "legacy-activity".into(),
            attempt: 2,
            attempt_id: "attempt-2".into(),
            idempotency_key: "idem-2".into(),
            fencing_token: "fence-2".into(),
        },
    )
    .await;
    assert!(scheduled_rows(store.executor()).await.is_empty());
}

#[tokio::test]
async fn sqlite_delayed_activity_retry_is_a_scheduled_wakeup() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-activity-retry-wakeup";
    create_run(&store, run_id).await;
    let retry_at = timestamp("2026-08-07T00:00:02.000000200Z");
    store
        .append(
            run_id,
            FlowEvent::ActivityCreated {
                activity_id: "flaky".into(),
                activity_name: "flakyActivity".into(),
                input: json!({}),
                retry: RetryPolicy::fixed(3, Duration::from_secs(1)),
                timeout_ms: None,
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ActivityStarted {
                activity_id: "flaky".into(),
                attempt: 1,
                attempt_id: "attempt-1".into(),
                idempotency_key: "idem-1".into(),
                fencing_token: "fence-1".into(),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ActivityRetrying {
                activity_id: "flaky".into(),
                attempt: 1,
                attempt_id: "attempt-1".into(),
                fencing_token: "fence-1".into(),
                error: "retry later".into(),
                retry_after: Some(retry_at),
            },
        )
        .await
        .unwrap();

    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, 1);
    assert_eq!(rows[0].2, "flaky");
    assert_eq!(rows[0].3, "2026-08-07T00:00:02.000000200Z");
    assert!(store
        .list_due_wakeups(timestamp("2026-08-07T00:00:02.000000199Z"))
        .await
        .unwrap()
        .is_empty());
    let due = store.list_due_wakeups(retry_at).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].kind, ScheduledWakeupKind::ActivityRetry);

    store
        .append(
            run_id,
            FlowEvent::ActivityStarted {
                activity_id: "flaky".into(),
                attempt: 2,
                attempt_id: "attempt-2".into(),
                idempotency_key: "idem-2".into(),
                fencing_token: "fence-2".into(),
            },
        )
        .await
        .unwrap();
    assert!(scheduled_rows(store.executor()).await.is_empty());
}

#[tokio::test]
async fn sqlite_step_and_activity_retries_with_same_id_both_remain_indexed() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-shared-retry-id";
    create_run(&store, run_id).await;
    let step_retry_at = timestamp("2026-08-07T00:00:02.000000100Z");
    let activity_retry_at = timestamp("2026-08-07T00:00:02.000000200Z");

    store
        .append(
            run_id,
            FlowEvent::StepCreated {
                step_id: "shared".into(),
                step_name: "sharedStep".into(),
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
                step_id: "shared".into(),
                attempt: 1,
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::StepRetrying {
                step_id: "shared".into(),
                attempt: 1,
                error: "step later".into(),
                retry_after: Some(step_retry_at),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ActivityCreated {
                activity_id: "shared".into(),
                activity_name: "sharedActivity".into(),
                input: json!({}),
                retry: RetryPolicy::fixed(3, Duration::from_secs(1)),
                timeout_ms: None,
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ActivityStarted {
                activity_id: "shared".into(),
                attempt: 1,
                attempt_id: "attempt-1".into(),
                idempotency_key: "idem-1".into(),
                fencing_token: "fence-1".into(),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::ActivityRetrying {
                activity_id: "shared".into(),
                attempt: 1,
                attempt_id: "attempt-1".into(),
                fencing_token: "fence-1".into(),
                error: "activity later".into(),
                retry_after: Some(activity_retry_at),
            },
        )
        .await
        .unwrap();

    let rows = scheduled_rows(store.executor()).await;
    assert_eq!(
        rows.len(),
        2,
        "step and activity retries must not share one wakeup primary key: {rows:?}"
    );
    assert!(rows.iter().any(|row| {
        row.1 == 2 && row.2 == "shared" && row.3 == "2026-08-07T00:00:02.000000100Z"
    }));
    assert!(rows.iter().any(|row| {
        row.1 == 1 && row.2 == "shared" && row.3 == "2026-08-07T00:00:02.000000200Z"
    }));
    let due = store.list_due_wakeups(activity_retry_at).await.unwrap();
    assert_eq!(due.len(), 2);
    assert!(due
        .iter()
        .any(|wakeup| wakeup.kind == ScheduledWakeupKind::Retry));
    assert!(due
        .iter()
        .any(|wakeup| wakeup.kind == ScheduledWakeupKind::ActivityRetry));
}
