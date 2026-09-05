use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, validate_child_workflow_command, ChildWorkflowCommand, FlowEvent,
    FlowEventEnvelope, HookStatus, RuntimeCommand, ScheduledWakeup, StepStatus, WaitStatus,
    WorkflowRunSnapshot, WorkflowRunStatus, WorkflowSpec,
};
use crate::observe::{FlowEventObserver, NoopFlowEventObserver};
use crate::runtime::{FlowRuntime, WorkflowInvocation};
use crate::runtime_build::{RuntimeBuildCompatibility, RuntimeBuildId};
use crate::store::{FlowEventStore, FlowStoreCapabilities, InMemoryEventStore};

mod child_workflows;
mod continuation;
mod hooks;
mod inspection;
mod operations;
mod runs;
mod scheduling;
mod signals;
mod steps;
mod validation;
use signals::SignalWaitCommandOutcome;
use steps::{interrupted_terminal_event, StepExecutionContext};
use validation::{
    ensure_child_operation_matches, ensure_child_workflow_batch_valid, ensure_hook_command_matches,
    ensure_progress_matches, ensure_retry_policy_valid, ensure_step_batch_valid,
    ensure_step_command_matches, ensure_wait_command_matches, is_event_conflict,
};

const DEFAULT_MAX_CONTINUE_AS_NEW_HOPS: usize = 64;
const DEFAULT_MAX_CHILD_WORKFLOW_DEPTH: usize = 32;
const DEFAULT_OBSERVER_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct HookResolutionOutcome {
    pub(crate) hook_run_id: String,
    pub(crate) hook_id: String,
    pub(crate) snapshot: WorkflowRunSnapshot,
    pub(crate) committed: bool,
}

pub(crate) struct WaitResolutionOutcome {
    pub(crate) wait_run_id: String,
    pub(crate) wait_id: String,
    pub(crate) snapshot: WorkflowRunSnapshot,
    pub(crate) committed: bool,
}

pub(crate) struct ScheduledRunOutcome {
    pub(crate) snapshot: WorkflowRunSnapshot,
    pub(crate) due: Vec<ScheduledWakeup>,
    pub(crate) resumed_waits: Vec<(String, String)>,
}

/// Builder for a [`FlowEngine`].
pub struct FlowEngineBuilder {
    store: Arc<dyn FlowEventStore>,
    runtime: Arc<dyn FlowRuntime>,
    observer: Arc<dyn FlowEventObserver>,
    runtime_build_compatibility: Option<RuntimeBuildCompatibility>,
    max_replay_iterations: usize,
    max_continue_as_new_hops: usize,
    max_child_workflow_depth: usize,
    observer_timeout: Duration,
}

impl FlowEngineBuilder {
    /// Create a builder with an in-memory store and no-op observer.
    pub fn new(runtime: Arc<dyn FlowRuntime>) -> Self {
        Self {
            store: Arc::new(InMemoryEventStore::new()),
            runtime,
            observer: Arc::new(NoopFlowEventObserver),
            runtime_build_compatibility: None,
            max_replay_iterations: 1024,
            max_continue_as_new_hops: DEFAULT_MAX_CONTINUE_AS_NEW_HOPS,
            max_child_workflow_depth: DEFAULT_MAX_CHILD_WORKFLOW_DEPTH,
            observer_timeout: DEFAULT_OBSERVER_TIMEOUT,
        }
    }

    /// Use `store` for durable workflow histories.
    pub fn with_store(mut self, store: Arc<dyn FlowEventStore>) -> Self {
        self.store = store;
        self
    }

    /// Observe each event after it has been durably appended.
    pub fn with_observer(mut self, observer: Arc<dyn FlowEventObserver>) -> Self {
        self.observer = observer;
        self
    }

