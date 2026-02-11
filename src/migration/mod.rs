//! Migration framework for importing infrastructure from external tools.
//!
//! This module provides a structured approach to migrating configuration
//! data from other infrastructure management systems into Rustible's
//! inventory format. Each source system has its own submodule with
//! parsers and mappers.
//!
//! # Supported Sources
//!
//! - **Warewulf** (feature `hpc`): Import container images and overlays
//!   from Warewulf cluster provisioning configurations.

pub mod error;
pub mod report;

#[cfg(feature = "hpc")]
pub mod warewulf;

pub use error::{MigrationError, MigrationResult};
pub use report::{MigrationDiagnostic, MigrationFinding, MigrationReport, MigrationSeverity};
