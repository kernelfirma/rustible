//! Compliance Scanning Module
//!
//! This module provides comprehensive security compliance scanning capabilities
//! for target systems, supporting multiple compliance frameworks:
//!
//! - **CIS Benchmarks**: Center for Internet Security configuration guidelines
//! - **STIG**: Security Technical Implementation Guides (DoD standards)
//! - **PCI-DSS**: Payment Card Industry Data Security Standard
//!
//! ## Architecture
//!
//! The compliance module is organized around these key concepts:
//!
//! - `ComplianceCheck`: Individual security checks with pass/fail/warning results
//! - `ComplianceScanner`: Trait for implementing framework-specific scanners
//! - `ComplianceReport`: Aggregated results with severity levels and recommendations
//!
//! ## Usage Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::compliance::{ComplianceContext, ComplianceScanner, CisScanner};
//! # let connection = std::sync::Arc::new(rustible::connection::local::LocalConnection::new());
//!
//! let scanner = CisScanner::new();
//! let context = ComplianceContext::new(connection);
//! let findings = scanner.scan(&context).await?;
//!
//! println!("Findings: {}", findings.len());
//! for finding in &findings {
//!     println!("{}: {}", finding.severity, finding.description);
//! }
//! # Ok(())
//! # }
//! ```

pub mod checks;
pub mod cis;
pub mod pci_dss;
pub mod report;
pub mod stig;

use crate::connection::Connection;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use thiserror::Error;

// Re-export main types
pub use checks::{CheckCategory, CheckResult, ComplianceCheck, FileCheck, ServiceCheck, SysctlCheck};
pub use cis::CisScanner;
pub use pci_dss::PciDssScanner;
pub use report::{ComplianceReport, ComplianceReportBuilder, ReportFormat};
pub use stig::StigScanner;

/// Errors that can occur during compliance scanning
#[derive(Error, Debug)]
pub enum ComplianceError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Check execution failed: {0}")]
    CheckFailed(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Report generation failed: {0}")]
    ReportGeneration(String),
}

/// Result type for compliance operations
pub type ComplianceResult<T> = Result<T, ComplianceError>;

/// Severity level of a compliance finding
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational finding, no action required
    Info,
    /// Low severity, minor configuration improvement
    Low,
    /// Medium severity, should be addressed
    Medium,
    /// High severity, significant security risk
    High,
    /// Critical severity, immediate action required
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Low => write!(f, "LOW"),
            Severity::Medium => write!(f, "MEDIUM"),
            Severity::High => write!(f, "HIGH"),
            Severity::Critical => write!(f, "CRITICAL"),
        }
    }
}

impl Severity {
    /// Returns a numeric score for the severity (higher = more severe)
    pub fn score(&self) -> u32 {
        match self {
            Severity::Info => 0,
            Severity::Low => 1,
            Severity::Medium => 2,
            Severity::High => 3,
            Severity::Critical => 4,
        }
    }

    /// Returns the color code for terminal output
    pub fn color_code(&self) -> &'static str {
        match self {
            Severity::Info => "\x1b[36m",    // Cyan
            Severity::Low => "\x1b[32m",     // Green
            Severity::Medium => "\x1b[33m",  // Yellow
            Severity::High => "\x1b[91m",    // Light Red
            Severity::Critical => "\x1b[31m", // Red
        }
    }
}

/// Result status of a compliance check
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Check passed, system is compliant
    Pass,
    /// Check failed, system is non-compliant
    Fail,
    /// Check produced a warning, manual review recommended
    Warning,
    /// Check was skipped (not applicable or missing prerequisites)
    Skipped,
    /// Check encountered an error during execution
    Error,
    /// Check result is unknown (could not determine)
    Unknown,
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "PASS"),
            CheckStatus::Fail => write!(f, "FAIL"),
            CheckStatus::Warning => write!(f, "WARN"),
            CheckStatus::Skipped => write!(f, "SKIP"),
            CheckStatus::Error => write!(f, "ERROR"),
            CheckStatus::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

