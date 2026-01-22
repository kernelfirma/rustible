//! Analysis Report Generation
//!
//! This module provides reporting capabilities for static analysis results.

use super::{AnalysisCategory, AnalysisFinding, ComplexityMetrics, DependencyGraph, Severity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Format for the analysis report
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportFormat {
    /// Plain text format
    Text,
    /// JSON format
    Json,
    /// SARIF format (Static Analysis Results Interchange Format)
    Sarif,
    /// GitHub Actions format
    GithubActions,
}

/// Complete analysis report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    /// Source file or project analyzed
    pub source: Option<String>,
    /// All findings from the analysis
    pub findings: Vec<AnalysisFinding>,
    /// Complexity metrics (if analyzed)
    pub complexity_metrics: Option<ComplexityMetrics>,
    /// Dependency graph (if analyzed)
    #[serde(skip)]
    pub dependency_graph: Option<DependencyGraph>,
    /// Summary statistics
    pub summary: ReportSummary,
}

/// Summary statistics for the report
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReportSummary {
    /// Total number of findings
    pub total_findings: usize,
    /// Count by severity
    pub by_severity: HashMap<String, usize>,
    /// Count by category
    pub by_category: HashMap<String, usize>,
    /// Whether there are any critical issues
    pub has_critical: bool,
    /// Whether there are any errors
    pub has_errors: bool,
    /// Overall health score (0-100)
    pub health_score: f64,
}

impl AnalysisReport {
    /// Create a new empty report
    pub fn new() -> Self {
        Self {
            source: None,
            findings: Vec::new(),
            complexity_metrics: None,
            dependency_graph: None,
            summary: ReportSummary::default(),
        }
    }

    /// Get findings filtered by severity
    pub fn findings_by_severity(&self, severity: Severity) -> Vec<&AnalysisFinding> {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .collect()
    }

    /// Get findings filtered by category
    pub fn findings_by_category(&self, category: AnalysisCategory) -> Vec<&AnalysisFinding> {
        self.findings
            .iter()
            .filter(|f| f.category == category)
            .collect()
    }

    /// Get the total number of issues
    pub fn issue_count(&self) -> usize {
        self.findings.len()
    }

    /// Check if there are any critical issues
    pub fn has_critical_issues(&self) -> bool {
        self.summary.has_critical
    }

    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        self.summary.has_errors
    }

    /// Get the exit code for CLI usage
    pub fn exit_code(&self) -> i32 {
        if self.summary.has_critical {
            3
        } else if self.summary.has_errors {
            2
        } else if self.summary.total_findings > 0 {
            1
        } else {
            0
        }
    }

    /// Format the report as text
    pub fn to_text(&self) -> String {
        let mut output = String::new();

        if let Some(source) = &self.source {
            output.push_str(&format!("Analysis Report for: {}\n", source));
            output.push_str(&"=".repeat(50));
            output.push('\n');
        }

        output.push_str("\nSummary:\n");
        output.push_str(&format!(
            "  Total findings: {}\n",
            self.summary.total_findings
        ));
        output.push_str(&format!(
            "  Health score: {:.1}/100\n",
            self.summary.health_score
        ));

        if !self.summary.by_severity.is_empty() {
            output.push_str("\n  By Severity:\n");
            for (severity, count) in &self.summary.by_severity {
                output.push_str(&format!("    {}: {}\n", severity, count));
            }
        }

        if !self.summary.by_category.is_empty() {
            output.push_str("\n  By Category:\n");
            for (category, count) in &self.summary.by_category {
                output.push_str(&format!("    {}: {}\n", category, count));
            }
        }

        if !self.findings.is_empty() {
            output.push_str("\nFindings:\n");
            output.push_str(&"-".repeat(50));
            output.push('\n');

            for finding in &self.findings {
                output.push_str(&format!(
                    "\n[{}] {} ({})\n",
                    finding.severity, finding.rule_id, finding.category
                ));
                output.push_str(&format!("  {}\n", finding.message));
                if !finding.description.is_empty() {
                    output.push_str(&format!("  Description: {}\n", finding.description));
                }
                output.push_str(&format!("  Location: {}\n", finding.location));
                if let Some(suggestion) = &finding.suggestion {
                    output.push_str(&format!("  Suggestion: {}\n", suggestion));
                }
            }
        }

        if let Some(metrics) = &self.complexity_metrics {
            output.push_str("\nComplexity Metrics:\n");
            output.push_str(&"-".repeat(50));
            output.push('\n');
            output.push_str(&format!("  Plays: {}\n", metrics.play_count));
            output.push_str(&format!("  Tasks: {}\n", metrics.task_count));
            output.push_str(&format!("  Handlers: {}\n", metrics.handler_count));
            output.push_str(&format!(
                "  Max Nesting Depth: {}\n",
                metrics.max_nesting_depth
            ));
            output.push_str(&format!(
                "  Cyclomatic Complexity: {}\n",
                metrics.cyclomatic_complexity
            ));
            output.push_str(&format!(
                "  Maintainability Index: {:.1}\n",
                metrics.maintainability_index
            ));
        }

        output
    }

    /// Format the report as JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Returns all findings as an iterator
    pub fn issues(&self) -> impl Iterator<Item = &AnalysisFinding> {
        self.findings.iter()
    }
}

