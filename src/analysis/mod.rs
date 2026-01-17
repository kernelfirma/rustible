//! Static Analysis Module
//!
//! This module provides comprehensive static analysis capabilities for Rustible playbooks,
//! including variable usage analysis, dead code detection, dependency analysis,
//! complexity metrics, and security static analysis.
//!
//! ## Features
//!
//! - **Variable Usage Analysis**: Track variable definitions, usage, and detect undefined/unused variables
//! - **Dead Code Detection**: Identify unused tasks, handlers, and unreachable code paths
//! - **Dependency Analysis**: Analyze task dependencies, role dependencies, and execution order
//! - **Complexity Metrics**: Calculate cyclomatic complexity, nesting depth, and maintainability index
//! - **Security Analysis**: Detect hardcoded secrets, unsafe patterns, and security vulnerabilities
//!
//! ## Usage Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::analysis::{StaticAnalyzer, AnalysisConfig};
//! # use rustible::playbook::Playbook;
//! # let playbook = Playbook::from_yaml(r#"- hosts: all
//! #   tasks:
//! #     - name: Ping
//! #       ping: {}
//! # "#, None)?;
//!
//! let analyzer = StaticAnalyzer::new(AnalysisConfig::default());
//! let report = analyzer.analyze(&playbook)?;
//!
//! println!("Issues found: {}", report.issue_count());
//! for issue in report.issues() {
//!     println!("{}: {}", issue.severity, issue.message);
//! }
//! # Ok(())
//! # }
//! ```

pub mod complexity;
pub mod dead_code;
pub mod dependencies;
pub mod report;
pub mod security;
pub mod variables;

use crate::playbook::{Play, Playbook, Task};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use thiserror::Error;

// Re-export main types
pub use complexity::{ComplexityAnalyzer, ComplexityMetrics, ComplexityReport};
pub use dead_code::{DeadCodeAnalyzer, DeadCodeFinding, DeadCodeType};
pub use dependencies::{DependencyAnalyzer, DependencyGraph, DependencyType};
pub use report::{AnalysisReport, AnalysisReportBuilder, ReportFormat};
pub use security::{SecurityAnalyzer, SecurityFinding, SecurityRule, VulnerabilityType};
pub use variables::{VariableAnalyzer, VariableScope, VariableUsage};

/// Errors that can occur during static analysis
#[derive(Error, Debug)]
pub enum AnalysisError {
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Invalid playbook structure: {0}")]
    InvalidStructure(String),

    #[error("Analysis failed: {0}")]
    AnalysisFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Result type for analysis operations
pub type AnalysisResult<T> = Result<T, AnalysisError>;

/// Severity level of an analysis finding
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Hint - minor suggestion for improvement
    Hint,
    /// Info - informational finding
    Info,
    /// Warning - potential issue that should be reviewed
    Warning,
    /// Error - definite issue that should be fixed
    Error,
    /// Critical - serious issue requiring immediate attention
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Hint => write!(f, "HINT"),
            Severity::Info => write!(f, "INFO"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Error => write!(f, "ERROR"),
            Severity::Critical => write!(f, "CRITICAL"),
        }
    }
}

impl Severity {
    /// Returns a numeric score for the severity (higher = more severe)
    pub fn score(&self) -> u32 {
        match self {
            Severity::Hint => 0,
            Severity::Info => 1,
            Severity::Warning => 2,
            Severity::Error => 3,
            Severity::Critical => 4,
        }
    }

    /// Returns the color code for terminal output
    pub fn color_code(&self) -> &'static str {
        match self {
            Severity::Hint => "\x1b[90m",     // Gray
            Severity::Info => "\x1b[36m",     // Cyan
            Severity::Warning => "\x1b[33m",  // Yellow
            Severity::Error => "\x1b[91m",    // Light Red
            Severity::Critical => "\x1b[31m", // Red
        }
    }
}

/// Category of an analysis finding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnalysisCategory {
    /// Variable-related issues
    Variable,
    /// Dead code detection
    DeadCode,
    /// Dependency-related issues
    Dependency,
    /// Complexity-related issues
    Complexity,
    /// Security-related issues
    Security,
    /// Best practices
    BestPractice,
    /// Performance-related issues
    Performance,
    /// Style and formatting issues
    Style,
}

impl fmt::Display for AnalysisCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnalysisCategory::Variable => write!(f, "Variable"),
            AnalysisCategory::DeadCode => write!(f, "Dead Code"),
            AnalysisCategory::Dependency => write!(f, "Dependency"),
            AnalysisCategory::Complexity => write!(f, "Complexity"),
            AnalysisCategory::Security => write!(f, "Security"),
            AnalysisCategory::BestPractice => write!(f, "Best Practice"),
            AnalysisCategory::Performance => write!(f, "Performance"),
            AnalysisCategory::Style => write!(f, "Style"),
        }
    }
}

