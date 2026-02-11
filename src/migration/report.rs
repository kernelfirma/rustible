//! Migration report types shared across all migration tools.

use serde::{Deserialize, Serialize};

/// Outcome of a migration or validation operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationOutcome {
    Pass,
    PassWithWarnings,
    Fail,
}

/// Severity of a migration diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MigrationSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Category of a migration diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticCategory {
    UnsupportedField,
    TypeMismatch,
    MissingDependency,
    AttributeMismatch,
    OutputMismatch,
    SemanticDivergence,
    IntegrityFailure,
    DeprecatedFeature,
    CompatibilityGap,
    RoutingValidation,
}

/// A single diagnostic finding from a migration operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationDiagnostic {
    pub category: DiagnosticCategory,
    pub severity: MigrationSeverity,
    pub source_path: Option<String>,
    pub source_field: Option<String>,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Status of a migration finding for a single item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingStatus {
    Mapped,
    PartiallyMapped,
    Skipped,
    Divergent,
    Matched,
}

/// A finding for a single resource/item during migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationFinding {
    pub source_item: String,
    pub target_item: Option<String>,
    pub status: FindingStatus,
    pub diagnostics: Vec<MigrationDiagnostic>,
}

/// Summary statistics for a migration report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportSummary {
    pub total_items: usize,
    pub mapped: usize,
    pub partially_mapped: usize,
    pub skipped: usize,
    pub divergent: usize,
    pub matched: usize,
    pub errors: usize,
    pub warnings: usize,
}

/// Complete migration report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub tool: String,
    pub operation: String,
    pub outcome: MigrationOutcome,
    pub compatibility_score: f64,
    pub findings: Vec<MigrationFinding>,
    pub summary: ReportSummary,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl MigrationReport {
    /// Create a new migration report.
    pub fn new(tool: &str, operation: &str) -> Self {
        Self {
            tool: tool.to_string(),
            operation: operation.to_string(),
            outcome: MigrationOutcome::Pass,
            compatibility_score: 1.0,
            findings: Vec::new(),
            summary: ReportSummary::default(),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Serialize the report to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Get the exit code based on outcome.
    pub fn exit_code(&self) -> i32 {
        match self.outcome {
            MigrationOutcome::Pass => 0,
            MigrationOutcome::PassWithWarnings => 0,
            MigrationOutcome::Fail => 1,
        }
    }

    /// Compute summary from findings.
    pub fn compute_summary(&mut self) {
        let mut summary = ReportSummary {
            total_items: self.findings.len(),
            ..Default::default()
        };
        for finding in &self.findings {
            match finding.status {
                FindingStatus::Mapped => summary.mapped += 1,
                FindingStatus::PartiallyMapped => summary.partially_mapped += 1,
                FindingStatus::Skipped => summary.skipped += 1,
                FindingStatus::Divergent => summary.divergent += 1,
                FindingStatus::Matched => summary.matched += 1,
            }
            for diag in &finding.diagnostics {
                match diag.severity {
                    MigrationSeverity::Error | MigrationSeverity::Critical => {
                        summary.errors += 1
                    }
                    MigrationSeverity::Warning => summary.warnings += 1,
                    _ => {}
                }
            }
        }
        self.summary = summary;
    }

    /// Compute the outcome based on threshold.
    pub fn compute_outcome(&mut self, threshold: f64) {
        if self.summary.total_items == 0 {
            self.outcome = MigrationOutcome::Pass;
            self.compatibility_score = 1.0;
            return;
        }
        let score = (self.summary.matched as f64
            + self.summary.mapped as f64
            + 0.5 * self.summary.partially_mapped as f64)
            / self.summary.total_items as f64;
        self.compatibility_score = score;
        if score >= threshold && self.summary.errors == 0 {
            if self.summary.warnings > 0 {
                self.outcome = MigrationOutcome::PassWithWarnings;
            } else {
                self.outcome = MigrationOutcome::Pass;
            }
        } else {
            self.outcome = MigrationOutcome::Fail;
        }
    }
}
