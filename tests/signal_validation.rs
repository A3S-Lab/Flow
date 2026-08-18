use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventStore, FlowRuntime, InMemoryEventStore,
    RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowSignal, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;

const RUN_ID: &str = "signal-validation-history";
const APPROVAL_SIGNAL: &str = "order.approved";
const RELEASE_SIGNAL: &str = "order.released";

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.signal-validation",
        "1",
        "tests::signal_validation",
        "main",
    )
    .with_signal(APPROVAL_SIGNAL)
    .with_signal(RELEASE_SIGNAL)
}

struct SnapshotOnlyRuntime;

#[async_trait]
impl FlowRuntime for SnapshotOnlyRuntime {
    async fn run_workflow(
        &self,
        _invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Err(FlowError::Runtime(
            "signal projection validation must not invoke workflow code".to_string(),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(
            "signal projection validation must not invoke steps".to_string(),
        ))
    }
}

async fn snapshot_error(events: Vec<FlowEvent>) -> FlowError {
    let store = Arc::new(InMemoryEventStore::new());
    store
        .append(
            RUN_ID,
            FlowEvent::RunCreated {
                spec: spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store.append(RUN_ID, FlowEvent::RunStarted).await.unwrap();
    for event in events {
        store.append(RUN_ID, event).await.unwrap();
    }
    FlowEngine::new(store, Arc::new(SnapshotOnlyRuntime))
        .snapshot(RUN_ID)
        .await
        .unwrap_err()
}

fn signal(signal_id: &str, name: &str) -> FlowEvent {
    FlowEvent::SignalReceived {
        signal: WorkflowSignal::new(signal_id, name, json!({ "id": signal_id })),
    }
}

#[tokio::test]
async fn workflow_spec_rejects_an_empty_signal_contract_before_history_is_written() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(store.clone(), Arc::new(SnapshotOnlyRuntime));
    let error = engine
        .start_with_id("invalid-signal-spec", spec().with_signal(" "), json!({}))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidWorkflow(message)
            if message.contains("workflow signal name must not be empty")
    ));
    assert!(matches!(
        store.list("invalid-signal-spec").await.unwrap_err(),
        FlowError::RunNotFound(_)
    ));
}

#[tokio::test]
async fn projection_rejects_signal_events_before_run_started() {
    let store = Arc::new(InMemoryEventStore::new());
    store
        .append(
            RUN_ID,
            FlowEvent::RunCreated {
                spec: spec(),
                input: json!({}),
            },
        )
        .await
        .unwrap();
    store
        .append(RUN_ID, signal("delivery-1", APPROVAL_SIGNAL))
        .await
        .unwrap();
    let error = FlowEngine::new(store, Arc::new(SnapshotOnlyRuntime))
        .snapshot(RUN_ID)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FlowError::InvalidTransition(message)
            if message.contains("signal_received cannot precede run_started")
    ));
}

#[tokio::test]
async fn projection_rejects_undeclared_and_duplicate_signal_deliveries() {
    let undeclared = snapshot_error(vec![signal("delivery-1", "order.unknown")]).await;
    assert!(matches!(
        undeclared,
        FlowError::InvalidTransition(message) if message.contains("undeclared workflow signal name")
    ));

    let duplicate = snapshot_error(vec![
        signal("delivery-1", APPROVAL_SIGNAL),
        signal("delivery-1", APPROVAL_SIGNAL),
    ])
    .await;
    assert!(matches!(
        duplicate,
        FlowError::InvalidTransition(message) if message.contains("duplicates signal delivery-1")
    ));
}

