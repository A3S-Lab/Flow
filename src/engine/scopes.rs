use crate::error::{FlowError, Result};
use crate::model::{CancellationScopeStatus, FlowEvent, WorkflowRunSnapshot, WorkflowRunStatus};

use super::{validation::is_event_conflict, FlowEngine};

impl FlowEngine {
    /// Cancel a declared cancellation scope and drive owned suspensions closed.
    ///
    /// Retrying with the same run, scope, and reason is idempotent. Changing the
    /// reason after a durable cancellation is an explicit conflict. Completing a
    /// scope and later cancelling it is rejected as an invalid transition.
    pub async fn cancel_scope(
        &self,
        run_id: &str,
        scope_id: impl Into<String>,
        reason: Option<String>,
    ) -> Result<WorkflowRunSnapshot> {
        let scope_id = scope_id.into();
        if scope_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "workflow scope id must not be empty".to_string(),
            ));
        }

        for _ in 0..self.max_replay_iterations {
            let leaf = self.ensure_continuation_leaf(run_id).await?;
            if leaf.status == WorkflowRunStatus::Pending {
                self.ensure_run_started_with_admission(
                    &leaf.run_id,
                    &leaf.spec,
                    &leaf.input,
                    false,
                )
                .await?;
                continue;
            }
            match leaf.scope(&scope_id) {
                Some(existing) if existing.status == CancellationScopeStatus::Cancelled => {
                    if existing.reason != reason {
                        return Err(FlowError::ScopeConflict {
                            run_id: leaf.run_id.clone(),
                            scope_id,
                            reason: "cancellation reason differs from the durable scope"
                                .to_string(),
                        });
                    }
                    return Ok(leaf);
                }
                Some(existing) if existing.status == CancellationScopeStatus::Completed => {
                    return Err(FlowError::InvalidTransition(format!(
                        "scope {scope_id} for workflow run {} is already completed",
                        leaf.run_id
                    )));
                }
                Some(_) => {}
                None => {
                    return Err(FlowError::InvalidTransition(format!(
                        "scope {scope_id} for workflow run {} has not been opened",
                        leaf.run_id
                    )));
                }
            }
            if leaf.status.is_terminal() {
                return Err(FlowError::RunTerminal(leaf.run_id.clone()));
            }
            self.ensure_runtime_build_available(&leaf.run_id, &leaf.spec)?;
            match self
                .record_event_at(
                    &leaf.run_id,
                    leaf.last_sequence,
                    FlowEvent::ScopeCancelled {
                        scope_id: scope_id.clone(),
                        reason: reason.clone(),
                    },
                )
                .await
            {
                Ok(_) => match self.drive_forcing_workflow_replay(run_id).await {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(error) if is_event_conflict(&error) => continue,
                    Err(error) => return Err(error),
                },
                Err(error) if is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    pub(super) async fn schedule_open_scope(
        &self,
        snapshot: &WorkflowRunSnapshot,
        scope_id: String,
    ) -> Result<bool> {
        if scope_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "workflow scope id must not be empty".to_string(),
            ));
        }
        match snapshot.scope(&scope_id) {
            Some(existing) => {
                if existing.is_open()
                    || existing.status == CancellationScopeStatus::Completed
                    || existing.status == CancellationScopeStatus::Cancelled
                {
                    // Replay of an already recorded open command.
                    Ok(false)
                } else {
                    Err(FlowError::InvalidTransition(format!(
                        "scope {scope_id} has unexpected status {:?}",
                        existing.status
                    )))
                }
            }
            None => {
                self.record_event_at(
                    &snapshot.run_id,
                    snapshot.last_sequence,
                    FlowEvent::ScopeOpened {
                        scope_id,
                        parent_scope_id: snapshot.innermost_open_scope().map(str::to_string),
                    },
                )
                .await?;
                Ok(true)
            }
        }
    }

    pub(super) async fn schedule_complete_scope(
        &self,
        snapshot: &WorkflowRunSnapshot,
        scope_id: String,
    ) -> Result<bool> {
        let Some(existing) = snapshot.scope(&scope_id) else {
            return Err(FlowError::InvalidTransition(format!(
                "scope_completed references unknown scope {scope_id}"
            )));
        };
        match existing.status {
            CancellationScopeStatus::Completed => Ok(false),
            CancellationScopeStatus::Cancelled => Err(FlowError::InvalidTransition(format!(
                "scope {scope_id} cannot complete after cancellation"
            ))),
            CancellationScopeStatus::Open => {
                if snapshot.innermost_open_scope() != Some(scope_id.as_str()) {
                    return Err(FlowError::InvalidTransition(format!(
                        "scope_completed requires {scope_id} to be the innermost open scope"
                    )));
                }
                self.record_event_at(
                    &snapshot.run_id,
                    snapshot.last_sequence,
                    FlowEvent::ScopeCompleted { scope_id },
                )
                .await?;
                Ok(true)
            }
        }
    }

    pub(super) async fn schedule_cancel_scope(
        &self,
        snapshot: &WorkflowRunSnapshot,
        scope_id: String,
        reason: Option<String>,
    ) -> Result<bool> {
        let Some(existing) = snapshot.scope(&scope_id) else {
            return Err(FlowError::InvalidTransition(format!(
                "scope_cancelled references unknown scope {scope_id}"
            )));
        };
        match existing.status {
            CancellationScopeStatus::Cancelled => {
                if existing.reason != reason {
                    return Err(FlowError::ScopeConflict {
                        run_id: snapshot.run_id.clone(),
                        scope_id,
                        reason: "cancellation reason differs from the durable scope".to_string(),
                    });
                }
                Ok(false)
            }
            CancellationScopeStatus::Completed => Err(FlowError::InvalidTransition(format!(
                "scope {scope_id} cannot cancel after completion"
            ))),
            CancellationScopeStatus::Open => {
                self.record_event_at(
                    &snapshot.run_id,
                    snapshot.last_sequence,
                    FlowEvent::ScopeCancelled { scope_id, reason },
                )
                .await?;
                Ok(true)
            }
        }
    }
}