impl CheckStatus {
    /// Returns the color code for terminal output
    pub fn color_code(&self) -> &'static str {
        match self {
            CheckStatus::Pass => "\x1b[32m",    // Green
            CheckStatus::Fail => "\x1b[31m",    // Red
            CheckStatus::Warning => "\x1b[33m", // Yellow
            CheckStatus::Skipped => "\x1b[90m", // Gray
            CheckStatus::Error => "\x1b[91m",   // Light Red
            CheckStatus::Unknown => "\x1b[35m", // Magenta
        }
    }
}

/// Compliance framework identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComplianceFramework {
    /// CIS Benchmarks
    Cis,
    /// DISA STIG
    Stig,
    /// PCI-DSS
    PciDss,
    /// Custom framework
    Custom,
}

impl fmt::Display for ComplianceFramework {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComplianceFramework::Cis => write!(f, "CIS"),
            ComplianceFramework::Stig => write!(f, "STIG"),
            ComplianceFramework::PciDss => write!(f, "PCI-DSS"),
            ComplianceFramework::Custom => write!(f, "Custom"),
        }
    }
}

/// A single compliance finding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Unique identifier for this check
    pub check_id: String,
    /// Human-readable title
    pub title: String,
    /// Detailed description of the check
    pub description: String,
    /// Compliance framework this check belongs to
    pub framework: ComplianceFramework,
    /// Severity level
    pub severity: Severity,
    /// Check status (pass/fail/etc)
    pub status: CheckStatus,
    /// Current observed value (if applicable)
    pub observed: Option<String>,
    /// Expected value for compliance (if applicable)
    pub expected: Option<String>,
    /// Remediation steps
    pub remediation: String,
    /// Additional references (URLs, documentation)
    #[serde(default)]
    pub references: Vec<String>,
    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Finding {
    /// Create a new finding
    pub fn new(
        check_id: impl Into<String>,
        title: impl Into<String>,
        framework: ComplianceFramework,
    ) -> Self {
        Self {
            check_id: check_id.into(),
            title: title.into(),
            description: String::new(),
            framework,
            severity: Severity::Medium,
            status: CheckStatus::Unknown,
            observed: None,
            expected: None,
            remediation: String::new(),
            references: Vec::new(),
            tags: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Builder pattern methods
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_status(mut self, status: CheckStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_observed(mut self, observed: impl Into<String>) -> Self {
        self.observed = Some(observed.into());
        self
    }

    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = remediation.into();
        self
    }

    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.references.push(reference.into());
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Returns true if this finding represents a failed check
    pub fn is_failure(&self) -> bool {
        matches!(self.status, CheckStatus::Fail | CheckStatus::Error)
    }

    /// Returns true if this finding requires attention
    pub fn needs_attention(&self) -> bool {
        matches!(
            self.status,
            CheckStatus::Fail | CheckStatus::Warning | CheckStatus::Error
        )
    }
}

/// Context for compliance scanning operations
pub struct ComplianceContext {
    /// Connection to the target system
    pub connection: Arc<dyn Connection + Send + Sync>,
    /// Target system facts (OS, version, etc.)
    pub facts: HashMap<String, serde_json::Value>,
    /// Check mode - don't make any changes
    pub check_mode: bool,
    /// Verbose output
    pub verbose: bool,
    /// Tags to filter checks
    pub include_tags: Vec<String>,
    /// Tags to exclude
    pub exclude_tags: Vec<String>,
    /// Severity threshold - only report findings at or above this level
    pub severity_threshold: Severity,
    /// Custom variables for check execution
    pub variables: HashMap<String, String>,
}

impl ComplianceContext {
    /// Create a new compliance context
    pub fn new(connection: Arc<dyn Connection + Send + Sync>) -> Self {
        Self {
            connection,
            facts: HashMap::new(),
            check_mode: false,
            verbose: false,
            include_tags: Vec::new(),
            exclude_tags: Vec::new(),
            severity_threshold: Severity::Info,
            variables: HashMap::new(),
        }
    }

    /// Builder pattern methods
    pub fn with_facts(mut self, facts: HashMap<String, serde_json::Value>) -> Self {
        self.facts = facts;
        self
    }

    pub fn with_check_mode(mut self, check_mode: bool) -> Self {
        self.check_mode = check_mode;
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn with_severity_threshold(mut self, threshold: Severity) -> Self {
        self.severity_threshold = threshold;
        self
    }

    pub fn with_include_tags(mut self, tags: Vec<String>) -> Self {
        self.include_tags = tags;
        self
    }

    pub fn with_exclude_tags(mut self, tags: Vec<String>) -> Self {
        self.exclude_tags = tags;
        self
    }

    pub fn with_variable(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.variables.insert(key.into(), value.into());
        self
    }

    /// Get the OS family from facts
    pub fn os_family(&self) -> Option<&str> {
        self.facts
            .get("os_family")
            .and_then(|v| v.as_str())
            .or_else(|| {
                self.facts
                    .get("ansible_os_family")
                    .and_then(|v| v.as_str())
            })
    }

    /// Get the distribution name from facts
    pub fn distribution(&self) -> Option<&str> {
        self.facts
            .get("distribution")
            .and_then(|v| v.as_str())
            .or_else(|| {
                self.facts
                    .get("ansible_distribution")
                    .and_then(|v| v.as_str())
            })
    }

    /// Check if a tag should be included
    pub fn should_include_tag(&self, tags: &[String]) -> bool {
        // If no include tags specified, include all
        if self.include_tags.is_empty() {
            // But still check exclude tags
            !tags.iter().any(|t| self.exclude_tags.contains(t))
        } else {
            // Must have at least one include tag and no exclude tags
            tags.iter().any(|t| self.include_tags.contains(t))
                && !tags.iter().any(|t| self.exclude_tags.contains(t))
        }
    }
}

/// Trait for implementing compliance scanners
#[async_trait]
pub trait ComplianceScanner: Send + Sync {
    /// Returns the framework this scanner implements
    fn framework(&self) -> ComplianceFramework;

    /// Returns the name of this scanner
    fn name(&self) -> &str;

    /// Returns a description of this scanner
    fn description(&self) -> &str;

    /// Returns the version of the compliance standard implemented
    fn version(&self) -> &str;

    /// Run all compliance checks
    async fn scan(&self, context: &ComplianceContext) -> ComplianceResult<Vec<Finding>>;

    /// Run a specific check by ID
    async fn run_check(
        &self,
        check_id: &str,
        context: &ComplianceContext,
    ) -> ComplianceResult<Finding>;

    /// List all available checks
    fn list_checks(&self) -> Vec<&str>;

    /// Get information about a specific check
    fn get_check_info(&self, check_id: &str) -> Option<CheckInfo>;
}

/// Information about a compliance check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckInfo {
    /// Unique check identifier
    pub id: String,
    /// Human-readable title
    pub title: String,
    /// Detailed description
    pub description: String,
    /// Severity level
    pub severity: Severity,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Whether this check can be auto-remediated
    pub auto_remediable: bool,
    /// Estimated time to remediate (in minutes)
    pub remediation_time_minutes: Option<u32>,
}

/// Aggregate compliance statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComplianceStats {
    /// Total number of checks executed
    pub total_checks: u32,
    /// Number of passed checks
    pub passed: u32,
    /// Number of failed checks
    pub failed: u32,
    /// Number of warnings
    pub warnings: u32,
    /// Number of skipped checks
    pub skipped: u32,
    /// Number of errors
    pub errors: u32,
    /// Findings by severity
    pub by_severity: HashMap<String, u32>,
    /// Findings by category/tag
    pub by_category: HashMap<String, u32>,
}

impl ComplianceStats {
    /// Calculate compliance percentage
    pub fn compliance_percentage(&self) -> f64 {
        let applicable = self.total_checks - self.skipped - self.errors;
        if applicable == 0 {
            100.0
        } else {
            (self.passed as f64 / applicable as f64) * 100.0
        }
    }

    /// Get a letter grade based on compliance percentage
    pub fn grade(&self) -> &'static str {
        let pct = self.compliance_percentage();
        if pct >= 95.0 {
            "A+"
        } else if pct >= 90.0 {
            "A"
        } else if pct >= 85.0 {
            "B+"
        } else if pct >= 80.0 {
            "B"
        } else if pct >= 75.0 {
            "C+"
        } else if pct >= 70.0 {
            "C"
        } else if pct >= 65.0 {
            "D+"
        } else if pct >= 60.0 {
            "D"
        } else {
            "F"
        }
    }

    /// Update stats from a finding
    pub fn record_finding(&mut self, finding: &Finding) {
        self.total_checks += 1;

        match finding.status {
            CheckStatus::Pass => self.passed += 1,
            CheckStatus::Fail => self.failed += 1,
            CheckStatus::Warning => self.warnings += 1,
            CheckStatus::Skipped => self.skipped += 1,
            CheckStatus::Error => self.errors += 1,
            CheckStatus::Unknown => {}
        }

        // Track by severity
        let sev_key = format!("{}", finding.severity);
        *self.by_severity.entry(sev_key).or_insert(0) += 1;

        // Track by tags/categories
        for tag in &finding.tags {
            *self.by_category.entry(tag.clone()).or_insert(0) += 1;
        }
    }
}

