#[cfg(feature = "sqlite")]
use a3s_flow::{
    FlowEngine, FlowError, FlowHistoryRetentionPolicy, FlowRuntime, RuntimeCommand,
    SqliteEventStore, StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
#[cfg(feature = "sqlite")]
use async_trait::async_trait;
#[cfg(feature = "sqlite")]
use chrono::{Duration, Utc};
#[cfg(feature = "sqlite")]
use serde_json::json;
#[cfg(feature = "sqlite")]
use std::sync::Arc;

#[cfg(feature = "sqlite")]
struct RetentionRuntime;

#[cfg(feature = "sqlite")]
#[async_trait]
impl FlowRuntime for RetentionRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        match ctx.input()["mode"].as_str() {
            Some("complete") => Ok(ctx.complete(json!({ "archived": true }))),
            Some("wait") => Ok(ctx.wait_until("retention-window", Utc::now() + Duration::hours(1))),
            other => Err(FlowError::Runtime(format!(
                "unknown retention mode: {other:?}"
            ))),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("retention example does not schedule steps")
    }
}

#[cfg(feature = "sqlite")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let directory = tempfile::tempdir()?;
    let database_url = format!("sqlite://{}", directory.path().join("flow.db").display());
    let store = Arc::new(SqliteEventStore::connect(database_url).await?);
    let engine = FlowEngine::new(store.clone(), Arc::new(RetentionRuntime));
    let spec =
        WorkflowSpec::rust_embedded("examples.sqlite-retention", "0.1.0", "examples", "main");

    engine
        .start_with_id("finished-run", spec.clone(), json!({ "mode": "complete" }))
        .await?;
    engine
        .start_with_id("held-run", spec.clone(), json!({ "mode": "complete" }))
        .await?;
    engine
        .start_with_id("waiting-run", spec, json!({ "mode": "wait" }))
        .await?;
    store
        .hold_history("held-run", "audit-export", "audit export is pending")
        .await?;

    let policy = FlowHistoryRetentionPolicy::new(Utc::now() + Duration::minutes(1));
    let report = store.prune_terminal_history(policy.clone()).await?;
    let waiting = engine.snapshot("waiting-run").await?;
    let tombstone = store.history_tombstone("finished-run").await?;

    println!("deleted={:?}", report.deleted_run_ids);
    println!("held={:?}", report.held_run_ids);
    println!("waiting_status={:?}", waiting.status);
    println!("finished_tombstone={tombstone:?}");
    assert_eq!(report.deleted_run_ids, vec!["finished-run"]);
    assert_eq!(report.held_run_ids, vec!["held-run"]);
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert!(tombstone.is_some());

    store
        .release_history_hold("held-run", "audit-export")
        .await?;
    let released = store.prune_terminal_history(policy).await?;
    assert_eq!(released.deleted_run_ids, vec!["held-run"]);
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
fn main() {
    println!("sqlite feature not enabled; run with:");
    println!("cargo run --example sqlite_retention --features sqlite");
}
