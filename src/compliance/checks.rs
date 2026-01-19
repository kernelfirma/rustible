//! Common compliance check utilities and implementations
//!
//! This module provides reusable check implementations and utilities
//! that can be used across different compliance frameworks.

use super::{CheckStatus, ComplianceContext, ComplianceError, ComplianceResult, Severity};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Category for grouping related checks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckCategory {
    /// Filesystem and mount configuration
    Filesystem,
    /// User and group management
    UserAccounts,
    /// Service and daemon configuration
    Services,
    /// Network configuration
    Network,
    /// Audit and logging
    Auditing,
    /// Authentication and authorization
    Authentication,
    /// System access and permissions
    AccessControl,
    /// Kernel and sysctl parameters
    Kernel,
    /// SSH configuration
    Ssh,
    /// Cryptographic settings
    Cryptography,
    /// Patch and update management
    Patching,
    /// Process and resource limits
    ResourceLimits,
    /// Time synchronization
    TimeSync,
    /// Banner and warning messages
    Banners,
    /// Maintenance and housekeeping
    Maintenance,
}

impl std::fmt::Display for CheckCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckCategory::Filesystem => write!(f, "Filesystem"),
            CheckCategory::UserAccounts => write!(f, "User Accounts"),
            CheckCategory::Services => write!(f, "Services"),
            CheckCategory::Network => write!(f, "Network"),
            CheckCategory::Auditing => write!(f, "Auditing"),
            CheckCategory::Authentication => write!(f, "Authentication"),
            CheckCategory::AccessControl => write!(f, "Access Control"),
            CheckCategory::Kernel => write!(f, "Kernel"),
            CheckCategory::Ssh => write!(f, "SSH"),
            CheckCategory::Cryptography => write!(f, "Cryptography"),
            CheckCategory::Patching => write!(f, "Patching"),
            CheckCategory::ResourceLimits => write!(f, "Resource Limits"),
            CheckCategory::TimeSync => write!(f, "Time Synchronization"),
            CheckCategory::Banners => write!(f, "Banners"),
            CheckCategory::Maintenance => write!(f, "Maintenance"),
        }
    }
}

/// Result of a single check execution
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Status of the check
    pub status: CheckStatus,
    /// Observed value on the system
    pub observed: Option<String>,
    /// Additional details
    pub details: Option<String>,
}

impl CheckResult {
    pub fn pass() -> Self {
        Self {
            status: CheckStatus::Pass,
            observed: None,
            details: None,
        }
    }

    pub fn fail() -> Self {
        Self {
            status: CheckStatus::Fail,
            observed: None,
            details: None,
        }
    }

    pub fn warning() -> Self {
        Self {
            status: CheckStatus::Warning,
            observed: None,
            details: None,
        }
    }

    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Skipped,
            observed: None,
            details: Some(reason.into()),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: CheckStatus::Error,
            observed: None,
            details: Some(message.into()),
        }
    }

    pub fn with_observed(mut self, value: impl Into<String>) -> Self {
        self.observed = Some(value.into());
        self
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// Trait for individual compliance checks
#[async_trait::async_trait]
pub trait ComplianceCheck: Send + Sync {
    /// Returns the unique identifier for this check
    fn id(&self) -> &str;

    /// Returns the human-readable title
    fn title(&self) -> &str;

    /// Returns the check description
    fn description(&self) -> &str;

    /// Returns the severity level
    fn severity(&self) -> Severity;

    /// Returns the category
    fn category(&self) -> CheckCategory;

    /// Returns tags for filtering
    fn tags(&self) -> Vec<String>;

    /// Returns remediation instructions
    fn remediation(&self) -> &str;

    /// Returns documentation references
    fn references(&self) -> Vec<String> {
        Vec::new()
    }

    /// Execute the check
    async fn execute(&self, context: &ComplianceContext) -> ComplianceResult<CheckResult>;
}

/// Check for file existence and properties
pub struct FileCheck {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub category: CheckCategory,
    pub path: String,
    pub should_exist: bool,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>,
    pub content_pattern: Option<String>,
    pub remediation: String,
    pub tags: Vec<String>,
}

impl FileCheck {
    pub fn new(id: impl Into<String>, title: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            severity: Severity::Medium,
            category: CheckCategory::Filesystem,
            path: path.into(),
            should_exist: true,
            owner: None,
            group: None,
            mode: None,
            content_pattern: None,
            remediation: String::new(),
            tags: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_category(mut self, category: CheckCategory) -> Self {
        self.category = category;
        self
    }

    pub fn should_not_exist(mut self) -> Self {
        self.should_exist = false;
        self
    }

    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = Some(mode.into());
        self
    }

    pub fn with_content_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.content_pattern = Some(pattern.into());
        self
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = remediation.into();
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }
}

