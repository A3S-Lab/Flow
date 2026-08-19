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
    values: Vec<u64>,
}

#[derive(Deserialize, Serialize)]
struct ChildInput {
    value: u64,
}

#[derive(Deserialize, Serialize)]
struct ChildOutput {
    value: u64,
    square: u64,
}

fn child_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        "examples.child-workflow-batch.child",
        "1",
        "examples::child_workflow_batch",
        "child",
    )
}

struct BatchRuntime;

#[async_trait]
impl FlowRuntime for BatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.spec().name.as_str() {
            "examples.child-workflow-batch.parent" => {
                let input = context.input_as::<ParentInput>()?;
                let mut outputs = Vec::with_capacity(input.values.len());
                let mut all_resolved = true;
                for ordinal in 0..input.values.len() {
                    let child_id = format!("item-{ordinal:04}");
                    match context.child_workflow_outcome(&child_id) {
                        Some(WorkflowTerminalOutcome::Completed { output }) => {
                            outputs.push(serde_json::from_value::<ChildOutput>(output.clone())?);
                        }
                        Some(outcome) => {
                            return Ok(
                                context.fail(format!("child {child_id} failed: {outcome:?}"))
                            );
                        }
                        None => all_resolved = false,
                    }
                }
                if all_resolved {
                    return Ok(context.complete(json!(outputs)));
                }

                let children = input
                    .values
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, value)| {
                        context.child_workflow(
                            format!("item-{ordinal:04}"),
                            child_spec(),
                            json!(ChildInput { value }),
                        )
                    })
                    .collect();
                Ok(context.start_child_workflows(children))
            }
            "examples.child-workflow-batch.child" => {
                let input = context.input_as::<ChildInput>()?;
                let square = input.value.checked_mul(input.value).ok_or_else(|| {
                    FlowError::Runtime("child square exceeds the u64 range".to_string())
                })?;
                Ok(context.complete(json!(ChildOutput {
                    value: input.value,
                    square,
                })))
            }
            name => Err(FlowError::Runtime(format!(
                "unknown workflow in child batch example: {name}"
            ))),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<Value> {
        Err(FlowError::Runtime(format!(
            "child workflow batch example does not define step {}",
            invocation.step_name
        )))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(BatchRuntime));
    let parent_spec = WorkflowSpec::rust_embedded(
        "examples.child-workflow-batch.parent",
        "1",
        "examples::child_workflow_batch",
        "parent",
    );
    let run_id = engine
        .start_with_id(
            "child-workflow-batch-demo",
            parent_spec,
            json!({ "values": [2, 3, 5] }),
        )
        .await?;
    let parent = engine.snapshot(&run_id).await?;

    println!("run={} status={:?}", parent.run_id, parent.status);
    println!("output={}", parent.output.unwrap_or(Value::Null));
    Ok(())
}
