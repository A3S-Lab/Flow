use a3s_flow::{
    CancellationRequest, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore,
    FlowRuntime, InMemoryEventStore, JsonValue, QueryInvocation, RuntimeCommand,
    WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

struct ScopeRuntime {
    workflow_calls: AtomicUsize,
}

#[async_trait]
impl FlowRuntime for ScopeRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        self.workflow_calls.fetch_add(1, Ordering::SeqCst);
        let ctx = invocation.context();
        if ctx.cancellation_request().is_some() {
            return Ok(RuntimeCommand::Cancel);
        }
        if ctx.scope_cancelled("payment") {
            return Ok(RuntimeCommand::Complete {
                output: json!({ "cancelled": true }),
            });
        }
        if !ctx.has_scope("payment") {
            return Ok(ctx.open_scope("payment"));
        }
        if ctx.wait_status("pause").is_none() {
            return Ok(ctx.wait_until("pause", "2030-01-01T00:00:00Z".parse().unwrap()));
        }
        Ok(RuntimeCommand::Complete {
            output: json!({ "cancelled": false }),
        })
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("steps unused".into()))
    }

    async fn run_query(&self, _invocation: QueryInvocation) -> a3s_flow::Result<JsonValue> {
        Err(FlowError::Runtime("queries unused".into()))
    }
}

fn scope_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("scope.demo", "1", "tests::scope", "main")
}

