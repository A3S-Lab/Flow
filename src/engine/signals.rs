use crate::error::{FlowError, Result};
use crate::model::{
    validate_signal_wait, FlowEvent, SignalWaitStatus, WorkflowRunSnapshot, WorkflowRunStatus,
    WorkflowSignal, WorkflowSignalSnapshot,
};

use super::{
    validation::{ensure_signal_wait_command_matches, is_event_conflict},
    FlowEngine,
};

pub(super) enum SignalWaitCommandOutcome {
    Replay,
    Waiting,
}

impl FlowEngine {
    /// Durably deliver a named asynchronous signal to an active execution.
    ///
    /// The target follows persisted continue-as-new links. Retrying with the
    /// same target run ID and `signal_id` is idempotent across that descendant
    /// chain; changing the name or payload is an explicit conflict. New and
    /// matching deliveries repair and drive the active leaf, including a
    /// successor missing after its predecessor link committed.
    pub async fn send_signal(
        &self,
        run_id: &str,
        signal: WorkflowSignal,
    ) -> Result<WorkflowRunSnapshot> {
        let (snapshot, _) = self.send_signal_with_commit(run_id, signal).await?;
        Ok(snapshot)
    }

    /// Deliver a signal and return the stream where this call committed it.
    pub(crate) async fn send_signal_with_commit(
        &self,
        run_id: &str,
        signal: WorkflowSignal,
    ) -> Result<(WorkflowRunSnapshot, Option<String>)> {
        signal.validate()?;
        let mut committed_run_id = None;

        for _ in 0..self.max_replay_iterations {
            // Repair an interrupted continuation before scanning its complete
            // descendant chain for an earlier delivery attempt.
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
                    .signal(&signal.signal_id)
                    .map(|existing| (snapshot.run_id.as_str(), existing))
            });
            if let Some((delivery_run_id, existing)) = existing {
                ensure_signal_matches(delivery_run_id, existing, &signal)?;
                match self.recover_and_drive_continuation_leaf(run_id).await {
                    Ok(snapshot) => return Ok((snapshot, committed_run_id)),
                    Err(error) if is_event_conflict(&error) => continue,
                    Err(error) => return Err(error),
                }
            }

            if leaf.status.is_terminal() {
                return Err(FlowError::RunTerminal(leaf.run_id.clone()));
            }
            if !leaf.spec.accepts_signal(&signal.name) {
                return Err(FlowError::InvalidTransition(format!(
                    "workflow run {} does not declare signal {}",
                    leaf.run_id, signal.name
                )));
            }
            self.ensure_runtime_build_available(&leaf.run_id, &leaf.spec)?;
            match self
                .record_event_at(
                    &leaf.run_id,
                    leaf.last_sequence,
                    FlowEvent::SignalReceived {
                        signal: signal.clone(),
                    },
                )
                .await
            {
                Ok(_) => {
                    committed_run_id = Some(leaf.run_id.clone());
                    match self.drive(run_id).await {
                        Ok(snapshot) => return Ok((snapshot, committed_run_id)),
                        Err(error) if is_event_conflict(&error) => continue,
                        Err(error) => return Err(error),
                    }
                }
                Err(error) if is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }

        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Pair one waiting signal command with the oldest matching unconsumed
    /// delivery. One event is appended per replay iteration to preserve the
    /// store's optimistic concurrency boundary.
    pub(super) async fn reconcile_signal_waits(
        &self,
        snapshot: &WorkflowRunSnapshot,
    ) -> Result<bool> {
        let mut waits = snapshot
            .signal_waits
            .values()
            .filter(|wait| wait.status == SignalWaitStatus::Waiting)
            .collect::<Vec<_>>();
        waits.sort_by_key(|wait| wait.created_sequence);

        for wait in waits {
            let signal = snapshot
                .signals
                .iter()
                .filter(|signal| signal.consumed_by.is_none() && signal.name == wait.signal_name)
                .min_by_key(|signal| signal.received_sequence);
            let Some(signal) = signal else {
                continue;
            };

            self.record_event_at(
                &snapshot.run_id,
                snapshot.last_sequence,
                FlowEvent::SignalWaitCompleted {
                    wait_id: wait.wait_id.clone(),
                    signal_id: signal.signal_id.clone(),
                },
            )
            .await?;
            return Ok(true);
        }

        Ok(false)
    }

    pub(super) async fn schedule_signal_wait(
        &self,
        snapshot: &WorkflowRunSnapshot,
        wait_id: String,
        signal_name: String,
    ) -> Result<SignalWaitCommandOutcome> {
        validate_signal_wait(&wait_id, &signal_name)?;
        if !snapshot.spec.accepts_signal(&signal_name) {
            return Err(FlowError::InvalidTransition(format!(
                "workflow run {} does not declare signal {signal_name}",
                snapshot.run_id
            )));
        }

        match snapshot.signal_waits.get(&wait_id) {
            Some(wait) => {
                ensure_signal_wait_command_matches(&snapshot.run_id, wait, &signal_name)?;
                match wait.status {
                    SignalWaitStatus::Completed => Ok(SignalWaitCommandOutcome::Replay),
                    SignalWaitStatus::Waiting => Ok(SignalWaitCommandOutcome::Waiting),
                    SignalWaitStatus::Cancelled => Err(FlowError::InvalidTransition(format!(
                        "workflow rescheduled cancelled signal wait {wait_id}; cancellation cleanup must use a distinct stable identity"
                    ))),
                }
            }
            None => {
                self.record_event_at(
                    &snapshot.run_id,
                    snapshot.last_sequence,
                    FlowEvent::SignalWaitCreated {
                        wait_id,
                        signal_name,
                    },
                )
                .await?;
                Ok(SignalWaitCommandOutcome::Replay)
            }
        }
    }
}

fn ensure_signal_matches(
    run_id: &str,
    existing: &WorkflowSignalSnapshot,
    signal: &WorkflowSignal,
) -> Result<()> {
    if existing.name != signal.name || existing.payload != signal.payload {
        return Err(FlowError::SignalConflict {
            run_id: run_id.to_string(),
            signal_id: signal.signal_id.clone(),
            reason: "name or payload differs from the durable delivery".to_string(),
        });
    }
    Ok(())
}