    /// Bound how long a committed-event observer may delay an engine call.
    ///
    /// The default is five seconds. A zero duration preserves the legacy
    /// unbounded wait while still isolating observer panics from durable state.
    /// When a non-zero deadline expires, the event remains committed and the
    /// observer task is cancelled after a warning is emitted.
    pub fn with_observer_timeout(mut self, timeout: Duration) -> Self {
        self.observer_timeout = timeout;
        self
    }

    /// Fence workflow execution to an explicit runtime build compatibility set.
    pub fn with_runtime_build_compatibility(
        mut self,
        compatibility: RuntimeBuildCompatibility,
    ) -> Self {
        self.runtime_build_compatibility = Some(compatibility);
        self
    }

    /// Set the maximum workflow replay iterations per drive operation.
    ///
    /// Values below one are clamped to one.
    pub fn with_max_replay_iterations(mut self, max_replay_iterations: usize) -> Self {
        self.max_replay_iterations = max_replay_iterations.max(1);
        self
    }

    /// Bound the number of continue-as-new links one drive call may follow.
    ///
    /// The default is 64. Zero disables new continuations; when the limit is
    /// reached, the engine rejects the runtime command before appending it.
    pub fn with_max_continue_as_new_hops(mut self, max_hops: usize) -> Self {
        self.max_continue_as_new_hops = max_hops;
        self
    }

    /// Bound first-class child nesting performed by one drive call.
    ///
    /// The default is 32. Zero disables new child workflows; the engine
    /// rejects a command at the boundary before appending its parent link.
    pub fn with_max_child_workflow_depth(mut self, max_depth: usize) -> Self {
        self.max_child_workflow_depth = max_depth;
        self
    }

    /// Build the configured workflow engine.
    pub fn build(self) -> FlowEngine {
        FlowEngine {
            store: self.store,
            runtime: self.runtime,
            observer: self.observer,
            runtime_build_compatibility: self.runtime_build_compatibility,
            max_replay_iterations: self.max_replay_iterations,
            max_continue_as_new_hops: self.max_continue_as_new_hops,
            max_child_workflow_depth: self.max_child_workflow_depth,
            observer_timeout: self.observer_timeout,
        }
    }
}

/// Event-sourced workflow engine.
#[derive(Clone)]
pub struct FlowEngine {
    store: Arc<dyn FlowEventStore>,
    runtime: Arc<dyn FlowRuntime>,
    observer: Arc<dyn FlowEventObserver>,
    runtime_build_compatibility: Option<RuntimeBuildCompatibility>,
    max_replay_iterations: usize,
    max_continue_as_new_hops: usize,
    max_child_workflow_depth: usize,
    observer_timeout: Duration,
}

impl FlowEngine {
    /// Create an engine builder for `runtime`.
    pub fn builder(runtime: Arc<dyn FlowRuntime>) -> FlowEngineBuilder {
        FlowEngineBuilder::new(runtime)
    }

    /// Create an engine with the supplied store, runtime, and default limits.
    pub fn new(store: Arc<dyn FlowEventStore>, runtime: Arc<dyn FlowRuntime>) -> Self {
        Self {
            store,
            runtime,
            observer: Arc::new(NoopFlowEventObserver),
            runtime_build_compatibility: None,
            max_replay_iterations: 1024,
            max_continue_as_new_hops: DEFAULT_MAX_CONTINUE_AS_NEW_HOPS,
            max_child_workflow_depth: DEFAULT_MAX_CHILD_WORKFLOW_DEPTH,
            observer_timeout: DEFAULT_OBSERVER_TIMEOUT,
        }
    }

    /// Create an engine backed by a new in-memory event store.
    pub fn in_memory(runtime: Arc<dyn FlowRuntime>) -> Self {
        Self::new(Arc::new(InMemoryEventStore::new()), runtime)
    }

    /// Clone the engine's event-store handle.
    pub fn store(&self) -> Arc<dyn FlowEventStore> {
        Arc::clone(&self.store)
    }

    /// Return the declared execution guarantees of this engine's event store.
    pub fn store_capabilities(&self) -> FlowStoreCapabilities {
        self.store.capabilities()
    }

