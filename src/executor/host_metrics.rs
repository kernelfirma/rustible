//! Per-host task execution metrics and failure summaries.
//!
//! This module provides structured metrics for tracking execution performance
//! on a per-host basis, along with aggregated failure summaries across all hosts.
//!
//! # Overview
//!
//! - [`HostTaskMetrics`] tracks individual host execution statistics including
//!   queue time, run time, retry counts, and per-task timings.
//! - [`FailureSummary`] aggregates failure information across all hosts,
//!   capturing first errors and stderr snippets.
//! - [`MetricsCollector`] provides a convenient interface for accumulating
//!   metrics during playbook execution.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Per-host task execution metrics.
///
/// Tracks detailed timing and outcome data for a single host during
/// playbook execution.
///
/// # Example
///
/// ```rust
/// use rustible::executor::host_metrics::HostTaskMetrics;
/// use std::time::Duration;
///
/// let mut metrics = HostTaskMetrics::new("web-server-01");
/// metrics.record_task("install_nginx", Duration::from_secs(5), true, false, false);
/// metrics.record_task("configure_ssl", Duration::from_secs(2), false, false, false);
///
/// assert_eq!(metrics.total_tasks(), 2);
/// assert!((metrics.success_rate() - 100.0).abs() < f64::EPSILON);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostTaskMetrics {
    /// Host name.
    pub host: String,
    /// Time spent waiting in queue before execution.
    pub queue_time: Duration,
    /// Total execution wall-clock time.
    pub run_time: Duration,
    /// Number of task retries.
    pub retry_count: u32,
    /// Number of SSH reconnection attempts.
    pub reconnect_count: u32,
    /// SSH connection establishment time.
    pub ssh_connect_time: Duration,
    /// Per-task timings (task name -> duration).
    pub task_timings: HashMap<String, Duration>,
    /// Number of tasks completed successfully.
    pub tasks_ok: usize,
    /// Number of tasks that changed state.
    pub tasks_changed: usize,
    /// Number of tasks that failed.
    pub tasks_failed: usize,
    /// Number of tasks skipped.
    pub tasks_skipped: usize,
}

impl HostTaskMetrics {
    /// Create a new `HostTaskMetrics` initialized with zeroes for the given host.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            queue_time: Duration::ZERO,
            run_time: Duration::ZERO,
            retry_count: 0,
            reconnect_count: 0,
            ssh_connect_time: Duration::ZERO,
            task_timings: HashMap::new(),
            tasks_ok: 0,
            tasks_changed: 0,
            tasks_failed: 0,
            tasks_skipped: 0,
        }
    }

    /// Record the outcome of a single task execution.
    ///
    /// Updates the appropriate counters and stores the task timing.
    /// A task that is both `changed` and not `failed`/`skipped` increments
    /// `tasks_changed`. A task that is not changed, not failed, and not
    /// skipped increments `tasks_ok`.
    pub fn record_task(
        &mut self,
        task_name: &str,
        duration: Duration,
        changed: bool,
        failed: bool,
        skipped: bool,
    ) {
        self.task_timings.insert(task_name.to_string(), duration);

        if failed {
            self.tasks_failed += 1;
        } else if skipped {
            self.tasks_skipped += 1;
        } else if changed {
            self.tasks_changed += 1;
        } else {
            self.tasks_ok += 1;
        }
    }

    /// Return the total number of tasks recorded.
    pub fn total_tasks(&self) -> usize {
        self.tasks_ok + self.tasks_changed + self.tasks_failed + self.tasks_skipped
    }

    /// Return the success rate as a percentage (0.0 -- 100.0).
    ///
    /// Success is defined as tasks that were either OK or changed state.
    /// Returns 100.0 if no tasks have been recorded.
    pub fn success_rate(&self) -> f64 {
        let total = self.total_tasks();
        if total == 0 {
            return 100.0;
        }
        ((self.tasks_ok + self.tasks_changed) as f64 / total as f64) * 100.0
    }
}