#[async_trait::async_trait]
impl ComplianceCheck for FileCheck {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn category(&self) -> CheckCategory {
        self.category
    }

    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }

    fn remediation(&self) -> &str {
        &self.remediation
    }

    async fn execute(&self, context: &ComplianceContext) -> ComplianceResult<CheckResult> {
        let path = Path::new(&self.path);
        let exists = context
            .connection
            .path_exists(path)
            .await
            .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

        if !self.should_exist {
            return if exists {
                Ok(CheckResult::fail().with_observed(format!("File {} exists", self.path)))
            } else {
                Ok(CheckResult::pass().with_observed(format!("File {} does not exist", self.path)))
            };
        }

        if !exists {
            return Ok(
                CheckResult::fail().with_observed(format!("File {} does not exist", self.path))
            );
        }

        // Check file stats
        let stat_cmd = format!("stat -c '%U %G %a' '{}'", self.path);
        let result = context
            .connection
            .execute(&stat_cmd, None)
            .await
            .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

        if !result.success {
            return Ok(CheckResult::error(format!(
                "Failed to stat file: {}",
                result.stderr
            )));
        }

        let parts: Vec<&str> = result.stdout.split_whitespace().collect();
        if parts.len() < 3 {
            return Ok(CheckResult::error("Unexpected stat output format"));
        }

        let (file_owner, file_group, file_mode) = (parts[0], parts[1], parts[2]);
        let mut issues = Vec::new();

        if let Some(ref expected_owner) = self.owner {
            if file_owner != expected_owner {
                issues.push(format!(
                    "owner is {} (expected {})",
                    file_owner, expected_owner
                ));
            }
        }

        if let Some(ref expected_group) = self.group {
            if file_group != expected_group {
                issues.push(format!(
                    "group is {} (expected {})",
                    file_group, expected_group
                ));
            }
        }

        if let Some(ref expected_mode) = self.mode {
            // Normalize mode comparison (handle leading zeros)
            let expected_mode_normalized = expected_mode.trim_start_matches('0');
            let file_mode_normalized = file_mode.trim_start_matches('0');
            if file_mode_normalized != expected_mode_normalized {
                issues.push(format!(
                    "mode is {} (expected {})",
                    file_mode, expected_mode
                ));
            }
        }

        // Check content pattern if specified
        if let Some(ref pattern) = self.content_pattern {
            let cat_cmd = format!("cat '{}'", self.path);
            let content_result = context
                .connection
                .execute(&cat_cmd, None)
                .await
                .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

            if content_result.success {
                let re = Regex::new(pattern).map_err(|e| {
                    ComplianceError::InvalidConfig(format!("Invalid regex pattern: {}", e))
                })?;
                if !re.is_match(&content_result.stdout) {
                    issues.push(format!("content does not match pattern: {}", pattern));
                }
            }
        }

        if issues.is_empty() {
            Ok(CheckResult::pass().with_observed(format!(
                "{}: owner={}, group={}, mode={}",
                self.path, file_owner, file_group, file_mode
            )))
        } else {
            Ok(CheckResult::fail()
                .with_observed(format!(
                    "{}: owner={}, group={}, mode={}",
                    self.path, file_owner, file_group, file_mode
                ))
                .with_details(issues.join("; ")))
        }
    }
}

/// Check for sysctl kernel parameter values
pub struct SysctlCheck {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub parameter: String,
    pub expected: String,
    pub comparison: Comparison,
    pub remediation: String,
    pub tags: Vec<String>,
}

/// Comparison operation for numeric checks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterOrEqual,
    LessThan,
    LessOrEqual,
}

