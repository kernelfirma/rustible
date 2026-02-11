//! Migration framework for importing infrastructure from external tools.
//!
//! This module provides a structured approach to migrating configuration
//! data from other infrastructure management systems into Rustible's
//! inventory format. Each source system has its own submodule with
//! parsers and mappers.
//!
//! # Supported Sources
//!
//! - **xCAT** (feature `hpc`): Import node definitions, groups, and
//!   network objects from xCAT's `lsdef` output.

pub mod error;
pub mod report;

#[cfg(feature = "hpc")]
pub mod xcat;

pub use error::{MigrationError, MigrationResult};
pub use report::{MigrationDiagnostic, MigrationFinding, MigrationReport, MigrationSeverity};
