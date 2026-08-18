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
    /// chain; changing the name or payload is an explicit conflict. The method
    /// drives the active leaf after the event commits so a matching signal wait
    /// can resume immediately.
    pub async fn send_signal(
        &self,
        run_id: &str,
        signal: WorkflowSignal,
    ) -> Result<WorkflowRunSnapshot> {
        signal.validate()?;

        for _ in 0..self.max_replay_iterations {
            // Repair an interrupted continuation before scanning its complete
            // descendant chain for an earlier delivery attempt.
            let candidate = self.ensure_continuation_leaf(run_id, false).await?;
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
                if delivery_run_id != leaf.run_id || leaf.status.is_terminal() {
                    return Ok(leaf.clone());
                }
                self.ensure_runtime_build_available(&leaf.run_id, &leaf.spec)?;
                match self.drive(run_id).await {
                    Ok(snapshot) => return Ok(snapshot),
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
                Ok(_) => match self.drive(run_id).await {
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