#[tokio::test]
async fn cancelling_a_scope_cancels_scoped_waits_without_terminating_the_run() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(
        store,
        Arc::new(ScopeRuntime {
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    let run_id = engine
        .start_with_id("scope-run", scope_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine.snapshot(&run_id).await.unwrap();
    assert!(snapshot.scope("payment").unwrap().is_open());
    assert_eq!(
        snapshot.waits.get("pause").unwrap().scope_id.as_deref(),
        Some("payment")
    );
    assert_eq!(
        snapshot.waits.get("pause").unwrap().status,
        a3s_flow::WaitStatus::Waiting
    );

    let after_cancel = engine
        .cancel_scope(&run_id, "payment", Some("customer aborted".into()))
        .await
        .unwrap();
    assert!(after_cancel.scope("payment").unwrap().is_cancelled());
    assert_eq!(
        after_cancel.waits.get("pause").unwrap().status,
        a3s_flow::WaitStatus::Cancelled
    );
    assert!(after_cancel.status.is_terminal());
    assert_eq!(after_cancel.output, Some(json!({ "cancelled": true })));
}

#[tokio::test]
async fn scope_cancellation_is_idempotent_and_conflict_safe() {
    let store = Arc::new(InMemoryEventStore::new());
    let engine = FlowEngine::new(
        store.clone(),
        Arc::new(ScopeRuntime {
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    engine
        .start_with_id("scope-idem", scope_spec(), json!({}))
        .await
        .unwrap();
    let first = engine
        .cancel_scope("scope-idem", "payment", Some("once".into()))
        .await
        .unwrap();
    assert!(
        first.scope("payment").unwrap().is_cancelled(),
        "status={:?} scopes={:?}",
        first.status,
        first.scopes
    );
    let before = store.list("scope-idem").await.unwrap().len();
    let second = engine
        .cancel_scope("scope-idem", "payment", Some("once".into()))
        .await
        .unwrap();
    let after = store.list("scope-idem").await.unwrap().len();
    assert_eq!(before, after);
    assert_eq!(
        first.scope("payment").unwrap().reason,
        second.scope("payment").unwrap().reason
    );

    let conflict = engine
        .cancel_scope("scope-idem", "payment", Some("different".into()))
        .await
        .unwrap_err();
    assert!(matches!(conflict, FlowError::ScopeConflict { .. }));
}

#[tokio::test]
async fn run_cancellation_cancels_open_scopes() {
    let engine = FlowEngine::new(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(ScopeRuntime {
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    engine
        .start_with_id("scope-run-cancel", scope_spec(), json!({}))
        .await
        .unwrap();
    let snapshot = engine
        .request_cancellation(
            "scope-run-cancel",
            CancellationRequest::new(Some("operator".into())),
        )
        .await
        .unwrap();
    assert!(snapshot.scope("payment").unwrap().is_cancelled());
}

struct CrashBeforeScopeDriveStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashBeforeScopeDriveStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashBeforeScopeDriveStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        if matches!(event, FlowEvent::RunCompleted { .. })
            && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash before scope-cancel drive became durable".into(),
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

#[tokio::test]
async fn scope_cancel_redelivery_recovers_after_scope_cancelled_before_drive_completes() {
    let run_id = "scope-receipt-before-drive";
    let store = Arc::new(CrashBeforeScopeDriveStore::new());
    let engine = FlowEngine::new(
        store.clone(),
        Arc::new(ScopeRuntime {
            workflow_calls: AtomicUsize::new(0),
        }),
    );
    engine
        .start_with_id(run_id, scope_spec(), json!({}))
        .await
        .unwrap();

    let interrupted = engine
        .cancel_scope(run_id, "payment", Some("customer aborted".into()))
        .await
        .expect_err("losing RunCompleted after ScopeCancelled must interrupt");
    assert!(matches!(interrupted, FlowError::Store(_)));

    let mid = engine.snapshot(run_id).await.unwrap();
    assert!(!mid.status.is_terminal());
    assert!(mid.scope("payment").unwrap().is_cancelled());

    let recovered = engine
        .cancel_scope(run_id, "payment", Some("customer aborted".into()))
        .await
        .unwrap();
    assert_eq!(recovered.status, WorkflowRunStatus::Completed);
    assert_eq!(recovered.output, Some(json!({ "cancelled": true })));

    let history = store.list(run_id).await.unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(
                &envelope.event,
                FlowEvent::ScopeCancelled { scope_id, .. } if scope_id == "payment"
            ))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|envelope| matches!(envelope.event, FlowEvent::RunCompleted { .. }))
            .count(),
        1
    );
}

/// Host scope cancel must force workflow replay even when an unscoped timer
/// would otherwise short-circuit drive — same class as hook/signal wakes.
struct ScopeCancelBesideOpenWaitRuntime;

#[async_trait]
impl FlowRuntime for ScopeCancelBesideOpenWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.scope_cancelled("payment") {
            return Ok(ctx.complete(json!({ "cancelled": true })));
        }
        // Arm the unscoped timer first so its wait has no scope_id.
        if ctx.wait_status("outer").is_none() {
            return Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()));
        }
        if ctx.update("arm").is_some() && !ctx.has_scope("payment") {
            return Ok(ctx.open_scope("payment"));
        }
        // Stay suspended on the unscoped timer while the nested scope stays open.
        Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        unreachable!("scope-beside-wait runtime does not schedule steps")
    }

    async fn run_update(
        &self,
        _invocation: a3s_flow::UpdateInvocation,
    ) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "armed": true }))
    }
}

#[tokio::test]
async fn scope_cancel_wakes_workflow_while_unscoped_timer_wait_is_open() {
    let engine = FlowEngine::in_memory(Arc::new(ScopeCancelBesideOpenWaitRuntime));
    engine
        .start_with_id(
            "scope-beside-wait",
            scope_spec().with_update("arm"),
            json!({}),
        )
        .await
        .unwrap();

    let armed = engine
        .apply_update(
            "scope-beside-wait",
            a3s_flow::WorkflowUpdate::new("arm-1", "arm", json!({})),
        )
        .await
        .unwrap();
    assert_eq!(armed.snapshot.status, WorkflowRunStatus::Suspended);
    assert!(armed.snapshot.scope("payment").unwrap().is_open());
    assert_eq!(
        armed.snapshot.waits["outer"].status,
        a3s_flow::WaitStatus::Waiting
    );
    assert!(armed.snapshot.waits["outer"].scope_id.is_none());

    let after = engine
        .cancel_scope("scope-beside-wait", "payment", Some("host".into()))
        .await
        .unwrap();
    assert_eq!(
        after.status,
        WorkflowRunStatus::Completed,
        "ScopeCancelled must force replay so cleanup can observe scope_cancelled beside an open timer"
    );
    assert_eq!(after.output, Some(json!({ "cancelled": true })));
    assert!(after.scope("payment").unwrap().is_cancelled());
    assert_eq!(
        after.waits["outer"].status,
        a3s_flow::WaitStatus::Waiting,
        "unscoped timer must remain open; only the cancelled scope tree is cleaned"
    );
}