/// Location of an issue in the playbook
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    /// File path (if available)
    pub file: Option<String>,
    /// Play index (0-based)
    pub play_index: Option<usize>,
    /// Play name
    pub play_name: Option<String>,
    /// Task index within play (0-based)
    pub task_index: Option<usize>,
    /// Task name
    pub task_name: Option<String>,
    /// Line number (if available)
    pub line: Option<usize>,
    /// Column number (if available)
    pub column: Option<usize>,
}

impl SourceLocation {
    pub fn new() -> Self {
        Self {
            file: None,
            play_index: None,
            play_name: None,
            task_index: None,
            task_name: None,
            line: None,
            column: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_play(mut self, index: usize, name: impl Into<String>) -> Self {
        self.play_index = Some(index);
        self.play_name = Some(name.into());
        self
    }

    pub fn with_task(mut self, index: usize, name: impl Into<String>) -> Self {
        self.task_index = Some(index);
        self.task_name = Some(name.into());
        self
    }

    pub fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }
}

impl Default for SourceLocation {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        if let Some(file) = &self.file {
            parts.push(file.clone());
        }

        if let Some(play_name) = &self.play_name {
            parts.push(format!("play '{}'", play_name));
        }

        if let Some(task_name) = &self.task_name {
            parts.push(format!("task '{}'", task_name));
        }

        if let Some(line) = self.line {
            if let Some(col) = self.column {
                parts.push(format!("line {}:{}", line, col));
            } else {
                parts.push(format!("line {}", line));
            }
        }

        write!(f, "{}", parts.join(", "))
    }
}

/// A single analysis finding/issue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisFinding {
    /// Unique identifier for this type of finding
    pub rule_id: String,
    /// Category of the finding
    pub category: AnalysisCategory,
    /// Severity level
    pub severity: Severity,
    /// Short message describing the issue
    pub message: String,
    /// Detailed description with more context
    pub description: String,
    /// Location in the source
    pub location: SourceLocation,
    /// Suggested fix (if available)
    pub suggestion: Option<String>,
    /// Related documentation URL
    pub documentation_url: Option<String>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl AnalysisFinding {
    pub fn new(
        rule_id: impl Into<String>,
        category: AnalysisCategory,
        severity: Severity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            rule_id: rule_id.into(),
            category,
            severity,
            message: message.into(),
            description: String::new(),
            location: SourceLocation::new(),
            suggestion: None,
            documentation_url: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_location(mut self, location: SourceLocation) -> Self {
        self.location = location;
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn with_documentation(mut self, url: impl Into<String>) -> Self {
        self.documentation_url = Some(url.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

impl fmt::Display for AnalysisFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} ({}): {}",
            self.severity, self.rule_id, self.category, self.message
        )?;
        if !self.location.file.is_none() || !self.location.play_name.is_none() {
            write!(f, " at {}", self.location)?;
        }
        Ok(())
    }
}

/// Configuration for the static analyzer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    /// Enable variable usage analysis
    pub analyze_variables: bool,
    /// Enable dead code detection
    pub analyze_dead_code: bool,
    /// Enable dependency analysis
    pub analyze_dependencies: bool,
    /// Enable complexity metrics
    pub analyze_complexity: bool,
    /// Enable security analysis
    pub analyze_security: bool,
    /// Minimum severity level to report
    pub min_severity: Severity,
    /// Maximum complexity threshold (triggers warning above this)
    pub max_complexity: u32,
    /// Maximum nesting depth threshold
    pub max_nesting_depth: u32,
    /// Security rules to enable (empty = all)
    pub enabled_security_rules: Vec<String>,
    /// Security rules to disable
    pub disabled_security_rules: Vec<String>,
    /// Custom patterns for secret detection
    pub secret_patterns: Vec<String>,
    /// Ignore patterns for files/paths
    pub ignore_patterns: Vec<String>,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            analyze_variables: true,
            analyze_dead_code: true,
            analyze_dependencies: true,
            analyze_complexity: true,
            analyze_security: true,
            min_severity: Severity::Hint,
            max_complexity: 10,
            max_nesting_depth: 4,
            enabled_security_rules: Vec::new(),
            disabled_security_rules: Vec::new(),
            secret_patterns: Vec::new(),
            ignore_patterns: Vec::new(),
        }
    }
}

impl AnalysisConfig {
    /// Create a strict configuration that reports all issues
    pub fn strict() -> Self {
        Self {
            min_severity: Severity::Hint,
            max_complexity: 5,
            max_nesting_depth: 3,
            ..Default::default()
        }
    }

    /// Create a relaxed configuration for legacy codebases
    pub fn relaxed() -> Self {
        Self {
            min_severity: Severity::Warning,
            max_complexity: 20,
            max_nesting_depth: 6,
            analyze_dead_code: false,
            ..Default::default()
        }
    }

