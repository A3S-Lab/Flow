#![cfg(feature = "boot")]

use std::sync::Arc;
use std::time::Duration;

use a3s_boot::{ModuleRef, Queue, QueueJobOptions, QueueOptions, QueueRetryPolicy};
#[cfg(feature = "sqlite")]
use a3s_flow::SqliteEventStore;
use a3s_flow::{
    BootFlowTaskDeduplication, BootFlowTaskManager, BootFlowTaskPolicy, FlowEngine, FlowError,
    FlowRuntime, FlowScheduler, FlowTask, FlowTaskDispatcher, RuntimeBuildCompatibility,
    RuntimeBuildId, RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowRunStatus,
    WorkflowSignal, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use tokio::sync::Notify;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("boot.workflow", "0.1.0", "tests::boot", "main")
}

fn build_id(value: &str) -> RuntimeBuildId {
    RuntimeBuildId::new(value).unwrap()
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

#[derive(Default)]
struct BlockingScheduledRuntime {
    started: Notify,
    release: Notify,
}

#[async_trait]
impl FlowRuntime for BlockingScheduledRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.wait_completed("sleep") {
            self.started.notify_one();
            self.release.notified().await;
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
        unreachable!("blocking scheduled runtime does not schedule steps")
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

#[tokio::test]
async fn boot_task_manager_dispatches_only_engine_compatible_runtime_builds() {
    let current = build_id("worker-v2");
    let compatible = build_id("worker-v1");
    let unsupported = build_id("worker-v3");
    let engine = FlowEngine::builder(Arc::new(SleepRuntime))
        .with_runtime_build_compatibility(
            RuntimeBuildCompatibility::new(current.clone())
                .with_compatible_build(compatible.clone()),
        )
        .build();
    let queue = Arc::new(Queue::in_process("flow-build-routing-tests"));
    let manager = BootFlowTaskManager::new(engine, queue.clone());

    manager
        .dispatch_for_runtime_build(
            Some(&compatible),
            FlowTask::DriveRun {
                run_id: "compatible-run".to_string(),
            },
        )
        .await
        .unwrap();
    assert_eq!(queue.stats().unwrap().pending, 1);

    for required in [Some(&unsupported), None] {
        let error = manager
            .dispatch_for_runtime_build(
                required,
                FlowTask::DriveRun {
                    run_id: "rejected-run".to_string(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, FlowError::RuntimeBuildRouteNotFound { .. }));
    }
    assert_eq!(queue.stats().unwrap().pending, 1);
    assert!(manager.has_runtime_build_route(Some(&current)));
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

#[test]
fn boot_task_policy_maps_typed_options_and_scan_targets() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let queue = Arc::new(Queue::in_process("flow-policy-tests"));
    let policy = BootFlowTaskPolicy::new()
        .with_retry_policy(QueueRetryPolicy::fixed(2, Duration::from_millis(25)))
        .with_timeout(Duration::from_secs(30))
        .with_max_stalled_count(4)
        .remove_on_complete(true)
        .remove_on_fail(true)
        .with_deduplication(BootFlowTaskDeduplication::UntilTerminalOrTtl(
            Duration::from_secs(60),
        ));
    let manager = BootFlowTaskManager::new(engine, queue)
        .with_task_policy(policy.clone())
        .unwrap();
    assert_eq!(manager.task_policy(), &policy);

    let first_scan = FlowTask::ResumeDueWaits { now: Utc::now() };
    let later_scan = FlowTask::ResumeDueWaits {
        now: Utc::now() + ChronoDuration::seconds(1),
    };
    let retry_scan = FlowTask::ResumeDueRetries { now: Utc::now() };
    let first_options = manager.job_options_for(&first_scan);
    let later_options = manager.job_options_for(&later_scan);
    let retry_options = manager.job_options_for(&retry_scan);
    let first_run = manager.job_options_for(&FlowTask::ResumeScheduledRun {
        run_id: "run-1".to_string(),
        now: Utc::now(),
    });
    let later_same_run = manager.job_options_for(&FlowTask::ResumeScheduledRun {
        run_id: "run-1".to_string(),
        now: Utc::now() + ChronoDuration::seconds(1),
    });
    let other_run = manager.job_options_for(&FlowTask::ResumeScheduledRun {
        run_id: "run-2".to_string(),
        now: Utc::now(),
    });

    assert_eq!(first_options.retry_policy, policy.retry_policy().clone());
    assert_eq!(first_options.timeout, Some(Duration::from_secs(30)));
    assert_eq!(first_options.max_stalled_count, 4);
    assert!(first_options.remove_on_complete);
    assert!(first_options.remove_on_fail);
    let first_deduplication = first_options.deduplication.unwrap();
    let later_deduplication = later_options.deduplication.unwrap();
    let retry_deduplication = retry_options.deduplication.unwrap();
    assert_eq!(first_deduplication.id, later_deduplication.id);
    assert_ne!(first_deduplication.id, retry_deduplication.id);
    assert_eq!(first_deduplication.ttl, Some(Duration::from_secs(60)));
    assert!(first_deduplication.keep_last_if_active);
    let first_run = first_run.deduplication.unwrap();
    let later_same_run = later_same_run.deduplication.unwrap();
    let other_run = other_run.deduplication.unwrap();
    assert_eq!(first_run.id, later_same_run.id);
    assert_ne!(first_run.id, other_run.id);
    assert!(first_run.id.starts_with("a3s-flow:resume_scheduled_run:"));
    assert!(first_run.keep_last_if_active);
}

#[test]
fn boot_task_deduplication_redacts_tokens_and_preserves_target_semantics() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let queue = Arc::new(Queue::in_process("flow-deduplication-tests"));
    let manager = BootFlowTaskManager::new(engine, queue)
        .with_task_policy(
            BootFlowTaskPolicy::new().with_deduplication(BootFlowTaskDeduplication::UntilTerminal),
        )
        .unwrap();

    let token = "public-callback-token-that-must-stay-secret";
    let first_resume = manager.job_options_for(&FlowTask::ResumeHookByToken {
        token: token.to_string(),
        payload: json!({ "decision": "approve" }),
    });
    let duplicate_resume = manager.job_options_for(&FlowTask::ResumeHookByToken {
        token: token.to_string(),
        payload: json!({ "decision": "reject" }),
    });
    let dispose = manager.job_options_for(&FlowTask::DisposeHookByToken {
        token: token.to_string(),
    });
    let drive = manager.job_options_for(&FlowTask::DriveRun {
        run_id: "run-1".to_string(),
    });
    let wait = manager.job_options_for(&FlowTask::ResumeWait {
        run_id: "run-1".to_string(),
        wait_id: "wait-1".to_string(),
    });
    let first_signal = manager.job_options_for(&FlowTask::SendSignal {
        run_id: "run-1".to_string(),
        signal: WorkflowSignal::new("delivery-1", "order.approved", json!({ "approved": true })),
    });
    let duplicate_signal = manager.job_options_for(&FlowTask::SendSignal {
        run_id: "run-1".to_string(),
        signal: WorkflowSignal::new("delivery-1", "order.approved", json!({ "approved": false })),
    });

    let first_resume = first_resume.deduplication.unwrap();
    let duplicate_resume = duplicate_resume.deduplication.unwrap();
    let dispose = dispose.deduplication.unwrap();
    let drive = drive.deduplication.unwrap();
    let wait = wait.deduplication.unwrap();
    let first_signal = first_signal.deduplication.unwrap();
    let duplicate_signal = duplicate_signal.deduplication.unwrap();
    assert_eq!(first_resume.id, duplicate_resume.id);
    assert_ne!(first_resume.id, dispose.id);
    assert!(!first_resume.id.contains(token));
    assert!(first_resume
        .id
        .starts_with("a3s-flow:resume_hook_by_token:"));
    assert!(drive.keep_last_if_active);
    assert!(!wait.keep_last_if_active);
    assert_eq!(first_signal.id, duplicate_signal.id);
    assert!(first_signal.id.starts_with("a3s-flow:send_signal:"));
    assert!(first_signal.keep_last_if_active);
}

#[test]
fn boot_task_manager_rejects_a_zero_deduplication_ttl() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let queue = Arc::new(Queue::in_process("flow-policy-tests"));
    let result = BootFlowTaskManager::new(engine, queue).with_task_policy(
        BootFlowTaskPolicy::new().with_deduplication(
            BootFlowTaskDeduplication::UntilTerminalOrTtl(Duration::ZERO),
        ),
    );
    let error = match result {
        Ok(_) => panic!("zero deduplication TTL should fail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        FlowError::InvalidWorkerConfiguration(message)
            if message.contains("deduplication TTL")
    ));
}

#[tokio::test]
async fn boot_task_manager_deduplicates_scheduled_runs_and_accepts_explicit_job_options() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let queue = Arc::new(Queue::in_process("flow-options-tests"));
    let manager = BootFlowTaskManager::new(engine, queue.clone())
        .with_task_policy(
            BootFlowTaskPolicy::new().with_deduplication(BootFlowTaskDeduplication::UntilTerminal),
        )
        .unwrap();

    let first = manager
        .enqueue_with_receipt(FlowTask::ResumeScheduledRun {
            run_id: "scheduled-run".to_string(),
            now: Utc::now(),
        })
        .await
        .unwrap();
    let duplicate = manager
        .enqueue_with_receipt(FlowTask::ResumeScheduledRun {
            run_id: "scheduled-run".to_string(),
            now: Utc::now() + ChronoDuration::seconds(1),
        })
        .await
        .unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(queue.stats().unwrap().pending, 1);

    let distinct = manager
        .enqueue_with_receipt(FlowTask::ResumeScheduledRun {
            run_id: "other-run".to_string(),
            now: Utc::now(),
        })
        .await
        .unwrap();
    assert_ne!(first.id, distinct.id);
    assert_eq!(queue.stats().unwrap().pending, 2);

    let explicit = manager
        .enqueue_with_options(
            FlowTask::DriveRun {
                run_id: "explicit-run".to_string(),
            },
            QueueJobOptions::new().with_job_id("flow-explicit-job"),
        )
        .await
        .unwrap();
    assert_eq!(explicit.id, "flow-explicit-job");
    assert_eq!(queue.stats().unwrap().pending, 3);
}

