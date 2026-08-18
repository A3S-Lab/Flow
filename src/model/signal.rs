use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

use super::JsonValue;

/// One durable asynchronous message delivered to a workflow execution.
///
/// `signal_id` is the caller-owned idempotency identity. A caller must reuse
/// both the target run ID and this value when retrying an uncertain delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowSignal {
    pub signal_id: String,
    pub name: String,
    pub payload: JsonValue,
}

impl WorkflowSignal {
    pub fn new(signal_id: impl Into<String>, name: impl Into<String>, payload: JsonValue) -> Self {
        Self {
            signal_id: signal_id.into(),
            name: name.into(),
            payload,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.signal_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "workflow signal id must not be empty".to_string(),
            ));
        }
        validate_signal_name(&self.name)
    }
}

/// A received signal together with its durable history position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowSignalSnapshot {
    pub signal_id: String,
    pub name: String,
    pub payload: JsonValue,
    pub received_at: DateTime<Utc>,
    pub received_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_by: Option<String>,
}

impl WorkflowSignalSnapshot {
    /// Decode the persisted signal payload into a host-defined serde type.
    pub fn payload_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.payload.clone()).map_err(FlowError::from)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalWaitStatus {
    Waiting,
    Completed,
    Cancelled,
}

/// A deterministic workflow wait for the next unconsumed signal of one name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalWaitSnapshot {
    pub wait_id: String,
    pub signal_name: String,
    pub status: SignalWaitStatus,
    pub created_at: DateTime<Utc>,
    pub created_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_sequence: Option<u64>,
}

pub(crate) fn validate_signal_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "workflow signal name must not be empty".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_signal_wait(wait_id: &str, signal_name: &str) -> Result<()> {
    if wait_id.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "workflow signal wait id must not be empty".to_string(),
        ));
    }
    validate_signal_name(signal_name)
}
