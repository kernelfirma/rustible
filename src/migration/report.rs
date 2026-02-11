//! Migration report types.
//!
//! Structured reporting for migration operations, including diagnostics,
//! findings, summaries, and overall outcome assessment.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Severity level for a migration diagnostic or finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationSeverity {
    /// Informational message; no action required.
    Info,
    /// A potential issue that may need attention.
    Warning,
    /// A problem that prevents correct migration of a specific item.
    Error,
}

impl fmt::Display for MigrationSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Category of a diagnostic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    /// Related to data parsing or format issues.
    Parsing,
    /// Related to field mapping or transformation.
    Mapping,
    /// Related to validation of migrated data.
    Validation,
    /// Related to missing or incomplete data.
    Completeness,
    /// Related to compatibility between source and target.
    Compatibility,
}

impl fmt::Display for DiagnosticCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parsing => write!(f, "parsing"),
            Self::Mapping => write!(f, "mapping"),
            Self::Validation => write!(f, "validation"),
            Self::Completeness => write!(f, "completeness"),
            Self::Compatibility => write!(f, "compatibility"),
        }
    }
}

/// Status of a finding (whether it was resolved automatically or requires action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    /// The issue was resolved automatically during migration.
    AutoResolved,
    /// The issue requires manual intervention.
    NeedsAction,
    /// The issue was acknowledged but deferred.
    Deferred,
}

impl fmt::Display for FindingStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AutoResolved => write!(f, "auto-resolved"),
            Self::NeedsAction => write!(f, "needs-action"),
            Self::Deferred => write!(f, "deferred"),
        }
    }
}

/// A single diagnostic message emitted during migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationDiagnostic {
    /// Severity of this diagnostic.
    pub severity: MigrationSeverity,
    /// Category of this diagnostic.
    pub category: DiagnosticCategory,
    /// Human-readable message.
    pub message: String,
    /// Optional context (e.g. the entity that triggered the diagnostic).
    pub context: Option<String>,
}

impl fmt::Display for MigrationDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.severity, self.category, self.message)?;
        if let Some(ctx) = &self.context {
            write!(f, " ({})", ctx)?;
        }
        Ok(())
    }
}

/// A migration finding that summarises an issue and its resolution status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationFinding {
    /// Severity of this finding.
    pub severity: MigrationSeverity,
    /// Category of this finding.
    pub category: DiagnosticCategory,
    /// Resolution status.
    pub status: FindingStatus,
    /// Human-readable title.
    pub title: String,
    /// Detailed description.
    pub description: String,
    /// Optional recommendation for resolution.
    pub recommendation: Option<String>,
}

impl fmt::Display for MigrationFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} ({}): {}",
            self.severity, self.title, self.status, self.description
        )
    }
}

/// Numeric summary of migration diagnostics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportSummary {
    /// Total number of entities processed.
    pub total_entities: usize,
    /// Number of entities successfully migrated.
    pub successful: usize,
    /// Number of entities with warnings.
    pub with_warnings: usize,
    /// Number of entities that failed to migrate.
    pub failed: usize,
    /// Count of info-level diagnostics.
    pub info_count: usize,
    /// Count of warning-level diagnostics.
    pub warning_count: usize,
    /// Count of error-level diagnostics.
    pub error_count: usize,
}

/// Outcome of a migration operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationOutcome {
    /// All entities migrated successfully with no issues.
    Success,
    /// Migration completed but with warnings that should be reviewed.
    PartialSuccess,
    /// Migration completed but error rate exceeds the acceptable threshold.
    Degraded,
    /// Migration failed entirely.
    Failed,
}

impl fmt::Display for MigrationOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::PartialSuccess => write!(f, "partial-success"),
            Self::Degraded => write!(f, "degraded"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Full migration report containing diagnostics, findings, summary, and outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    /// Source system description (e.g. "Warewulf 4 profiles").
    pub source: String,
    /// Diagnostics emitted during migration.
    pub diagnostics: Vec<MigrationDiagnostic>,
    /// High-level findings.
    pub findings: Vec<MigrationFinding>,
    /// Computed summary (populated by `compute_summary`).
    pub summary: Option<ReportSummary>,
    /// Overall outcome (populated by `compute_outcome`).
    pub outcome: Option<MigrationOutcome>,
}