#[tokio::test]
async fn boot_task_manager_retains_the_latest_scheduled_run_while_active() {
    let now = Utc::now();
    let runtime = Arc::new(BlockingScheduledRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let queue = Arc::new(Queue::in_process_with_options(
        "flow-active-successor-tests",
        QueueOptions::new().with_poll_interval(Duration::from_millis(5)),
    ));
    let manager = BootFlowTaskManager::new(engine, queue.clone())
        .with_task_policy(
            BootFlowTaskPolicy::new().with_deduplication(BootFlowTaskDeduplication::UntilTerminal),
        )
        .unwrap();
    manager.register().unwrap();
    queue.start(ModuleRef::new()).await.unwrap();

    let first = manager
        .enqueue_with_receipt(FlowTask::ResumeScheduledRun {
            run_id: run_id.clone(),
            now,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), runtime.started.notified())
        .await
        .expect("first scheduled task should become active");
    assert_eq!(queue.stats().unwrap().active, 1);

    let successor = manager
        .enqueue_with_receipt(FlowTask::ResumeScheduledRun {
            run_id,
            now: now + ChronoDuration::seconds(1),
        })
        .await
        .unwrap();
    assert_eq!(first.id, successor.id);

    runtime.release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let stats = queue.stats().unwrap();
            if stats.completed == 2 && stats.active == 0 && stats.pending == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the retained successor should run after the active owner");
    queue.shutdown().await.unwrap();
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
