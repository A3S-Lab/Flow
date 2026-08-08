use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowTask, FlowTaskQueue, FlowWorker,
    LocalFileFlowTaskQueue, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::Arc;

struct SleepRuntime;

#[async_trait]
impl FlowRuntime for SleepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.wait_completed("sleep") {
            return Ok(ctx.complete(json!({ "slept": true })));
        }

        let resume_at = ctx.input()["resumeAt"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing resumeAt".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|err| FlowError::Runtime(format!("invalid resumeAt: {err}")))?;

        Ok(ctx.wait_until("sleep", resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("sleep runtime does not schedule steps")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let now = Utc::now();
    let queue_root = std::env::temp_dir().join(format!(
        "a3s-flow-task-queue-example-{}",
        std::process::id()
    ));
    let _ = tokio::fs::remove_dir_all(&queue_root).await;

    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let spec = WorkflowSpec::rust_embedded("examples.task-queue", "0.1.0", "examples", "main");
    let run_id = engine
        .start(
            spec,
            json!({ "resumeAt": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await?;

    {
        let queue = LocalFileFlowTaskQueue::new(&queue_root);
        queue
            .enqueue(FlowTask::ResumeScheduledRun {
                run_id: run_id.clone(),
                now,
            })
            .await?;
        println!("pending_before_restart={}", queue.len().await?);
    }

    let restarted_queue = LocalFileFlowTaskQueue::new(&queue_root);
    let leased = restarted_queue
        .lease()
        .await?
        .expect("pending task should survive queue reconstruction");
    println!("leased_task={:?}", leased.task);
    println!(
        "inflight_after_crash={}",
        restarted_queue.inflight_len().await?
    );
    drop(leased);

    let recovered_queue = Arc::new(LocalFileFlowTaskQueue::new(&queue_root));
    let requeued = recovered_queue.requeue_inflight().await?;
    let worker = FlowWorker::new(engine.clone(), recovered_queue.clone())
        .with_heartbeat_interval(std::time::Duration::from_secs(30))?;
    let outcomes = worker.run_until_idle().await?;
    let snapshot = engine.snapshot(&run_id).await?;

    recovered_queue
        .enqueue(FlowTask::DriveRun {
            run_id: "poison-run".to_string(),
        })
        .await?;
    let stale = recovered_queue
        .lease()
        .await?
        .expect("poison task should be leased");
    let dead_lettered = recovered_queue
        .dead_letter_inflight_older_than(
            Utc::now() + ChronoDuration::seconds(1),
            "example poison task",
        )
        .await?;

    println!("requeued_inflight={requeued}");
    println!("worker_outcomes={}", outcomes.len());
    println!("status={:?}", snapshot.status);
    println!("pending_after_worker={}", recovered_queue.len().await?);
    println!("dead_lettered={dead_lettered}");
    println!("dead_letter_lease={}", stale.lease_id);

    let _ = tokio::fs::remove_dir_all(&queue_root).await;
    Ok(())
}
