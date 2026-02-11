//! Timeline export in multiple formats
//!
//! Supports exporting timeline entries as JSON, CSV, or Markdown.

use serde::{Deserialize, Serialize};

use super::timeline::TimelineEntry;

/// Supported export formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// JSON array.
    Json,
    /// Comma-separated values with a header row.
    Csv,
    /// Markdown table.
    Markdown,
}

/// Exporter that serialises timeline entries into a string.
pub struct TimelineExporter;

impl TimelineExporter {
    /// Export the given entries in the requested format.
    pub fn export(entries: &[TimelineEntry], format: &ExportFormat) -> String {
        match format {
            ExportFormat::Json => Self::export_json(entries),
            ExportFormat::Csv => Self::export_csv(entries),
            ExportFormat::Markdown => Self::export_markdown(entries),
        }
    }

    fn export_json(entries: &[TimelineEntry]) -> String {
        serde_json::to_string_pretty(entries).unwrap_or_else(|_| "[]".to_string())
    }

    fn export_csv(entries: &[TimelineEntry]) -> String {
        let mut lines = vec!["timestamp,event_type,resource,details".to_string()];
        for entry in entries {
            let event_type = serde_json::to_value(&entry.event_type)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("{:?}", entry.event_type));
            // Escape double quotes in details for CSV
            let details = entry.details.replace('"', "\"\"");
            lines.push(format!(
                "{},{},\"{}\",\"{}\"",
                entry.timestamp.to_rfc3339(),
                event_type,
                entry.resource,
                details,
            ));
        }
        lines.join("\n")
    }

    fn export_markdown(entries: &[TimelineEntry]) -> String {
        let mut lines = vec![
            "| Timestamp | Event | Resource | Details |".to_string(),
            "|-----------|-------|----------|---------|".to_string(),
        ];
        for entry in entries {
            let event_type = serde_json::to_value(&entry.event_type)
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| format!("{:?}", entry.event_type));
            lines.push(format!(
                "| {} | {} | {} | {} |",
                entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
                event_type,
                entry.resource,
                entry.details,
            ));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drift::history::timeline::TimelineEventType;
    use chrono::TimeZone;

    fn sample_entries() -> Vec<TimelineEntry> {
        let ts = chrono::Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();
        vec![
            TimelineEntry {
                timestamp: ts,
                event_type: TimelineEventType::DriftDetected,
                resource: "nginx@web-01".to_string(),
                details: "severity=high, expected=running, actual=stopped".to_string(),
            },
            TimelineEntry {
                timestamp: ts,
                event_type: TimelineEventType::DriftResolved,
                resource: "sshd@web-01".to_string(),
                details: "Drift no longer detected".to_string(),
            },
        ]
    }

    #[test]
    fn test_export_json() {
        let entries = sample_entries();
        let json = TimelineExporter::export(&entries, &ExportFormat::Json);
        assert!(json.contains("nginx@web-01"));
        assert!(json.contains("drift_detected"));
        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_export_csv() {
        let entries = sample_entries();
        let csv = TimelineExporter::export(&entries, &ExportFormat::Csv);
        let lines: Vec<&str> = csv.lines().collect();
        // Header + 2 data rows
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("timestamp,"));
        assert!(lines[1].contains("nginx@web-01"));
    }

    #[test]
    fn test_export_markdown() {
        let entries = sample_entries();
        let md = TimelineExporter::export(&entries, &ExportFormat::Markdown);
        assert!(md.contains("| Timestamp |"));
        assert!(md.contains("nginx@web-01"));
        assert!(md.contains("sshd@web-01"));
        // Header + separator + 2 data rows = 4 lines
        let lines: Vec<&str> = md.lines().collect();
        assert_eq!(lines.len(), 4);
    }
}
