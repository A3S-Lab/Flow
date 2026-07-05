use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
#[cfg(feature = "native-ts")]
use sha2::{Digest, Sha256};
#[cfg(feature = "native-ts")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(feature = "native-ts")]
use tokio::io::AsyncWriteExt;
#[cfg(feature = "native-ts")]
use tokio::process::Command;

use crate::context::WorkflowContext;
use crate::error::{FlowError, Result};
#[cfg(feature = "native-ts")]
use crate::model::RuntimeKind;
use crate::model::{FlowEventEnvelope, JsonValue, RuntimeCommand, WorkflowSpec};
#[cfg(feature = "native-ts")]
use crate::protocol::{
    NativeRuntimeKind, NativeRuntimeRequest, NativeRuntimeResponse, NATIVE_RUNTIME_PROTOCOL,
};

/// Workflow replay request passed to a runtime implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInvocation {
    pub run_id: String,
    pub spec: WorkflowSpec,
    pub input: JsonValue,
    pub history: Vec<FlowEventEnvelope>,
}

impl WorkflowInvocation {
    /// Build a deterministic helper view over this workflow invocation.
    pub fn context(&self) -> WorkflowContext<'_> {
        WorkflowContext::new(self)
    }

    /// Decode the workflow input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.input.clone()).map_err(FlowError::from)
    }
}

/// Step execution request passed to a runtime implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepInvocation {
    pub run_id: String,
    pub step_id: String,
    pub step_name: String,
    pub input: JsonValue,
    pub history: Vec<FlowEventEnvelope>,
}

impl StepInvocation {
    /// Decode the step input into a host-defined serde type.
    pub fn input_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.input.clone()).map_err(FlowError::from)
    }
}

/// Runtime boundary for workflow code and side-effecting steps.
#[async_trait]
pub trait FlowRuntime: Send + Sync {
    /// Replay the deterministic workflow function and return the next command.
    async fn run_workflow(&self, invocation: WorkflowInvocation) -> Result<RuntimeCommand>;

    /// Execute one side-effecting step. The engine persists success/failure.
    async fn run_step(&self, invocation: StepInvocation) -> Result<JsonValue>;
}

/// Configuration for the native TypeScript runtime adapter.
#[derive(Debug, Clone)]
pub struct NativeTsRuntimeConfig {
    pub compiler_binary: PathBuf,
    pub cache_dir: PathBuf,
    pub working_dir: PathBuf,
}

impl NativeTsRuntimeConfig {
    pub fn new(
        compiler_binary: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
        working_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            compiler_binary: compiler_binary.into(),
            cache_dir: cache_dir.into(),
            working_dir: working_dir.into(),
        }
    }
}

impl Default for NativeTsRuntimeConfig {
    fn default() -> Self {
        Self {
            compiler_binary: PathBuf::from("a3s-flow-native-compiler"),
            cache_dir: PathBuf::from(".a3s-flow/native-ts"),
            working_dir: PathBuf::from("."),
        }
    }
}

/// Runtime that compiles TypeScript to a native executable and speaks JSON over
/// stdin/stdout with that executable.
#[derive(Debug, Clone)]
pub struct NativeTsRuntime {
    config: NativeTsRuntimeConfig,
}

#[cfg(feature = "native-ts")]
#[derive(Debug, Clone)]
struct NativeArtifact {
    entrypoint: PathBuf,
    binary: PathBuf,
    source_hash: String,
}

/// Result of validating and compiling a native TypeScript workflow source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeTsRuntimePreflight {
    /// Resolved workflow source entrypoint used by the compiler.
    pub entrypoint: PathBuf,
    /// Resolved native artifact path that will be invoked by the runtime.
    pub artifact: PathBuf,
    /// Stable hash of the workflow source and runtime identity fields.
    pub source_hash: String,
    /// True when the existing artifact cache entry was reused.
    pub cache_hit: bool,
}

