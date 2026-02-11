//! Drift snapshot storage
//!
//! Provides an in-memory store for drift detection snapshots, allowing
//! callers to record, list, and retrieve historical drift data.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What triggered a drift detection scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftTrigger {
    /// Triggered by a cron / periodic schedule.
    Scheduled,
    /// Triggered manually by an operator.
    Manual,
    /// Triggered by an external event (webhook, file watch, etc.).
    EventDriven,
    /// Triggered from within a CI/CD pipeline.
    CiPipeline,
}

/// A single resource entry inside a drift snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftHistoryItem {
    /// Resource identifier (e.g. "nginx@web-01").
    pub resource: String,
    /// Severity label (critical, high, medium, low).
    pub severity: String,
    /// Expected value or state.
    pub expected: String,
    /// Actual observed value or state.
    pub actual: String,
}

/// A point-in-time snapshot of detected drift.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftSnapshot {
    /// Unique snapshot identifier.
    pub id: String,
    /// When the snapshot was captured.
    pub timestamp: DateTime<Utc>,
    /// What triggered the scan.
    pub trigger: DriftTrigger,
    /// Individual drift items detected in this scan.
    pub items: Vec<DriftHistoryItem>,
}

/// In-memory store of drift snapshots.
#[derive(Debug, Default)]
pub struct DriftHistoryStore {
    snapshots: Vec<DriftSnapshot>,
}

impl DriftHistoryStore {
    /// Create a new empty history store.
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
        }
    }

    /// Record a new snapshot.
    pub fn add_snapshot(&mut self, snapshot: DriftSnapshot) {
        self.snapshots.push(snapshot);
    }

    /// Get a snapshot by its id.
    pub fn get_snapshot(&self, id: &str) -> Option<&DriftSnapshot> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    /// List all snapshots in chronological order.
    pub fn list_snapshots(&self) -> &[DriftSnapshot] {
        &self.snapshots
    }

    /// Return the most recent snapshot, if any.
    pub fn latest(&self) -> Option<&DriftSnapshot> {
        self.snapshots.iter().max_by_key(|s| s.timestamp)
    }

    /// Remove all snapshots whose timestamp is strictly before `before`.
    pub fn prune_before(&mut self, before: DateTime<Utc>) -> usize {
        let original_len = self.snapshots.len();
        self.snapshots.retain(|s| s.timestamp >= before);
        original_len - self.snapshots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_snapshot(id: &str, ts: DateTime<Utc>) -> DriftSnapshot {
        DriftSnapshot {
            id: id.to_string(),
            timestamp: ts,
            trigger: DriftTrigger::Manual,
            items: vec![DriftHistoryItem {
                resource: "nginx@web-01".to_string(),
                severity: "high".to_string(),
                expected: "running".to_string(),
                actual: "stopped".to_string(),
            }],
        }
    }

    #[test]
    fn test_add_and_list_snapshots() {
        let mut store = DriftHistoryStore::new();
        assert!(store.list_snapshots().is_empty());

        let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let ts2 = Utc.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap();
        store.add_snapshot(make_snapshot("snap-1", ts1));
        store.add_snapshot(make_snapshot("snap-2", ts2));

        assert_eq!(store.list_snapshots().len(), 2);
        assert_eq!(store.get_snapshot("snap-1").unwrap().id, "snap-1");
        assert!(store.get_snapshot("nonexistent").is_none());
    }

    #[test]
    fn test_latest_returns_most_recent() {
        let mut store = DriftHistoryStore::new();
        let ts1 = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();
        let ts2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

        // Insert out of order
        store.add_snapshot(make_snapshot("snap-old", ts2));
        store.add_snapshot(make_snapshot("snap-new", ts1));

        let latest = store.latest().unwrap();
        assert_eq!(latest.id, "snap-new");
    }

    #[test]
    fn test_prune_before() {
        let mut store = DriftHistoryStore::new();
        let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let ts2 = Utc.with_ymd_and_hms(2025, 6, 1, 0, 0, 0).unwrap();
        store.add_snapshot(make_snapshot("old", ts1));
        store.add_snapshot(make_snapshot("new", ts2));

        let cutoff = Utc.with_ymd_and_hms(2025, 3, 1, 0, 0, 0).unwrap();
        let pruned = store.prune_before(cutoff);
        assert_eq!(pruned, 1);
        assert_eq!(store.list_snapshots().len(), 1);
        assert_eq!(store.list_snapshots()[0].id, "new");
    }
}
