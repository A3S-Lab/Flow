use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
#[cfg(feature = "native-ts")]
use sha2::{Digest, Sha256};
#[cfg(feature = "native-ts")]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use crate::context::WorkflowContext;
use crate::error::{FlowError, Result};
#[cfg(feature = "native-ts")]
use crate::model::RuntimeKind;
use crate::model::{FlowEventEnvelope, JsonValue, RuntimeCommand, WorkflowSpec};
#[cfg(feature = "native-ts")]
use crate::protocol::{
    NativeRuntimeKind, NativeRuntimeRequest, NativeRuntimeResponse, NATIVE_RUNTIME_PROTOCOL,
};
#[cfg(feature = "native-ts")]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
#[cfg(feature = "native-ts")]
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

#[cfg(feature = "native-ts")]
mod native_ts;

#[cfg(feature = "native-ts")]
use native_ts::{
    artifact_binary_path, verified_dependency_graph, ArtifactCache, ArtifactCacheState,
    CompilerIdentityCache, SourceSnapshot, TemporaryCacheEntryGuard, VerifiedDependencyGraph,
    MAX_DEPENDENCY_MANIFEST_BYTES,
};

/// Workflow replay request passed to a runtime implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WorkflowInvocation {
    /// Durable identifier of the workflow run being replayed.
    pub run_id: String,
    /// Workflow definition recorded when the run was created.
    pub spec: WorkflowSpec,
    /// Original workflow input.
    pub input: JsonValue,
    /// Complete persisted event history in sequence order.
    pub history: Vec<FlowEventEnvelope>,
}

impl WorkflowInvocation {
    /// Create a workflow invocation from its complete durable replay input.
    pub fn new(
        run_id: impl Into<String>,
        spec: WorkflowSpec,
        input: JsonValue,
        history: Vec<FlowEventEnvelope>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            spec,
            input,
            history,
        }
    }

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
#[non_exhaustive]
pub struct StepInvocation {
    /// Durable identifier of the workflow run that scheduled the step.
    pub run_id: String,
    /// Replay-stable identifier of this step invocation.
    pub step_id: String,
    /// Host-defined step handler name.
    pub step_name: String,
    /// Input supplied by the workflow command.
    pub input: JsonValue,
    /// Complete persisted workflow history in sequence order.
    pub history: Vec<FlowEventEnvelope>,
}

impl StepInvocation {
    /// Create a step invocation from its complete durable execution input.
    pub fn new(
        run_id: impl Into<String>,
        step_id: impl Into<String>,
        step_name: impl Into<String>,
        input: JsonValue,
        history: Vec<FlowEventEnvelope>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            step_id: step_id.into(),
            step_name: step_name.into(),
            input,
            history,
        }
    }

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

/// Source-identity policy for native TypeScript artifacts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeTsDependencyMode {
    /// Preserve the original entrypoint-plus-deployment-version identity.
    #[default]
    EntrypointOnly,
    /// Ask the compiler for its complete source graph and verify every file.
    CompilerManifest,
}

/// Configuration for the native TypeScript runtime adapter.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NativeTsRuntimeConfig {
    /// Compiler executable. Bare names use `PATH`; relative paths with a
    /// directory component are resolved against the host process directory.
    pub compiler_binary: PathBuf,
    /// Artifact cache directory. Relative paths are resolved against the host
    /// process directory before the compiler changes its working directory.
    pub cache_dir: PathBuf,
    /// Runtime working directory. Relative paths are resolved against the host
    /// process directory, and workflow entrypoints are resolved from it.
    pub working_dir: PathBuf,
}

impl NativeTsRuntimeConfig {
    /// Create a native TypeScript configuration from explicit host paths.
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
            cache_dir: PathBuf::from(".a3s/flow/native-ts"),
            working_dir: PathBuf::from("."),
        }
    }
}

/// Runtime that compiles TypeScript to a native executable and speaks JSON over
/// stdin/stdout with that executable.
#[derive(Debug, Clone)]
pub struct NativeTsRuntime {
    config: NativeTsRuntimeConfig,
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
    compile_timeout: Option<Duration>,
    invocation_timeout: Option<Duration>,
    dependency_mode: NativeTsDependencyMode,
    #[cfg(feature = "native-ts")]
    compiler_identity_cache: CompilerIdentityCache,
    #[cfg(feature = "native-ts")]
    artifact_cache: ArtifactCache,
}

#[cfg(feature = "native-ts")]
#[derive(Debug, Clone)]
struct NativeArtifact {
    compiler_binary: PathBuf,
    working_dir: PathBuf,
    entrypoint: PathBuf,
    cache_entry: PathBuf,
    cache_key: String,
    binary: PathBuf,
    source_hash: String,
    source_snapshot: SourceSnapshot,
    dependency_compiler_identity: Option<String>,
}

