use crate::error::{FlowError, Result};

use super::{
    validate_run_id, ActivityStatus, CancellationRequestSnapshot, ChildWorkflowSnapshot, FlowEvent,
    FlowEventEnvelope, HookSnapshot, HookStatus, SignalWaitSnapshot, SignalWaitStatus,
    StepFailureAction, StepSnapshot, StepStatus, WaitSnapshot, WaitStatus, WorkflowContinuation,
    WorkflowRunSnapshot, WorkflowRunStatus, WorkflowSignalSnapshot, WorkflowTerminalOutcome,
};

mod activity;

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
    spec.validate()?;

    project_run_from_snapshot(
        run_id,
        WorkflowRunSnapshot::new(run_id, spec, input),
        events,
    )
}

/// Continue projecting a run from a previously validated materialized state.
///
/// The caller must provide only the contiguous tail immediately after
/// `snapshot.last_sequence`. The same reducer and transition checks used for a
/// complete replay are applied, so an incremental projection cannot weaken
/// history validation.
pub(crate) fn project_run_from_snapshot(
    run_id: &str,
    mut snapshot: WorkflowRunSnapshot,
    events: &[FlowEventEnvelope],
) -> Result<WorkflowRunSnapshot> {
    // Suspended is a derived state: it is recomputed after reducing the tail.
    // Treat a checkpoint in that state as running while applying new events so
    // the incremental path has the same transition semantics as a full replay.
    if snapshot.status == WorkflowRunStatus::Suspended {
        snapshot.status = WorkflowRunStatus::Running;
    }

    for envelope in events {
        envelope.validate_schema_version()?;
        let expected_sequence = snapshot.last_sequence.checked_add(1).ok_or_else(|| {
            FlowError::InvalidTransition(format!(
                "event sequence overflowed while projecting run {run_id}"
            ))
        })?;
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
        if snapshot.status.is_terminal() {
            return Err(FlowError::InvalidTransition(format!(
                "event {} appears after terminal run state",
                envelope.event.event_key()
            )));
        }
        let is_first_event = snapshot.last_sequence == 0;
        if is_first_event && !matches!(&envelope.event, FlowEvent::RunCreated { .. }) {
            return Err(FlowError::InvalidTransition(
                "first run event must be run_created".to_string(),
            ));
        }
        snapshot.last_sequence = envelope.sequence;
        match &envelope.event {
            FlowEvent::RunCreated { .. } => {
                if !is_first_event {
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
                ensure_no_blocking_child_workflows(&snapshot)?;
                snapshot.status = WorkflowRunStatus::Completed;
                snapshot.output = Some(output.clone());
                snapshot.error = None;
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::Completed {
                    output: output.clone(),
                });
            }
            FlowEvent::RunFailed { error } => {
                ensure_no_blocking_child_workflows(&snapshot)?;
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
                for activity in snapshot.activities.values_mut() {
                    if matches!(
                        activity.status,
                        ActivityStatus::Pending | ActivityStatus::Running | ActivityStatus::Unknown
                    ) {
                        activity.status = ActivityStatus::Cancelled;
                        activity.retry_after = None;
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
                for wait in snapshot.signal_waits.values_mut() {
                    if wait.status == SignalWaitStatus::Waiting {
                        wait.status = SignalWaitStatus::Cancelled;
                    }
                }
            }
            FlowEvent::RunCancelled { reason } => {
                ensure_no_blocking_child_workflows(&snapshot)?;
                snapshot.status = WorkflowRunStatus::Cancelled;
                snapshot.error = reason.clone();
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::Cancelled {
                    reason: reason.clone(),
                });
            }
            FlowEvent::RunTimedOut { deadline, reason } => {
                ensure_no_blocking_child_workflows(&snapshot)?;
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
                ensure_no_blocking_child_workflows(&snapshot)?;
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
                ensure_no_blocking_child_workflows(&snapshot)?;
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
            FlowEvent::RunContinuedAsNew {
                successor_run_id,
                input,
            } => {
                if snapshot.status != WorkflowRunStatus::Running {
                    return Err(FlowError::InvalidTransition(
                        "run_continued_as_new can only follow a running run".to_string(),
                    ));
                }
                if snapshot
                    .child_workflows
                    .values()
                    .any(ChildWorkflowSnapshot::is_open)
                {
                    return Err(FlowError::InvalidTransition(
                        "run_continued_as_new cannot abandon an open child workflow".to_string(),
                    ));
                }
                if snapshot
                    .signal_waits
                    .values()
                    .any(|wait| wait.status == SignalWaitStatus::Waiting)
                {
                    return Err(FlowError::InvalidTransition(
                        "run_continued_as_new cannot abandon an open signal wait".to_string(),
                    ));
                }
                if let Some(signal) = snapshot
                    .signals
                    .iter()
                    .find(|signal| signal.consumed_by.is_none())
                {
                    return Err(FlowError::InvalidTransition(format!(
                        "run_continued_as_new cannot abandon unconsumed signal {}",
                        signal.signal_id
                    )));
                }
                validate_run_id(successor_run_id)?;
                snapshot.status = WorkflowRunStatus::ContinuedAsNew;
                snapshot.output = None;
                snapshot.error = None;
                snapshot.continuation = Some(WorkflowContinuation {
                    successor_run_id: successor_run_id.clone(),
                    input: input.clone(),
                });
                snapshot.terminal_outcome = Some(WorkflowTerminalOutcome::ContinuedAsNew {
                    successor_run_id: successor_run_id.clone(),
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
            FlowEvent::ChildWorkflowRequested {
                child_id,
                child_run_id,
                spec,
                input,
                cancellation_policy,
            } => {
                if snapshot.child_workflows.contains_key(child_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "child_workflow_requested duplicates child {child_id}"
                    )));
                }
                let child = ChildWorkflowSnapshot {
                    child_id: child_id.clone(),
                    run_id: child_run_id.clone(),
                    spec: spec.clone(),
                    input: input.clone(),
                    cancellation_policy: *cancellation_policy,
                    requested_at: envelope.timestamp,
                    requested_sequence: envelope.sequence,
                    outcome: None,
                    resolved_at: None,
                    resolved_sequence: None,
                };
                child.validate_request()?;
                snapshot.child_workflows.insert(child_id.clone(), child);
            }
            FlowEvent::ChildWorkflowResolved { child_id, outcome } => {
                if matches!(outcome, WorkflowTerminalOutcome::ContinuedAsNew { .. }) {
                    return Err(FlowError::InvalidTransition(format!(
                        "child workflow {child_id} cannot resolve to a continuation segment"
                    )));
                }
                let child = snapshot.child_workflows.get_mut(child_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "child_workflow_resolved references unknown child {child_id}"
                    ))
                })?;
                if !child.is_open() {
                    return Err(FlowError::InvalidTransition(format!(
                        "child_workflow_resolved duplicates child {child_id}"
                    )));
                }
                child.outcome = Some(outcome.clone());
                child.resolved_at = Some(envelope.timestamp);
                child.resolved_sequence = Some(envelope.sequence);
            }
            FlowEvent::SignalReceived { signal } => {
                if snapshot.status == WorkflowRunStatus::Pending {
                    return Err(FlowError::InvalidTransition(
                        "signal_received cannot precede run_started".to_string(),
                    ));
                }
                signal.validate()?;
                if !snapshot.spec.accepts_signal(&signal.name) {
                    return Err(FlowError::InvalidTransition(format!(
                        "signal {} uses undeclared workflow signal name {}",
                        signal.signal_id, signal.name
                    )));
                }
                if snapshot
                    .signals
                    .iter()
                    .any(|existing| existing.signal_id == signal.signal_id)
                {
                    return Err(FlowError::InvalidTransition(format!(
                        "signal_received duplicates signal {}",
                        signal.signal_id
                    )));
                }
                snapshot.signals.push(WorkflowSignalSnapshot {
                    signal_id: signal.signal_id.clone(),
                    name: signal.name.clone(),
                    payload: signal.payload.clone(),
                    received_at: envelope.timestamp,
                    received_sequence: envelope.sequence,
                    consumed_by: None,
                });
            }
            FlowEvent::SignalWaitCreated {
                wait_id,
                signal_name,
            } => {
                if snapshot.status == WorkflowRunStatus::Pending {
                    return Err(FlowError::InvalidTransition(
                        "signal_wait_created cannot precede run_started".to_string(),
                    ));
                }
                super::validate_signal_wait(wait_id, signal_name)?;
                if !snapshot.spec.accepts_signal(signal_name) {
                    return Err(FlowError::InvalidTransition(format!(
                        "signal wait {wait_id} uses undeclared workflow signal name {signal_name}"
                    )));
                }
                if snapshot.signal_waits.contains_key(wait_id) {
                    return Err(FlowError::InvalidTransition(format!(
                        "signal_wait_created duplicates wait {wait_id}"
                    )));
                }
                snapshot.signal_waits.insert(
                    wait_id.clone(),
                    SignalWaitSnapshot {
                        wait_id: wait_id.clone(),
                        signal_name: signal_name.clone(),
                        status: SignalWaitStatus::Waiting,
                        created_at: envelope.timestamp,
                        created_sequence: envelope.sequence,
                        signal_id: None,
                        completed_at: None,
                        completed_sequence: None,
                    },
                );
            }
            FlowEvent::SignalWaitCompleted { wait_id, signal_id } => {
                if snapshot.status == WorkflowRunStatus::Pending {
                    return Err(FlowError::InvalidTransition(
                        "signal_wait_completed cannot precede run_started".to_string(),
                    ));
                }
                let wait = snapshot.signal_waits.get(wait_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "signal_wait_completed references unknown wait {wait_id}"
                    ))
                })?;
                if wait.status != SignalWaitStatus::Waiting {
                    return Err(FlowError::InvalidTransition(format!(
                        "signal_wait_completed cannot follow {:?} for wait {wait_id}",
                        wait.status
                    )));
                }
                let signal_index = snapshot
                    .signals
                    .iter()
                    .position(|signal| signal.signal_id == *signal_id)
                    .ok_or_else(|| {
                        FlowError::InvalidTransition(format!(
                            "signal_wait_completed references unknown signal {signal_id}"
                        ))
                    })?;
                let signal = &snapshot.signals[signal_index];
                if signal.name != wait.signal_name {
                    return Err(FlowError::InvalidTransition(format!(
                        "signal_wait_completed pairs wait {wait_id} for {} with signal {signal_id} named {}",
                        wait.signal_name, signal.name
                    )));
                }
                if let Some(consumed_by) = &signal.consumed_by {
                    return Err(FlowError::InvalidTransition(format!(
                        "signal_wait_completed reuses signal {signal_id} consumed by wait {consumed_by}"
                    )));
                }
                ensure_signal_pair_is_fifo(&snapshot, wait, signal)?;

                snapshot.signals[signal_index].consumed_by = Some(wait_id.clone());
                let wait = snapshot.signal_waits.get_mut(wait_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "signal_wait_completed lost wait {wait_id} during projection"
                    ))
                })?;
                wait.status = SignalWaitStatus::Completed;
                wait.signal_id = Some(signal_id.clone());
                wait.completed_at = Some(envelope.timestamp);
                wait.completed_sequence = Some(envelope.sequence);
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
            FlowEvent::StepNonRetryable {
                step_id,
                attempt,
                error,
            } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_non_retryable references unknown step {step_id}"
                    ))
                })?;
                if step.status != StepStatus::Running {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_non_retryable cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                if *attempt != step.attempt {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_non_retryable attempt {attempt} does not match running attempt {} for step {step_id}",
                        step.attempt
                    )));
                }
                step.status = StepStatus::Failed;
                step.attempt = *attempt;
                step.error = Some(error.clone());
                step.retry_after = None;
            }
            FlowEvent::StepCancelled {
                step_id,
                attempt,
                reason,
            } => {
                let step = snapshot.steps.get_mut(step_id).ok_or_else(|| {
                    FlowError::InvalidTransition(format!(
                        "step_cancelled references unknown step {step_id}"
                    ))
                })?;
                if !matches!(step.status, StepStatus::Pending | StepStatus::Running) {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_cancelled cannot follow {:?} for step {step_id}",
                        step.status
                    )));
                }
                if *attempt != step.attempt {
                    return Err(FlowError::InvalidTransition(format!(
                        "step_cancelled attempt {attempt} does not match current attempt {} for step {step_id}",
                        step.attempt
                    )));
                }
                step.status = StepStatus::Cancelled;
                step.error = Some(reason.clone());
                step.retry_after = None;
            }
            FlowEvent::ActivityCreated { .. }
            | FlowEvent::ActivityStarted { .. }
            | FlowEvent::ActivityLeaseAcquired { .. }
            | FlowEvent::ActivityCompleted { .. }
            | FlowEvent::ActivityRetrying { .. }
            | FlowEvent::ActivityFailed { .. }
            | FlowEvent::ActivityNonRetryable { .. }
            | FlowEvent::ActivityUnknown { .. }
            | FlowEvent::ActivityHeartbeat { .. }
            | FlowEvent::ActivityCancelled { .. } => {
                activity::project_activity(&mut snapshot, envelope)?;
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

fn ensure_signal_pair_is_fifo(
    snapshot: &WorkflowRunSnapshot,
    wait: &SignalWaitSnapshot,
    signal: &WorkflowSignalSnapshot,
) -> Result<()> {
    let oldest_wait = snapshot
        .signal_waits
        .values()
        .filter(|candidate| {
            candidate.status == SignalWaitStatus::Waiting
                && candidate.signal_name == wait.signal_name
        })
        .min_by_key(|candidate| candidate.created_sequence);
    if let Some(older) = oldest_wait.filter(|candidate| candidate.wait_id != wait.wait_id) {
        return Err(FlowError::InvalidTransition(format!(
            "signal_wait_completed for wait {} skips older wait {} for signal name {}",
            wait.wait_id, older.wait_id, wait.signal_name
        )));
    }

    let oldest_signal = snapshot
        .signals
        .iter()
        .filter(|candidate| candidate.consumed_by.is_none() && candidate.name == signal.name)
        .min_by_key(|candidate| candidate.received_sequence);
    if let Some(older) = oldest_signal.filter(|candidate| candidate.signal_id != signal.signal_id) {
        return Err(FlowError::InvalidTransition(format!(
            "signal_wait_completed for wait {} skips older signal {} for signal name {}",
            wait.wait_id, older.signal_id, wait.signal_name
        )));
    }

    Ok(())
}

fn ensure_no_blocking_child_workflows(snapshot: &WorkflowRunSnapshot) -> Result<()> {
    if let Some(child) = snapshot.child_workflows.values().find(|child| {
        child.is_open()
            && child.cancellation_policy
                == super::ChildWorkflowCancellationPolicy::RequestCancellation
    }) {
        return Err(FlowError::InvalidTransition(format!(
            "workflow run {} cannot terminate while child workflow {} is open",
            snapshot.run_id, child.child_id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    fn envelope(run_id: &str, sequence: u64, event: FlowEvent) -> FlowEventEnvelope {
        FlowEventEnvelope::new(
            run_id,
            sequence,
            Uuid::from_u128(u128::from(sequence)),
            Utc::now(),
            event,
        )
    }

    fn history(run_id: &str) -> Vec<FlowEventEnvelope> {
        vec![
            envelope(
                run_id,
                1,
                FlowEvent::RunCreated {
                    spec: super::super::WorkflowSpec::rust_embedded(
                        "projection.test",
                        "1",
                        "tests::projection",
                        "main",
                    ),
                    input: json!({"source": "projection-test"}),
                },
            ),
            envelope(run_id, 2, FlowEvent::RunStarted),
            envelope(
                run_id,
                3,
                FlowEvent::WaitCreated {
                    wait_id: "pause".to_string(),
                    resume_at: "2030-01-01T00:00:00Z".parse().unwrap(),
                },
            ),
            envelope(
                run_id,
                4,
                FlowEvent::WaitCompleted {
                    wait_id: "pause".to_string(),
                },
            ),
        ]
    }

    #[test]
    fn incremental_projection_matches_full_replay_for_multiple_events() {
        let run_id = "projection-tail";
        let events = history(run_id);
        let full = project_run(run_id, &events).unwrap();
        let checkpoint = project_run(run_id, &events[..2]).unwrap();
        let incremental = project_run_from_snapshot(run_id, checkpoint, &events[2..]).unwrap();
        assert_eq!(incremental, full);
    }

    #[test]
    fn incremental_projection_recomputes_derived_suspension_state() {
        let run_id = "projection-suspension";
        let events = history(run_id);
        let full = project_run(run_id, &events).unwrap();
        let checkpoint = project_run(run_id, &events[..3]).unwrap();
        assert_eq!(checkpoint.status, WorkflowRunStatus::Suspended);
        let incremental = project_run_from_snapshot(run_id, checkpoint, &events[3..]).unwrap();
        assert_eq!(incremental, full);
    }
}
