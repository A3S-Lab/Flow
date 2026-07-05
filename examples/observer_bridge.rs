use a3s_flow::{
    FlowEngine, FlowEventEnvelope, FlowEventObserver, FlowRuntime, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default)]
struct AuditObserver {
    lines: Mutex<Vec<String>>,
}

impl AuditObserver {
    async fn lines(&self) -> Vec<String> {
        self.lines.lock().await.clone()
    }
}

#[async_trait]
impl FlowEventObserver for AuditObserver {
    async fn observe(&self, envelope: FlowEventEnvelope) {
        self.lines.lock().await.push(format!(
            "{} seq={} key={}",
            envelope.run_id,
            envelope.sequence,
            envelope.event.event_key()
        ));
    }
}

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
    let observer = Arc::new(AuditObserver::default());
    let engine = FlowEngine::builder(Arc::new(AuditRuntime))
        .with_observer(observer.clone())
        .build();
    let spec = WorkflowSpec::rust_embedded("examples.audit", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id("observer-demo", spec, json!({ "topic": "A3S Flow" }))
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("status={:?}", snapshot.status);
    println!("observed_events:");
    for line in observer.lines().await {
        println!("  {line}");
    }
    Ok(())
}
