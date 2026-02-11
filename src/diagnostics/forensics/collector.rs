//! Forensics data collector
//!
//! Gathers audit events, state snapshots, drift reports, and system information
//! into a [`BundleData`] ready for redaction and export.

use serde::{Deserialize, Serialize};

use super::schema::{BundleContents, ForensicsBundleManifest, TimeRange};

/// Configuration controlling which data sources the collector gathers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    /// Include audit log events.
    pub include_audit: bool,
    /// Include state snapshots.
    pub include_state: bool,
    /// Include drift reports.
    pub include_drift: bool,
    /// Include host/OS system information.
    pub include_system_info: bool,
    /// Optional time range to restrict collected data.
    pub time_range: Option<TimeRange>,
    /// Optional host filter pattern.
    pub host_filter: Option<String>,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            include_audit: true,
            include_state: true,
            include_drift: true,
            include_system_info: true,
            time_range: None,
            host_filter: None,
        }
    }
}

/// System information snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemInfo {
    /// Hostname of the machine that created the bundle.
    pub hostname: String,
    /// Operating system description.
    pub os: String,
    /// Rustible version string.
    pub rustible_version: String,
    /// ISO-8601 timestamp when system info was captured.
    pub timestamp: String,
}

/// All data collected for a forensics bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleData {
    /// Bundle manifest describing the contents.
    pub manifest: ForensicsBundleManifest,
    /// Collected audit event entries (serialized strings).
    pub audit_events: Vec<String>,
    /// Serialized state snapshot data, if collected.
    pub state_data: Option<String>,
    /// Serialized drift report data, if collected.
    pub drift_data: Option<String>,
    /// System information, if collected.
    pub system_info: Option<SystemInfo>,
}

/// Collects forensics data from configured sources.
#[derive(Debug, Clone)]
pub struct ForensicsCollector {
    config: CollectorConfig,
}

impl ForensicsCollector {
    /// Create a new collector with the given configuration.
    pub fn new(config: CollectorConfig) -> Self {
        Self { config }
    }

    /// Collect data from all configured sources and return a [`BundleData`].
    pub fn collect(&self) -> BundleData {
        let now = chrono::Utc::now().to_rfc3339();

        let audit_events = if self.config.include_audit {
            self.collect_audit_events()
        } else {
            Vec::new()
        };

        let state_data = if self.config.include_state {
            Some(self.collect_state_data())
        } else {
            None
        };

        let drift_data = if self.config.include_drift {
            Some(self.collect_drift_data())
        } else {
            None
        };

        let system_info = if self.config.include_system_info {
            Some(self.collect_system_info(&now))
        } else {
            None
        };

        let time_range = self.config.time_range.clone().unwrap_or_else(|| TimeRange {
            from: now.clone(),
            to: now.clone(),
        });

        let manifest = ForensicsBundleManifest {
            version: "1.0.0".to_string(),
            created_at: now,
            time_range,
            host_filter: self.config.host_filter.clone(),
            contents: BundleContents {
                audit_events: audit_events.len(),
                state_snapshots: if state_data.is_some() { 1 } else { 0 },
                drift_reports: if drift_data.is_some() { 1 } else { 0 },
                system_info: system_info.is_some(),
            },
        };

        BundleData {
            manifest,
            audit_events,
            state_data,
            drift_data,
            system_info,
        }
    }

    /// Collect audit log events.
    ///
    /// In a full implementation this would read from the audit log pipeline;
    /// for now it returns an empty vec as a placeholder.
    fn collect_audit_events(&self) -> Vec<String> {
        // TODO: integrate with the audit log pipeline once available
        Vec::new()
    }

    /// Collect state snapshot data.
    fn collect_state_data(&self) -> String {
        // TODO: integrate with state management subsystem
        serde_json::json!({ "state": "no snapshot available" }).to_string()
    }

    /// Collect drift report data.
    fn collect_drift_data(&self) -> String {
        // TODO: integrate with drift detection subsystem
        serde_json::json!({ "drift": "no report available" }).to_string()
    }

    /// Collect system information.
    fn collect_system_info(&self, timestamp: &str) -> SystemInfo {
        SystemInfo {
            hostname: hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            os: std::env::consts::OS.to_string(),
            rustible_version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp: timestamp.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_collects_all() {
        let config = CollectorConfig::default();
        assert!(config.include_audit);
        assert!(config.include_state);
        assert!(config.include_drift);
        assert!(config.include_system_info);
    }

    #[test]
    fn test_collect_with_defaults() {
        let collector = ForensicsCollector::new(CollectorConfig::default());
        let data = collector.collect();

        assert_eq!(data.manifest.version, "1.0.0");
        assert!(data.system_info.is_some());
        assert!(data.state_data.is_some());
        assert!(data.drift_data.is_some());
        assert!(data.manifest.contents.system_info);
    }

    #[test]
    fn test_collect_with_filters() {
        let config = CollectorConfig {
            include_audit: false,
            include_state: false,
            include_drift: false,
            include_system_info: false,
            time_range: None,
            host_filter: Some("db*".to_string()),
        };
        let collector = ForensicsCollector::new(config);
        let data = collector.collect();

        assert!(data.audit_events.is_empty());
        assert!(data.state_data.is_none());
        assert!(data.drift_data.is_none());
        assert!(data.system_info.is_none());
        assert_eq!(data.manifest.host_filter, Some("db*".to_string()));
    }
}
