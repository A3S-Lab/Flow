use std::sync::Arc;

use a3s_flow::{
    FlowError, FlowEvent, FlowEventStore, InMemoryEventStore, SelectArm, SelectMode, WorkflowSpec,
};
use chrono::{TimeZone, Utc};
use serde_json::json;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.select-validation",
        "1",
        "tests::select_validation",
        "main",
    )
}

async fn create_started(store: &InMemoryEventStore, run_id: &str) {
    store
        .append(
            run_id,
            FlowEvent::RunCreated {
                spec: spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
}

fn open_timer_select() -> FlowEvent {
    FlowEvent::SelectCreated {
        select_id: "race".into(),
        arms: vec![
            SelectArm::timer("fast", Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap()),
            SelectArm::timer("slow", Utc.with_ymd_and_hms(2030, 1, 2, 0, 0, 0).unwrap()),
        ],
        mode: SelectMode::Race,
    }
}

#[tokio::test]
async fn projection_rejects_graceful_terminals_with_an_open_select() {
    let store = Arc::new(InMemoryEventStore::new());
    create_started(&store, "open-select-complete").await;
    store
        .append("open-select-complete", open_timer_select())
        .await
        .unwrap();
    let completed = store
        .append(
            "open-select-complete",
            FlowEvent::RunCompleted { output: json!({}) },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        completed,
        FlowError::InvalidTransition(message)
            if message.contains("select race") && message.contains("open")
    ));

    create_started(&store, "open-select-can").await;
    store
        .append("open-select-can", open_timer_select())
        .await
        .unwrap();
    let continued = store
        .append(
            "open-select-can",
            FlowEvent::RunContinuedAsNew {
                successor_run_id: "open-select-successor".into(),
                input: json!({}),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        continued,
        FlowError::InvalidTransition(message)
            if message.contains("select race") && message.contains("open")
    ));
}
