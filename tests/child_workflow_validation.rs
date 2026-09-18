use std::sync::Arc;

use a3s_flow::{
    ChildWorkflowCancellationPolicy, ChildWorkflowCommand, FlowEngine, FlowError, FlowEvent,
    FlowEventStore, FlowRuntime, InMemoryEventStore, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use serde_json::{json, Value};

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "child-validation",
        "1",
        "tests::child_workflow_validation",
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

fn request(child_id: &str, child_run_id: &str, input: Value) -> FlowEvent {
    FlowEvent::ChildWorkflowRequested {
        child_id: child_id.into(),
        child_run_id: child_run_id.into(),
        spec: spec(),
        input,
        cancellation_policy: ChildWorkflowCancellationPolicy::RequestCancellation,
    }
}

#[tokio::test]
async fn projection_rejects_parent_completion_with_a_blocking_child() {
    let store = Arc::new(InMemoryEventStore::new());
    create_started(&store, "blocking-parent").await;
    store
        .append(
            "blocking-parent",
            request("child", "blocking-child", json!({})),
        )
        .await
        .unwrap();
    let error = store
        .append(
            "blocking-parent",
            FlowEvent::RunCompleted { output: json!({}) },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidTransition(message)
            if message.contains("cannot terminate while child workflow child is open")
    ));
}

#[tokio::test]
async fn projection_rejects_unknown_duplicate_and_intermediate_child_resolutions() {
    let store = Arc::new(InMemoryEventStore::new());
    create_started(&store, "unknown-resolution").await;
    let error = store
        .append(
            "unknown-resolution",
            FlowEvent::ChildWorkflowResolved {
                child_id: "missing".into(),
                outcome: WorkflowTerminalOutcome::Completed { output: json!({}) },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidTransition(message) if message.contains("unknown child missing")
    ));

    create_started(&store, "continued-resolution").await;
    store
        .append(
            "continued-resolution",
            request("child", "continued-child", json!({})),
        )
        .await
        .unwrap();
    let error = store
        .append(
            "continued-resolution",
            FlowEvent::ChildWorkflowResolved {
                child_id: "child".into(),
                outcome: WorkflowTerminalOutcome::ContinuedAsNew {
                    successor_run_id: "continued-leaf".into(),
                },
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidTransition(message)
            if message.contains("cannot resolve to a continuation segment")
    ));
}

struct DriftedReplayRuntime;

#[async_trait]
impl FlowRuntime for DriftedReplayRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation
            .context()
            .start_child_workflow("child", spec(), json!({ "value": 99 })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("validation tests do not execute steps")
    }
}

#[tokio::test]
async fn resolved_child_command_drift_is_reported_as_non_determinism() {
    let store = Arc::new(InMemoryEventStore::new());
    create_started(&store, "drift-parent").await;
    store
        .append(
            "drift-parent",
            request("child", "drift-child", json!({ "value": 1 })),
        )
        .await
        .unwrap();
    store
        .append(
            "drift-parent",
            FlowEvent::ChildWorkflowResolved {
                child_id: "child".into(),
                outcome: WorkflowTerminalOutcome::Completed { output: json!({}) },
            },
        )
        .await
        .unwrap();

    let engine = FlowEngine::new(store, Arc::new(DriftedReplayRuntime));
    assert!(matches!(
        engine.drive("drift-parent").await,
        Err(FlowError::NonDeterministic { reason, .. })
            if reason.contains("child workflow child differs")
    ));
}

#[tokio::test]
async fn projection_rejects_graceful_terminals_with_an_open_child_workflow_map() {
    let store = Arc::new(InMemoryEventStore::new());
    create_started(&store, "open-map-complete").await;
    store
        .append(
            "open-map-complete",
            FlowEvent::ChildWorkflowMapOpened {
                map_id: "batch".into(),
                children: vec![ChildWorkflowCommand::new(
                    "item-0000",
                    spec(),
                    json!({ "ordinal": 0 }),
                )],
                concurrency: 1,
            },
        )
        .await
        .unwrap();
    let completed = store
        .append(
            "open-map-complete",
            FlowEvent::RunCompleted { output: json!({}) },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        completed,
        FlowError::InvalidTransition(message)
            if message.contains("child workflow map batch") && message.contains("open")
    ));

    create_started(&store, "open-map-can").await;
    store
        .append(
            "open-map-can",
            FlowEvent::ChildWorkflowMapOpened {
                map_id: "batch".into(),
                children: vec![ChildWorkflowCommand::new(
                    "item-0000",
                    spec(),
                    json!({ "ordinal": 0 }),
                )],
                concurrency: 1,
            },
        )
        .await
        .unwrap();
    let continued = store
        .append(
            "open-map-can",
            FlowEvent::RunContinuedAsNew {
                successor_run_id: "open-map-successor".into(),
                input: json!({}),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        continued,
        FlowError::InvalidTransition(message)
            if message.contains("child workflow map batch") && message.contains("open")
    ));
}

#[tokio::test]
async fn projection_allows_completion_after_child_workflow_map_completes() {
    let store = Arc::new(InMemoryEventStore::new());
    create_started(&store, "closed-map-complete").await;
    let plan = ChildWorkflowCommand::new("item-0000", spec(), json!({ "ordinal": 0 }));
    store
        .append(
            "closed-map-complete",
            FlowEvent::ChildWorkflowMapOpened {
                map_id: "batch".into(),
                children: vec![plan.clone()],
                concurrency: 1,
            },
        )
        .await
        .unwrap();
    store
        .append(
            "closed-map-complete",
            request("item-0000", "closed-map-child", plan.input.clone()),
        )
        .await
        .unwrap();
    store
        .append(
            "closed-map-complete",
            FlowEvent::ChildWorkflowResolved {
                child_id: "item-0000".into(),
                outcome: WorkflowTerminalOutcome::Completed {
                    output: json!({ "ordinal": 0 }),
                },
            },
        )
        .await
        .unwrap();
    store
        .append(
            "closed-map-complete",
            FlowEvent::ChildWorkflowMapCompleted {
                map_id: "batch".into(),
            },
        )
        .await
        .unwrap();
    store
        .append(
            "closed-map-complete",
            FlowEvent::RunCompleted {
                output: json!({ "joined": true }),
            },
        )
        .await
        .unwrap();
}
