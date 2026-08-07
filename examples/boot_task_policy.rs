#[cfg(feature = "boot")]
use a3s_boot::{ModuleRef, Queue, QueueOptions, QueueRetryPolicy};
#[cfg(feature = "boot")]
use a3s_flow::{
    BootFlowTaskDeduplication, BootFlowTaskManager, BootFlowTaskPolicy, FlowEngine, FlowError,
    FlowRuntime, FlowScheduler, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus, WorkflowSpec,
};
#[cfg(feature = "boot")]
use async_trait::async_trait;
#[cfg(feature = "boot")]
use chrono::{DateTime, Duration as ChronoDuration, Utc};
#[cfg(feature = "boot")]
use serde_json::json;
#[cfg(feature = "boot")]
use std::sync::Arc;
#[cfg(feature = "boot")]
use std::time::Duration;

#[cfg(feature = "boot")]
struct ReminderRuntime;

#[cfg(feature = "boot")]
#[async_trait]
impl FlowRuntime for ReminderRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.wait_completed("reminder") {
            return Ok(context.complete(json!({ "delivered": true })));
        }
        let resume_at = context.input()["resume_at"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing resume_at".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|error| FlowError::Runtime(format!("invalid resume_at: {error}")))?;
        Ok(context.wait_until("reminder", resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("Boot task policy example does not schedule steps")
    }
}

#[cfg(feature = "boot")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(ReminderRuntime));
    let run_id = engine
        .start_with_id(
            "boot-policy-reminder",
            WorkflowSpec::rust_embedded("examples.boot-task-policy", "0.1.0", "examples", "main"),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await?;

    let queue = Arc::new(Queue::in_process_with_options(
        "flow",
        QueueOptions::new().with_poll_interval(Duration::from_millis(5)),
    ));
    let policy = BootFlowTaskPolicy::new()
        .with_retry_policy(QueueRetryPolicy::fixed(2, Duration::from_millis(50)))
        .with_timeout(Duration::from_secs(30))
        .with_max_stalled_count(2)
        .remove_on_complete(true)
        .with_deduplication(BootFlowTaskDeduplication::UntilTerminalOrTtl(
            Duration::from_secs(300),
        ));
    let manager =
        Arc::new(BootFlowTaskManager::new(engine.clone(), queue.clone()).with_task_policy(policy)?);
    manager.register()?;

    let scheduler = FlowScheduler::new(engine.clone(), manager);
    scheduler.enqueue_due_work(now).await?;
    scheduler
        .enqueue_due_work(now + ChronoDuration::milliseconds(1))
        .await?;
    assert_eq!(queue.stats().map_err(boot_error)?.pending, 1);
    println!("pending_after_duplicate_scans=1");

    queue.start(ModuleRef::new()).await.map_err(boot_error)?;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if engine.snapshot(&run_id).await?.status == WorkflowRunStatus::Completed {
                return Ok::<(), a3s_flow::FlowError>(());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| FlowError::TaskManagement("Boot job did not finish in time".to_string()))??;
    queue.shutdown().await.map_err(boot_error)?;

    assert!(queue.jobs().map_err(boot_error)?.is_empty());
    println!("status={:?}", engine.snapshot(&run_id).await?.status);
    println!("completed_job_removed=true");
    Ok(())
}

#[cfg(feature = "boot")]
fn boot_error(error: a3s_boot::BootError) -> FlowError {
    FlowError::TaskManagement(format!("A3S Boot queue error: {error}"))
}

#[cfg(not(feature = "boot"))]
fn main() {
    println!("boot feature not enabled; run with:");
    println!("cargo run --example boot_task_policy --features boot");
}
