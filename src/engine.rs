use chrono::{Duration as ChronoDuration, Utc};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, FlowEvent, HookStatus, RuntimeCommand, StepStatus, WaitStatus,
    WorkflowRunSnapshot, WorkflowRunStatus, WorkflowSpec,
};
use crate::runtime::{FlowRuntime, StepInvocation, WorkflowInvocation};
use crate::store::{FlowEventStore, InMemoryEventStore};

/// Builder for a [`FlowEngine`].
pub struct FlowEngineBuilder {
    store: Arc<dyn FlowEventStore>,
    runtime: Arc<dyn FlowRuntime>,
    max_replay_iterations: usize,
}

impl FlowEngineBuilder {
    pub fn new(runtime: Arc<dyn FlowRuntime>) -> Self {
        Self {
            store: Arc::new(InMemoryEventStore::new()),
            runtime,
            max_replay_iterations: 1024,
        }
    }

    pub fn with_store(mut self, store: Arc<dyn FlowEventStore>) -> Self {
        self.store = store;
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
            max_replay_iterations: self.max_replay_iterations,
        }
    }
}

/// Event-sourced workflow engine.
#[derive(Clone)]
pub struct FlowEngine {
    store: Arc<dyn FlowEventStore>,
    runtime: Arc<dyn FlowRuntime>,
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
            max_replay_iterations: 1024,
        }
    }

    pub fn in_memory(runtime: Arc<dyn FlowRuntime>) -> Self {
        Self::new(Arc::new(InMemoryEventStore::new()), runtime)
    }

    pub fn store(&self) -> Arc<dyn FlowEventStore> {
        Arc::clone(&self.store)
    }

    /// Start a workflow run and drive it until completion or suspension.
    pub async fn start(&self, spec: WorkflowSpec, input: serde_json::Value) -> Result<String> {
        spec.validate()?;
        let run_id = Uuid::new_v4().to_string();
        self.store
            .append(&run_id, FlowEvent::RunCreated { spec, input })
            .await?;
        self.store.append(&run_id, FlowEvent::RunStarted).await?;
        self.drive(&run_id).await?;
        Ok(run_id)
    }

    /// Resume a wait once its timer has fired.
    pub async fn resume_wait(&self, run_id: &str, wait_id: &str) -> Result<()> {
        let snapshot = self.snapshot(run_id).await?;
        if snapshot.status.is_terminal() {
            return Err(FlowError::RunTerminal(run_id.to_string()));
        }
        match snapshot.waits.get(wait_id) {
            Some(wait) if wait.status == WaitStatus::Waiting => {
                self.store
                    .append(
                        run_id,
                        FlowEvent::WaitCompleted {
                            wait_id: wait_id.to_string(),
                        },
                    )
                    .await?;
                self.drive(run_id).await.map(|_| ())
            }
            Some(_) => Ok(()),
            None => Err(FlowError::InvalidTransition(format!(
                "wait {wait_id} does not exist for run {run_id}"
            ))),
        }
    }

    /// Resume an active hook with external payload.
    pub async fn resume_hook(
        &self,
        run_id: &str,
        hook_id: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        let snapshot = self.snapshot(run_id).await?;
        if snapshot.status.is_terminal() {
            return Err(FlowError::RunTerminal(run_id.to_string()));
        }
        match snapshot.hooks.get(hook_id) {
            Some(hook) if hook.status == HookStatus::Active => {
                self.store
                    .append(
                        run_id,
                        FlowEvent::HookReceived {
                            hook_id: hook_id.to_string(),
                            payload,
                        },
                    )
                    .await?;
                self.drive(run_id).await.map(|_| ())
            }
            Some(_) => Ok(()),
            None => Err(FlowError::InvalidTransition(format!(
                "hook {hook_id} does not exist for run {run_id}"
            ))),
        }
    }

    pub async fn cancel(&self, run_id: &str, reason: Option<String>) -> Result<()> {
        let snapshot = self.snapshot(run_id).await?;
        if snapshot.status.is_terminal() {
            return Ok(());
        }
        self.store
            .append(run_id, FlowEvent::RunCancelled { reason })
            .await?;
        Ok(())
    }

    pub async fn snapshot(&self, run_id: &str) -> Result<WorkflowRunSnapshot> {
        let history = self.store.list(run_id).await?;
        project_run(run_id, &history)
    }

    /// Replay and dispatch until the run reaches a terminal state or an open
    /// wait/hook suspension.
    pub async fn drive(&self, run_id: &str) -> Result<WorkflowRunSnapshot> {
        for _ in 0..self.max_replay_iterations {
            let history = self.store.list(run_id).await?;
            let snapshot = project_run(run_id, &history)?;
            if snapshot.status.is_terminal() || snapshot.status == WorkflowRunStatus::Suspended {
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
                    self.store
                        .append(run_id, FlowEvent::RunCompleted { output })
                        .await?;
                    return self.snapshot(run_id).await;
                }
                RuntimeCommand::Fail { error } => {
                    self.store
                        .append(run_id, FlowEvent::RunFailed { error })
                        .await?;
                    return self.snapshot(run_id).await;
                }
                RuntimeCommand::ScheduleStep {
                    step_id,
                    step_name,
                    input,
                    retry,
                } => {
                    self.execute_step(run_id, &snapshot, step_id, step_name, input, retry)
                        .await?;
                }
                RuntimeCommand::WaitUntil { wait_id, resume_at } => {
                    match snapshot.waits.get(&wait_id) {
                        Some(wait) if wait.status == WaitStatus::Completed => continue,
                        Some(_) => return self.snapshot(run_id).await,
                        None => {
                            self.store
                                .append(run_id, FlowEvent::WaitCreated { wait_id, resume_at })
                                .await?;
                            return self.snapshot(run_id).await;
                        }
                    }
                }
                RuntimeCommand::CreateHook {
                    hook_id,
                    token,
                    metadata,
                } => match snapshot.hooks.get(&hook_id) {
                    Some(hook) if hook.status == HookStatus::Received => continue,
                    Some(_) => return self.snapshot(run_id).await,
                    None => {
                        self.store
                            .append(
                                run_id,
                                FlowEvent::HookCreated {
                                    hook_id,
                                    token,
                                    metadata,
                                },
                            )
                            .await?;
                        return self.snapshot(run_id).await;
                    }
                },
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
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
        if let Some(step) = snapshot.steps.get(&step_id) {
            if step.status == StepStatus::Completed {
                return Ok(());
            }
        } else {
            self.store
                .append(
                    run_id,
                    FlowEvent::StepCreated {
                        step_id: step_id.clone(),
                        step_name: step_name.clone(),
                        input: input.clone(),
                    },
                )
                .await?;
        }

        let max_attempts = retry.max_attempts.max(1);
        let mut attempt = snapshot
            .steps
            .get(&step_id)
            .map(|step| step.attempt)
            .unwrap_or(0);

        loop {
            attempt += 1;
            self.store
                .append(
                    run_id,
                    FlowEvent::StepStarted {
                        step_id: step_id.clone(),
                        attempt,
                    },
                )
                .await?;

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
                    self.store
                        .append(run_id, FlowEvent::StepCompleted { step_id, output })
                        .await?;
                    return Ok(());
                }
                Err(err) if attempt < max_attempts => {
                    let retry_after = if retry.delay_ms > 0 {
                        Some(Utc::now() + ChronoDuration::milliseconds(retry.delay_ms as i64))
                    } else {
                        None
                    };
                    self.store
                        .append(
                            run_id,
                            FlowEvent::StepRetrying {
                                step_id: step_id.clone(),
                                attempt,
                                error: err.to_string(),
                                retry_after,
                            },
                        )
                        .await?;
                    if retry.delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(retry.delay_ms)).await;
                    }
                }
                Err(err) => {
                    let error = err.to_string();
                    self.store
                        .append(
                            run_id,
                            FlowEvent::StepFailed {
                                step_id: step_id.clone(),
                                attempt,
                                error: error.clone(),
                            },
                        )
                        .await?;
                    self.store
                        .append(run_id, FlowEvent::RunFailed { error })
                        .await?;
                    return Ok(());
                }
            }
        }
    }
}
