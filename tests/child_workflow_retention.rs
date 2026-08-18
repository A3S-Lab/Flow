use a3s_flow::{
    ChildWorkflowCancellationPolicy, FlowError, FlowEvent, FlowEventStore, LocalFileEventStore,
    WorkflowSpec, WorkflowTerminalOutcome,
};
use chrono::{Duration, Utc};
use serde_json::json;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-retention",
        "1",
        "tests::child_workflow_retention",
        "main",
    )
}

async fn create_started(store: &dyn FlowEventStore, run_id: &str, input: serde_json::Value) {
    store
        .append(
            run_id,
            FlowEvent::RunCreated {
                spec: spec(),
                input,
            },
        )
        .await
        .unwrap();
    store.append(run_id, FlowEvent::RunStarted).await.unwrap();
}

#[tokio::test]
async fn dangling_abandoned_child_link_protects_a_terminal_parent() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    create_started(&store, "dangling-parent", json!({})).await;
    store
        .append(
            "dangling-parent",
            FlowEvent::ChildWorkflowRequested {
                child_id: "child".into(),
                child_run_id: "missing-child".into(),
                spec: spec(),
                input: json!({}),
                cancellation_policy: ChildWorkflowCancellationPolicy::Abandon,
            },
        )
        .await
        .unwrap();
    store
        .append(
            "dangling-parent",
            FlowEvent::RunCancelled {
                reason: Some("parent stopped".into()),
            },
        )
        .await
        .unwrap();

    assert!(store
        .prune_terminal_runs_older_than(Utc::now() + Duration::minutes(1))
        .await
        .unwrap()
        .is_empty());
    assert!(store.list("dangling-parent").await.is_ok());
}

#[tokio::test]
async fn retention_deletes_only_a_fully_resolved_child_component() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    create_started(&store, "resolved-parent", json!({})).await;
    store
        .append(
            "resolved-parent",
            FlowEvent::ChildWorkflowRequested {
                child_id: "child".into(),
                child_run_id: "resolved-child".into(),
                spec: spec(),
                input: json!({ "value": 1 }),
                cancellation_policy: ChildWorkflowCancellationPolicy::RequestCancellation,
            },
        )
        .await
        .unwrap();
    create_started(&store, "resolved-child", json!({ "value": 1 })).await;
    let outcome = WorkflowTerminalOutcome::Completed {
        output: json!({ "value": 2 }),
    };
    store
        .append(
            "resolved-child",
            FlowEvent::RunCompleted {
                output: json!({ "value": 2 }),
            },
        )
        .await
        .unwrap();
    store
        .append(
            "resolved-parent",
            FlowEvent::ChildWorkflowResolved {
                child_id: "child".into(),
                outcome,
            },
        )
        .await
        .unwrap();
    store
        .append(
            "resolved-parent",
            FlowEvent::RunCompleted { output: json!({}) },
        )
        .await
        .unwrap();

    assert_eq!(
        store
            .prune_terminal_runs_older_than(Utc::now() + Duration::minutes(1))
            .await
            .unwrap(),
        vec!["resolved-child".to_string(), "resolved-parent".to_string()]
    );
}

#[tokio::test]
async fn retention_rejects_child_start_drift_from_the_parent_request() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    create_started(&store, "drift-parent", json!({})).await;
    store
        .append(
            "drift-parent",
            FlowEvent::ChildWorkflowRequested {
                child_id: "child".into(),
                child_run_id: "drift-child".into(),
                spec: spec(),
                input: json!({ "value": 1 }),
                cancellation_policy: ChildWorkflowCancellationPolicy::RequestCancellation,
            },
        )
        .await
        .unwrap();
    create_started(&store, "drift-child", json!({ "value": 99 })).await;
    store
        .append("drift-child", FlowEvent::RunCompleted { output: json!({}) })
        .await
        .unwrap();

    assert!(matches!(
        store
            .prune_terminal_runs_older_than(Utc::now() + Duration::minutes(1))
            .await,
        Err(FlowError::RunConflict { run_id, reason })
            if run_id == "drift-child"
                && reason == "child workflow input differs from parent request"
    ));
}

#[tokio::test]
async fn retention_rejects_a_first_class_child_ownership_cycle() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalFileEventStore::new(directory.path());
    create_started(&store, "cycle-parent", json!({})).await;
    create_started(&store, "cycle-child", json!({})).await;

    for (run_id, child_id, child_run_id) in [
        ("cycle-parent", "child", "cycle-child"),
        ("cycle-child", "parent", "cycle-parent"),
    ] {
        store
            .append(
                run_id,
                FlowEvent::ChildWorkflowRequested {
                    child_id: child_id.into(),
                    child_run_id: child_run_id.into(),
                    spec: spec(),
                    input: json!({}),
                    cancellation_policy: ChildWorkflowCancellationPolicy::Abandon,
                },
            )
            .await
            .unwrap();
        store
            .append(
                run_id,
                FlowEvent::RunCancelled {
                    reason: Some("cycle fixture".into()),
                },
            )
            .await
            .unwrap();
    }

    assert!(matches!(
        store
            .prune_terminal_runs_older_than(Utc::now() + Duration::minutes(1))
            .await,
        Err(FlowError::ChildWorkflowCycle(run_id))
            if run_id == "cycle-child" || run_id == "cycle-parent"
    ));
}
