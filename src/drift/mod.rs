//! Configuration drift detection
//!
//! This module provides comprehensive drift detection capabilities to identify
//! when actual system state diverges from desired configuration state.

use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::connection::{Connection, CommandResult};

/// Drift detection configuration
#[derive(Debug, Clone)]
pub struct DriftConfig {
    /// Check for drift in files
    pub check_files: bool,
    /// Check for drift in packages
    pub check_packages: bool,
    /// Check for drift in services
    pub check_services: bool,
    /// Check for drift in users
    pub check_users: bool,
    /// Check for drift in permissions
    pub check_permissions: bool,
    /// Ignore specific drift types
    pub ignore_patterns: Vec<String>,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            check_files: true,
            check_packages: true,
            check_services: true,
            check_users: true,
            check_permissions: true,
            ignore_patterns: vec![
                "/var/log/*".to_string(),
                "/tmp/*".to_string(),
                "/proc/*".to_string(),
            ],
        }
    }
}

impl DriftConfig {
    /// Create a comprehensive config (check everything)
    pub fn comprehensive() -> Self {
        Self {
            check_files: true,
            check_packages: true,
            check_services: true,
            check_users: true,
            check_permissions: true,
            ignore_patterns: vec![],
        }
    }

    /// Create a minimal config (only critical checks)
    pub fn minimal() -> Self {
        Self {
            check_files: false,
            check_packages: true,
            check_services: true,
            check_users: false,
            check_permissions: false,
            ignore_patterns: vec![
                "/var/log/*".to_string(),
                "/tmp/*".to_string(),
            ],
        }
    }
}

/// Drift severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriftSeverity {
    /// Critical drift - immediate attention required
    Critical,
    /// High drift - should be addressed soon
    High,
    /// Medium drift - should be addressed when convenient
    Medium,
    /// Low drift - informational only
    Low,
}

/// Drift type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftType {
    /// File content drift
    FileContent { path: String },
    /// File permission drift
    FilePermissions { path: String },
    /// Package version drift
    PackageVersion { name: String },
    /// Package state drift
    PackageState { name: String },
    /// Service status drift
    ServiceStatus { name: String },
    /// Service configuration drift
    ServiceConfig { name: String },
    /// User existence drift
    UserExistence { name: String },
    /// Group membership drift
    GroupMembership { user: String, group: String },
    /// Unknown drift
    Unknown { description: String },
}

/// Drift item representing a single detected drift
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftItem {
    /// Unique identifier for this drift
    pub id: String,
    /// Host where drift was detected
    pub host: String,
    /// Type of drift
    pub drift_type: DriftType,
    /// Severity level
    pub severity: DriftSeverity,
    /// Expected state
    pub expected_state: serde_json::Value,
    /// Actual state
    pub actual_state: serde_json::Value,
    /// When this drift was detected
    pub detected_at: DateTime<Utc>,
    /// When this drift was first detected (if known)
    pub first_detected_at: Option<DateTime<Utc>>,
    /// Additional notes
    pub notes: Option<String>,
}

impl DriftItem {
    /// Create a new drift item
    pub fn new(
        host: impl Into<String>,
        drift_type: DriftType,
        severity: DriftSeverity,
        expected: serde_json::Value,
        actual: serde_json::Value,
    ) -> Self {
        let host_string = host.into();
        let id = format!("{}-{}", host_string, uuid::Uuid::new_v4());
        
        Self {
            id,
            host: host_string,
            drift_type,
            severity,
            expected_state: expected,
            actual_state: actual,
            detected_at: Utc::now(),
            first_detected_at: None,
            notes: None,
        }
    }

    /// Set first detection time
    pub fn with_first_detected(mut self, time: DateTime<Utc>) -> Self {
        self.first_detected_at = Some(time);
        self
    }

    /// Add notes
    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }

    /// Check if this drift is new (first detection within 24 hours)
    pub fn is_new(&self) -> bool {
        if let Some(first) = self.first_detected_at {
            Utc::now().signed_duration_since(first).num_hours() < 24
        } else {
            true
        }
    }

    /// Calculate drift age in hours
    pub fn age_hours(&self) -> Option<i64> {
        self.first_detected_at
            .map(|t| Utc::now().signed_duration_since(t).num_hours())
    }
}

