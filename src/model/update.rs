use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

use super::{JsonValue, WorkflowRunSnapshot};

/// One durable synchronous update delivered to a workflow execution.
///
/// `update_id` is the caller-owned idempotency identity. A caller must reuse
/// both the target run ID and this value when retrying an uncertain delivery.
/// Changing the update name or input for the same identity is an explicit
/// conflict.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct WorkflowUpdate {
    /// Caller-owned idempotency identity.
    pub update_id: String,
    /// Declared update contract name.
    pub name: String,
    /// Application-defined JSON input.
    pub input: JsonValue,
}

impl WorkflowUpdate {
    /// Creates an update delivery with a stable caller-owned identity.
    pub fn new(update_id: impl Into<String>, name: impl Into<String>, input: JsonValue) -> Self {
        Self {
            update_id: update_id.into(),
            name: name.into(),
            input,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.update_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "workflow update id must not be empty".to_string(),
            ));
        }
        validate_update_name(&self.name)
    }
}

/// An applied update together with its durable handler output and history position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct WorkflowUpdateSnapshot {
    /// Caller-owned idempotency identity.
    pub update_id: String,
    /// Declared update contract name.
    pub name: String,
    /// Application-defined JSON input.
    pub input: JsonValue,
    /// JSON output returned by the update handler.
    pub output: JsonValue,
    /// UTC time at which the update was persisted.
    pub applied_at: DateTime<Utc>,
    /// Event sequence that recorded the application.
    pub applied_sequence: u64,
}

impl WorkflowUpdateSnapshot {
    /// Decode the persisted update input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.input.clone()).map_err(FlowError::from)
    }

    /// Decode the persisted update output into a host-defined serde type.
    pub fn output_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.output.clone()).map_err(FlowError::from)
    }
}

/// Result of applying one durable update to a workflow run.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WorkflowUpdateOutcome {
    /// Projected state after the update was applied and the leaf was driven.
    pub snapshot: WorkflowRunSnapshot,
    /// Handler output recorded with the durable update event.
    pub output: JsonValue,
}

pub(crate) fn validate_update_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "workflow update name must not be empty".to_string(),
        ));
    }
    Ok(())
}
