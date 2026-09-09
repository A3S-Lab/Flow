use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::FlowEventEnvelope;

/// Tip-pinned ownership receipt for a contiguous history export.
///
/// Flow keeps the append-only event log authoritative. The host owns archive
/// bytes, destination retries, and retention. This seal proves which tip and
/// content digest were exported so Cloud can rebuild or verify without treating
/// the archive as a second execution history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowHistoryArchiveSeal {
    /// Workflow run whose history prefix was exported.
    pub run_id: String,
    /// Last sequence included in the seal (the tip pinned at export start).
    pub tip_sequence: u64,
    /// Event ID at `tip_sequence` when the tip was pinned.
    pub tip_event_id: Uuid,
    /// Number of events included in the seal.
    pub event_count: u64,
    /// Number of bounded pages delivered to the host callback.
    pub page_count: u64,
    /// SHA-256 digest over ordered event IDs from sequence 1 through the tip.
    pub content_sha256: String,
}

impl FlowHistoryArchiveSeal {
    /// Validate internal identity and digest shape invariants.
    pub fn validate(&self) -> Result<()> {
        if self.run_id.is_empty() {
            return Err(FlowError::Store(
                "history archive seal run identity is empty".to_string(),
            ));
        }
        if self.tip_sequence == 0 || self.event_count == 0 || self.page_count == 0 {
            return Err(FlowError::Store(format!(
                "history archive seal for {} must include a non-empty tip-pinned export",
                self.run_id
            )));
        }
        if self.event_count != self.tip_sequence {
            return Err(FlowError::Store(format!(
                "history archive seal for {} event count {} does not match tip sequence {}",
                self.run_id, self.event_count, self.tip_sequence
            )));
        }
        validate_content_digest(&self.run_id, &self.content_sha256)?;
        Ok(())
    }
}

/// Immutable contiguous history partition within one run.
///
/// Partitions index sealed ranges over the authoritative event log. They never
/// rewrite history. Hosts may archive sealed ranges by digest while Flow retains
/// the index and the live tip for replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowHistoryPartition {
    /// Workflow run that owns the partition.
    pub run_id: String,
    /// Zero-based ordinal; sealed partitions must be contiguous from zero.
    pub ordinal: u32,
    /// Inclusive first sequence in the partition.
    pub first_sequence: u64,
    /// Inclusive last sequence in the partition.
    pub last_sequence: u64,
    /// Event ID at `first_sequence`.
    pub first_event_id: Uuid,
    /// Event ID at `last_sequence`.
    pub last_event_id: Uuid,
    /// Number of events in the closed range.
    pub event_count: u64,
    /// SHA-256 digest over ordered event IDs in the closed range.
    pub content_sha256: String,
}

impl FlowHistoryPartition {
    /// Construct and validate a sealed partition from a contiguous event page.
    pub fn from_events(
        run_id: impl Into<String>,
        ordinal: u32,
        events: &[FlowEventEnvelope],
    ) -> Result<Self> {
        let run_id = run_id.into();
        if events.is_empty() {
            return Err(FlowError::Store(format!(
                "history partition for {run_id} cannot be empty"
            )));
        }
        let first = &events[0];
        let last = events.last().expect("non-empty events");
        let mut expected = first.sequence;
        for envelope in events {
            if envelope.sequence != expected {
                return Err(FlowError::Store(format!(
                    "history partition for {run_id} is not contiguous at sequence {}; expected {expected}",
                    envelope.sequence
                )));
            }
            expected = expected.checked_add(1).ok_or_else(|| {
                FlowError::Store(format!(
                    "history partition sequence overflow for workflow run {run_id}"
                ))
            })?;
        }
        let event_count = u64::try_from(events.len()).map_err(|_| {
            FlowError::Store(format!(
                "history partition event count overflow for workflow run {run_id}"
            ))
        })?;
        let partition = Self {
            run_id,
            ordinal,
            first_sequence: first.sequence,
            last_sequence: last.sequence,
            first_event_id: first.event_id,
            last_event_id: last.event_id,
            event_count,
            content_sha256: history_content_digest(events),
        };
        partition.validate()?;
        Ok(partition)
    }

    /// Validate internal range and digest invariants.
    pub fn validate(&self) -> Result<()> {
        if self.run_id.is_empty() {
            return Err(FlowError::Store(
                "history partition run identity is empty".to_string(),
            ));
        }
        if self.first_sequence == 0
            || self.last_sequence < self.first_sequence
            || self.event_count == 0
        {
            return Err(FlowError::Store(format!(
                "history partition for {} has an invalid sequence range",
                self.run_id
            )));
        }
        let expected_count = self
            .last_sequence
            .checked_sub(self.first_sequence)
            .and_then(|delta| delta.checked_add(1))
            .ok_or_else(|| {
                FlowError::Store(format!(
                    "history partition sequence overflow for {}",
                    self.run_id
                ))
            })?;
        if self.event_count != expected_count {
            return Err(FlowError::Store(format!(
                "history partition for {} event count {} does not match range {}-{}",
                self.run_id, self.event_count, self.first_sequence, self.last_sequence
            )));
        }
        validate_content_digest(&self.run_id, &self.content_sha256)?;
        Ok(())
    }
}

pub(crate) fn history_content_digest<'a>(
    events: impl IntoIterator<Item = &'a FlowEventEnvelope>,
) -> String {
    let mut hasher = Sha256::new();
    for event in events {
        hasher.update(event.event_id.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn validate_content_digest(run_id: &str, digest: &str) -> Result<()> {
    if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(FlowError::Store(format!(
            "history content digest for {run_id} must be a 64-character hex SHA-256"
        )));
    }
    Ok(())
}
