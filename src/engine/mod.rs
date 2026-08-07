use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, ActiveHookSnapshot, FlowEvent, FlowEventEnvelope, HookStatus, RuntimeCommand,
    StepStatus, WaitStatus, WorkflowRunSnapshot, WorkflowRunStatus, WorkflowRunSummary,
    WorkflowRunSuspension, WorkflowSpec,
};
use crate::observe::{FlowEventObserver, NoopFlowEventObserver};
use crate::runtime::{FlowRuntime, WorkflowInvocation};
use crate::store::{FlowEventStore, InMemoryEventStore};

mod operations;
mod steps;
mod validation;
use steps::StepExecutionContext;
use validation::{
    ensure_child_operation_matches, ensure_hook_command_matches, ensure_progress_matches,
    ensure_same_start, ensure_step_batch_valid, ensure_step_command_matches,
    ensure_wait_command_matches, is_event_conflict, validate_run_id,
};

/// Builder for a [`FlowEngine`].
pub struct FlowEngineBuilder {
    store: Arc<dyn FlowEventStore>,
    runtime: Arc<dyn FlowRuntime>,
    observer: Arc<dyn FlowEventObserver>,
    max_replay_iterations: usize,
}

impl FlowEngineBuilder {
    pub fn new(runtime: Arc<dyn FlowRuntime>) -> Self {
        Self {
            store: Arc::new(InMemoryEventStore::new()),
            runtime,
            observer: Arc::new(NoopFlowEventObserver),
            max_replay_iterations: 1024,
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

    pub fn with_max_replay_iterations(mut self, max_replay_iterations: usize) -> Self {
        self.max_replay_iterations = max_replay_iterations.max(1);
        self
    }

    pub fn build(self) -> FlowEngine {
        FlowEngine {
            store: self.store,
            runtime: self.runtime,
            observer: self.observer,
            max_replay_iterations: self.max_replay_iterations,
        }
    }
}

/// Event-sourced workflow engine.
#[derive(Clone)]
pub struct FlowEngine {
    store: Arc<dyn FlowEventStore>,
    runtime: Arc<dyn FlowRuntime>,
    observer: Arc<dyn FlowEventObserver>,
    max_replay_iterations: usize,
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
            max_replay_iterations: 1024,
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

    /// Start a workflow run and drive it until completion or suspension.
    pub async fn start(&self, spec: WorkflowSpec, input: serde_json::Value) -> Result<String> {
        let run_id = Uuid::new_v4().to_string();
        self.start_with_id(run_id, spec, input).await
    }

    /// Start a workflow run using a caller-provided durable run id.
    ///
    /// Reusing the same `run_id` with the same workflow spec and input is
    /// idempotent. Reusing it with different spec or input returns a conflict.
    pub async fn start_with_id(
        &self,
        run_id: impl Into<String>,
        spec: WorkflowSpec,
        input: serde_json::Value,
    ) -> Result<String> {
        spec.validate()?;
        let run_id = run_id.into();
        validate_run_id(&run_id)?;

        for _ in 0..self.max_replay_iterations {
            match self.store.list(&run_id).await {
                Ok(history) => {
                    let snapshot = project_run(&run_id, &history)?;
                    ensure_same_start(&run_id, &snapshot, &spec, &input)?;
                    if !history
                        .iter()
                        .any(|event| matches!(event.event, FlowEvent::RunStarted))
                    {
                        match self
                            .record_event_at(&run_id, snapshot.last_sequence, FlowEvent::RunStarted)
                            .await
                        {
                            Ok(_) => {}
                            Err(err) if is_event_conflict(&err) => continue,
                            Err(err) => return Err(err),
                        }
                    }
                    match self.drive(&run_id).await {
                        Ok(_) => return Ok(run_id),
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                Err(FlowError::RunNotFound(_)) => {
                    let created = match self
                        .record_event_at(
                            &run_id,
                            0,
                            FlowEvent::RunCreated {
                                spec: spec.clone(),
                                input: input.clone(),
                            },
                        )
                        .await
                    {
                        Ok(created) => created,
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    };
                    match self
                        .record_event_at(&run_id, created.sequence, FlowEvent::RunStarted)
                        .await
                    {
                        Ok(_) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                    match self.drive(&run_id).await {
                        Ok(_) => return Ok(run_id),
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                Err(err) => return Err(err),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Resume a wait once its timer has fired.
    pub async fn resume_wait(&self, run_id: &str, wait_id: &str) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Err(FlowError::RunTerminal(run_id.to_string()));
            }
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

    /// Resume an active hook with external payload.
    pub async fn resume_hook(
        &self,
        run_id: &str,
        hook_id: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Err(FlowError::RunTerminal(run_id.to_string()));
            }
            match snapshot.hooks.get(hook_id) {
                Some(hook) if hook.status == HookStatus::Active => {
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::HookReceived {
                                hook_id: hook_id.to_string(),
                                payload: payload.clone(),
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
                        "hook {hook_id} does not exist for run {run_id}"
                    )))
                }
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Dispose an active hook without accepting a callback payload.
    ///
    /// This is useful when a host withdraws an approval request, expires a
    /// webhook token, or closes an external callback route. The workflow is
    /// driven after the disposal event so replay code can observe
    /// [`WorkflowContext::hook_disposed`](crate::context::WorkflowContext::hook_disposed)
    /// and complete, fail, or schedule an alternate path.
    pub async fn dispose_hook(&self, run_id: &str, hook_id: &str) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Err(FlowError::RunTerminal(run_id.to_string()));
            }
            match snapshot.hooks.get(hook_id) {
                Some(hook) if hook.status == HookStatus::Active => {
                    match self
                        .record_event_at(
                            run_id,
                            snapshot.last_sequence,
                            FlowEvent::HookDisposed {
                                hook_id: hook_id.to_string(),
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
                        "hook {hook_id} does not exist for run {run_id}"
                    )))
                }
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Resume an active hook by its external token.
    ///
    /// This is the API webhook handlers normally want: the callback receives a
    /// token, while `run_id` and `hook_id` remain engine internals.
    pub async fn resume_hook_by_token(
        &self,
        token: &str,
        payload: serde_json::Value,
    ) -> Result<(String, String)> {
        let mut matches = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            let snapshot = self.snapshot(&run_id).await?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for hook in snapshot.hooks.values() {
                if hook.status == HookStatus::Active && hook.token == token {
                    matches.push((run_id.clone(), hook.hook_id.clone()));
                }
            }
        }

        match matches.len() {
            0 => Err(FlowError::HookTokenNotFound(token.to_string())),
            1 => {
                let (run_id, hook_id) = matches.remove(0);
                self.resume_hook(&run_id, &hook_id, payload).await?;
                Ok((run_id, hook_id))
            }
            _ => Err(FlowError::InvalidTransition(format!(
                "hook token {token:?} is active in multiple runs"
            ))),
        }
    }

    /// Dispose an active hook by its external token.
    ///
    /// This mirrors [`resume_hook_by_token`](Self::resume_hook_by_token) for
    /// callback routers that only know the public token.
    pub async fn dispose_hook_by_token(&self, token: &str) -> Result<(String, String)> {
        let mut matches = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            let snapshot = self.snapshot(&run_id).await?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for hook in snapshot.hooks.values() {
                if hook.status == HookStatus::Active && hook.token == token {
                    matches.push((run_id.clone(), hook.hook_id.clone()));
                }
            }
        }

        match matches.len() {
            0 => Err(FlowError::HookTokenNotFound(token.to_string())),
            1 => {
                let (run_id, hook_id) = matches.remove(0);
                self.dispose_hook(&run_id, &hook_id).await?;
                Ok((run_id, hook_id))
            }
            _ => Err(FlowError::InvalidTransition(format!(
                "hook token {token:?} is active in multiple runs"
            ))),
        }
    }

    /// List active waits whose `resume_at` is at or before `now`.
    ///
    /// Scheduler integrations can use this to inspect due timers before
    /// deciding how aggressively to drive them.
    pub async fn list_due_waits(&self, now: DateTime<Utc>) -> Result<Vec<(String, String)>> {
        let mut due = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            let snapshot = self.snapshot(&run_id).await?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for wait in snapshot.waits.values() {
                if wait.status == WaitStatus::Waiting && wait.resume_at <= now {
                    due.push((run_id.clone(), wait.wait_id.clone()));
                }
            }
        }
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
        let mut due = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            let snapshot = self.snapshot(&run_id).await?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for (step_id, _) in snapshot.due_retries(now) {
                due.push((run_id.clone(), step_id));
            }
        }
        due.sort();
        Ok(due)
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

    /// List open waits, active hooks, and pending delayed retries.
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
        let mut hooks = Vec::new();
        for run_id in self.store.list_run_ids().await? {
            let snapshot = self.snapshot(&run_id).await?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for hook in snapshot.hooks.values() {
                if hook.status == HookStatus::Active {
                    hooks.push(ActiveHookSnapshot {
                        run_id: run_id.clone(),
                        hook: hook.clone(),
                    });
                }
            }
        }
        hooks.sort_by(|left, right| {
            (left.run_id.as_str(), left.hook.hook_id.as_str())
                .cmp(&(right.run_id.as_str(), right.hook.hook_id.as_str()))
        });
        Ok(hooks)
    }

    /// Replay and dispatch until the run reaches a terminal state or an open
    /// wait/hook suspension.
    pub async fn drive(&self, run_id: &str) -> Result<WorkflowRunSnapshot> {
        self.drive_at(run_id, Utc::now()).await
    }

    async fn drive_at(&self, run_id: &str, now: DateTime<Utc>) -> Result<WorkflowRunSnapshot> {
        'replay: for _ in 0..self.max_replay_iterations {
            let history = self.store.list(run_id).await?;
            let snapshot = project_run(run_id, &history)?;
            if snapshot.status.is_terminal()
                || snapshot
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

    async fn terminate_run(&self, run_id: &str, event: FlowEvent) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Ok(());
            }
            match self
                .record_event_at(run_id, snapshot.last_sequence, event.clone())
                .await
            {
                Ok(_) => return Ok(()),
                Err(err) if is_event_conflict(&err) => continue,
                Err(err) => return Err(err),
            }
        }
        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
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
        for existing_run_id in self.store.list_run_ids().await? {
            let snapshot = self.snapshot(&existing_run_id).await?;
            if snapshot.status.is_terminal() {
                continue;
            }
            for hook in snapshot.hooks.values() {
                if hook.status != HookStatus::Active || hook.token != token {
                    continue;
                }
                if existing_run_id == run_id && hook.hook_id == hook_id {
                    continue;
                }
                return Err(FlowError::HookTokenConflict {
                    token: token.to_string(),
                    existing_run_id,
                    existing_hook_id: hook.hook_id.clone(),
                });
            }
        }
        Ok(())
    }
}
