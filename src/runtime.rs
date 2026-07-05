use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[cfg(feature = "native-ts")]
use tokio::io::AsyncWriteExt;
#[cfg(feature = "native-ts")]
use tokio::process::Command;

use crate::error::{FlowError, Result};
use crate::model::{FlowEventEnvelope, JsonValue, RuntimeCommand, WorkflowSpec};

/// Workflow replay request passed to a runtime implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInvocation {
    pub run_id: String,
    pub spec: WorkflowSpec,
    pub input: JsonValue,
    pub history: Vec<FlowEventEnvelope>,
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

/// Runtime boundary for workflow code and side-effecting steps.
#[async_trait]
pub trait FlowRuntime: Send + Sync {
    /// Replay the deterministic workflow function and return the next command.
    async fn run_workflow(&self, invocation: WorkflowInvocation) -> Result<RuntimeCommand>;

    /// Execute one side-effecting step. The engine persists success/failure.
    async fn run_step(&self, invocation: StepInvocation) -> Result<JsonValue>;
}

/// Configuration for the Perry-style native TypeScript runtime adapter.
#[derive(Debug, Clone)]
pub struct NativeTsRuntimeConfig {
    pub perry_binary: PathBuf,
    pub cache_dir: PathBuf,
    pub working_dir: PathBuf,
}

impl NativeTsRuntimeConfig {
    pub fn new(
        perry_binary: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
        working_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            perry_binary: perry_binary.into(),
            cache_dir: cache_dir.into(),
            working_dir: working_dir.into(),
        }
    }
}

impl Default for NativeTsRuntimeConfig {
    fn default() -> Self {
        Self {
            perry_binary: PathBuf::from("perry"),
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

impl NativeTsRuntime {
    pub fn new(config: NativeTsRuntimeConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &NativeTsRuntimeConfig {
        &self.config
    }

    fn binary_path(&self, spec: &WorkflowSpec) -> PathBuf {
        let mut hasher = DefaultHasher::new();
        spec.name.hash(&mut hasher);
        spec.version.hash(&mut hasher);
        spec.runtime.entrypoint.hash(&mut hasher);
        spec.runtime.export_name.hash(&mut hasher);
        let hash = hasher.finish();
        let name = format!("{}-{hash:x}", sanitize_filename(&spec.name));
        self.config.cache_dir.join(name)
    }

    #[cfg(feature = "native-ts")]
    async fn compile_if_needed(&self, spec: &WorkflowSpec) -> Result<PathBuf> {
        let binary = self.binary_path(spec);
        if binary.exists() {
            return Ok(binary);
        }

        tokio::fs::create_dir_all(&self.config.cache_dir).await?;
        let entrypoint = resolve_against(&self.config.working_dir, &spec.runtime.entrypoint);
        let output = Command::new(&self.config.perry_binary)
            .arg("compile")
            .arg(&entrypoint)
            .arg("-o")
            .arg(&binary)
            .current_dir(&self.config.working_dir)
            .output()
            .await?;

        if !output.status.success() {
            return Err(FlowError::Runtime(format!(
                "perry compile failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(binary)
    }

    #[cfg(feature = "native-ts")]
    async fn invoke<I, O>(&self, spec: &WorkflowSpec, kind: &str, payload: I) -> Result<O>
    where
        I: Serialize + Send,
        O: for<'de> Deserialize<'de>,
    {
        let binary = self.compile_if_needed(spec).await?;
        let request = serde_json::json!({
            "protocol": "a3s.flow.native_ts.v1",
            "kind": kind,
            "exportName": spec.runtime.export_name,
            "payload": payload,
        });

        let mut child = Command::new(binary)
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

        let output = child.wait_with_output().await?;
        if !output.status.success() {
            return Err(FlowError::Runtime(format!(
                "native TypeScript runtime failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        serde_json::from_slice(&output.stdout).map_err(FlowError::from)
    }
}

#[async_trait]
impl FlowRuntime for NativeTsRuntime {
    #[cfg(feature = "native-ts")]
    async fn run_workflow(&self, invocation: WorkflowInvocation) -> Result<RuntimeCommand> {
        let spec = invocation.spec.clone();
        self.invoke(&spec, "workflow", invocation).await
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
        self.invoke(&spec, "step", invocation).await
    }

    #[cfg(not(feature = "native-ts"))]
    async fn run_step(&self, _invocation: StepInvocation) -> Result<JsonValue> {
        Err(FlowError::Runtime(
            "native-ts feature is disabled for NativeTsRuntime".to_string(),
        ))
    }
}

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

fn resolve_against(root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}
