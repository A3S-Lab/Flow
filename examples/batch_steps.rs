use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RetryPolicy, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

struct ResearchRuntime;

#[async_trait]
impl FlowRuntime for ResearchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let facts = ctx.step_output("facts");
        let risks = ctx.step_output("risks");
        let sources = ctx.step_output("sources");

        match (facts, risks, sources) {
            (Some(facts), Some(risks), Some(sources)) => Ok(ctx.complete(json!({
                "facts": facts,
                "risks": risks,
                "sources": sources,
            }))),
            _ => Ok(ctx.schedule_steps(vec![
                ctx.step(
                    "facts",
                    "research_track",
                    json!({ "track": "facts", "query": ctx.input()["query"] }),
                ),
                ctx.step_with_retry(
                    "risks",
                    "research_track",
                    json!({ "track": "risks", "query": ctx.input()["query"] }),
                    RetryPolicy::fixed(2, Duration::from_millis(0)),
                ),
                ctx.step(
                    "sources",
                    "research_track",
                    json!({ "track": "sources", "query": ctx.input()["query"] }),
                ),
            ])),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "research_track" => Ok(json!({
                "track": invocation.input["track"],
                "query": invocation.input["query"],
                "notes": ["evidence gathered", "ready for synthesis"],
            })),
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(ResearchRuntime));
    let spec = WorkflowSpec::rust_embedded("examples.research", "0.1.0", "examples", "main");

    let run_id = engine
        .start(spec, json!({ "query": "A3S Flow durable workflows" }))
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("status={:?}", snapshot.status);
    println!("durable_steps={}", snapshot.steps.len());
    println!(
        "output={}",
        serde_json::to_string_pretty(&snapshot.output).unwrap()
    );
    Ok(())
}
