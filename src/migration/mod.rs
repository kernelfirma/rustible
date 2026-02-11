//! Migration framework for importing from external tools.
//!
//! Provides shared types, error handling, and reporting for migrating
//! configurations and state from Terraform, Ansible, xCAT, Warewulf, etc.

pub mod error;
pub mod report;

#[cfg(feature = "provisioning")]
pub mod terraform;

pub use error::{MigrationError, MigrationResult};
pub use report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationOutcome,
    MigrationReport, MigrationSeverity, ReportSummary,
};
