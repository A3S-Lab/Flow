use std::fmt;

use thiserror::Error;

use crate::runtime_build::RuntimeBuildId;

/// Crate-local result type.
pub type Result<T> = std::result::Result<T, FlowError>;

/// Errors surfaced by the workflow engine and runtime adapters.
#[derive(Error)]
#[non_exhaustive]
pub enum FlowError {
    /// A requested workflow run does not exist.
    #[error("workflow run not found: {0}")]
    RunNotFound(String),

    /// An operation attempted to append work to a terminal run.
    #[error("workflow run {0} is already terminal")]
    RunTerminal(String),

    /// A run identifier violates the storage-safe identifier contract.
    #[error("workflow run id is invalid: {0}")]
    InvalidRunId(String),

    /// A continue-as-new chain contains a repeated run.
    #[error("continue-as-new chain contains a cycle at workflow run {0}")]
    ContinueAsNewCycle(String),

    /// A continue-as-new chain exceeds the configured traversal bound.
    #[error("continue-as-new chain exceeded the configured limit of {0} hops")]
    ContinueAsNewLimitExceeded(usize),

    /// Parent and child ownership links contain a cycle.
    #[error("child workflow graph contains a cycle at workflow run {0}")]
    ChildWorkflowCycle(String),

    /// A child workflow request exceeds the configured nesting bound.
    #[error("child workflow nesting exceeded the configured depth of {0}")]
    ChildWorkflowDepthExceeded(usize),

    /// An idempotent run start disagrees with the existing run definition.
    #[error("workflow run {run_id} conflicts with existing run: {reason}")]
    RunConflict {
        /// Existing run identifier.
        run_id: String,
        /// Description of the conflicting immutable input.
        reason: String,
    },

    /// A runtime build identity is empty or malformed.
    #[error("invalid runtime build identity: {0}")]
    InvalidRuntimeBuildId(String),

    /// A workflow patch identity is empty, malformed, or too long.
    #[error("invalid workflow patch identity: {0}")]
    InvalidWorkflowPatchId(String),

