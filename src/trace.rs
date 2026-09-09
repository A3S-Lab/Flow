use std::future::Future;

use serde::{Deserialize, Serialize};
use tokio::task_local;

use crate::error::{FlowError, Result};

task_local! {
    static CURRENT_TRACE_CONTEXT: Option<FlowTraceContext>;
}

/// W3C Trace Context carrier propagated across Flow host operations.
///
/// Trace context is ambient for a drive/attempt and is not written into the
/// replay log. Hosts bind it with [`with_trace_context`] so invocations and
/// observers can correlate work without embedding high-cardinality IDs in
/// history or metric labels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowTraceContext {
    /// W3C `traceparent` header value.
    pub traceparent: String,
    /// Optional W3C `tracestate` header value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracestate: Option<String>,
}

impl FlowTraceContext {
    /// Parse and validate a W3C Trace Context carrier.
    pub fn new(traceparent: impl Into<String>, tracestate: Option<String>) -> Result<Self> {
        let traceparent = traceparent.into();
        validate_traceparent(&traceparent)?;
        if let Some(state) = tracestate.as_ref() {
            if state.trim().is_empty() {
                return Err(FlowError::InvalidTransition(
                    "tracestate must not be empty when provided".to_string(),
                ));
            }
        }
        Ok(Self {
            traceparent,
            tracestate,
        })
    }

    /// Returns the 32-hex trace id from `traceparent`, when valid.
    pub fn trace_id(&self) -> Option<&str> {
        self.traceparent.split('-').nth(1)
    }

    /// Returns the 16-hex parent span id from `traceparent`, when valid.
    pub fn parent_id(&self) -> Option<&str> {
        self.traceparent.split('-').nth(2)
    }
}

/// Run `future` with an ambient [`FlowTraceContext`] visible to Flow invocations.
pub async fn with_trace_context<F, T>(trace: FlowTraceContext, future: F) -> T
where
    F: Future<Output = T>,
{
    CURRENT_TRACE_CONTEXT.scope(Some(trace), future).await
}

/// Run `future` without an ambient trace context.
pub async fn without_trace_context<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    CURRENT_TRACE_CONTEXT.scope(None, future).await
}

/// Return the ambient trace context for the current async task, if any.
pub fn current_trace_context() -> Option<FlowTraceContext> {
    CURRENT_TRACE_CONTEXT
        .try_with(|value| value.clone())
        .ok()
        .flatten()
}

fn validate_traceparent(traceparent: &str) -> Result<()> {
    let parts = traceparent.split('-').collect::<Vec<_>>();
    if parts.len() != 4 {
        return Err(FlowError::InvalidTransition(
            "traceparent must have four dash-separated fields".to_string(),
        ));
    }
    let [version, trace_id, parent_id, flags] = [parts[0], parts[1], parts[2], parts[3]];
    if version != "00" {
        return Err(FlowError::InvalidTransition(format!(
            "traceparent version {version} is unsupported"
        )));
    }
    if !is_hex(trace_id, 32) || trace_id.chars().all(|ch| ch == '0') {
        return Err(FlowError::InvalidTransition(
            "traceparent trace-id must be 32 non-zero hex characters".to_string(),
        ));
    }
    if !is_hex(parent_id, 16) || parent_id.chars().all(|ch| ch == '0') {
        return Err(FlowError::InvalidTransition(
            "traceparent parent-id must be 16 non-zero hex characters".to_string(),
        ));
    }
    if !is_hex(flags, 2) {
        return Err(FlowError::InvalidTransition(
            "traceparent flags must be 2 hex characters".to_string(),
        ));
    }
    Ok(())
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_w3c_traceparent() {
        let trace = FlowTraceContext::new(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            Some("rojo=00f067aa0ba902b7".into()),
        )
        .unwrap();
        assert_eq!(trace.trace_id(), Some("4bf92f3577b34da6a3ce929d0e0e4736"));
        assert_eq!(trace.parent_id(), Some("00f067aa0ba902b7"));
    }

    #[test]
    fn rejects_all_zero_trace_id() {
        assert!(FlowTraceContext::new(
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            None,
        )
        .is_err());
    }
}
