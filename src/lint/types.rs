//! Linter types and error definitions.
//!
//! This module defines the core types used throughout the linting system,
//! including severity levels, lint results, and issue representations.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Severity level for lint issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Severity {
    /// Informational hint - style suggestion, not a problem.
    Hint,
    /// Warning - potential issue that should be reviewed.
    #[default]
    Warning,
    /// Error - definite problem that will cause issues.
    Error,
    /// Critical - severe security or correctness issue.
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Hint => write!(f, "hint"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
            Severity::Critical => write!(f, "critical"),
        }
    }
}

/// Category of lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleCategory {
    /// YAML syntax and structure issues.
    Syntax,
    /// Module parameter validation.
    Parameters,
    /// Best practices and style.
    BestPractices,
    /// Security vulnerabilities and risks.
    Security,
    /// Deprecation warnings.
    Deprecation,
    /// Performance concerns.
    Performance,
    /// Custom user-defined rules.
    Custom,
}

impl std::fmt::Display for RuleCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleCategory::Syntax => write!(f, "syntax"),
            RuleCategory::Parameters => write!(f, "parameters"),
            RuleCategory::BestPractices => write!(f, "best-practices"),
            RuleCategory::Security => write!(f, "security"),
            RuleCategory::Deprecation => write!(f, "deprecation"),
            RuleCategory::Performance => write!(f, "performance"),
            RuleCategory::Custom => write!(f, "custom"),
        }
    }
}

/// Location information for a lint issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// Path to the file.
    pub file: PathBuf,
    /// Line number (1-indexed).
    pub line: Option<usize>,
    /// Column number (1-indexed).
    pub column: Option<usize>,
    /// Play index (0-indexed) if applicable.
    pub play_index: Option<usize>,
    /// Task index (0-indexed) within the play if applicable.
    pub task_index: Option<usize>,
    /// Play name if available.
    pub play_name: Option<String>,
    /// Task name if available.
    pub task_name: Option<String>,
}

impl Location {
    /// Create a new location with just a file path.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            file: path.into(),
            line: None,
            column: None,
            play_index: None,
            task_index: None,
            play_name: None,
            task_name: None,
        }
    }

    /// Add line number.
    pub fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    /// Add column number.
    pub fn with_column(mut self, column: usize) -> Self {
        self.column = Some(column);
        self
    }

    /// Add play context.
    pub fn with_play(mut self, index: usize, name: Option<String>) -> Self {
        self.play_index = Some(index);
        self.play_name = name;
        self
    }

    /// Add task context.
    pub fn with_task(mut self, index: usize, name: Option<String>) -> Self {
        self.task_index = Some(index);
        self.task_name = name;
        self
    }

    /// Format as a location string.
    pub fn to_location_string(&self) -> String {
        let mut s = self.file.display().to_string();
        if let Some(line) = self.line {
            s.push(':');
            s.push_str(&line.to_string());
            if let Some(col) = self.column {
                s.push(':');
                s.push_str(&col.to_string());
            }
        }
        s
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_location_string())
    }
}

/// A single lint issue found during analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintIssue {
    /// Unique rule identifier (e.g., "E001", "W002", "S001").
    pub rule_id: String,
    /// Human-readable rule name.
    pub rule_name: String,
    /// Severity level.
    pub severity: Severity,
    /// Category of the rule.
    pub category: RuleCategory,
    /// Description of the issue.
    pub message: String,
    /// Location where the issue was found.
    pub location: Location,
    /// Suggested fix or improvement.
    pub suggestion: Option<String>,
    /// URL to documentation for this rule.
    pub documentation_url: Option<String>,
}

impl LintIssue {
    /// Create a new lint issue.
    pub fn new(
        rule_id: impl Into<String>,
        rule_name: impl Into<String>,
        severity: Severity,
        category: RuleCategory,
        message: impl Into<String>,
        location: Location,
    ) -> Self {
        Self {
            rule_id: rule_id.into(),
            rule_name: rule_name.into(),
            severity,
            category,
            message: message.into(),
            location,
            suggestion: None,
            documentation_url: None,
        }
    }