/// Scanner registry for managing multiple compliance scanners
pub struct ScannerRegistry {
    scanners: HashMap<ComplianceFramework, Arc<dyn ComplianceScanner>>,
}

impl ScannerRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            scanners: HashMap::new(),
        }
    }

    /// Create a registry with all built-in scanners
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(CisScanner::new()));
        registry.register(Arc::new(StigScanner::new()));
        registry.register(Arc::new(PciDssScanner::new()));
        registry
    }

    /// Register a scanner
    pub fn register(&mut self, scanner: Arc<dyn ComplianceScanner>) {
        self.scanners.insert(scanner.framework(), scanner);
    }

    /// Get a scanner by framework
    pub fn get(&self, framework: ComplianceFramework) -> Option<Arc<dyn ComplianceScanner>> {
        self.scanners.get(&framework).cloned()
    }

    /// Run all registered scanners
    pub async fn scan_all(&self, context: &ComplianceContext) -> ComplianceResult<ComplianceReport> {
        let mut builder = ComplianceReportBuilder::new();

        for (framework, scanner) in &self.scanners {
            let findings = scanner.scan(context).await?;
            builder = builder.with_framework_findings(*framework, findings);
        }

        Ok(builder.build())
    }

    /// List all registered frameworks
    pub fn frameworks(&self) -> Vec<ComplianceFramework> {
        self.scanners.keys().copied().collect()
    }
}