    /// Create a security-focused configuration
    pub fn security_focused() -> Self {
        Self {
            analyze_variables: false,
            analyze_dead_code: false,
            analyze_dependencies: false,
            analyze_complexity: false,
            analyze_security: true,
            min_severity: Severity::Warning,
            ..Default::default()
        }
    }
}

/// Main static analyzer that coordinates all analysis components
pub struct StaticAnalyzer {
    config: AnalysisConfig,
    variable_analyzer: VariableAnalyzer,
    dead_code_analyzer: DeadCodeAnalyzer,
    dependency_analyzer: DependencyAnalyzer,
    complexity_analyzer: ComplexityAnalyzer,
    security_analyzer: SecurityAnalyzer,
}

impl StaticAnalyzer {
    /// Create a new static analyzer with the given configuration
    pub fn new(config: AnalysisConfig) -> Self {
        Self {
            variable_analyzer: VariableAnalyzer::new(),
            dead_code_analyzer: DeadCodeAnalyzer::new(),
            dependency_analyzer: DependencyAnalyzer::new(),
            complexity_analyzer: ComplexityAnalyzer::new(config.max_complexity, config.max_nesting_depth),
            security_analyzer: SecurityAnalyzer::new(),
            config,
        }
    }

    /// Create a new analyzer with default configuration
    pub fn with_defaults() -> Self {
        Self::new(AnalysisConfig::default())
    }

    /// Analyze a playbook and return a comprehensive report
    pub fn analyze(&self, playbook: &Playbook) -> AnalysisResult<AnalysisReport> {
        let mut builder = AnalysisReportBuilder::new();

        // Set source file if available
        if let Some(path) = &playbook.source_path {
            builder = builder.with_source(path.to_string_lossy().to_string());
        }

        // Variable usage analysis
        if self.config.analyze_variables {
            let findings = self.variable_analyzer.analyze(playbook)?;
            for finding in findings {
                if finding.severity >= self.config.min_severity {
                    builder = builder.add_finding(finding);
                }
            }
        }

        // Dead code detection
        if self.config.analyze_dead_code {
            let findings = self.dead_code_analyzer.analyze(playbook)?;
            for finding in findings {
                if finding.severity >= self.config.min_severity {
                    builder = builder.add_finding(finding);
                }
            }
        }

        // Dependency analysis
        if self.config.analyze_dependencies {
            let (graph, findings) = self.dependency_analyzer.analyze(playbook)?;
            builder = builder.with_dependency_graph(graph);
            for finding in findings {
                if finding.severity >= self.config.min_severity {
                    builder = builder.add_finding(finding);
                }
            }
        }

        // Complexity analysis
        if self.config.analyze_complexity {
            let (metrics, findings) = self.complexity_analyzer.analyze(playbook)?;
            builder = builder.with_complexity_metrics(metrics);
            for finding in findings {
                if finding.severity >= self.config.min_severity {
                    builder = builder.add_finding(finding);
                }
            }
        }

        // Security analysis
        if self.config.analyze_security {
            let findings = self.security_analyzer.analyze(playbook, &self.config)?;
            for finding in findings {
                if finding.severity >= self.config.min_severity {
                    builder = builder.add_finding(finding);
                }
            }
        }

        Ok(builder.build())
    }

    /// Analyze multiple playbooks and aggregate results
    pub fn analyze_all(&self, playbooks: &[Playbook]) -> AnalysisResult<AnalysisReport> {
        let mut builder = AnalysisReportBuilder::new();

        for playbook in playbooks {
            let report = self.analyze(playbook)?;
            for finding in report.findings {
                builder = builder.add_finding(finding);
            }
        }

        Ok(builder.build())
    }

    /// Quick check for security issues only
    pub fn security_check(&self, playbook: &Playbook) -> AnalysisResult<Vec<AnalysisFinding>> {
        self.security_analyzer.analyze(playbook, &self.config)
    }

    /// Get complexity metrics for a playbook
    pub fn get_complexity(&self, playbook: &Playbook) -> AnalysisResult<ComplexityMetrics> {
        let (metrics, _) = self.complexity_analyzer.analyze(playbook)?;
        Ok(metrics)
    }

    /// Get dependency graph for a playbook
    pub fn get_dependencies(&self, playbook: &Playbook) -> AnalysisResult<DependencyGraph> {
        let (graph, _) = self.dependency_analyzer.analyze(playbook)?;
        Ok(graph)
    }
}

/// Helper functions for extracting variables from task/play structures
pub(crate) mod helpers {
    use super::*;
    use regex::Regex;
    use std::collections::HashSet;

