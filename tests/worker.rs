#[cfg(feature = "postgres")]
use a3s_flow::PostgresFlowTaskQueue;
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowTask, FlowTaskQueue, FlowWorker, HookStatus,
    InMemoryFlowTaskQueue, LocalFileFlowTaskQueue, RetryPolicy, RuntimeCommand, StepInvocation,
    WaitStatus, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
#[cfg(feature = "postgres")]
use uuid::Uuid;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("worker.workflow", "0.1.0", "tests::worker", "main")
}

#[cfg(feature = "postgres")]
fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn completed_wait(invocation: &WorkflowInvocation, wait_id: &str) -> bool {
    invocation.history.iter().any(|event| {
        matches!(
            &event.event,
            a3s_flow::FlowEvent::WaitCompleted { wait_id: id } if id == wait_id
        )
    })
}

fn received_hook(invocation: &WorkflowInvocation, hook_id: &str) -> Option<serde_json::Value> {
    invocation
        .history
        .iter()
        .find_map(|event| match &event.event {
            a3s_flow::FlowEvent::HookReceived {
                hook_id: id,
                payload,
            } if id == hook_id => Some(payload.clone()),
            _ => None,
        })
}

fn disposed_hook(invocation: &WorkflowInvocation, hook_id: &str) -> bool {
    invocation.history.iter().any(|event| {
        matches!(
            &event.event,
            a3s_flow::FlowEvent::HookDisposed { hook_id: id } if id == hook_id
        )
    })
}

struct SleepRuntime;

#[async_trait]
impl FlowRuntime for SleepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if completed_wait(&invocation, "sleep") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "slept": true }),
            });
        }

        let resume_at = invocation.input["resume_at"]
            .as_str()
            .ok_or_else(|| FlowError::Runtime("missing resume_at".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|err| FlowError::Runtime(format!("invalid resume_at: {err}")))?;

        Ok(RuntimeCommand::WaitUntil {
            wait_id: "sleep".to_string(),
            resume_at,
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("sleep runtime does not schedule steps")
    }
}

struct HookRuntime;

#[async_trait]
impl FlowRuntime for HookRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if let Some(payload) = received_hook(&invocation, "approval") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "approved": payload["approved"] }),
            });
        }
        if disposed_hook(&invocation, "approval") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "status": "disposed" }),
            });
        }

        Ok(RuntimeCommand::CreateHook {
            hook_id: "approval".to_string(),
            token: invocation.input["token"]
                .as_str()
                .unwrap_or("approval-token")
                .to_string(),
            metadata: json!({ "kind": "approval" }),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("hook runtime does not schedule steps")
    }
}

struct DropCounter(Arc<AtomicUsize>);

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct BlockingAfterWaitRuntime {
    started: Notify,
    dropped: Arc<AtomicUsize>,
}

#[async_trait]
impl FlowRuntime for BlockingAfterWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.wait_completed("blocked") {
            let _drop_counter = DropCounter(self.dropped.clone());
            self.started.notify_one();
            std::future::pending::<()>().await;
            unreachable!("blocking runtime only completes when its future is dropped")
        }

        Ok(ctx.wait_until("blocked", Utc::now() + ChronoDuration::hours(1)))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("blocking runtime does not schedule steps")
    }
}

#[derive(Default)]
struct DelayedRetryRuntime {
    attempts: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for DelayedRetryRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(output) = ctx.step_output("flaky") {
            return Ok(ctx.complete(output.clone()));
        }

        Ok(ctx.schedule_step_with_retry(
            "flaky",
            "flakyStep",
            json!({}),
            RetryPolicy::fixed(2, Duration::from_secs(60)),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            Err(FlowError::Runtime("first attempt failed".to_string()))
        } else {
            Ok(json!({ "attempt": attempt + 1 }))
        }
    }
}

#[tokio::test]
async fn worker_resumes_due_waits_from_queue() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let due_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let future_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let worker = FlowWorker::new(engine.clone(), queue.clone());
    queue
        .enqueue(FlowTask::ResumeDueWaits { now })
        .await
        .unwrap();

    let outcome = worker.run_once().await.unwrap().unwrap();
    assert_eq!(
        outcome.resumed_waits,
        vec![(due_run_id.clone(), "sleep".to_string())]
    );
    assert_eq!(outcome.run_ids, vec![due_run_id.clone()]);
    assert!(queue.is_empty().await.unwrap());

    let due = engine.snapshot(&due_run_id).await.unwrap();
    assert_eq!(due.status, WorkflowRunStatus::Completed);
    assert_eq!(due.waits["sleep"].status, WaitStatus::Completed);

    let future = engine.snapshot(&future_run_id).await.unwrap();
    assert_eq!(future.status, WorkflowRunStatus::Suspended);
    assert_eq!(future.waits["sleep"].status, WaitStatus::Waiting);
}

