use serde::{Deserialize, Serialize};

use crate::model::JsonValue;

pub const NATIVE_RUNTIME_PROTOCOL: &str = "a3s.flow.native_ts.v1";
pub const NATIVE_COMPILER_PROTOCOL: &str = "a3s.flow.native_ts.compiler.v1";
pub const NATIVE_DEPENDENCY_MANIFEST_PROTOCOL: &str = "a3s.flow.native_ts.dependencies.v1";

/// Capabilities reported by the installable native TypeScript compiler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeTsCompilerCapabilities {
    pub protocol: String,
    #[serde(rename = "dependencyManifest")]
    pub dependency_manifest: bool,
}

impl NativeTsCompilerCapabilities {
    pub fn current() -> Self {
        Self {
            protocol: NATIVE_COMPILER_PROTOCOL.to_string(),
            dependency_manifest: true,
        }
    }
}

/// Compiler-owned source graph used to bind native artifacts to every input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeTsDependencyManifest {
    pub protocol: String,
    #[serde(rename = "compilerIdentity")]
    pub compiler_identity: String,
    pub files: Vec<String>,
}

impl NativeTsDependencyManifest {
    pub fn new(compiler_identity: impl Into<String>, files: Vec<String>) -> Self {
        Self {
            protocol: NATIVE_DEPENDENCY_MANIFEST_PROTOCOL.to_string(),
            compiler_identity: compiler_identity.into(),
            files,
        }
    }
}

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
