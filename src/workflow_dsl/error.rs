use thiserror::Error;

/// Failure returned while parsing, validating, planning, or encoding workflow DSL.
#[derive(Debug, Error)]
pub enum WorkflowDslError {
    /// The source exceeds the bounded parser input size.
    #[error("workflow DSL document is {actual_bytes} bytes; maximum is {maximum_bytes} bytes")]
    DocumentTooLarge {
        /// Actual UTF-8 byte length of the source.
        actual_bytes: usize,
        /// Maximum accepted UTF-8 byte length.
        maximum_bytes: usize,
    },

    /// YAML syntax or data conversion is invalid.
    #[error("workflow DSL YAML is invalid: {message}")]
    InvalidYaml {
        /// Parser or conversion error description.
        message: String,
    },

    /// JSON syntax or data conversion is invalid.
    #[error("workflow DSL JSON is invalid: {message}")]
    InvalidJson {
        /// Parser or conversion error description.
        message: String,
    },

    /// Document-level metadata or compatibility is invalid.
    #[error("workflow DSL document is invalid: {message}")]
    InvalidDocument {
        /// Validation error description.
        message: String,
    },

    /// The DAG cannot produce a valid deterministic execution plan.
    #[error("workflow DAG is invalid: {message}")]
    InvalidGraph {
        /// Structural validation error description.
        message: String,
    },

    /// A validated document or graph could not be serialized.
    #[error("workflow DSL serialization failed: {message}")]
    Serialization {
        /// Serializer error description.
        message: String,
    },
}