#[tokio::test]
async fn worker_resumes_due_retries_from_queue() {
    let now = Utc::now();
    let runtime = Arc::new(DelayedRetryRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 1);

    let worker = FlowWorker::in_memory(engine.clone());
    worker
        .enqueue(FlowTask::ResumeDueRetries {
            now: now + ChronoDuration::seconds(120),
        })
        .await
        .unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_retries,
        vec![(run_id.clone(), "flaky".to_string())]
    );

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output.unwrap()["attempt"], 2);
}

#[tokio::test]
async fn worker_resumes_hook_by_token_from_queue() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    let run_id = engine
        .start(spec(), json!({ "token": "approval-token" }))
        .await
        .unwrap();
    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.hooks["approval"].status, HookStatus::Active);

    let worker = FlowWorker::in_memory(engine.clone());
    worker
        .enqueue(FlowTask::ResumeHookByToken {
            token: "approval-token".to_string(),
            payload: json!({ "approved": true }),
        })
        .await
        .unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_hook,
        Some((run_id.clone(), "approval".to_string()))
    );
    assert_eq!(outcomes[0].run_ids, vec![run_id.clone()]);

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.output.unwrap()["approved"], true);
}

#[tokio::test]
async fn worker_disposes_hook_by_token_from_queue() {
    let engine = FlowEngine::in_memory(Arc::new(HookRuntime));
    let run_id = engine
        .start(spec(), json!({ "token": "approval-token" }))
        .await
        .unwrap();
    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.hooks["approval"].status, HookStatus::Active);

    let worker = FlowWorker::in_memory(engine.clone());
    worker
        .enqueue(FlowTask::DisposeHookByToken {
            token: "approval-token".to_string(),
        })
        .await
        .unwrap();

    let outcomes = worker.run_until_idle().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].disposed_hook,
        Some((run_id.clone(), "approval".to_string()))
    );
    assert_eq!(outcomes[0].run_ids, vec![run_id.clone()]);

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.hooks["approval"].status, HookStatus::Disposed);
    assert_eq!(completed.output.unwrap()["status"], "disposed");
}

#[tokio::test]
async fn in_memory_task_queue_is_fifo() {
    let queue = InMemoryFlowTaskQueue::new();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "first".to_string(),
        })
        .await
        .unwrap();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "second".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(queue.len().await.unwrap(), 2);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "first".to_string()
        })
    );
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "second".to_string()
        })
    );
    assert_eq!(queue.dequeue().await.unwrap(), None);
}

#[tokio::test]
async fn in_memory_task_queue_rotates_heartbeat_fence_and_rejects_stale_ack() {
    let queue = InMemoryFlowTaskQueue::new();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "fenced".to_string(),
        })
        .await
        .unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    let renewed_lease_id = queue.heartbeat(&lease.lease_id).await.unwrap();
    assert_ne!(renewed_lease_id, lease.lease_id);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    let err = queue.ack(&lease.lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == lease.lease_id));
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    queue.ack(&renewed_lease_id).await.unwrap();
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    let err = queue.ack(&renewed_lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == renewed_lease_id));
}

#[tokio::test]
async fn local_file_task_queue_persists_pending_tasks_across_instances() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "first".to_string(),
        })
        .await
        .unwrap();
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "second".to_string(),
        })
        .await
        .unwrap();

    let queue = LocalFileFlowTaskQueue::new(dir.path());
    assert_eq!(queue.len().await.unwrap(), 2);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "first".to_string()
        })
    );
    assert_eq!(queue.len().await.unwrap(), 1);

    let queue = LocalFileFlowTaskQueue::new(dir.path());
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "second".to_string()
        })
    );
    assert_eq!(queue.dequeue().await.unwrap(), None);
}

#[tokio::test]
async fn local_file_task_queue_leases_and_acks_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "leased".to_string(),
        })
        .await
        .unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(
        lease.task,
        FlowTask::DriveRun {
            run_id: "leased".to_string()
        }
    );
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    queue.ack(&lease.lease_id).await.unwrap();
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.dequeue().await.unwrap(), None);
}

