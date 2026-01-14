use std::collections::HashMap;

use super::task::{TaskResult, TaskStatus};
use super::Executor;

/// Statistics collected during playbook execution.
///
/// Tracks the count of tasks in each final state across all hosts.
/// Used for generating execution summaries.
///
/// # Example
///
/// ```rust
/// use rustible::executor::ExecutionStats;
///
/// let mut stats = ExecutionStats::default();
/// stats.ok = 5;
/// stats.changed = 3;
/// println!("OK: {}, Changed: {}", stats.ok, stats.changed);
/// ```
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ExecutionStats {
    /// Number of tasks that succeeded without changes.
    pub ok: usize,
    /// Number of tasks that made changes.
    pub changed: usize,
    /// Number of tasks that failed.
    pub failed: usize,
    /// Number of tasks that were skipped (condition not met).
    pub skipped: usize,
    /// Number of tasks that could not run due to unreachable host.
    pub unreachable: usize,
}

impl ExecutionStats {
    /// Merge another set of statistics into this one.
    ///
    /// Adds the counts from `other` to the current statistics.
    pub fn merge(&mut self, other: &ExecutionStats) {
        self.ok += other.ok;
        self.changed += other.changed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.unreachable += other.unreachable;
    }
}

/// Execution result for a single host.
///
/// Contains the aggregated statistics and final state for one host
/// after all tasks have been processed.
#[derive(Debug, Clone)]
pub struct HostResult {
    /// The hostname or identifier.
    pub host: String,
    /// Aggregated task statistics for this host.
    pub stats: ExecutionStats,
    /// Whether any task failed on this host.
    pub failed: bool,
    /// Whether this host became unreachable during execution.
    pub unreachable: bool,
}

pub(super) fn update_stats(stats: &mut ExecutionStats, result: &TaskResult) {
    match result.status {
        TaskStatus::Ok => {
            if result.changed {
                stats.changed += 1;
            } else {
                stats.ok += 1;
            }
        }
        TaskStatus::Changed => stats.changed += 1,
        TaskStatus::Failed => stats.failed += 1,
        TaskStatus::Skipped => stats.skipped += 1,
        TaskStatus::Unreachable => stats.unreachable += 1,
    }
}

impl Executor {
    /// Get execution statistics summary
    pub fn summarize_results(results: &HashMap<String, HostResult>) -> ExecutionStats {
        let mut summary = ExecutionStats::default();
        for result in results.values() {
            summary.merge(&result.stats);
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionStats;

    #[test]
    fn test_execution_stats_merge() {
        let mut stats1 = ExecutionStats {
            ok: 1,
            changed: 2,
            failed: 0,
            skipped: 1,
            unreachable: 0,
        };

        let stats2 = ExecutionStats {
            ok: 2,
            changed: 1,
            failed: 1,
            skipped: 0,
            unreachable: 1,
        };

        stats1.merge(&stats2);

        assert_eq!(stats1.ok, 3);
        assert_eq!(stats1.changed, 3);
        assert_eq!(stats1.failed, 1);
        assert_eq!(stats1.skipped, 1);
        assert_eq!(stats1.unreachable, 1);
    }
}