    /// Clone the engine's event-observer handle.
    pub fn observer(&self) -> Arc<dyn FlowEventObserver> {
        Arc::clone(&self.observer)
    }

    /// Return the configured committed-event observer deadline.
    pub fn observer_timeout(&self) -> Duration {
        self.observer_timeout
    }

    /// Return this engine's explicit runtime-build admission policy.
    pub fn runtime_build_compatibility(&self) -> Option<&RuntimeBuildCompatibility> {
        self.runtime_build_compatibility.as_ref()
    }

    /// Return whether this engine can replay a pinned or legacy run.
    pub fn supports_runtime_build(&self, required_build_id: Option<&RuntimeBuildId>) -> bool {
        match &self.runtime_build_compatibility {
            Some(compatibility) => compatibility.supports(required_build_id),
            None => required_build_id.is_none(),
        }
    }

    /// Read the runtime build identity pinned by one run.
    pub async fn runtime_build_id(&self, run_id: &str) -> Result<Option<RuntimeBuildId>> {
        Ok(self.snapshot(run_id).await?.spec.runtime_build_id)
    }

    async fn drive_run_at(
        &self,
        run_id: &str,
        now: DateTime<Utc>,
        allow_continue_as_new: bool,
        child_depth: usize,
        ancestry: &BTreeSet<String>,
    ) -> Result<WorkflowRunSnapshot> {
        let mut replay_iterations = 0;
        'replay: while replay_iterations < self.max_replay_iterations {
            let history = self.store.list(run_id).await?;
            let snapshot = project_run(run_id, &history)?;
            if snapshot.status.is_terminal() {
                return Ok(snapshot);
            }
            if snapshot.status == WorkflowRunStatus::Pending {
                // Repair the lifecycle before charging the workflow replay budget.
                self.ensure_run_started(run_id, &snapshot.spec, &snapshot.input)
                    .await?;
                continue;
            }
            replay_iterations += 1;
            self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
            if let Some(event) = interrupted_terminal_event(&snapshot, &history) {
                match self
                    .record_event_at(run_id, snapshot.last_sequence, event)
                    .await
                {
                    Ok(_) => continue,
                    Err(err) if is_event_conflict(&err) => continue,
                    Err(err) => return Err(err),
                }
            }
            match self
                .reconcile_child_workflows(&snapshot, now, child_depth, ancestry)
                .await?
            {
                child_workflows::ChildReconciliation::Ready => {}
                child_workflows::ChildReconciliation::Replay => continue,
                child_workflows::ChildReconciliation::Waiting => {
                    return self.snapshot(run_id).await;
                }
            }
            match self.reconcile_signal_waits(&snapshot).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(err) if is_event_conflict(&err) => continue,
                Err(err) => return Err(err),
            }
            if snapshot
                .waits
                .values()
                .any(|wait| wait.status == WaitStatus::Waiting)
                || snapshot
                    .hooks
                    .values()
                    .any(|hook| hook.status == HookStatus::Active)
                || snapshot
                    .signal_waits
                    .values()
                    .any(|wait| wait.status == crate::model::SignalWaitStatus::Waiting)
                || (snapshot.has_future_retry(now) && snapshot.due_retries(now).is_empty())
            {
                return Ok(snapshot);
            }

            let command = self
                .runtime
                .run_workflow(WorkflowInvocation {
                    run_id: run_id.to_string(),
                    spec: snapshot.spec.clone(),
                    input: snapshot.input.clone(),
                    history,
                })
                .await?;

