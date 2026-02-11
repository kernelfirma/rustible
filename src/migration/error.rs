//! Error types for the migration framework.

use thiserror::Error;

/// Errors that can occur during a migration operation.
#[derive(Debug, Error)]
pub enum MigrationError {
    /// The specified migration source file or directory was not found.
    #[error("migration source not found: {0}")]
    SourceNotFound(String),

    /// Failed to parse the source data format.
    #[error("parse error: {0}")]
    ParseError(String),

    /// A field in the source data is not supported by the target model.
    #[error("unsupported field '{field}' in object '{object}'")]
    UnsupportedField {
        /// The object that contains the unsupported field.
        object: String,
        /// The field name that is not supported.
        field: String,
    },

    /// Validation of the migrated data failed.
    #[error("validation failed: {0}")]
    ValidationFailed(String),

    /// An I/O error occurred during migration.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// A YAML serialization/deserialization error.
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Convenience type alias for migration results.
pub type MigrationResult<T> = std::result::Result<T, MigrationError>;
