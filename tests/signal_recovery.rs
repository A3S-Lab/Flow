use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowRuntime, LocalFileEventStore, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSignal, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;

const SIGNAL_NAME: &str = "order.approved";

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "test.signal-recovery",
        "1",
        "tests::signal_recovery",
        "main",
    )
    .with_signal(SIGNAL_NAME)
}

struct RecoveryRuntime {
    fail_once_after_pairing: AtomicBool,
}

#[async_trait]
impl FlowRuntime for RecoveryRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let Some(payload) = context.signal_payload("approval") else {
            return Ok(context.wait_for_signal("approval", SIGNAL_NAME));
        };
        if self.fail_once_after_pairing.swap(false, Ordering::SeqCst) {
            return Err(FlowError::Runtime(
                "simulated host loss after durable signal pairing".to_string(),
            ));
        }
        Ok(context.complete(payload.clone()))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("signal recovery workflow does not execute steps")
    }
}

#[tokio::test]
async fn local_file_replacement_recovers_a_committed_signal_without_redelivery_duplication() {
    let directory = tempfile::tempdir().unwrap();
    let first = FlowEngine::new(
        Arc::new(LocalFileEventStore::new(directory.path())),
        Arc::new(RecoveryRuntime {
            fail_once_after_pairing: AtomicBool::new(true),
        }),
    );
    let run_id = first
        .start_with_id("local-signal-recovery", spec(), json!({}))
        .await
        .unwrap();
    let signal = WorkflowSignal::new(
        "approval-delivery-1",
        SIGNAL_NAME,
        json!({ "approved": true }),
    );

    let error = first
        .send_signal(&run_id, signal.clone())
        .await
        .unwrap_err();
    assert!(matches!(error, FlowError::Runtime(message) if message.contains("host loss")));
    drop(first);

    let replacement = FlowEngine::new(
        Arc::new(LocalFileEventStore::new(directory.path())),
        Arc::new(RecoveryRuntime {
            fail_once_after_pairing: AtomicBool::new(false),
        }),
    );
    let completed = replacement.send_signal(&run_id, signal).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output, Some(json!({ "approved": true })));
    let history = replacement.history(&run_id).await.unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalReceived { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::SignalWaitCompleted { .. }))
            .count(),
        1
    );
}
