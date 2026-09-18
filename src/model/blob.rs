use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use crate::error::{FlowError, Result};

/// Reserved JSON object key for embedding a [`FlowBlobRef`] inside an inline
/// step/activity/run output value.
pub const FLOW_BLOB_REF_OUTPUT_KEY: &str = "$flow_blob_ref";

/// Host-owned encryption metadata for a content-addressed blob.
///
/// Flow stores these fields for audit and idempotency only. The host owns key
/// material and decryption policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowBlobEncryption {
    /// Encryption scheme identifier (host-defined, for example `aes-256-gcm`).
    pub scheme: String,
    /// Opaque key identity resolved by the host secret provider.
    pub key_id: String,
}

impl FlowBlobEncryption {
    /// Create encryption metadata.
    pub fn new(scheme: impl Into<String>, key_id: impl Into<String>) -> Self {
        Self {
            scheme: scheme.into(),
            key_id: key_id.into(),
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.scheme.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "blob encryption scheme must not be empty".to_string(),
            ));
        }
        if self.key_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "blob encryption key id must not be empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Host-owned content-addressed reference to large payload bytes.
///
/// Flow persists the reference in history so oversized inputs/outputs do not
/// need to be inlined under [`super::MAX_FLOW_EVENT_BYTES`]. Hosts resolve
/// `content_digest` from their own CAS; Flow never fetches blob bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowBlobRef {
    /// Caller-owned blob identity stable for this run.
    pub blob_id: String,
    /// Content digest of the referenced bytes (host-defined hex or URI form).
    pub content_digest: String,
    /// Optional codec identifier (for example `identity`, `gzip`, `json.gz`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    /// Optional encryption metadata; key material stays host-owned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption: Option<FlowBlobEncryption>,
    /// Optional declared byte length of the referenced payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
}

impl FlowBlobRef {
    /// Create a blob reference with required identity fields.
    pub fn new(blob_id: impl Into<String>, content_digest: impl Into<String>) -> Self {
        Self {
            blob_id: blob_id.into(),
            content_digest: content_digest.into(),
            codec: None,
            encryption: None,
            byte_length: None,
        }
    }

    /// Set an optional codec identifier.
    pub fn with_codec(mut self, codec: impl Into<String>) -> Self {
        self.codec = Some(codec.into());
        self
    }

    /// Set optional encryption metadata.
    pub fn with_encryption(mut self, encryption: FlowBlobEncryption) -> Self {
        self.encryption = Some(encryption);
        self
    }

    /// Set an optional declared byte length.
    pub fn with_byte_length(mut self, byte_length: u64) -> Self {
        self.byte_length = Some(byte_length);
        self
    }

    /// Encode this reference as a step/activity/run output JSON marker.
    pub fn to_output_value(&self) -> JsonValue {
        json!({ FLOW_BLOB_REF_OUTPUT_KEY: self })
    }

    /// Parse a blob-ref output marker, if the value uses the reserved key.
    pub fn try_from_output_value(value: &JsonValue) -> Result<Option<Self>> {
        let Some(object) = value.as_object() else {
            return Ok(None);
        };
        let Some(raw) = object.get(FLOW_BLOB_REF_OUTPUT_KEY) else {
            return Ok(None);
        };
        if object.len() != 1 {
            return Err(FlowError::InvalidTransition(
                "blob ref output marker must contain only $flow_blob_ref".to_string(),
            ));
        }
        let blob: Self = serde_json::from_value(raw.clone()).map_err(|error| {
            FlowError::InvalidTransition(format!("invalid $flow_blob_ref payload: {error}"))
        })?;
        blob.validate()?;
        Ok(Some(blob))
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.blob_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "blob id must not be empty".to_string(),
            ));
        }
        if self.content_digest.trim().is_empty() {
            return Err(FlowError::InvalidTransition(format!(
                "blob {} content digest must not be empty",
                self.blob_id
            )));
        }
        if let Some(codec) = &self.codec {
            if codec.trim().is_empty() {
                return Err(FlowError::InvalidTransition(format!(
                    "blob {} codec must not be empty when set",
                    self.blob_id
                )));
            }
        }
        if let Some(encryption) = &self.encryption {
            encryption.validate()?;
        }
        Ok(())
    }
}

/// Validate blob-ref markers embedded in durable JSON payloads.
pub(crate) fn validate_json_blob_ref_marker(value: &JsonValue) -> Result<()> {
    let _ = FlowBlobRef::try_from_output_value(value)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_ref_round_trips_through_output_marker() {
        let blob = FlowBlobRef::new("result", "sha256:abc")
            .with_codec("gzip")
            .with_byte_length(42)
            .with_encryption(FlowBlobEncryption::new("aes-256-gcm", "key-1"));
        let value = blob.to_output_value();
        let parsed = FlowBlobRef::try_from_output_value(&value).unwrap();
        assert_eq!(parsed, Some(blob));
    }

    #[test]
    fn empty_digest_fails_closed() {
        let err = FlowBlobRef::new("result", "  ").validate().unwrap_err();
        assert!(err.to_string().contains("content digest"));
    }
}