/// Drift report for a host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDriftReport {
    /// Host name
    pub host: String,
    /// All drift items
    pub drifts: Vec<DriftItem>,
    /// Report timestamp
    pub timestamp: DateTime<Utc>,
    /// Total drift count
    pub total_count: usize,
    /// Critical drift count
    pub critical_count: usize,
    /// High drift count
    pub high_count: usize,
    /// Medium drift count
    pub medium_count: usize,
    /// Low drift count
    pub low_count: usize,
}

impl HostDriftReport {
    /// Create a new host drift report
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            drifts: Vec::new(),
            timestamp: Utc::now(),
            total_count: 0,
            critical_count: 0,
            high_count: 0,
            medium_count: 0,
            low_count: 0,
        }
    }

    /// Add a drift item
    pub fn add_drift(&mut self, drift: DriftItem) {
        match drift.severity {
            DriftSeverity::Critical => self.critical_count += 1,
            DriftSeverity::High => self.high_count += 1,
            DriftSeverity::Medium => self.medium_count += 1,
            DriftSeverity::Low => self.low_count += 1,
        }
        self.total_count += 1;
        self.drifts.push(drift);
    }

    /// Check if host has any critical drift
    pub fn has_critical_drift(&self) -> bool {
        self.critical_count > 0
    }

    /// Check if host has any drift
    pub fn has_drift(&self) -> bool {
        self.total_count > 0
    }

    /// Get severity summary
    pub fn severity_summary(&self) -> String {
        let mut parts = Vec::new();
        if self.critical_count > 0 {
            parts.push(format!("{} critical", self.critical_count));
        }
        if self.high_count > 0 {
            parts.push(format!("{} high", self.high_count));
        }
        if self.medium_count > 0 {
            parts.push(format!("{} medium", self.medium_count));
        }
        if self.low_count > 0 {
            parts.push(format!("{} low", self.low_count));
        }
        
        if parts.is_empty() {
            "No drift detected".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Overall drift report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftReport {
    /// Reports for each host
    pub hosts: Vec<HostDriftReport>,
    /// Report timestamp
    pub timestamp: DateTime<Utc>,
    /// Total hosts checked
    pub total_hosts: usize,
    /// Hosts with drift
    pub hosts_with_drift: usize,
    /// Total drift items across all hosts
    pub total_drifts: usize,
    /// Summary statistics
    pub summary: DriftSummary,
}

impl DriftReport {
    /// Create a new drift report
    pub fn new() -> Self {
        Self {
            hosts: Vec::new(),
            timestamp: Utc::now(),
            total_hosts: 0,
            hosts_with_drift: 0,
            total_drifts: 0,
            summary: DriftSummary::default(),
        }
    }

    /// Add a host report
    pub fn add_host_report(&mut self, host_report: HostDriftReport) {
        self.total_hosts += 1;
        if host_report.has_drift() {
            self.hosts_with_drift += 1;
        }
        self.total_drifts += host_report.total_count;
        
        self.summary.critical += host_report.critical_count;
        self.summary.high += host_report.high_count;
        self.summary.medium += host_report.medium_count;
        self.summary.low += host_report.low_count;
        
        self.hosts.push(host_report);
    }

    /// Check if report has any drift
    pub fn has_drift(&self) -> bool {
        self.total_drifts > 0
    }

    /// Format as human-readable summary
    pub fn format_summary(&self) -> String {
        let mut output = format!(
            "Drift Report - {}\n",
            self.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        );
        output.push_str(&format!("Hosts checked: {}/{}\n", self.hosts_with_drift, self.total_hosts));
        output.push_str(&format!("Total drifts: {}\n", self.total_drifts));
        output.push_str(&format!("  Critical: {}\n", self.summary.critical));
        output.push_str(&format!("  High: {}\n", self.summary.high));
        output.push_str(&format!("  Medium: {}\n", self.summary.medium));
        output.push_str(&format!("  Low: {}\n", self.summary.low));
        
        output
    }
}

impl Default for DriftReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Drift summary statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriftSummary {
    /// Critical drift count
    pub critical: usize,
    /// High drift count
    pub high: usize,
    /// Medium drift count
    pub medium: usize,
    /// Low drift count
    pub low: usize,
}

/// Drift detector that compares actual remote state against desired configuration.
///
/// Requires a [`Connection`] to execute remote commands for state inspection.
/// Without a connection, check methods return errors indicating no connection is available.
pub struct DriftDetector {
    config: DriftConfig,
    connection: Option<Arc<dyn Connection + Send + Sync>>,
}

impl DriftDetector {
    /// Create a new drift detector without a connection.
    ///
    /// Check methods will return errors until a connection is provided via
    /// [`with_connection`](Self::with_connection).
    pub fn new(config: DriftConfig) -> Self {
        Self {
            config,
            connection: None,
        }
    }

    /// Create a new drift detector with a connection for remote command execution.
    pub fn with_connection(
        config: DriftConfig,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> Self {
        Self {
            config,
            connection: Some(connection),
        }
    }

    /// Detect drift for a single host by comparing desired state against actual remote state.
    ///
    /// Iterates over files, packages, and services in `desired_state` (according to the
    /// config flags) and creates [`DriftItem`] entries for any detected differences.
    pub async fn detect_drift(
        &self,
        host: &str,
        desired_state: &serde_json::Value,
    ) -> Result<HostDriftReport, Box<dyn std::error::Error>> {
        let mut report = HostDriftReport::new(host);

        // Check file drift
        if self.config.check_files {
            if let Some(files) = desired_state.get("files").and_then(|v| v.as_object()) {
                for (path, expected) in files {
                    match self.check_file_state(host, path, expected).await {
                        Ok(Some(drift_item)) => report.add_drift(drift_item),
                        Ok(None) => { /* in sync */ }
                        Err(e) => {
                            report.add_drift(DriftItem::new(
                                host,
                                DriftType::Unknown {
                                    description: format!("File check failed for {}: {}", path, e),
                                },
                                DriftSeverity::Low,
                                expected.clone(),
                                serde_json::json!({"error": e.to_string()}),
                            ));
                        }
                    }
                }
            }
        }

        // Check package drift
        if self.config.check_packages {
            if let Some(packages) = desired_state.get("packages").and_then(|v| v.as_object()) {
                for (name, expected) in packages {
                    match self.check_package_state(host, name, expected).await {
                        Ok(Some(drift_item)) => report.add_drift(drift_item),
                        Ok(None) => { /* in sync */ }
                        Err(e) => {
                            report.add_drift(DriftItem::new(
                                host,
                                DriftType::Unknown {
                                    description: format!(
                                        "Package check failed for {}: {}",
                                        name, e
                                    ),
                                },
                                DriftSeverity::Low,
                                expected.clone(),
                                serde_json::json!({"error": e.to_string()}),
                            ));
                        }
                    }
                }
            }
        }

        // Check service drift
        if self.config.check_services {
            if let Some(services) = desired_state.get("services").and_then(|v| v.as_object()) {
                for (name, expected) in services {
                    match self.check_service_state(host, name, expected).await {
                        Ok(Some(drift_item)) => report.add_drift(drift_item),
                        Ok(None) => { /* in sync */ }
                        Err(e) => {
                            report.add_drift(DriftItem::new(
                                host,
                                DriftType::Unknown {
                                    description: format!(
                                        "Service check failed for {}: {}",
                                        name, e
                                    ),
                                },
                                DriftSeverity::Low,
                                expected.clone(),
                                serde_json::json!({"error": e.to_string()}),
                            ));
                        }
                    }
                }
            }
        }

        Ok(report)
    }

    /// Check file state on the remote host.
    ///
    /// Runs `stat` and `sha256sum` to compare permissions, ownership, and content
    /// checksum against the expected values.
    ///
    /// Expected JSON fields: `checksum`, `owner`, `group`, `mode`.
    async fn check_file_state(
        &self,
        host: &str,
        path: &str,
        expected: &serde_json::Value,
    ) -> Result<Option<DriftItem>, Box<dyn std::error::Error>> {
        let conn = self
            .connection
            .as_ref()
            .ok_or("No connection available for drift detection")?;

        let result = conn
            .execute(
                &format!(
                    "stat -c '%a %U %G' {} 2>/dev/null && sha256sum {} 2>/dev/null",
                    path, path
                ),
                None,
            )
            .await?;

        if !result.success {
            return Ok(Some(DriftItem::new(
                host,
                DriftType::FileContent {
                    path: path.to_string(),
                },
                DriftSeverity::High,
                expected.clone(),
                serde_json::json!({"exists": false}),
            )));
        }

        let (actual_mode, actual_owner, actual_group, actual_checksum) =
            parse_file_state(&result.stdout);

        let mut diffs = serde_json::Map::new();

        if let Some(exp_checksum) = expected.get("checksum").and_then(|v| v.as_str()) {
            if let Some(ref actual) = actual_checksum {
                if actual != exp_checksum {
                    diffs.insert(
                        "checksum".to_string(),
                        serde_json::json!({"expected": exp_checksum, "actual": actual}),
                    );
                }
            }
        }

        if let Some(exp_owner) = expected.get("owner").and_then(|v| v.as_str()) {
            if let Some(ref actual) = actual_owner {
                if actual != exp_owner {
                    diffs.insert(
                        "owner".to_string(),
                        serde_json::json!({"expected": exp_owner, "actual": actual}),
                    );
                }
            }
        }

        if let Some(exp_group) = expected.get("group").and_then(|v| v.as_str()) {
            if let Some(ref actual) = actual_group {
                if actual != exp_group {
                    diffs.insert(
                        "group".to_string(),
                        serde_json::json!({"expected": exp_group, "actual": actual}),
                    );
                }
            }
        }

        if let Some(exp_mode) = expected.get("mode").and_then(|v| v.as_str()) {
            if let Some(ref actual) = actual_mode {
                if actual != exp_mode {
                    diffs.insert(
                        "mode".to_string(),
                        serde_json::json!({"expected": exp_mode, "actual": actual}),
                    );
                }
            }
        }

        if diffs.is_empty() {
            Ok(None)
        } else {
            // Determine the most appropriate drift type
            let drift_type = if diffs.contains_key("checksum") {
                DriftType::FileContent {
                    path: path.to_string(),
                }
            } else {
                DriftType::FilePermissions {
                    path: path.to_string(),
                }
            };

            Ok(Some(DriftItem::new(
                host,
                drift_type,
                DriftSeverity::Medium,
                expected.clone(),
                serde_json::Value::Object(diffs),
            )))
        }
    }

    /// Check package state on the remote host.
    ///
    /// Tries `dpkg-query` (Debian/Ubuntu) first, then falls back to `rpm` (RHEL/CentOS).
    ///
    /// Expected JSON fields: `state` (present/absent), `version`.
    async fn check_package_state(
        &self,
        host: &str,
        name: &str,
        expected: &serde_json::Value,
    ) -> Result<Option<DriftItem>, Box<dyn std::error::Error>> {
        let conn = self
            .connection
            .as_ref()
            .ok_or("No connection available for drift detection")?;

        let result = conn
            .execute(
                &format!(
                    "dpkg-query -W -f='${{Status}} ${{Version}}' {} 2>/dev/null || rpm -q --qf '%{{VERSION}}-%{{RELEASE}}' {} 2>/dev/null",
                    name, name
                ),
                None,
            )
            .await?;

        let expected_state = expected
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present");

        let (is_installed, actual_version) = parse_package_state(&result);

        match expected_state {
            "absent" => {
                if is_installed {
                    Ok(Some(DriftItem::new(
                        host,
                        DriftType::PackageState {
                            name: name.to_string(),
                        },
                        DriftSeverity::Medium,
                        expected.clone(),
                        serde_json::json!({"state": "present", "version": actual_version}),
                    )))
                } else {
                    Ok(None)
                }
            }
            // "present" or "latest"
            _ => {
                if !is_installed {
                    return Ok(Some(DriftItem::new(
                        host,
                        DriftType::PackageState {
                            name: name.to_string(),
                        },
                        DriftSeverity::High,
                        expected.clone(),
                        serde_json::json!({"state": "absent"}),
                    )));
                }

                // Check version if specified
                if let Some(exp_version) = expected.get("version").and_then(|v| v.as_str()) {
                    if let Some(ref actual_ver) = actual_version {
                        if actual_ver != exp_version {
                            return Ok(Some(DriftItem::new(
                                host,
                                DriftType::PackageVersion {
                                    name: name.to_string(),
                                },
                                DriftSeverity::Medium,
                                expected.clone(),
                                serde_json::json!({"state": "present", "version": actual_ver}),
                            )));
                        }
                    }
                }

                Ok(None)
            }
        }
    }

    /// Check service state on the remote host.
    ///
    /// Uses `systemctl is-active` and `systemctl is-enabled` to inspect service status.
    ///
    /// Expected JSON fields: `state` (started/stopped), `enabled` (true/false).
    async fn check_service_state(
        &self,
        host: &str,
        name: &str,
        expected: &serde_json::Value,
    ) -> Result<Option<DriftItem>, Box<dyn std::error::Error>> {
        let conn = self
            .connection
            .as_ref()
            .ok_or("No connection available for drift detection")?;

        let result = conn
            .execute(
                &format!(
                    "systemctl is-active {} 2>/dev/null; echo '---'; systemctl is-enabled {} 2>/dev/null",
                    name, name
                ),
                None,
            )
            .await?;

        let (actual_active, actual_enabled) = parse_service_state(&result.stdout);

        let mut diffs = serde_json::Map::new();

        if let Some(exp_state) = expected.get("state").and_then(|v| v.as_str()) {
            let expected_active = match exp_state {
                "started" | "running" => true,
                "stopped" | "inactive" => false,
                _ => true,
            };
            if actual_active != expected_active {
                let actual_str = if actual_active { "started" } else { "stopped" };
                diffs.insert(
                    "state".to_string(),
                    serde_json::json!({"expected": exp_state, "actual": actual_str}),
                );
            }
        }

        if let Some(exp_enabled) = expected.get("enabled") {
            let expected_enabled = match exp_enabled {
                serde_json::Value::Bool(b) => *b,
                serde_json::Value::String(s) => s == "true" || s == "enabled",
                _ => true,
            };
            if actual_enabled != expected_enabled {
                diffs.insert(
                    "enabled".to_string(),
                    serde_json::json!({"expected": expected_enabled, "actual": actual_enabled}),
                );
            }
        }

        if diffs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DriftItem::new(
                host,
                DriftType::ServiceStatus {
                    name: name.to_string(),
                },
                DriftSeverity::High,
                expected.clone(),
                serde_json::Value::Object(diffs),
            )))
        }
    }

    /// Detect drift for multiple hosts
    pub async fn detect_drift_multi(
        &self,
        hosts: &[String],
        desired_states: &HashMap<String, serde_json::Value>,
    ) -> Result<DriftReport, Box<dyn std::error::Error>> {
        let mut report = DriftReport::new();

        for host in hosts {
            if let Some(desired_state) = desired_states.get(host) {
                let host_report = self.detect_drift(host, desired_state).await?;
                report.add_host_report(host_report);
            }
        }

        Ok(report)
    }
}