impl NativeTsRuntime {
    pub fn new(config: NativeTsRuntimeConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &NativeTsRuntimeConfig {
        &self.config
    }

    #[cfg(feature = "native-ts")]
    pub async fn preflight(&self, spec: &WorkflowSpec) -> Result<NativeTsRuntimePreflight> {
        self.compile_if_needed(spec).await
    }

    #[cfg(not(feature = "native-ts"))]
    pub async fn preflight(&self, _spec: &WorkflowSpec) -> Result<NativeTsRuntimePreflight> {
        Err(FlowError::Runtime(
            "native-ts feature is disabled for NativeTsRuntime".to_string(),
        ))
    }

    #[cfg(feature = "native-ts")]
    async fn artifact_for(&self, spec: &WorkflowSpec) -> Result<NativeArtifact> {
        validate_native_ts_spec(spec)?;
        let entrypoint = resolve_against(&self.config.working_dir, &spec.runtime.entrypoint);
        let source = tokio::fs::read(&entrypoint).await?;
        let source_hash = stable_hash([
            b"source".as_slice(),
            spec.name.as_bytes(),
            spec.version.as_bytes(),
            spec.runtime.entrypoint.as_bytes(),
            spec.runtime.export_name.as_bytes(),
            &source,
        ]);
        let name = format!("{}-{source_hash}", sanitize_filename(&spec.name));
        Ok(NativeArtifact {
            entrypoint,
            binary: self.config.cache_dir.join(name),
            source_hash,
        })
    }

    #[cfg(feature = "native-ts")]
    async fn compile_if_needed(&self, spec: &WorkflowSpec) -> Result<NativeTsRuntimePreflight> {
        let artifact = self.artifact_for(spec).await?;
        if tokio::fs::metadata(&artifact.binary).await.is_ok() {
            return Ok(NativeTsRuntimePreflight {
                entrypoint: artifact.entrypoint,
                artifact: artifact.binary,
                source_hash: artifact.source_hash,
                cache_hit: true,
            });
        }

        tokio::fs::create_dir_all(&self.config.cache_dir).await?;
        let output = Command::new(&self.config.compiler_binary)
            .arg("compile")
            .arg(&artifact.entrypoint)
            .arg("-o")
            .arg(&artifact.binary)
            .current_dir(&self.config.working_dir)
            .output()
            .await?;

        if !output.status.success() {
            return Err(FlowError::Runtime(format!(
                "native TypeScript compile failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        tokio::fs::metadata(&artifact.binary).await.map_err(|err| {
            FlowError::Runtime(format!(
                "native TypeScript compiler did not produce artifact {}: {err}",
                artifact.binary.display()
            ))
        })?;

        Ok(NativeTsRuntimePreflight {
            entrypoint: artifact.entrypoint,
            artifact: artifact.binary,
            source_hash: artifact.source_hash,
            cache_hit: false,
        })
    }

    #[cfg(feature = "native-ts")]
    async fn invoke<I, O>(
        &self,
        spec: &WorkflowSpec,
        kind: NativeRuntimeKind,
        payload: I,
    ) -> Result<O>
    where
        I: Serialize + Send,
        O: DeserializeOwned,
    {
        let preflight = self.compile_if_needed(spec).await?;
        let request = NativeRuntimeRequest::new(
            kind,
            spec.runtime.export_name.clone(),
            preflight.source_hash,
            payload,
        );

        let mut child = Command::new(&preflight.artifact)
            .arg("--a3s-flow-runtime")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(&self.config.working_dir)
            .spawn()?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| FlowError::Runtime("failed to open runtime stdin".to_string()))?;
        stdin
            .write_all(serde_json::to_string(&request)?.as_bytes())
            .await?;
        stdin.shutdown().await?;
        drop(stdin);

        let output = child.wait_with_output().await?;
        if !output.status.success() {
            return Err(FlowError::Runtime(format!(
                "native TypeScript runtime failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        decode_native_response(kind, &output.stdout)
    }
}

#[cfg(feature = "native-ts")]
fn validate_native_ts_spec(spec: &WorkflowSpec) -> Result<()> {
    spec.validate()?;
    if spec.runtime.kind != RuntimeKind::NativeTs {
        return Err(FlowError::InvalidWorkflow(format!(
            "NativeTsRuntime requires a native_ts workflow spec, got {:?}",
            spec.runtime.kind
        )));
    }
    Ok(())
}

#[async_trait]
impl FlowRuntime for NativeTsRuntime {
    #[cfg(feature = "native-ts")]
    async fn run_workflow(&self, invocation: WorkflowInvocation) -> Result<RuntimeCommand> {
        let spec = invocation.spec.clone();
        self.invoke(&spec, NativeRuntimeKind::Workflow, invocation)
            .await
    }

    #[cfg(not(feature = "native-ts"))]
    async fn run_workflow(&self, _invocation: WorkflowInvocation) -> Result<RuntimeCommand> {
        Err(FlowError::Runtime(
            "native-ts feature is disabled for NativeTsRuntime".to_string(),
        ))
    }

    #[cfg(feature = "native-ts")]
    async fn run_step(&self, invocation: StepInvocation) -> Result<JsonValue> {
        let spec = workflow_spec_from_history(&invocation.history)?;
        self.invoke(&spec, NativeRuntimeKind::Step, invocation)
            .await
    }

    #[cfg(not(feature = "native-ts"))]
    async fn run_step(&self, _invocation: StepInvocation) -> Result<JsonValue> {
        Err(FlowError::Runtime(
            "native-ts feature is disabled for NativeTsRuntime".to_string(),
        ))
    }
}

#[cfg(feature = "native-ts")]
fn workflow_spec_from_history(history: &[FlowEventEnvelope]) -> Result<WorkflowSpec> {
    let first = history
        .first()
        .ok_or_else(|| FlowError::Runtime("step invocation has empty history".to_string()))?;
    match &first.event {
        crate::model::FlowEvent::RunCreated { spec, .. } => Ok(spec.clone()),
        _ => Err(FlowError::Runtime(
            "first history event is not run_created".to_string(),
        )),
    }
}

#[cfg(feature = "native-ts")]
fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(feature = "native-ts")]
fn resolve_against(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

#[cfg(feature = "native-ts")]
fn stable_hash(parts: impl IntoIterator<Item = impl AsRef<[u8]>>) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        let bytes = part.as_ref();
        hasher.update(bytes.len().to_le_bytes());
        hasher.update(bytes);
    }
    hex_lower(&hasher.finalize())
}

#[cfg(feature = "native-ts")]
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(feature = "native-ts")]
fn decode_native_response<O>(kind: NativeRuntimeKind, bytes: &[u8]) -> Result<O>
where
    O: DeserializeOwned,
{
    let response: NativeRuntimeResponse = serde_json::from_slice(bytes)?;
    if response.protocol != NATIVE_RUNTIME_PROTOCOL {
        return Err(FlowError::Runtime(format!(
            "native TypeScript runtime protocol mismatch: expected {NATIVE_RUNTIME_PROTOCOL}, got {}",
            response.protocol
        )));
    }
    if response.kind != kind {
        return Err(FlowError::Runtime(format!(
            "native TypeScript runtime response kind mismatch: expected {}, got {}",
            kind.as_str(),
            response.kind.as_str()
        )));
    }
    if !response.ok {
        let error = response
            .error
            .unwrap_or_else(|| "runtime returned ok=false without an error".to_string());
        return Err(FlowError::Runtime(error));
    }
    let output = response.output.ok_or_else(|| {
        FlowError::Runtime("native TypeScript runtime returned ok=true without output".to_string())
    })?;
    serde_json::from_value(output).map_err(FlowError::from)
}