/// Workflow-emitted CancelScope must be observable via ordinary `drive()` recovery
/// when an unscoped timer would otherwise short-circuit (DriveRun path).
struct WorkflowCancelScopeBesideOpenWaitRuntime {
    cancel_phase: AtomicBool,
}

#[async_trait]
impl FlowRuntime for WorkflowCancelScopeBesideOpenWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        if ctx.scope_cancelled("payment") {
            return Ok(ctx.complete(json!({ "cancelled": true })));
        }
        if ctx.wait_status("outer").is_none() {
            return Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()));
        }
        if ctx.update("arm").is_some() && !ctx.has_scope("payment") {
            return Ok(ctx.open_scope("payment"));
        }
        if self.cancel_phase.load(Ordering::SeqCst)
            && ctx.has_scope("payment")
            && !ctx.scope_cancelled("payment")
        {
            return Ok(ctx.cancel_scope("payment", Some("workflow".into())));
        }
        Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        unreachable!("workflow-cancel-beside-wait runtime does not schedule steps")
    }

    async fn run_update(
        &self,
        _invocation: a3s_flow::UpdateInvocation,
    ) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "armed": true }))
    }
}

struct CrashAfterWorkflowScopeCancelledStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashAfterWorkflowScopeCancelledStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashAfterWorkflowScopeCancelledStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        let envelope = self
            .inner
            .append_if_sequence(run_id, expected_sequence, event.clone())
            .await?;
        if matches!(
            event,
            FlowEvent::ScopeCancelled {
                ref scope_id,
                ..
            } if scope_id == "payment"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash after workflow ScopeCancelled before observation drive".into(),
            ));
        }
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn workflow_scope_cancelled_recovery_wakes_via_drive_while_unscoped_timer_is_open() {
    let store = Arc::new(CrashAfterWorkflowScopeCancelledStore::new());
    let runtime = Arc::new(WorkflowCancelScopeBesideOpenWaitRuntime {
        cancel_phase: AtomicBool::new(false),
    });
    let engine = FlowEngine::new(store.clone(), runtime.clone());
    engine
        .start_with_id(
            "workflow-scope-cancel-drive",
            scope_spec().with_update("arm"),
            json!({}),
        )
        .await
        .unwrap();
    engine
        .apply_update(
            "workflow-scope-cancel-drive",
            a3s_flow::WorkflowUpdate::new("arm-1", "arm", json!({})),
        )
        .await
        .unwrap();

    runtime.cancel_phase.store(true, Ordering::SeqCst);
    store.armed.store(true, Ordering::SeqCst);
    let interrupted = engine
        .apply_update(
            "workflow-scope-cancel-drive",
            a3s_flow::WorkflowUpdate::new("cancel-1", "arm", json!({})),
        )
        .await
        .expect_err("crash after durable ScopeCancelled must interrupt");
    assert!(matches!(interrupted, FlowError::Store(_)));

    let mid = engine
        .snapshot("workflow-scope-cancel-drive")
        .await
        .unwrap();
    assert!(!mid.status.is_terminal());
    assert!(mid.scope("payment").unwrap().is_cancelled());
    assert_eq!(mid.waits["outer"].status, a3s_flow::WaitStatus::Waiting);

    // Ordinary DriveRun recovery — not host cancel_scope.
    let recovered = engine.drive("workflow-scope-cancel-drive").await.unwrap();
    assert_eq!(
        recovered.status,
        WorkflowRunStatus::Completed,
        "drive must observe tip ScopeCancelled beside an open unscoped timer"
    );
    assert_eq!(recovered.output, Some(json!({ "cancelled": true })));
}