impl Default for AnalysisReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating analysis reports
pub struct AnalysisReportBuilder {
    source: Option<String>,
    findings: Vec<AnalysisFinding>,
    complexity_metrics: Option<ComplexityMetrics>,
    dependency_graph: Option<DependencyGraph>,
}

impl AnalysisReportBuilder {
    /// Create a new report builder
    pub fn new() -> Self {
        Self {
            source: None,
            findings: Vec::new(),
            complexity_metrics: None,
            dependency_graph: None,
        }
    }

    /// Set the source file/project
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Add a finding
    pub fn add_finding(mut self, finding: AnalysisFinding) -> Self {
        self.findings.push(finding);
        self
    }

    /// Add multiple findings
    pub fn add_findings(mut self, findings: impl IntoIterator<Item = AnalysisFinding>) -> Self {
        self.findings.extend(findings);
        self
    }

    /// Set complexity metrics
    pub fn with_complexity_metrics(mut self, metrics: ComplexityMetrics) -> Self {
        self.complexity_metrics = Some(metrics);
        self
    }

    /// Set dependency graph
    pub fn with_dependency_graph(mut self, graph: DependencyGraph) -> Self {
        self.dependency_graph = Some(graph);
        self
    }

    /// Build the report
    pub fn build(self) -> AnalysisReport {
        let mut summary = ReportSummary {
            total_findings: self.findings.len(),
            ..Default::default()
        };

        // Count by severity
        for finding in &self.findings {
            let severity_key = format!("{}", finding.severity);
            *summary.by_severity.entry(severity_key).or_insert(0) += 1;

            let category_key = format!("{}", finding.category);
            *summary.by_category.entry(category_key).or_insert(0) += 1;

            if finding.severity == Severity::Critical {
                summary.has_critical = true;
            }
            if finding.severity == Severity::Error {
                summary.has_errors = true;
            }
        }

        // Calculate health score
        summary.health_score = calculate_health_score(&self.findings, &self.complexity_metrics);

        AnalysisReport {
            source: self.source,
            findings: self.findings,
            complexity_metrics: self.complexity_metrics,
            dependency_graph: self.dependency_graph,
            summary,
        }
    }
}

impl Default for AnalysisReportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate a health score based on findings and metrics
fn calculate_health_score(
    findings: &[AnalysisFinding],
    complexity_metrics: &Option<ComplexityMetrics>,
) -> f64 {
    let mut score = 100.0;

    // Deduct points based on findings
    for finding in findings {
        let deduction = match finding.severity {
            Severity::Critical => 15.0,
            Severity::Error => 10.0,
            Severity::Warning => 3.0,
            Severity::Info => 1.0,
            Severity::Hint => 0.5,
        };
        score -= deduction;
    }

    // Factor in maintainability index if available
    if let Some(metrics) = complexity_metrics {
        // Weight the maintainability index (0-100) into the score
        score = (score * 0.7) + (metrics.maintainability_index * 0.3);
    }

    score.clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::super::SourceLocation;
    use super::*;

    #[test]
    fn test_report_builder() {
        let report = AnalysisReportBuilder::new()
            .with_source("test.yml")
            .add_finding(AnalysisFinding::new(
                "TEST001",
                AnalysisCategory::Variable,
                Severity::Warning,
                "Test finding",
            ))
            .build();

        assert_eq!(report.source, Some("test.yml".to_string()));
        assert_eq!(report.summary.total_findings, 1);
        assert!(!report.summary.has_critical);
    }

    #[test]
    fn test_health_score() {
        let findings = vec![
            AnalysisFinding::new("T1", AnalysisCategory::Variable, Severity::Warning, "msg"),
            AnalysisFinding::new("T2", AnalysisCategory::Variable, Severity::Error, "msg"),
        ];

        let score = calculate_health_score(&findings, &None);
        assert!(score < 100.0);
        assert!(score > 0.0);
    }

    #[test]
    fn test_report_text_output() {
        let report = AnalysisReportBuilder::new()
            .with_source("playbook.yml")
            .add_finding(
                AnalysisFinding::new(
                    "VAR001",
                    AnalysisCategory::Variable,
                    Severity::Warning,
                    "Undefined variable",
                )
                .with_location(SourceLocation::new().with_file("playbook.yml")),
            )
            .build();

        let text = report.to_text();
        assert!(text.contains("playbook.yml"));
        assert!(text.contains("VAR001"));
    }
}
