use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus, RetryPolicy,
    RuntimeCommand, StepCommand, StepFailureAction, StepSnapshot, StepStatus, WaitSnapshot,
    WaitStatus, WorkflowRunSnapshot, WorkflowSpec,
};
use crate::observe::{FlowEventObserver, NoopFlowEventObserver};
use crate::runtime::{FlowRuntime, StepInvocation, WorkflowInvocation};
use crate::store::{FlowEventStore, InMemoryEventStore};

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
    /// completed by another caller is skipped by [`resume_wait`].
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

    pub async fn cancel(&self, run_id: &str, reason: Option<String>) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            if snapshot.status.is_terminal() {
                return Ok(());
            }
            match self
                .record_event_at(
                    run_id,
                    snapshot.last_sequence,
                    FlowEvent::RunCancelled {
                        reason: reason.clone(),
                    },
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(err) if is_event_conflict(&err) => continue,
                Err(err) => return Err(err),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
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
                || snapshot.has_future_retry(now)
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
                RuntimeCommand::ScheduleStep {
                    step_id,
                    step_name,
                    input,
                    retry,
                } => {
                    match self
                        .execute_step(run_id, &snapshot, step_id, step_name, input, retry)
                        .await
                    {
                        Ok(()) => {}
                        Err(err) if is_event_conflict(&err) => continue,
                        Err(err) => return Err(err),
                    }
                }
                RuntimeCommand::ScheduleSteps { steps } => {
                    ensure_step_batch_valid(&steps)?;
                    for step in steps {
                        let current_snapshot = self.snapshot(run_id).await?;
                        match self
                            .execute_step(
                                run_id,
                                &current_snapshot,
                                step.step_id,
                                step.step_name,
                                step.input,
                                step.retry,
                            )
                            .await
                        {
                            Ok(()) => {}
                            Err(err) if is_event_conflict(&err) => continue 'replay,
                            Err(err) => return Err(err),
                        }
                    }
                }
                RuntimeCommand::WaitUntil { wait_id, resume_at } => {
                    match snapshot.waits.get(&wait_id) {
                        Some(wait) => {
                            ensure_wait_command_matches(run_id, wait, resume_at)?;
                            if wait.status == WaitStatus::Completed {
                                continue;
                            }
                            return self.snapshot(run_id).await;
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
                        if matches!(hook.status, HookStatus::Received | HookStatus::Disposed) {
                            continue;
                        }
                        return self.snapshot(run_id).await;
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

    async fn execute_step(
        &self,
        run_id: &str,
        snapshot: &WorkflowRunSnapshot,
        step_id: String,
        step_name: String,
        input: serde_json::Value,
        retry: crate::model::RetryPolicy,
    ) -> Result<()> {
        let mut expected_sequence = snapshot.last_sequence;
        if let Some(step) = snapshot.steps.get(&step_id) {
            ensure_step_command_matches(run_id, step, &step_name, &input, retry)?;
            if matches!(step.status, StepStatus::Completed | StepStatus::Failed) {
                return Ok(());
            }
        } else {
            let envelope = self
                .record_event_at(
                    run_id,
                    expected_sequence,
                    FlowEvent::StepCreated {
                        step_id: step_id.clone(),
                        step_name: step_name.clone(),
                        input: input.clone(),
                        retry,
                    },
                )
                .await?;
            expected_sequence = envelope.sequence;
        }

        let max_attempts = retry.max_attempts.max(1);
        let mut attempt = snapshot
            .steps
            .get(&step_id)
            .map(|step| step.attempt)
            .unwrap_or(0);

        loop {
            attempt += 1;
            let started = self
                .record_event_at(
                    run_id,
                    expected_sequence,
                    FlowEvent::StepStarted {
                        step_id: step_id.clone(),
                        attempt,
                    },
                )
                .await?;
            expected_sequence = started.sequence;

            let history = self.store.list(run_id).await?;
            let invocation = StepInvocation {
                run_id: run_id.to_string(),
                step_id: step_id.clone(),
                step_name: step_name.clone(),
                input: input.clone(),
                history,
            };

            match self.runtime.run_step(invocation).await {
                Ok(output) => {
                    self.record_event_at(
                        run_id,
                        expected_sequence,
                        FlowEvent::StepCompleted { step_id, output },
                    )
                    .await?;
                    return Ok(());
                }
                Err(err) if attempt < max_attempts => {
                    let retry_after = if retry.delay_ms > 0 {
                        Some(Utc::now() + ChronoDuration::milliseconds(retry.delay_ms as i64))
                    } else {
                        None
                    };
                    let retrying = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepRetrying {
                                step_id: step_id.clone(),
                                attempt,
                                error: err.to_string(),
                                retry_after,
                            },
                        )
                        .await?;
                    expected_sequence = retrying.sequence;
                    if retry_after.is_some() {
                        return Ok(());
                    }
                }
                Err(err) => {
                    let error = err.to_string();
                    let failed = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepFailed {
                                step_id: step_id.clone(),
                                attempt,
                                error: error.clone(),
                            },
                        )
                        .await?;
                    if retry.on_exhausted == StepFailureAction::ContinueWorkflow {
                        return Ok(());
                    }
                    self.record_event_at(run_id, failed.sequence, FlowEvent::RunFailed { error })
                        .await?;
                    return Ok(());
                }
            }
        }
    }
}

fn validate_run_id(run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || !run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(FlowError::InvalidRunId(run_id.to_string()));
    }
    Ok(())
}