impl SysctlCheck {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        parameter: impl Into<String>,
        expected: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            severity: Severity::Medium,
            parameter: parameter.into(),
            expected: expected.into(),
            comparison: Comparison::Equal,
            remediation: String::new(),
            tags: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_comparison(mut self, comparison: Comparison) -> Self {
        self.comparison = comparison;
        self
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = remediation.into();
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    fn compare(&self, actual: &str) -> bool {
        match self.comparison {
            Comparison::Equal => actual.trim() == self.expected.trim(),
            Comparison::NotEqual => actual.trim() != self.expected.trim(),
            Comparison::GreaterThan => {
                actual
                    .trim()
                    .parse::<i64>()
                    .ok()
                    .zip(self.expected.trim().parse::<i64>().ok())
                    .map(|(a, e)| a > e)
                    .unwrap_or(false)
            }
            Comparison::GreaterOrEqual => {
                actual
                    .trim()
                    .parse::<i64>()
                    .ok()
                    .zip(self.expected.trim().parse::<i64>().ok())
                    .map(|(a, e)| a >= e)
                    .unwrap_or(false)
            }
            Comparison::LessThan => {
                actual
                    .trim()
                    .parse::<i64>()
                    .ok()
                    .zip(self.expected.trim().parse::<i64>().ok())
                    .map(|(a, e)| a < e)
                    .unwrap_or(false)
            }
            Comparison::LessOrEqual => {
                actual
                    .trim()
                    .parse::<i64>()
                    .ok()
                    .zip(self.expected.trim().parse::<i64>().ok())
                    .map(|(a, e)| a <= e)
                    .unwrap_or(false)
            }
        }
    }
}

#[async_trait::async_trait]
impl ComplianceCheck for SysctlCheck {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn category(&self) -> CheckCategory {
        CheckCategory::Kernel
    }

    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }

    fn remediation(&self) -> &str {
        &self.remediation
    }

    async fn execute(&self, context: &ComplianceContext) -> ComplianceResult<CheckResult> {
        let cmd = format!("sysctl -n {}", self.parameter);
        let result = context
            .connection
            .execute(&cmd, None)
            .await
            .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

        if !result.success {
            return Ok(CheckResult::error(format!(
                "Failed to read sysctl parameter: {}",
                result.stderr
            )));
        }

        let actual = result.stdout.trim();
        if self.compare(actual) {
            Ok(CheckResult::pass().with_observed(format!("{} = {}", self.parameter, actual)))
        } else {
            Ok(CheckResult::fail()
                .with_observed(format!("{} = {}", self.parameter, actual))
                .with_details(format!("Expected: {}", self.expected)))
        }
    }
}

/// Check for service status
pub struct ServiceCheck {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub service_name: String,
    pub should_be_enabled: Option<bool>,
    pub should_be_running: Option<bool>,
    pub should_exist: bool,
    pub remediation: String,
    pub tags: Vec<String>,
}

impl ServiceCheck {
    pub fn new(id: impl Into<String>, title: impl Into<String>, service: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            severity: Severity::Medium,
            service_name: service.into(),
            should_be_enabled: None,
            should_be_running: None,
            should_exist: true,
            remediation: String::new(),
            tags: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn should_be_enabled(mut self, enabled: bool) -> Self {
        self.should_be_enabled = Some(enabled);
        self
    }

    pub fn should_be_running(mut self, running: bool) -> Self {
        self.should_be_running = Some(running);
        self
    }

    pub fn should_not_exist(mut self) -> Self {
        self.should_exist = false;
        self
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = remediation.into();
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }
}

#[async_trait::async_trait]
impl ComplianceCheck for ServiceCheck {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn category(&self) -> CheckCategory {
        CheckCategory::Services
    }

    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }

    fn remediation(&self) -> &str {
        &self.remediation
    }

