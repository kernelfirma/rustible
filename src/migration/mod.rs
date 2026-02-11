//! Migration framework for importing configuration from external systems.
//!
//! This module provides a structured approach to migrating inventory, profiles,
//! and configuration data from HPC cluster management tools (such as Warewulf
//! and xCAT) into Rustible's inventory format.
//!
//! # Modules
//!
//! - [`error`] - Standard error types for migration operations.
//! - [`report`] - Structured reporting with diagnostics, findings, and outcomes.
//! - [`warewulf`] - Warewulf 4 profile and image import (requires `hpc` feature).

pub mod error;
pub mod report;

#[cfg(feature = "hpc")]
pub mod warewulf;

pub use error::{MigrationError, MigrationResult};
pub use report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationOutcome,
    MigrationReport, MigrationSeverity, ReportSummary,
};