#[cfg(feature = "native-ts")]
struct NativeProcessOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(feature = "native-ts")]
#[derive(Debug)]
enum NativeProcessOutputError {
    Io(std::io::Error),
    LimitExceeded { stream: &'static str, limit: usize },
}

/// Result of validating and compiling a native TypeScript workflow source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
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
    /// Default maximum bytes retained from a compiler or runtime stdout pipe.
    pub const DEFAULT_MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;

    /// Default maximum bytes retained from a compiler or runtime stderr pipe.
    pub const DEFAULT_MAX_STDERR_BYTES: usize = 256 * 1024;

    /// Create a native TypeScript runtime with default limits and cache policy.
    pub fn new(config: NativeTsRuntimeConfig) -> Self {
        Self {
            config,
            max_stdout_bytes: Self::DEFAULT_MAX_STDOUT_BYTES,
            max_stderr_bytes: Self::DEFAULT_MAX_STDERR_BYTES,
            compile_timeout: None,
            invocation_timeout: None,
            dependency_mode: NativeTsDependencyMode::default(),
            #[cfg(feature = "native-ts")]
            compiler_identity_cache: CompilerIdentityCache::default(),
            #[cfg(feature = "native-ts")]
            artifact_cache: ArtifactCache::default(),
        }
    }

    /// Return the compiler, cache, and working-directory configuration.
    pub fn config(&self) -> &NativeTsRuntimeConfig {
        &self.config
    }

    /// Override the independent byte limits for each compiler and runtime
    /// stdout/stderr pipe.
    ///
    /// Exceeding either limit terminates the direct child process and returns a
    /// runtime error. A zero limit allows an empty pipe but rejects its first
    /// byte of output.
    pub fn with_output_limits(mut self, max_stdout_bytes: usize, max_stderr_bytes: usize) -> Self {
        self.max_stdout_bytes = max_stdout_bytes;
        self.max_stderr_bytes = max_stderr_bytes;
        self
    }

    /// Return the configured byte limit for each stdout pipe.
    pub fn max_stdout_bytes(&self) -> usize {
        self.max_stdout_bytes
    }

    /// Return the configured byte limit for each stderr pipe.
    pub fn max_stderr_bytes(&self) -> usize {
        self.max_stderr_bytes
    }

    /// Set the maximum duration of each cold compiler process.
    ///
    /// Cache hits do not start a compiler and therefore do not consume this
    /// timeout. By default, compilation has no runtime-owned timeout and
    /// remains bounded only by caller cancellation or an outer host timeout.
    pub fn with_compile_timeout(mut self, timeout: Duration) -> Self {
        self.compile_timeout = Some(timeout);
        self
    }

    /// Set the maximum duration of each workflow or step artifact invocation.
    ///
    /// The timeout covers writing the complete request to stdin, reading both
    /// output pipes, and waiting for process exit. By default, invocation has
    /// no runtime-owned timeout and remains bounded only by caller cancellation
    /// or an outer host timeout.
    pub fn with_invocation_timeout(mut self, timeout: Duration) -> Self {
        self.invocation_timeout = Some(timeout);
        self
    }

    /// Return the configured cold-compilation timeout, if any.
    pub fn compile_timeout(&self) -> Option<Duration> {
        self.compile_timeout
    }

    /// Return the configured workflow and step invocation timeout, if any.
    pub fn invocation_timeout(&self) -> Option<Duration> {
        self.invocation_timeout
    }

    /// Bind source and cache identity to the compiler-owned dependency graph.
    ///
    /// This mode requires the compiler's `dependencies` command and fails
    /// closed on a missing, malformed, unsafe, or changing manifest. The
    /// default remains entrypoint-only for compatibility with older compilers.
    pub fn with_dependency_mode(mut self, mode: NativeTsDependencyMode) -> Self {
        self.dependency_mode = mode;
        self
    }

    /// Return the configured native TypeScript source-identity policy.
    pub fn dependency_mode(&self) -> NativeTsDependencyMode {
        self.dependency_mode
    }

    #[cfg(feature = "native-ts")]
    /// Validate the workflow specification and ensure its native artifact is ready.
    pub async fn preflight(&self, spec: &WorkflowSpec) -> Result<NativeTsRuntimePreflight> {
        let (artifact, cache_hit) = self.compile_if_needed(spec).await?;
        Ok(NativeTsRuntimePreflight {
            entrypoint: artifact.entrypoint,
            artifact: artifact.binary,
            source_hash: artifact.source_hash,
            cache_hit,
        })
    }