    async fn execute(&self, context: &ComplianceContext) -> ComplianceResult<CheckResult> {
        // Check if service exists
        let exists_cmd = format!(
            "systemctl list-unit-files {} 2>/dev/null | grep -q {}",
            self.service_name, self.service_name
        );
        let exists_result = context
            .connection
            .execute(&exists_cmd, None)
            .await
            .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

        let service_exists = exists_result.success;

        if !self.should_exist {
            return if service_exists {
                Ok(CheckResult::fail()
                    .with_observed(format!("Service {} exists", self.service_name)))
            } else {
                Ok(CheckResult::pass()
                    .with_observed(format!("Service {} does not exist", self.service_name)))
            };
        }

        if !service_exists {
            return Ok(CheckResult::skipped(format!(
                "Service {} not found",
                self.service_name
            )));
        }

        let mut issues = Vec::new();
        let mut observations = Vec::new();

        // Check enabled status
        if let Some(should_be_enabled) = self.should_be_enabled {
            let enabled_cmd = format!("systemctl is-enabled {} 2>/dev/null", self.service_name);
            let enabled_result = context
                .connection
                .execute(&enabled_cmd, None)
                .await
                .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

            let is_enabled = enabled_result.stdout.trim() == "enabled";
            observations.push(format!(
                "enabled={}",
                if is_enabled { "yes" } else { "no" }
            ));

            if should_be_enabled && !is_enabled {
                issues.push("should be enabled but is not");
            } else if !should_be_enabled && is_enabled {
                issues.push("should be disabled but is enabled");
            }
        }

        // Check running status
        if let Some(should_be_running) = self.should_be_running {
            let running_cmd = format!("systemctl is-active {} 2>/dev/null", self.service_name);
            let running_result = context
                .connection
                .execute(&running_cmd, None)
                .await
                .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

            let is_running = running_result.stdout.trim() == "active";
            observations.push(format!(
                "running={}",
                if is_running { "yes" } else { "no" }
            ));

            if should_be_running && !is_running {
                issues.push("should be running but is not");
            } else if !should_be_running && is_running {
                issues.push("should be stopped but is running");
            }
        }

        let observed = format!("{}: {}", self.service_name, observations.join(", "));

        if issues.is_empty() {
            Ok(CheckResult::pass().with_observed(observed))
        } else {
            Ok(CheckResult::fail()
                .with_observed(observed)
                .with_details(issues.join("; ")))
        }
    }
}

/// Check for command output matching expected pattern
pub struct CommandCheck {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub category: CheckCategory,
    pub command: String,
    pub expected_pattern: Option<String>,
    pub expected_exit_code: Option<i32>,
    pub not_expected_pattern: Option<String>,
    pub remediation: String,
    pub tags: Vec<String>,
}

impl CommandCheck {
    pub fn new(id: impl Into<String>, title: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            severity: Severity::Medium,
            category: CheckCategory::Maintenance,
            command: command.into(),
            expected_pattern: None,
            expected_exit_code: None,
            not_expected_pattern: None,
            remediation: String::new(),
            tags: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_category(mut self, category: CheckCategory) -> Self {
        self.category = category;
        self
    }

    pub fn with_expected_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.expected_pattern = Some(pattern.into());
        self
    }

    pub fn with_expected_exit_code(mut self, code: i32) -> Self {
        self.expected_exit_code = Some(code);
        self
    }

    pub fn with_not_expected_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.not_expected_pattern = Some(pattern.into());
        self
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = remediation.into();
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }
}

#[async_trait::async_trait]
impl ComplianceCheck for CommandCheck {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn category(&self) -> CheckCategory {
        self.category
    }

    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }

    fn remediation(&self) -> &str {
        &self.remediation
    }

    async fn execute(&self, context: &ComplianceContext) -> ComplianceResult<CheckResult> {
        let result = context
            .connection
            .execute(&self.command, None)
            .await
            .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

        let output = format!("{}{}", result.stdout, result.stderr);
        let mut issues = Vec::new();

        // Check exit code
        if let Some(expected_code) = self.expected_exit_code {
            if result.exit_code != expected_code {
                issues.push(format!(
                    "exit code {} (expected {})",
                    result.exit_code, expected_code
                ));
            }
        }

        // Check expected pattern
        if let Some(ref pattern) = self.expected_pattern {
            let re = Regex::new(pattern).map_err(|e| {
                ComplianceError::InvalidConfig(format!("Invalid regex: {}", e))
            })?;
            if !re.is_match(&output) {
                issues.push(format!("output does not match pattern: {}", pattern));
            }
        }

        // Check not-expected pattern
        if let Some(ref pattern) = self.not_expected_pattern {
            let re = Regex::new(pattern).map_err(|e| {
                ComplianceError::InvalidConfig(format!("Invalid regex: {}", e))
            })?;
            if re.is_match(&output) {
                issues.push(format!("output matches forbidden pattern: {}", pattern));
            }
        }

        let observed = if output.len() > 200 {
            format!("{}...", &output[..200])
        } else {
            output.clone()
        };

        if issues.is_empty() {
            Ok(CheckResult::pass().with_observed(observed))
        } else {
            Ok(CheckResult::fail()
                .with_observed(observed)
                .with_details(issues.join("; ")))
        }
    }
}