/// Summary of execution failures across all hosts.
///
/// Provides a high-level view of which hosts succeeded, failed, or were
/// unreachable, along with first-error and stderr details for failed hosts.
///
/// # Example
///
/// ```rust
/// use rustible::executor::host_metrics::FailureSummary;
///
/// let summary = FailureSummary::new();
/// assert!(summary.is_success());
/// assert_eq!(summary.summary_line(), "0/0 ok, 0 failed, 0 unreachable");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureSummary {
    /// Total hosts attempted.
    pub total_hosts: usize,
    /// Hosts that completed successfully.
    pub ok_hosts: usize,
    /// Hosts that had failures.
    pub failed_hosts: usize,
    /// Hosts that were unreachable.
    pub unreachable_hosts: usize,
    /// First error per failed host (host -> error message).
    pub first_errors: HashMap<String, String>,
    /// Last stderr snippet per failed host.
    pub last_stderr: HashMap<String, String>,
}

impl FailureSummary {
    /// Create an empty `FailureSummary` with all counters at zero.
    pub fn new() -> Self {
        Self {
            total_hosts: 0,
            ok_hosts: 0,
            failed_hosts: 0,
            unreachable_hosts: 0,
            first_errors: HashMap::new(),
            last_stderr: HashMap::new(),
        }
    }

    /// Build a `FailureSummary` from a map of host execution results.
    ///
    /// Iterates over every [`super::HostResult`], classifying each host as
    /// OK, failed, or unreachable. For failed hosts the first error message
    /// (derived from the stats) is captured in `first_errors`.
    pub fn from_host_results(results: &HashMap<String, super::HostResult>) -> Self {
        let mut summary = Self::new();
        summary.total_hosts = results.len();

        for (host, result) in results {
            if result.unreachable {
                summary.unreachable_hosts += 1;
                summary
                    .first_errors
                    .insert(host.clone(), "Host unreachable".to_string());
            } else if result.failed {
                summary.failed_hosts += 1;
                let msg = format!(
                    "{} task(s) failed out of {} total",
                    result.stats.failed,
                    result.stats.ok
                        + result.stats.changed
                        + result.stats.failed
                        + result.stats.skipped
                );
                summary.first_errors.insert(host.clone(), msg);
            } else {
                summary.ok_hosts += 1;
            }
        }

        summary
    }

    /// Return a one-line human-readable summary string.
    ///
    /// Format: `"<ok>/<total> ok, <failed> failed, <unreachable> unreachable"`
    pub fn summary_line(&self) -> String {
        format!(
            "{}/{} ok, {} failed, {} unreachable",
            self.ok_hosts, self.total_hosts, self.failed_hosts, self.unreachable_hosts
        )
    }

    /// Return `true` if no hosts failed and none were unreachable.
    pub fn is_success(&self) -> bool {
        self.failed_hosts == 0 && self.unreachable_hosts == 0
    }
}

impl Default for FailureSummary {
    fn default() -> Self {
        Self::new()
    }
}

/// Collects per-host metrics during playbook execution.
///
/// Provides a centralized store for [`HostTaskMetrics`] indexed by hostname,
/// with helper methods for creating metrics on first access and generating
/// JSON-friendly summaries.
///
/// # Example
///
/// ```rust
/// use rustible::executor::host_metrics::MetricsCollector;
/// use std::time::Duration;
///
/// let mut collector = MetricsCollector::new();
/// let metrics = collector.get_or_create("db-server-01");
/// metrics.record_task("backup", Duration::from_secs(30), true, false, false);
///
/// let summary = collector.summary();
/// assert!(summary.contains_key("db-server-01"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct MetricsCollector {
    host_metrics: HashMap<String, HostTaskMetrics>,
}

impl MetricsCollector {
    /// Create a new empty `MetricsCollector`.
    pub fn new() -> Self {
        Self {
            host_metrics: HashMap::new(),
        }
    }

    /// Return a mutable reference to the metrics for `host`, creating a new
    /// entry if one does not already exist.
    pub fn get_or_create(&mut self, host: &str) -> &mut HostTaskMetrics {
        self.host_metrics
            .entry(host.to_string())
            .or_insert_with(|| HostTaskMetrics::new(host))
    }

    /// Return a reference to all collected host metrics.
    pub fn all_metrics(&self) -> &HashMap<String, HostTaskMetrics> {
        &self.host_metrics
    }

