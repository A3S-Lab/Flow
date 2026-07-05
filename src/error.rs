use thiserror::Error;

/// Crate-local result type.
pub type Result<T> = std::result::Result<T, FlowError>;

/// Errors surfaced by the workflow engine and runtime adapters.
#[derive(Debug, Error)]
pub enum FlowError {
    #[error("workflow run not found: {0}")]
    RunNotFound(String),

    #[error("workflow run {0} is already terminal")]
    RunTerminal(String),

    #[error("workflow run id is invalid: {0}")]
    InvalidRunId(String),

    #[error("workflow run {run_id} conflicts with existing run: {reason}")]
    RunConflict { run_id: String, reason: String },

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

    #[error("active hook token not found: {0}")]
    HookTokenNotFound(String),

    #[error(
        "active hook token {token:?} is already used by run {existing_run_id} hook {existing_hook_id}"
    )]
    HookTokenConflict {
        token: String,
        existing_run_id: String,
        existing_hook_id: String,
    },

    #[error("invalid workflow definition: {0}")]
    InvalidWorkflow(String),

    #[error("invalid state transition: {0}")]
    InvalidTransition(String),

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
