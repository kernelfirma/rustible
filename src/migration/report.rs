//! Migration reporting types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Severity of a migration finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationSeverity {
    Error,
    Warning,
    Info,
}

/// Category of a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    ResourceMismatch,
    AttributeDivergence,
    DependencyMismatch,
    OutputMismatch,
    MissingResource,
    ExtraResource,
    UnsupportedFeature,
    CompatibilityIssue,
    Other(String),
}

/// Status of a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingStatus {
    Pass,
    Fail,
    Partial,
    Skipped,
}

/// A single diagnostic message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationDiagnostic {
    pub category: DiagnosticCategory,
    pub severity: MigrationSeverity,
    pub message: String,
    pub context: Option<String>,
}

/// A finding in a migration report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationFinding {
    pub name: String,
    pub status: FindingStatus,
    pub severity: MigrationSeverity,
    pub diagnostics: Vec<MigrationDiagnostic>,
}

/// Summary of a migration report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub partial: usize,
    pub skipped: usize,
    pub score: f64,
}

/// Outcome of a migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationOutcome {
    Pass,
    Fail,
}

/// A migration report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub title: String,
    pub source: String,
    pub target: String,
    pub timestamp: DateTime<Utc>,
    pub findings: Vec<MigrationFinding>,
    pub summary: Option<ReportSummary>,
    pub outcome: Option<MigrationOutcome>,
}

impl MigrationReport {
    /// Create a new empty report.
    pub fn new(title: impl Into<String>, source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            source: source.into(),
            target: target.into(),
            timestamp: Utc::now(),
            findings: Vec::new(),
            summary: None,
            outcome: None,
        }
    }

    /// Add a finding.
    pub fn add_finding(&mut self, finding: MigrationFinding) {
        self.findings.push(finding);
    }

    /// Compute the summary from findings.
    pub fn compute_summary(&mut self) {
        let total = self.findings.len();
        let passed = self.findings.iter().filter(|f| f.status == FindingStatus::Pass).count();
        let failed = self.findings.iter().filter(|f| f.status == FindingStatus::Fail).count();
        let partial = self.findings.iter().filter(|f| f.status == FindingStatus::Partial).count();
        let skipped = self.findings.iter().filter(|f| f.status == FindingStatus::Skipped).count();
        let score = if total > 0 {
            (passed as f64 + 0.5 * partial as f64) / total as f64 * 100.0
        } else {
            100.0
        };
        self.summary = Some(ReportSummary { total, passed, failed, partial, skipped, score });
    }

    /// Compute outcome based on a pass threshold (0-100).
    pub fn compute_outcome(&mut self, threshold: f64) {
        self.compute_summary();
        if let Some(ref s) = self.summary {
            self.outcome = Some(if s.score >= threshold {
                MigrationOutcome::Pass
            } else {
                MigrationOutcome::Fail
            });
        }
    }
}
