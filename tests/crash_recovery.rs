use a3s_flow::{
    ActivityInvocation, ActivityStatus, CancellationRequest, FlowEngine, FlowError, FlowEvent,
    FlowEventEnvelope, FlowEventStore, FlowRuntime, FlowTask, FlowWorker, InMemoryEventStore,
    InMemoryFlowTaskQueue, RetryPolicy, RuntimeCommand, StepInvocation, StepStatus,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use serde_json::json;
use std::future::pending;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

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

struct CrashBeforeBatchSettlementStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeBatchSettlementStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeBatchSettlementStore {
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
            FlowEvent::StepCancelled { step_id, .. } if step_id == "slow"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before batch sibling settlement became durable".into(),
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

struct CrashBeforeActivityCompletionStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeActivityCompletionStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeActivityCompletionStore {
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
            FlowEvent::ActivityCompleted { activity_id, .. } if activity_id == "durable-effect"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before activity completion became durable".into(),
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

struct CrashBeforeActivityStartedStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeActivityStartedStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeActivityStartedStore {
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
            FlowEvent::ActivityStarted { activity_id, .. } if activity_id == "durable-effect"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before activity start became durable".into(),
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

struct CrashBeforeActivityUnknownStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeActivityUnknownStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeActivityUnknownStore {
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
            FlowEvent::ActivityUnknown { activity_id, .. } if activity_id == "durable-effect"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before activity unknown outcome became durable".into(),
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
struct DurableActivityEffectRuntime {
    effect_invocations: AtomicUsize,
}

#[derive(Default)]
struct AmbiguousThenCompleteActivityRuntime {
    effect_invocations: AtomicUsize,
}

struct HeartbeatThenCrashActivityRuntime {
    started: Arc<Notify>,
    release: Arc<Notify>,
    redelivered: Arc<Notify>,
    release_redelivery: Arc<Notify>,
    effect_invocations: AtomicUsize,
}

#[derive(Default)]
struct PermanentFailureRuntime {
    step_invocations: AtomicUsize,
}

#[derive(Default)]
struct BatchFailureRecoveryRuntime {
    step_invocations: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for BatchFailureRecoveryRuntime {
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
        self.step_invocations.fetch_add(1, Ordering::SeqCst);
        if invocation.step_id == "fail" {
            return Err(FlowError::Runtime("permanent batch failure".into()));
        }
        pending::<a3s_flow::Result<serde_json::Value>>().await
    }
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

#[async_trait]
impl FlowRuntime for DurableActivityEffectRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.activity_output("durable-effect") {
            Some(output) => Ok(context.complete(output.clone())),
            None => Ok(context.schedule_activity_with_retry(
                "durable-effect",
                "persistDurableEffect",
                json!({"effectId": "stable-effect"}),
                RetryPolicy::none(),
            )),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!(null))
    }

    async fn run_activity(
        &self,
        _invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        self.effect_invocations.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"effectId": "stable-effect"}))
    }
}

#[async_trait]
impl FlowRuntime for AmbiguousThenCompleteActivityRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.activity_output("durable-effect") {
            Some(output) => Ok(context.complete(output.clone())),
            None => Ok(context.schedule_activity_with_retry(
                "durable-effect",
                "persistDurableEffect",
                json!({"effectId": "stable-effect"}),
                RetryPolicy::none(),
            )),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!(null))
    }

    async fn run_activity(
        &self,
        _invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        let call = self.effect_invocations.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            return Err(FlowError::UnknownOutcome(
                "provider connection lost after request".to_string(),
            ));
        }
        Ok(json!({"effectId": "stable-effect"}))
    }
}

#[async_trait]
impl FlowRuntime for HeartbeatThenCrashActivityRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let context = invocation.context();
        match context.activity_output("durable-effect") {
            Some(output) => Ok(context.complete(output.clone())),
            None => Ok(context.schedule_activity_with_retry(
                "durable-effect",
                "persistDurableEffect",
                json!({"effectId": "stable-effect"}),
                RetryPolicy::none(),
            )),
        }
    }

    async fn run_step(&self, _invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Ok(json!(null))
    }

    async fn run_activity(
        &self,
        _invocation: ActivityInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        let call = self.effect_invocations.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            self.started.notify_one();
            self.release.notified().await;
        } else {
            self.redelivered.notify_one();
            self.release_redelivery.notified().await;
        }
        Ok(json!({"effectId": "stable-effect", "cursor": 42}))
    }
}

