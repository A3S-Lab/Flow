use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeSet;

use crate::error::{FlowError, Result};
use crate::model::{
    validate_child_workflow_command, ChildOperationReference, ChildWorkflowCancellationPolicy,
    ChildWorkflowCommand, ChildWorkflowSnapshot, HookSnapshot, RetryPolicy, SignalWaitSnapshot,
    StepCommand, StepSnapshot, WaitSnapshot, WorkflowProgress, WorkflowRunSnapshot, WorkflowSpec,
    MAX_CHILD_WORKFLOW_BATCH_SIZE,
};

pub(super) fn ensure_same_start(
    run_id: &str,
    snapshot: &WorkflowRunSnapshot,
    spec: &WorkflowSpec,
    input: &serde_json::Value,
) -> Result<()> {
    if snapshot.spec != *spec {
        return Err(FlowError::RunConflict {
            run_id: run_id.to_string(),
            reason: "workflow spec differs".to_string(),
        });
    }
    if snapshot.input != *input {
        return Err(FlowError::RunConflict {
            run_id: run_id.to_string(),
            reason: "workflow input differs".to_string(),
        });
    }
    Ok(())
}

pub(super) fn ensure_step_batch_valid(steps: &[StepCommand]) -> Result<()> {
    if steps.is_empty() {
        return Err(FlowError::InvalidTransition(
            "schedule_steps requires at least one step".to_string(),
        ));
    }

    let mut ids = BTreeSet::new();
    for step in steps {
        if step.step_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "scheduled step id must not be empty".to_string(),
            ));
        }
        if !ids.insert(step.step_id.as_str()) {
            return Err(FlowError::InvalidTransition(format!(
                "schedule_steps contains duplicate step id {}",
                step.step_id
            )));
        }
    }

    Ok(())
}

pub(super) fn ensure_child_workflow_batch_valid(children: &[ChildWorkflowCommand]) -> Result<()> {
    if children.is_empty() {
        return Err(FlowError::InvalidTransition(
            "start_child_workflows requires at least one child".to_string(),
        ));
    }
    if children.len() > MAX_CHILD_WORKFLOW_BATCH_SIZE {
        return Err(FlowError::InvalidTransition(format!(
            "start_child_workflows batch size {} exceeds {MAX_CHILD_WORKFLOW_BATCH_SIZE}",
            children.len()
        )));
    }

    let mut ids = BTreeSet::new();
    for child in children {
        validate_child_workflow_command(&child.child_id, &child.spec)?;
        if !ids.insert(child.child_id.as_str()) {
            return Err(FlowError::InvalidTransition(format!(
                "start_child_workflows contains duplicate child id {}",
                child.child_id
            )));
        }
    }
    Ok(())
}

pub(super) fn ensure_retry_policy_valid(retry: RetryPolicy) -> Result<()> {
    retry.retry_after(Utc::now()).map(|_| ())
}

pub(super) fn ensure_step_command_matches(
    run_id: &str,
    step: &StepSnapshot,
    step_name: &str,
    input: &serde_json::Value,
    retry: RetryPolicy,
) -> Result<()> {
    if step.step_name != step_name {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "step {} name differs: {}",
                step.step_id,
                replay_diff(&step.step_name, &step_name.to_string())
            ),
        });
    }
    if step.input != *input {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "step {} input differs: {}",
                step.step_id,
                replay_diff(&step.input, input)
            ),
        });
    }
    if step.retry != retry {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "step {} retry policy differs: {}",
                step.step_id,
                replay_diff(&step.retry, &retry)
            ),
        });
    }
    Ok(())
}

pub(super) fn ensure_wait_command_matches(
    run_id: &str,
    wait: &WaitSnapshot,
    resume_at: DateTime<Utc>,
) -> Result<()> {
    if wait.resume_at != resume_at {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "wait {} resume_at differs: {}",
                wait.wait_id,
                replay_diff(&wait.resume_at, &resume_at)
            ),
        });
    }
    Ok(())
}

pub(super) fn ensure_hook_command_matches(
    run_id: &str,
    hook: &HookSnapshot,
    token: &str,
    metadata: &serde_json::Value,
) -> Result<()> {
    if hook.token != token {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "hook {} token differs: history token and replay token are different (values redacted)",
                hook.hook_id
            ),
        });
    }
    if hook.metadata != *metadata {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "hook {} metadata differs: {}",
                hook.hook_id,
                replay_diff(&hook.metadata, metadata)
            ),
        });
    }
    Ok(())
}

pub(super) fn ensure_signal_wait_command_matches(
    run_id: &str,
    wait: &SignalWaitSnapshot,
    signal_name: &str,
) -> Result<()> {
    if wait.signal_name != signal_name {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "signal wait {} name differs: {}",
                wait.wait_id,
                replay_diff(&wait.signal_name, &signal_name.to_string())
            ),
        });
    }
    Ok(())
}

pub(super) fn ensure_progress_matches(
    run_id: &str,
    existing: &WorkflowProgress,
    replay: &WorkflowProgress,
) -> Result<()> {
    if existing != replay {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "progress {} differs: {}",
                existing.progress_id,
                replay_diff(existing, replay)
            ),
        });
    }
    Ok(())
}

pub(super) fn ensure_child_operation_matches(
    run_id: &str,
    existing: &ChildOperationReference,
    replay: &ChildOperationReference,
) -> Result<()> {
    if existing != replay {
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "child operation {} differs: {}",
                existing.reference_id,
                replay_diff(existing, replay)
            ),
        });
    }
    Ok(())
}

pub(super) fn ensure_child_workflow_command_matches(
    run_id: &str,
    existing: &ChildWorkflowSnapshot,
    spec: &WorkflowSpec,
    input: &serde_json::Value,
    cancellation_policy: ChildWorkflowCancellationPolicy,
) -> Result<()> {
    if existing.spec != *spec
        || existing.input != *input
        || existing.cancellation_policy != cancellation_policy
    {
        let replay = (spec, input, cancellation_policy);
        let history = (
            &existing.spec,
            &existing.input,
            existing.cancellation_policy,
        );
        return Err(FlowError::NonDeterministic {
            run_id: run_id.to_string(),
            reason: format!(
                "child workflow {} differs: {}",
                existing.child_id,
                replay_diff(&history, &replay)
            ),
        });
    }
    Ok(())
}

fn replay_diff<T>(history: &T, replay: &T) -> String
where
    T: Serialize,
{
    format!(
        "history={}; replay={}",
        compact_json(history),
        compact_json(replay)
    )
}

fn compact_json<T>(value: &T) -> String
where
    T: Serialize,
{
    serde_json::to_string(value).unwrap_or_else(|err| format!("<failed to encode: {err}>"))
}

pub(super) fn is_event_conflict(err: &FlowError) -> bool {
    matches!(err, FlowError::EventConflict { .. })
}
