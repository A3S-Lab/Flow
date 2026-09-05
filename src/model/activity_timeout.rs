use chrono::{DateTime, Duration, Utc};

use crate::error::{FlowError, Result};

/// Validate a bounded timeout and derive its persisted attempt deadline.
pub(crate) fn activity_deadline(
    started_at: DateTime<Utc>,
    timeout_ms: Option<u64>,
) -> Result<Option<DateTime<Utc>>> {
    let Some(milliseconds) = timeout_ms else {
        return Ok(None);
    };
    if milliseconds == 0 {
        return Err(FlowError::InvalidTransition(
            "activity timeout must be positive".to_string(),
        ));
    }
    let duration = i64::try_from(milliseconds)
        .ok()
        .and_then(Duration::try_milliseconds)
        .ok_or_else(|| {
            FlowError::InvalidTransition("activity timeout is out of range".to_string())
        })?;
    started_at
        .checked_add_signed(duration)
        .map(Some)
        .ok_or_else(|| {
            FlowError::InvalidTransition(
                "activity timeout exceeds representable UTC range".to_string(),
            )
        })
}
