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
    /// Caller-owned idempotency identity.
    pub signal_id: String,
    /// Declared signal contract name.
    pub name: String,
    /// Application-defined JSON payload.
    pub payload: JsonValue,
}

impl WorkflowSignal {
    /// Creates a signal delivery with a stable caller-owned identity.
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
    /// Caller-owned idempotency identity.
    pub signal_id: String,
    /// Declared signal contract name.
    pub name: String,
    /// Application-defined JSON payload.
    pub payload: JsonValue,
    /// UTC time at which the signal was persisted.
    pub received_at: DateTime<Utc>,
    /// Event sequence that recorded the delivery.
    pub received_sequence: u64,
    /// Stable wait identity that consumed the signal.
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

/// Materialized lifecycle state of a deterministic signal wait.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum SignalWaitStatus {
    /// The wait has not consumed a signal.
    Waiting,
    /// The wait was paired with one durable signal.
    Completed,
    /// The owning run cancelled the wait.
    Cancelled,
}

/// A deterministic workflow wait for the next unconsumed signal of one name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalWaitSnapshot {
    /// Replay-stable identity of the wait.
    pub wait_id: String,
    /// Signal contract accepted by the wait.
    pub signal_name: String,
    /// Current lifecycle state.
    pub status: SignalWaitStatus,
    /// UTC time at which the wait was created.
    pub created_at: DateTime<Utc>,
    /// Event sequence that created the wait.
    pub created_sequence: u64,
    /// Signal paired with the wait, when completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_id: Option<String>,
    /// UTC time at which the signal was paired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Event sequence that completed the wait.
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