    #[cfg(not(feature = "native-ts"))]
    /// Return an error because native TypeScript support is not compiled in.
    pub async fn preflight(&self, _spec: &WorkflowSpec) -> Result<NativeTsRuntimePreflight> {
        Err(FlowError::Runtime(
            "native-ts feature is disabled for NativeTsRuntime".to_string(),
        ))
    }

    #[cfg(feature = "native-ts")]
    async fn artifact_for(&self, spec: &WorkflowSpec) -> Result<NativeArtifact> {
        validate_native_ts_spec(spec)?;
        let (compiler_binary, compiler_fingerprint) = self
            .compiler_identity_cache
            .resolve_and_fingerprint(&self.config.compiler_binary)
            .await?;
        let working_dir = absolute_from_current_dir(&self.config.working_dir)?;
        let entrypoint = resolve_against(&working_dir, &spec.runtime.entrypoint);
        let cache_dir = absolute_from_current_dir(&self.config.cache_dir)?;
        let (source_hash, source_snapshot, dependency_compiler_identity) = match self
            .dependency_mode
        {
            NativeTsDependencyMode::EntrypointOnly => {
                let (source_hash, source_snapshot) =
                    SourceSnapshot::read(&entrypoint, spec).await?;
                (source_hash, source_snapshot, None)
            }
            NativeTsDependencyMode::CompilerManifest => {
                let graph = Box::pin(self.compiler_dependency_graph(
                    &compiler_binary,
                    &working_dir,
                    &entrypoint,
                ))
                .await?;
                let (source_hash, source_snapshot) =
                    Box::pin(SourceSnapshot::read_dependency_graph(graph.sources, spec)).await?;
                (source_hash, source_snapshot, Some(graph.compiler_identity))
            }
        };
        // Keep the protocol-visible source hash portable, but scope the local
        // native executable cache to every compile-environment input that can
        // make identical workflow source produce an incompatible artifact.
        let artifact_hash = native_artifact_cache_key(
            &source_hash,
            &compiler_binary,
            &compiler_fingerprint,
            &working_dir,
            &entrypoint,
            NATIVE_RUNTIME_PROTOCOL,
            dependency_compiler_identity.as_deref(),
        );
        let name = format!("{}-{artifact_hash}", sanitize_filename(&spec.name));
        let cache_entry = cache_dir.join(name);
        Ok(NativeArtifact {
            compiler_binary,
            working_dir,
            entrypoint,
            binary: artifact_binary_path(&cache_entry),
            cache_entry,
            cache_key: artifact_hash,
            source_hash,
            source_snapshot,
            dependency_compiler_identity,
        })
    }