fn ensure_same_start(
    run_id: &str,
    snapshot: &WorkflowRunSnapshot,
    spec: &WorkflowSpec,
    input: &serde_json::Value,
) -> Result<()> {
    if snapshot.spec != *spec {
        return Err(FlowError::RunConflict {
            run_id: run_id.to_string(),
            reason: "workflow spec differs".to_string(),
        });
    }
    if snapshot.input != *input {
        return Err(FlowError::RunConflict {
            run_id: run_id.to_string(),
            reason: "workflow input differs".to_string(),
        });
    }
    Ok(())
}

fn ensure_step_batch_valid(steps: &[StepCommand]) -> Result<()> {
    if steps.is_empty() {
        return Err(FlowError::InvalidTransition(
            "schedule_steps requires at least one step".to_string(),
        ));
    }

    let mut ids = BTreeSet::new();
    for step in steps {
        if step.step_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "scheduled step id must not be empty".to_string(),
            ));
        }
        if !ids.insert(step.step_id.as_str()) {
            return Err(FlowError::InvalidTransition(format!(
                "schedule_steps contains duplicate step id {}",
                step.step_id
            )));
        }
    }

    Ok(())
}

fn ensure_step_command_matches(
    run_id: &str,
    step: &StepSnapshot,
    step_name: &str,
    input: &serde_json::Value,
    retry: RetryPolicy,
) -> Result<()> {
    if step.step_name != step_name {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "step {} name differs: {}",
                step.step_id,
                replay_diff(&step.step_name, &step_name.to_string())
            ),
        });
    }
    if step.input != *input {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "step {} input differs: {}",
                step.step_id,
                replay_diff(&step.input, input)
            ),
        });
    }
    if step.retry != retry {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "step {} retry policy differs: {}",
                step.step_id,
                replay_diff(&step.retry, &retry)
            ),
        });
    }
    Ok(())
}

fn ensure_wait_command_matches(
    run_id: &str,
    wait: &WaitSnapshot,
    resume_at: chrono::DateTime<Utc>,
) -> Result<()> {
    if wait.resume_at != resume_at {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "wait {} resume_at differs: {}",
                wait.wait_id,
                replay_diff(&wait.resume_at, &resume_at)
            ),
        });
    }
    Ok(())
}

fn ensure_hook_command_matches(
    run_id: &str,
    hook: &HookSnapshot,
    token: &str,
    metadata: &serde_json::Value,
) -> Result<()> {
    if hook.token != token {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "hook {} token differs: history token and replay token are different (values redacted)",
                hook.hook_id
            ),
        });
    }
    if hook.metadata != *metadata {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "hook {} metadata differs: {}",
                hook.hook_id,
                replay_diff(&hook.metadata, metadata)
            ),
        });
    }
    Ok(())
}

fn replay_diff<T>(history: &T, replay: &T) -> String
where
    T: Serialize,
{
    format!(
        "history={}; replay={}",
        compact_json(history),
        compact_json(replay)
    )
}

fn compact_json<T>(value: &T) -> String
where
    T: Serialize,
{
    serde_json::to_string(value).unwrap_or_else(|err| format!("<failed to encode: {err}>"))
}

fn is_event_conflict(err: &FlowError) -> bool {
    matches!(err, FlowError::EventConflict { .. })
}
