use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::WorkflowRunSnapshot;

/// A disposable materialized projection checkpoint.
///
/// The event history remains the only source of truth. A checkpoint is usable
/// only when its run, sequence, and anchor event ID still match the durable
/// history; otherwise callers must replay the history and may replace it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct FlowProjectionCheckpoint {
    /// Workflow run whose projection was materialized.
    pub run_id: String,
    /// Last event sequence included in the projection.
    pub last_sequence: u64,
    /// Event ID at `last_sequence`, used to detect stale or replaced history.
    pub last_event_id: Uuid,
    /// Materialized projection at `last_sequence`.
    pub snapshot: WorkflowRunSnapshot,
}

impl FlowProjectionCheckpoint {
    /// Construct a checkpoint from a durable history tail and its projection.
    pub fn new(
        run_id: impl Into<String>,
        last_sequence: u64,
        last_event_id: Uuid,
        snapshot: WorkflowRunSnapshot,
    ) -> Result<Self> {
        let run_id = run_id.into();
        let checkpoint = Self {
            run_id,
            last_sequence,
            last_event_id,
            snapshot,
        };
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    /// Validate internal identity and sequence invariants.
    pub fn validate(&self) -> Result<()> {
        if self.run_id.is_empty() || self.snapshot.run_id != self.run_id {
            return Err(FlowError::Store(format!(
                "projection checkpoint run identity mismatch for {}",
                self.run_id
            )));
        }
        if self.last_sequence == 0 || self.snapshot.last_sequence != self.last_sequence {
            return Err(FlowError::Store(format!(
                "projection checkpoint sequence mismatch for {}",
                self.run_id
            )));
        }
        Ok(())
    }
}
