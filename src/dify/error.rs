use thiserror::Error;

#[derive(Debug, Error)]
pub enum DifyImportError {
    #[error("Dify document is {actual_bytes} bytes; maximum is {maximum_bytes} bytes")]
    DocumentTooLarge {
        actual_bytes: usize,
        maximum_bytes: usize,
    },

    #[error("Dify YAML is invalid: {message}")]
    InvalidYaml { message: String },

    #[error("Dify JSON is invalid: {message}")]
    InvalidJson { message: String },

    #[error("Dify document is invalid: {message}")]
    InvalidDocument { message: String },

    #[error("Dify graph is invalid: {message}")]
    InvalidGraph { message: String },

    #[error("Dify document serialization failed: {message}")]
    Serialization { message: String },
}
