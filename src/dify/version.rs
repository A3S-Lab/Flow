use semver::Version;

use super::{DifyImportError, DIFY_TESTED_DSL_VERSION};

/// Compatibility of an imported Dify app DSL with the version exercised by
/// this release of A3S Flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DifyDslCompatibility {
    /// The imported DSL can be consumed directly.
    Compatible,
    /// The imported DSL is from an older minor and should surface a warning.
    CompatibleWithWarnings,
    /// A newer DSL or different major requires an explicit host decision.
    RequiresConfirmation,
}

pub(super) fn classify_dsl_version(
    imported: &str,
) -> Result<DifyDslCompatibility, DifyImportError> {
    let current = Version::parse(DIFY_TESTED_DSL_VERSION).map_err(|error| {
        DifyImportError::InvalidDocument {
            message: format!(
                "A3S Flow tested Dify DSL version {DIFY_TESTED_DSL_VERSION:?} is invalid: {error}"
            ),
        }
    })?;
    let imported = Version::parse(imported).map_err(|error| DifyImportError::InvalidDocument {
        message: format!("Dify DSL version {imported:?} is invalid: {error}"),
    })?;

    if imported > current || imported.major < current.major {
        return Ok(DifyDslCompatibility::RequiresConfirmation);
    }
    if imported.minor < current.minor {
        return Ok(DifyDslCompatibility::CompatibleWithWarnings);
    }
    Ok(DifyDslCompatibility::Compatible)
}
