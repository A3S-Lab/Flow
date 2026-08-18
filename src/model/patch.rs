use serde::de::{Error as _, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Borrow;
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use crate::error::{FlowError, Result};

const MAX_WORKFLOW_PATCH_ID_BYTES: usize = 128;

/// Maximum number of replay-safe patch markers pinned to one workflow run.
pub const MAX_WORKFLOW_PATCH_MARKERS: usize = 256;

/// Stable identity of a replay-safe workflow code change.
///
/// Patch IDs are persisted in [`WorkflowSpec`](crate::WorkflowSpec) when a run
/// is created. They are lowercase, bounded identifiers so they remain safe in
/// event history, diagnostics, and native runtime payloads.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct WorkflowPatchId(String);

impl WorkflowPatchId {
    /// Validate and create a durable workflow patch identity.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_workflow_patch_id(&value)?;
        Ok(Self(value))
    }

    /// Return the validated patch identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkflowPatchId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for WorkflowPatchId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for WorkflowPatchId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for WorkflowPatchId {
    type Err = FlowError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for WorkflowPatchId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

pub(crate) fn deserialize_patch_markers<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeSet<WorkflowPatchId>, D::Error>
where
    D: Deserializer<'de>,
{
    struct PatchMarkerSetVisitor;

    impl<'de> Visitor<'de> for PatchMarkerSetVisitor {
        type Value = BTreeSet<WorkflowPatchId>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "at most {MAX_WORKFLOW_PATCH_MARKERS} unique workflow patch IDs"
            )
        }

        fn visit_seq<A>(
            self,
            mut sequence: A,
        ) -> std::result::Result<BTreeSet<WorkflowPatchId>, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if sequence
                .size_hint()
                .is_some_and(|size| size > MAX_WORKFLOW_PATCH_MARKERS)
            {
                return Err(A::Error::custom(format!(
                    "workflow patch marker count exceeds {MAX_WORKFLOW_PATCH_MARKERS}"
                )));
            }

            let mut markers = BTreeSet::new();
            while let Some(marker) = sequence.next_element::<WorkflowPatchId>()? {
                if markers.len() == MAX_WORKFLOW_PATCH_MARKERS {
                    return Err(A::Error::custom(format!(
                        "workflow patch marker count exceeds {MAX_WORKFLOW_PATCH_MARKERS}"
                    )));
                }
                if !markers.insert(marker) {
                    return Err(A::Error::custom(
                        "workflow patch markers contain a duplicate ID",
                    ));
                }
            }
            Ok(markers)
        }
    }

    deserializer.deserialize_seq(PatchMarkerSetVisitor)
}

fn validate_workflow_patch_id(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(FlowError::InvalidWorkflowPatchId(
            "workflow patch id must not be empty".to_string(),
        ));
    }
    if value.len() > MAX_WORKFLOW_PATCH_ID_BYTES {
        return Err(FlowError::InvalidWorkflowPatchId(format!(
            "workflow patch id must not exceed {MAX_WORKFLOW_PATCH_ID_BYTES} bytes"
        )));
    }
    if !value.is_ascii() {
        return Err(FlowError::InvalidWorkflowPatchId(
            "workflow patch id must contain only ASCII characters".to_string(),
        ));
    }
    if !value
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        || !value
            .as_bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(FlowError::InvalidWorkflowPatchId(
            "workflow patch id must start and end with a lowercase ASCII letter or digit"
                .to_string(),
        ));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
    }) {
        return Err(FlowError::InvalidWorkflowPatchId(
            "workflow patch id contains an unsupported character".to_string(),
        ));
    }
    Ok(())
}