/// Helper function to execute a command and return the output
pub async fn exec_command(
    context: &ComplianceContext,
    command: &str,
) -> ComplianceResult<String> {
    let result = context
        .connection
        .execute(command, None)
        .await
        .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

    if result.success {
        Ok(result.stdout)
    } else {
        Err(ComplianceError::CheckFailed(format!(
            "Command failed: {}",
            result.stderr
        )))
    }
}

/// Helper function to check if a file exists on the remote system
pub async fn file_exists(context: &ComplianceContext, path: &str) -> ComplianceResult<bool> {
    context
        .connection
        .path_exists(Path::new(path))
        .await
        .map_err(|e| ComplianceError::CheckFailed(e.to_string()))
}

/// Helper function to read file contents
pub async fn read_file(context: &ComplianceContext, path: &str) -> ComplianceResult<String> {
    let cmd = format!("cat '{}'", path);
    exec_command(context, &cmd).await
}

/// Helper function to check if a line exists in a file
pub async fn file_contains(
    context: &ComplianceContext,
    path: &str,
    pattern: &str,
) -> ComplianceResult<bool> {
    let cmd = format!("grep -qE '{}' '{}' 2>/dev/null", pattern, path);
    let result = context
        .connection
        .execute(&cmd, None)
        .await
        .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

    Ok(result.success)
}

/// Helper function to get sysctl value
pub async fn get_sysctl(context: &ComplianceContext, parameter: &str) -> ComplianceResult<String> {
    let cmd = format!("sysctl -n {}", parameter);
    exec_command(context, &cmd).await.map(|s| s.trim().to_string())
}

/// Helper function to check if a package is installed
pub async fn package_installed(
    context: &ComplianceContext,
    package: &str,
) -> ComplianceResult<bool> {
    // Try dpkg first (Debian/Ubuntu)
    let dpkg_cmd = format!("dpkg -l {} 2>/dev/null | grep -q '^ii'", package);
    let dpkg_result = context.connection.execute(&dpkg_cmd, None).await;

    if let Ok(result) = dpkg_result {
        if result.success {
            return Ok(true);
        }
    }

    // Try rpm (RHEL/CentOS)
    let rpm_cmd = format!("rpm -q {} 2>/dev/null", package);
    let rpm_result = context
        .connection
        .execute(&rpm_cmd, None)
        .await
        .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

    Ok(rpm_result.success)
}

/// Helper to check if a service is enabled
pub async fn service_enabled(
    context: &ComplianceContext,
    service: &str,
) -> ComplianceResult<bool> {
    let cmd = format!("systemctl is-enabled {} 2>/dev/null", service);
    let result = context
        .connection
        .execute(&cmd, None)
        .await
        .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

    Ok(result.stdout.trim() == "enabled")
}

