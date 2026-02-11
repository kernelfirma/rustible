//! Migration error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("Source not found: {0}")]
    SourceNotFound(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Unsupported field: {0}")]
    UnsupportedField(String),
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

pub type MigrationResult<T> = Result<T, MigrationError>;
