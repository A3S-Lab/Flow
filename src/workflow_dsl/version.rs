use semver::Version;

use super::{WorkflowDslError, TESTED_WORKFLOW_DSL_VERSION};

/// Compatibility of an imported workflow DSL with the version exercised by
/// this release of A3S Flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkflowDslCompatibility {
    /// The imported DSL can be consumed directly.
    Compatible,
    /// The imported DSL is from an older minor and should surface a warning.
    CompatibleWithWarnings,
    /// A newer DSL or different major requires an explicit host decision.
    RequiresConfirmation,
}

pub(super) fn classify_dsl_version(
    imported: &str,
) -> Result<WorkflowDslCompatibility, WorkflowDslError> {
    let current = Version::parse(TESTED_WORKFLOW_DSL_VERSION).map_err(|error| {
        WorkflowDslError::InvalidDocument {
            message: format!(
                "A3S Flow tested workflow DSL version {TESTED_WORKFLOW_DSL_VERSION:?} is invalid: {error}"
            ),
        }
    })?;
    let imported = Version::parse(imported).map_err(|error| WorkflowDslError::InvalidDocument {
        message: format!("workflow DSL version {imported:?} is invalid: {error}"),
    })?;

    if imported > current || imported.major < current.major {
        return Ok(WorkflowDslCompatibility::RequiresConfirmation);
    }
    if imported.minor < current.minor {
        return Ok(WorkflowDslCompatibility::CompatibleWithWarnings);
    }
    Ok(WorkflowDslCompatibility::Compatible)
}
