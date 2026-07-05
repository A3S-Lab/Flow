use a3s_flow::{
    FlowEngine, FlowRuntime, HookCallbackRoute, HookMetadata, HookStatus, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowSpec,
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
                "approved": payload["approved"],
                "reviewer": payload["reviewer"],
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
    let spec = WorkflowSpec::rust_embedded("examples.approval", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id(
            "approval-demo-0001",
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

    let (resumed_run_id, hook_id) = engine
        .resume_hook_by_token(
            "approval-token-0001",
            json!({ "approved": true, "reviewer": "finance@example.com" }),
        )
        .await?;
    let completed = engine.snapshot(&resumed_run_id).await?;

    println!("resumed_hook={hook_id}");
    println!("status={:?}", completed.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&completed.output).unwrap()
    );
    Ok(())
}