/// Parse stat + sha256sum output into (mode, owner, group, checksum).
///
/// Expected format:
/// ```text
/// 644 root root
/// abc123def456...  /path/to/file
/// ```
fn parse_file_state(
    output: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let lines: Vec<&str> = output.lines().collect();
    let mut mode = None;
    let mut owner = None;
    let mut group = None;
    let mut checksum = None;

    // First line: stat output "mode owner group"
    if let Some(stat_line) = lines.first() {
        let parts: Vec<&str> = stat_line.split_whitespace().collect();
        if parts.len() >= 3 {
            mode = Some(parts[0].to_string());
            owner = Some(parts[1].to_string());
            group = Some(parts[2].to_string());
        }
    }

    // Second line: sha256sum output "hash  filename"
    if let Some(hash_line) = lines.get(1) {
        if let Some(hash) = hash_line.split_whitespace().next() {
            checksum = Some(hash.to_string());
        }
    }

    (mode, owner, group, checksum)
}

/// Parse dpkg-query or rpm output to determine installation state and version.
///
/// dpkg format: `install ok installed <version>`
/// rpm format: `<version>-<release>` (or error message if not installed)
fn parse_package_state(result: &CommandResult) -> (bool, Option<String>) {
    if !result.success {
        return (false, None);
    }

    let stdout = result.stdout.trim();

    // dpkg-query format: "install ok installed <version>"
    if stdout.contains("install ok installed") {
        let version = stdout
            .strip_prefix("install ok installed ")
            .map(|v| v.trim().to_string());
        return (true, version);
    }

    // rpm format: just the version string, or "package <name> is not installed"
    if stdout.contains("is not installed") || stdout.is_empty() {
        return (false, None);
    }

    // Assume rpm version output
    (true, Some(stdout.to_string()))
}

