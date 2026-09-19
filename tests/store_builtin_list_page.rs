use a3s_flow::{
    FlowEvent, FlowEventStore, InMemoryEventStore, LocalFileEventStore, WorkflowProgress,
    WorkflowSpec, MAX_FLOW_HISTORY_PAGE_SIZE,
};
use serde_json::json;

fn run_created() -> FlowEvent {
    FlowEvent::RunCreated {
        spec: WorkflowSpec::rust_embedded(
            "test.builtin-list-page",
            "1",
            "tests::store_builtin_list_page",
            "main",
        ),
        input: json!({}),
    }
}

async fn seed_padded_history(store: &dyn FlowEventStore, run_id: &str) {
    store
        .append_if_sequence(run_id, 0, run_created())
        .await
        .unwrap();
    store
        .append_if_sequence(run_id, 1, FlowEvent::RunStarted)
        .await
        .unwrap();
    let pads = MAX_FLOW_HISTORY_PAGE_SIZE - 1;
    for index in 0..pads {
        store
            .append_if_sequence(
                run_id,
                2 + index as u64,
                FlowEvent::RunProgressRecorded {
                    progress: WorkflowProgress::new(format!("pad-{index}"), index as u64),
                },
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn in_memory_list_page_returns_exclusive_cursor_window() {
    let store = InMemoryEventStore::new();
    let run_id = "memory-page";
    seed_padded_history(&store, run_id).await;

    let page = store
        .list_page(run_id, MAX_FLOW_HISTORY_PAGE_SIZE as u64, 2)
        .await
        .expect("in-memory list_page");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].sequence, (MAX_FLOW_HISTORY_PAGE_SIZE + 1) as u64);
}

#[tokio::test]
async fn local_file_list_page_returns_exclusive_cursor_window() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = LocalFileEventStore::new(directory.path());
    let run_id = "local-page";
    seed_padded_history(&store, run_id).await;

    let page = store
        .list_page(run_id, MAX_FLOW_HISTORY_PAGE_SIZE as u64, 2)
        .await
        .expect("local-file list_page");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].sequence, (MAX_FLOW_HISTORY_PAGE_SIZE + 1) as u64);
}

#[tokio::test]
async fn local_file_latest_event_returns_the_tip() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = LocalFileEventStore::new(directory.path());
    let run_id = "local-tip";
    seed_padded_history(&store, run_id).await;

    let tip = store
        .latest_event(run_id)
        .await
        .expect("latest")
        .expect("tip");
    assert_eq!(tip.0, (MAX_FLOW_HISTORY_PAGE_SIZE + 1) as u64);
}
