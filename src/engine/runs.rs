use std::collections::BTreeSet;

use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{project_run, validate_run_id, FlowEvent, WorkflowRunStatus, WorkflowSpec};

use super::validation::{ensure_same_start, is_event_conflict};
use super::FlowEngine;

impl FlowEngine {
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
        let run_id = run_id.into();
        self.ensure_run_started(&run_id, &spec, &input).await?;
        self.drive(&run_id).await?;
        Ok(run_id)
    }

    pub(super) async fn terminate_run(&self, run_id: &str, event: FlowEvent) -> Result<()> {
        self.terminate_run_with_context(run_id, event, 0, &BTreeSet::new())
            .await
    }

    pub(super) async fn terminate_run_with_context(
        &self,
        run_id: &str,
        event: FlowEvent,
        child_depth: usize,
        ancestry: &BTreeSet<String>,
    ) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.ensure_continuation_leaf(run_id, false).await?;
            if ancestry.contains(&snapshot.run_id) {
                return Err(FlowError::ChildWorkflowCycle(snapshot.run_id));
            }
            if snapshot.status.is_terminal() {
                return Ok(());
            }
            let mut active_ancestry = ancestry.clone();
            active_ancestry.insert(snapshot.run_id.clone());
            if matches!(
                self.terminate_child_workflows(
                    &snapshot,
                    terminal_reason(&event),
                    child_depth,
                    &active_ancestry,
                )
                .await?,
                super::child_workflows::ChildReconciliation::Replay
            ) {
                continue;
            }
            match self
                .record_event_at(&snapshot.run_id, snapshot.last_sequence, event.clone())
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) if is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    pub(super) async fn ensure_run_started(
        &self,
        run_id: &str,
        spec: &WorkflowSpec,
        input: &serde_json::Value,
    ) -> Result<()> {
        self.ensure_run_started_with_admission(run_id, spec, input, true)
            .await
    }

    pub(super) async fn ensure_run_started_with_admission(
        &self,
        run_id: &str,
        spec: &WorkflowSpec,
        input: &serde_json::Value,
        require_runtime_build: bool,
    ) -> Result<()> {
        spec.validate()?;
        validate_run_id(run_id)?;
        if require_runtime_build {
            self.ensure_runtime_build_available(run_id, spec)?;
        }

        for _ in 0..self.max_replay_iterations {
            match self.store.list(run_id).await {
                Ok(history) => {
                    let snapshot = project_run(run_id, &history)?;
                    ensure_same_start(run_id, &snapshot, spec, input)?;
                    if snapshot.status != WorkflowRunStatus::Pending {
                        return Ok(());
                    }
                    match self
                        .record_event_at(run_id, snapshot.last_sequence, FlowEvent::RunStarted)
                        .await
                    {
                        Ok(_) => return Ok(()),
                        Err(error) if is_event_conflict(&error) => continue,
                        Err(error) => return Err(error),
                    }
                }
                Err(FlowError::RunNotFound(_)) => {
                    match self
                        .record_event_at(
                            run_id,
                            0,
                            FlowEvent::RunCreated {
                                spec: spec.clone(),
                                input: input.clone(),
                            },
                        )
                        .await
                    {
                        Ok(_) => continue,
                        Err(error) if is_event_conflict(&error) => continue,
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }
}

fn terminal_reason(event: &FlowEvent) -> Option<String> {
    match event {
        FlowEvent::RunCancelled { reason }
        | FlowEvent::RunTimedOut { reason, .. }
        | FlowEvent::RunHostShutdown { reason } => reason.clone(),
        FlowEvent::RunFailed { error } | FlowEvent::RunRetryExhausted { error, .. } => {
            Some(error.clone())
        }
        _ => None,
    }
}
