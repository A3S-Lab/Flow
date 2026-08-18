use crate::error::{FlowError, Result};

/// Validate a run identity at every event-stream boundary.
pub(crate) fn validate_run_id(run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || !run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(FlowError::InvalidRunId(run_id.to_string()));
    }
    Ok(())
}
