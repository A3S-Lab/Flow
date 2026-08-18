use super::*;

#[derive(Default)]
struct FlakyRuntime {
    attempts: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for FlakyRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if let Some(output) = completed_step(&invocation, "flaky") {
            return Ok(RuntimeCommand::Complete { output });
        }

        Ok(RuntimeCommand::ScheduleStep {
            step_id: "flaky".to_string(),
            step_name: "flakyStep".to_string(),
            input: json!({}),
            retry: RetryPolicy::fixed(2, Duration::from_millis(0)),
        })
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
async fn retries_failed_step_before_failing_run() {
    let runtime = Arc::new(FlakyRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.steps["flaky"].attempt, 2);
    assert_eq!(snapshot.steps["flaky"].status, StepStatus::Completed);
}

struct RecoverableStepFailureRuntime;

struct ExhaustedStepFailureRuntime;

#[async_trait]
impl FlowRuntime for ExhaustedStepFailureRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        Ok(invocation.context().schedule_step_with_retry(
            "primary",
            "primaryStep",
            json!({}),
            RetryPolicy::none(),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime("primary failed".to_string()))
    }
}

#[tokio::test]
async fn exhausted_step_failure_fails_run_by_default() {
    let engine = FlowEngine::in_memory(Arc::new(ExhaustedStepFailureRuntime));
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    let history = engine.history(&run_id).await.unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    assert_eq!(snapshot.steps["primary"].status, StepStatus::Failed);
    assert_eq!(snapshot.steps["primary"].attempt, 1);
    assert_eq!(
        snapshot.steps["primary"].retry.on_exhausted,
        StepFailureAction::FailRun
    );
    assert_eq!(
        snapshot.terminal_outcome,
        Some(WorkflowTerminalOutcome::RetryExhausted {
            step_id: "primary".to_string(),
            attempt: 1,
            error: "runtime error: primary failed".to_string(),
        })
    );
    assert!(history
        .iter()
        .any(|envelope| matches!(envelope.event, FlowEvent::RunRetryExhausted { .. })));
}

#[async_trait]
impl FlowRuntime for RecoverableStepFailureRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if let Some(fallback) = ctx.step_output("fallback") {
            return Ok(ctx.complete(json!({
                "status": "degraded",
                "fallback": fallback,
            })));
        }
        if let Some(error) = ctx.step_failed("primary") {
            return Ok(ctx.schedule_step(
                "fallback",
                "fallbackStep",
                json!({ "primaryError": error }),
            ));
        }

        Ok(ctx.schedule_step_with_retry(
            "primary",
            "primaryStep",
            json!({}),
            RetryPolicy::none().continue_workflow_on_failure(),
        ))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "primaryStep" => Err(FlowError::Runtime("primary system unavailable".to_string())),
            "fallbackStep" => Ok(json!({
                "used": true,
                "primaryError": invocation.input["primaryError"],
            })),
            step => Err(FlowError::Runtime(format!("unknown step {step}"))),
        }
    }
}

#[tokio::test]
async fn recoverable_step_failure_replays_to_workflow_for_fallback() {
    let engine = FlowEngine::in_memory(Arc::new(RecoverableStepFailureRuntime));
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(snapshot.steps["primary"].status, StepStatus::Failed);
    assert_eq!(snapshot.steps["primary"].attempt, 1);
    assert_eq!(
        snapshot.steps["primary"].retry.on_exhausted,
        StepFailureAction::ContinueWorkflow
    );
    assert_eq!(snapshot.steps["fallback"].status, StepStatus::Completed);
    assert_eq!(snapshot.output.as_ref().unwrap()["status"], "degraded");
    assert_eq!(
        snapshot.output.as_ref().unwrap()["fallback"]["primaryError"],
        "runtime error: primary system unavailable"
    );
}

#[derive(Default)]
struct RepeatedTerminalStepRuntime {
    workflow_invocations: AtomicUsize,
    step_invocations: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for RepeatedTerminalStepRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_invocations.fetch_add(1, Ordering::SeqCst);
        Ok(invocation.context().schedule_step_with_retry(
            "primary",
            "primaryStep",
            json!({}),
            RetryPolicy::none().continue_workflow_on_failure(),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        self.step_invocations.fetch_add(1, Ordering::SeqCst);
        Err(FlowError::Runtime("primary failed".to_string()))
    }
}

#[tokio::test]
async fn rescheduling_a_terminal_step_fails_without_replay_spam() {
    let runtime = Arc::new(RepeatedTerminalStepRuntime::default());
    let engine = FlowEngine::builder(runtime.clone())
        .with_max_replay_iterations(8)
        .build();

    let error = engine.start(spec(), json!({})).await.unwrap_err();

    assert!(
        matches!(error, FlowError::InvalidTransition(ref message) if message.contains("rescheduled terminal step primary")),
        "{error}"
    );
    assert_eq!(runtime.workflow_invocations.load(Ordering::SeqCst), 2);
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 1);
}

#[derive(Default)]
struct DelayedFlakyRuntime {
    attempts: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for DelayedFlakyRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        if let Some(output) = completed_step(&invocation, "delayed-flaky") {
            return Ok(RuntimeCommand::Complete { output });
        }

