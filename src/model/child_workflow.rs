use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

use super::{validate_run_id, JsonValue, WorkflowSpec, WorkflowTerminalOutcome};

/// Action applied to an open child when its parent enters cancellation.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChildWorkflowCancellationPolicy {
    /// Request cleanup-aware cancellation and keep the parent cancelling until
    /// the child reaches a durable terminal outcome.
    #[default]
    RequestCancellation,
    /// Leave the child independent and allow the parent to finish cancelling.
    Abandon,
}

/// Parent-owned projection of one first-class child workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChildWorkflowSnapshot {
    pub child_id: String,
    pub run_id: String,
    pub spec: WorkflowSpec,
    pub input: JsonValue,
    pub cancellation_policy: ChildWorkflowCancellationPolicy,
    pub requested_at: DateTime<Utc>,
    pub requested_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<WorkflowTerminalOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_sequence: Option<u64>,
}

impl ChildWorkflowSnapshot {
    pub fn is_open(&self) -> bool {
        self.outcome.is_none()
    }

    /// Decode a completed child output into a host-defined serde type.
    pub fn output_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        match &self.outcome {
            Some(WorkflowTerminalOutcome::Completed { output }) => {
                serde_json::from_value(output.clone())
                    .map(Some)
                    .map_err(FlowError::from)
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn validate_request(&self) -> Result<()> {
        validate_child_workflow_request(
            &self.child_id,
            &self.run_id,
            &self.spec,
            self.requested_sequence,
        )
    }
}

pub(crate) fn validate_child_workflow_command(child_id: &str, spec: &WorkflowSpec) -> Result<()> {
    if child_id.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "child workflow id must not be empty".to_string(),
        ));
    }
    spec.validate()
}

fn validate_child_workflow_request(
    child_id: &str,
    run_id: &str,
    spec: &WorkflowSpec,
    requested_sequence: u64,
) -> Result<()> {
    validate_child_workflow_command(child_id, spec)?;
    validate_run_id(run_id)?;
    if requested_sequence == 0 {
        return Err(FlowError::InvalidTransition(format!(
            "child workflow {child_id} requested sequence must be positive"
        )));
    }
    Ok(())
}