    #[cfg(feature = "native-ts")]
    async fn compiler_dependency_graph(
        &self,
        compiler_binary: &Path,
        working_dir: &Path,
        entrypoint: &Path,
    ) -> Result<VerifiedDependencyGraph> {
        let child = Command::new(compiler_binary)
            .arg("dependencies")
            .arg(entrypoint)
            .current_dir(working_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let output = communicate_with_bounded_output(
            child,
            "compiler dependency scan",
            None,
            MAX_DEPENDENCY_MANIFEST_BYTES,
            self.max_stderr_bytes,
            self.compile_timeout,
        )
        .await?;
        if !output.status.success() {
            return Err(FlowError::Runtime(format!(
                "native TypeScript dependency scan failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        verified_dependency_graph(&output.stdout, working_dir, entrypoint).await
    }

    #[cfg(feature = "native-ts")]
    async fn compile_if_needed(&self, spec: &WorkflowSpec) -> Result<(NativeArtifact, bool)> {
        let artifact = self.artifact_for(spec).await?;
        match self
            .artifact_cache
            .inspect(&artifact.cache_entry, &artifact.cache_key)
            .await?
        {
            ArtifactCacheState::Valid => return Ok((artifact, true)),
            ArtifactCacheState::Missing => {}
            ArtifactCacheState::Invalid(reason) => {
                tracing::warn!(
                    path = %artifact.cache_entry.display(),
                    %reason,
                    "repairing invalid native TypeScript cache entry"
                );
            }
        }

        let cache_dir = artifact.cache_entry.parent().ok_or_else(|| {
            FlowError::Runtime(format!(
                "native TypeScript cache entry {} has no parent directory",
                artifact.cache_entry.display()
            ))
        })?;
        tokio::fs::create_dir_all(cache_dir).await?;
        // Bundle the executable and its integrity manifest in a unique cache
        // entry. Renaming the directory is the atomic publish boundary, so a
        // reader can observe either the complete entry or no entry at all.
        let mut temporary_entry = TemporaryCacheEntryGuard::create(&artifact.cache_entry).await?;
        let child = match Command::new(&artifact.compiler_binary)
            .arg("compile")
            .arg(&artifact.entrypoint)
            .arg("-o")
            .arg(temporary_entry.binary())
            .current_dir(&artifact.working_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // A cancelled preflight must not leave the compiler running after
            // its Rust future and temporary-artifact cleanup have disappeared.
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                temporary_entry.remove().await;
                return Err(error.into());
            }
        };
        let output = match communicate_with_bounded_output(
            child,
            "compiler",
            None,
            self.max_stdout_bytes,
            self.max_stderr_bytes,
            self.compile_timeout,
        )
        .await
        {
            Ok(output) => output,
            Err(error) => {
                temporary_entry.remove().await;
                return Err(error);
            }
        };

        if !output.status.success() {
            temporary_entry.remove().await;
            return Err(FlowError::Runtime(format!(
                "native TypeScript compile failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let source_still_matches = match &artifact.dependency_compiler_identity {
            None => artifact.source_snapshot.still_matches(spec).await,
            Some(expected_identity) => {
                match Box::pin(self.compiler_dependency_graph(
                    &artifact.compiler_binary,
                    &artifact.working_dir,
                    &artifact.entrypoint,
                ))
                .await
                {
                    Ok(graph) if graph.compiler_identity == *expected_identity => {
                        Box::pin(
                            artifact
                                .source_snapshot
                                .still_matches_dependency_graph(graph.sources, spec),
                        )
                        .await
                    }
                    Ok(_) => Ok(false),
                    Err(error) => Err(error),
                }
            }
        };
        match source_still_matches {
            Ok(true) => {}
            Ok(false) => {
                temporary_entry.remove().await;
                let subject = if artifact.dependency_compiler_identity.is_some() {
                    "dependency graph or compiler identity".to_string()
                } else {
                    format!("source {}", artifact.entrypoint.display())
                };
                return Err(FlowError::Runtime(format!(
                    "native TypeScript {subject} changed while it was being compiled; refusing to publish the artifact"
                )));
            }
            Err(error) => {
                temporary_entry.remove().await;
                return Err(error);
            }
        }

        if let Err(error) = self
            .artifact_cache
            .prepare(temporary_entry.path(), &artifact.cache_key)
            .await
        {
            temporary_entry.remove().await;
            return Err(error);
        }
        if let Err(error) = self
            .artifact_cache
            .publish(
                temporary_entry.path(),
                &artifact.cache_entry,
                &artifact.cache_key,
            )
            .await
        {
            temporary_entry.remove().await;
            return Err(error);
        }
        temporary_entry.disarm();

        match self
            .artifact_cache
            .inspect(&artifact.cache_entry, &artifact.cache_key)
            .await?
        {
            ArtifactCacheState::Valid => {}
            ArtifactCacheState::Missing => {
                return Err(FlowError::Runtime(format!(
                    "native TypeScript cache entry {} disappeared after publication",
                    artifact.cache_entry.display()
                )));
            }
            ArtifactCacheState::Invalid(reason) => {
                return Err(FlowError::Runtime(format!(
                    "native TypeScript cache entry {} is invalid after publication: {reason}",
                    artifact.cache_entry.display()
                )));
            }
        }

        Ok((artifact, false))
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
        let (artifact, _) = self.compile_if_needed(spec).await?;
        let request = serde_json::to_vec(&NativeRuntimeRequest::new(
            kind,
            spec.runtime.export_name.clone(),
            artifact.source_hash,
            payload,
        ))?;

        let child = Command::new(&artifact.binary)
            .arg("--a3s-flow-runtime")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(&artifact.working_dir)
            // Boot timeouts, lease loss, shutdown, and caller cancellation all
            // drop this future. Tie the direct artifact process to that owner.
            .kill_on_drop(true)
            .spawn()?;

        let output = communicate_with_bounded_output(
            child,
            "runtime",
            Some(request),
            self.max_stdout_bytes,
            self.max_stderr_bytes,
            self.invocation_timeout,
        )
        .await?;
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
async fn communicate_with_bounded_output(
    mut child: Child,
    process_kind: &'static str,
    stdin_bytes: Option<Vec<u8>>,
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
    process_timeout: Option<Duration>,
) -> Result<NativeProcessOutput> {
    let stdin = match stdin_bytes {
        Some(bytes) => Some((
            child.stdin.take().ok_or_else(|| {
                FlowError::Runtime(format!(
                    "native TypeScript {process_kind} stdin pipe is unavailable"
                ))
            })?,
            bytes,
        )),
        None => None,
    };
    let stdout = child.stdout.take().ok_or_else(|| {
        FlowError::Runtime(format!(
            "native TypeScript {process_kind} stdout pipe is unavailable"
        ))
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        FlowError::Runtime(format!(
            "native TypeScript {process_kind} stderr pipe is unavailable"
        ))
    })?;

    let communication = collect_native_process_output(
        &mut child,
        stdin,
        stdout,
        stderr,
        max_stdout_bytes,
        max_stderr_bytes,
    );
    let output = match process_timeout {
        Some(timeout) => match tokio::time::timeout(timeout, communication).await {
            Ok(output) => output,
            Err(_) => {
                terminate_and_reap(&mut child).await;
                return Err(FlowError::Runtime(format!(
                    "native TypeScript {process_kind} timed out after {timeout:?}"
                )));
            }
        },
        None => communication.await,
    };

    match output {
        Ok(output) => Ok(output),
        Err(error) => {
            // The read that crossed the limit stops consuming its pipe. Kill
            // and reap the child so a blocked writer cannot outlive this call.
            terminate_and_reap(&mut child).await;
            match error {
                NativeProcessOutputError::Io(error) => Err(error.into()),
                NativeProcessOutputError::LimitExceeded { stream, limit } => {
                    Err(FlowError::Runtime(format!(
                        "native TypeScript {process_kind} {stream} exceeded the {limit}-byte limit"
                    )))
                }
            }
        }
    }
}

#[cfg(feature = "native-ts")]
async fn collect_native_process_output(
    child: &mut Child,
    stdin: Option<(ChildStdin, Vec<u8>)>,
    stdout: ChildStdout,
    stderr: ChildStderr,
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
) -> std::result::Result<NativeProcessOutput, NativeProcessOutputError> {
    let write_stdin = async move {
        if let Some((mut stdin, bytes)) = stdin {
            stdin
                .write_all(&bytes)
                .await
                .map_err(NativeProcessOutputError::Io)?;
            stdin
                .shutdown()
                .await
                .map_err(NativeProcessOutputError::Io)?;
        }
        Ok(())
    };
    let wait = async { child.wait().await.map_err(NativeProcessOutputError::Io) };
    let stdout = read_bounded_output(stdout, "stdout", max_stdout_bytes);
    let stderr = read_bounded_output(stderr, "stderr", max_stderr_bytes);
    let (status, (), stdout, stderr) = tokio::try_join!(wait, write_stdin, stdout, stderr)?;
    Ok(NativeProcessOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(feature = "native-ts")]
async fn terminate_and_reap(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

#[cfg(feature = "native-ts")]
async fn read_bounded_output<R>(
    mut reader: R,
    stream: &'static str,
    limit: usize,
) -> std::result::Result<Vec<u8>, NativeProcessOutputError>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(NativeProcessOutputError::Io)?;
        if count == 0 {
            return Ok(output);
        }
        if count > limit.saturating_sub(output.len()) {
            return Err(NativeProcessOutputError::LimitExceeded { stream, limit });
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

#[cfg(feature = "native-ts")]
fn native_artifact_cache_key(
    source_hash: &str,
    compiler_binary: &Path,
    compiler_fingerprint: &str,
    working_dir: &Path,
    entrypoint: &Path,
    protocol: &str,
    dependency_compiler_identity: Option<&str>,
) -> String {
    let legacy_identity = stable_hash([
        b"a3s.flow.native_ts.artifact.v3".as_slice(),
        source_hash.as_bytes(),
        protocol.as_bytes(),
        compiler_binary.as_os_str().as_encoded_bytes(),
        compiler_fingerprint.as_bytes(),
        working_dir.as_os_str().as_encoded_bytes(),
        entrypoint.as_os_str().as_encoded_bytes(),
        std::env::consts::OS.as_bytes(),
        std::env::consts::ARCH.as_bytes(),
    ]);
    match dependency_compiler_identity {
        None => legacy_identity,
        Some(identity) => stable_hash([
            b"a3s.flow.native_ts.artifact.compiler_identity.v1".as_slice(),
            legacy_identity.as_bytes(),
            identity.as_bytes(),
        ]),
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
fn absolute_from_current_dir(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(path))
}

#[cfg(feature = "native-ts")]
fn stable_hash(parts: impl IntoIterator<Item = impl AsRef<[u8]>>) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        update_stable_hash_part(&mut hasher, part.as_ref());
    }
    hex_lower(&hasher.finalize())
}

#[cfg(feature = "native-ts")]
fn update_stable_hash_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
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

#[cfg(test)]
mod tests;
