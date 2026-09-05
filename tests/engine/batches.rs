use super::*;

struct BatchStepRuntime;

#[async_trait]
impl FlowRuntime for BatchStepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let user = ctx.step_output("load-user");
        let orders = ctx.step_output("load-orders");

        match (user, orders) {
            (Some(user), Some(orders)) => Ok(ctx.complete(json!({
                "user": user,
                "orders": orders,
            }))),
            _ => Ok(ctx.schedule_steps(vec![
                ctx.step(
                    "load-user",
                    "loadUser",
                    json!({ "userId": ctx.input()["userId"] }),
                ),
                ctx.step_with_retry(
                    "load-orders",
                    "loadOrders",
                    json!({ "userId": ctx.input()["userId"] }),
                    RetryPolicy::fixed(2, Duration::from_millis(0)),
                ),
            ])),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "loadUser" => Ok(json!({ "id": invocation.input["userId"], "name": "Ada" })),
            "loadOrders" => Ok(json!([{ "id": "o1" }, { "id": "o2" }])),
            other => Err(FlowError::Runtime(format!("unknown step {other}"))),
        }
    }
}

#[tokio::test]
async fn schedule_steps_fans_out_multiple_durable_steps() {
    let engine = FlowEngine::in_memory(Arc::new(BatchStepRuntime));
    let run_id = engine
        .start(spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.steps.len(), 2);
    assert_eq!(snapshot.steps["load-user"].status, StepStatus::Completed);
    assert_eq!(snapshot.steps["load-orders"].status, StepStatus::Completed);
    assert_eq!(snapshot.steps["load-orders"].retry.max_attempts, 2);
    assert_eq!(snapshot.output.unwrap()["orders"][1]["id"], "o2");
}

struct ConcurrentBatchStepRuntime {
    barrier: Barrier,
    in_flight: AtomicUsize,
    maximum_in_flight: AtomicUsize,
}

impl ConcurrentBatchStepRuntime {
    fn new(step_count: usize) -> Self {
        Self {
            barrier: Barrier::new(step_count),
            in_flight: AtomicUsize::new(0),
            maximum_in_flight: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl FlowRuntime for ConcurrentBatchStepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.step_output("alpha").is_some() && ctx.step_output("beta").is_some() {
            return Ok(ctx.complete(json!({ "done": true })));
        }
        Ok(ctx.schedule_steps(vec![
            ctx.step("alpha", "barrier", json!({ "value": "alpha" })),
            ctx.step("beta", "barrier", json!({ "value": "beta" })),
        ]))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        let in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum_in_flight
            .fetch_max(in_flight, Ordering::SeqCst);
        self.barrier.wait().await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(invocation.input)
    }
}

#[tokio::test]
async fn schedule_steps_runs_durable_siblings_concurrently() {
    let runtime = Arc::new(ConcurrentBatchStepRuntime::new(2));
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = tokio::time::timeout(Duration::from_secs(1), engine.start(spec(), json!({})))
        .await
        .expect("both batch steps must enter the runtime without waiting for a sibling")
        .unwrap();

    assert_eq!(runtime.maximum_in_flight.load(Ordering::SeqCst), 2);
    let history = engine.history(&run_id).await.unwrap();
    let second_started = history
        .iter()
        .position(|event| {
            matches!(
                &event.event,
                FlowEvent::StepStarted { step_id, .. } if step_id == "beta"
            )
        })
        .unwrap();
    let first_completed = history
        .iter()
        .position(|event| matches!(event.event, FlowEvent::StepCompleted { .. }))
        .unwrap();
    assert!(
        second_started < first_completed,
        "every sibling start must be durable before any batch completion"
    );
}

struct HangingBatchSiblingRuntime;

#[async_trait]
impl FlowRuntime for HangingBatchSiblingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        Ok(ctx.schedule_steps(vec![
            ctx.step("fast", "partialBatch", json!({})),
            ctx.step("hanging", "partialBatch", json!({})),
        ]))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        if invocation.step_id == "hanging" {
            return pending().await;
        }
        Ok(json!({ "step": invocation.step_id }))
    }
}

