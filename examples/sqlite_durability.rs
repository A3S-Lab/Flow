#[cfg(feature = "sqlite")]
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, SqliteEventStore, StepInvocation,
    WorkflowInvocation, WorkflowSpec,
};
#[cfg(feature = "sqlite")]
use async_trait::async_trait;
#[cfg(feature = "sqlite")]
use serde_json::json;
#[cfg(feature = "sqlite")]
use std::sync::Arc;

#[cfg(feature = "sqlite")]
struct GreetingRuntime;

#[cfg(feature = "sqlite")]
#[async_trait]
impl FlowRuntime for GreetingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(output) = ctx.step_output("greet") {
            return Ok(ctx.complete(output.clone()));
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
            step => Err(FlowError::Runtime(format!("unknown step {step}"))),
        }
    }
}

#[cfg(feature = "sqlite")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let db_path =
        std::env::temp_dir().join(format!("a3s-flow-sqlite-example-{}.db", std::process::id()));
    let _ = tokio::fs::remove_file(&db_path).await;
    let url = format!("sqlite://{}", db_path.display());

    let spec = WorkflowSpec::rust_embedded("examples.sqlite", "0.1.0", "examples", "main");
    {
        let store = Arc::new(SqliteEventStore::connect(&url).await?);
        let engine = FlowEngine::new(store, Arc::new(GreetingRuntime));
        engine
            .start_with_id(
                "sqlite-greeting-ada",
                spec.clone(),
                json!({ "name": "Ada" }),
            )
            .await?;
    }

    let store = Arc::new(SqliteEventStore::connect(&url).await?);
    let engine = FlowEngine::new(store, Arc::new(GreetingRuntime));
    let snapshot = engine.snapshot("sqlite-greeting-ada").await?;

    println!("run_id={}", snapshot.run_id);
    println!("status={:?}", snapshot.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&snapshot.output).unwrap()
    );

    let _ = tokio::fs::remove_file(&db_path).await;
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
fn main() {
    println!("sqlite feature not enabled; run with:");
    println!("cargo run --example sqlite_durability --features sqlite");
}
