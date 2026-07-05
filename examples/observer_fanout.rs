use a3s_flow::{
    A3sFlowEventBridge, FanoutFlowEventObserver, FlowEngine, FlowRuntime, InMemoryA3sFlowEventSink,
    InMemoryFlowEventObserver, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowSpec,
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
    let raw_observer = Arc::new(InMemoryFlowEventObserver::new());
    let a3s_sink = Arc::new(InMemoryA3sFlowEventSink::new());
    let a3s_bridge = Arc::new(A3sFlowEventBridge::new(a3s_sink.clone()));
    let observer = Arc::new(
        FanoutFlowEventObserver::new()
            .with_observer(raw_observer.clone())
            .with_observer(a3s_bridge),
    );
    let engine = FlowEngine::builder(Arc::new(AuditRuntime))
        .with_observer(observer)
        .build();
    let spec = WorkflowSpec::rust_embedded("examples.observer-fanout", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id("observer-fanout-demo", spec, json!({ "topic": "A3S Flow" }))
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;
    let raw_events = raw_observer.events().await;
    let a3s_events = a3s_sink.events().await;

    println!("status={:?}", snapshot.status);
    println!("raw_events={}", raw_events.len());
    println!("a3s_events={}", a3s_events.len());
    println!(
        "last_a3s_key={}",
        a3s_events
            .last()
            .map(|event| event.key.as_str())
            .unwrap_or("none")
    );
    Ok(())
}
