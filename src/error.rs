use std::fmt;

use thiserror::Error;

use crate::runtime_build::RuntimeBuildId;

/// Crate-local result type.
pub type Result<T> = std::result::Result<T, FlowError>;

/// Errors surfaced by the workflow engine and runtime adapters.
#[derive(Error)]
pub enum FlowError {
    #[error("workflow run not found: {0}")]
    RunNotFound(String),

    #[error("workflow run {0} is already terminal")]
    RunTerminal(String),

    #[error("workflow run id is invalid: {0}")]
    InvalidRunId(String),

    #[error("continue-as-new chain contains a cycle at workflow run {0}")]
    ContinueAsNewCycle(String),

    #[error("continue-as-new chain exceeded the configured limit of {0} hops")]
    ContinueAsNewLimitExceeded(usize),

    #[error("child workflow graph contains a cycle at workflow run {0}")]
    ChildWorkflowCycle(String),

    #[error("child workflow nesting exceeded the configured depth of {0}")]
    ChildWorkflowDepthExceeded(usize),

    #[error("workflow run {run_id} conflicts with existing run: {reason}")]
    RunConflict { run_id: String, reason: String },

    #[error("invalid runtime build identity: {0}")]
    InvalidRuntimeBuildId(String),

    #[error("invalid workflow patch identity: {0}")]
    InvalidWorkflowPatchId(String),

    #[error(
        "workflow run {run_id} requires runtime build {required_build_id:?}, but the configured current build is {current_build_id:?}"
    )]
    RuntimeBuildUnavailable {
        run_id: String,
        required_build_id: Option<RuntimeBuildId>,
        current_build_id: Option<RuntimeBuildId>,
    },

    #[error("no Flow task route is registered for runtime build {required_build_id:?}")]
    RuntimeBuildRouteNotFound {
        required_build_id: Option<RuntimeBuildId>,
    },

    #[error("non-deterministic workflow replay for run {run_id}: {reason}")]
    NonDeterministic { run_id: String, reason: String },

    #[error(
        "event sequence conflict for run {run_id}: expected {expected_sequence}, actual {actual_sequence}"
    )]
    EventConflict {
        run_id: String,
        expected_sequence: u64,
        actual_sequence: u64,
    },

    /// The original token remains available for programmatic routing, while
    /// `Display` and `Debug` deliberately redact it.
    #[error("active hook token not found (value redacted)")]
    HookTokenNotFound(String),

    #[error("workflow task lease is no longer active: {0}")]
    LeaseLost(String),

    /// The conflicting token remains available for programmatic handling,
    /// while `Display` and `Debug` deliberately redact it.
    #[error(
        "active hook token is already used by run {existing_run_id} hook {existing_hook_id} (value redacted)"
    )]
    HookTokenConflict {
        token: String,
        existing_run_id: String,
        existing_hook_id: String,
    },

    #[error("hook {hook_id} for workflow run {run_id} conflicts with request: {reason}")]
    HookConflict {
        run_id: String,
        hook_id: String,
        reason: String,
    },

    #[error("signal {signal_id} for workflow run {run_id} conflicts with request: {reason}")]
    SignalConflict {
        run_id: String,
        signal_id: String,
        reason: String,
    },

    #[error("invalid workflow definition: {0}")]
    InvalidWorkflow(String),

    #[error("invalid state transition: {0}")]
    InvalidTransition(String),

    #[error("invalid worker configuration: {0}")]
    InvalidWorkerConfiguration(String),

    #[error("task manager error: {0}")]
    TaskManagement(String),

    #[error("event store error: {0}")]
    Store(String),

    #[error("runtime error: {0}")]
    Runtime(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("workflow replay exceeded {0} iterations")]
    ReplayLimitExceeded(usize),
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
