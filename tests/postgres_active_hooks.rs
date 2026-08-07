#![cfg(feature = "postgres")]

use a3s_flow::{FlowError, FlowEvent, FlowEventStore, PostgresEventStore, WorkflowSpec};
use a3s_orm::{
    sql_query, Database, DatabaseError, PostgresDialect, PostgresError, PostgresExecutor,
};
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.postgres-active-hooks",
        "1",
        "tests::postgres_active_hooks",
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

async fn insert_raw_event(
    executor: &PostgresExecutor,
    run_id: &str,
    sequence: i64,
    event: FlowEvent,
) {
    try_insert_raw_event(executor, run_id, sequence, event)
        .await
        .unwrap();
}

async fn try_insert_raw_event(
    executor: &PostgresExecutor,
    run_id: &str,
    sequence: i64,
    event: FlowEvent,
) -> std::result::Result<(), DatabaseError<PostgresError>> {
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
        .map(|_| ())
}

async fn active_hook_rows(
    store: &PostgresEventStore,
    token: &str,
) -> Vec<(String, String, String, String, i64)> {
    Database::new(PostgresDialect, store.executor().clone())
        .fetch_all_as(
            sql_query::<(String, String, String, String, i64)>(
                "SELECT run_id, hook_id, token, metadata_json, created_sequence \
                 FROM flow_active_hooks WHERE token = ",
            )
            .bind(token)
            .append(" ORDER BY run_id, hook_id"),
        )
        .await
        .unwrap()
        .rows
}

#[tokio::test]
async fn postgres_active_hook_index_rejects_concurrent_duplicate_tokens() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let scope = Uuid::new_v4();
    let first_run = format!("postgres-hook-{scope}-a");
    let second_run = format!("postgres-hook-{scope}-b");
    let token = format!("postgres-hook-{scope}-token");
    let first = PostgresEventStore::connect(&postgres_url).await.unwrap();
    let second = PostgresEventStore::connect(&postgres_url).await.unwrap();
    create_run(&first, &first_run).await;
    create_run(&second, &second_run).await;

    let (first_result, second_result) = tokio::join!(
        first.append(
            &first_run,
            FlowEvent::HookCreated {
                hook_id: "approval".into(),
                token: token.clone(),
                metadata: json!({ "owner": "a" }),
            },
        ),
        second.append(
            &second_run,
            FlowEvent::HookCreated {
                hook_id: "approval".into(),
                token: token.clone(),
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
    let rows = active_hook_rows(&first, &token).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, "approval");
    assert_eq!(rows[0].2, token);
    assert!(rows[0].0 == first_run || rows[0].0 == second_run);

    first
        .append(
            &first_run,
            FlowEvent::RunCancelled {
                reason: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
    second
        .append(
            &second_run,
            FlowEvent::RunCancelled {
                reason: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
    assert!(active_hook_rows(&first, &token).await.is_empty());
}

#[tokio::test]
async fn postgres_active_hook_trigger_tracks_legacy_writer_events() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let scope = Uuid::new_v4();
    let run_id = format!("postgres-hook-trigger-{scope}");
    let token = format!("postgres-hook-trigger-{scope}-token");
    let store = PostgresEventStore::connect(&postgres_url).await.unwrap();
    create_run(&store, &run_id).await;

    let metadata = json!(["legacy", "postgres"]);
    insert_raw_event(
        store.executor(),
        &run_id,
        3,
        FlowEvent::HookCreated {
            hook_id: "legacy".into(),
            token: token.clone(),
            metadata: metadata.clone(),
        },
    )
    .await;
    let rows = active_hook_rows(&store, &token).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, run_id);
    assert_eq!(rows[0].1, "legacy");
    assert_eq!(rows[0].2, token);
    assert_eq!(serde_json::from_str::<Value>(&rows[0].3).unwrap(), metadata);
    assert_eq!(rows[0].4, 3);

    insert_raw_event(
        store.executor(),
        &run_id,
        4,
        FlowEvent::HookDisposed {
            hook_id: "legacy".into(),
        },
    )
    .await;
    assert!(active_hook_rows(&store, &token).await.is_empty());
    store
        .append(
            &run_id,
            FlowEvent::RunCancelled {
                reason: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn postgres_active_hook_trigger_rejects_concurrent_legacy_writers() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let scope = Uuid::new_v4();
    let first_run = format!("postgres-legacy-hook-{scope}-a");
    let second_run = format!("postgres-legacy-hook-{scope}-b");
    let token = format!("postgres-legacy-hook-{scope}-token");
    let first = PostgresEventStore::connect(&postgres_url).await.unwrap();
    let second = PostgresEventStore::connect(&postgres_url).await.unwrap();
    create_run(&first, &first_run).await;
    create_run(&second, &second_run).await;

    let (first_result, second_result) = tokio::join!(
        try_insert_raw_event(
            first.executor(),
            &first_run,
            3,
            FlowEvent::HookCreated {
                hook_id: "legacy".into(),
                token: token.clone(),
                metadata: json!({ "owner": "a" }),
            },
        ),
        try_insert_raw_event(
            second.executor(),
            &second_run,
            3,
            FlowEvent::HookCreated {
                hook_id: "legacy".into(),
                token: token.clone(),
                metadata: json!({ "owner": "b" }),
            },
        ),
    );

    let results = [first_result, second_result];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let error = results
        .iter()
        .find_map(|result| result.as_ref().err())
        .unwrap();
    let diagnostic = format!("{error:?}");
    assert!(!diagnostic.contains(&token));
    match error {
        DatabaseError::Execute(PostgresError::Database(source)) => {
            let database_error = source
                .as_db_error()
                .expect("trigger rejection must carry a PostgreSQL database error");
            assert_eq!(database_error.code().code(), "23505");
            assert_eq!(database_error.message(), "flow active hook token conflict");
        }
        other => panic!("unexpected legacy-writer error: {other:?}"),
    }
    assert_eq!(active_hook_rows(&first, &token).await.len(), 1);

    first
        .append(
            &first_run,
            FlowEvent::RunCancelled {
                reason: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
    second
        .append(
            &second_run,
            FlowEvent::RunCancelled {
                reason: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
    assert!(active_hook_rows(&first, &token).await.is_empty());
}

#[tokio::test]
async fn postgres_active_hook_hash_index_accepts_large_bearer_tokens() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let scope = Uuid::new_v4();
    let run_id = format!("postgres-large-hook-{scope}");
    let token = format!("{scope}-{}", "x".repeat(8 * 1024));
    let store = PostgresEventStore::connect(&postgres_url).await.unwrap();
    create_run(&store, &run_id).await;

    store
        .append(
            &run_id,
            FlowEvent::HookCreated {
                hook_id: "large-token".into(),
                token: token.clone(),
                metadata: json!({}),
            },
        )
        .await
        .unwrap();
    let rows = active_hook_rows(&store, &token).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, run_id);
    assert_eq!(rows[0].1, "large-token");
    assert_eq!(rows[0].2, token);

    store
        .append(
            &run_id,
            FlowEvent::RunCancelled {
                reason: Some("test cleanup".into()),
            },
        )
        .await
        .unwrap();
    assert!(active_hook_rows(&store, &token).await.is_empty());
}
