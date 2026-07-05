use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, HookCallbackRoute, HookMetadata, HookStatus,
    RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct ApprovalRuntime;

#[async_trait]
impl FlowRuntime for ApprovalRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(payload) = ctx.hook_payload("approval") {
            return Ok(ctx.complete(json!({
                "status": "received",
                "approved": payload["approved"],
            })));
        }
        if ctx.hook_disposed("approval") {
            return Ok(ctx.complete(json!({
                "status": "withdrawn",
                "reason": "approval request was closed before callback",
            })));
        }

        let invoice_id = ctx.input()["invoiceId"].as_str().unwrap_or("unknown");
        let metadata = HookMetadata::human_approval(format!("invoice:{invoice_id}"))
            .with_callback_route(HookCallbackRoute::post("/callbacks/flow/hooks/{token}"))
            .with_data("invoiceId", json!(invoice_id));

        Ok(ctx.create_hook_with_metadata(
            "approval",
            ctx.input()["approvalToken"]
                .as_str()
                .unwrap_or("approval-token"),
            metadata,
        )?)
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("approval runtime does not schedule steps")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(ApprovalRuntime));
    let spec =
        WorkflowSpec::rust_embedded("examples.approval-disposal", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id(
            "approval-disposal-demo-0001",
            spec,
            json!({
                "invoiceId": "inv-0001",
                "approvalToken": "approval-token-0001",
            }),
        )
        .await?;
    let waiting = engine.snapshot(&run_id).await?;
    assert_eq!(waiting.hooks["approval"].status, HookStatus::Active);
    println!("waiting_for_token={}", waiting.hooks["approval"].token);

    let (disposed_run_id, hook_id) = engine.dispose_hook_by_token("approval-token-0001").await?;
    let completed = engine.snapshot(&disposed_run_id).await?;
    assert_eq!(completed.hooks["approval"].status, HookStatus::Disposed);
    assert_eq!(completed.output.as_ref().unwrap()["status"], "withdrawn");

    println!("disposed_hook={hook_id}");
    println!("status={:?}", completed.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&completed.output).unwrap()
    );

    let late = engine
        .resume_hook_by_token("approval-token-0001", json!({ "approved": true }))
        .await;
    assert!(
        matches!(late, Err(FlowError::HookTokenNotFound(token)) if token == "approval-token-0001")
    );
    println!("late_callback=rejected");

    Ok(())
}