/// Parse systemctl output to determine active and enabled states.
///
/// Expected format:
/// ```text
/// active
/// ---
/// enabled
/// ```
fn parse_service_state(output: &str) -> (bool, bool) {
    let parts: Vec<&str> = output.split("---").collect();

    let active = parts
        .first()
        .map(|s| s.trim() == "active")
        .unwrap_or(false);

    let enabled = parts
        .get(1)
        .map(|s| s.trim() == "enabled")
        .unwrap_or(false);

    (active, enabled)
}

impl Default for DriftDetector {
    fn default() -> Self {
        Self::new(DriftConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{
        CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
        TransferOptions,
    };
    use async_trait::async_trait;
    use std::path::Path;
    use std::sync::Mutex;

    /// A mock connection that returns preconfigured responses for testing.
    struct MockConnection {
        responses: Mutex<Vec<CommandResult>>,
    }

    impl MockConnection {
        fn new(responses: Vec<CommandResult>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl Connection for MockConnection {
        fn identifier(&self) -> &str {
            "mock"
        }

        async fn is_alive(&self) -> bool {
            true
        }

        async fn execute(
            &self,
            _command: &str,
            _options: Option<ExecuteOptions>,
        ) -> ConnectionResult<CommandResult> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Ok(CommandResult::failure(
                    1,
                    String::new(),
                    "no mock response".to_string(),
                ))
            } else {
                Ok(responses.remove(0))
            }
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

        async fn download(
            &self,
            _remote_path: &Path,
            _local_path: &Path,
        ) -> ConnectionResult<()> {
            Ok(())
        }

        async fn download_content(&self, _remote_path: &Path) -> ConnectionResult<Vec<u8>> {
            Ok(vec![])
        }

        async fn path_exists(&self, _path: &Path) -> ConnectionResult<bool> {
            Ok(false)
        }

        async fn is_directory(&self, _path: &Path) -> ConnectionResult<bool> {
            Ok(false)
        }

        async fn stat(&self, _path: &Path) -> ConnectionResult<FileStat> {
            Err(ConnectionError::ExecutionFailed(
                "not implemented".to_string(),
            ))
        }

        async fn close(&self) -> ConnectionResult<()> {
            Ok(())
        }
    }

    #[test]
    fn test_drift_config() {
        let config = DriftConfig::comprehensive();
        assert!(config.check_files);
        assert!(config.check_packages);
    }

    #[test]
    fn test_drift_item() {
        let drift = DriftItem::new(
            "test-host",
            DriftType::FileContent {
                path: "/etc/hosts".to_string(),
            },
            DriftSeverity::Critical,
            serde_json::json!("expected"),
            serde_json::json!("actual"),
        );

        assert_eq!(drift.host, "test-host");
        assert!(drift.is_new());
    }

    #[test]
    fn test_host_drift_report() {
        let mut report = HostDriftReport::new("test-host");
        assert!(!report.has_drift());

        let drift = DriftItem::new(
            "test-host",
            DriftType::PackageVersion {
                name: "nginx".to_string(),
            },
            DriftSeverity::High,
            serde_json::json!("1.18.0"),
            serde_json::json!("1.19.0"),
        );

        report.add_drift(drift);
        assert!(report.has_drift());
        assert_eq!(report.high_count, 1);
    }

    #[test]
    fn test_drift_report() {
        let mut report = DriftReport::new();
        assert!(!report.has_drift());

        let mut host_report = HostDriftReport::new("host1");
        let drift = DriftItem::new(
            "host1",
            DriftType::ServiceStatus {
                name: "nginx".to_string(),
            },
            DriftSeverity::Medium,
            serde_json::json!("running"),
            serde_json::json!("stopped"),
        );
        host_report.add_drift(drift);

        report.add_host_report(host_report);
        assert!(report.has_drift());
    }

    // --- Parsing tests ---

    #[test]
    fn test_parse_file_state_full() {
        let output = "644 root www-data\nabc123  /etc/nginx/nginx.conf\n";
        let (mode, owner, group, checksum) = parse_file_state(output);
        assert_eq!(mode.as_deref(), Some("644"));
        assert_eq!(owner.as_deref(), Some("root"));
        assert_eq!(group.as_deref(), Some("www-data"));
        assert_eq!(checksum.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_parse_file_state_stat_only() {
        let output = "755 deploy deploy\n";
        let (mode, owner, group, checksum) = parse_file_state(output);
        assert_eq!(mode.as_deref(), Some("755"));
        assert_eq!(owner.as_deref(), Some("deploy"));
        assert_eq!(group.as_deref(), Some("deploy"));
        assert_eq!(checksum, None);
    }

    #[test]
    fn test_parse_file_state_empty() {
        let (mode, owner, group, checksum) = parse_file_state("");
        assert_eq!(mode, None);
        assert_eq!(owner, None);
        assert_eq!(group, None);
        assert_eq!(checksum, None);
    }

    #[test]
    fn test_parse_package_state_dpkg_installed() {
        let result = CommandResult::success(
            "install ok installed 1.18.0-6ubuntu1".to_string(),
            String::new(),
        );
        let (installed, version) = parse_package_state(&result);
        assert!(installed);
        assert_eq!(version.as_deref(), Some("1.18.0-6ubuntu1"));
    }

    #[test]
    fn test_parse_package_state_not_installed() {
        let result = CommandResult::failure(1, String::new(), "not found".to_string());
        let (installed, version) = parse_package_state(&result);
        assert!(!installed);
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_package_state_rpm() {
        let result = CommandResult::success("1.18.0-2.el8".to_string(), String::new());
        let (installed, version) = parse_package_state(&result);
        assert!(installed);
        assert_eq!(version.as_deref(), Some("1.18.0-2.el8"));
    }

    #[test]
    fn test_parse_package_state_rpm_not_installed() {
        let result = CommandResult::success(
            "package nginx is not installed".to_string(),
            String::new(),
        );
        let (installed, _) = parse_package_state(&result);
        assert!(!installed);
    }

    #[test]
    fn test_parse_service_state_active_enabled() {
        let output = "active\n---\nenabled\n";
        let (active, enabled) = parse_service_state(output);
        assert!(active);
        assert!(enabled);
    }

    #[test]
    fn test_parse_service_state_inactive_disabled() {
        let output = "inactive\n---\ndisabled\n";
        let (active, enabled) = parse_service_state(output);
        assert!(!active);
        assert!(!enabled);
    }

    #[test]
    fn test_parse_service_state_active_disabled() {
        let output = "active\n---\ndisabled\n";
        let (active, enabled) = parse_service_state(output);
        assert!(active);
        assert!(!enabled);
    }

    // --- Integration tests with MockConnection ---

    #[tokio::test]
    async fn test_detect_drift_no_connection() {
        let detector = DriftDetector::default();
        let state = serde_json::json!({
            "files": {"/etc/test": {"checksum": "abc"}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        // Without a connection, each check creates an Unknown drift item from the error
        assert!(report.has_drift());
        assert_eq!(report.total_count, 1);
        assert_eq!(report.low_count, 1);
    }

    #[tokio::test]
    async fn test_detect_drift_file_missing() {
        let conn = Arc::new(MockConnection::new(vec![CommandResult::failure(
            1,
            String::new(),
            "No such file".to_string(),
        )]));

        let detector = DriftDetector::with_connection(DriftConfig::comprehensive(), conn);
        let state = serde_json::json!({
            "files": {"/etc/missing": {"checksum": "abc123"}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        assert!(report.has_drift());
        assert_eq!(report.high_count, 1);
    }

    #[tokio::test]
    async fn test_detect_drift_file_in_sync() {
        let conn = Arc::new(MockConnection::new(vec![CommandResult::success(
            "644 root root\nabc123  /etc/test.conf\n".to_string(),
            String::new(),
        )]));

        let detector = DriftDetector::with_connection(DriftConfig::comprehensive(), conn);
        let state = serde_json::json!({
            "files": {"/etc/test.conf": {"checksum": "abc123", "owner": "root", "group": "root", "mode": "644"}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        assert!(!report.has_drift());
    }

    #[tokio::test]
    async fn test_detect_drift_file_checksum_mismatch() {
        let conn = Arc::new(MockConnection::new(vec![CommandResult::success(
            "644 root root\nwronghash  /etc/test.conf\n".to_string(),
            String::new(),
        )]));

        let detector = DriftDetector::with_connection(DriftConfig::comprehensive(), conn);
        let state = serde_json::json!({
            "files": {"/etc/test.conf": {"checksum": "abc123", "owner": "root", "group": "root", "mode": "644"}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        assert!(report.has_drift());
        assert_eq!(report.total_count, 1);
    }

    #[tokio::test]
    async fn test_detect_drift_package_missing() {
        let conn = Arc::new(MockConnection::new(vec![CommandResult::failure(
            1,
            String::new(),
            "not found".to_string(),
        )]));

        let detector = DriftDetector::with_connection(
            DriftConfig {
                check_files: false,
                check_services: false,
                ..DriftConfig::comprehensive()
            },
            conn,
        );
        let state = serde_json::json!({
            "packages": {"nginx": {"state": "present"}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        assert!(report.has_drift());
        assert_eq!(report.high_count, 1);
    }

    #[tokio::test]
    async fn test_detect_drift_package_version_drift() {
        let conn = Arc::new(MockConnection::new(vec![CommandResult::success(
            "install ok installed 1.19.0".to_string(),
            String::new(),
        )]));

        let detector = DriftDetector::with_connection(
            DriftConfig {
                check_files: false,
                check_services: false,
                ..DriftConfig::comprehensive()
            },
            conn,
        );
        let state = serde_json::json!({
            "packages": {"nginx": {"state": "present", "version": "1.18.0"}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        assert!(report.has_drift());
        assert_eq!(report.medium_count, 1);
    }

    #[tokio::test]
    async fn test_detect_drift_service_stopped() {
        let conn = Arc::new(MockConnection::new(vec![CommandResult::success(
            "inactive\n---\nenabled\n".to_string(),
            String::new(),
        )]));

        let detector = DriftDetector::with_connection(
            DriftConfig {
                check_files: false,
                check_packages: false,
                ..DriftConfig::comprehensive()
            },
            conn,
        );
        let state = serde_json::json!({
            "services": {"nginx": {"state": "started", "enabled": true}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        assert!(report.has_drift());
        assert_eq!(report.high_count, 1);
    }

    #[tokio::test]
    async fn test_detect_drift_service_in_sync() {
        let conn = Arc::new(MockConnection::new(vec![CommandResult::success(
            "active\n---\nenabled\n".to_string(),
            String::new(),
        )]));

        let detector = DriftDetector::with_connection(
            DriftConfig {
                check_files: false,
                check_packages: false,
                ..DriftConfig::comprehensive()
            },
            conn,
        );
        let state = serde_json::json!({
            "services": {"nginx": {"state": "started", "enabled": true}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        assert!(!report.has_drift());
    }

    #[tokio::test]
    async fn test_detect_drift_package_should_be_absent() {
        let conn = Arc::new(MockConnection::new(vec![CommandResult::success(
            "install ok installed 1.18.0".to_string(),
            String::new(),
        )]));

        let detector = DriftDetector::with_connection(
            DriftConfig {
                check_files: false,
                check_services: false,
                ..DriftConfig::comprehensive()
            },
            conn,
        );
        let state = serde_json::json!({
            "packages": {"badpkg": {"state": "absent"}},
        });

        let report = detector.detect_drift("host1", &state).await.unwrap();
        assert!(report.has_drift());
        assert_eq!(report.medium_count, 1);
    }
}
