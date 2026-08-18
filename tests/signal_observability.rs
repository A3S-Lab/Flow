use std::sync::Arc;

use a3s_flow::{
    A3sFlowEventBridge, FlowEngine, FlowError, FlowRuntime, InMemoryA3sFlowEventSink,
    RuntimeCommand, SignalWaitStatus, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowRunSuspension, WorkflowSignal, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

const SIGNAL_NAME: &str = "order.approved";

struct ObservableSignalRuntime;

#[async_trait]
impl FlowRuntime for ObservableSignalRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let Some(payload) = context.signal_payload("approval") else {
            return Ok(context.wait_for_signal("approval", SIGNAL_NAME));
        };
        Ok(context.complete(payload.clone()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(
            "signal observability workflow does not execute steps".to_string(),
        ))
    }
}

#[tokio::test]
async fn signal_waits_are_visible_to_inspection_and_the_event_bridge() {
    let sink = Arc::new(InMemoryA3sFlowEventSink::new());
    let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
    let engine = FlowEngine::builder(Arc::new(ObservableSignalRuntime))
        .with_observer(observer)
        .build();
    let spec = WorkflowSpec::rust_embedded(
        "test.signal-observability",
        "1",
        "tests::signal_observability",
        "main",
    )
    .with_signal(SIGNAL_NAME);
    let run_id = engine
        .start_with_id("observable-signal-run", spec, json!({}))
        .await
        .unwrap();

    let suspensions = engine.list_open_suspensions(Utc::now()).await.unwrap();
    assert_eq!(suspensions.len(), 1);
    assert!(matches!(
        &suspensions[0],
        WorkflowRunSuspension::Signal { run_id: owner, wait }
            if owner == &run_id
                && wait.wait_id == "approval"
                && wait.signal_name == SIGNAL_NAME
                && wait.status == SignalWaitStatus::Waiting
    ));

    let completed = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new(
                "approval-delivery-1",
                SIGNAL_NAME,
                json!({ "approved": true }),
            ),
        )
        .await
        .unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert!(engine
        .list_open_suspensions(Utc::now())
        .await
        .unwrap()
        .is_empty());

    let events = sink.events().await;
    let received = events
        .iter()
        .find(|event| event.key == "flow.signal.received")
        .unwrap();
    assert_eq!(received.status.as_deref(), Some("received"));
    assert_eq!(received.subject.as_ref().unwrap().kind, "signal");
    assert_eq!(received.subject.as_ref().unwrap().id, "approval-delivery-1");
    assert!(received.workflow.as_ref().is_some_and(|workflow| {
        workflow.name == "test.signal-observability" && workflow.version == "1"
    }));

    let wait_events = events
        .iter()
        .filter(|event| {
            event
                .subject
                .as_ref()
                .is_some_and(|subject| subject.kind == "signal_wait" && subject.id == "approval")
        })
        .collect::<Vec<_>>();
    assert_eq!(wait_events.len(), 2);
    assert_eq!(wait_events[0].key, "flow.signal.wait.created");
    assert_eq!(wait_events[0].status.as_deref(), Some("waiting"));
    assert_eq!(wait_events[1].key, "flow.signal.wait.completed");
    assert_eq!(wait_events[1].status.as_deref(), Some("completed"));

    let labels = received.safe_metric_labels();
    assert_eq!(labels["event_key"], "flow.signal.received");
    assert_eq!(labels["status"], "received");
    assert!(!labels.contains_key("run_id"));
    assert!(!labels.contains_key("signal_id"));
}
