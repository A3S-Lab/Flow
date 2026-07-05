use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, LocalFileEventStore, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct GreetingRuntime;

#[async_trait]
impl FlowRuntime for GreetingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(greeting) = ctx.step_output("greet") {
            return Ok(ctx.complete(greeting.clone()));
        }

        Ok(ctx.schedule_step(
            "greet",
            "greet_user",
            json!({ "name": ctx.input()["name"] }),
        ))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "greet_user" => Ok(json!({
                "message": format!("hello {}", invocation.input["name"].as_str().unwrap_or("unknown")),
            })),
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let root = std::env::temp_dir().join(format!(
        "a3s-flow-local-file-example-{}",
        std::process::id()
    ));
    let _ = tokio::fs::remove_dir_all(&root).await;

    let runtime = Arc::new(GreetingRuntime);
    let store = Arc::new(LocalFileEventStore::new(&root));
    let engine = FlowEngine::new(store.clone(), runtime.clone());
    let spec = WorkflowSpec::rust_embedded("examples.local-file", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id("local-file-demo", spec.clone(), json!({ "name": "Ada" }))
        .await?;
    let first_snapshot = engine.snapshot(&run_id).await?;
    assert_eq!(first_snapshot.status, WorkflowRunStatus::Completed);

    let rebuilt_engine = FlowEngine::new(Arc::new(LocalFileEventStore::new(&root)), runtime);
    let rebuilt_snapshot = rebuilt_engine.snapshot("local-file-demo").await?;
    let replayed_run_id = rebuilt_engine
        .start_with_id("local-file-demo", spec, json!({ "name": "Ada" }))
        .await?;
    let history = rebuilt_engine.history("local-file-demo").await?;

    println!("event_store={}", root.display());
    println!("run_id={replayed_run_id}");
    println!("status={:?}", rebuilt_snapshot.status);
    println!("history_events={}", history.len());
    println!(
        "output={}",
        serde_json::to_string_pretty(&rebuilt_snapshot.output).unwrap()
    );

    let _ = tokio::fs::remove_dir_all(&root).await;
    Ok(())
}
