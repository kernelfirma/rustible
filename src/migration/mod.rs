//! Migration framework for importing from external tools.

pub mod error;
pub mod report;
pub mod ansible;

pub use error::{MigrationError, MigrationResult};
pub use report::{
    DiagnosticCategory, FindingStatus, MigrationDiagnostic, MigrationFinding, MigrationOutcome,
    MigrationReport, MigrationSeverity, ReportSummary,
};
