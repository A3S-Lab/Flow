use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, validate_child_workflow_command, ActiveHookSnapshot, FlowEvent, FlowEventEnvelope,
    HookStatus, RuntimeCommand, ScheduledWakeup, ScheduledWakeupKind, StepStatus, WaitStatus,
    WorkflowRunSnapshot, WorkflowRunStatus, WorkflowRunSummary, WorkflowRunSuspension,
    WorkflowSpec,
};
use crate::observe::{FlowEventObserver, NoopFlowEventObserver};
use crate::runtime::{FlowRuntime, WorkflowInvocation};
use crate::runtime_build::{RuntimeBuildCompatibility, RuntimeBuildId};
use crate::store::{scheduled_wakeups_for_snapshot, FlowEventStore, InMemoryEventStore};

mod child_workflows;
mod continuation;
mod hooks;
mod operations;
mod runs;
mod steps;
mod validation;
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

    /// Resume a wait once its timer has fired.
    pub async fn resume_wait(&self, run_id: &str, wait_id: &str) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Err(FlowError::RunTerminal(run_id.to_string()));
            }
            self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
            match snapshot.waits.get(wait_id) {
                Some(wait) if wait.status == WaitStatus::Waiting => {
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::WaitCompleted {
                                wait_id: wait_id.to_string(),
                            },
                        )
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                    match self.drive(run_id).await {
                        Ok(_) => return Ok(()),
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                Some(_) => match self.drive(run_id).await {
                    Ok(_) => return Ok(()),
                    Err(err) if is_event_conflict(&err) => continue,
                    Err(err) => return Err(err),
                },
                None => {
                    return Err(FlowError::InvalidTransition(format!(
                        "wait {wait_id} does not exist for run {run_id}"
                    )))
                }
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// List active waits whose `resume_at` is at or before `now`.
    ///
    /// Scheduler integrations can use this to inspect due timers before
    /// deciding how aggressively to drive them.
    pub async fn list_due_waits(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let mut due = self
            .list_due_wakeups(now)
            .await?
            .into_iter()
            .filter(|wakeup| wakeup.kind == ScheduledWakeupKind::Wait)
            .map(|wakeup| (wakeup.run_id, wakeup.subject_id))
            .collect::<Vec<_>>();
        due.sort();
        Ok(due)
    }

    /// Complete every due wait and drive the affected workflows.
    ///
    /// Returns the `(run_id, wait_id)` pairs that were resumed. A wait already
    /// completed by another caller is skipped by [`Self::resume_wait`].
    pub async fn resume_due_waits(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let due = self.list_due_waits(now).await?;
        let mut resumed = Vec::with_capacity(due.len());
        for (run_id, wait_id) in due {
            self.resume_wait(&run_id, &wait_id).await?;
            resumed.push((run_id, wait_id));
        }
        Ok(resumed)
    }

    /// List pending step retries whose `retry_after` is at or before `now`.
    pub async fn list_due_retries(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let mut due = self
            .list_due_wakeups(now)
            .await?
            .into_iter()
            .filter(|wakeup| wakeup.kind == ScheduledWakeupKind::Retry)
            .map(|wakeup| (wakeup.run_id, wakeup.subject_id))
            .collect::<Vec<_>>();
        due.sort();
        Ok(due)
    }

    /// List all due wait timers and delayed retries through the store boundary.
    pub async fn list_due_wakeups(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledWakeup>> {
        let mut wakeups = self.store.list_due_wakeups(now).await?;
        wakeups.sort_by(|left, right| {
            (left.kind, left.run_id.as_str(), left.subject_id.as_str()).cmp(&(
                right.kind,
                right.run_id.as_str(),
                right.subject_id.as_str(),
            ))
        });
        Ok(wakeups)
    }

    /// Drive every run with a due step retry.
    pub async fn resume_due_retries(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let due = self.list_due_retries(now).await?;
        let mut run_ids = Vec::new();
        for (run_id, _) in &due {
            if !run_ids.contains(run_id) {
                run_ids.push(run_id.clone());
            }
        }
        for run_id in run_ids {
            self.drive_at(&run_id, now).await?;
        }
        Ok(due)
    }

    /// Resume the due waits and delayed retries for one targeted run.
    ///
    /// Unlike the compatibility-wide `resume_due_*` methods, this path loads
    /// only `run_id` and never performs another global due-wakeup query. The
    /// returned records describe the wakeups that were still due when the task
    /// began handling.
    pub async fn resume_scheduled_run(
        &self,
        run_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<ScheduledWakeup>> {
        let history = self.store.list(run_id).await?;
        let snapshot = project_run(run_id, &history)?;
        if snapshot.status.is_terminal() {
            return Ok(Vec::new());
        }
        self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
        let due = scheduled_wakeups_for_snapshot(&snapshot)
            .into_iter()
            .filter(|wakeup| wakeup.scheduled_at <= now)
            .collect::<Vec<_>>();

        let due_wait_ids = due
            .iter()
            .filter(|wakeup| wakeup.kind == ScheduledWakeupKind::Wait)
            .map(|wakeup| wakeup.subject_id.clone())
            .collect::<Vec<_>>();
        let has_due_retries = due
            .iter()
            .any(|wakeup| wakeup.kind == ScheduledWakeupKind::Retry);

        for wait_id in due_wait_ids {
            self.resume_wait(run_id, &wait_id).await?;
        }
        if has_due_retries {
            self.drive_at(run_id, now).await?;
        }

        Ok(due)
    }

    pub async fn snapshot(&self, run_id: &str) -> Result<WorkflowRunSnapshot> {
        let history = self.store.list(run_id).await?;
        project_run(run_id, &history)
    }

    pub async fn history(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>> {
        self.store.list(run_id).await
    }

    pub async fn list_run_ids(&self) -> Result<Vec<String>> {
        self.store.list_run_ids().await
    }

    pub async fn list_snapshots(&self) -> Result<Vec<WorkflowRunSnapshot>> {
        let mut snapshots = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            snapshots.push(self.snapshot(&run_id).await?);
        }
        Ok(snapshots)
    }

    /// Summarize run state across the active store.
    ///
    /// Suspension counters include only non-terminal runs, so a cancelled run
    /// that still has an old wait or hook in history is not reported as
    /// actionable work.
    pub async fn run_summary(&self) -> Result<WorkflowRunSummary> {
        let snapshots = self.list_snapshots().await?;
        Ok(WorkflowRunSummary::from_snapshots(&snapshots))
    }

    /// List open waits, active hooks, pending delayed retries, and child runs.
    ///
    /// The `due` flag on wait and retry suspensions is computed against `now`.
    /// Terminal runs are skipped so cancelled histories do not produce
    /// actionable operator work.
    pub async fn list_open_suspensions(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<WorkflowRunSuspension>> {
        let mut suspensions = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            let snapshot = self.snapshot(&run_id).await?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for wait in snapshot.waits.values() {
                if wait.status == WaitStatus::Waiting {
                    suspensions.push(WorkflowRunSuspension::Wait {
                        run_id: run_id.clone(),
                        wait: wait.clone(),
                        due: wait.resume_at <= now,
                    });
                }
            }
            for hook in snapshot.hooks.values() {
                if hook.status == HookStatus::Active {
                    suspensions.push(WorkflowRunSuspension::Hook {
                        run_id: run_id.clone(),
                        hook: hook.clone(),
                    });
                }
            }
            for step in snapshot.steps.values() {
                if step.status == StepStatus::Pending {
                    if let Some(retry_after) = step.retry_after {
                        suspensions.push(WorkflowRunSuspension::Retry {
                            run_id: run_id.clone(),
                            step: step.clone(),
                            due: retry_after <= now,
                        });
                    }
                }
            }
            for child in snapshot.child_workflows.values() {
                if child.is_open() {
                    suspensions.push(WorkflowRunSuspension::ChildWorkflow {
                        run_id: run_id.clone(),
                        child: child.clone(),
                    });
                }
            }
        }
        suspensions.sort_by(|left, right| {
            (left.run_id(), left.kind_order(), left.subject_id()).cmp(&(
                right.run_id(),
                right.kind_order(),
                right.subject_id(),
            ))
        });
        Ok(suspensions)
    }

    /// Return the earliest open wait or delayed retry across non-terminal runs.
    ///
    /// This is useful for hosts that want to sleep until the next scheduler tick
    /// instead of polling at a fixed interval. Active hooks are intentionally
    /// ignored because they do not have a scheduled wake-up time.
    pub async fn next_wakeup(&self, now: DateTime<Utc>) -> Result<Option<WorkflowRunSuspension>> {
        for _ in 0..2 {
            let Some(wakeup) = self.store.next_scheduled_wakeup().await? else {
                return Ok(None);
            };
            match self.snapshot(&wakeup.run_id).await {
                Ok(snapshot) => {
                    if let Some(suspension) = resolve_scheduled_wakeup(&snapshot, &wakeup, now) {
                        return Ok(Some(suspension));
                    }
                }
                Err(FlowError::RunNotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        self.next_wakeup_by_replay(now).await
    }

    async fn next_wakeup_by_replay(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<WorkflowRunSuspension>> {
        let mut wakeups = self.list_open_suspensions(now).await?;
        wakeups.retain(|suspension| suspension.scheduled_at().is_some());
        wakeups.sort_by(|left, right| {
            (
                left.scheduled_at(),
                left.run_id(),
                left.kind_order(),
                left.subject_id(),
            )
                .cmp(&(
                    right.scheduled_at(),
                    right.run_id(),
                    right.kind_order(),
                    right.subject_id(),
                ))
        });
        Ok(wakeups.into_iter().next())
    }

    /// List active external callback hooks across non-terminal runs.
    ///
    /// Callback routers and dashboards can use this to discover public hook
    /// tokens and their audit metadata without projecting every run manually.
    /// The result is sorted by run ID and hook ID for stable polling output.
    pub async fn list_active_hooks(&self) -> Result<Vec<ActiveHookSnapshot>> {
        self.store.list_active_hooks().await
    }

    async fn drive_run_at(
        &self,
        run_id: &str,
        now: DateTime<Utc>,
        allow_continue_as_new: bool,
        child_depth: usize,
        ancestry: &BTreeSet<String>,
    ) -> Result<WorkflowRunSnapshot> {
        'replay: for _ in 0..self.max_replay_iterations {
            let history = self.store.list(run_id).await?;
            let snapshot = project_run(run_id, &history)?;
            if snapshot.status.is_terminal() {
                return Ok(snapshot);
            }
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
            if snapshot
                .waits
                .values()
                .any(|wait| wait.status == WaitStatus::Waiting)
                || snapshot
                    .hooks
                    .values()
                    .any(|hook| hook.status == HookStatus::Active)
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

fn resolve_scheduled_wakeup(
    snapshot: &WorkflowRunSnapshot,
    wakeup: &ScheduledWakeup,
    now: DateTime<Utc>,
) -> Option<WorkflowRunSuspension> {
    if snapshot.run_id != wakeup.run_id || snapshot.status.is_terminal() {
        return None;
    }
    match wakeup.kind {
        ScheduledWakeupKind::Wait => {
            let wait = snapshot.waits.get(&wakeup.subject_id)?;
            if wait.status != WaitStatus::Waiting || wait.resume_at != wakeup.scheduled_at {
                return None;
            }
            Some(WorkflowRunSuspension::Wait {
                run_id: wakeup.run_id.clone(),
                wait: wait.clone(),
                due: wakeup.scheduled_at <= now,
            })
        }
        ScheduledWakeupKind::Retry => {
            let step = snapshot.steps.get(&wakeup.subject_id)?;
            if step.status != StepStatus::Pending || step.retry_after != Some(wakeup.scheduled_at) {
                return None;
            }
            Some(WorkflowRunSuspension::Retry {
                run_id: wakeup.run_id.clone(),
                step: step.clone(),
                due: wakeup.scheduled_at <= now,
            })
        }
    }
}
