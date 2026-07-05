use a3s_flow::{
    A3sFlowEventBridge, FlowEngine, FlowRuntime, LocalFileA3sFlowEventSink, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct AuditRuntime;

#[async_trait]
impl FlowRuntime for AuditRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(output) = ctx.step_output("build-report") {
            return Ok(ctx.complete(output.clone()));
        }

        Ok(ctx.schedule_step(
            "build-report",
            "build_report",
            json!({ "topic": ctx.input()["topic"] }),
        ))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!({
            "title": format!("Report: {}", invocation.input["topic"].as_str().unwrap_or("unknown")),
            "ready": true,
        }))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let dir = tempfile::tempdir()?;
    let audit_path = dir.path().join("audit/flow-events.jsonl");
    let sink = Arc::new(LocalFileA3sFlowEventSink::new(&audit_path));
    let observer = Arc::new(A3sFlowEventBridge::new(sink.clone()));
    let engine = FlowEngine::builder(Arc::new(AuditRuntime))
        .with_observer(observer)
        .build();
    let spec = WorkflowSpec::rust_embedded("examples.local-audit", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id("local-audit-demo", spec, json!({ "topic": "A3S Flow" }))
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;
    let events = sink.events().await?;

    println!("audit_log={}", audit_path.display());
    println!("status={:?}", snapshot.status);
    println!("audit_events={}", events.len());
    println!("first_event={}", events.first().unwrap().key);
    println!("last_event={}", events.last().unwrap().key);
    println!("last_error={:?}", sink.last_error().await);
    Ok(())
}
