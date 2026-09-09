use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

use super::JsonValue;

/// Materialized lifecycle state of a durable compensation marker.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum CompensationMarkerStatus {
    /// Forward work claimed a compensating action that has not finished.
    Open,
    /// The compensating action reached a durable completion record.
    Completed,
}

/// Durable bookkeeping for one saga-style compensating obligation.
///
/// Markers do not execute compensation. They make the obligation visible to
/// replay so workflow code can schedule ordinary steps or children without
/// losing track of what still needs cleanup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct CompensationMarker {
    /// Replay-stable idempotency identity for this obligation.
    pub marker_id: String,
    /// Application identity of the forward work being compensated.
    pub compensates: String,
    /// Optional structured details for the compensating action.
    #[serde(default, skip_serializing_if = "JsonValue::is_null")]
    pub details: JsonValue,
}

impl CompensationMarker {
    /// Create a compensation marker with optional details.
    pub fn new(
        marker_id: impl Into<String>,
        compensates: impl Into<String>,
        details: JsonValue,
    ) -> Self {
        Self {
            marker_id: marker_id.into(),
            compensates: compensates.into(),
            details,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.marker_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "compensation marker id must not be empty".to_string(),
            ));
        }
        if self.compensates.trim().is_empty() {
            return Err(FlowError::InvalidTransition(format!(
                "compensation marker {} compensates identity must not be empty",
                self.marker_id
            )));
        }
        Ok(())
    }
}

/// Materialized compensation marker projected from run history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct CompensationMarkerSnapshot {
    /// Replay-stable idempotency identity.
    pub marker_id: String,
    /// Application identity of the forward work being compensated.
    pub compensates: String,
    /// Optional structured details recorded with the obligation.
    #[serde(default, skip_serializing_if = "JsonValue::is_null")]
    pub details: JsonValue,
    /// Current lifecycle state.
    pub status: CompensationMarkerStatus,
    /// Optional outcome recorded when compensation completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<JsonValue>,
}

impl CompensationMarkerSnapshot {
    /// Returns whether compensation is still outstanding.
    pub fn is_open(&self) -> bool {
        self.status == CompensationMarkerStatus::Open
    }

    /// Returns whether compensation has a durable completion record.
    pub fn is_completed(&self) -> bool {
        self.status == CompensationMarkerStatus::Completed
    }

    pub(crate) fn matches_record(&self, marker: &CompensationMarker) -> bool {
        self.marker_id == marker.marker_id
            && self.compensates == marker.compensates
            && self.details == marker.details
    }
}