/// Workflow-emitted CompleteScope must be observable via ordinary `drive()` recovery
/// when an unscoped timer would otherwise short-circuit (DriveRun path).
struct WorkflowCompleteScopeBesideOpenWaitRuntime {
    complete_phase: AtomicBool,
}

#[async_trait]
impl FlowRuntime for WorkflowCompleteScopeBesideOpenWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let scope_completed = ctx.history().iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::ScopeCompleted { scope_id, .. } if scope_id == "payment"
            )
        });
        if scope_completed {
            return Ok(ctx.complete(json!({ "closed": true })));
        }
        if ctx.wait_status("outer").is_none() {
            return Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()));
        }
        let scope_opened = ctx.history().iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::ScopeOpened { scope_id, .. } if scope_id == "payment"
            )
        });
        if ctx.update("arm").is_some() && !scope_opened {
            return Ok(ctx.open_scope("payment"));
        }
        if self.complete_phase.load(Ordering::SeqCst) && scope_opened {
            return Ok(ctx.complete_scope("payment"));
        }
        Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        unreachable!("workflow-complete-beside-wait runtime does not schedule steps")
    }

    async fn run_update(
        &self,
        _invocation: a3s_flow::UpdateInvocation,
    ) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "armed": true }))
    }
}

struct CrashAfterWorkflowScopeCompletedStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashAfterWorkflowScopeCompletedStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashAfterWorkflowScopeCompletedStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        let envelope = self
            .inner
            .append_if_sequence(run_id, expected_sequence, event.clone())
            .await?;
        if matches!(
            event,
            FlowEvent::ScopeCompleted {
                ref scope_id,
                ..
            } if scope_id == "payment"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash after workflow ScopeCompleted before observation drive".into(),
            ));
        }
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn workflow_scope_completed_recovery_wakes_via_drive_while_unscoped_timer_is_open() {
    let store = Arc::new(CrashAfterWorkflowScopeCompletedStore::new());
    let runtime = Arc::new(WorkflowCompleteScopeBesideOpenWaitRuntime {
        complete_phase: AtomicBool::new(false),
    });
    let engine = FlowEngine::new(store.clone(), runtime.clone());
    engine
        .start_with_id(
            "workflow-scope-complete-drive",
            scope_spec().with_update("arm"),
            json!({}),
        )
        .await
        .unwrap();
    engine
        .apply_update(
            "workflow-scope-complete-drive",
            a3s_flow::WorkflowUpdate::new("arm-1", "arm", json!({})),
        )
        .await
        .unwrap();

    runtime.complete_phase.store(true, Ordering::SeqCst);
    store.armed.store(true, Ordering::SeqCst);
    let interrupted = engine
        .apply_update(
            "workflow-scope-complete-drive",
            a3s_flow::WorkflowUpdate::new("close-1", "arm", json!({})),
        )
        .await
        .expect_err("crash after durable ScopeCompleted must interrupt");
    assert!(matches!(interrupted, FlowError::Store(_)));

    let mid = engine
        .snapshot("workflow-scope-complete-drive")
        .await
        .unwrap();
    assert!(!mid.status.is_terminal());
    assert_eq!(
        mid.scope("payment").unwrap().status,
        a3s_flow::CancellationScopeStatus::Completed
    );
    assert_eq!(mid.waits["outer"].status, a3s_flow::WaitStatus::Waiting);

    let recovered = engine.drive("workflow-scope-complete-drive").await.unwrap();
    assert_eq!(
        recovered.status,
        WorkflowRunStatus::Completed,
        "drive must observe tip ScopeCompleted beside an open unscoped timer"
    );
    assert_eq!(recovered.output, Some(json!({ "closed": true })));
}

/// Workflow-emitted OpenScope must be observable via ordinary `drive()` recovery
/// when an unscoped timer would otherwise short-circuit (DriveRun path).
struct WorkflowOpenScopeBesideOpenWaitRuntime {
    open_phase: AtomicBool,
}

