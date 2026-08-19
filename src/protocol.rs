use serde::{Deserialize, Serialize};

use crate::model::JsonValue;

/// Wire protocol implemented by native TypeScript workflow executables.
pub const NATIVE_RUNTIME_PROTOCOL: &str = "a3s.flow.native_ts.v1";
/// Command protocol implemented by the installable native TypeScript compiler.
pub const NATIVE_COMPILER_PROTOCOL: &str = "a3s.flow.native_ts.compiler.v1";
/// Dependency-manifest protocol emitted by the native TypeScript compiler.
pub const NATIVE_DEPENDENCY_MANIFEST_PROTOCOL: &str = "a3s.flow.native_ts.dependencies.v1";

/// Capabilities reported by the installable native TypeScript compiler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeTsCompilerCapabilities {
    /// Compiler command protocol version.
    pub protocol: String,
    /// Whether dependency-manifest extraction is supported.
    #[serde(rename = "dependencyManifest")]
    pub dependency_manifest: bool,
}

impl NativeTsCompilerCapabilities {
    /// Returns capabilities implemented by this crate's compiler binary.
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
    /// Dependency-manifest protocol version.
    pub protocol: String,
    /// Stable identity of the compiler environment that resolved the graph.
    #[serde(rename = "compilerIdentity")]
    pub compiler_identity: String,
    /// Canonical source files that contribute to the compiled artifact.
    pub files: Vec<String>,
}

impl NativeTsDependencyManifest {
    /// Creates a dependency manifest using the current protocol version.
    pub fn new(compiler_identity: impl Into<String>, files: Vec<String>) -> Self {
        Self {
            protocol: NATIVE_DEPENDENCY_MANIFEST_PROTOCOL.to_string(),
            compiler_identity: compiler_identity.into(),
            files,
        }
    }
}

/// Kind of invocation sent to a native TypeScript runtime.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeRuntimeKind {
    /// Replay a workflow function.
    Workflow,
    /// Execute a registered step function.
    Step,
}

impl NativeRuntimeKind {
    /// Returns the stable wire representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Step => "step",
        }
    }
}

/// Versioned request envelope sent to a native TypeScript executable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeRuntimeRequest<T> {
    /// Native runtime protocol version.
    pub protocol: String,
    /// Kind of function being invoked.
    pub kind: NativeRuntimeKind,
    /// Exported function name within the compiled entrypoint.
    #[serde(rename = "exportName")]
    pub export_name: String,
    /// Digest binding the request to its complete source graph.
    #[serde(rename = "sourceHash")]
    pub source_hash: String,
    /// Invocation-specific request payload.
    pub payload: T,
}

impl<T> NativeRuntimeRequest<T> {
    /// Creates a request using the current native runtime protocol.
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

/// Versioned response envelope returned by a native TypeScript executable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeRuntimeResponse {
    /// Native runtime protocol version.
    pub protocol: String,
    /// Kind copied from the matching request.
    pub kind: NativeRuntimeKind,
    /// Whether execution completed without a runtime error.
    pub ok: bool,
    /// Successful JSON output.
    #[serde(default)]
    pub output: Option<JsonValue>,
    /// Runtime or protocol error description.
    #[serde(default)]
    pub error: Option<String>,
}
