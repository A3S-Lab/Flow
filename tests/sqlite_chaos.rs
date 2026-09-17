//! FLOW-R3/R6 SQLite chaos gates for production-capable local SQL stores.
//!
//! Mirrors the PostgreSQL concurrent-writer invariant without requiring a
//! remote database: tempfile SQLite stays offline-safe in default CI.

#![cfg(feature = "sqlite")]

use a3s_flow::{FlowError, FlowEvent, FlowEventStore, SqliteEventStore, WorkflowSpec};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Barrier;
use uuid::Uuid;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("cert.sqlite.chaos", "1", "tests::sqlite_chaos", "main")
}

#[tokio::test]
async fn sqlite_chaos_concurrent_writers_have_one_winner() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("flow-chaos.db").display()
    );
    let store = Arc::new(SqliteEventStore::connect(&database_url).await.unwrap());
    let run_id = format!("sqlite-chaos-writers-{}", Uuid::new_v4());

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
}