#[tokio::test]
async fn completed_batch_sibling_is_durable_while_another_sibling_is_running() {
    let engine = FlowEngine::in_memory(Arc::new(HangingBatchSiblingRuntime));
    let worker = {
        let engine = engine.clone();
        tokio::spawn(async move {
            engine
                .start_with_id("partial-concurrent-batch", spec(), json!({}))
                .await
        })
    };

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Ok(snapshot) = engine.snapshot("partial-concurrent-batch").await {
                if snapshot
                    .steps
                    .get("fast")
                    .is_some_and(|step| step.status == StepStatus::Completed)
                    && snapshot
                        .steps
                        .get("hanging")
                        .is_some_and(|step| step.status == StepStatus::Running)
                {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the fast sibling must commit without waiting for the hanging sibling");
    worker.abort();
    let _ = worker.await;

    let snapshot = engine.snapshot("partial-concurrent-batch").await.unwrap();
    assert_eq!(snapshot.steps["fast"].status, StepStatus::Completed);
    assert_eq!(snapshot.steps["hanging"].status, StepStatus::Running);
    let history = engine.history("partial-concurrent-batch").await.unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepStarted { .. }))
            .count(),
        2
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepCompleted { .. }))
            .count(),
        1
    );
}

struct FailingBatchWithSlowSiblingRuntime {
    rendezvous: Arc<Barrier>,
}

#[async_trait]
impl FlowRuntime for FailingBatchWithSlowSiblingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation.context().schedule_steps(vec![
            invocation.context().step_with_retry(
                "fail",
                "failBatchStep",
                json!({}),
                RetryPolicy::none(),
            ),
            invocation.context().step_with_retry(
                "slow",
                "slowBatchStep",
                json!({}),
                RetryPolicy::none(),
            ),
        ]))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        self.rendezvous.wait().await;
        if invocation.step_id == "fail" {
            return Err(FlowError::Runtime("intentional batch failure".to_string()));
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        Ok(json!({ "step": invocation.step_id }))
    }
}

#[tokio::test]
async fn terminal_batch_failure_settles_every_started_sibling() {
    let runtime = Arc::new(FailingBatchWithSlowSiblingRuntime {
        rendezvous: Arc::new(Barrier::new(2)),
    });
    let engine = FlowEngine::in_memory(runtime);
    let run_id = tokio::time::timeout(Duration::from_secs(1), engine.start(spec(), json!({})))
        .await
        .expect("a terminal batch failure must not wait for a slow sibling")
        .expect("batch failure is a durable run outcome");

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    assert_eq!(snapshot.steps["fail"].status, StepStatus::Failed);
    assert_eq!(snapshot.steps["slow"].status, StepStatus::Cancelled);
    assert!(snapshot.steps.values().all(|step| {
        matches!(
            step.status,
            StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled
        )
    }));

    let history = engine.history(&run_id).await.unwrap();
    assert!(history.iter().any(|event| {
        matches!(
            &event.event,
            FlowEvent::StepCancelled { step_id, reason, .. }
                if step_id == "slow" && reason.contains("outcome is unknown")
        )
    }));
    assert!(matches!(
        history.last().map(|event| &event.event),
        Some(FlowEvent::RunRetryExhausted { step_id, .. }) if step_id == "fail"
    ));
}

struct MixedRetryBatchRuntime {
    retry_recorded: Arc<Barrier>,
}

#[async_trait]
impl FlowRuntime for MixedRetryBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation.context().schedule_steps(vec![
            invocation.context().step_with_retry(
                "fail",
                "mixedFail",
                json!({}),
                RetryPolicy::none(),
            ),
            invocation.context().step_with_retry(
                "delayed",
                "mixedDelayed",
                json!({}),
                RetryPolicy::fixed(2, Duration::from_secs(3600)),
            ),
        ]))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        if invocation.step_id == "fail" {
            // The observer releases this barrier only after the delayed
            // sibling's retry event has committed. This makes the regression
            // exercise the next batch round rather than depending on join
            // scheduling luck.
            self.retry_recorded.wait().await;
        }
        Err(FlowError::Runtime(format!(
            "{} failed on attempt {}",
            invocation.step_id,
            invocation
                .history
                .iter()
                .filter(|event| {
                    matches!(
                        event.event,
                        FlowEvent::StepStarted { ref step_id, .. } if step_id == &invocation.step_id
                    )
                })
                .count()
        )))
    }
}