    /// Add a suggestion for fixing the issue.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Add a documentation URL.
    pub fn with_docs(mut self, url: impl Into<String>) -> Self {
        self.documentation_url = Some(url.into());
        self
    }

    /// Check if this is an error or critical issue.
    pub fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error | Severity::Critical)
    }
}

impl std::fmt::Display for LintIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: [{}] {} - {}",
            self.location, self.rule_id, self.severity, self.message
        )?;
        if let Some(ref suggestion) = self.suggestion {
            write!(f, "\n  Suggestion: {}", suggestion)?;
        }
        Ok(())
    }
}

/// Result of linting a playbook or file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintResult {
    /// List of issues found.
    pub issues: Vec<LintIssue>,
    /// Files that were analyzed.
    pub files_analyzed: Vec<PathBuf>,
    /// Total number of plays analyzed.
    pub plays_analyzed: usize,
    /// Total number of tasks analyzed.
    pub tasks_analyzed: usize,
}

impl LintResult {
    /// Create a new empty result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an issue.
    pub fn add_issue(&mut self, issue: LintIssue) {
        self.issues.push(issue);
    }

    /// Add multiple issues.
    pub fn add_issues(&mut self, issues: impl IntoIterator<Item = LintIssue>) {
        self.issues.extend(issues);
    }

    /// Merge another result into this one.
    pub fn merge(&mut self, other: LintResult) {
        self.issues.extend(other.issues);
        self.files_analyzed.extend(other.files_analyzed);
        self.plays_analyzed += other.plays_analyzed;
        self.tasks_analyzed += other.tasks_analyzed;
    }

    /// Get issues filtered by severity.
    pub fn issues_by_severity(&self, severity: Severity) -> Vec<&LintIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == severity)
            .collect()
    }

    /// Get issues filtered by category.
    pub fn issues_by_category(&self, category: RuleCategory) -> Vec<&LintIssue> {
        self.issues
            .iter()
            .filter(|i| i.category == category)
            .collect()
    }

    /// Check if there are any errors or critical issues.
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.is_error())
    }

    /// Get count of issues by severity.
    pub fn count_by_severity(&self) -> std::collections::HashMap<Severity, usize> {
        let mut counts = std::collections::HashMap::new();
        for issue in &self.issues {
            *counts.entry(issue.severity).or_insert(0) += 1;
        }
        counts
    }

    /// Get the exit code based on issues found.
    /// Returns 0 if no errors, 1 if there are warnings, 2 if there are errors.
    pub fn exit_code(&self) -> i32 {
        if self
            .issues
            .iter()
            .any(|i| matches!(i.severity, Severity::Critical))
        {
            3
        } else if self
            .issues
            .iter()
            .any(|i| matches!(i.severity, Severity::Error))
        {
            2
        } else if self
            .issues
            .iter()
            .any(|i| matches!(i.severity, Severity::Warning))
        {
            1
        } else {
            0
        }
    }

    /// Get a summary string.
    pub fn summary(&self) -> String {
        let counts = self.count_by_severity();
        let critical = counts.get(&Severity::Critical).copied().unwrap_or(0);
        let errors = counts.get(&Severity::Error).copied().unwrap_or(0);
        let warnings = counts.get(&Severity::Warning).copied().unwrap_or(0);
        let hints = counts.get(&Severity::Hint).copied().unwrap_or(0);

        format!(
            "Analyzed {} file(s), {} play(s), {} task(s): {} critical, {} error(s), {} warning(s), {} hint(s)",
            self.files_analyzed.len(),
            self.plays_analyzed,
            self.tasks_analyzed,
            critical,
            errors,
            warnings,
            hints
        )
    }
}

/// Error type for linter operations.
#[derive(Error, Debug)]
pub enum LintError {
    /// Error reading a file.
    #[error("Failed to read file '{path}': {message}")]
    FileRead { path: PathBuf, message: String },

    /// YAML parsing error.
    #[error("YAML parsing error in '{path}': {message}")]
    YamlParse {
        path: PathBuf,
        message: String,
        line: Option<usize>,
    },

    /// Invalid playbook structure.
    #[error("Invalid playbook structure: {0}")]
    InvalidStructure(String),

