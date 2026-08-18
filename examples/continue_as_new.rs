use std::sync::Arc;

use a3s_flow::{
    FlowEngine, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SegmentInput {
    cursor: u64,
    total: u64,
}

struct SegmentedImport;

#[async_trait]
impl FlowRuntime for SegmentedImport {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        let input: SegmentInput = context.input_as()?;
        if input.cursor < input.total {
            return Ok(context.continue_as_new(json!(SegmentInput {
                cursor: input.cursor + 1,
                total: input.total,
            })));
        }
        Ok(context.complete(json!({ "processed": input.total })))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<Value> {
        unreachable!("this history-segmentation example has no side-effecting steps")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(SegmentedImport));
    let spec = WorkflowSpec::rust_embedded(
        "examples.segmented-import",
        "1",
        "examples::continue_as_new",
        "main",
    );

    let root_run_id = engine
        .start_with_id(
            "segmented-import",
            spec,
            json!(SegmentInput {
                cursor: 0,
                total: 3,
            }),
        )
        .await?;
    let chain = engine.continuation_chain(&root_run_id).await?;

    for snapshot in &chain {
        println!(
            "run={} status={:?} events={}",
            snapshot.run_id,
            snapshot.status,
            engine.history(&snapshot.run_id).await?.len()
        );
    }
    assert_eq!(chain.len(), 4);
    assert_eq!(chain.last().unwrap().status, WorkflowRunStatus::Completed);
    Ok(())
}