impl MigrationReport {
    /// Create a new empty report for the given source system.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            diagnostics: Vec::new(),
            findings: Vec::new(),
            summary: None,
            outcome: None,
        }
    }

    /// Add a finding to the report.
    pub fn add_finding(&mut self, finding: MigrationFinding) {
        self.findings.push(finding);
    }

    /// Add a diagnostic to the report.
    pub fn add_diagnostic(&mut self, diagnostic: MigrationDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Compute the summary from the current diagnostics and findings.
    ///
    /// `total_entities` is the total number of entities that were processed.
    /// `successful` is the count of entities that migrated without errors.
    pub fn compute_summary(&mut self, total_entities: usize, successful: usize) {
        let info_count = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == MigrationSeverity::Info)
            .count();
        let warning_count = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == MigrationSeverity::Warning)
            .count();
        let error_count = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == MigrationSeverity::Error)
            .count();

        let failed = total_entities.saturating_sub(successful);
        let with_warnings = self
            .findings
            .iter()
            .filter(|f| f.severity == MigrationSeverity::Warning)
            .count();

        self.summary = Some(ReportSummary {
            total_entities,
            successful,
            with_warnings,
            failed,
            info_count,
            warning_count,
            error_count,
        });
    }

    /// Compute the overall outcome based on the summary.
    ///
    /// `threshold` is the maximum acceptable error ratio (0.0..=1.0).
    /// For example, `0.1` means up to 10% errors are tolerated as partial success.
    pub fn compute_outcome(&mut self, threshold: f64) -> MigrationOutcome {
        let summary = self.summary.clone().unwrap_or_default();

        let outcome = if summary.total_entities == 0 {
            MigrationOutcome::Success
        } else if summary.failed == 0 && summary.warning_count == 0 {
            MigrationOutcome::Success
        } else if summary.failed == 0 {
            MigrationOutcome::PartialSuccess
        } else {
            let error_ratio = summary.failed as f64 / summary.total_entities as f64;
            if error_ratio <= threshold {
                MigrationOutcome::PartialSuccess
            } else if summary.successful > 0 {
                MigrationOutcome::Degraded
            } else {
                MigrationOutcome::Failed
            }
        };

        self.outcome = Some(outcome);
        outcome
    }
}

impl fmt::Display for MigrationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Migration Report: {}", self.source)?;
        writeln!(f, "{}", "=".repeat(60))?;

        if let Some(ref summary) = self.summary {
            writeln!(f, "Entities: {} total, {} successful, {} failed",
                summary.total_entities, summary.successful, summary.failed)?;
            writeln!(f, "Diagnostics: {} info, {} warnings, {} errors",
                summary.info_count, summary.warning_count, summary.error_count)?;
        }

        if let Some(outcome) = self.outcome {
            writeln!(f, "Outcome: {}", outcome)?;
        }

        if !self.findings.is_empty() {
            writeln!(f, "\nFindings:")?;
            for finding in &self.findings {
                writeln!(f, "  - {}", finding)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_empty_is_success() {
        let mut report = MigrationReport::new("test");
        report.compute_summary(0, 0);
        let outcome = report.compute_outcome(0.1);
        assert_eq!(outcome, MigrationOutcome::Success);
    }

    #[test]
    fn test_report_all_successful() {
        let mut report = MigrationReport::new("test");
        report.compute_summary(5, 5);
        let outcome = report.compute_outcome(0.1);
        assert_eq!(outcome, MigrationOutcome::Success);
    }

    #[test]
    fn test_report_with_warnings_is_partial() {
        let mut report = MigrationReport::new("test");
        report.add_diagnostic(MigrationDiagnostic {
            severity: MigrationSeverity::Warning,
            category: DiagnosticCategory::Mapping,
            message: "unmapped field".to_string(),
            context: None,
        });
        report.compute_summary(5, 5);
        let outcome = report.compute_outcome(0.1);
        assert_eq!(outcome, MigrationOutcome::PartialSuccess);
    }

    #[test]
    fn test_report_with_failures_under_threshold() {
        let mut report = MigrationReport::new("test");
        report.compute_summary(10, 9);
        let outcome = report.compute_outcome(0.15);
        assert_eq!(outcome, MigrationOutcome::PartialSuccess);
    }

    #[test]
    fn test_report_with_failures_over_threshold() {
        let mut report = MigrationReport::new("test");
        report.compute_summary(10, 5);
        let outcome = report.compute_outcome(0.1);
        assert_eq!(outcome, MigrationOutcome::Degraded);
    }

    #[test]
    fn test_report_all_failed() {
        let mut report = MigrationReport::new("test");
        report.compute_summary(10, 0);
        let outcome = report.compute_outcome(0.1);
        assert_eq!(outcome, MigrationOutcome::Failed);
    }
}