        Ok(RuntimeCommand::ScheduleStep {
            step_id: "delayed-flaky".to_string(),
            step_name: "delayedFlakyStep".to_string(),
            input: json!({}),
            retry: RetryPolicy::fixed(2, Duration::from_secs(60)),
        })
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
async fn delayed_step_retry_suspends_until_due() {
    let now = Utc::now();
    let runtime = Arc::new(DelayedFlakyRuntime::default());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({})).await.unwrap();
    let waiting = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 1);
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(waiting.steps["delayed-flaky"].status, StepStatus::Pending);
    assert!(waiting.steps["delayed-flaky"].retry_after.is_some());
    assert_eq!(
        engine.list_due_retries(now).await.unwrap(),
        Vec::<(String, String)>::new()
    );

    let resumed = engine
        .resume_due_retries(now + ChronoDuration::seconds(120))
        .await
        .unwrap();
    assert_eq!(resumed, vec![(run_id.clone(), "delayed-flaky".to_string())]);

    let completed = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(runtime.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(
        completed.steps["delayed-flaky"].status,
        StepStatus::Completed
    );
    assert_eq!(completed.steps["delayed-flaky"].retry_after, None);
    assert_eq!(completed.output.unwrap()["attempt"], 2);
}

#[tokio::test]
async fn run_summary_counts_statuses_and_actionable_work() {
    let store = Arc::new(InMemoryEventStore::new());
    let completed_engine = FlowEngine::new(store.clone(), Arc::new(SequentialRuntime));
    let failed_engine = FlowEngine::new(store.clone(), Arc::new(ExhaustedStepFailureRuntime));
    let wait_engine = FlowEngine::new(store.clone(), Arc::new(InputSleepRuntime));
    let hook_engine = FlowEngine::new(store.clone(), Arc::new(DisposableHookRuntime));
    let retry_engine = FlowEngine::new(store.clone(), Arc::new(DelayedFlakyRuntime::default()));
    let started_at = Utc::now();
    let future = (started_at + ChronoDuration::hours(1)).to_rfc3339();

    completed_engine
        .start_with_id("summary-completed", spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    failed_engine
        .start_with_id("summary-failed", spec(), json!({}))
        .await
        .unwrap();
    wait_engine
        .start_with_id("summary-wait", spec(), json!({ "resume_at": future }))
        .await
        .unwrap();
    let cancelled_run_id = wait_engine
        .start_with_id(
            "summary-cancelled",
            spec(),
            json!({ "resume_at": (Utc::now() + ChronoDuration::hours(2)).to_rfc3339() }),
        )
        .await
        .unwrap();
    wait_engine
        .cancel(&cancelled_run_id, Some("not actionable".to_string()))
        .await
        .unwrap();
    hook_engine
        .start_with_id("summary-hook", spec(), json!({ "token": "summary-token" }))
        .await
        .unwrap();
    retry_engine
        .start_with_id("summary-retry", spec(), json!({}))
        .await
        .unwrap();

    let summary = completed_engine.run_summary().await.unwrap();
    assert_eq!(summary.total_runs, 6);
    assert_eq!(summary.completed_runs, 1);
    assert_eq!(summary.failed_runs, 1);
    assert_eq!(summary.cancelled_runs, 1);
    assert_eq!(summary.suspended_runs, 3);
    assert_eq!(summary.terminal_runs, 3);
    assert_eq!(summary.non_terminal_runs, 3);
    assert_eq!(summary.open_waits, 1);
    assert_eq!(summary.active_hooks, 1);
    assert_eq!(summary.pending_retries, 1);

    let snapshots = completed_engine.list_snapshots().await.unwrap();
    assert_eq!(WorkflowRunSummary::from_snapshots(&snapshots), summary);
    assert_eq!(
        completed_engine
            .snapshot("summary-cancelled")
            .await
            .unwrap()
            .waits["nap"]
            .status,
        WaitStatus::Waiting
    );

    let suspensions = completed_engine
        .list_open_suspensions(started_at + ChronoDuration::seconds(120))
        .await
        .unwrap();
    assert_eq!(
        suspensions
            .iter()
            .map(|suspension| (
                suspension.run_id(),
                suspension.subject_id(),
                suspension.is_due()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("summary-hook", "approval", false),
            ("summary-retry", "delayed-flaky", true),
            ("summary-wait", "nap", false),
        ]
    );
    assert!(matches!(
        &suspensions[0],
        WorkflowRunSuspension::Hook { hook, .. } if hook.token == "summary-token"
    ));
    assert!(matches!(
        &suspensions[1],
        WorkflowRunSuspension::Retry { step, due: true, .. }
            if step.retry_after.is_some()
    ));
    assert!(matches!(
        &suspensions[2],
        WorkflowRunSuspension::Wait { wait, due: false, .. }
            if wait.resume_at > started_at
    ));

    let next_wakeup = completed_engine
        .next_wakeup(started_at + ChronoDuration::seconds(120))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next_wakeup.run_id(), "summary-retry");
    assert_eq!(next_wakeup.subject_id(), "delayed-flaky");
    assert!(next_wakeup.is_due());
    assert!(next_wakeup.scheduled_at().unwrap() < started_at + ChronoDuration::hours(1));
}

struct RecordingRuntime {
    workflow_invocations: Mutex<Vec<usize>>,
}

impl RecordingRuntime {
    fn new() -> Self {
        Self {
            workflow_invocations: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl FlowRuntime for RecordingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_invocations
            .lock()
            .unwrap()
            .push(invocation.history.len());
        Ok(RuntimeCommand::Complete {
            output: json!({ "ok": true }),
        })
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        unreachable!("recording runtime does not schedule steps")
    }
}

#[tokio::test]
async fn stores_spec_with_run_for_runtime_replay() {
    let runtime = Arc::new(RecordingRuntime::new());
    let engine = FlowEngine::in_memory(runtime.clone());
    let run_id = engine.start(spec(), json!({ "x": 1 })).await.unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(snapshot.spec.name, "test.workflow");
    assert_eq!(snapshot.spec.runtime.entrypoint, "tests::runtime");
    assert_eq!(
        *runtime.workflow_invocations.lock().unwrap(),
        vec![2],
        "workflow replay receives run_created and run_started"
    );
}
