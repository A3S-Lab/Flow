use serde::{Deserialize, Serialize};

use crate::model::JsonValue;

pub const NATIVE_RUNTIME_PROTOCOL: &str = "a3s.flow.native_ts.v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeRuntimeKind {
    Workflow,
    Step,
}

impl NativeRuntimeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Step => "step",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeRuntimeRequest<T> {
    pub protocol: String,
    pub kind: NativeRuntimeKind,
    #[serde(rename = "exportName")]
    pub export_name: String,
    #[serde(rename = "sourceHash")]
    pub source_hash: String,
    pub payload: T,
}

impl<T> NativeRuntimeRequest<T> {
    pub fn new(
        kind: NativeRuntimeKind,
        export_name: impl Into<String>,
        source_hash: impl Into<String>,
        payload: T,
    ) -> Self {
        Self {
            protocol: NATIVE_RUNTIME_PROTOCOL.to_string(),
            kind,
            export_name: export_name.into(),
            source_hash: source_hash.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeRuntimeResponse {
    pub protocol: String,
    pub kind: NativeRuntimeKind,
    pub ok: bool,
    #[serde(default)]
    pub output: Option<JsonValue>,
    #[serde(default)]
    pub error: Option<String>,
}
