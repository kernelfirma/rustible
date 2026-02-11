//! Forensics bundle schema definitions
//!
//! Defines the manifest and metadata structures used to describe the contents
//! and provenance of a forensics bundle.

use serde::{Deserialize, Serialize};

/// Time range bounding the data captured in a forensics bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimeRange {
    /// ISO-8601 start timestamp.
    pub from: String,
    /// ISO-8601 end timestamp.
    pub to: String,
}

/// Summary of what a forensics bundle contains.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleContents {
    /// Number of audit-log events included.
    pub audit_events: usize,
    /// Number of state snapshots included.
    pub state_snapshots: usize,
    /// Number of drift reports included.
    pub drift_reports: usize,
    /// Whether host/OS system information is included.
    pub system_info: bool,
}

/// Top-level manifest embedded in every forensics bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForensicsBundleManifest {
    /// Schema version for forward compatibility.
    pub version: String,
    /// ISO-8601 timestamp when the bundle was created.
    pub created_at: String,
    /// Time range of captured data.
    pub time_range: TimeRange,
    /// Optional host filter that was applied during collection.
    pub host_filter: Option<String>,
    /// Summary of bundle contents.
    pub contents: BundleContents,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_round_trip() {
        let manifest = ForensicsBundleManifest {
            version: "1.0.0".to_string(),
            created_at: "2026-02-11T00:00:00Z".to_string(),
            time_range: TimeRange {
                from: "2026-02-10T00:00:00Z".to_string(),
                to: "2026-02-11T00:00:00Z".to_string(),
            },
            host_filter: Some("webservers".to_string()),
            contents: BundleContents {
                audit_events: 42,
                state_snapshots: 3,
                drift_reports: 1,
                system_info: true,
            },
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let deserialized: ForensicsBundleManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, deserialized);
    }

    #[test]
    fn test_time_range_serialization() {
        let range = TimeRange {
            from: "2026-01-01T00:00:00Z".to_string(),
            to: "2026-01-02T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&range).unwrap();
        assert!(json.contains("2026-01-01"));
        assert!(json.contains("2026-01-02"));
    }
}
