use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Bounded policy for deleting complete terminal PostgreSQL histories.
///
/// Flow never rewrites or partially compacts an event stream. A retention scan
/// removes an entire terminal history only when it is older than
/// `terminal_before`, has no durable audit hold, and belongs to a linked-run
/// component whose other histories are eligible in the same scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryRetentionPolicy {
    pub terminal_before: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ids: Option<BTreeSet<String>>,
}

impl FlowHistoryRetentionPolicy {
    pub fn new(terminal_before: DateTime<Utc>) -> Self {
        Self {
            terminal_before,
            run_ids: None,
        }
    }

    /// Restrict deletion candidates to an explicit set of run IDs.
    ///
    /// Linked histories outside this set remain protected and therefore also
    /// protect candidate histories connected to them.
    pub fn with_run_ids<I, S>(mut self, run_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.run_ids = Some(run_ids.into_iter().map(Into::into).collect());
        self
    }

    pub(crate) fn includes(&self, run_id: &str) -> bool {
        self.run_ids
            .as_ref()
            .is_none_or(|run_ids| run_ids.contains(run_id))
    }
}

/// Persistent reason that prevents a run history from being pruned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryHold {
    pub run_id: String,
    pub hold_id: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

/// Minimal audit record retained after a complete event history is deleted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryTombstone {
    pub run_id: String,
    pub deleted_at: DateTime<Utc>,
    pub terminal_sequence: u64,
    pub terminal_event_id: Uuid,
    pub terminal_event_key: String,
    pub history_sha256: String,
}

/// Detailed result of a PostgreSQL terminal-history retention scan.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryRetentionReport {
    pub deleted_run_ids: Vec<String>,
    pub held_run_ids: Vec<String>,
    pub referenced_run_ids: Vec<String>,
    pub non_terminal_run_ids: Vec<String>,
    pub recent_terminal_run_ids: Vec<String>,
}