            match command {
                RuntimeCommand::Complete { output } => {
                    if snapshot.status == WorkflowRunStatus::Cancelling {
                        return Err(FlowError::InvalidTransition(format!(
                            "workflow run {run_id} completed after cancellation was requested; cleanup-aware cancellation must return cancel or fail"
                        )));
                    }
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::RunCompleted { output },
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                    return self.snapshot(run_id).await;
                }
                RuntimeCommand::Fail { error } => {
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::RunFailed { error },
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                    return self.snapshot(run_id).await;
                }
                RuntimeCommand::Cancel => {
                    let cancellation = snapshot.cancellation.as_ref().ok_or_else(|| {
                        FlowError::InvalidTransition(format!(
                            "workflow run {run_id} returned cancel without a durable cancellation request"
                        ))
                    })?;
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::RunCancelled {
                                reason: cancellation.request.reason.clone(),
                            },
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                    return self.snapshot(run_id).await;
                }
                RuntimeCommand::Timeout { deadline, reason } => {
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::RunTimedOut { deadline, reason },
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                    return self.snapshot(run_id).await;
                }
                RuntimeCommand::ContinueAsNew { input } => {
                    if snapshot.status == WorkflowRunStatus::Cancelling {
                        return Err(FlowError::InvalidTransition(format!(
                            "workflow run {run_id} continued as new after cancellation was requested; cleanup-aware cancellation must return cancel or fail"
                        )));
                    }
                    if let Some(signal) = snapshot
                        .signals
                        .iter()
                        .find(|signal| signal.consumed_by.is_none())
                    {
                        return Err(FlowError::InvalidTransition(format!(
                            "workflow run {run_id} cannot continue as new with unconsumed signal {}",
                            signal.signal_id
                        )));
                    }
                    if !allow_continue_as_new {
                        return Err(FlowError::ContinueAsNewLimitExceeded(
                            self.max_continue_as_new_hops,
                        ));
                    }
                    let successor_run_id = Uuid::new_v4().to_string();
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::RunContinuedAsNew {
                                successor_run_id,
                                input,
                            },
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                    return self.snapshot(run_id).await;
                }
                RuntimeCommand::RecordProgress { progress } => {
                    progress.validate()?;
                    if let Some(existing) = snapshot.progress(&progress.progress_id) {
                        ensure_progress_matches(run_id, existing, &progress)?;
                        return Err(FlowError::InvalidTransition(format!(
                            "workflow rescheduled progress {} without progress",
                            progress.progress_id
                        )));
                    }
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::RunProgressRecorded { progress },
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                RuntimeCommand::LinkChildOperation { child } => {
                    child.validate()?;
                    if let Some(existing) = snapshot.child_operation(&child.reference_id) {
                        ensure_child_operation_matches(run_id, existing, &child)?;
                        return Err(FlowError::InvalidTransition(format!(
                            "workflow rescheduled child operation {} without progress",
                            child.reference_id
                        )));
                    }
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::ChildOperationLinked { child },
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                RuntimeCommand::StartChildWorkflow {
                    child_id,
                    spec,
                    input,
                    cancellation_policy,
                } => {
                    validate_child_workflow_command(&child_id, &spec)?;
                    let child = ChildWorkflowCommand::new(child_id.clone(), spec, input)
                        .with_cancellation_policy(cancellation_policy);
                    match self
                        .persist_child_workflow_requests(
                            &snapshot,
                            std::slice::from_ref(&child),
                            child_depth,
                        )
                        .await
                    {
                        Ok(true) => continue,
                        Ok(false) => {
                            let existing = snapshot.child_workflow(&child_id).ok_or_else(|| {
                                FlowError::InvalidTransition(format!(
                                    "child workflow {child_id} disappeared after request validation"
                                ))
                            })?;
                            if existing.outcome.is_some() {
                                return Err(FlowError::InvalidTransition(format!(
                                    "workflow rescheduled resolved child workflow {child_id} without progress"
                                )));
                            }
                            continue;
                        }
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                RuntimeCommand::StartChildWorkflows { children } => {
                    ensure_child_workflow_batch_valid(&children)?;
                    match self
                        .persist_child_workflow_requests(&snapshot, &children, child_depth)
                        .await
                    {
                        Ok(true) => continue,
                        Ok(false) => {
                            if children.iter().all(|child| {
                                snapshot
                                    .child_workflow(&child.child_id)
                                    .is_some_and(|existing| existing.outcome.is_some())
                            }) {
                                let child_ids = children
                                    .iter()
                                    .map(|child| child.child_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                return Err(FlowError::InvalidTransition(format!(
                                    "workflow rescheduled only resolved child workflows without progress: {child_ids}"
                                )));
                            }
                            continue;
                        }
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                RuntimeCommand::ScheduleStep {
                    step_id,
                    step_name,
                    input,
                    retry,
                } => {
                    if let Some(step) = snapshot.steps.get(&step_id) {
                        ensure_step_command_matches(run_id, step, &step_name, &input, retry)?;
                        if matches!(
                            step.status,
                            StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled
                        ) {
                            return Err(FlowError::InvalidTransition(format!(
                                "workflow rescheduled terminal step {step_id} without progress"
                            )));
                        }
                    }
                    ensure_retry_policy_valid(retry)?;
                    match self
                        .execute_step(
                            run_id,
                            &snapshot,
                            StepExecutionContext {
                                step_id,
                                step_name,
                                input,
                                retry,
                                now,
                            },
                        )
                        .await
                    {
                        Ok(()) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                RuntimeCommand::ScheduleSteps { steps } => {
                    ensure_step_batch_valid(&steps)?;
                    for step in &steps {
                        if let Some(existing) = snapshot.steps.get(&step.step_id) {
                            ensure_step_command_matches(
                                run_id,
                                existing,
                                &step.step_name,
                                &step.input,
                                step.retry,
                            )?;
                        }
                    }
                    if steps.iter().all(|step| {
                        snapshot.steps.get(&step.step_id).is_some_and(|existing| {
                            matches!(
                                existing.status,
                                StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled
                            )
                        })
                    }) {
                        let step_ids = steps
                            .iter()
                            .map(|step| step.step_id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        return Err(FlowError::InvalidTransition(format!(
                            "workflow rescheduled only terminal steps without progress: {step_ids}"
                        )));
                    }
                    for step in &steps {
                        ensure_retry_policy_valid(step.retry)?;
                    }
                    match self.execute_step_batch(run_id, &snapshot, steps, now).await {
                        Ok(()) => {}
                        Err(err) if is_event_conflict(&err) => continue 'replay,
                        Err(err) => return Err(err),
                    }
                }
                RuntimeCommand::WaitUntil { wait_id, resume_at } => {
                    match snapshot.waits.get(&wait_id) {
                        Some(wait) => {
                            ensure_wait_command_matches(run_id, wait, resume_at)?;
                            match wait.status {
                                WaitStatus::Completed => continue,
                                WaitStatus::Waiting => return self.snapshot(run_id).await,
                                WaitStatus::Cancelled => {
                                    return Err(FlowError::InvalidTransition(format!(
                                        "workflow rescheduled cancelled wait {wait_id}; cancellation cleanup must use a distinct stable identity"
                                    )))
                                }
                            }
                        }
                        None => {
                            match self
                                .record_event_at(
                                    run_id,
                                    snapshot.last_sequence,
                                    FlowEvent::WaitCreated { wait_id, resume_at },
                                )
                                .await
                            {
                                Ok(_) => {}
                                Err(err) if is_event_conflict(&err) => continue,
                                Err(err) => return Err(err),
                            }
                            return self.snapshot(run_id).await;
                        }
                    }
                }
                RuntimeCommand::CreateHook {
                    hook_id,
                    token,
                    metadata,
                } => match snapshot.hooks.get(&hook_id) {
                    Some(hook) => {
                        ensure_hook_command_matches(run_id, hook, &token, &metadata)?;
                        match hook.status {
                            HookStatus::Received | HookStatus::Disposed => continue,
                            HookStatus::Active => return self.snapshot(run_id).await,
                            HookStatus::Cancelled => {
                                return Err(FlowError::InvalidTransition(format!(
                                    "workflow rescheduled cancelled hook {hook_id}; cancellation cleanup must use a distinct stable identity"
                                )))
                            }
                        }
                    }
                    None => {
                        match self
                            .record_hook_created_at(
                                run_id,
                                snapshot.last_sequence,
                                hook_id,
                                token,
                                metadata,
                            )
                            .await
                        {
                            Ok(_) => {}
                            Err(err) if is_event_conflict(&err) => continue,
                            Err(err) => return Err(err),
                        }
                        return self.snapshot(run_id).await;
                    }
                },
                RuntimeCommand::WaitForSignal {
                    wait_id,
                    signal_name,
                } => match self
                    .schedule_signal_wait(&snapshot, wait_id, signal_name)
                    .await
                {
                    Ok(SignalWaitCommandOutcome::Replay) => continue,
                    Ok(SignalWaitCommandOutcome::Waiting) => return self.snapshot(run_id).await,
                    Err(err) if is_event_conflict(&err) => continue,
                    Err(err) => return Err(err),
                },
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    pub(crate) fn ensure_runtime_build_available(
        &self,
        run_id: &str,
        spec: &WorkflowSpec,
    ) -> Result<()> {
        let required_build_id = spec.runtime_build_id.as_ref();
        if self.supports_runtime_build(required_build_id) {
            return Ok(());
        }
        Err(FlowError::RuntimeBuildUnavailable {
            run_id: run_id.to_string(),
            required_build_id: required_build_id.cloned(),
            current_build_id: self
                .runtime_build_compatibility
                .as_ref()
                .map(|compatibility| compatibility.current_build_id().clone()),
        })
    }

    async fn record_event_at(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope> {
        let envelope = self
            .store
            .append_validated_if_sequence(run_id, expected_sequence, event)
            .await?;
        self.observe_committed(envelope.clone()).await;
        Ok(envelope)
    }

    async fn record_hook_created_at(
        &self,
        run_id: &str,
        expected_sequence: u64,
        hook_id: String,
        token: String,
        metadata: serde_json::Value,
    ) -> Result<FlowEventEnvelope> {
        let envelope = self
            .store
            .append_hook_if_token_available(run_id, expected_sequence, hook_id, token, metadata)
            .await?;
        self.observe_committed(envelope.clone()).await;
        Ok(envelope)
    }

    async fn observe_committed(&self, envelope: FlowEventEnvelope) {
        let event_key = envelope.event.event_key().to_string();
        let run_id = envelope.run_id.clone();
        let sequence = envelope.sequence;
        let observer = Arc::clone(&self.observer);
        let mut task = tokio::spawn(async move {
            observer.observe(envelope).await;
        });

        let result = if self.observer_timeout().is_zero() {
            task.await.map_err(|error| (false, error.to_string()))
        } else {
            match tokio::time::timeout(self.observer_timeout(), &mut task).await {
                Ok(result) => result.map_err(|error| (false, error.to_string())),
                Err(_) => {
                    task.abort();
                    // Do not await the aborted handle here: a misbehaving
                    // observer may be executing a non-cooperative future, and
                    // waiting for its destructor would defeat the deadline.
                    // Tokio will drop a cooperative task at its next
                    // cancellation point; the durable transition is already
                    // independent of it.
                    Err((true, String::new()))
                }
            }
        };

        if let Err((timed_out, error)) = result {
            if timed_out {
                tracing::warn!(
                    run_id = %run_id,
                    sequence,
                    event_key = %event_key,
                    timeout_ms = self.observer_timeout().as_millis() as u64,
                    "flow event observer timed out; durable event remains committed"
                );
            } else {
                tracing::warn!(
                    run_id = %run_id,
                    sequence,
                    event_key = %event_key,
                    error = %error,
                    "flow event observer failed; durable event remains committed"
                );
            }
        }
    }
}
