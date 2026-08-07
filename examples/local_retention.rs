use a3s_flow::{
    ChildOperationReference, FlowEngine, FlowError, FlowEventStore, FlowRuntime,
    LocalFileEventStore, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec,
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
        .start_with_id("linked-child", spec.clone(), json!({ "mode": "complete" }))
        .await?;
    engine
        .start_with_id("linked-parent", spec, json!({ "mode": "wait" }))
        .await?;
    engine
        .link_child_operation(
            "linked-parent",
            ChildOperationReference::new("child", "flow.run", "linked-child")
                .with_flow_run_id("linked-child"),
        )
        .await?;

    let terminal_before = Utc::now() + ChronoDuration::minutes(1);
    let first_removed = store
        .prune_terminal_runs_older_than(terminal_before)
        .await?;
    let parent = engine.snapshot("linked-parent").await?;

    println!("first_removed={first_removed:?}");
    println!("parent_status={:?}", parent.status);
    assert_eq!(first_removed, vec!["finished-run".to_string()]);
    assert_eq!(parent.status, WorkflowRunStatus::Suspended);
    assert!(store.list("linked-child").await.is_ok());

    engine
        .cancel("linked-parent", Some("retention window closed".into()))
        .await?;
    let component_removed = store
        .prune_terminal_runs_older_than(terminal_before)
        .await?;
    println!("component_removed={component_removed:?}");
    assert_eq!(
        component_removed,
        vec!["linked-child".to_string(), "linked-parent".to_string()]
    );

    let _ = tokio::fs::remove_dir_all(&root).await;
    Ok(())
}
