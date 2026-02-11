//! Migration framework for importing from external tools.

pub mod error;
pub mod report;

#[cfg(feature = "hpc")]
pub mod xcat;

pub use error::{MigrationError, MigrationResult};
pub use report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationOutcome,
    MigrationReport, MigrationSeverity, ReportSummary,
};
