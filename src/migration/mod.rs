//! Migration and compatibility tools for importing from external tools.
//!
//! Provides importers for Warewulf, xCAT, and validators for
//! Terraform state/plan parity and Ansible compatibility.

pub mod error;
pub mod report;

#[cfg(feature = "provisioning")]
pub mod terraform;

pub use error::{MigrationError, MigrationResult};
pub use report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationOutcome,
    MigrationReport, MigrationSeverity, ReportSummary,
};