    /// Return a JSON-friendly summary of all host metrics.
    ///
    /// Each host entry includes run time (in milliseconds), task outcome
    /// counts, success rate, and retry/reconnect counts.
    pub fn summary(&self) -> HashMap<String, serde_json::Value> {
        let mut result = HashMap::new();
        for (host, metrics) in &self.host_metrics {
            result.insert(
                host.clone(),
                serde_json::json!({
                    "run_time_ms": metrics.run_time.as_millis(),
                    "tasks_ok": metrics.tasks_ok,
                    "tasks_changed": metrics.tasks_changed,
                    "tasks_failed": metrics.tasks_failed,
                    "tasks_skipped": metrics.tasks_skipped,
                    "success_rate": metrics.success_rate(),
                    "retry_count": metrics.retry_count,
                    "reconnect_count": metrics.reconnect_count,
                }),
            );
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_task_metrics_new() {
        let metrics = HostTaskMetrics::new("test-host");
        assert_eq!(metrics.host, "test-host");
        assert_eq!(metrics.queue_time, Duration::ZERO);
        assert_eq!(metrics.run_time, Duration::ZERO);
        assert_eq!(metrics.retry_count, 0);
        assert_eq!(metrics.reconnect_count, 0);
        assert_eq!(metrics.ssh_connect_time, Duration::ZERO);
        assert!(metrics.task_timings.is_empty());
        assert_eq!(metrics.tasks_ok, 0);
        assert_eq!(metrics.tasks_changed, 0);
        assert_eq!(metrics.tasks_failed, 0);
        assert_eq!(metrics.tasks_skipped, 0);
    }

    #[test]
    fn test_record_task_ok() {
        let mut metrics = HostTaskMetrics::new("host1");
        metrics.record_task("task1", Duration::from_secs(1), false, false, false);
        assert_eq!(metrics.tasks_ok, 1);
        assert_eq!(metrics.tasks_changed, 0);
        assert_eq!(metrics.tasks_failed, 0);
        assert_eq!(metrics.tasks_skipped, 0);
        assert_eq!(metrics.total_tasks(), 1);
        assert_eq!(
            *metrics.task_timings.get("task1").unwrap(),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn test_record_task_changed() {
        let mut metrics = HostTaskMetrics::new("host1");
        metrics.record_task("task1", Duration::from_secs(2), true, false, false);
        assert_eq!(metrics.tasks_ok, 0);
        assert_eq!(metrics.tasks_changed, 1);
        assert_eq!(metrics.total_tasks(), 1);
    }

    #[test]
    fn test_record_task_failed() {
        let mut metrics = HostTaskMetrics::new("host1");
        metrics.record_task("task1", Duration::from_secs(3), false, true, false);
        assert_eq!(metrics.tasks_failed, 1);
        assert_eq!(metrics.tasks_ok, 0);
        assert_eq!(metrics.total_tasks(), 1);
    }

    #[test]
    fn test_record_task_skipped() {
        let mut metrics = HostTaskMetrics::new("host1");
        metrics.record_task("task1", Duration::from_secs(0), false, false, true);
        assert_eq!(metrics.tasks_skipped, 1);
        assert_eq!(metrics.total_tasks(), 1);
    }

    #[test]
    fn test_record_task_failed_takes_priority() {
        // If both `changed` and `failed` are true, failed wins
        let mut metrics = HostTaskMetrics::new("host1");
        metrics.record_task("task1", Duration::from_secs(1), true, true, false);
        assert_eq!(metrics.tasks_failed, 1);
        assert_eq!(metrics.tasks_changed, 0);
    }

    #[test]
    fn test_total_tasks() {
        let mut metrics = HostTaskMetrics::new("host1");
        metrics.record_task("t1", Duration::from_millis(100), false, false, false);
        metrics.record_task("t2", Duration::from_millis(200), true, false, false);
        metrics.record_task("t3", Duration::from_millis(50), false, true, false);
        metrics.record_task("t4", Duration::from_millis(0), false, false, true);
        assert_eq!(metrics.total_tasks(), 4);
    }

    #[test]
    fn test_success_rate_all_ok() {
        let mut metrics = HostTaskMetrics::new("host1");
        metrics.record_task("t1", Duration::from_secs(1), false, false, false);
        metrics.record_task("t2", Duration::from_secs(1), true, false, false);
        assert!((metrics.success_rate() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_with_failures() {
        let mut metrics = HostTaskMetrics::new("host1");
        metrics.record_task("t1", Duration::from_secs(1), false, false, false);
        metrics.record_task("t2", Duration::from_secs(1), false, true, false);
        // 1 ok / 2 total = 50%
        assert!((metrics.success_rate() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_no_tasks() {
        let metrics = HostTaskMetrics::new("host1");
        assert!((metrics.success_rate() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_failure_summary_new() {
        let summary = FailureSummary::new();
        assert_eq!(summary.total_hosts, 0);
        assert_eq!(summary.ok_hosts, 0);
        assert_eq!(summary.failed_hosts, 0);
        assert_eq!(summary.unreachable_hosts, 0);
        assert!(summary.first_errors.is_empty());
        assert!(summary.last_stderr.is_empty());
        assert!(summary.is_success());
    }

    #[test]
    fn test_failure_summary_from_host_results() {
        let mut results = HashMap::new();

        results.insert(
            "host-ok".to_string(),
            super::super::HostResult {
                host: "host-ok".to_string(),
                stats: super::super::ExecutionStats {
                    ok: 5,
                    changed: 2,
                    failed: 0,
                    skipped: 1,
                    unreachable: 0,
                },
                failed: false,
                unreachable: false,
            },
        );

        results.insert(
            "host-fail".to_string(),
            super::super::HostResult {
                host: "host-fail".to_string(),
                stats: super::super::ExecutionStats {
                    ok: 3,
                    changed: 1,
                    failed: 2,
                    skipped: 0,
                    unreachable: 0,
                },
                failed: true,
                unreachable: false,
            },
        );

        results.insert(
            "host-down".to_string(),
            super::super::HostResult {
                host: "host-down".to_string(),
                stats: super::super::ExecutionStats {
                    ok: 0,
                    changed: 0,
                    failed: 0,
                    skipped: 0,
                    unreachable: 1,
                },
                failed: false,
                unreachable: true,
            },
        );

        let summary = FailureSummary::from_host_results(&results);
        assert_eq!(summary.total_hosts, 3);
        assert_eq!(summary.ok_hosts, 1);
        assert_eq!(summary.failed_hosts, 1);
        assert_eq!(summary.unreachable_hosts, 1);
        assert!(!summary.is_success());
        assert!(summary.first_errors.contains_key("host-fail"));
        assert!(summary.first_errors.contains_key("host-down"));
    }

    #[test]
    fn test_failure_summary_summary_line() {
        let mut summary = FailureSummary::new();
        summary.total_hosts = 500;
        summary.ok_hosts = 487;
        summary.failed_hosts = 8;
        summary.unreachable_hosts = 5;
        assert_eq!(
            summary.summary_line(),
            "487/500 ok, 8 failed, 5 unreachable"
        );
    }

    #[test]
    fn test_failure_summary_is_success() {
        let mut summary = FailureSummary::new();
        summary.total_hosts = 10;
        summary.ok_hosts = 10;
        assert!(summary.is_success());

        summary.failed_hosts = 1;
        assert!(!summary.is_success());
    }

    #[test]
    fn test_metrics_collector_new() {
        let collector = MetricsCollector::new();
        assert!(collector.all_metrics().is_empty());
    }

    #[test]
    fn test_metrics_collector_get_or_create() {
        let mut collector = MetricsCollector::new();
        {
            let metrics = collector.get_or_create("host1");
            metrics.record_task("task1", Duration::from_secs(1), false, false, false);
        }
        assert_eq!(collector.all_metrics().len(), 1);
        assert_eq!(collector.all_metrics()["host1"].tasks_ok, 1);

        // Access again should return the same entry
        {
            let metrics = collector.get_or_create("host1");
            assert_eq!(metrics.tasks_ok, 1);
        }
    }

    #[test]
    fn test_metrics_collector_summary() {
        let mut collector = MetricsCollector::new();
        {
            let metrics = collector.get_or_create("host1");
            metrics.run_time = Duration::from_millis(1500);
            metrics.record_task("t1", Duration::from_secs(1), false, false, false);
            metrics.record_task("t2", Duration::from_secs(1), true, false, false);
        }

        let summary = collector.summary();
        assert!(summary.contains_key("host1"));
        let host_summary = &summary["host1"];
        assert_eq!(host_summary["run_time_ms"], 1500);
        assert_eq!(host_summary["tasks_ok"], 1);
        assert_eq!(host_summary["tasks_changed"], 1);
        assert_eq!(host_summary["tasks_failed"], 0);
        assert_eq!(host_summary["tasks_skipped"], 0);
        assert_eq!(host_summary["success_rate"], 100.0);
        assert_eq!(host_summary["retry_count"], 0);
        assert_eq!(host_summary["reconnect_count"], 0);
    }
}