#[tokio::test]
async fn projection_rejects_unknown_mismatched_and_reused_signal_pairings() {
    let unknown_wait = snapshot_error(vec![
        signal("delivery-1", APPROVAL_SIGNAL),
        FlowEvent::SignalWaitCompleted {
            wait_id: "missing".to_string(),
            signal_id: "delivery-1".to_string(),
        },
    ])
    .await;
    assert!(matches!(
        unknown_wait,
        FlowError::InvalidTransition(message) if message.contains("unknown wait missing")
    ));

    let mismatched = snapshot_error(vec![
        FlowEvent::SignalWaitCreated {
            wait_id: "approval".to_string(),
            signal_name: APPROVAL_SIGNAL.to_string(),
        },
        signal("delivery-1", RELEASE_SIGNAL),
        FlowEvent::SignalWaitCompleted {
            wait_id: "approval".to_string(),
            signal_id: "delivery-1".to_string(),
        },
    ])
    .await;
    assert!(matches!(
        mismatched,
        FlowError::InvalidTransition(message) if message.contains("pairs wait approval")
    ));

    let reused = snapshot_error(vec![
        FlowEvent::SignalWaitCreated {
            wait_id: "approval-1".to_string(),
            signal_name: APPROVAL_SIGNAL.to_string(),
        },
        FlowEvent::SignalWaitCreated {
            wait_id: "approval-2".to_string(),
            signal_name: APPROVAL_SIGNAL.to_string(),
        },
        signal("delivery-1", APPROVAL_SIGNAL),
        FlowEvent::SignalWaitCompleted {
            wait_id: "approval-1".to_string(),
            signal_id: "delivery-1".to_string(),
        },
        FlowEvent::SignalWaitCompleted {
            wait_id: "approval-2".to_string(),
            signal_id: "delivery-1".to_string(),
        },
    ])
    .await;
    assert!(matches!(
        reused,
        FlowError::InvalidTransition(message) if message.contains("reuses signal delivery-1")
    ));
}

#[tokio::test]
async fn projection_rejects_skipping_an_older_wait_for_the_same_signal_name() {
    let error = snapshot_error(vec![
        FlowEvent::SignalWaitCreated {
            wait_id: "approval-1".to_string(),
            signal_name: APPROVAL_SIGNAL.to_string(),
        },
        FlowEvent::SignalWaitCreated {
            wait_id: "approval-2".to_string(),
            signal_name: APPROVAL_SIGNAL.to_string(),
        },
        signal("delivery-1", APPROVAL_SIGNAL),
        FlowEvent::SignalWaitCompleted {
            wait_id: "approval-2".to_string(),
            signal_id: "delivery-1".to_string(),
        },
    ])
    .await;

    assert!(matches!(
        error,
        FlowError::InvalidTransition(message)
            if message.contains("skips older wait approval-1")
    ));
}

#[tokio::test]
async fn projection_rejects_skipping_an_older_unconsumed_signal_delivery() {
    let error = snapshot_error(vec![
        FlowEvent::SignalWaitCreated {
            wait_id: "approval".to_string(),
            signal_name: APPROVAL_SIGNAL.to_string(),
        },
        signal("delivery-1", APPROVAL_SIGNAL),
        signal("delivery-2", APPROVAL_SIGNAL),
        FlowEvent::SignalWaitCompleted {
            wait_id: "approval".to_string(),
            signal_id: "delivery-2".to_string(),
        },
    ])
    .await;

    assert!(matches!(
        error,
        FlowError::InvalidTransition(message)
            if message.contains("skips older signal delivery-1")
    ));
}

#[tokio::test]
async fn projection_prevents_continue_as_new_from_abandoning_signal_state() {
    let open_wait = snapshot_error(vec![
        FlowEvent::SignalWaitCreated {
            wait_id: "approval".to_string(),
            signal_name: APPROVAL_SIGNAL.to_string(),
        },
        FlowEvent::RunContinuedAsNew {
            successor_run_id: "signal-validation-successor".to_string(),
            input: json!({}),
        },
    ])
    .await;
    assert!(matches!(
        open_wait,
        FlowError::InvalidTransition(message) if message.contains("open signal wait")
    ));

    let unconsumed = snapshot_error(vec![
        signal("delivery-1", APPROVAL_SIGNAL),
        FlowEvent::RunContinuedAsNew {
            successor_run_id: "signal-validation-successor".to_string(),
            input: json!({}),
        },
    ])
    .await;
    assert!(matches!(
        unconsumed,
        FlowError::InvalidTransition(message) if message.contains("unconsumed signal delivery-1")
    ));
}
