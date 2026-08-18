use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowPatchId,
    WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::{json, Value};

const PATCH_ID: &str = "checkout.calculation-v2";

struct CheckoutRuntime;

#[async_trait]
impl FlowRuntime for CheckoutRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let implementation = if context.has_patch_marker(PATCH_ID) {
            "v2"
        } else {
            "v1"
        };
        Ok(context.complete(json!({ "implementation": implementation })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("this workflow has no side-effecting steps")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(CheckoutRuntime));
    let base = WorkflowSpec::rust_embedded(
        "checkout.calculate",
        "2",
        "examples::replay_safe_patch",
        "main",
    );
    let patched = base
        .clone()
        .with_patch_marker(WorkflowPatchId::new(PATCH_ID)?);

    let legacy_run = engine.start(base, json!({})).await?;
    let patched_run = engine.start(patched, json!({})).await?;

    println!(
        "legacy={}",
        engine.snapshot(&legacy_run).await?.output.unwrap()
    );
    println!(
        "patched={}",
        engine.snapshot(&patched_run).await?.output.unwrap()
    );
    Ok(())
}
