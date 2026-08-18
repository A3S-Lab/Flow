use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

use super::JsonValue;

/// Durable link from a closed history segment to its successor run.
///
/// The successor inherits the predecessor's complete [`super::WorkflowSpec`].
/// Only input changes across the boundary, so replay-code admission and patch
/// markers cannot drift while an execution is segmented.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowContinuation {
    pub successor_run_id: String,
    pub input: JsonValue,
}

impl WorkflowContinuation {
    /// Decode the successor input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.input.clone()).map_err(FlowError::from)
    }
}

/// Durable request for a workflow to stop through its cleanup-aware path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CancellationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl CancellationRequest {
    pub fn new(reason: Option<String>) -> Self {
        Self { reason }
    }
}

/// Projected cancellation request with its durable event position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CancellationRequestSnapshot {
    pub request: CancellationRequest,
    pub requested_at: DateTime<Utc>,
    pub sequence: u64,
}

/// A durable, idempotently identified progress update.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowProgress {
    pub progress_id: String,
    pub completed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "JsonValue::is_null")]
    pub details: JsonValue,
}

impl WorkflowProgress {
    pub fn new(progress_id: impl Into<String>, completed: u64) -> Self {
        Self {
            progress_id: progress_id.into(),
            completed,
            total: None,
            message: None,
            details: JsonValue::Null,
        }
    }

    pub fn with_total(mut self, total: u64) -> Self {
        self.total = Some(total);
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn with_details(mut self, details: JsonValue) -> Self {
        self.details = details;
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.progress_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "workflow progress id must not be empty".to_string(),
            ));
        }
        if self
            .total
            .is_some_and(|total| total == 0 || self.completed > total)
        {
            return Err(FlowError::InvalidTransition(format!(
                "workflow progress {} must satisfy completed <= total and total > 0",
                self.progress_id
            )));
        }
        Ok(())
    }
}

/// Durable reference from a parent workflow to a child operation.
///
/// `flow_run_id` is set only when the child is another A3S Flow run. The
/// reference itself does not imply automatic cancellation; the parent
/// workflow owns propagation through durable, idempotent cleanup steps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChildOperationReference {
    pub reference_id: String,
    pub kind: String,
    pub operation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "JsonValue::is_null")]
    pub metadata: JsonValue,
}

impl ChildOperationReference {
    pub fn new(
        reference_id: impl Into<String>,
        kind: impl Into<String>,
        operation_id: impl Into<String>,
    ) -> Self {
        Self {
            reference_id: reference_id.into(),
            kind: kind.into(),
            operation_id: operation_id.into(),
            flow_run_id: None,
            metadata: JsonValue::Null,
        }
    }

    pub fn with_flow_run_id(mut self, flow_run_id: impl Into<String>) -> Self {
        self.flow_run_id = Some(flow_run_id.into());
        self
    }

    pub fn with_metadata(mut self, metadata: JsonValue) -> Self {
        self.metadata = metadata;
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.reference_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "child operation reference id must not be empty".to_string(),
            ));
        }
        if self.kind.trim().is_empty() {
            return Err(FlowError::InvalidTransition(format!(
                "child operation {} kind must not be empty",
                self.reference_id
            )));
        }
        if self.operation_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(format!(
                "child operation {} operation id must not be empty",
                self.reference_id
            )));
        }
        if self
            .flow_run_id
            .as_deref()
            .is_some_and(|run_id| run_id.trim().is_empty())
        {
            return Err(FlowError::InvalidTransition(format!(
                "child operation {} Flow run id must not be empty",
                self.reference_id
            )));
        }
        Ok(())
    }
}

/// Typed terminal result projected from the final run event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowTerminalOutcome {
    Completed {
        output: JsonValue,
    },
    Failed {
        error: String,
    },
    Cancelled {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    TimedOut {
        deadline: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    RetryExhausted {
        step_id: String,
        attempt: u32,
        error: String,
    },
    HostShutdown {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    ContinuedAsNew {
        successor_run_id: String,
    },
}
