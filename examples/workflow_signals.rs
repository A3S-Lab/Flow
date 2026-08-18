use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowSignal, WorkflowSpec,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

const APPROVAL_SIGNAL: &str = "invoice.approved";

#[derive(Deserialize)]
struct Approval {
    reviewer: String,
}

struct InvoiceRuntime;

#[async_trait]
impl FlowRuntime for InvoiceRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let Some(approval) = context.signal_payload_as::<Approval>("approval")? else {
            return Ok(context.wait_for_signal("approval", APPROVAL_SIGNAL));
        };
        Ok(context.complete(json!({
            "status": "approved",
            "reviewer": approval.reviewer,
        })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(
            "invoice signal example does not execute steps".to_string(),
        ))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(InvoiceRuntime));
    let spec = WorkflowSpec::rust_embedded(
        "example.invoice-approval",
        "1",
        "examples::workflow_signals",
        "main",
    )
    .with_signal(APPROVAL_SIGNAL);
    let run_id = engine
        .start_with_id("invoice-2026-0001", spec, json!({}))
        .await?;

    let waiting = engine.snapshot(&run_id).await?;
    let wait_status = waiting
        .signal_waits
        .get("approval")
        .ok_or_else(|| FlowError::Runtime("approval signal wait was not created".to_string()))?
        .status;
    println!("waiting={wait_status:?}");

    let completed = engine
        .send_signal(
            &run_id,
            WorkflowSignal::new(
                "approval-decision-2026-0001",
                APPROVAL_SIGNAL,
                json!({ "reviewer": "finance@example.com" }),
            ),
        )
        .await?;
    println!(
        "status={:?} output={:?}",
        completed.status, completed.output
    );
    Ok(())
}
