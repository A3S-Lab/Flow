use crate::error::{FlowError, Result};
use crate::model::{
    FlowEvent, WorkflowRunStatus, WorkflowUpdate, WorkflowUpdateOutcome, WorkflowUpdateSnapshot,
};
use crate::runtime::UpdateInvocation;

use super::{validation::is_event_conflict, FlowEngine};

impl FlowEngine {
    /// Apply a declared synchronous update and drive the active leaf.
    ///
    /// The target follows persisted continue-as-new links. Retrying with the
    /// same target run ID and `update_id` is idempotent across that descendant
    /// chain; changing the name or input is an explicit conflict. New updates
    /// invoke `FlowRuntime::run_update` before appending history, then drive
    /// workflow replay so the durable update is visible to the next command.
    pub async fn apply_update(
        &self,
        run_id: &str,
        update: WorkflowUpdate,
    ) -> Result<WorkflowUpdateOutcome> {
        update.validate()?;

        for _ in 0..self.max_replay_iterations {
            let candidate = self.ensure_continuation_leaf(run_id).await?;
            if candidate.status == WorkflowRunStatus::Pending {
                self.ensure_run_started_with_admission(
                    &candidate.run_id,
                    &candidate.spec,
                    &candidate.input,
                    false,
                )
                .await?;
                continue;
            }
            let chain = self.continuation_chain(run_id).await?;
            let leaf = chain
                .last()
                .ok_or_else(|| FlowError::RunNotFound(run_id.to_string()))?;

            let existing = chain.iter().find_map(|snapshot| {
                snapshot
                    .update(&update.update_id)
                    .map(|existing| (snapshot.run_id.as_str(), existing))
            });
            if let Some((delivery_run_id, existing)) = existing {
                ensure_update_matches(delivery_run_id, existing, &update)?;
                let output = existing.output.clone();
                match self.recover_and_drive_continuation_leaf(run_id).await {
                    Ok(snapshot) => {
                        return Ok(WorkflowUpdateOutcome { snapshot, output });
                    }
                    Err(error) if is_event_conflict(&error) => continue,
                    Err(error) => return Err(error),
                }
            }

            if leaf.status.is_terminal() {
                return Err(FlowError::RunTerminal(leaf.run_id.clone()));
            }
            if !leaf.spec.accepts_update(&update.name) {
                return Err(FlowError::InvalidTransition(format!(
                    "workflow run {} does not declare update {}",
                    leaf.run_id, update.name
                )));
            }
            self.ensure_runtime_build_available(&leaf.run_id, &leaf.spec)?;
            let history = self.store.list(&leaf.run_id).await?;
            let invocation = UpdateInvocation::new(
                leaf.run_id.clone(),
                leaf.spec.clone(),
                update.clone(),
                history,
            );
            let output = self.runtime.run_update(invocation).await?;
            match self
                .record_event_at(
                    &leaf.run_id,
                    leaf.last_sequence,
                    FlowEvent::UpdateApplied {
                        update: update.clone(),
                        output: output.clone(),
                    },
                )
                .await
            {
                Ok(_) => match self.drive_forcing_workflow_replay(run_id).await {
                    Ok(snapshot) => {
                        return Ok(WorkflowUpdateOutcome { snapshot, output });
                    }
                    Err(error) if is_event_conflict(&error) => continue,
                    Err(error) => return Err(error),
                },
                Err(error) if is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }
}

fn ensure_update_matches(
    run_id: &str,
    existing: &WorkflowUpdateSnapshot,
    update: &WorkflowUpdate,
) -> Result<()> {
    if existing.name != update.name || existing.input != update.input {
        return Err(FlowError::UpdateConflict {
            run_id: run_id.to_string(),
            update_id: update.update_id.clone(),
            reason: "name or input differs from the durable update".to_string(),
        });
    }
    Ok(())
}