#[async_trait]
impl FlowRuntime for WorkflowOpenScopeBesideOpenWaitRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let scope_opened = ctx.history().iter().any(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::ScopeOpened { scope_id, .. } if scope_id == "payment"
            )
        });
        if scope_opened {
            return Ok(ctx.complete(json!({ "opened": true })));
        }
        if ctx.wait_status("outer").is_none() {
            return Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()));
        }
        if self.open_phase.load(Ordering::SeqCst) && ctx.update("arm").is_some() {
            return Ok(ctx.open_scope("payment"));
        }
        Ok(ctx.wait_until("outer", "2030-01-01T00:00:00Z".parse().unwrap()))
    }

    async fn run_step(&self, _invocation: a3s_flow::StepInvocation) -> a3s_flow::Result<JsonValue> {
        unreachable!("workflow-open-beside-wait runtime does not schedule steps")
    }

    async fn run_update(
        &self,
        _invocation: a3s_flow::UpdateInvocation,
    ) -> a3s_flow::Result<JsonValue> {
        Ok(json!({ "armed": true }))
    }
}

struct CrashAfterWorkflowScopeOpenedStore {
    inner: InMemoryEventStore,
    armed: AtomicBool,
}

impl CrashAfterWorkflowScopeOpenedStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            armed: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl FlowEventStore for CrashAfterWorkflowScopeOpenedStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> a3s_flow::Result<FlowEventEnvelope> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> a3s_flow::Result<FlowEventEnvelope> {
        let envelope = self
            .inner
            .append_if_sequence(run_id, expected_sequence, event.clone())
            .await?;
        if matches!(
            event,
            FlowEvent::ScopeOpened {
                ref scope_id,
                ..
            } if scope_id == "payment"
        ) && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(FlowError::Store(
                "injected crash after workflow ScopeOpened before observation drive".into(),
            ));
        }
        Ok(envelope)
    }

    async fn list(&self, run_id: &str) -> a3s_flow::Result<Vec<FlowEventEnvelope>> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> a3s_flow::Result<Vec<String>> {
        self.inner.list_run_ids().await
    }
}

#[tokio::test]
async fn workflow_scope_opened_recovery_wakes_via_drive_while_unscoped_timer_is_open() {
    let store = Arc::new(CrashAfterWorkflowScopeOpenedStore::new());
    let runtime = Arc::new(WorkflowOpenScopeBesideOpenWaitRuntime {
        open_phase: AtomicBool::new(false),
    });
    let engine = FlowEngine::new(store.clone(), runtime.clone());
    engine
        .start_with_id(
            "workflow-scope-open-drive",
            scope_spec().with_update("arm"),
            json!({}),
        )
        .await
        .unwrap();

    // Suspend on the unscoped timer without opening a scope yet.
    assert_eq!(
        engine
            .snapshot("workflow-scope-open-drive")
            .await
            .unwrap()
            .status,
        WorkflowRunStatus::Suspended
    );

    runtime.open_phase.store(true, Ordering::SeqCst);
    store.armed.store(true, Ordering::SeqCst);
    let interrupted = engine
        .apply_update(
            "workflow-scope-open-drive",
            a3s_flow::WorkflowUpdate::new("open-1", "arm", json!({})),
        )
        .await
        .expect_err("crash after durable ScopeOpened must interrupt");
    assert!(matches!(interrupted, FlowError::Store(_)));

    let mid = engine.snapshot("workflow-scope-open-drive").await.unwrap();
    assert!(!mid.status.is_terminal());
    assert!(mid.scope("payment").unwrap().is_open());
    assert_eq!(mid.waits["outer"].status, a3s_flow::WaitStatus::Waiting);

    let recovered = engine.drive("workflow-scope-open-drive").await.unwrap();
    assert_eq!(
        recovered.status,
        WorkflowRunStatus::Completed,
        "drive must observe tip ScopeOpened beside an open unscoped timer"
    );
    assert_eq!(recovered.output, Some(json!({ "opened": true })));
}
