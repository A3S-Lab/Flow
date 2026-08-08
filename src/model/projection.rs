use std::collections::BTreeMap;

use crate::error::{FlowError, Result};

use super::{
    CancellationRequestSnapshot, FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus,
    StepFailureAction, StepSnapshot, StepStatus, WaitSnapshot, WaitStatus, WorkflowRunSnapshot,
    WorkflowRunStatus, WorkflowTerminalOutcome,
};

pub(crate) fn project_run(
    run_id: &str,
    events: &[FlowEventEnvelope],
) -> Result<WorkflowRunSnapshot> {
    let first = events
        .first()
        .ok_or_else(|| FlowError::RunNotFound(run_id.to_string()))?;

    let (spec, input) = match &first.event {
        FlowEvent::RunCreated { spec, input } => (spec.clone(), input.clone()),
        _ => {
            return Err(FlowError::InvalidTransition(
                "first run event must be run_created".to_string(),
            ))
        }
    };

    let mut snapshot = WorkflowRunSnapshot {
        run_id: run_id.to_string(),
        spec,
        input,
        status: WorkflowRunStatus::Pending,
        steps: BTreeMap::new(),
        waits: BTreeMap::new(),
        hooks: BTreeMap::new(),
        cancellation: None,
        progress: Vec::new(),
        child_operations: BTreeMap::new(),
        output: None,
        error: None,
        terminal_outcome: None,
        last_sequence: first.sequence,
    };

    for (index, envelope) in events.iter().enumerate() {
        let expected_sequence = index as u64 + 1;
        if envelope.sequence != expected_sequence {
            return Err(FlowError::InvalidTransition(format!(
                "event sequence must be contiguous for run {run_id}: expected {expected_sequence}, got {}",
                envelope.sequence
            )));
        }
        if envelope.run_id != run_id {
            return Err(FlowError::InvalidTransition(format!(
                "event {} belongs to run {} not {}",
                envelope.event_id, envelope.run_id, run_id
            )));
        }
        if index > 0 && snapshot.status.is_terminal() {
            return Err(FlowError::InvalidTransition(format!(
                "event {} appears after terminal run state",
                envelope.event.event_key()
            )));
        }
        snapshot.last_sequence = envelope.sequence;
        match &envelope.event {
            FlowEvent::RunCreated { .. } => {
                if index > 0 {
                    return Err(FlowError::InvalidTransition(
                        "run_created must only appear as the first event".to_string(),
                    ));
                }
            }
            FlowEvent::RunStarted => {
                if snapshot.status != WorkflowRunStatus::Pending {
                    return Err(FlowError::InvalidTransition(
                        "run_started can only follow a pending run".to_string(),
                    ));
                }
                snapshot.status = WorkflowRunStatus::Running;
            }
            FlowEvent::RunCompleted { output } => {
                if snapshot.status == WorkflowRunStatus::Cancelling {
                    return Err(FlowError::InvalidTransition(
                        "a cancelling run must finish as cancelled or failed".to_string(),
                    ));
                }
                snapshot.status = WorkflowRunStatus::Completed;
                snapshot.output = Some(output.clone());
                snapshot.error = None;
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::Completed {
                    output: output.clone(),
                });
            }
            FlowEvent::RunFailed { error } => {
                snapshot.status = WorkflowRunStatus::Failed;
                snapshot.error = Some(error.clone());
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::Failed {
                    error: error.clone(),
                });
            }
            FlowEvent::RunCancellationRequested { request } => {
                if snapshot.cancellation.is_some() {
                    return Err(FlowError::InvalidTransition(
                        "run_cancellation_requested must occur at most once".to_string(),
                    ));
                }
                snapshot.status = WorkflowRunStatus::Cancelling;
                snapshot.cancellation = Some(CancellationRequestSnapshot {
                    request: request.clone(),
                    requested_at: envelope.timestamp,
                    sequence: envelope.sequence,
                });

                // Work that was open before the request is no longer actionable.
                // Cleanup code must use distinct stable step/wait/hook identities.
                for step in snapshot.steps.values_mut() {
                    if matches!(step.status, StepStatus::Pending | StepStatus::Running) {
                        step.status = StepStatus::Cancelled;
                        step.retry_after = None;
                    }
                }
                for wait in snapshot.waits.values_mut() {
                    if wait.status == WaitStatus::Waiting {
                        wait.status = WaitStatus::Cancelled;
                    }
                }
                for hook in snapshot.hooks.values_mut() {
                    if hook.status == HookStatus::Active {
                        hook.status = HookStatus::Cancelled;
                    }
                }
            }
            FlowEvent::RunCancelled { reason } => {
                snapshot.status = WorkflowRunStatus::Cancelled;
                snapshot.error = reason.clone();
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::Cancelled {
                    reason: reason.clone(),
                });
            }
            FlowEvent::RunTimedOut { deadline, reason } => {
                snapshot.status = WorkflowRunStatus::Failed;
                snapshot.error = Some(
                    reason
                        .clone()
                        .unwrap_or_else(|| format!("workflow timed out at {deadline}")),
                );
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::TimedOut {
                    deadline: *deadline,
                    reason: reason.clone(),
                });
            }
            FlowEvent::RunRetryExhausted {
                step_id,
                attempt,
                error,
            } => {
                let step = snapshot.steps.get(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "run_retry_exhausted references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Failed || step.attempt != *attempt {
                    return Err(FlowError::InvalidTransition(format!(
                        "run_retry_exhausted does not match failed step {step_id} attempt {attempt}"
                    )));
                }
                if step.retry.on_exhausted == StepFailureAction::ContinueWorkflow {
                    return Err(FlowError::InvalidTransition(format!(
                        "run_retry_exhausted conflicts with continue_workflow for step {step_id}"
                    )));
                }
                if step.error.as_deref() != Some(error.as_str()) {
                    return Err(FlowError::InvalidTransition(format!(
                        "run_retry_exhausted error does not match failed step {step_id}"
                    )));
                }
                snapshot.status = WorkflowRunStatus::Failed;
                snapshot.error = Some(error.clone());
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::RetryExhausted {
                    step_id: step_id.clone(),
                    attempt: *attempt,
                    error: error.clone(),
                });
            }
            FlowEvent::RunHostShutdown { reason } => {
                snapshot.status = WorkflowRunStatus::Failed;
                snapshot.error = Some(
                    reason
                        .clone()
                        .unwrap_or_else(|| "workflow terminated by host shutdown".to_string()),
                );
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::HostShutdown {
                    reason: reason.clone(),
                });
            }
            FlowEvent::RunProgressRecorded { progress } => {
                progress.validate()?;
                if snapshot
                    .progress
                    .iter()
                    .any(|existing| existing.progress_id == progress.progress_id)
                {
                    return Err(FlowError::InvalidTransition(format!(
                        "run_progress_recorded duplicates progress {}",
                        progress.progress_id
                    )));
                }
                snapshot.progress.push(progress.clone());
            }
            FlowEvent::ChildOperationLinked { child } => {
                child.validate()?;
                if snapshot.child_operations.contains_key(&child.reference_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "child_operation_linked duplicates reference {}",
                        child.reference_id
                    )));
                }
                snapshot
                    .child_operations
                    .insert(child.reference_id.clone(), child.clone());
            }
            FlowEvent::StepCreated {
                step_id,
                step_name,
                input,
                retry,
            } => {
                if snapshot.steps.contains_key(step_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_created duplicates step {step_id}"
                    )));
                }
                retry.retry_after(envelope.timestamp).map(|_| ())?;
                snapshot.steps.insert(
                    step_id.clone(),
                    StepSnapshot {
                        step_id: step_id.clone(),
                        step_name: step_name.clone(),
                        status: StepStatus::Pending,
                        input: input.clone(),
                        retry: *retry,
                        output: None,
                        error: None,
                        attempt: 0,
                        retry_after: None,
                    },
                );
            }
            FlowEvent::StepStarted { step_id, attempt } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_started references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Pending {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_started cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                let expected_attempt = step.attempt.checked_add(1).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_started cannot advance attempt beyond {} for step {step_id}",
                        step.attempt
                    ))
                })?;
                if *attempt != expected_attempt {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_started attempt {attempt} must be {expected_attempt} for step {step_id}"
                    )));
                }
                step.status = StepStatus::Running;
                step.attempt = *attempt;
                step.retry_after = None;
            }
            FlowEvent::StepCompleted { step_id, output } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_completed references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Running {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_completed cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                step.status = StepStatus::Completed;
                step.output = Some(output.clone());
                step.error = None;
                step.retry_after = None;
            }
            FlowEvent::StepRetrying {
                step_id,
                attempt,
                error,
                retry_after,
            } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_retrying references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Running {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_retrying cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                if *attempt != step.attempt {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_retrying attempt {attempt} does not match running attempt {} for step {step_id}",
                        step.attempt
                    )));
                }
                let max_attempts = step.retry.max_attempts.max(1);
                if *attempt >= max_attempts {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_retrying exceeds retry budget for step {step_id}: attempt {attempt}, max_attempts {max_attempts}"
                    )));
                }
                if step.retry.delay_ms > 0 && retry_after.is_none() {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_retrying for delayed step {step_id} requires retry_after"
                    )));
                }
                if step.retry.delay_ms == 0 && retry_after.is_some() {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_retrying for immediate step {step_id} must not include retry_after"
                    )));
                }
                step.status = StepStatus::Pending;
                step.attempt = *attempt;
                step.error = Some(error.clone());
                step.retry_after = *retry_after;
            }
            FlowEvent::StepFailed {
                step_id,
                attempt,
                error,
            } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_failed references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Running {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_failed cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                if *attempt != step.attempt {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_failed attempt {attempt} does not match running attempt {} for step {step_id}",
                        step.attempt
                    )));
                }
                let max_attempts = step.retry.max_attempts.max(1);
                if *attempt < max_attempts {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_failed before retry budget was exhausted for step {step_id}: attempt {attempt}, max_attempts {max_attempts}"
                    )));
                }
                step.status = StepStatus::Failed;
                step.attempt = *attempt;
                step.error = Some(error.clone());
                step.retry_after = None;
            }
            FlowEvent::WaitCreated { wait_id, resume_at } => {
                if snapshot.waits.contains_key(wait_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "wait_created duplicates wait {wait_id}"
                    )));
                }
                snapshot.waits.insert(
                    wait_id.clone(),
                    WaitSnapshot {
                        wait_id: wait_id.clone(),
                        status: WaitStatus::Waiting,
                        resume_at: *resume_at,
                    },
                );
            }
            FlowEvent::WaitCompleted { wait_id } => {
                let wait = snapshot.waits.get_mut(wait_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "wait_completed references unknown wait {wait_id}"
                    ))
                })?;
                if wait.status != WaitStatus::Waiting {
                    return Err(FlowError::InvalidTransition(format!(
                        "wait_completed cannot follow {:?} for wait {wait_id}",
                        wait.status
                    )));
                }
                wait.status = WaitStatus::Completed;
            }
            FlowEvent::HookCreated {
                hook_id,
                token,
                metadata,
            } => {
                if snapshot.hooks.contains_key(hook_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "hook_created duplicates hook {hook_id}"
                    )));
                }
                snapshot.hooks.insert(
                    hook_id.clone(),
                    HookSnapshot {
                        hook_id: hook_id.clone(),
                        token: token.clone(),
                        status: HookStatus::Active,
                        metadata: metadata.clone(),
                        payload: None,
                    },
                );
            }
            FlowEvent::HookReceived { hook_id, payload } => {
                let hook = snapshot.hooks.get_mut(hook_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "hook_received references unknown hook {hook_id}"
                    ))
                })?;
                if hook.status != HookStatus::Active {
                    return Err(FlowError::InvalidTransition(format!(
                        "hook_received cannot follow {:?} for hook {hook_id}",
                        hook.status
                    )));
                }
                hook.status = HookStatus::Received;
                hook.payload = Some(payload.clone());
            }
            FlowEvent::HookDisposed { hook_id } => {
                let hook = snapshot.hooks.get_mut(hook_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "hook_disposed references unknown hook {hook_id}"
                    ))
                })?;
                if hook.status != HookStatus::Active {
                    return Err(FlowError::InvalidTransition(format!(
                        "hook_disposed cannot follow {:?} for hook {hook_id}",
                        hook.status
                    )));
                }
                hook.status = HookStatus::Disposed;
            }
        }
    }

    if snapshot.status == WorkflowRunStatus::Running && snapshot.has_open_suspension() {
        snapshot.status = WorkflowRunStatus::Suspended;
    }

    Ok(snapshot)
}