#[tokio::test]
async fn running_activity_is_redelivered_after_completion_persistence_is_lost() {
    let run_id = "activity-crash-recovery";
    let store = Arc::new(CrashBeforeActivityCompletionStore::new());
    let runtime = Arc::new(DurableActivityEffectRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine.snapshot(run_id).await.expect("running snapshot");
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);
    assert_eq!(
        interrupted.activities["durable-effect"].status,
        ActivityStatus::Running
    );
    assert_eq!(interrupted.activities["durable-effect"].attempt, 1);
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 1);

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    restarted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect("restarted engine must redeliver the running activity");

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
            .filter(|event| matches!(event.event, FlowEvent::ActivityStarted { .. }))
            .count(),
        1,
        "redelivery must reuse the interrupted attempt instead of consuming retry budget"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityLeaseAcquired { .. }))
            .count(),
        1,
        "replacement ownership must rotate the fencing token for the same attempt"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityCompleted { .. }))
            .count(),
        1
    );
    let recovered = restarted.snapshot(run_id).await.expect("final snapshot");
    assert_eq!(recovered.activities["durable-effect"].attempt, 1);
}

#[tokio::test]
async fn heartbeat_checkpoint_survives_completion_persistence_loss_and_redelivery() {
    let run_id = "activity-heartbeat-crash-recovery";
    let store = Arc::new(CrashBeforeActivityCompletionStore::new());
    let runtime = Arc::new(HeartbeatThenCrashActivityRuntime {
        started: Arc::new(Notify::new()),
        release: Arc::new(Notify::new()),
        redelivered: Arc::new(Notify::new()),
        release_redelivery: Arc::new(Notify::new()),
        effect_invocations: AtomicUsize::new(0),
    });
    let engine = Arc::new(FlowEngine::new(store.clone(), runtime.clone()));
    let task_engine = Arc::clone(&engine);
    let drive = tokio::spawn(async move {
        task_engine
            .start_with_id(run_id, workflow_spec(), json!({}))
            .await
    });

    runtime.started.notified().await;
    let running = engine.snapshot(run_id).await.expect("running snapshot");
    let activity = running.activities.get("durable-effect").unwrap();
    let pre_crash_fence = activity.fencing_token.clone();
    let attempt_id = activity.attempt_id.clone();
    engine
        .heartbeat_activity(
            run_id,
            "durable-effect",
            activity.attempt,
            &attempt_id,
            &pre_crash_fence,
            Some(json!({ "cursor": 42 })),
        )
        .await
        .expect("heartbeat checkpoint must persist");
    assert_eq!(
        engine.snapshot(run_id).await.unwrap().activities["durable-effect"].checkpoint,
        Some(json!({ "cursor": 42 }))
    );

    runtime.release.notify_one();
    let failure = drive
        .await
        .unwrap()
        .expect_err("completion persistence must fail");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine.snapshot(run_id).await.expect("interrupted snapshot");
    assert_eq!(
        interrupted.activities["durable-effect"].status,
        ActivityStatus::Running
    );
    assert_eq!(
        interrupted.activities["durable-effect"].checkpoint,
        Some(json!({ "cursor": 42 })),
        "durable heartbeat checkpoint must survive the completion crash window"
    );
    assert_eq!(interrupted.activities["durable-effect"].attempt, 1);
    assert_eq!(
        interrupted.activities["durable-effect"].attempt_id,
        attempt_id
    );

    drop(engine);
    let restarted = Arc::new(FlowEngine::new(store.clone(), runtime.clone()));
    let redelivery_engine = Arc::clone(&restarted);
    let redelivery = tokio::spawn(async move {
        redelivery_engine
            .start_with_id(run_id, workflow_spec(), json!({}))
            .await
    });
    runtime.redelivered.notified().await;
    let leased = restarted
        .snapshot(run_id)
        .await
        .expect("lease-rotated snapshot");
    assert_eq!(
        leased.activities["durable-effect"].checkpoint,
        Some(json!({ "cursor": 42 }))
    );
    assert_ne!(
        leased.activities["durable-effect"].fencing_token,
        pre_crash_fence
    );
    let stale = restarted
        .heartbeat_activity(
            run_id,
            "durable-effect",
            1,
            &attempt_id,
            &pre_crash_fence,
            Some(json!({ "cursor": 99 })),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(stale, FlowError::InvalidTransition(ref message) if message.contains("stale")),
        "{stale:?}"
    );

    runtime.release_redelivery.notify_one();
    redelivery
        .await
        .unwrap()
        .expect("redelivery must complete the same attempt");
    let completed = restarted
        .snapshot(run_id)
        .await
        .expect("completed snapshot");
    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    assert_eq!(completed.activities["durable-effect"].attempt, 1);
    assert_eq!(
        completed.activities["durable-effect"].attempt_id,
        attempt_id
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 2);
    let history = store.list(run_id).await.expect("history");
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityHeartbeat { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityLeaseAcquired { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityCompleted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn pending_activity_starts_after_start_persistence_is_lost() {
    let run_id = "activity-start-crash-recovery";
    let store = Arc::new(CrashBeforeActivityStartedStore::new());
    let runtime = Arc::new(DurableActivityEffectRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine.snapshot(run_id).await.expect("pending snapshot");
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);
    assert_eq!(
        interrupted.activities["durable-effect"].status,
        ActivityStatus::Pending
    );
    assert_eq!(interrupted.activities["durable-effect"].attempt, 0);
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 0);

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    restarted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect("restarted engine must start the pending activity");

    assert_eq!(
        restarted
            .snapshot(run_id)
            .await
            .expect("completed snapshot")
            .status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 1);
    let history = store.list(run_id).await.expect("recovered history");
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityCreated { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityStarted { .. }))
            .count(),
        1,
        "start must be recorded exactly once after the crash window"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityCompleted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn running_activity_redelivers_after_unknown_outcome_persistence_is_lost() {
    let run_id = "activity-unknown-crash-recovery";
    let store = Arc::new(CrashBeforeActivityUnknownStore::new());
    let runtime = Arc::new(AmbiguousThenCompleteActivityRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine.snapshot(run_id).await.expect("running snapshot");
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);
    assert_eq!(
        interrupted.activities["durable-effect"].status,
        ActivityStatus::Running
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 1);
    assert!(!store
        .list(run_id)
        .await
        .unwrap()
        .iter()
        .any(|event| { matches!(event.event, FlowEvent::ActivityUnknown { .. }) }));

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    restarted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect("restarted engine must redeliver the still-running attempt");

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
            .filter(|event| matches!(event.event, FlowEvent::ActivityStarted { .. }))
            .count(),
        1,
        "lost unknown-outcome persistence must not open a new attempt"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityLeaseAcquired { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityUnknown { .. }))
            .count(),
        0,
        "undurable unknown outcomes must not leave a suspended Unknown state"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::ActivityCompleted { .. }))
            .count(),
        1
    );
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
async fn batch_terminal_settlement_recovers_after_sibling_cleanup_persistence_is_lost() {
    let run_id = "batch-settlement-crash-recovery";
    let store = Arc::new(CrashBeforeBatchSettlementStore::new());
    let runtime = Arc::new(BatchFailureRecoveryRuntime::default());
    let engine = FlowEngine::new(store.clone(), runtime.clone());

    let failure = engine
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected sibling-settlement loss must interrupt the first engine");
    assert!(matches!(failure, FlowError::Store(_)));
    let interrupted = engine
        .snapshot(run_id)
        .await
        .expect("interrupted batch snapshot");
    assert_eq!(interrupted.status, WorkflowRunStatus::Running);
    assert_eq!(interrupted.steps["fail"].status, StepStatus::Failed);
    assert_eq!(interrupted.steps["slow"].status, StepStatus::Running);
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 2);

    drop(engine);
    let restarted = FlowEngine::new(store.clone(), runtime.clone());
    restarted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect("restart must finish the interrupted batch terminal transition");
    let recovered = restarted
        .snapshot(run_id)
        .await
        .expect("recovered batch snapshot");
    assert_eq!(recovered.status, WorkflowRunStatus::Failed);
    assert_eq!(recovered.steps["fail"].status, StepStatus::Failed);
    assert_eq!(recovered.steps["slow"].status, StepStatus::Cancelled);
    assert!(recovered.steps.values().all(|step| {
        matches!(
            step.status,
            StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled
        )
    }));
    assert_eq!(runtime.step_invocations.load(Ordering::SeqCst), 2);

    let history = store.list(run_id).await.expect("recovered history");
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
async fn drive_task_recovers_run_started_before_replaying_a_pending_run() {
    let run_id = "drive-run-start-crash-recovery";
    let store = Arc::new(CrashBeforeRunStartedStore::new());
    let runtime = Arc::new(DurableEffectRuntime::default());
    let interrupted = FlowEngine::builder(runtime.clone())
        .with_store(store.clone())
        .with_max_replay_iterations(2)
        .build();

    let failure = interrupted
        .start_with_id(run_id, workflow_spec(), json!({}))
        .await
        .expect_err("the injected persistence loss must interrupt the initial start");
    assert!(matches!(failure, FlowError::Store(_)));
    assert_eq!(
        interrupted.snapshot(run_id).await.unwrap().status,
        WorkflowRunStatus::Pending
    );
    drop(interrupted);

    let replacement = FlowEngine::builder(runtime.clone())
        .with_store(store.clone())
        .with_max_replay_iterations(2)
        .build();
    let worker = FlowWorker::new(replacement.clone(), Arc::new(InMemoryFlowTaskQueue::new()));
    worker
        .enqueue(FlowTask::DriveRun {
            run_id: run_id.to_string(),
        })
        .await
        .unwrap();

    let outcome = worker.run_once().await.unwrap().unwrap();
    assert_eq!(outcome.run_ids, vec![run_id]);
    assert_eq!(
        replacement.snapshot(run_id).await.unwrap().status,
        WorkflowRunStatus::Completed
    );
    assert_eq!(runtime.effect_invocations.load(Ordering::SeqCst), 1);

    let history = store.list(run_id).await.unwrap();
    assert!(matches!(history[0].event, FlowEvent::RunCreated { .. }));
    assert!(
        matches!(history[1].event, FlowEvent::RunStarted),
        "worker replay must recover run_started before workflow-owned events"
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunStarted))
            .count(),
        1
    );
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
