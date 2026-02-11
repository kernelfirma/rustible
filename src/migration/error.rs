//! Migration error types.
//!
//! Standard error types for migration operations including parsing,
//! validation, and I/O failures.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during migration operations.
#[derive(Error, Debug)]
pub enum MigrationError {
    /// Failed to read a source file.
    #[error("failed to read migration source '{path}': {message}")]
    IoError {
        /// Path to the file that could not be read.
        path: PathBuf,
        /// Human-readable description of the failure.
        message: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse input data (YAML, JSON, etc.).
    #[error("failed to parse migration input: {message}")]
    ParseError {
        /// Human-readable description of the parse failure.
        message: String,
        /// Optional source path where the parse error occurred.
        source_path: Option<PathBuf>,
    },

    /// A required field or value was missing from the source data.
    #[error("missing required field '{field}' in {context}")]
    MissingField {
        /// Name of the missing field.
        field: String,
        /// Context where the field was expected (e.g. "node definition").
        context: String,
    },

    /// A value failed validation constraints.
    #[error("validation error: {message}")]
    ValidationError {
        /// Description of the validation failure.
        message: String,
    },

    /// The migration source format is not supported.
    #[error("unsupported migration source: {format}")]
    UnsupportedFormat {
        /// Name or description of the unsupported format.
        format: String,
    },

    /// A mapping or transformation failed.
    #[error("mapping error for '{entity}': {message}")]
    MappingError {
        /// The entity being mapped (e.g. "node compute-01").
        entity: String,
        /// Description of the mapping failure.
        message: String,
    },
}

/// Result type alias for migration operations.
pub type MigrationResult<T> = std::result::Result<T, MigrationError>;