    /// Rule configuration error.
    #[error("Rule configuration error: {0}")]
    RuleConfig(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for linter operations.
pub type LintOpResult<T> = Result<T, LintError>;

/// Configuration for the linter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintConfig {
    /// Rules to skip (by rule ID).
    #[serde(default)]
    pub skip_rules: Vec<String>,
    /// Only run these rules (by rule ID). Empty means run all.
    #[serde(default)]
    pub only_rules: Vec<String>,
    /// Categories to skip.
    #[serde(default)]
    pub skip_categories: Vec<RuleCategory>,
    /// Minimum severity to report.
    #[serde(default)]
    pub min_severity: Severity,
    /// Whether to treat warnings as errors.
    #[serde(default)]
    pub warnings_as_errors: bool,
    /// Paths to exclude from linting.
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    /// Custom rule files to load.
    #[serde(default)]
    pub custom_rule_files: Vec<PathBuf>,
    /// Enable/disable specific rule categories.
    #[serde(default)]
    pub enabled_categories: Option<Vec<RuleCategory>>,
    /// Project-specific variable names to trust.
    #[serde(default)]
    pub trusted_variables: Vec<String>,
    /// Known module names for parameter validation.
    #[serde(default)]
    pub known_modules: Vec<String>,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            skip_rules: Vec::new(),
            only_rules: Vec::new(),
            skip_categories: Vec::new(),
            min_severity: Severity::Hint,
            warnings_as_errors: false,
            exclude_paths: Vec::new(),
            custom_rule_files: Vec::new(),
            enabled_categories: None,
            trusted_variables: Vec::new(),
            known_modules: Vec::new(),
        }
    }
}

impl LintConfig {
    /// Create a new default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a rule should be run.
    pub fn should_run_rule(
        &self,
        rule_id: &str,
        category: RuleCategory,
        severity: Severity,
    ) -> bool {
        // Check skip rules
        if self.skip_rules.contains(&rule_id.to_string()) {
            return false;
        }

        // Check only rules
        if !self.only_rules.is_empty() && !self.only_rules.contains(&rule_id.to_string()) {
            return false;
        }

        // Check skip categories
        if self.skip_categories.contains(&category) {
            return false;
        }

        // Check enabled categories
        if let Some(ref enabled) = self.enabled_categories {
            if !enabled.contains(&category) {
                return false;
            }
        }

        // Check minimum severity
        severity >= self.min_severity
    }

    /// Load configuration from a YAML file.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> LintOpResult<Self> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| LintError::FileRead {
            path: path.as_ref().to_path_buf(),
            message: e.to_string(),
        })?;

        serde_yaml::from_str(&content).map_err(|e| LintError::RuleConfig(e.to_string()))
    }

    /// Create a strict configuration that treats all issues as errors.
    pub fn strict() -> Self {
        Self {
            warnings_as_errors: true,
            min_severity: Severity::Hint,
            ..Default::default()
        }
    }

    /// Create a relaxed configuration for quick checks.
    pub fn relaxed() -> Self {
        Self {
            min_severity: Severity::Error,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical > Severity::Error);
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Hint);
    }

    #[test]
    fn test_location_formatting() {
        let loc = Location::file("/path/to/playbook.yml")
            .with_line(10)
            .with_column(5);
        assert_eq!(loc.to_location_string(), "/path/to/playbook.yml:10:5");
    }

    #[test]
    fn test_lint_result_summary() {
        let mut result = LintResult::new();
        result.files_analyzed.push(PathBuf::from("test.yml"));
        result.plays_analyzed = 1;
        result.tasks_analyzed = 5;

        result.add_issue(LintIssue::new(
            "E001",
            "test",
            Severity::Error,
            RuleCategory::Syntax,
            "Test error",
            Location::file("test.yml"),
        ));

        assert!(result.has_errors());
        assert_eq!(result.exit_code(), 2);
    }

    #[test]
    fn test_config_should_run_rule() {
        let config = LintConfig::new();
        assert!(config.should_run_rule("E001", RuleCategory::Syntax, Severity::Error));

        let mut config_skip = LintConfig::new();
        config_skip.skip_rules.push("E001".to_string());
        assert!(!config_skip.should_run_rule("E001", RuleCategory::Syntax, Severity::Error));
    }
}