struct RetryRecordingObserver {
    barrier: Arc<Barrier>,
}

#[async_trait]
impl FlowEventObserver for RetryRecordingObserver {
    async fn observe(&self, envelope: FlowEventEnvelope) {
        if matches!(
            envelope.event,
            FlowEvent::StepRetrying { ref step_id, .. } if step_id == "delayed"
        ) {
            self.barrier.wait().await;
        }
    }
}

#[tokio::test]
async fn terminal_batch_failure_settles_a_sibling_left_in_delayed_retry() {
    let retry_recorded = Arc::new(Barrier::new(2));
    let engine = FlowEngine::builder(Arc::new(MixedRetryBatchRuntime {
        retry_recorded: retry_recorded.clone(),
    }))
    .with_observer(Arc::new(RetryRecordingObserver {
        barrier: retry_recorded,
    }))
    .build();
    let run_id = tokio::time::timeout(Duration::from_secs(1), engine.start(spec(), json!({})))
        .await
        .expect("mixed retry batch must terminate promptly")
        .expect("terminal batch failure is a durable run outcome");

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    assert_eq!(snapshot.steps["fail"].status, StepStatus::Failed);
    assert_eq!(snapshot.steps["delayed"].status, StepStatus::Cancelled);
    assert!(snapshot.steps["delayed"].retry_after.is_none());
    assert!(engine
        .list_open_suspensions(Utc::now())
        .await
        .unwrap()
        .is_empty());
    let history = engine.history(&run_id).await.unwrap();
    assert!(history.iter().any(|event| {
        matches!(
            event.event,
            FlowEvent::StepRetrying { ref step_id, retry_after: Some(_), .. }
                if step_id == "delayed"
        )
    }));
    assert!(history.iter().any(|event| {
        matches!(
            event.event,
            FlowEvent::StepCancelled { ref step_id, .. } if step_id == "delayed"
        )
    }));
}

struct PanickingBatchRuntime;

#[async_trait]
impl FlowRuntime for PanickingBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation.context().schedule_steps(vec![
            invocation
                .context()
                .step("panic", "panicBatchStep", json!({})),
            invocation
                .context()
                .step("slow", "panicSlowBatchStep", json!({})),
        ]))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        if invocation.step_id == "panic" {
            panic!("intentional concurrent batch task panic");
        }
        pending().await
    }
}

#[tokio::test]
async fn panicking_batch_tasks_are_durably_settled_before_run_failure() {
    let engine = FlowEngine::in_memory(Arc::new(PanickingBatchRuntime));
    let run_id = tokio::time::timeout(Duration::from_secs(1), engine.start(spec(), json!({})))
        .await
        .expect("a panicking batch task must not strand its sibling")
        .expect("task panic is converted into a durable run failure");

    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    assert_eq!(snapshot.steps["panic"].status, StepStatus::Cancelled);
    assert_eq!(snapshot.steps["slow"].status, StepStatus::Cancelled);
    let history = engine.history(&run_id).await.unwrap();
    assert!(matches!(
        history.last().map(|event| &event.event),
        Some(FlowEvent::RunFailed { error }) if error.contains("before returning an outcome")
    ));
}

struct DelayedConcurrentBatchRuntime {
    barrier: Barrier,
    alpha_attempts: AtomicUsize,
    beta_attempts: AtomicUsize,
}

