#![cfg(feature = "sqlite")]

use a3s_flow::{
    FlowEvent, FlowEventStore, SqliteEventStore, WorkflowProgress, WorkflowSpec,
    MAX_FLOW_HISTORY_PAGE_SIZE,
};
use a3s_orm::{sql_query, Database, SqliteDialect};
use serde_json::json;

fn run_created() -> FlowEvent {
    FlowEvent::RunCreated {
        spec: WorkflowSpec::rust_embedded(
            "test.sqlite-cold-append",
            "1",
            "tests::sqlite_cold_append",
            "main",
        ),
        input: json!({}),
    }
}

#[tokio::test]
async fn sqlite_cold_append_after_checkpoint_loss_rebuilds_from_pages() {
    let store = SqliteEventStore::connect("sqlite::memory:").await.unwrap();
    let run_id = "cold-append-pages";
    store.append(run_id, run_created()).await.unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
    let pads = MAX_FLOW_HISTORY_PAGE_SIZE - 1;
    for index in 0..pads {
        store
            .append(
                run_id,
                FlowEvent::RunProgressRecorded {
                    progress: WorkflowProgress::new(format!("pad-{index}"), index as u64),
                },
            )
            .await
            .unwrap();
    }
    let before = store.latest_event(run_id).await.unwrap().unwrap();
    assert_eq!(before.0, (MAX_FLOW_HISTORY_PAGE_SIZE + 1) as u64);
    assert!(store.load_checkpoint(run_id).await.unwrap().is_some());

    Database::new(SqliteDialect, store.executor().clone())
        .execute(sql_query::<()>("DELETE FROM flow_projection_checkpoints"))
        .await
        .unwrap();
    assert!(store.load_checkpoint(run_id).await.unwrap().is_none());

    let appended = store
        .append(
            run_id,
            FlowEvent::RunProgressRecorded {
                progress: WorkflowProgress::new("after-cold", 9_999),
            },
        )
        .await
        .expect("cold append must rebuild from bounded history pages");
    assert_eq!(appended.sequence, (MAX_FLOW_HISTORY_PAGE_SIZE + 2) as u64);

    let checkpoint = store
        .load_checkpoint(run_id)
        .await
        .unwrap()
        .expect("append must refresh the tip checkpoint");
    assert_eq!(checkpoint.last_sequence, appended.sequence);
    assert_eq!(checkpoint.last_event_id, appended.event_id);
}
