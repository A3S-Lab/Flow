#[cfg(feature = "postgres")]
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, PostgresEventStore, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowSpec,
};
#[cfg(feature = "postgres")]
use async_trait::async_trait;
#[cfg(feature = "postgres")]
use serde_json::json;
#[cfg(feature = "postgres")]
use std::sync::Arc;
#[cfg(feature = "postgres")]
use uuid::Uuid;

#[cfg(feature = "postgres")]
struct GreetingRuntime;

#[cfg(feature = "postgres")]
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

#[cfg(feature = "postgres")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let Some(url) = std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
    else {
        println!("set A3S_FLOW_POSTGRES_URL=postgres://user:pass@host:5432/db to run it");
        return Ok(());
    };

    let run_id = format!("postgres-greeting-{}", Uuid::new_v4());
    let spec = WorkflowSpec::rust_embedded("examples.postgres", "0.1.0", "examples", "main");
    {
        let store = Arc::new(PostgresEventStore::connect(&url).await?);
        let engine = FlowEngine::new(store, Arc::new(GreetingRuntime));
        engine
            .start_with_id(&run_id, spec.clone(), json!({ "name": "Ada" }))
            .await?;
    }

    let store = Arc::new(PostgresEventStore::connect(&url).await?);
    let engine = FlowEngine::new(store, Arc::new(GreetingRuntime));
    let snapshot = engine.snapshot(&run_id).await?;

    println!("run_id={}", snapshot.run_id);
    println!("status={:?}", snapshot.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&snapshot.output).unwrap()
    );

    Ok(())
}

#[cfg(not(feature = "postgres"))]
fn main() {
    println!("postgres feature not enabled; run with:");
    println!("cargo run --example postgres_durability --features postgres");
}
