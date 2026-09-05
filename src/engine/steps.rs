use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::task::{Id, JoinSet};

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, FlowEvent, FlowEventEnvelope, RetryPolicy, StepCommand, StepFailureAction,
    StepStatus, WorkflowRunSnapshot,
};
use crate::runtime::StepInvocation;

use super::validation::ensure_step_command_matches;
use super::FlowEngine;

pub(super) struct StepExecutionContext {
    pub(super) step_id: String,
    pub(super) step_name: String,
    pub(super) input: serde_json::Value,
    pub(super) retry: RetryPolicy,
    pub(super) now: DateTime<Utc>,
}

struct BatchTerminalFailure {
    step_index: usize,
    attempt: u32,
    error: String,
    retry_exhausted: bool,
}

/// Prefix reserved for the durable marker emitted when a concurrent task
/// terminates without returning a `Result`.  Keeping this marker stable lets a
/// later host finish the terminal transition after a crash between the marker
/// and the run-level outcome.
const BATCH_TASK_FAILURE_REASON_PREFIX: &str =
    "concurrent step task failed before returning an outcome: ";

fn batch_abort_reason(failing_step_id: &str) -> String {
    format!(
        "concurrent batch aborted after sibling failure {failing_step_id}; step outcome is unknown"
    )
}

fn next_open_step(snapshot: &WorkflowRunSnapshot) -> Option<(&str, u32)> {
    snapshot.steps.values().find_map(|step| {
        matches!(step.status, StepStatus::Pending | StepStatus::Running)
            .then_some((step.step_id.as_str(), step.attempt))
    })
}

/// Rebuild the next missing event after a host stopped during terminal batch
/// settlement.  The function deliberately returns one event at a time: each
/// event is validated and committed through the normal expected-sequence path,
/// so a crash or writer race can safely replay this decision.
pub(super) fn interrupted_terminal_event(
    snapshot: &WorkflowRunSnapshot,
    history: &[FlowEventEnvelope],
) -> Option<FlowEvent> {
    if snapshot.status.is_terminal() {
        return None;
    }

    if let Some((step_id, attempt, error)) = history.iter().find_map(|envelope| {
        let FlowEvent::StepFailed {
            step_id,
            attempt,
            error,
        } = &envelope.event
        else {
            return None;
        };
        let step = snapshot.steps.get(step_id)?;
        (step.status == StepStatus::Failed
            && step.retry.on_exhausted == StepFailureAction::FailRun
            && step.attempt == *attempt
            && step.error.as_deref() == Some(error.as_str()))
        .then(|| (step_id.clone(), *attempt, error.clone()))
    }) {
        if let Some((open_step_id, open_attempt)) = next_open_step(snapshot) {
            return Some(FlowEvent::StepCancelled {
                step_id: open_step_id.to_string(),
                attempt: open_attempt,
                reason: batch_abort_reason(&step_id),
            });
        }
        return Some(FlowEvent::RunRetryExhausted {
            step_id,
            attempt,
            error,
        });
    }

    if let Some((failed_step_id, error)) = history.iter().find_map(|envelope| {
        let FlowEvent::StepNonRetryable { step_id, error, .. } = &envelope.event else {
            return None;
        };
        let step = snapshot.steps.get(step_id)?;
        (step.status == StepStatus::Failed
            && step.retry.on_exhausted == StepFailureAction::FailRun
            && step.error.as_deref() == Some(error.as_str()))
        .then(|| (step_id.clone(), error.clone()))
    }) {
        if let Some((open_step_id, open_attempt)) = next_open_step(snapshot) {
            return Some(FlowEvent::StepCancelled {
                step_id: open_step_id.to_string(),
                attempt: open_attempt,
                reason: batch_abort_reason(&failed_step_id),
            });
        }
        return Some(FlowEvent::RunFailed { error });
    }

    // A task panic/cancellation has no `StepFailed` event to anchor recovery.
    // Its first durable marker carries a reserved reason prefix; use that
    // marker as the intent record and finish cancelling any remaining peers.
    let (failed_step_id, task_failure_reason) = history.iter().find_map(|envelope| {
        let FlowEvent::StepCancelled {
            step_id, reason, ..
        } = &envelope.event
        else {
            return None;
        };
        reason
            .starts_with(BATCH_TASK_FAILURE_REASON_PREFIX)
            .then(|| (step_id.clone(), reason.clone()))
    })?;
    if let Some((open_step_id, open_attempt)) = next_open_step(snapshot) {
        return Some(FlowEvent::StepCancelled {
            step_id: open_step_id.to_string(),
            attempt: open_attempt,
            reason: batch_abort_reason(&failed_step_id),
        });
    }
    Some(FlowEvent::RunFailed {
        error: task_failure_reason,
    })
}

