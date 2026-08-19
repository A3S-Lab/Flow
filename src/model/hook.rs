use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::{FlowError, Result};

use super::JsonValue;

/// HTTP route metadata for external hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookCallbackRoute {
    /// Uppercase HTTP method expected by the callback route.
    pub method: String,
    /// Host-owned callback path.
    pub path: String,
}

impl HookCallbackRoute {
    /// Creates callback route metadata and normalizes the method to uppercase.
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            method: method.into().to_ascii_uppercase(),
            path: path.into(),
        }
    }

    /// Creates callback route metadata for an HTTP `POST` endpoint.
    pub fn post(path: impl Into<String>) -> Self {
        Self::new("POST", path)
    }
}

/// Typed helper for common hook metadata fields.
///
/// Hook metadata is still persisted as JSON in `flow.hook.created` events. This
/// type only gives Rust workflow authors a stable shape for audit and callback
/// routing fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookMetadata {
    /// Application-defined hook kind used for routing and audit.
    pub kind: String,
    /// Optional human-readable subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Optional callback route exposed by the host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback: Option<HookCallbackRoute>,
    /// Searchable string labels.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Application-defined structured metadata.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, JsonValue>,
}

impl HookMetadata {
    /// Creates metadata for an application-defined hook kind.
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            subject: None,
            callback: None,
            labels: BTreeMap::new(),
            data: BTreeMap::new(),
        }
    }

    /// Creates metadata for a human-approval hook.
    pub fn human_approval(subject: impl Into<String>) -> Self {
        Self::new("human_approval").with_subject(subject)
    }

    /// Sets the human-readable subject.
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Sets the host callback route.
    pub fn with_callback_route(mut self, callback: HookCallbackRoute) -> Self {
        self.callback = Some(callback);
        self
    }

    /// Adds or replaces one searchable label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Adds or replaces one structured metadata value.
    pub fn with_data(mut self, key: impl Into<String>, value: impl Into<JsonValue>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }

    /// Serializes the typed metadata into its durable JSON representation.
    pub fn into_json(self) -> Result<JsonValue> {
        serde_json::to_value(self).map_err(FlowError::from)
    }
}
