use crate::error::{FlowError, Result};
use crate::model::{ActivitySnapshot, ActivityStatus, FlowEvent, FlowEventEnvelope};

pub(super) fn project_activity(
    snapshot: &mut crate::model::WorkflowRunSnapshot,
    envelope: &FlowEventEnvelope,
) -> Result<()> {
    match &envelope.event {
        FlowEvent::ActivityCreated {
            activity_id,
            activity_name,
            input,
            retry,
        } => {
            if snapshot.activities.contains_key(activity_id) {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_created duplicates activity {activity_id}"
                )));
            }
            retry.retry_after(envelope.timestamp).map(|_| ())?;
            snapshot.activities.insert(
                activity_id.clone(),
                ActivitySnapshot {
                    activity_id: activity_id.clone(),
                    activity_name: activity_name.clone(),
                    status: ActivityStatus::Pending,
                    input: input.clone(),
                    retry: *retry,
                    output: None,
                    error: None,
                    attempt: 0,
                    attempt_id: String::new(),
                    idempotency_key: String::new(),
                    fencing_token: String::new(),
                    checkpoint: None,
                    retry_after: None,
                    last_heartbeat_at: None,
                },
            );
        }
        FlowEvent::ActivityStarted {
            activity_id,
            attempt,
            attempt_id,
            idempotency_key,
            fencing_token,
        } => {
            let activity = snapshot.activities.get_mut(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity_started references unknown activity {activity_id}"
                ))
            })?;
            if activity.status != ActivityStatus::Pending {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_started cannot follow {:?} for activity {activity_id}",
                    activity.status
                )));
            }
            let expected_attempt = activity.attempt.checked_add(1).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity_started cannot advance attempt beyond {} for activity {activity_id}",
                    activity.attempt
                ))
            })?;
            if *attempt != expected_attempt {
                return Err(FlowError::InvalidTransition(format!(
                "activity_started attempt {attempt} must be {expected_attempt} for activity {activity_id}"
            )));
            }
            if attempt_id.is_empty() || idempotency_key.is_empty() || fencing_token.is_empty() {
                return Err(FlowError::InvalidTransition(format!(
                "activity_started requires attempt, idempotency, and fencing identities for {activity_id}"
            )));
            }
            activity.status = ActivityStatus::Running;
            activity.attempt = *attempt;
            activity.attempt_id = attempt_id.clone();
            activity.idempotency_key = idempotency_key.clone();
            activity.fencing_token = fencing_token.clone();
            activity.retry_after = None;
            activity.error = None;
        }
        FlowEvent::ActivityLeaseAcquired {
            activity_id,
            attempt,
            attempt_id,
            fencing_token,
        } => {
            let activity = snapshot.activities.get_mut(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity_lease_acquired references unknown activity {activity_id}"
                ))
            })?;
            if activity.status != ActivityStatus::Running
                || activity.attempt != *attempt
                || activity.attempt_id != *attempt_id
            {
                return Err(FlowError::InvalidTransition(format!(
                "activity_lease_acquired identity does not match running activity {activity_id}"
            )));
            }
            if fencing_token.is_empty() || fencing_token == &activity.fencing_token {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_lease_acquired requires a new fencing token for {activity_id}"
                )));
            }
            activity.fencing_token = fencing_token.clone();
        }
        FlowEvent::ActivityCompleted {
            activity_id,
            attempt_id,
            fencing_token,
            output,
        } => {
            let activity = snapshot.activities.get_mut(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity_completed references unknown activity {activity_id}"
                ))
            })?;
            if !matches!(
                activity.status,
                ActivityStatus::Running | ActivityStatus::Unknown
            ) {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_completed cannot follow {:?} for activity {activity_id}",
                    activity.status
                )));
            }
            if &activity.attempt_id != attempt_id || &activity.fencing_token != fencing_token {
                return Err(FlowError::InvalidTransition(format!(
                "activity_completed fencing identity does not match running activity {activity_id}"
            )));
            }
            activity.status = ActivityStatus::Completed;
            activity.output = Some(output.clone());
            activity.error = None;
            activity.retry_after = None;
        }
        FlowEvent::ActivityRetrying {
            activity_id,
            attempt,
            attempt_id,
            fencing_token,
            error,
            retry_after,
        } => {
            let activity = snapshot.activities.get_mut(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity_retrying references unknown activity {activity_id}"
                ))
            })?;
            if !matches!(
                activity.status,
                ActivityStatus::Running | ActivityStatus::Unknown
            ) || activity.attempt != *attempt
                || activity.attempt_id != *attempt_id
                || activity.fencing_token != *fencing_token
            {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_retrying identity does not match running activity {activity_id}"
                )));
            }
            let max_attempts = activity.retry.max_attempts.max(1);
            if *attempt >= max_attempts {
                return Err(FlowError::InvalidTransition(format!(
                "activity_retrying exceeds retry budget for activity {activity_id}: attempt {attempt}, max_attempts {max_attempts}"
            )));
            }
            if activity.retry.delay_ms > 0 && retry_after.is_none() {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_retrying for delayed activity {activity_id} requires retry_after"
                )));
            }
            if activity.retry.delay_ms == 0 && retry_after.is_some() {
                return Err(FlowError::InvalidTransition(format!(
                "activity_retrying for immediate activity {activity_id} must not include retry_after"
            )));
            }
            activity.status = ActivityStatus::Pending;
            activity.error = Some(error.clone());
            activity.retry_after = *retry_after;
        }
        FlowEvent::ActivityUnknown {
            activity_id,
            attempt,
            attempt_id,
            fencing_token,
            reason,
        } => {
            let activity = snapshot.activities.get_mut(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity_unknown references unknown activity {activity_id}"
                ))
            })?;
            if activity.status != ActivityStatus::Running
                || activity.attempt != *attempt
                || activity.attempt_id != *attempt_id
                || activity.fencing_token != *fencing_token
            {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_unknown identity does not match running activity {activity_id}"
                )));
            }
            if reason.trim().is_empty() {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_unknown requires a reason for {activity_id}"
                )));
            }
            activity.status = ActivityStatus::Unknown;
            activity.error = Some(reason.clone());
            activity.retry_after = None;
        }
        FlowEvent::ActivityFailed {
            activity_id,
            attempt,
            attempt_id,
            fencing_token,
            error,
        }
        | FlowEvent::ActivityNonRetryable {
            activity_id,
            attempt,
            attempt_id,
            fencing_token,
            error,
        } => {
            let activity = snapshot.activities.get_mut(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity failure references unknown activity {activity_id}"
                ))
            })?;
            if !matches!(
                activity.status,
                ActivityStatus::Running | ActivityStatus::Unknown
            ) || activity.attempt != *attempt
                || activity.attempt_id != *attempt_id
                || activity.fencing_token != *fencing_token
            {
                return Err(FlowError::InvalidTransition(format!(
                    "activity failure identity does not match running activity {activity_id}"
                )));
            }
            if matches!(&envelope.event, FlowEvent::ActivityFailed { .. })
                && *attempt < activity.retry.max_attempts.max(1)
            {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_failed before retry budget was exhausted for activity {activity_id}"
                )));
            }
            activity.status = ActivityStatus::Failed;
            activity.error = Some(error.clone());
            activity.retry_after = None;
        }
        FlowEvent::ActivityHeartbeat {
            activity_id,
            attempt,
            attempt_id,
            fencing_token,
            checkpoint,
        } => {
            let activity = snapshot.activities.get_mut(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity_heartbeat references unknown activity {activity_id}"
                ))
            })?;
            if activity.status != ActivityStatus::Running
                || activity.attempt != *attempt
                || activity.attempt_id != *attempt_id
                || activity.fencing_token != *fencing_token
            {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_heartbeat identity does not match running activity {activity_id}"
                )));
            }
            activity.checkpoint = checkpoint.clone();
            activity.last_heartbeat_at = Some(envelope.timestamp);
        }
        FlowEvent::ActivityCancelled {
            activity_id,
            attempt,
            reason,
        } => {
            let activity = snapshot.activities.get_mut(activity_id).ok_or_else(|| {
                FlowError::InvalidTransition(format!(
                    "activity_cancelled references unknown activity {activity_id}"
                ))
            })?;
            if !matches!(
                activity.status,
                ActivityStatus::Pending | ActivityStatus::Running | ActivityStatus::Unknown
            ) || activity.attempt != *attempt
            {
                return Err(FlowError::InvalidTransition(format!(
                    "activity_cancelled cannot follow {:?} for activity {activity_id}",
                    activity.status
                )));
            }
            activity.status = ActivityStatus::Cancelled;
            activity.error = Some(reason.clone());
            activity.retry_after = None;
        }
        _ => {
            return Err(FlowError::InvalidTransition(
                "non-activity event passed to activity projection".to_string(),
            ));
        }
    }
    Ok(())
}
