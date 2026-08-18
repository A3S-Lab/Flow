use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkflowDslError {
    #[error("workflow DSL document is {actual_bytes} bytes; maximum is {maximum_bytes} bytes")]
    DocumentTooLarge {
        actual_bytes: usize,
        maximum_bytes: usize,
    },

    #[error("workflow DSL YAML is invalid: {message}")]
    InvalidYaml { message: String },

    #[error("workflow DSL JSON is invalid: {message}")]
    InvalidJson { message: String },

    #[error("workflow DSL document is invalid: {message}")]
    InvalidDocument { message: String },

    #[error("workflow DAG is invalid: {message}")]
    InvalidGraph { message: String },

    #[error("workflow DSL serialization failed: {message}")]
    Serialization { message: String },
}