    /// No configured runtime can replay the run's pinned build.
    #[error(
        "workflow run {run_id} requires runtime build {required_build_id:?}, but the configured current build is {current_build_id:?}"
    )]
    RuntimeBuildUnavailable {
        /// Run that requires replay admission.
        run_id: String,
        /// Runtime build pinned by the run, if any.
        required_build_id: Option<RuntimeBuildId>,
        /// Runtime build configured on the attempted executor, if any.
        current_build_id: Option<RuntimeBuildId>,
    },

    /// No task queue route is registered for a required runtime build.
    #[error("no Flow task route is registered for runtime build {required_build_id:?}")]
    RuntimeBuildRouteNotFound {
        /// Runtime build required by the task, if pinned.
        required_build_id: Option<RuntimeBuildId>,
    },

    /// Runtime replay emitted a command that conflicts with durable history.
    #[error("non-deterministic workflow replay for run {run_id}: {reason}")]
    NonDeterministic {
        /// Run whose replay diverged.
        run_id: String,
        /// Description of the durable command mismatch.
        reason: String,
    },

    /// An optimistic event append used a stale expected sequence.
    #[error(
        "event sequence conflict for run {run_id}: expected {expected_sequence}, actual {actual_sequence}"
    )]
    EventConflict {
        /// Run whose history changed concurrently.
        run_id: String,
        /// Last sequence assumed by the caller.
        expected_sequence: u64,
        /// Last sequence currently stored.
        actual_sequence: u64,
    },

    /// A persisted event envelope uses a schema version this crate cannot
    /// safely replay. Hosts should migrate or upcast the history first.
    #[error(
        "unsupported flow event envelope schema version {version}; supported version is {supported}"
    )]
    UnsupportedEventSchemaVersion {
        /// Version encoded by the persisted envelope.
        version: u16,
        /// Highest version understood by this crate.
        supported: u16,
    },

    /// The original token remains available for programmatic routing, while
    /// `Display` and `Debug` deliberately redact it.
    #[error("active hook token not found (value redacted)")]
    HookTokenNotFound(String),

    /// A queue lease was lost before its task could be acknowledged.
    #[error("workflow task lease is no longer active: {0}")]
    LeaseLost(String),

    /// The conflicting token remains available for programmatic handling,
    /// while `Display` and `Debug` deliberately redact it.
    #[error(
        "active hook token is already used by run {existing_run_id} hook {existing_hook_id} (value redacted)"
    )]
    HookTokenConflict {
        /// Conflicting bearer token, retained only for programmatic recovery.
        token: String,
        /// Run that already owns the token.
        existing_run_id: String,
        /// Hook that already owns the token.
        existing_hook_id: String,
    },

    /// A hook retry conflicts with its durable identity or resolution.
    #[error("hook {hook_id} for workflow run {run_id} conflicts with request: {reason}")]
    HookConflict {
        /// Run that owns the hook.
        run_id: String,
        /// Replay-stable hook identity.
        hook_id: String,
        /// Description of the conflicting request.
        reason: String,
    },

    /// A signal retry conflicts with its durable identity or payload.
    #[error("signal {signal_id} for workflow run {run_id} conflicts with request: {reason}")]
    SignalConflict {
        /// Run targeted by the signal.
        run_id: String,
        /// Caller-owned signal identity.
        signal_id: String,
        /// Description of the conflicting delivery.
        reason: String,
    },

    /// A workflow definition violates a static invariant.
    #[error("invalid workflow definition: {0}")]
    InvalidWorkflow(String),

    /// An event or command violates the current durable state.
    #[error("invalid state transition: {0}")]
    InvalidTransition(String),

    /// Worker settings cannot provide the requested execution guarantees.
    #[error("invalid worker configuration: {0}")]
    InvalidWorkerConfiguration(String),

    /// An external task manager rejected or failed an operation.
    #[error("task manager error: {0}")]
    TaskManagement(String),

    /// A durable event store rejected or failed an operation.
    #[error("event store error: {0}")]
    Store(String),

    /// A workflow or step runtime failed outside application commands.
    #[error("runtime error: {0}")]
    Runtime(String),

    /// A step returned an application error that must not be retried.
    #[error("non-retryable step error: {0}")]
    NonRetryable(String),

    /// JSON serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Filesystem, process, or stream I/O failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Replay emitted more commands than the configured safety bound.
    #[error("workflow replay exceeded {0} iterations")]
    ReplayLimitExceeded(usize),
}

impl FlowError {
    /// Return whether a failed step may be retried by its configured policy.
    ///
    /// Runtime failures remain retryable for backwards compatibility; step
    /// handlers opt out explicitly with [`FlowError::NonRetryable`].
    pub fn is_retryable(&self) -> bool {
        !matches!(self, Self::NonRetryable(_))
    }
}

