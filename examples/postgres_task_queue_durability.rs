#[cfg(feature = "postgres")]
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowTask, FlowTaskQueue, FlowWorker, PostgresEventStore,
    PostgresFlowTaskQueue, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowSpec,
};
#[cfg(feature = "postgres")]
use async_trait::async_trait;
#[cfg(feature = "postgres")]
use chrono::{DateTime, Duration as ChronoDuration, Utc};
#[cfg(feature = "postgres")]
use serde_json::json;
#[cfg(feature = "postgres")]
use std::sync::Arc;
#[cfg(feature = "postgres")]
use uuid::Uuid;

#[cfg(feature = "postgres")]
struct SleepRuntime;

#[cfg(feature = "postgres")]
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

#[cfg(feature = "postgres")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let Some(url) = std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
    else {
        println!("set A3S_FLOW_POSTGRES_URL=postgres://user:pass@host:5432/db to run it");
        return Ok(());
    };

    let now = Utc::now();
    let run_id = format!("postgres-queue-{}", Uuid::new_v4());
    let queue_name = format!("examples-postgres-queue-{}", Uuid::new_v4());
    let spec =
        WorkflowSpec::rust_embedded("examples.postgres-task-queue", "0.1.0", "examples", "main");

    let store = Arc::new(PostgresEventStore::connect(&url).await?);
    let queue = Arc::new(PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name).await?);
    let engine = FlowEngine::new(store, Arc::new(SleepRuntime));

    engine
        .start_with_id(
            &run_id,
            spec,
            json!({ "resumeAt": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await?;

    queue.enqueue(FlowTask::ResumeDueWaits { now }).await?;
    println!("queue_name={queue_name}");
    println!("pending_before_lease={}", queue.len().await?);

    let lease = queue.lease().await?.expect("pending task should be leased");
    println!("leased_task={:?}", lease.task);
    println!("inflight_after_lease={}", queue.inflight_len().await?);
    drop(lease);

    let requeued = queue.requeue_inflight().await?;
    let worker = FlowWorker::new(engine.clone(), queue.clone())
        .with_heartbeat_interval(std::time::Duration::from_secs(30))?;
    let outcomes = worker.run_until_idle().await?;
    let snapshot = engine.snapshot(&run_id).await?;

    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "postgres-poison-run".to_string(),
        })
        .await?;
    let stale = queue.lease().await?.expect("poison task should be leased");
    let dead_lettered = queue
        .dead_letter_inflight_older_than(
            Utc::now() + ChronoDuration::seconds(1),
            "example poison task",
        )
        .await?;

    println!("requeued_inflight={requeued}");
    println!("worker_outcomes={}", outcomes.len());
    println!("status={:?}", snapshot.status);
    println!("pending_after_worker={}", queue.len().await?);
    println!("dead_lettered={dead_lettered}");
    println!("dead_letter_lease={}", stale.lease_id);

    Ok(())
}

#[cfg(not(feature = "postgres"))]
fn main() {
    println!("postgres feature not enabled; run with:");
    println!("cargo run --example postgres_task_queue_durability --features postgres");
}
