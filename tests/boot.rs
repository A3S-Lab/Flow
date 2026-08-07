#![cfg(feature = "boot")]

use std::sync::Arc;
use std::time::Duration;

use a3s_boot::{ModuleRef, Queue, QueueOptions};
#[cfg(feature = "sqlite")]
use a3s_flow::SqliteEventStore;
use a3s_flow::{
    BootFlowTaskManager, FlowEngine, FlowError, FlowRuntime, FlowScheduler, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("boot.workflow", "0.1.0", "tests::boot", "main")
}

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

        let resume_at = ctx.input()["resume_at"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing resume_at".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|error| FlowError::Runtime(format!("invalid resume_at: {error}")))?;
        Ok(ctx.wait_until("sleep", resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("sleep runtime does not schedule steps")
    }
}

#[tokio::test]
async fn boot_task_manager_processes_scheduler_work_through_boot_lifecycle() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let queue = Arc::new(Queue::in_process_with_options(
        "flow-tests",
        QueueOptions::new()
            .with_poll_interval(Duration::from_millis(5))
            .with_lease_duration(Duration::from_secs(1)),
    ));
    let manager = Arc::new(BootFlowTaskManager::new(engine.clone(), queue.clone()));
    manager.register().unwrap();
    queue.start(ModuleRef::new()).await.unwrap();

    let scheduler = FlowScheduler::new(engine.clone(), manager.clone());
    let tick = scheduler.enqueue_due_work(now).await.unwrap();
    assert_eq!(tick.enqueued_tasks, 1);
    assert_eq!(tick.due_waits, vec![(run_id.clone(), "sleep".to_string())]);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if engine.snapshot(&run_id).await.unwrap().status == WorkflowRunStatus::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Boot queue should process the Flow task");
    queue.shutdown().await.unwrap();

    let stats = queue.stats().unwrap();
    assert_eq!(stats.completed, 1);
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.active, 0);
}

#[tokio::test]
async fn boot_task_manager_records_invalid_flow_payload_as_failed_job() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let queue = Arc::new(Queue::in_process_with_options(
        "flow-invalid-task-tests",
        QueueOptions::new().with_poll_interval(Duration::from_millis(5)),
    ));
    let manager = BootFlowTaskManager::new(engine, queue.clone());
    manager.register().unwrap();
    queue.start(ModuleRef::new()).await.unwrap();

    queue
        .enqueue_value(manager.job_name(), json!({ "not": "a flow task" }))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if queue.stats().unwrap().failed == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Boot queue should retain the failed task");
    queue.shutdown().await.unwrap();

    let failures = queue.failures().unwrap();
    assert_eq!(failures.len(), 1);
    assert!(failures[0].message.contains("invalid queued job data"));
}

#[test]
fn boot_task_manager_rejects_an_empty_job_name() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let queue = Arc::new(Queue::in_process("flow-tests"));
    let result = BootFlowTaskManager::new(engine, queue).with_job_name("  ");
    let error = match result {
        Ok(_) => panic!("empty Boot job name should fail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        FlowError::InvalidWorkerConfiguration(message) if message.contains("job name")
    ));
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn boot_task_manager_drives_an_orm_backed_engine_across_restart() {
    let directory = tempfile::tempdir().unwrap();
    let database_url = format!("sqlite://{}", directory.path().join("flow.db").display());
    let now = Utc::now();

    let run_id = {
        let store = Arc::new(SqliteEventStore::connect(&database_url).await.unwrap());
        let engine = FlowEngine::new(store, Arc::new(SleepRuntime));
        let run_id = engine
            .start(
                spec(),
                json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
            )
            .await
            .unwrap();
        assert_eq!(
            engine.snapshot(&run_id).await.unwrap().status,
            WorkflowRunStatus::Suspended
        );
        run_id
    };

    let store = Arc::new(SqliteEventStore::connect(&database_url).await.unwrap());
    let restarted = FlowEngine::new(store, Arc::new(SleepRuntime));
    assert_eq!(
        restarted.snapshot(&run_id).await.unwrap().status,
        WorkflowRunStatus::Suspended
    );

    let queue = Arc::new(Queue::in_process_with_options(
        "flow-orm-tests",
        QueueOptions::new().with_poll_interval(Duration::from_millis(5)),
    ));
    let manager = Arc::new(BootFlowTaskManager::new(restarted.clone(), queue.clone()));
    manager.register().unwrap();
    queue.start(ModuleRef::new()).await.unwrap();

    let tick = FlowScheduler::new(restarted.clone(), manager)
        .enqueue_due_work(now)
        .await
        .unwrap();
    assert_eq!(tick.enqueued_tasks, 1);
    assert_eq!(tick.due_waits, vec![(run_id.clone(), "sleep".to_string())]);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if restarted.snapshot(&run_id).await.unwrap().status == WorkflowRunStatus::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Boot should finish the ORM-backed Flow task after restart");
    queue.shutdown().await.unwrap();
    drop(restarted);

    let store = Arc::new(SqliteEventStore::connect(&database_url).await.unwrap());
    let verified = FlowEngine::new(store, Arc::new(SleepRuntime));
    assert_eq!(
        verified.snapshot(&run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}