impl DelayedConcurrentBatchRuntime {
    fn new() -> Self {
        Self {
            barrier: Barrier::new(2),
            alpha_attempts: AtomicUsize::new(0),
            beta_attempts: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl FlowRuntime for DelayedConcurrentBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.step_output("alpha").is_some() && ctx.step_output("beta").is_some() {
            return Ok(ctx.complete(json!({ "done": true })));
        }
        let retry = RetryPolicy::fixed(2, Duration::from_millis(10));
        Ok(ctx.schedule_steps(vec![
            ctx.step_with_retry("alpha", "delayedBarrier", json!({}), retry),
            ctx.step_with_retry("beta", "delayedBarrier", json!({}), retry),
        ]))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        let attempts = match invocation.step_id.as_str() {
            "alpha" => &self.alpha_attempts,
            "beta" => &self.beta_attempts,
            other => return Err(FlowError::Runtime(format!("unknown batch step {other}"))),
        };
        let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
        self.barrier.wait().await;
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
async fn delayed_batch_retries_resume_all_due_siblings_concurrently() {
    let runtime = Arc::new(DelayedConcurrentBatchRuntime::new());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let suspended = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(suspended.status, WorkflowRunStatus::Suspended);
    assert_eq!(suspended.steps["alpha"].status, StepStatus::Pending);
    assert_eq!(suspended.steps["beta"].status, StepStatus::Pending);
    assert_eq!(runtime.alpha_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.beta_attempts.load(Ordering::SeqCst), 1);

    let resumed = tokio::time::timeout(
        Duration::from_secs(1),
        engine.resume_due_retries(Utc::now() + ChronoDuration::seconds(1)),
    )
    .await
    .expect("both due retries must re-enter the runtime together")
    .unwrap();
    assert_eq!(
        resumed,
        vec![
            (run_id.clone(), "alpha".to_string()),
            (run_id.clone(), "beta".to_string())
        ]
    );

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.steps["alpha"].attempt, 2);
    assert_eq!(completed.steps["beta"].attempt, 2);
    assert_eq!(runtime.alpha_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(runtime.beta_attempts.load(Ordering::SeqCst), 2);
}

struct StaggeredDelayedBatchRuntime {
    alpha_attempts: AtomicUsize,
    beta_attempts: AtomicUsize,
}

impl StaggeredDelayedBatchRuntime {
    fn new() -> Self {
        Self {
            alpha_attempts: AtomicUsize::new(0),
            beta_attempts: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl FlowRuntime for StaggeredDelayedBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.step_output("alpha").is_some() && ctx.step_output("beta").is_some() {
            return Ok(ctx.complete(json!({ "done": true })));
        }
        Ok(ctx.schedule_steps(vec![
            ctx.step_with_retry(
                "alpha",
                "staggeredDelayed",
                json!({}),
                RetryPolicy::fixed(2, Duration::from_millis(10)),
            ),
            ctx.step_with_retry(
                "beta",
                "staggeredDelayed",
                json!({}),
                RetryPolicy::fixed(2, Duration::from_secs(60)),
            ),
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
async fn due_batch_retry_is_not_blocked_or_joined_by_a_future_sibling() {
    let runtime = Arc::new(StaggeredDelayedBatchRuntime::new());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let suspended = engine.snapshot(&run_id).await.unwrap();
    let alpha_due = suspended.steps["alpha"].retry_after.unwrap();
    let beta_due = suspended.steps["beta"].retry_after.unwrap();

    assert!(alpha_due < beta_due);
    assert_eq!(
        engine.resume_due_retries(alpha_due).await.unwrap(),
        vec![(run_id.clone(), "alpha".to_string())]
    );

    let partially_resumed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(partially_resumed.status, WorkflowRunStatus::Suspended);
    assert_eq!(
        partially_resumed.steps["alpha"].status,
        StepStatus::Completed
    );
    assert_eq!(partially_resumed.steps["alpha"].attempt, 2);
    assert_eq!(partially_resumed.steps["beta"].status, StepStatus::Pending);
    assert_eq!(partially_resumed.steps["beta"].attempt, 1);
    assert_eq!(runtime.alpha_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(runtime.beta_attempts.load(Ordering::SeqCst), 1);

    assert_eq!(
        engine
            .resume_due_retries(beta_due + ChronoDuration::seconds(1))
            .await
            .unwrap(),
        vec![(run_id.clone(), "beta".to_string())]
    );
    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(runtime.beta_attempts.load(Ordering::SeqCst), 2);
}

struct DuplicateStepBatchRuntime;

#[async_trait]
impl FlowRuntime for DuplicateStepBatchRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        Ok(ctx.schedule_steps(vec![
            ctx.step("duplicate", "first", json!({})),
            ctx.step("duplicate", "second", json!({})),
        ]))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("duplicate batch should fail before running steps")
    }
}

#[tokio::test]
async fn schedule_steps_rejects_duplicate_step_ids() {
    let engine = FlowEngine::in_memory(Arc::new(DuplicateStepBatchRuntime));
    let err = engine.start(spec(), json!({})).await.unwrap_err();

    assert!(
        matches!(err, FlowError::InvalidTransition(message) if message.contains("duplicate step id duplicate"))
    );
}
