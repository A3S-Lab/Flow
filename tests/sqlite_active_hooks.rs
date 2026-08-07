#![cfg(feature = "sqlite")]

use a3s_flow::{
    CancellationRequest, FlowError, FlowEvent, FlowEventStore, SqliteEventStore, WorkflowSpec,
};
use a3s_orm::{sql_query, Database, Migration, Migrator, SqliteDialect, SqliteExecutor};
use chrono::Utc;
use serde_json::{json, Value};
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

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.sqlite-active-hooks",
        "1",
        "tests::sqlite_active_hooks",
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

async fn active_hook_rows(executor: &SqliteExecutor) -> Vec<(String, String, String, String, i64)> {
    Database::new(SqliteDialect, executor.clone())
        .fetch_all_as(sql_query::<(String, String, String, String, i64)>(
            "SELECT run_id, hook_id, token, metadata_json, created_sequence \
             FROM flow_active_hooks ORDER BY run_id, hook_id",
        ))
        .await
        .unwrap()
        .rows
}

fn assert_projected_hook(
    row: &(String, String, String, String, i64),
    run_id: &str,
    hook_id: &str,
    token: &str,
    metadata: &Value,
    created_sequence: i64,
) {
    assert_eq!(row.0, run_id);
    assert_eq!(row.1, hook_id);
    assert_eq!(row.2, token);
    assert_eq!(serde_json::from_str::<Value>(&row.3).unwrap(), *metadata);
    assert_eq!(row.4, created_sequence);
}

#[tokio::test]
async fn sqlite_active_hook_projection_tracks_event_lifecycle() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "sqlite-active-hook-lifecycle";
    create_run(&store, run_id).await;

    let metadata = json!("scalar metadata must remain valid JSON");
    store
        .append(
            run_id,
            FlowEvent::HookCreated {
                hook_id: "approval".into(),
                token: "sqlite-lifecycle-token".into(),
                metadata: metadata.clone(),
            },
        )
        .await
        .unwrap();

    let rows = active_hook_rows(store.executor()).await;
    assert_eq!(rows.len(), 1);
    assert_projected_hook(
        &rows[0],
        run_id,
        "approval",
        "sqlite-lifecycle-token",
        &metadata,
        3,
    );

    store
        .append(
            run_id,
            FlowEvent::HookReceived {
                hook_id: "approval".into(),
                payload: json!({ "approved": true }),
            },
        )
        .await
        .unwrap();
    assert!(active_hook_rows(store.executor()).await.is_empty());

    store
        .append(
            run_id,
            FlowEvent::HookCreated {
                hook_id: "secondary".into(),
                token: "sqlite-terminal-token".into(),
                metadata: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append(
            run_id,
            FlowEvent::RunCancelled {
                reason: Some("test complete".into()),
            },
        )
        .await
        .unwrap();
    assert!(active_hook_rows(store.executor()).await.is_empty());
}

#[tokio::test]
async fn sqlite_active_hook_migration_backfills_and_triggers_legacy_writes() {
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

    let run_id = "sqlite-active-hook-upgrade";
    insert_raw_event(
        &executor,
        run_id,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, run_id, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        run_id,
        3,
        FlowEvent::HookCreated {
            hook_id: "legacy".into(),
            token: "sqlite-upgrade-token".into(),
            metadata: json!(["legacy", "metadata"]),
        },
    )
    .await;

    let cancelling_run_id = "sqlite-cancelling-hook-upgrade";
    insert_raw_event(
        &executor,
        cancelling_run_id,
        1,
        FlowEvent::RunCreated {
            spec: spec(),
            input: json!({}),
        },
    )
    .await;
    insert_raw_event(&executor, cancelling_run_id, 2, FlowEvent::RunStarted).await;
    insert_raw_event(
        &executor,
        cancelling_run_id,
        3,
        FlowEvent::HookCreated {
            hook_id: "pre-cancellation".into(),
            token: "sqlite-pre-cancellation-token".into(),
            metadata: json!({}),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        cancelling_run_id,
        4,
        FlowEvent::RunCancellationRequested {
            request: CancellationRequest::new(Some("cleanup required".into())),
        },
    )
    .await;
    insert_raw_event(
        &executor,
        cancelling_run_id,
        5,
        FlowEvent::HookCreated {
            hook_id: "cleanup".into(),
            token: "sqlite-cleanup-token".into(),
            metadata: json!({ "phase": "cleanup" }),
        },
    )
    .await;
    drop(executor);

    let store = SqliteEventStore::connect(format!("sqlite://{}", database_path.display()))
        .await
        .unwrap();
    let rows = active_hook_rows(store.executor()).await;
    assert_eq!(rows.len(), 2);
    assert_projected_hook(
        &rows[0],
        run_id,
        "legacy",
        "sqlite-upgrade-token",
        &json!(["legacy", "metadata"]),
        3,
    );
    assert_projected_hook(
        &rows[1],
        cancelling_run_id,
        "cleanup",
        "sqlite-cleanup-token",
        &json!({ "phase": "cleanup" }),
        5,
    );

    insert_raw_event(
        store.executor(),
        run_id,
        4,
        FlowEvent::HookReceived {
            hook_id: "legacy".into(),
            payload: json!({}),
        },
    )
    .await;
    let rows = active_hook_rows(store.executor()).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "sqlite-cleanup-token");

    insert_raw_event(
        store.executor(),
        run_id,
        5,
        FlowEvent::HookCreated {
            hook_id: "legacy-second".into(),
            token: "sqlite-trigger-token".into(),
            metadata: json!(null),
        },
    )
    .await;
    let rows = active_hook_rows(store.executor()).await;
    assert_eq!(rows.len(), 2);
    assert_projected_hook(
        rows.iter()
            .find(|row| row.2 == "sqlite-trigger-token")
            .unwrap(),
        run_id,
        "legacy-second",
        "sqlite-trigger-token",
        &Value::Null,
        5,
    );

    insert_raw_event(
        store.executor(),
        run_id,
        6,
        FlowEvent::RunCancelled {
            reason: Some("legacy writer stopped".into()),
        },
    )
    .await;
    let rows = active_hook_rows(store.executor()).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "sqlite-cleanup-token");

    insert_raw_event(
        store.executor(),
        cancelling_run_id,
        6,
        FlowEvent::RunCancelled {
            reason: Some("cleanup complete".into()),
        },
    )
    .await;
    assert!(active_hook_rows(store.executor()).await.is_empty());
}

