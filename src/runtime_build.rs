use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use crate::error::{FlowError, Result};

const MAX_RUNTIME_BUILD_ID_BYTES: usize = 128;

/// Immutable identity of a deployed workflow runtime build.
///
/// Runs pin this identity in their [`WorkflowSpec`](crate::WorkflowSpec). A
/// worker admits the run only when its explicit compatibility set contains the
/// same identity. The value is bounded and path-independent so it is safe to
/// persist in event history and queue-routing metadata.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct RuntimeBuildId(String);

impl RuntimeBuildId {
    /// Validate and create an opaque deployed-runtime identity.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_runtime_build_id(&value)?;
        Ok(Self(value))
    }

    /// Return the validated identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuntimeBuildId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for RuntimeBuildId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for RuntimeBuildId {
    type Err = FlowError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for RuntimeBuildId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// Runtime builds one engine instance can execute deterministically.
///
/// The current build is always admitted. Older compatible builds must be
/// registered explicitly. Unpinned histories are rejected by default once a
/// worker opts into build fencing; hosts can enable them during a bounded
/// migration with [`Self::accept_unpinned`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBuildCompatibility {
    current_build_id: RuntimeBuildId,
    compatible_build_ids: BTreeSet<RuntimeBuildId>,
    accepts_unpinned: bool,
}

impl RuntimeBuildCompatibility {
    /// Create a strict compatibility set containing the current build.
    pub fn new(current_build_id: RuntimeBuildId) -> Self {
        let mut compatible_build_ids = BTreeSet::new();
        compatible_build_ids.insert(current_build_id.clone());
        Self {
            current_build_id,
            compatible_build_ids,
            accepts_unpinned: false,
        }
    }

    /// Declare an older build that this worker can still replay exactly.
    pub fn with_compatible_build(mut self, build_id: RuntimeBuildId) -> Self {
        self.compatible_build_ids.insert(build_id);
        self
    }

    /// Temporarily admit histories created before build pinning was enabled.
    pub fn accept_unpinned(mut self) -> Self {
        self.accepts_unpinned = true;
        self
    }

    /// Return the identity advertised as the current deployed build.
    pub fn current_build_id(&self) -> &RuntimeBuildId {
        &self.current_build_id
    }

    /// Iterate over the current and explicitly compatible build identities.
    pub fn compatible_build_ids(&self) -> impl Iterator<Item = &RuntimeBuildId> {
        self.compatible_build_ids.iter()
    }

    /// Return whether this worker admits legacy unpinned histories.
    pub fn accepts_unpinned(&self) -> bool {
        self.accepts_unpinned
    }

    /// Return whether this compatibility set admits a persisted requirement.
    pub fn supports(&self, required_build_id: Option<&RuntimeBuildId>) -> bool {
        match required_build_id {
            Some(build_id) => self.compatible_build_ids.contains(build_id),
            None => self.accepts_unpinned,
        }
    }
}

fn validate_runtime_build_id(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(FlowError::InvalidRuntimeBuildId(
            "runtime build id must not be empty".to_string(),
        ));
    }
    if value.len() > MAX_RUNTIME_BUILD_ID_BYTES {
        return Err(FlowError::InvalidRuntimeBuildId(format!(
            "runtime build id must not exceed {MAX_RUNTIME_BUILD_ID_BYTES} bytes"
        )));
    }
    if !value.is_ascii() {
        return Err(FlowError::InvalidRuntimeBuildId(
            "runtime build id must contain only ASCII characters".to_string(),
        ));
    }
    if !value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        || !value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
    {
        return Err(FlowError::InvalidRuntimeBuildId(
            "runtime build id must start and end with an ASCII alphanumeric character".to_string(),
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:+/@".contains(&byte))
    {
        return Err(FlowError::InvalidRuntimeBuildId(
            "runtime build id contains an unsupported character".to_string(),
        ));
    }
    Ok(())
}
