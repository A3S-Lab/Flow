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
    /// Run identifier assigned to the successor history segment.
    pub successor_run_id: String,
    /// Initial JSON input supplied to the successor.
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
    /// Optional operator- or application-supplied reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl CancellationRequest {
    /// Creates a cancellation request with an optional reason.
    pub fn new(reason: Option<String>) -> Self {
        Self { reason }
    }
}

/// Projected cancellation request with its durable event position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CancellationRequestSnapshot {
    /// Immutable request delivered during cleanup replay.
    pub request: CancellationRequest,
    /// UTC time at which the request was persisted.
    pub requested_at: DateTime<Utc>,
    /// Event sequence that introduced the request.
    pub sequence: u64,
}

/// A durable, idempotently identified progress update.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowProgress {
    /// Caller-chosen idempotency identity for this update.
    pub progress_id: String,
    /// Number of completed work units.
    pub completed: u64,
    /// Optional total number of work units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Optional human-readable progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Application-defined structured progress details.
    #[serde(default, skip_serializing_if = "JsonValue::is_null")]
    pub details: JsonValue,
}

impl WorkflowProgress {
    /// Creates a progress update without a total, message, or details.
    pub fn new(progress_id: impl Into<String>, completed: u64) -> Self {
        Self {
            progress_id: progress_id.into(),
            completed,
            total: None,
            message: None,
            details: JsonValue::Null,
        }
    }

    /// Sets the total number of work units.
    pub fn with_total(mut self, total: u64) -> Self {
        self.total = Some(total);
        self
    }

    /// Sets a human-readable progress message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Sets application-defined structured details.
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
    /// Replay-stable parent-local identity of the reference.
    pub reference_id: String,
    /// Application-defined operation kind.
    pub kind: String,
    /// Identifier assigned by the child operation's owner.
    pub operation_id: String,
    /// Linked A3S Flow run identifier, when the child is a workflow run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_run_id: Option<String>,
    /// Application-defined structured metadata.
    #[serde(default, skip_serializing_if = "JsonValue::is_null")]
    pub metadata: JsonValue,
}

impl ChildOperationReference {
    /// Creates a child-operation reference without Flow ownership or metadata.
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

    /// Associates the operation with an A3S Flow run.
    pub fn with_flow_run_id(mut self, flow_run_id: impl Into<String>) -> Self {
        self.flow_run_id = Some(flow_run_id.into());
        self
    }

    /// Sets application-defined structured metadata.
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
    /// The workflow returned a successful output.
    Completed {
        /// Final JSON value returned by the workflow.
        output: JsonValue,
    },
    /// The workflow terminated with an application or runtime error.
    Failed {
        /// Human-readable failure description.
        error: String,
    },
    /// The workflow completed cancellation.
    Cancelled {
        /// Optional operator- or application-supplied cancellation reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The workflow exceeded its deadline.
    TimedOut {
        /// UTC deadline that caused the timeout.
        deadline: DateTime<Utc>,
        /// Optional context for the timeout decision.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A step exhausted every permitted attempt.
    RetryExhausted {
        /// Stable identifier of the exhausted step.
        step_id: String,
        /// Final attempt number that failed.
        attempt: u32,
        /// Error returned by the final attempt.
        error: String,
    },
    /// The owning host terminated the run during shutdown.
    HostShutdown {
        /// Optional host shutdown reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// This history segment closed after creating a successor.
    ContinuedAsNew {
        /// Identifier assigned to the successor run.
        successor_run_id: String,
    },
}
