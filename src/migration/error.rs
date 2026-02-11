//! Migration error types.

use thiserror::Error;

/// Errors that can occur during migration operations.
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

    #[error("Integrity error: {0}")]
    IntegrityError(String),

    #[error("State mismatch: {0}")]
    StateMismatch(String),

    #[error("Plan divergence: {0}")]
    PlanDivergence(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Result type alias for migration operations.
pub type MigrationResult<T> = Result<T, MigrationError>;
