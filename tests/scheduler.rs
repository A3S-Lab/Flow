use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, FlowScheduler, FlowTask, FlowTaskQueue, FlowWorker,
    InMemoryFlowTaskQueue, RetryPolicy, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("scheduler.workflow", "0.1.0", "tests::scheduler", "main")
}

fn completed_wait(invocation: &WorkflowInvocation, wait_id: &str) -> bool {
    invocation.history.iter().any(|event| {
        matches!(
            &event.event,
            a3s_flow::FlowEvent::WaitCompleted { wait_id: id } if id == wait_id
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

#[derive(Default)]
struct DelayedBatchRetryRuntime {
    alpha_attempts: AtomicUsize,
    beta_attempts: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for DelayedBatchRetryRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.step_output("alpha").is_some() && ctx.step_output("beta").is_some() {
            return Ok(ctx.complete(json!({ "done": true })));
        }

        let retry = RetryPolicy::fixed(2, Duration::from_secs(60));
        Ok(ctx.schedule_steps(vec![
            ctx.step_with_retry("alpha", "batchStep", json!({}), retry),
            ctx.step_with_retry("beta", "batchStep", json!({}), retry),
        ]))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        let attempts = match invocation.step_id.as_str() {
            "alpha" => &self.alpha_attempts,
            "beta" => &self.beta_attempts,
            other => return Err(FlowError::Runtime(format!("unknown batch step {other}"))),
        };
        let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if attempt == 1 {
            Err(FlowError::Runtime(format!(
                "{} failed once",
                invocation.step_id
            )))
        } else {
            Ok(json!({ "attempt": attempt }))
        }
    }
}

#[tokio::test]
async fn scheduler_enqueues_due_wait_work_only_when_due() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let due_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    engine
        .start(
            spec(),
            json!({ "resume_at": (now + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine, queue.clone());
    let tick = scheduler.enqueue_due_work(now).await.unwrap();

    assert_eq!(
        tick.due_waits,
        vec![(due_run_id.clone(), "sleep".to_string())]
    );
    assert!(tick.due_retries.is_empty());
    assert_eq!(tick.enqueued_tasks, 1);
    assert_eq!(
        queue.dequeue().await.unwrap(),
        Some(FlowTask::ResumeScheduledRun {
            run_id: due_run_id,
            now,
        })
    );
    assert_eq!(
        scheduler
            .enqueue_due_work(now - ChronoDuration::hours(1))
            .await
            .unwrap()
            .enqueued_tasks,
        0
    );
}

#[tokio::test]
async fn scheduler_reports_next_wakeup_delay() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let future_run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now + ChronoDuration::hours(1)).to_rfc3339() }),
        )
        .await
        .unwrap();
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine, queue);

    let wakeup = scheduler.next_wakeup(now).await.unwrap().unwrap();
    assert_eq!(wakeup.run_id(), future_run_id);
    assert_eq!(wakeup.subject_id(), "sleep");
    assert_eq!(
        wakeup.scheduled_at().unwrap(),
        now + ChronoDuration::hours(1)
    );
    assert_eq!(
        scheduler.next_wakeup_delay(now).await.unwrap(),
        Some(Duration::from_secs(60 * 60))
    );
    assert_eq!(
        scheduler
            .next_wakeup_delay(now + ChronoDuration::hours(2))
            .await
            .unwrap(),
        Some(Duration::ZERO)
    );
}

#[tokio::test]
async fn scheduler_reports_no_wakeup_without_timers_or_retries() {
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine, queue);

    assert!(scheduler.next_wakeup(Utc::now()).await.unwrap().is_none());
    assert!(scheduler
        .next_wakeup_delay(Utc::now())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn scheduler_skips_cancelled_due_waits() {
    let now = Utc::now();
    let engine = FlowEngine::in_memory(Arc::new(SleepRuntime));
    let run_id = engine
        .start(
            spec(),
            json!({ "resume_at": (now - ChronoDuration::seconds(1)).to_rfc3339() }),
        )
        .await
        .unwrap();

    engine
        .cancel(&run_id, Some("operator cancelled".to_string()))
        .await
        .unwrap();

    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
    let tick = scheduler.enqueue_due_work(now).await.unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Cancelled);
    assert_eq!(snapshot.error.as_deref(), Some("operator cancelled"));
    assert!(tick.due_waits.is_empty());
    assert!(tick.due_retries.is_empty());
    assert_eq!(tick.enqueued_tasks, 0);
    assert_eq!(queue.len().await.unwrap(), 0);
}

#[tokio::test]
async fn scheduler_enqueues_due_retry_and_worker_drains_it() {
    let now = Utc::now();
    let runtime = Arc::new(DelayedRetryRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();

    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
    let tick = scheduler
        .enqueue_due_work(now + ChronoDuration::seconds(120))
        .await
        .unwrap();

    assert!(tick.due_waits.is_empty());
    assert_eq!(
        tick.due_retries,
        vec![(run_id.clone(), "flaky".to_string())]
    );
    assert_eq!(tick.enqueued_tasks, 1);

    let worker = FlowWorker::new(engine.clone(), queue);
    let outcomes = worker.run_until_idle().await.unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].resumed_retries,
        vec![(run_id.clone(), "flaky".to_string())]
    );

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
}

#[tokio::test]
async fn scheduler_groups_due_retry_siblings_into_one_run_task() {
    let now = Utc::now();
    let runtime = Arc::new(DelayedBatchRetryRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();

    let queue = Arc::new(InMemoryFlowTaskQueue::new());
    let scheduler = FlowScheduler::new(engine.clone(), queue.clone());
    let dispatch_at = now + ChronoDuration::seconds(120);
    let tick = scheduler.enqueue_due_work(dispatch_at).await.unwrap();

    assert_eq!(
        tick.due_retries,
        vec![
            (run_id.clone(), "alpha".to_string()),
            (run_id.clone(), "beta".to_string()),
        ]
    );
    assert_eq!(tick.enqueued_tasks, 1);
    let task = queue.dequeue().await.unwrap();
    assert_eq!(
        task,
        Some(FlowTask::ResumeScheduledRun {
            run_id: run_id.clone(),
            now: dispatch_at,
        })
    );

    let outcome = FlowWorker::in_memory(engine.clone())
        .handle(task.unwrap())
        .await
        .unwrap();
    assert_eq!(
        outcome.resumed_retries,
        vec![
            (run_id.clone(), "alpha".to_string()),
            (run_id.clone(), "beta".to_string()),
        ]
    );
    assert_eq!(runtime.alpha_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(runtime.beta_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        engine.snapshot(&run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
}
