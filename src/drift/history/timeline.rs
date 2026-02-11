//! Drift timeline construction and filtering
//!
//! Builds a chronological timeline of drift events from the snapshot store,
//! with optional filtering by time range, resource pattern, or event type.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::store::DriftHistoryStore;

/// The kind of event recorded in the timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventType {
    /// A new drift was detected for a resource.
    DriftDetected,
    /// A previously detected drift was resolved.
    DriftResolved,
    /// A drift escalated in severity.
    DriftEscalated,
    /// The baseline was updated (new desired state).
    BaselineUpdated,
}

/// A single entry in the drift timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// What kind of event this is.
    pub event_type: TimelineEventType,
    /// The affected resource identifier.
    pub resource: String,
    /// Human-readable details about the event.
    pub details: String,
}

/// Filter criteria for timeline queries.
#[derive(Debug, Clone, Default)]
pub struct TimelineFilter {
    /// Only include entries at or after this time.
    pub from: Option<DateTime<Utc>>,
    /// Only include entries at or before this time.
    pub to: Option<DateTime<Utc>>,
    /// Only include entries whose resource contains this substring.
    pub resource_pattern: Option<String>,
    /// Only include entries with this event type.
    pub event_type: Option<TimelineEventType>,
}

/// Timeline builder that converts snapshots into a flat event stream.
pub struct DriftTimeline;

impl DriftTimeline {
    /// Build a timeline from all snapshots in the store.
    ///
    /// Each drift item in each snapshot produces a `DriftDetected` event.
    /// If a resource that appeared in an earlier snapshot is absent from a
    /// later one, a `DriftResolved` event is emitted.
    pub fn build_from(store: &DriftHistoryStore) -> Vec<TimelineEntry> {
        let snapshots = store.list_snapshots();
        let mut entries = Vec::new();
        let mut prev_resources: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for snapshot in snapshots {
            let mut current_resources: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            for item in &snapshot.items {
                current_resources.insert(item.resource.clone());
                entries.push(TimelineEntry {
                    timestamp: snapshot.timestamp,
                    event_type: TimelineEventType::DriftDetected,
                    resource: item.resource.clone(),
                    details: format!(
                        "severity={}, expected={}, actual={}",
                        item.severity, item.expected, item.actual
                    ),
                });
            }

            // Resources that were drifted before but are no longer present
            for resolved in prev_resources.difference(&current_resources) {
                entries.push(TimelineEntry {
                    timestamp: snapshot.timestamp,
                    event_type: TimelineEventType::DriftResolved,
                    resource: resolved.clone(),
                    details: "Drift no longer detected".to_string(),
                });
            }

            prev_resources = current_resources;
        }

        entries.sort_by_key(|e| e.timestamp);
        entries
    }

    /// Apply a filter to a set of timeline entries.
    pub fn filter(entries: &[TimelineEntry], filter: &TimelineFilter) -> Vec<TimelineEntry> {
        entries
            .iter()
            .filter(|e| {
                if let Some(ref from) = filter.from {
                    if e.timestamp < *from {
                        return false;
                    }
                }
                if let Some(ref to) = filter.to {
                    if e.timestamp > *to {
                        return false;
                    }
                }
                if let Some(ref pattern) = filter.resource_pattern {
                    if !e.resource.contains(pattern.as_str()) {
                        return false;
                    }
                }
                if let Some(ref evt) = filter.event_type {
                    if e.event_type != *evt {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drift::history::store::{
        DriftHistoryItem, DriftHistoryStore, DriftSnapshot, DriftTrigger,
    };
    use chrono::TimeZone;

    fn sample_store() -> DriftHistoryStore {
        let mut store = DriftHistoryStore::new();
        let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let ts2 = Utc.with_ymd_and_hms(2025, 1, 2, 10, 0, 0).unwrap();

        store.add_snapshot(DriftSnapshot {
            id: "s1".to_string(),
            timestamp: ts1,
            trigger: DriftTrigger::Scheduled,
            items: vec![
                DriftHistoryItem {
                    resource: "nginx@web-01".to_string(),
                    severity: "high".to_string(),
                    expected: "running".to_string(),
                    actual: "stopped".to_string(),
                },
                DriftHistoryItem {
                    resource: "sshd@web-01".to_string(),
                    severity: "medium".to_string(),
                    expected: "enabled".to_string(),
                    actual: "disabled".to_string(),
                },
            ],
        });

        store.add_snapshot(DriftSnapshot {
            id: "s2".to_string(),
            timestamp: ts2,
            trigger: DriftTrigger::Manual,
            items: vec![DriftHistoryItem {
                resource: "nginx@web-01".to_string(),
                severity: "high".to_string(),
                expected: "running".to_string(),
                actual: "stopped".to_string(),
            }],
        });

        store
    }

    #[test]
    fn test_build_timeline_detects_and_resolves() {
        let store = sample_store();
        let entries = DriftTimeline::build_from(&store);

        // s1: 2 detected, s2: 1 detected + 1 resolved (sshd)
        assert_eq!(entries.len(), 4);

        let resolved: Vec<_> = entries
            .iter()
            .filter(|e| e.event_type == TimelineEventType::DriftResolved)
            .collect();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].resource, "sshd@web-01");
    }

    #[test]
    fn test_filter_by_resource_pattern() {
        let store = sample_store();
        let entries = DriftTimeline::build_from(&store);

        let filter = TimelineFilter {
            resource_pattern: Some("sshd".to_string()),
            ..Default::default()
        };

        let filtered = DriftTimeline::filter(&entries, &filter);
        assert_eq!(filtered.len(), 2); // 1 detected + 1 resolved
        assert!(filtered.iter().all(|e| e.resource.contains("sshd")));
    }

    #[test]
    fn test_filter_by_time_range() {
        let store = sample_store();
        let entries = DriftTimeline::build_from(&store);

        let from = Utc.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap();
        let filter = TimelineFilter {
            from: Some(from),
            ..Default::default()
        };

        let filtered = DriftTimeline::filter(&entries, &filter);
        // Only entries from s2 (1 detected + 1 resolved)
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|e| e.timestamp >= from));
    }
}
