//! Error types for migration operations.

use thiserror::Error;

/// Result type for migration operations.
pub type MigrationResult<T> = Result<T, MigrationError>;

/// Errors that can occur during migration operations.
#[derive(Error, Debug)]
pub enum MigrationError {
    #[error("Source file not found: {0}")]
    SourceNotFound(String),

    #[error("Parse error in {file}: {message}")]
    ParseError { file: String, message: String },

    #[error("Schema mapping error: {field} has no Rustible equivalent")]
    UnsupportedField { field: String, source_tool: String },

    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Integrity check failed: expected {expected}, got {actual}")]
    IntegrityError { expected: String, actual: String },

    #[error("State mismatch: {resource}: {message}")]
    StateMismatch { resource: String, message: String },

    #[error("Plan divergence: {resource}: {message}")]
    PlanDivergence { resource: String, message: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Inventory error: {0}")]
    Inventory(String),

    #[error("Configuration error: {0}")]
    Config(String),
}