impl Default for ScannerRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        assert!(Severity::Medium > Severity::Low);
        assert!(Severity::Low > Severity::Info);
    }

    #[test]
    fn test_severity_score() {
        assert_eq!(Severity::Info.score(), 0);
        assert_eq!(Severity::Critical.score(), 4);
    }

    #[test]
    fn test_finding_builder() {
        let finding = Finding::new("CIS-1.1", "Test Check", ComplianceFramework::Cis)
            .with_description("Test description")
            .with_severity(Severity::High)
            .with_status(CheckStatus::Fail)
            .with_observed("current value")
            .with_expected("expected value")
            .with_remediation("Fix by doing X");

        assert_eq!(finding.check_id, "CIS-1.1");
        assert_eq!(finding.severity, Severity::High);
        assert!(finding.is_failure());
        assert!(finding.needs_attention());
    }

    #[test]
    fn test_compliance_stats() {
        let mut stats = ComplianceStats::default();

        let pass = Finding::new("1", "Pass", ComplianceFramework::Cis)
            .with_status(CheckStatus::Pass)
            .with_severity(Severity::Medium);
        let fail = Finding::new("2", "Fail", ComplianceFramework::Cis)
            .with_status(CheckStatus::Fail)
            .with_severity(Severity::High);

        stats.record_finding(&pass);
        stats.record_finding(&fail);

        assert_eq!(stats.total_checks, 2);
        assert_eq!(stats.passed, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.compliance_percentage(), 50.0);
        assert_eq!(stats.grade(), "F");
    }

    #[test]
    fn test_grade_calculation() {
        let mut stats = ComplianceStats::default();
        stats.total_checks = 100;
        stats.passed = 95;
        stats.failed = 5;
        assert_eq!(stats.grade(), "A+");

        stats.passed = 75;
        stats.failed = 25;
        assert_eq!(stats.grade(), "C+");
    }
}
