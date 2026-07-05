use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowScheduler, FlowWorker, InMemoryFlowTaskQueue,
    RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::Arc;

struct ReminderRuntime;

#[async_trait]
impl FlowRuntime for ReminderRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.wait_completed("reminder-due") {
            return Ok(ctx.complete(json!({
                "sent": true,
                "message": ctx.input()["message"],
            })));
        }

        let resume_at = ctx.input()["resumeAt"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing resumeAt".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|err| FlowError::Runtime(format!("invalid resumeAt: {err}")))?;

        Ok(ctx.wait_until("reminder-due", resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("reminder runtime does not schedule steps")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(ReminderRuntime));
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
    let worker = FlowWorker::new(engine.clone(), queue);
    let spec = WorkflowSpec::rust_embedded("examples.reminder", "0.1.0", "examples", "main");

    let run_id = engine
        .start(
            spec,
            json!({
                "message": "review the release checklist",
                "resumeAt": (now - ChronoDuration::seconds(1)).to_rfc3339(),
            }),
        )
        .await?;
    let suspended = engine.snapshot(&run_id).await?;
    println!("before_scheduler={:?}", suspended.status);

    let tick = scheduler.enqueue_due_work(now).await?;
    println!(
        "scheduler_due_waits={} enqueued_tasks={}",
        tick.due_waits.len(),
        tick.enqueued_tasks
    );

    let outcomes = worker.run_until_idle().await?;
    let completed = engine.snapshot(&run_id).await?;

    println!("worker_outcomes={}", outcomes.len());
    println!("after_worker={:?}", completed.status);
    println!(
        "output={}",
        serde_json::to_string_pretty(&completed.output).unwrap()
    );
    Ok(())
}
