use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{
    ActivityResolution, ActivityStatus, FlowEvent, RetryPolicy, WorkflowRunSnapshot,
};
use crate::runtime::{activity_attempt_id, activity_attempt_idempotency_key, ActivityInvocation};

use super::validation::ensure_activity_command_matches;
use super::FlowEngine;

/// Inputs required to execute one first-class activity.
pub(super) struct ActivityExecutionContext {
    pub(super) activity_id: String,
    pub(super) activity_name: String,
    pub(super) input: serde_json::Value,
    pub(super) retry: RetryPolicy,
    pub(super) now: DateTime<Utc>,
}

impl FlowEngine {
    /// Reconcile an activity whose external side-effect outcome is unknown.
    ///
    /// The caller must use the attempt and idempotency identities exposed in
    /// [`ActivitySnapshot`](crate::ActivitySnapshot) to query the provider.
    /// Resolution is fenced by the current attempt token and is safe to retry
    /// after an event-sequence conflict.
    pub async fn resolve_unknown_activity(
        &self,
        run_id: &str,
        activity_id: &str,
        resolution: ActivityResolution,
    ) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            let activity = snapshot.activities.get(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity {activity_id} does not exist for run {run_id}"
                ))
            })?;
            if activity.status != ActivityStatus::Unknown {
                if activity_resolution_matches_snapshot(activity, &resolution) {
                    return Ok(());
                }
                return Err(FlowError::InvalidTransition(format!(
                    "activity {activity_id} is not awaiting unknown-outcome reconciliation"
                )));
            }

            let event = match &resolution {
                ActivityResolution::Completed { output } => FlowEvent::ActivityCompleted {
                    activity_id: activity_id.to_string(),
                    attempt_id: activity.attempt_id.clone(),
                    fencing_token: activity.fencing_token.clone(),
                    output: output.clone(),
                },
                ActivityResolution::Retry { error, retry_after } => FlowEvent::ActivityRetrying {
                    activity_id: activity_id.to_string(),
                    attempt: activity.attempt,
                    attempt_id: activity.attempt_id.clone(),
                    fencing_token: activity.fencing_token.clone(),
                    error: error.clone(),
                    retry_after: *retry_after,
                },
                ActivityResolution::Failed { error } => FlowEvent::ActivityFailed {
                    activity_id: activity_id.to_string(),
                    attempt: activity.attempt,
                    attempt_id: activity.attempt_id.clone(),
                    fencing_token: activity.fencing_token.clone(),
                    error: error.clone(),
                },
                ActivityResolution::NonRetryable { error } => FlowEvent::ActivityNonRetryable {
                    activity_id: activity_id.to_string(),
                    attempt: activity.attempt,
                    attempt_id: activity.attempt_id.clone(),
                    fencing_token: activity.fencing_token.clone(),
                    error: error.clone(),
                },
                ActivityResolution::Cancelled { reason } => FlowEvent::ActivityCancelled {
                    activity_id: activity_id.to_string(),
                    attempt: activity.attempt,
                    reason: reason.clone(),
                },
            };

            match self
                .record_event_at(run_id, snapshot.last_sequence, event)
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) if super::validation::is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    /// Durably records an activity heartbeat and optional checkpoint.
    ///
    /// The attempt and fencing identities are checked against the current
    /// projection before appending. A stale worker therefore receives a
    /// deterministic transition error instead of being able to overwrite a
    /// newer attempt's checkpoint.
    pub async fn heartbeat_activity(
        &self,
        run_id: &str,
        activity_id: &str,
        attempt: u32,
        attempt_id: &str,
        fencing_token: &str,
        checkpoint: Option<serde_json::Value>,
    ) -> Result<()> {
        for _ in 0..self.max_replay_iterations {
            let snapshot = self.snapshot(run_id).await?;
            let activity = snapshot.activities.get(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity {activity_id} does not exist for run {run_id}"
                ))
            })?;
            if activity.status != ActivityStatus::Running
                || activity.attempt != attempt
                || activity.attempt_id != attempt_id
                || activity.fencing_token != fencing_token
            {
                return Err(FlowError::InvalidTransition(format!(
                    "activity {activity_id} heartbeat fencing identity is stale"
                )));
            }
            match self
                .record_event_at(
                    run_id,
                    snapshot.last_sequence,
                    FlowEvent::ActivityHeartbeat {
                        activity_id: activity_id.to_string(),
                        attempt,
                        attempt_id: attempt_id.to_string(),
                        fencing_token: fencing_token.to_string(),
                        checkpoint: checkpoint.clone(),
                    },
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) if super::validation::is_event_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(FlowError::ReplayLimitExceeded(self.max_replay_iterations))
    }

    pub(super) async fn execute_activity(
        &self,
        run_id: &str,
        snapshot: &WorkflowRunSnapshot,
        context: ActivityExecutionContext,
    ) -> Result<()> {
        let ActivityExecutionContext {
            activity_id,
            activity_name,
            input,
            retry,
            now,
        } = context;
        let mut expected_sequence = snapshot.last_sequence;
        if let Some(activity) = snapshot.activities.get(&activity_id) {
            ensure_activity_command_matches(run_id, activity, &activity_name, &input, retry)?;
            if matches!(
                activity.status,
                ActivityStatus::Completed
                    | ActivityStatus::Failed
                    | ActivityStatus::Cancelled
                    | ActivityStatus::Unknown
            ) {
                return Ok(());
            }
            if activity.status == ActivityStatus::Pending
                && activity
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
                    FlowEvent::ActivityCreated {
                        activity_id: activity_id.clone(),
                        activity_name: activity_name.clone(),
                        input: input.clone(),
                        retry,
                    },
                )
                .await?;
            expected_sequence = envelope.sequence;
        }

        let max_attempts = retry.max_attempts.max(1);
        let mut attempt = snapshot
            .activities
            .get(&activity_id)
            .map(|activity| activity.attempt)
            .unwrap_or(0);
        let mut redelivering_running = snapshot
            .activities
            .get(&activity_id)
            .is_some_and(|activity| activity.status == ActivityStatus::Running);
        let mut attempt_id = snapshot
            .activities
            .get(&activity_id)
            .map(|activity| activity.attempt_id.clone())
            .unwrap_or_default();
        let mut fencing_token;

        loop {
            if redelivering_running {
                // The side effect may have completed before a host crash. Re-run
                // the same attempt and idempotency identity so the host can
                // reconcile the ambiguous boundary without duplication.
                redelivering_running = false;
                fencing_token = Uuid::new_v4().to_string();
                self.record_event_at(
                    run_id,
                    expected_sequence,
                    FlowEvent::ActivityLeaseAcquired {
                        activity_id: activity_id.clone(),
                        attempt,
                        attempt_id: attempt_id.clone(),
                        fencing_token: fencing_token.clone(),
                    },
                )
                .await?;
            } else {
                attempt = attempt.checked_add(1).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "activity attempt overflowed for {activity_id}"
                    ))
                })?;
                attempt_id = activity_attempt_id(run_id, &activity_id, attempt);
                fencing_token = Uuid::new_v4().to_string();
                self.record_event_at(
                    run_id,
                    expected_sequence,
                    FlowEvent::ActivityStarted {
                        activity_id: activity_id.clone(),
                        attempt,
                        attempt_id: attempt_id.clone(),
                        idempotency_key: activity_attempt_idempotency_key(
                            run_id,
                            &activity_id,
                            attempt,
                        ),
                        fencing_token: fencing_token.clone(),
                    },
                )
                .await?;
            }

            let history = self.store.list(run_id).await?;
            let invocation = ActivityInvocation {
                run_id: run_id.to_string(),
                activity_id: activity_id.clone(),
                attempt,
                attempt_id: attempt_id.clone(),
                activity_name: activity_name.clone(),
                input: input.clone(),
                history,
                idempotency_key: activity_attempt_idempotency_key(run_id, &activity_id, attempt),
                fencing_token: fencing_token.clone(),
            };

            match self.runtime.run_activity(invocation).await {
                Ok(output) => {
                    let expected_sequence = self.snapshot(run_id).await?.last_sequence;
                    self.record_event_at(
                        run_id,
                        expected_sequence,
                        FlowEvent::ActivityCompleted {
                            activity_id,
                            attempt_id,
                            fencing_token,
                            output,
                        },
                    )
                    .await?;
                    return Ok(());
                }
                Err(FlowError::UnknownOutcome(reason)) => {
                    let expected_sequence = self.snapshot(run_id).await?.last_sequence;
                    self.record_event_at(
                        run_id,
                        expected_sequence,
                        FlowEvent::ActivityUnknown {
                            activity_id: activity_id.clone(),
                            attempt,
                            attempt_id: attempt_id.clone(),
                            fencing_token: fencing_token.clone(),
                            reason,
                        },
                    )
                    .await?;
                    return Ok(());
                }
                Err(err) if !err.is_retryable() => {
                    let error = err.to_string();
                    let expected_sequence = self.snapshot(run_id).await?.last_sequence;
                    let failed = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::ActivityNonRetryable {
                                activity_id: activity_id.clone(),
                                attempt,
                                attempt_id: attempt_id.clone(),
                                fencing_token: fencing_token.clone(),
                                error: error.clone(),
                            },
                        )
                        .await?;
                    if retry.on_exhausted == crate::model::StepFailureAction::ContinueWorkflow {
                        return Ok(());
                    }
                    self.record_event_at(
                        run_id,
                        failed.sequence,
                        FlowEvent::RunFailed {
                            error: format!("activity {activity_id} is non-retryable: {error}"),
                        },
                    )
                    .await?;
                    return Ok(());
                }
                Err(err) if attempt < max_attempts => {
                    let current_sequence = self.snapshot(run_id).await?.last_sequence;
                    let retry_after = retry.retry_after_for_step(
                        super::steps::retry_anchor(now),
                        attempt,
                        run_id,
                        &activity_id,
                    )?;
                    let retrying = self
                        .record_event_at(
                            run_id,
                            current_sequence,
                            FlowEvent::ActivityRetrying {
                                activity_id: activity_id.clone(),
                                attempt,
                                attempt_id: attempt_id.clone(),
                                fencing_token: fencing_token.clone(),
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
                    let expected_sequence = self.snapshot(run_id).await?.last_sequence;
                    let failed = self
                        .record_event_at(
                            run_id,
                            expected_sequence,
                            FlowEvent::ActivityFailed {
                                activity_id: activity_id.clone(),
                                attempt,
                                attempt_id,
                                fencing_token,
                                error: error.clone(),
                            },
                        )
                        .await?;
                    if retry.on_exhausted == crate::model::StepFailureAction::ContinueWorkflow {
                        return Ok(());
                    }
                    self.record_event_at(
                        run_id,
                        failed.sequence,
                        FlowEvent::RunFailed {
                            error: format!("activity {activity_id} retry exhausted: {error}"),
                        },
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    }
}

fn activity_resolution_matches_snapshot(
    activity: &crate::model::ActivitySnapshot,
    resolution: &ActivityResolution,
) -> bool {
    match resolution {
        ActivityResolution::Completed { output } => {
            activity.status == ActivityStatus::Completed && activity.output.as_ref() == Some(output)
        }
        ActivityResolution::Retry { error, retry_after } => {
            activity.status == ActivityStatus::Pending
                && activity.error.as_deref() == Some(error.as_str())
                && activity.retry_after == *retry_after
        }
        ActivityResolution::Failed { error } | ActivityResolution::NonRetryable { error } => {
            activity.status == ActivityStatus::Failed
                && activity.error.as_deref() == Some(error.as_str())
        }
        ActivityResolution::Cancelled { reason } => {
            activity.status == ActivityStatus::Cancelled
                && activity.error.as_deref() == Some(reason.as_str())
        }
    }
}
