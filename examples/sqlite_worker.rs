#[cfg(feature = "sqlite")]
use a3s_flow::{
    FlowEngine, FlowRuntime, FlowScheduler, FlowTaskQueue, FlowWorker, LocalFileFlowTaskQueue,
    RuntimeCommand, SqliteEventStore, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSpec,
};
#[cfg(feature = "sqlite")]
use async_trait::async_trait;
#[cfg(feature = "sqlite")]
use chrono::{DateTime, Duration as ChronoDuration, Utc};
#[cfg(feature = "sqlite")]
use serde::Deserialize;
#[cfg(feature = "sqlite")]
use serde_json::json;
#[cfg(feature = "sqlite")]
use std::sync::Arc;

#[cfg(feature = "sqlite")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReminderInput {
    message: String,
    resume_at: DateTime<Utc>,
}

#[cfg(feature = "sqlite")]
struct ReminderRuntime;

#[cfg(feature = "sqlite")]
#[async_trait]
impl FlowRuntime for ReminderRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let input = ctx.input_as::<ReminderInput>()?;

        if ctx.wait_completed("reminder-due") {
            return Ok(ctx.complete(json!({
                "sent": true,
                "message": input.message,
            })));
        }

        Ok(ctx.wait_until("reminder-due", input.resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("reminder runtime does not schedule steps")
    }
}

#[cfg(feature = "sqlite")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let now = Utc::now();
    let root = std::env::temp_dir().join(format!("a3s-flow-sqlite-worker-{}", std::process::id()));
    let db_path = root.join("flow.db");
    let task_root = root.join("tasks");
    let _ = tokio::fs::remove_dir_all(&root).await;
    tokio::fs::create_dir_all(&root).await?;

    let url = format!("sqlite://{}", db_path.display());
    let spec = WorkflowSpec::rust_embedded("examples.sqlite-worker", "0.1.0", "examples", "main");
    let run_id = "sqlite-worker-reminder";

    {
        let store = Arc::new(SqliteEventStore::connect(&url).await?);
        let engine = FlowEngine::new(store, Arc::new(ReminderRuntime));
        let queue = Arc::new(LocalFileFlowTaskQueue::new(&task_root));
        let scheduler = FlowScheduler::new(engine.clone(), queue.clone());

        engine
            .start_with_id(
                run_id,
                spec.clone(),
                json!({
                    "message": "review local durable worker setup",
                    "resumeAt": (now - ChronoDuration::seconds(1)).to_rfc3339(),
                }),
            )
            .await?;
        let suspended = engine.snapshot(run_id).await?;
        assert_eq!(suspended.status, WorkflowRunStatus::Suspended);

        let tick = scheduler.enqueue_due_work(now).await?;
        assert_eq!(
            tick.due_waits,
            vec![(run_id.to_string(), "reminder-due".to_string())]
        );
        assert_eq!(queue.len().await?, 1);
        println!("first_process_status={:?}", suspended.status);
        println!("queued_after_scheduler={}", queue.len().await?);
    }

    let store = Arc::new(SqliteEventStore::connect(&url).await?);
    let engine = FlowEngine::new(store, Arc::new(ReminderRuntime));
    let queue = Arc::new(LocalFileFlowTaskQueue::new(&task_root));
    queue.requeue_inflight().await?;
    let worker = FlowWorker::new(engine.clone(), queue.clone());
    let outcomes = worker.run_until_idle().await?;
    let completed = engine.snapshot(run_id).await?;

    assert_eq!(outcomes.len(), 1);
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(queue.len().await?, 0);

    println!("worker_outcomes={}", outcomes.len());
    println!("final_status={:?}", completed.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&completed.output).unwrap()
    );

    let _ = tokio::fs::remove_dir_all(&root).await;
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
fn main() {
    println!("sqlite feature not enabled; run with:");
    println!("cargo run --example sqlite_worker --features sqlite");
}