// Error values can retain callback tokens for programmatic recovery, but
// diagnostics must never reveal those bearer credentials. Keep ordinary
// variants structurally useful while replacing token fields in Debug output.
impl fmt::Debug for FlowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RunNotFound(run_id) => {
                formatter.debug_tuple("RunNotFound").field(run_id).finish()
            }
            Self::RunTerminal(run_id) => {
                formatter.debug_tuple("RunTerminal").field(run_id).finish()
            }
            Self::InvalidRunId(run_id) => {
                formatter.debug_tuple("InvalidRunId").field(run_id).finish()
            }
            Self::ContinueAsNewCycle(run_id) => formatter
                .debug_tuple("ContinueAsNewCycle")
                .field(run_id)
                .finish(),
            Self::ContinueAsNewLimitExceeded(limit) => formatter
                .debug_tuple("ContinueAsNewLimitExceeded")
                .field(limit)
                .finish(),
            Self::ChildWorkflowCycle(run_id) => formatter
                .debug_tuple("ChildWorkflowCycle")
                .field(run_id)
                .finish(),
            Self::ChildWorkflowDepthExceeded(limit) => formatter
                .debug_tuple("ChildWorkflowDepthExceeded")
                .field(limit)
                .finish(),
            Self::RunConflict { run_id, reason } => formatter
                .debug_struct("RunConflict")
                .field("run_id", run_id)
                .field("reason", reason)
                .finish(),
            Self::InvalidRuntimeBuildId(reason) => formatter
                .debug_tuple("InvalidRuntimeBuildId")
                .field(reason)
                .finish(),
            Self::InvalidWorkflowPatchId(reason) => formatter
                .debug_tuple("InvalidWorkflowPatchId")
                .field(reason)
                .finish(),
            Self::RuntimeBuildUnavailable {
                run_id,
                required_build_id,
                current_build_id,
            } => formatter
                .debug_struct("RuntimeBuildUnavailable")
                .field("run_id", run_id)
                .field("required_build_id", required_build_id)
                .field("current_build_id", current_build_id)
                .finish(),
            Self::RuntimeBuildRouteNotFound { required_build_id } => formatter
                .debug_struct("RuntimeBuildRouteNotFound")
                .field("required_build_id", required_build_id)
                .finish(),
            Self::NonDeterministic { run_id, reason } => formatter
                .debug_struct("NonDeterministic")
                .field("run_id", run_id)
                .field("reason", reason)
                .finish(),
            Self::EventConflict {
                run_id,
                expected_sequence,
                actual_sequence,
            } => formatter
                .debug_struct("EventConflict")
                .field("run_id", run_id)
                .field("expected_sequence", expected_sequence)
                .field("actual_sequence", actual_sequence)
                .finish(),
            Self::UnsupportedEventSchemaVersion { version, supported } => formatter
                .debug_struct("UnsupportedEventSchemaVersion")
                .field("version", version)
                .field("supported", supported)
                .finish(),
            Self::HookTokenNotFound(_) => formatter
                .debug_tuple("HookTokenNotFound")
                .field(&"<redacted>")
                .finish(),
            Self::LeaseLost(lease_id) => {
                formatter.debug_tuple("LeaseLost").field(lease_id).finish()
            }
            Self::HookTokenConflict {
                existing_run_id,
                existing_hook_id,
                ..
            } => formatter
                .debug_struct("HookTokenConflict")
                .field("token", &"<redacted>")
                .field("existing_run_id", existing_run_id)
                .field("existing_hook_id", existing_hook_id)
                .finish(),
            Self::HookConflict {
                run_id,
                hook_id,
                reason,
            } => formatter
                .debug_struct("HookConflict")
                .field("run_id", run_id)
                .field("hook_id", hook_id)
                .field("reason", reason)
                .finish(),
            Self::SignalConflict {
                run_id,
                signal_id,
                reason,
            } => formatter
                .debug_struct("SignalConflict")
                .field("run_id", run_id)
                .field("signal_id", signal_id)
                .field("reason", reason)
                .finish(),
            Self::InvalidWorkflow(message) => formatter
                .debug_tuple("InvalidWorkflow")
                .field(message)
                .finish(),
            Self::InvalidTransition(message) => formatter
                .debug_tuple("InvalidTransition")
                .field(message)
                .finish(),
            Self::InvalidWorkerConfiguration(message) => formatter
                .debug_tuple("InvalidWorkerConfiguration")
                .field(message)
                .finish(),
            Self::TaskManagement(message) => formatter
                .debug_tuple("TaskManagement")
                .field(message)
                .finish(),
            Self::Store(message) => formatter.debug_tuple("Store").field(message).finish(),
            Self::Runtime(message) => formatter.debug_tuple("Runtime").field(message).finish(),
            Self::NonRetryable(message) => formatter
                .debug_tuple("NonRetryable")
                .field(message)
                .finish(),
            Self::Serialization(error) => {
                formatter.debug_tuple("Serialization").field(error).finish()
            }
            Self::Io(error) => formatter.debug_tuple("Io").field(error).finish(),
            Self::ReplayLimitExceeded(limit) => formatter
                .debug_tuple("ReplayLimitExceeded")
                .field(limit)
                .finish(),
        }
    }
}
