use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowScheduler, FlowWorker, InMemoryFlowTaskQueue,
    RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::Arc;

struct ExportRuntime;

#[async_trait]
impl FlowRuntime for ExportRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.wait_completed("export-window") {
            return Ok(ctx.complete(json!({ "exported": true })));
        }

        let resume_at = ctx.input()["resumeAt"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing resumeAt".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|err| FlowError::Runtime(format!("invalid resumeAt: {err}")))?;

        Ok(ctx.wait_until("export-window", resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("export runtime does not schedule steps")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(ExportRuntime));
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
    let worker = FlowWorker::new(engine.clone(), queue);
    let spec = WorkflowSpec::rust_embedded("examples.cancellation", "0.1.0", "examples", "main");

    let run_id = engine
        .start_with_id(
            "export-2026-0001",
            spec,
            json!({ "resumeAt": (now + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await?;
    let suspended = engine.snapshot(&run_id).await?;
    println!("before_cancel={:?}", suspended.status);

    engine
        .cancel(&run_id, Some("user requested cancellation".to_string()))
        .await?;

    let cancelled = engine.snapshot(&run_id).await?;
    let tick = scheduler
        .enqueue_due_work(now + ChronoDuration::hours(2))
        .await?;
    let outcomes = worker.run_until_idle().await?;

    println!("after_cancel={:?}", cancelled.status);
    println!("reason={}", cancelled.error.as_deref().unwrap_or(""));
    println!("due_waits_after_cancel={}", tick.due_waits.len());
    println!("worker_outcomes={}", outcomes.len());

    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert!(tick.due_waits.is_empty());
    assert!(outcomes.is_empty());
    Ok(())
}