#[tokio::test]
async fn sqlite_active_hook_index_rejects_concurrent_duplicate_tokens() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = format!("sqlite://{}", directory.path().join("flow.db").display());
    let first = SqliteEventStore::connect(&database_url).await.unwrap();
    let second = SqliteEventStore::connect(&database_url).await.unwrap();
    create_run(&first, "sqlite-concurrent-hook-a").await;
    create_run(&second, "sqlite-concurrent-hook-b").await;

    let (first_result, second_result) = tokio::join!(
        first.append(
            "sqlite-concurrent-hook-a",
            FlowEvent::HookCreated {
                hook_id: "approval".into(),
                token: "sqlite-concurrent-token".into(),
                metadata: json!({ "owner": "a" }),
            },
        ),
        second.append(
            "sqlite-concurrent-hook-b",
            FlowEvent::HookCreated {
                hook_id: "approval".into(),
                token: "sqlite-concurrent-token".into(),
                metadata: json!({ "owner": "b" }),
            },
        ),
    );

    let results = [first_result, second_result];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(FlowError::HookTokenConflict { .. })))
            .count(),
        1
    );
    assert_eq!(active_hook_rows(first.executor()).await.len(), 1);

    first
        .append(
            "sqlite-concurrent-hook-a",
            FlowEvent::RunCancelled {
                reason: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
    second
        .append(
            "sqlite-concurrent-hook-b",
            FlowEvent::RunCancelled {
                reason: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
    assert!(active_hook_rows(first.executor()).await.is_empty());
}
