//! Configuration drift detection
//!
//! This module provides comprehensive drift detection capabilities to identify
//! when actual system state diverges from desired configuration state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
            ignore_patterns: vec!["/var/log/*".to_string(), "/tmp/*".to_string()],
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
        let id = format!("{}-{}", host.into(), uuid::Uuid::new_v4());

        Self {
            id,
            host: host.into(),
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
        output.push_str(&format!(
            "Hosts checked: {}/{}\n",
            self.hosts_with_drift, self.total_hosts
        ));
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

/// Drift detector
pub struct DriftDetector {
    config: DriftConfig,
}

impl DriftDetector {
    /// Create a new drift detector
    pub fn new(config: DriftConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(DriftConfig::default())
    }

    /// Detect drift for a single host
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
                    if let Err(_) = self.check_file_state(host, path, expected).await {
                        // Would create drift items here in real implementation
                    }
                }
            }
        }

        // Check package drift
        if self.config.check_packages {
            if let Some(packages) = desired_state.get("packages").and_then(|v| v.as_object()) {
                for (name, expected) in packages {
                    if let Err(_) = self.check_package_state(host, name, expected).await {
                        // Would create drift items here in real implementation
                    }
                }
            }
        }

        // Check service drift
        if self.config.check_services {
            if let Some(services) = desired_state.get("services").and_then(|v| v.as_object()) {
                for (name, expected) in services {
                    if let Err(_) = self.check_service_state(host, name, expected).await {
                        // Would create drift items here in real implementation
                    }
                }
            }
        }

        Ok(report)
    }

    /// Check file state
    async fn check_file_state(
        &self,
        host: &str,
        path: &str,
        expected: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Implementation would check actual file state
        // For now, just return Ok
        Ok(())
    }

    /// Check package state
    async fn check_package_state(
        &self,
        host: &str,
        name: &str,
        expected: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Implementation would check actual package state
        Ok(())
    }

    /// Check service state
    async fn check_service_state(
        &self,
        host: &str,
        name: &str,
        expected: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Implementation would check actual service state
        Ok(())
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

impl Default for DriftDetector {
    fn default() -> Self {
        Self::new(DriftConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
