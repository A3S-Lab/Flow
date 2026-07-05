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

    #[error("active hook token not found: {0}")]
    HookTokenNotFound(String),

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
