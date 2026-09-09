use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

/// Host-owned content-addressed reference to an external item dataset.
///
/// Flow stores the reference in history so large fan-out plans do not embed
/// item payloads in the event log. Hosts resolve `dataset_id` /
/// `content_digest` from their own CAS; Flow only enforces identity stability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExternalDatasetRef {
    /// Caller-owned dataset identity stable for this run.
    pub dataset_id: String,
    /// Content digest of the referenced bytes (host-defined hex or URI form).
    pub content_digest: String,
    /// Number of items represented by the referenced dataset.
    pub item_count: u64,
}

impl ExternalDatasetRef {
    /// Create an external dataset reference.
    pub fn new(
        dataset_id: impl Into<String>,
        content_digest: impl Into<String>,
        item_count: u64,
    ) -> Self {
        Self {
            dataset_id: dataset_id.into(),
            content_digest: content_digest.into(),
            item_count,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.dataset_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "external dataset id must not be empty".to_string(),
            ));
        }
        if self.content_digest.trim().is_empty() {
            return Err(FlowError::InvalidTransition(format!(
                "external dataset {} content digest must not be empty",
                self.dataset_id
            )));
        }
        Ok(())
    }
}
