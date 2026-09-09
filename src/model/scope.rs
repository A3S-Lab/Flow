use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Materialized lifecycle state of a cancellation scope.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum CancellationScopeStatus {
    /// The scope is open and may own suspensions.
    Open,
    /// The scope exited cleanly.
    Completed,
    /// The scope was cancelled explicitly or by run cancellation.
    Cancelled,
}

/// Materialized state of one cancellation scope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct CancellationScopeSnapshot {
    /// Replay-stable identity of the scope.
    pub scope_id: String,
    /// Current lifecycle state.
    pub status: CancellationScopeStatus,
    /// Optional parent scope that was open when this scope opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_scope_id: Option<String>,
    /// UTC time at which the scope opened.
    pub opened_at: DateTime<Utc>,
    /// Event sequence that opened the scope.
    pub opened_sequence: u64,
    /// Optional cancellation reason when cancelled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl CancellationScopeSnapshot {
    /// Returns whether the scope is still open.
    pub fn is_open(&self) -> bool {
        self.status == CancellationScopeStatus::Open
    }

    /// Returns whether the scope was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.status == CancellationScopeStatus::Cancelled
    }
}