/// Helper to check if a service is running
pub async fn service_running(
    context: &ComplianceContext,
    service: &str,
) -> ComplianceResult<bool> {
    let cmd = format!("systemctl is-active {} 2>/dev/null", service);
    let result = context
        .connection
        .execute(&cmd, None)
        .await
        .map_err(|e| ComplianceError::CheckFailed(e.to_string()))?;

    Ok(result.stdout.trim() == "active")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{
        CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
        TransferOptions,
    };
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Clone)]
    struct MockConnection {
        id: String,
        commands: HashMap<String, CommandResult>,
        paths: HashMap<String, bool>,
    }

    impl MockConnection {
        fn new() -> Self {
            Self {
                id: "mock".to_string(),
                commands: HashMap::new(),
                paths: HashMap::new(),
            }
        }

        fn with_command(mut self, command: impl Into<String>, result: CommandResult) -> Self {
            self.commands.insert(command.into(), result);
            self
        }

        fn with_path(mut self, path: impl Into<String>, exists: bool) -> Self {
            self.paths.insert(path.into(), exists);
            self
        }
    }

    #[async_trait::async_trait]
    impl Connection for MockConnection {
        fn identifier(&self) -> &str {
            &self.id
        }

        async fn is_alive(&self) -> bool {
            true
        }

        async fn execute(
            &self,
            command: &str,
            _options: Option<ExecuteOptions>,
        ) -> ConnectionResult<CommandResult> {
            self.commands
                .get(command)
                .cloned()
                .ok_or_else(|| ConnectionError::ExecutionFailed(format!("unexpected command: {}", command)))
        }

        async fn upload(
            &self,
            _local_path: &Path,
            _remote_path: &Path,
            _options: Option<TransferOptions>,
        ) -> ConnectionResult<()> {
            Ok(())
        }

        async fn upload_content(
            &self,
            _content: &[u8],
            _remote_path: &Path,
            _options: Option<TransferOptions>,
        ) -> ConnectionResult<()> {
            Ok(())
        }

        async fn download(&self, _remote_path: &Path, _local_path: &Path) -> ConnectionResult<()> {
            Ok(())
        }

        async fn download_content(&self, _remote_path: &Path) -> ConnectionResult<Vec<u8>> {
            Ok(Vec::new())
        }

        async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
            Ok(*self.paths.get(&path.display().to_string()).unwrap_or(&false))
        }

        async fn is_directory(&self, _path: &Path) -> ConnectionResult<bool> {
            Ok(false)
        }

        async fn stat(&self, _path: &Path) -> ConnectionResult<FileStat> {
            Ok(FileStat {
                size: 0,
                mode: 0,
                uid: 0,
                gid: 0,
                atime: 0,
                mtime: 0,
                is_dir: false,
                is_file: true,
                is_symlink: false,
            })
        }

        async fn close(&self) -> ConnectionResult<()> {
            Ok(())
        }
    }

    #[test]
    fn test_check_result_builders() {
        let pass = CheckResult::pass().with_observed("test");
        assert!(matches!(pass.status, CheckStatus::Pass));
        assert_eq!(pass.observed, Some("test".to_string()));

        let fail = CheckResult::fail().with_details("reason");
        assert!(matches!(fail.status, CheckStatus::Fail));
        assert_eq!(fail.details, Some("reason".to_string()));
    }

    #[test]
    fn test_sysctl_comparison() {
        let check = SysctlCheck::new("test", "Test", "param", "1");
        assert!(check.compare("1"));
        assert!(!check.compare("0"));

        let gte_check = SysctlCheck::new("test", "Test", "param", "10")
            .with_comparison(Comparison::GreaterOrEqual);
        assert!(gte_check.compare("10"));
        assert!(gte_check.compare("20"));
        assert!(!gte_check.compare("5"));
    }

    #[test]
    fn test_check_category_display() {
        assert_eq!(format!("{}", CheckCategory::Ssh), "SSH");
        assert_eq!(format!("{}", CheckCategory::Filesystem), "Filesystem");
    }

    #[tokio::test]
    async fn test_file_check_execute_pass() {
        let path = "/etc/secure.conf";
        let stat_cmd = "stat -c '%U %G %a' '/etc/secure.conf'";
        let cat_cmd = "cat '/etc/secure.conf'";

        let conn = MockConnection::new()
            .with_path(path, true)
            .with_command(
                stat_cmd,
                CommandResult::success("root root 0644".to_string(), String::new()),
            )
            .with_command(
                cat_cmd,
                CommandResult::success("ALLOW=1".to_string(), String::new()),
            );
        let context = ComplianceContext::new(Arc::new(conn));

        let check = FileCheck::new("file.check", "Secure file", path)
            .with_owner("root")
            .with_group("root")
            .with_mode("0644")
            .with_content_pattern("ALLOW=\\d");

        let result = check.execute(&context).await.unwrap();
        assert!(matches!(result.status, CheckStatus::Pass));
        assert!(result
            .observed
            .unwrap_or_default()
            .contains("owner=root"));
    }

    #[tokio::test]
    async fn test_file_check_execute_failures() {
        let path = "/etc/secure.conf";
        let stat_cmd = "stat -c '%U %G %a' '/etc/secure.conf'";
        let cat_cmd = "cat '/etc/secure.conf'";

        let conn = MockConnection::new()
            .with_path(path, true)
            .with_command(
                stat_cmd,
                CommandResult::success("root root 0600".to_string(), String::new()),
            )
            .with_command(
                cat_cmd,
                CommandResult::success("DENY=1".to_string(), String::new()),
            );
        let context = ComplianceContext::new(Arc::new(conn));

        let check = FileCheck::new("file.check", "Secure file", path)
            .with_mode("0644")
            .with_content_pattern("ALLOW=\\d");

        let result = check.execute(&context).await.unwrap();
        assert!(matches!(result.status, CheckStatus::Fail));
        assert!(result.details.unwrap_or_default().contains("mode is"));
    }

    #[tokio::test]
    async fn test_sysctl_check_execute() {
        let conn = MockConnection::new().with_command(
            "sysctl -n net.ipv4.ip_forward",
            CommandResult::success("0\n".to_string(), String::new()),
        );
        let context = ComplianceContext::new(Arc::new(conn));
        let check = SysctlCheck::new(
            "sysctl.check",
            "IP forward",
            "net.ipv4.ip_forward",
            "1",
        );

        let result = check.execute(&context).await.unwrap();
        assert!(matches!(result.status, CheckStatus::Fail));
    }

    #[tokio::test]
    async fn test_service_check_execute() {
        let conn = MockConnection::new()
            .with_command(
                "systemctl list-unit-files sshd 2>/dev/null | grep -q sshd",
                CommandResult::success(String::new(), String::new()),
            )
            .with_command(
                "systemctl is-enabled sshd 2>/dev/null",
                CommandResult::success("disabled\n".to_string(), String::new()),
            )
            .with_command(
                "systemctl is-active sshd 2>/dev/null",
                CommandResult::success("active\n".to_string(), String::new()),
            );
        let context = ComplianceContext::new(Arc::new(conn));

        let check = ServiceCheck::new("service.check", "SSH", "sshd")
            .should_be_enabled(true)
            .should_be_running(false);

        let result = check.execute(&context).await.unwrap();
        assert!(matches!(result.status, CheckStatus::Fail));
        assert!(result
            .details
            .unwrap_or_default()
            .contains("should be enabled"));
    }

    #[tokio::test]
    async fn test_command_check_execute() {
        let conn = MockConnection::new().with_command(
            "echo ok",
            CommandResult::success("hello".to_string(), String::new()),
        );
        let context = ComplianceContext::new(Arc::new(conn));

        let check = CommandCheck::new("cmd.check", "Echo", "echo ok")
            .with_expected_pattern("hello")
            .with_expected_exit_code(0)
            .with_not_expected_pattern("ERROR");

        let result = check.execute(&context).await.unwrap();
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[tokio::test]
    async fn test_helper_functions() {
        let conn = MockConnection::new()
            .with_path("/tmp/test.txt", true)
            .with_command(
                "cat '/tmp/test.txt'",
                CommandResult::success("hello".to_string(), String::new()),
            )
            .with_command(
                "grep -qE 'hello' '/tmp/test.txt' 2>/dev/null",
                CommandResult::success(String::new(), String::new()),
            )
            .with_command(
                "sysctl -n net.ipv4.ip_forward",
                CommandResult::success("1\n".to_string(), String::new()),
            )
            .with_command(
                "dpkg -l openssl 2>/dev/null | grep -q '^ii'",
                CommandResult::failure(1, String::new(), String::new()),
            )
            .with_command(
                "rpm -q openssl 2>/dev/null",
                CommandResult::success("openssl".to_string(), String::new()),
            )
            .with_command(
                "systemctl is-enabled sshd 2>/dev/null",
                CommandResult::success("enabled\n".to_string(), String::new()),
            )
            .with_command(
                "systemctl is-active sshd 2>/dev/null",
                CommandResult::success("active\n".to_string(), String::new()),
            );
        let context = ComplianceContext::new(Arc::new(conn));

        assert!(file_exists(&context, "/tmp/test.txt").await.unwrap());
        assert_eq!(read_file(&context, "/tmp/test.txt").await.unwrap(), "hello");
        assert!(file_contains(&context, "/tmp/test.txt", "hello")
            .await
            .unwrap());
        assert_eq!(
            get_sysctl(&context, "net.ipv4.ip_forward")
                .await
                .unwrap(),
            "1"
        );
        assert!(package_installed(&context, "openssl").await.unwrap());
        assert!(service_enabled(&context, "sshd").await.unwrap());
        assert!(service_running(&context, "sshd").await.unwrap());
    }
}
