use a3s_flow::{
    ChildOperationReference, FlowError, FlowEvent, FlowEventStore, InMemoryEventStore,
    LocalFileEventStore, WorkflowSpec,
};
use chrono::{Duration, Utc};
use serde_json::json;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.store-reference-integrity",
        "1",
        "tests::store_reference_integrity",
        "main",
    )
}

fn linked_child(child_run_id: &str) -> FlowEvent {
    FlowEvent::ChildOperationLinked {
        child: ChildOperationReference::new("child", "flow.run", child_run_id)
            .with_flow_run_id(child_run_id),
    }
}

async fn create_run(store: &dyn FlowEventStore, run_id: &str) {
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
}

async fn assert_missing_child_is_rejected(store: &dyn FlowEventStore) {
    let append_parent = "reference-append-parent";
    let conditional_parent = "reference-conditional-parent";
    let child_run_id = "reference-child";
    create_run(store, append_parent).await;
    create_run(store, conditional_parent).await;

    let append_error = store
        .append(append_parent, linked_child(child_run_id))
        .await
        .unwrap_err();
    assert!(matches!(
        append_error,
        FlowError::RunNotFound(ref run_id) if run_id == child_run_id
    ));

    let conditional_error = store
        .append_if_sequence(conditional_parent, 1, linked_child(child_run_id))
        .await
        .unwrap_err();
    assert!(matches!(
        conditional_error,
        FlowError::RunNotFound(ref run_id) if run_id == child_run_id
    ));

    assert_eq!(store.list(append_parent).await.unwrap().len(), 1);
    assert_eq!(store.list(conditional_parent).await.unwrap().len(), 1);

    create_run(store, child_run_id).await;
    assert_eq!(
        store
            .append(append_parent, linked_child(child_run_id))
            .await
            .unwrap()
            .sequence,
        2
    );
    assert_eq!(
        store
            .append_if_sequence(conditional_parent, 1, linked_child(child_run_id))
            .await
            .unwrap()
            .sequence,
        2
    );
}

#[tokio::test]
async fn in_memory_store_requires_linked_flow_runs_to_exist() {
    let store = InMemoryEventStore::new();
    assert_missing_child_is_rejected(&store).await;
}

#[tokio::test]
async fn local_file_store_requires_linked_flow_runs_to_exist_after_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    assert_missing_child_is_rejected(&store).await;
    drop(store);

    let reopened = LocalFileEventStore::new(directory.path());
    create_run(&reopened, "reference-reopen-parent").await;
    reopened
        .append("reference-reopen-parent", linked_child("reference-child"))
        .await
        .unwrap();
}

#[tokio::test]
async fn local_file_store_rejects_an_empty_linked_flow_history() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    create_run(&store, "reference-empty-parent").await;
    tokio::fs::write(directory.path().join("reference-empty-child.jsonl"), [])
        .await
        .unwrap();

    let error = store
        .append(
            "reference-empty-parent",
            linked_child("reference-empty-child"),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::RunNotFound(ref run_id) if run_id == "reference-empty-child"
    ));
    assert_eq!(store.list("reference-empty-parent").await.unwrap().len(), 1);
}

#[tokio::test]
async fn local_retention_deletes_only_complete_linked_components() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    let parent_run_id = "retention-linked-parent";
    let child_run_id = "retention-linked-child";
    create_run(&store, parent_run_id).await;
    create_run(&store, child_run_id).await;
    store
        .append(
            child_run_id,
            FlowEvent::RunCancelled {
                reason: Some("child finished".into()),
            },
        )
        .await
        .unwrap();
    store
        .append(parent_run_id, linked_child(child_run_id))
        .await
        .unwrap();

    let terminal_before = Utc::now() + Duration::minutes(1);
    assert!(store
        .prune_terminal_runs_older_than(terminal_before)
        .await
        .unwrap()
        .is_empty());
    assert!(store.list(parent_run_id).await.is_ok());
    assert!(store.list(child_run_id).await.is_ok());

    store
        .append(
            parent_run_id,
            FlowEvent::RunCancelled {
                reason: Some("parent finished".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .prune_terminal_runs_older_than(terminal_before)
            .await
            .unwrap(),
        vec![child_run_id.to_string(), parent_run_id.to_string()]
    );
    assert!(matches!(
        store.list(parent_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));
    assert!(matches!(
        store.list(child_run_id).await,
        Err(FlowError::RunNotFound(_))
    ));
}
