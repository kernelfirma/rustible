//! Drift correlation analysis
//!
//! Analyses a set of drift snapshots to find resources that repeatedly drift,
//! compute frequency statistics, and suggest probable causes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::store::DriftSnapshot;

/// Probable root cause of recurring drift for a resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbableCause {
    /// Drift likely caused by a configuration management change.
    ConfigChange,
    /// Drift likely caused by a package update.
    PackageUpdate,
    /// Drift likely caused by a service restart.
    ServiceRestart,
    /// Drift likely caused by an external / out-of-band modification.
    ExternalModification,
    /// Unable to determine cause.
    Unknown,
}

/// Summary of a resource's drift across multiple snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationResult {
    /// Resource identifier.
    pub resource: String,
    /// How many snapshots contained drift for this resource.
    pub frequency: usize,
    /// Earliest time this resource was seen drifting.
    pub first_seen: DateTime<Utc>,
    /// Most recent time this resource was seen drifting.
    pub last_seen: DateTime<Utc>,
    /// Heuristic guess at the probable cause.
    pub probable_cause: ProbableCause,
}

/// Correlator that analyses snapshots for recurring drift patterns.
pub struct DriftCorrelator;

impl DriftCorrelator {
    /// Analyse the given snapshots and produce a correlation result per resource.
    ///
    /// The probable cause is inferred via a simple heuristic:
    /// - If the resource name contains "package" or drift severity is about versions, guess `PackageUpdate`.
    /// - If the resource name contains "service", guess `ServiceRestart`.
    /// - If frequency is 1 (one-off), guess `ExternalModification`.
    /// - Otherwise `Unknown`.
    pub fn correlate(snapshots: &[DriftSnapshot]) -> Vec<CorrelationResult> {
        // resource -> (count, first_seen, last_seen, severities)
        let mut map: HashMap<String, (usize, DateTime<Utc>, DateTime<Utc>, Vec<String>)> =
            HashMap::new();

        for snapshot in snapshots {
            for item in &snapshot.items {
                let entry = map
                    .entry(item.resource.clone())
                    .or_insert_with(|| (0, snapshot.timestamp, snapshot.timestamp, Vec::new()));
                entry.0 += 1;
                if snapshot.timestamp < entry.1 {
                    entry.1 = snapshot.timestamp;
                }
                if snapshot.timestamp > entry.2 {
                    entry.2 = snapshot.timestamp;
                }
                entry.3.push(item.severity.clone());
            }
        }

        let mut results: Vec<CorrelationResult> = map
            .into_iter()
            .map(
                |(resource, (frequency, first_seen, last_seen, severities))| {
                    let probable_cause = Self::guess_cause(&resource, frequency, &severities);
                    CorrelationResult {
                        resource,
                        frequency,
                        first_seen,
                        last_seen,
                        probable_cause,
                    }
                },
            )
            .collect();

        // Sort by frequency descending so the most problematic resources appear first.
        results.sort_by(|a, b| b.frequency.cmp(&a.frequency));
        results
    }

    fn guess_cause(resource: &str, frequency: usize, _severities: &[String]) -> ProbableCause {
        let lower = resource.to_lowercase();
        if lower.contains("package") || lower.contains("version") {
            ProbableCause::PackageUpdate
        } else if lower.contains("service") || lower.contains("systemd") {
            ProbableCause::ServiceRestart
        } else if lower.contains("config") || lower.contains("conf") {
            ProbableCause::ConfigChange
        } else if frequency <= 1 {
            ProbableCause::ExternalModification
        } else {
            ProbableCause::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drift::history::store::{DriftHistoryItem, DriftSnapshot, DriftTrigger};
    use chrono::TimeZone;

    #[test]
    fn test_correlate_frequency_and_cause() {
        let ts1 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let ts2 = Utc.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap();
        let ts3 = Utc.with_ymd_and_hms(2025, 1, 3, 0, 0, 0).unwrap();

        let snapshots = vec![
            DriftSnapshot {
                id: "s1".to_string(),
                timestamp: ts1,
                trigger: DriftTrigger::Scheduled,
                items: vec![
                    DriftHistoryItem {
                        resource: "nginx-service@web-01".to_string(),
                        severity: "high".to_string(),
                        expected: "running".to_string(),
                        actual: "stopped".to_string(),
                    },
                    DriftHistoryItem {
                        resource: "openssl-package@web-01".to_string(),
                        severity: "medium".to_string(),
                        expected: "1.1.1".to_string(),
                        actual: "1.1.2".to_string(),
                    },
                ],
            },
            DriftSnapshot {
                id: "s2".to_string(),
                timestamp: ts2,
                trigger: DriftTrigger::Manual,
                items: vec![DriftHistoryItem {
                    resource: "nginx-service@web-01".to_string(),
                    severity: "high".to_string(),
                    expected: "running".to_string(),
                    actual: "stopped".to_string(),
                }],
            },
            DriftSnapshot {
                id: "s3".to_string(),
                timestamp: ts3,
                trigger: DriftTrigger::CiPipeline,
                items: vec![DriftHistoryItem {
                    resource: "nginx-service@web-01".to_string(),
                    severity: "high".to_string(),
                    expected: "running".to_string(),
                    actual: "stopped".to_string(),
                }],
            },
        ];

        let results = DriftCorrelator::correlate(&snapshots);

        // nginx-service appears 3 times, openssl-package 1 time
        assert_eq!(results.len(), 2);

        let nginx = results
            .iter()
            .find(|r| r.resource.contains("nginx"))
            .unwrap();
        assert_eq!(nginx.frequency, 3);
        assert_eq!(nginx.first_seen, ts1);
        assert_eq!(nginx.last_seen, ts3);
        assert_eq!(nginx.probable_cause, ProbableCause::ServiceRestart);

        let openssl = results
            .iter()
            .find(|r| r.resource.contains("openssl"))
            .unwrap();
        assert_eq!(openssl.frequency, 1);
        assert_eq!(openssl.probable_cause, ProbableCause::PackageUpdate);
    }
}
