use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, validate_child_workflow_command, FlowEvent, FlowEventEnvelope, HookStatus,
    RuntimeCommand, StepStatus, WaitStatus, WorkflowRunSnapshot, WorkflowRunStatus, WorkflowSpec,
};
use crate::observe::{FlowEventObserver, NoopFlowEventObserver};
use crate::runtime::{FlowRuntime, WorkflowInvocation};
use crate::runtime_build::{RuntimeBuildCompatibility, RuntimeBuildId};
use crate::store::{FlowEventStore, InMemoryEventStore};

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
use steps::{interrupted_retry_exhaustion_event, StepExecutionContext};
use validation::{
    ensure_child_operation_matches, ensure_child_workflow_command_matches,
    ensure_hook_command_matches, ensure_progress_matches, ensure_retry_policy_valid,
    ensure_step_batch_valid, ensure_step_command_matches, ensure_wait_command_matches,
    is_event_conflict,
};

const DEFAULT_MAX_CONTINUE_AS_NEW_HOPS: usize = 64;
const DEFAULT_MAX_CHILD_WORKFLOW_DEPTH: usize = 32;

/// Builder for a [`FlowEngine`].
pub struct FlowEngineBuilder {
    store: Arc<dyn FlowEventStore>,
    runtime: Arc<dyn FlowRuntime>,
    observer: Arc<dyn FlowEventObserver>,
    runtime_build_compatibility: Option<RuntimeBuildCompatibility>,
    max_replay_iterations: usize,
    max_continue_as_new_hops: usize,
    max_child_workflow_depth: usize,
}

impl FlowEngineBuilder {
    pub fn new(runtime: Arc<dyn FlowRuntime>) -> Self {
        Self {
            store: Arc::new(InMemoryEventStore::new()),
            runtime,
            observer: Arc::new(NoopFlowEventObserver),
            runtime_build_compatibility: None,
            max_replay_iterations: 1024,
            max_continue_as_new_hops: DEFAULT_MAX_CONTINUE_AS_NEW_HOPS,
            max_child_workflow_depth: DEFAULT_MAX_CHILD_WORKFLOW_DEPTH,
        }
    }

    pub fn with_store(mut self, store: Arc<dyn FlowEventStore>) -> Self {
        self.store = store;
        self
    }

    pub fn with_observer(mut self, observer: Arc<dyn FlowEventObserver>) -> Self {
        self.observer = observer;
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

    pub fn build(self) -> FlowEngine {
        FlowEngine {
            store: self.store,
            runtime: self.runtime,
            observer: self.observer,
            runtime_build_compatibility: self.runtime_build_compatibility,
            max_replay_iterations: self.max_replay_iterations,
            max_continue_as_new_hops: self.max_continue_as_new_hops,
            max_child_workflow_depth: self.max_child_workflow_depth,
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
}

impl FlowEngine {
    pub fn builder(runtime: Arc<dyn FlowRuntime>) -> FlowEngineBuilder {
        FlowEngineBuilder::new(runtime)
    }

    pub fn new(store: Arc<dyn FlowEventStore>, runtime: Arc<dyn FlowRuntime>) -> Self {
        Self {
            store,
            runtime,
            observer: Arc::new(NoopFlowEventObserver),
            runtime_build_compatibility: None,
            max_replay_iterations: 1024,
            max_continue_as_new_hops: DEFAULT_MAX_CONTINUE_AS_NEW_HOPS,
            max_child_workflow_depth: DEFAULT_MAX_CHILD_WORKFLOW_DEPTH,
        }
    }

    pub fn in_memory(runtime: Arc<dyn FlowRuntime>) -> Self {
        Self::new(Arc::new(InMemoryEventStore::new()), runtime)
    }

    pub fn store(&self) -> Arc<dyn FlowEventStore> {
        Arc::clone(&self.store)
    }

    pub fn observer(&self) -> Arc<dyn FlowEventObserver> {
        Arc::clone(&self.observer)
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
            if let Some(event) = interrupted_retry_exhaustion_event(&snapshot, &history) {
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
                    if let Some(existing) = snapshot.child_workflow(&child_id) {
                        ensure_child_workflow_command_matches(
                            run_id,
                            existing,
                            &spec,
                            &input,
                            cancellation_policy,
                        )?;
                        if existing.outcome.is_some() {
                            return Err(FlowError::InvalidTransition(format!(
                                "workflow rescheduled resolved child workflow {child_id} without progress"
                            )));
                        }
                        continue;
                    }
                    if child_depth >= self.max_child_workflow_depth {
                        return Err(FlowError::ChildWorkflowDepthExceeded(
                            self.max_child_workflow_depth,
                        ));
                    }
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::ChildWorkflowRequested {
                                child_id,
                                child_run_id: Uuid::new_v4().to_string(),
                                spec,
                                input,
                                cancellation_policy,
                            },
                        )
                        .await
                    {
                        Ok(_) => continue,
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
                        self.ensure_hook_token_available(run_id, &hook_id, &token)
                            .await?;
                        match self
                            .record_event_at(
                                run_id,
                                snapshot.last_sequence,
                                FlowEvent::HookCreated {
                                    hook_id,
                                    token,
                                    metadata,
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
            .append_if_sequence(run_id, expected_sequence, event)
            .await?;
        self.observer.observe(envelope.clone()).await;
        Ok(envelope)
    }

    async fn ensure_hook_token_available(
        &self,
        run_id: &str,
        hook_id: &str,
        token: &str,
    ) -> Result<()> {
        for active in self.store.find_active_hooks_by_token(token).await? {
            if active.run_id == run_id && active.hook.hook_id == hook_id {
                continue;
            }
            return Err(FlowError::HookTokenConflict {
                token: token.to_string(),
                existing_run_id: active.run_id,
                existing_hook_id: active.hook.hook_id,
            });
        }
        Ok(())
    }
}