    lazy_static::lazy_static! {
        /// Regex to match Jinja2 variable references: {{ var_name }}
        static ref JINJA_VAR_PATTERN: Regex = Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)*)\s*[}|]").unwrap();

        /// Regex to match Jinja2 variable references with filters: {{ var | filter }}
        static ref JINJA_VAR_FILTER_PATTERN: Regex = Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)*)\s*\|").unwrap();

        /// Regex to match when condition variables
        static ref WHEN_VAR_PATTERN: Regex = Regex::new(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\b").unwrap();
    }

    /// Extract variable names from a Jinja2 template string
    pub fn extract_jinja_variables(text: &str) -> HashSet<String> {
        let mut vars = HashSet::new();

        for cap in JINJA_VAR_PATTERN.captures_iter(text) {
            if let Some(var) = cap.get(1) {
                // Get the root variable name (before any dots)
                let var_name = var.as_str().split('.').next().unwrap_or(var.as_str());
                vars.insert(var_name.to_string());
            }
        }

        for cap in JINJA_VAR_FILTER_PATTERN.captures_iter(text) {
            if let Some(var) = cap.get(1) {
                let var_name = var.as_str().split('.').next().unwrap_or(var.as_str());
                vars.insert(var_name.to_string());
            }
        }

        vars
    }

    /// Extract variable names from a when condition
    pub fn extract_when_variables(condition: &str) -> HashSet<String> {
        let mut vars = HashSet::new();

        // Skip keywords and built-in functions
        let keywords: HashSet<&str> = [
            "and", "or", "not", "in", "is", "true", "false", "none", "null",
            "defined", "undefined", "succeeded", "failed", "skipped", "changed",
            "if", "else", "elif", "for", "endfor", "endif", "match", "search",
            "length", "lower", "upper", "title", "int", "float", "string",
            "bool", "list", "dict", "set", "range", "default", "first", "last",
            "ansible_facts", "hostvars", "groups", "group_names", "inventory_hostname",
        ].into_iter().collect();

        for cap in WHEN_VAR_PATTERN.captures_iter(condition) {
            if let Some(var) = cap.get(1) {
                let var_name = var.as_str();
                // Skip if it looks like a number or keyword
                if !var_name.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                    && !keywords.contains(var_name)
                {
                    vars.insert(var_name.to_string());
                }
            }
        }

        vars
    }

    /// Extract variables from a serde_json::Value recursively
    pub fn extract_value_variables(value: &serde_json::Value) -> HashSet<String> {
        let mut vars = HashSet::new();

        match value {
            serde_json::Value::String(s) => {
                vars.extend(extract_jinja_variables(s));
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    vars.extend(extract_value_variables(item));
                }
            }
            serde_json::Value::Object(obj) => {
                for (_, v) in obj {
                    vars.extend(extract_value_variables(v));
                }
            }
            _ => {}
        }

        vars
    }

    /// Get all tasks from a play (pre_tasks, tasks, post_tasks)
    pub fn get_all_tasks(play: &Play) -> Vec<&Task> {
        play.pre_tasks
            .iter()
            .chain(play.tasks.iter())
            .chain(play.post_tasks.iter())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical > Severity::Error);
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
        assert!(Severity::Info > Severity::Hint);
    }

    #[test]
    fn test_extract_jinja_variables() {
        let text = "Hello {{ name }}, your balance is {{ account.balance | default(0) }}";
        let vars = helpers::extract_jinja_variables(text);
        assert!(vars.contains("name"));
        assert!(vars.contains("account"));
    }

    #[test]
    fn test_extract_when_variables() {
        let condition = "ansible_os_family == 'Debian' and my_var is defined";
        let vars = helpers::extract_when_variables(condition);
        assert!(vars.contains("ansible_os_family"));
        assert!(vars.contains("my_var"));
        assert!(!vars.contains("and")); // keywords excluded
        assert!(!vars.contains("is"));
        assert!(!vars.contains("defined"));
    }

    #[test]
    fn test_analysis_finding_display() {
        let finding = AnalysisFinding::new(
            "VAR001",
            AnalysisCategory::Variable,
            Severity::Warning,
            "Undefined variable 'foo'",
        )
        .with_location(
            SourceLocation::new()
                .with_file("playbook.yml")
                .with_play(0, "Test Play"),
        );

        let display = format!("{}", finding);
        assert!(display.contains("VAR001"));
        assert!(display.contains("WARNING"));
    }

    #[test]
    fn test_config_presets() {
        let strict = AnalysisConfig::strict();
        assert_eq!(strict.min_severity, Severity::Hint);
        assert_eq!(strict.max_complexity, 5);

        let relaxed = AnalysisConfig::relaxed();
        assert_eq!(relaxed.min_severity, Severity::Warning);
        assert!(!relaxed.analyze_dead_code);

        let security = AnalysisConfig::security_focused();
        assert!(security.analyze_security);
        assert!(!security.analyze_variables);
    }
}