impl FlowEngine {
    pub(super) async fn execute_step(
        &self,
        run_id: &str,
        snapshot: &WorkflowRunSnapshot,
        context: StepExecutionContext,
    ) -> Result<()> {
        let StepExecutionContext {
            step_id,
            step_name,
            input,
            retry,
            now,
        } = context;
        let mut expected_sequence = snapshot.last_sequence;
        if let Some(step) = snapshot.steps.get(&step_id) {
            ensure_step_command_matches(run_id, step, &step_name, &input, retry)?;
            if matches!(
                step.status,
                StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled
            ) {
                return Ok(());
            }
            if step.status == StepStatus::Pending
                && step
                    .retry_after
                    .is_some_and(|retry_after| retry_after > now)
            {
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
        let mut redelivering_running_step = snapshot
            .steps
            .get(&step_id)
            .is_some_and(|step| step.status == StepStatus::Running);

        loop {
            if redelivering_running_step {
                // A process can die after the step side effect succeeds but before
                // StepCompleted is durable. Redeliver the same attempt so an
                // idempotent step can recover that ambiguous boundary.
                redelivering_running_step = false;
            } else {
                attempt = attempt.checked_add(1).ok_or_else(|| {
                    FlowError::InvalidTransition(format!("step attempt overflowed for {step_id}"))
                })?;
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
            }

            let history = self.store.list(run_id).await?;
            let invocation = StepInvocation {
                run_id: run_id.to_string(),
                step_id: step_id.clone(),
                attempt,
                step_name: step_name.clone(),
                input: input.clone(),
                history,
                idempotency_key: crate::runtime::step_attempt_idempotency_key(
                    run_id, &step_id, attempt,
                ),
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
                Err(err) if !err.is_retryable() => {
                    let error = err.to_string();
                    let failed = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepNonRetryable {
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
                Err(err) if attempt < max_attempts => {
                    // Anchor the deadline at the effective failure clock, not
                    // the beginning of a potentially long drive call. A
                    // scheduler may intentionally provide a future cutoff;
                    // retaining that lower bound prevents one catch-up pass
                    // from immediately consuming retries scheduled after its
                    // logical horizon.
                    let retry_anchor = retry_anchor(now);
                    let retry_after =
                        retry.retry_after_for_step(retry_anchor, attempt, run_id, &step_id)?;
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
                    self.record_event_at(
                        run_id,
                        failed.sequence,
                        FlowEvent::RunRetryExhausted {
                            step_id,
                            attempt,
                            error,
                        },
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    }

    pub(super) async fn execute_step_batch(
        &self,
        run_id: &str,
        snapshot: &WorkflowRunSnapshot,
        steps: Vec<StepCommand>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let mut expected_sequence = snapshot.last_sequence;

        // Make every sibling identity durable before any side effect starts.
        for step in &steps {
            if snapshot.steps.contains_key(&step.step_id) {
                continue;
            }
            let created = self
                .record_event_at(
                    run_id,
                    expected_sequence,
                    FlowEvent::StepCreated {
                        step_id: step.step_id.clone(),
                        step_name: step.step_name.clone(),
                        input: step.input.clone(),
                        retry: step.retry,
                    },
                )
                .await?;
            expected_sequence = created.sequence;
        }

        let mut active = Vec::new();
        for step in steps {
            let existing = snapshot.steps.get(&step.step_id);
            let attempt = match existing.map(|existing| existing.status) {
                Some(StepStatus::Completed | StepStatus::Failed | StepStatus::Cancelled) => {
                    continue
                }
                Some(StepStatus::Running) => {
                    existing.map(|existing| existing.attempt).ok_or_else(|| {
                        FlowError::InvalidTransition(format!(
                            "running batch step {} has no projected attempt",
                            step.step_id
                        ))
                    })?
                }
                Some(StepStatus::Pending) => {
                    if existing
                        .and_then(|existing| existing.retry_after)
                        .is_some_and(|retry_after| retry_after > now)
                    {
                        continue;
                    }
                    let attempt = existing
                        .and_then(|existing| existing.attempt.checked_add(1))
                        .ok_or_else(|| {
                            FlowError::InvalidTransition(format!(
                                "step attempt overflowed for {}",
                                step.step_id
                            ))
                        })?;
                    let started = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepStarted {
                                step_id: step.step_id.clone(),
                                attempt,
                            },
                        )
                        .await?;
                    expected_sequence = started.sequence;
                    attempt
                }
                None => {
                    let attempt = 1;
                    let started = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::StepStarted {
                                step_id: step.step_id.clone(),
                                attempt,
                            },
                        )
                        .await?;
                    expected_sequence = started.sequence;
                    attempt
                }
            };
            active.push((step, attempt));
        }

        while !active.is_empty() {
            let history = self.store.list(run_id).await?;
            let mut tasks = JoinSet::new();
            let mut task_indexes = HashMap::<Id, usize>::with_capacity(active.len());
            for (index, (step, _)) in active.iter().enumerate() {
                let runtime = Arc::clone(&self.runtime);
                let invocation = StepInvocation {
                    run_id: run_id.to_string(),
                    step_id: step.step_id.clone(),
                    attempt: active[index].1,
                    step_name: step.step_name.clone(),
                    input: step.input.clone(),
                    history: history.clone(),
                    idempotency_key: crate::runtime::step_attempt_idempotency_key(
                        run_id,
                        &step.step_id,
                        active[index].1,
                    ),
                };
                let task_id =
                    tasks.spawn(async move { (index, runtime.run_step(invocation).await) });
                task_indexes.insert(task_id.id(), index);
            }
            let mut observed_outcomes = vec![false; active.len()];
            let mut immediate_retries = Vec::new();
            let mut terminal_failure = None;
            while let Some(joined) = tasks.join_next_with_id().await {
                let (index, outcome) = match joined {
                    Ok((task_id, joined)) => {
                        let expected_index = task_indexes.remove(&task_id).ok_or_else(|| {
                            FlowError::Runtime(format!(
                                "concurrent step task {task_id} returned more than once"
                            ))
                        })?;
                        if expected_index != joined.0 {
                            return Err(FlowError::Runtime(format!(
                                "concurrent step task {task_id} reported index {}, expected {expected_index}",
                                joined.0
                            )));
                        }
                        joined
                    }
                    Err(error) => {
                        let index = task_indexes.remove(&error.id()).ok_or_else(|| {
                            FlowError::Runtime(format!(
                                "concurrent step task {} failed after it was already joined",
                                error.id()
                            ))
                        })?;
                        observed_outcomes[index] = true;
                        let (step, attempt) = &active[index];
                        let reason = format!("{BATCH_TASK_FAILURE_REASON_PREFIX}{error}");
                        let cancelled = self
                            .record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::StepCancelled {
                                    step_id: step.step_id.clone(),
                                    attempt: *attempt,
                                    reason: reason.clone(),
                                },
                            )
                            .await?;
                        expected_sequence = cancelled.sequence;
                        terminal_failure = Some(BatchTerminalFailure {
                            step_index: index,
                            attempt: *attempt,
                            error: reason,
                            retry_exhausted: false,
                        });
                        break;
                    }
                };
                if index >= observed_outcomes.len() || observed_outcomes[index] {
                    return Err(FlowError::InvalidTransition(
                        "concurrent step batch returned an invalid outcome index".to_string(),
                    ));
                }
                observed_outcomes[index] = true;
                let (step, attempt) = &active[index];
                match outcome {
                    Ok(output) => {
                        let completed = self
                            .record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::StepCompleted {
                                    step_id: step.step_id.clone(),
                                    output,
                                },
                            )
                            .await?;
                        expected_sequence = completed.sequence;
                    }
                    Err(error) if !error.is_retryable() => {
                        let error = error.to_string();
                        let failed = self
                            .record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::StepNonRetryable {
                                    step_id: step.step_id.clone(),
                                    attempt: *attempt,
                                    error: error.clone(),
                                },
                            )
                            .await?;
                        expected_sequence = failed.sequence;
                        if step.retry.on_exhausted == StepFailureAction::FailRun {
                            terminal_failure = Some(BatchTerminalFailure {
                                step_index: index,
                                attempt: *attempt,
                                error,
                                retry_exhausted: false,
                            });
                            break;
                        }
                    }
                    Err(error) if *attempt < step.retry.max_attempts.max(1) => {
                        let error = error.to_string();
                        let retry_anchor = retry_anchor(now);
                        let retry_after = step.retry.retry_after_for_step(
                            retry_anchor,
                            *attempt,
                            run_id,
                            &step.step_id,
                        )?;
                        let retrying = self
                            .record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::StepRetrying {
                                    step_id: step.step_id.clone(),
                                    attempt: *attempt,
                                    error,
                                    retry_after,
                                },
                            )
                            .await?;
                        expected_sequence = retrying.sequence;
                        if retry_after.is_none() {
                            immediate_retries.push((step.clone(), *attempt));
                        }
                    }
                    Err(error) => {
                        let error = error.to_string();
                        let failed = self
                            .record_event_at(
                                run_id,
                                expected_sequence,
                                FlowEvent::StepFailed {
                                    step_id: step.step_id.clone(),
                                    attempt: *attempt,
                                    error: error.clone(),
                                },
                            )
                            .await?;
                        expected_sequence = failed.sequence;
                        if step.retry.on_exhausted == StepFailureAction::FailRun {
                            terminal_failure = Some(BatchTerminalFailure {
                                step_index: index,
                                attempt: *attempt,
                                error,
                                retry_exhausted: true,
                            });
                            break;
                        }
                    }
                }
            }

            if let Some(failure) = terminal_failure {
                // A terminal sibling failure makes every still-open sibling
                // non-actionable. Abort local futures immediately, then leave
                // an explicit durable cancellation marker for each branch so
                // replay never presents a terminal run with a phantom Running
                // or Pending step. The reason deliberately says "unknown": an
                // external side effect may have crossed the host boundary
                // before the future was aborted.
                tasks.abort_all();
                drop(tasks);

                let failing_step_id = active[failure.step_index].0.step_id.clone();
                self.cancel_open_steps_after_terminal_failure(
                    run_id,
                    &mut expected_sequence,
                    &failing_step_id,
                )
                .await?;

                let terminal_event = if failure.retry_exhausted {
                    FlowEvent::RunRetryExhausted {
                        step_id: failing_step_id,
                        attempt: failure.attempt,
                        error: failure.error,
                    }
                } else {
                    FlowEvent::RunFailed {
                        error: failure.error,
                    }
                };
                self.record_event_at(run_id, expected_sequence, terminal_event)
                    .await?;
                return Ok(());
            }

            if let Some(index) = observed_outcomes.iter().position(|observed| !observed) {
                return Err(FlowError::Runtime(format!(
                    "concurrent step batch omitted outcome index {index}"
                )));
            }
            if immediate_retries.is_empty() {
                return Ok(());
            }

            let mut next_active = Vec::with_capacity(immediate_retries.len());
            for (step, attempt) in immediate_retries {
                let attempt = attempt.checked_add(1).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step attempt overflowed for {}",
                        step.step_id
                    ))
                })?;
                let started = self
                    .record_event_at(
                        run_id,
                        expected_sequence,
                        FlowEvent::StepStarted {
                            step_id: step.step_id.clone(),
                            attempt,
                        },
                    )
                    .await?;
                expected_sequence = started.sequence;
                next_active.push((step, attempt));
            }
            active = next_active;
        }

        Ok(())
    }

    /// Persist cancellation markers for every step that is still actionable
    /// after a terminal batch decision. Re-reading the projection is
    /// intentional: a sibling may have entered a delayed retry in an earlier
    /// batch round and therefore no longer be present in the current `active`
    /// vector. It also makes the normal path agree with crash recovery, which
    /// settles any open step before writing the run-level terminal event.
    async fn cancel_open_steps_after_terminal_failure(
        &self,
        run_id: &str,
        expected_sequence: &mut u64,
        failing_step_id: &str,
    ) -> Result<()> {
        let history = self.store.list(run_id).await?;
        let snapshot = project_run(run_id, &history)?;
        for step in snapshot.steps.values() {
            if !matches!(step.status, StepStatus::Pending | StepStatus::Running) {
                continue;
            }
            let cancelled = self
                .record_event_at(
                    run_id,
                    *expected_sequence,
                    FlowEvent::StepCancelled {
                        step_id: step.step_id.clone(),
                        attempt: step.attempt,
                        reason: batch_abort_reason(failing_step_id),
                    },
                )
                .await?;
            *expected_sequence = cancelled.sequence;
        }
        Ok(())
    }
}

pub(super) fn retry_anchor(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc::now().max(now)
}