#[tokio::test]
async fn local_file_task_queue_requeues_unacked_inflight_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let queue = Arc::new(LocalFileFlowTaskQueue::new(dir.path()));
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "missing-run".to_string(),
        })
        .await
        .unwrap();

    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let worker = FlowWorker::new(engine, queue.clone());
    let err = worker.run_once().await.unwrap_err();
    assert!(matches!(err, FlowError::RunNotFound(run_id) if run_id == "missing-run"));
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    let queue = LocalFileFlowTaskQueue::new(dir.path());
    assert_eq!(queue.requeue_inflight().await.unwrap(), 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "missing-run".to_string()
        })
    );
}

#[tokio::test]
async fn local_file_task_queue_requeues_expired_inflight_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "expired-run".to_string(),
        })
        .await
        .unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    assert_eq!(
        queue
            .requeue_inflight_older_than(Utc::now() - ChronoDuration::seconds(1))
            .await
            .unwrap(),
        0
    );
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    assert_eq!(
        queue
            .requeue_inflight_older_than(Utc::now() + ChronoDuration::seconds(1))
            .await
            .unwrap(),
        1
    );
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::DriveRun {
            run_id: "expired-run".to_string()
        })
    );
    assert_eq!(queue.dead_letter_len().await.unwrap(), 0);

    let err = queue.ack(&lease.lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == lease.lease_id));
}

#[tokio::test]
async fn local_file_task_queue_heartbeat_refreshes_age_and_fences_old_token() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    queue
        .enqueue(FlowTask::DriveRun {
            run_id: "heartbeat".to_string(),
        })
        .await
        .unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    let cutoff = Utc::now();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let renewed_lease_id = queue.heartbeat(&lease.lease_id).await.unwrap();
    assert_ne!(renewed_lease_id, lease.lease_id);

    assert_eq!(queue.requeue_inflight_older_than(cutoff).await.unwrap(), 0);
    let err = queue.ack(&lease.lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == lease.lease_id));
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    queue.ack(&renewed_lease_id).await.unwrap();
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
}

#[tokio::test]
async fn worker_drops_task_future_after_heartbeat_detects_lease_loss() {
    let runtime = Arc::new(BlockingAfterWaitRuntime {
        started: Notify::new(),
        dropped: Arc::new(AtomicUsize::new(0)),
    });
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let queue = Arc::new(LocalFileFlowTaskQueue::new(dir.path()));
    queue
        .enqueue(FlowTask::ResumeWait {
            run_id,
            wait_id: "blocked".to_string(),
        })
        .await
        .unwrap();
    let worker = FlowWorker::new(engine, queue.clone())
        .with_heartbeat_interval(Duration::from_millis(10))
        .unwrap();

    let worker_task = tokio::spawn(async move { worker.run_once().await });
    runtime.started.notified().await;
    assert_eq!(queue.requeue_inflight().await.unwrap(), 1);

    let result = tokio::time::timeout(Duration::from_secs(1), worker_task)
        .await
        .expect("worker should observe lease loss")
        .expect("worker task should not panic");
    let err = result.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(_)));
    assert_eq!(runtime.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 1);
}

#[tokio::test]
async fn local_file_task_queue_dead_letters_expired_inflight_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let queue = LocalFileFlowTaskQueue::new(dir.path());
    let task = FlowTask::DriveRun {
        run_id: "poison-run".to_string(),
    };
    queue.enqueue(task.clone()).await.unwrap();

    let lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(
        queue
            .dead_letter_inflight_older_than(
                Utc::now() - ChronoDuration::seconds(1),
                "lease still fresh",
            )
            .await
            .unwrap(),
        0
    );
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    assert_eq!(
        queue
            .dead_letter_inflight_older_than(
                Utc::now() + ChronoDuration::seconds(1),
                "lease expired after worker failure",
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.dead_letter_len().await.unwrap(), 1);

    let dead = queue.dead_lettered_tasks().await.unwrap();
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].lease_id, lease.lease_id);
    assert_eq!(dead[0].task, task);
    assert_eq!(dead[0].reason, "lease expired after worker failure");
}

