//! Migration reporting and diagnostics.
//!
//! Provides structured reporting for migration operations including
//! per-object diagnostics, severity classification, and outcome
//! computation based on configurable thresholds.

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Severity level of a migration diagnostic or finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationSeverity {
    /// Informational message, no action required.
    Info,
    /// A potential issue that may need attention.
    Warning,
    /// An error that prevented correct migration of an object.
    Error,
}

/// Category of a diagnostic finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    /// Related to data parsing.
    Parsing,
    /// Related to field mapping between source and target.
    FieldMapping,
    /// Related to data validation.
    Validation,
    /// Related to an unsupported feature or object type.
    Unsupported,
}

/// Status of a finding (whether it was resolved or remains open).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingStatus {
    /// The finding is still open / unresolved.
    Open,
    /// The finding has been resolved or accepted.
    Resolved,
}

/// A single diagnostic message attached to a migration operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationDiagnostic {
    /// Severity of the diagnostic.
    pub severity: MigrationSeverity,
    /// Category of the diagnostic.
    pub category: DiagnosticCategory,
    /// Human-readable description.
    pub message: String,
    /// The source object this diagnostic relates to, if any.
    pub source_object: Option<String>,
}

/// A top-level finding that summarises one or more diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationFinding {
    /// Unique identifier within the report.
    pub id: String,
    /// Severity of the finding.
    pub severity: MigrationSeverity,
    /// Status of the finding.
    pub status: FindingStatus,
    /// Short title.
    pub title: String,
    /// Detailed description.
    pub description: String,
    /// Category of the finding.
    pub category: DiagnosticCategory,
    /// Related diagnostics.
    pub diagnostics: Vec<MigrationDiagnostic>,
}

/// Summary statistics for a migration report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    /// Total number of findings.
    pub total_findings: usize,
    /// Number of error-level findings.
    pub errors: usize,
    /// Number of warning-level findings.
    pub warnings: usize,
    /// Number of info-level findings.
    pub info: usize,
}

/// Overall outcome of a migration operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationOutcome {
    /// Migration succeeded with no issues.
    Success,
    /// Migration completed but some findings exceed the threshold.
    Degraded,
    /// Migration failed due to too many errors.
    Failed,
}

/// A complete migration report containing all findings and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    /// ISO-8601 timestamp of when the report was created.
    pub created_at: String,
    /// Human-readable label for the migration source.
    pub source_label: String,
    /// Ordered list of findings.
    pub findings: Vec<MigrationFinding>,
}

impl MigrationReport {
    /// Create a new empty report for the given source.
    pub fn new(source_label: impl Into<String>) -> Self {
        Self {
            created_at: Utc::now().to_rfc3339(),
            source_label: source_label.into(),
            findings: Vec::new(),
        }
    }

    /// Append a finding to the report.
    pub fn add_finding(&mut self, finding: MigrationFinding) {
        self.findings.push(finding);
    }

    /// Compute summary statistics from the current findings.
    pub fn compute_summary(&self) -> ReportSummary {
        let mut errors = 0usize;
        let mut warnings = 0usize;
        let mut info = 0usize;

        for f in &self.findings {
            match f.severity {
                MigrationSeverity::Error => errors += 1,
                MigrationSeverity::Warning => warnings += 1,
                MigrationSeverity::Info => info += 1,
            }
        }

        ReportSummary {
            total_findings: self.findings.len(),
            errors,
            warnings,
            info,
        }
    }

    /// Compute the overall outcome based on the number of errors
    /// relative to the given `threshold`.
    ///
    /// - 0 errors => `Success`
    /// - errors <= threshold => `Degraded`
    /// - errors > threshold => `Failed`
    pub fn compute_outcome(&self, threshold: usize) -> MigrationOutcome {
        let summary = self.compute_summary();
        if summary.errors == 0 {
            MigrationOutcome::Success
        } else if summary.errors <= threshold {
            MigrationOutcome::Degraded
        } else {
            MigrationOutcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_report_is_success() {
        let report = MigrationReport::new("test");
        assert_eq!(report.compute_outcome(5), MigrationOutcome::Success);
        let summary = report.compute_summary();
        assert_eq!(summary.total_findings, 0);
        assert_eq!(summary.errors, 0);
    }

    #[test]
    fn test_report_with_warnings_is_success() {
        let mut report = MigrationReport::new("test");
        report.add_finding(MigrationFinding {
            id: "W001".into(),
            severity: MigrationSeverity::Warning,
            status: FindingStatus::Open,
            title: "minor issue".into(),
            description: "something".into(),
            category: DiagnosticCategory::FieldMapping,
            diagnostics: vec![],
        });
        assert_eq!(report.compute_outcome(5), MigrationOutcome::Success);
    }

    #[test]
    fn test_report_errors_below_threshold_is_degraded() {
        let mut report = MigrationReport::new("test");
        report.add_finding(MigrationFinding {
            id: "E001".into(),
            severity: MigrationSeverity::Error,
            status: FindingStatus::Open,
            title: "error".into(),
            description: "something broke".into(),
            category: DiagnosticCategory::Validation,
            diagnostics: vec![],
        });
        assert_eq!(report.compute_outcome(5), MigrationOutcome::Degraded);
    }

    #[test]
    fn test_report_errors_above_threshold_is_failed() {
        let mut report = MigrationReport::new("test");
        for i in 0..6 {
            report.add_finding(MigrationFinding {
                id: format!("E{:03}", i),
                severity: MigrationSeverity::Error,
                status: FindingStatus::Open,
                title: "error".into(),
                description: "fail".into(),
                category: DiagnosticCategory::Parsing,
                diagnostics: vec![],
            });
        }
        assert_eq!(report.compute_outcome(5), MigrationOutcome::Failed);
        let summary = report.compute_summary();
        assert_eq!(summary.errors, 6);
    }
}
