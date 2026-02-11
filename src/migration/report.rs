//! Migration reporting types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationSeverity { Error, Warning, Info }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    ResourceMismatch, AttributeDivergence, DependencyMismatch, OutputMismatch,
    MissingResource, ExtraResource, UnsupportedFeature, CompatibilityIssue, Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingStatus { Pass, Fail, Partial, Skipped }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationDiagnostic {
    pub category: DiagnosticCategory,
    pub severity: MigrationSeverity,
    pub message: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationFinding {
    pub name: String,
    pub status: FindingStatus,
    pub severity: MigrationSeverity,
    pub diagnostics: Vec<MigrationDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    pub total: usize, pub passed: usize, pub failed: usize,
    pub partial: usize, pub skipped: usize, pub score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationOutcome { Pass, Fail }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub title: String, pub source: String, pub target: String,
    pub timestamp: DateTime<Utc>, pub findings: Vec<MigrationFinding>,
    pub summary: Option<ReportSummary>, pub outcome: Option<MigrationOutcome>,
}

impl MigrationReport {
    pub fn new(title: impl Into<String>, source: impl Into<String>, target: impl Into<String>) -> Self {
        Self { title: title.into(), source: source.into(), target: target.into(),
            timestamp: Utc::now(), findings: Vec::new(), summary: None, outcome: None }
    }
    pub fn add_finding(&mut self, f: MigrationFinding) { self.findings.push(f); }
    pub fn compute_summary(&mut self) {
        let t = self.findings.len();
        let p = self.findings.iter().filter(|f| f.status == FindingStatus::Pass).count();
        let f = self.findings.iter().filter(|f| f.status == FindingStatus::Fail).count();
        let pa = self.findings.iter().filter(|f| f.status == FindingStatus::Partial).count();
        let s = self.findings.iter().filter(|f| f.status == FindingStatus::Skipped).count();
        let sc = if t > 0 { (p as f64 + 0.5 * pa as f64) / t as f64 * 100.0 } else { 100.0 };
        self.summary = Some(ReportSummary { total: t, passed: p, failed: f, partial: pa, skipped: s, score: sc });
    }
    pub fn compute_outcome(&mut self, threshold: f64) {
        self.compute_summary();
        if let Some(ref s) = self.summary {
            self.outcome = Some(if s.score >= threshold { MigrationOutcome::Pass } else { MigrationOutcome::Fail });
        }
    }
}