#[tokio::test]
async fn local_file_task_queue_drives_worker_after_restart() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();

    {
        let queue = LocalFileFlowTaskQueue::new(dir.path());
        queue
            .enqueue(FlowTask::ResumeDueWaits { now })
            .await
            .unwrap();
        assert_eq!(queue.len().await.unwrap(), 1);
    }

    let queue = Arc::new(LocalFileFlowTaskQueue::new(dir.path()));
    let worker = FlowWorker::new(engine.clone(), queue.clone());
    let outcomes = worker.run_until_idle().await.unwrap();

    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_waits,
        vec![(run_id.clone(), "sleep".to_string())]
    );
    assert!(queue.is_empty().await.unwrap());

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_task_queue_leases_requeues_and_dead_letters_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres queue integration test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let queue_name = format!("test-queue-{}", Uuid::new_v4());
    let queue = PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name)
        .await
        .unwrap();
    let task = FlowTask::DriveRun {
        run_id: "postgres-poison-run".to_string(),
    };

    queue.enqueue(task.clone()).await.unwrap();
    assert_eq!(queue.queue_name(), queue_name);
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);

    let first_lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(first_lease.task, task);
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    assert_eq!(
        queue
            .requeue_inflight_older_than(Utc::now() - ChronoDuration::seconds(1))
            .await
            .unwrap(),
        0
    );
    assert_eq!(queue.inflight_len().await.unwrap(), 1);

    assert_eq!(
        queue
            .requeue_inflight_older_than(Utc::now() + ChronoDuration::seconds(1))
            .await
            .unwrap(),
        1
    );
    assert_eq!(queue.len().await.unwrap(), 1);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);

    let second_lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(second_lease.task, task);
    let second_lease_id = queue.heartbeat(&second_lease.lease_id).await.unwrap();
    assert_ne!(second_lease_id, second_lease.lease_id);

    let err = queue.ack(&first_lease.lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == first_lease.lease_id));
    assert_eq!(queue.inflight_len().await.unwrap(), 1);
    assert_eq!(
        queue
            .dead_letter_inflight_older_than(
                Utc::now() + ChronoDuration::seconds(1),
                "lease expired after worker failure",
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);
    assert_eq!(queue.dead_letter_len().await.unwrap(), 1);

    let dead = queue.dead_lettered_tasks().await.unwrap();
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].lease_id, second_lease_id);
    assert_eq!(dead[0].task, task);
    assert_eq!(dead[0].reason, "lease expired after worker failure");

    let err = queue.ack(&second_lease_id).await.unwrap_err();
    assert!(matches!(err, FlowError::LeaseLost(lease_id) if lease_id == second_lease_id));
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_task_queue_competing_workers_lease_distinct_tasks_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres competing-worker test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let queue_name = format!("test-competing-workers-{}", Uuid::new_v4());
    let first_queue = PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name)
        .await
        .unwrap();
    let second_queue = PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name)
        .await
        .unwrap();
    first_queue
        .enqueue(FlowTask::DriveRun {
            run_id: "first".to_string(),
        })
        .await
        .unwrap();
    first_queue
        .enqueue(FlowTask::DriveRun {
            run_id: "second".to_string(),
        })
        .await
        .unwrap();

    let (first, second) = tokio::join!(first_queue.lease(), second_queue.lease());
    let first = first.unwrap().unwrap();
    let second = second.unwrap().unwrap();
    assert_ne!(first.lease_id, second.lease_id);
    assert_ne!(first.task, second.task);
    assert_eq!(first_queue.inflight_len().await.unwrap(), 2);

    first_queue.ack(&first.lease_id).await.unwrap();
    second_queue.ack(&second.lease_id).await.unwrap();
    assert_eq!(first_queue.inflight_len().await.unwrap(), 0);
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_task_queue_drives_worker_when_url_is_configured() {
    let Some(url) = postgres_url_from_env() else {
        eprintln!("skipping postgres queue worker integration test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let now = Utc::now();
    let queue_name = format!("test-worker-{}", Uuid::new_v4());
    let queue = Arc::new(
        PostgresFlowTaskQueue::connect_with_queue(&url, &queue_name)
            .await
            .unwrap(),
    );
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    queue
        .enqueue(FlowTask::ResumeDueWaits { now })
        .await
        .unwrap();
    let worker = FlowWorker::new(engine.clone(), queue.clone());
    let outcomes = worker.run_until_idle().await.unwrap();

    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_waits,
        vec![(run_id.clone(), "sleep".to_string())]
    );
    assert_eq!(queue.len().await.unwrap(), 0);
    assert_eq!(queue.inflight_len().await.unwrap(), 0);

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
}

#[test]
fn flow_task_serializes_for_external_queues() {
    let task = FlowTask::ResumeHookByToken {
        token: "approval-token".to_string(),
        payload: json!({ "approved": true }),
    };

    let encoded = serde_json::to_string(&task).unwrap();
    assert_eq!(
        encoded,
        r#"{"type":"resume_hook_by_token","token":"approval-token","payload":{"approved":true}}"#
    );

    let decoded: FlowTask = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, task);

    let task = FlowTask::DisposeHookByToken {
        token: "approval-token".to_string(),
    };

    let encoded = serde_json::to_string(&task).unwrap();
    assert_eq!(
        encoded,
        r#"{"type":"dispose_hook_by_token","token":"approval-token"}"#
    );

    let decoded: FlowTask = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, task);
}
