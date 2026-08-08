use a3s_flow::{
    CancellationRequest, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore,
    FlowRuntime, InMemoryEventStore, RetryPolicy, RuntimeCommand, StepInvocation, StepStatus,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("test.crash-recovery", "1", "tests::runtime", "main")
}

struct CrashBeforeRunStartedStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeRunStartedStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeRunStartedStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(event, FlowEvent::RunStarted) && self.armed.swap(false, Ordering::SeqCst) {
            return Err(FlowError::Store(
                "injected crash before run start became durable".into(),
            ));
        }
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

struct CrashBeforeRetryExhaustionStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeRetryExhaustionStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeRetryExhaustionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(
            &event,
            FlowEvent::RunRetryExhausted { step_id, .. } if step_id == "permanent-failure"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before retry exhaustion became durable".into(),
            ));
        }
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

struct CrashBeforeStepCompletionStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeStepCompletionStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeStepCompletionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(
            &event,
            FlowEvent::StepCompleted { step_id, .. } if step_id == "durable-effect"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before step completion became durable".into(),
            ));
        }
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[derive(Default)]
struct DurableEffectRuntime {
    effect_invocations: AtomicUsize,
}

#[derive(Default)]
struct PermanentFailureRuntime {
    step_invocations: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for PermanentFailureRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        if context.cancellation_request().is_some() {
            return Ok(context.cancel());
        }
        Ok(context.schedule_step_with_retry(
            "permanent-failure",
            "failPermanently",
            json!({"effectId": "stable-failure"}),
            RetryPolicy::none(),
        ))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        self.step_invocations.fetch_add(1, Ordering::SeqCst);
        Err(FlowError::Runtime("permanent step failure".into()))
    }
}

#[async_trait]
impl FlowRuntime for DurableEffectRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.step_output("durable-effect") {
            Some(output) => Ok(context.complete(output.clone())),
            None => Ok(context.schedule_step_with_retry(
                "durable-effect",
                "persistDurableEffect",
                json!({"effectId": "stable-effect"}),
                RetryPolicy::none(),
            )),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        self.effect_invocations.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"effectId": "stable-effect"}))
    }
}

#[tokio::test]
async fn running_step_is_redelivered_after_completion_persistence_is_lost() {
    let run_id = "crash-recovery";
    let store = Arc::new(CrashBeforeStepCompletionStore::new());
    let runtime = Arc::new(DurableEffectRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine.snapshot(run_id).await.expect("running snapshot");
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);
    assert_eq!(
        interrupted.steps["durable-effect"].status,
        StepStatus::Running
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 1);

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    restarted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect("restarted engine must redeliver the running step");

    assert_eq!(
        restarted
            .snapshot(run_id)
            .await
            .expect("completed snapshot")
            .status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 2);
    let history = store.list(run_id).await.expect("recovered history");
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepStarted { .. }))
            .count(),
        1,
        "redelivery must reuse the interrupted attempt"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepCompleted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn exhausted_step_terminalizes_after_retry_exhaustion_persistence_is_lost() {
    let run_id = "retry-exhaustion-crash-recovery";
    let store = Arc::new(CrashBeforeRetryExhaustionStore::new());
    let runtime = Arc::new(PermanentFailureRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine.snapshot(run_id).await.expect("failed step snapshot");
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);
    assert_eq!(
        interrupted.steps["permanent-failure"].status,
        StepStatus::Failed
    );
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 1);

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    restarted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect("restarted engine must finish the interrupted terminal transition");

    let recovered = restarted
        .snapshot(run_id)
        .await
        .expect("retry-exhausted snapshot");
    assert_eq!(recovered.status, WorkflowRunStatus::Failed);
    assert!(matches!(
        recovered.terminal_outcome,
        Some(WorkflowTerminalOutcome::RetryExhausted {
            ref step_id,
            attempt: 1,
            ref error,
        }) if step_id == "permanent-failure" && error == "runtime error: permanent step failure"
    ));
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 1);

    let history = store.list(run_id).await.expect("recovered history");
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepStarted { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::StepFailed { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunRetryExhausted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn exhausted_step_failure_wins_over_a_racing_cancellation_after_restart() {
    let run_id = "retry-exhaustion-cancellation-race";
    let store = Arc::new(CrashBeforeRetryExhaustionStore::new());
    let runtime = Arc::new(PermanentFailureRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the first engine");

    let recovered = engine
        .request_cancellation(
            run_id,
            CancellationRequest::new(Some("racing cancellation".into())),
        )
        .await
        .expect("the durable step failure must still reach its terminal outcome");
    assert_eq!(recovered.status, WorkflowRunStatus::Failed);
    assert!(matches!(
        recovered.terminal_outcome,
        Some(WorkflowTerminalOutcome::RetryExhausted {
            ref step_id,
            attempt: 1,
            ..
        }) if step_id == "permanent-failure"
    ));
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 1);

    let history = store.list(run_id).await.expect("recovered history");
    assert!(history
        .iter()
        .any(|event| matches!(event.event, FlowEvent::RunCancellationRequested { .. })));
    assert!(history
        .iter()
        .any(|event| matches!(event.event, FlowEvent::RunRetryExhausted { .. })));
    assert!(!history
        .iter()
        .any(|event| matches!(event.event, FlowEvent::RunCancelled { .. })));
}

#[tokio::test]
async fn terminal_run_is_not_started_after_run_start_persistence_is_lost() {
    let run_id = "run-start-crash-recovery";
    let store = Arc::new(CrashBeforeRunStartedStore::new());
    let runtime = Arc::new(DurableEffectRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    assert_eq!(
        engine
            .snapshot(run_id)
            .await
            .expect("pending snapshot")
            .status,
        WorkflowRunStatus::Pending
    );

    engine
        .force_cancel(run_id, Some("cancelled before start recovery".into()))
        .await
        .expect("the created run remains cancellable");
    drop(engine);

    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    let recovered_run_id = restarted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect("an idempotent start must preserve the terminal run");
    assert_eq!(recovered_run_id, run_id);

    let recovered = restarted
        .snapshot(run_id)
        .await
        .expect("cancelled snapshot");
    assert_eq!(recovered.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        recovered.terminal_outcome,
        Some(WorkflowTerminalOutcome::Cancelled {
            reason: Some("cancelled before start recovery".into()),
        })
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 0);

    let history = store.list(run_id).await.expect("preserved history");
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunStarted))
            .count(),
        0,
        "start recovery must not append after a terminal event"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunCancelled { .. }))
            .count(),
        1
    );
}
