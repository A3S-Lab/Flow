use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
struct ParentInput {
    batch: u64,
}

#[derive(Deserialize, Serialize)]
struct ChildInput {
    batch: u64,
}

#[derive(Deserialize, Serialize)]
struct ChildOutput {
    imported: u64,
}

fn child_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "examples.child-workflow.child",
        "1",
        "examples::child_workflow",
        "child",
    )
}

struct ParentChildRuntime;

#[async_trait]
impl FlowRuntime for ParentChildRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.spec().name.as_str() {
            "examples.child-workflow.parent" => {
                let input = context.input_as::<ParentInput>()?;
                match context.child_workflow_outcome("import") {
                    Some(WorkflowTerminalOutcome::Completed { output }) => {
                        let child: ChildOutput = serde_json::from_value(output.clone())?;
                        Ok(context.complete(json!(child)))
                    }
                    Some(outcome) => Ok(context.fail(format!("child import failed: {outcome:?}"))),
                    None => Ok(context.start_child_workflow(
                        "import",
                        child_spec(),
                        json!(ChildInput { batch: input.batch }),
                    )),
                }
            }
            "examples.child-workflow.child" => {
                let input = context.input_as::<ChildInput>()?;
                Ok(context.complete(json!(ChildOutput {
                    imported: input.batch,
                })))
            }
            name => Err(FlowError::Runtime(format!(
                "unknown workflow in child example: {name}"
            ))),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<Value> {
        Err(FlowError::Runtime(format!(
            "child workflow example does not define step {}",
            invocation.step_name
        )))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(ParentChildRuntime));
    let parent_spec = WorkflowSpec::rust_embedded(
        "examples.child-workflow.parent",
        "1",
        "examples::child_workflow",
        "parent",
    );
    let parent_run_id = engine
        .start_with_id("child-workflow-demo", parent_spec, json!({ "batch": 7 }))
        .await?;

    let parent = engine.snapshot(&parent_run_id).await?;
    let child = parent.child_workflow("import").ok_or_else(|| {
        FlowError::Runtime("parent completed without the durable child projection".to_string())
    })?;
    let output = child.output_as::<ChildOutput>()?.ok_or_else(|| {
        FlowError::Runtime("child workflow did not complete successfully".to_string())
    })?;

    println!("parent_run={} status={:?}", parent.run_id, parent.status);
    println!("child_run={} imported={}", child.run_id, output.imported);
    Ok(())
}
