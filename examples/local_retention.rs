use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, LocalFileEventStore, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::Arc;

struct RetentionRuntime;

#[async_trait]
impl FlowRuntime for RetentionRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        match ctx.input()["mode"].as_str() {
            Some("complete") => Ok(ctx.complete(json!({ "kept": false }))),
            Some("wait") => {
                Ok(ctx.wait_until("retention-window", Utc::now() + ChronoDuration::hours(1)))
            }
            other => Err(FlowError::Runtime(format!(
                "unknown retention mode: {other:?}"
            ))),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("retention example does not schedule steps")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let root =
        std::env::temp_dir().join(format!("a3s-flow-local-retention-{}", std::process::id()));
    let _ = tokio::fs::remove_dir_all(&root).await;

    let store = Arc::new(LocalFileEventStore::new(&root));
    let engine = FlowEngine::new(store.clone(), Arc::new(RetentionRuntime));
    let spec = WorkflowSpec::rust_embedded("examples.local-retention", "0.1.0", "examples", "main");

    engine
        .start_with_id("finished-run", spec.clone(), json!({ "mode": "complete" }))
        .await?;
    engine
        .start_with_id("waiting-run", spec, json!({ "mode": "wait" }))
        .await?;

    let removed = store
        .prune_terminal_runs_older_than(Utc::now() + ChronoDuration::minutes(1))
        .await?;
    let waiting = engine.snapshot("waiting-run").await?;

    println!("removed={removed:?}");
    println!("waiting_status={:?}", waiting.status);
    assert_eq!(removed, vec!["finished-run".to_string()]);
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);

    let _ = tokio::fs::remove_dir_all(&root).await;
    Ok(())
}
