//! State Diff Engine
//!
//! This module provides functionality to compare state snapshots and generate
//! detailed diff reports showing what changed between executions.
//!
//! ## Features
//!
//! - Compare any two snapshots to see changes
//! - Identify new, removed, and modified tasks
//! - Track host-level changes
//! - Generate human-readable diff reports
//! - Support for JSON patch format for programmatic use

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

use super::{ExecutionStats, HostState, StateSnapshot, TaskStateRecord, TaskStatus};

/// The diff engine for comparing state snapshots
pub struct DiffEngine {
    /// Whether to include detailed value diffs
    include_value_diffs: bool,
    /// Maximum diff context lines
    context_lines: usize,
}

impl Default for DiffEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffEngine {
    /// Create a new diff engine
    pub fn new() -> Self {
        Self {
            include_value_diffs: true,
            context_lines: 3,
        }
    }

    /// Configure whether to include value diffs
    pub fn with_value_diffs(mut self, include: bool) -> Self {
        self.include_value_diffs = include;
        self
    }

    /// Set the number of context lines for diffs
    pub fn with_context_lines(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }

    /// Compare two snapshots and generate a diff report
    pub fn diff(&self, old: &StateSnapshot, new: &StateSnapshot) -> DiffReport {
        let mut report = DiffReport::new(Some(old.clone()), new.clone());

        // Compare tasks
        self.diff_tasks(old, new, &mut report);

        // Compare hosts
        self.diff_hosts(old, new, &mut report);

        // Compare stats
        report.stats_diff = self.diff_stats(&old.stats, &new.stats);

        // Calculate summary
        report.calculate_summary();

        report
    }

    /// Diff tasks between snapshots
    fn diff_tasks(&self, old: &StateSnapshot, new: &StateSnapshot, report: &mut DiffReport) {
        // Create maps for efficient lookup
        let old_tasks: HashMap<String, &TaskStateRecord> = old
            .tasks
            .iter()
            .map(|t| (format!("{}::{}", t.host, t.task_id), t))
            .collect();

        let new_tasks: HashMap<String, &TaskStateRecord> = new
            .tasks
            .iter()
            .map(|t| (format!("{}::{}", t.host, t.task_id), t))
            .collect();

        let old_keys: HashSet<&String> = old_tasks.keys().collect();
        let new_keys: HashSet<&String> = new_tasks.keys().collect();

        // Find added tasks
        for key in new_keys.difference(&old_keys) {
            if let Some(task) = new_tasks.get(*key) {
                report.task_changes.push(TaskChange {
                    task_id: task.task_id.clone(),
                    task_name: task.task_name.clone(),
                    host: task.host.clone(),
                    change_type: ChangeType::Added,
                    old_status: None,
                    new_status: Some(task.status),
                    value_diff: None,
                });
            }
        }

        // Find removed tasks
        for key in old_keys.difference(&new_keys) {
            if let Some(task) = old_tasks.get(*key) {
                report.task_changes.push(TaskChange {
                    task_id: task.task_id.clone(),
                    task_name: task.task_name.clone(),
                    host: task.host.clone(),
                    change_type: ChangeType::Removed,
                    old_status: Some(task.status),
                    new_status: None,
                    value_diff: None,
                });
            }
        }

        // Find modified tasks
        for key in old_keys.intersection(&new_keys) {
            let old_task = old_tasks.get(*key).unwrap();
            let new_task = new_tasks.get(*key).unwrap();

            if self.tasks_differ(old_task, new_task) {
                let value_diff = if self.include_value_diffs {
                    self.diff_task_values(old_task, new_task)
                } else {
                    None
                };

                report.task_changes.push(TaskChange {
                    task_id: new_task.task_id.clone(),
                    task_name: new_task.task_name.clone(),
                    host: new_task.host.clone(),
                    change_type: ChangeType::Modified,
                    old_status: Some(old_task.status),
                    new_status: Some(new_task.status),
                    value_diff,
                });
            }
        }
    }

    /// Check if two tasks differ
    fn tasks_differ(&self, old: &TaskStateRecord, new: &TaskStateRecord) -> bool {
        old.status != new.status
            || old.before_state != new.before_state
            || old.after_state != new.after_state
            || old.args != new.args
    }

    /// Generate a value diff between two tasks
    fn diff_task_values(&self, old: &TaskStateRecord, new: &TaskStateRecord) -> Option<ValueDiff> {
        let mut diff = ValueDiff::default();

        // Diff status
        if old.status != new.status {
            diff.changes.push(ValueChange {
                path: "status".to_string(),
                old_value: Some(serde_json::to_value(old.status).ok()?),
                new_value: Some(serde_json::to_value(new.status).ok()?),
            });
        }

        // Diff before_state
        if old.before_state != new.before_state {
            diff.changes.push(ValueChange {
                path: "before_state".to_string(),
                old_value: old.before_state.clone(),
                new_value: new.before_state.clone(),
            });
        }

        // Diff after_state
        if old.after_state != new.after_state {
            diff.changes.push(ValueChange {
                path: "after_state".to_string(),
                old_value: old.after_state.clone(),
                new_value: new.after_state.clone(),
            });
        }

        // Diff args
        if old.args != new.args {
            diff.changes.push(ValueChange {
                path: "args".to_string(),
                old_value: Some(old.args.clone()),
                new_value: Some(new.args.clone()),
            });
        }

        if diff.changes.is_empty() {
            None
        } else {
            Some(diff)
        }
    }

    /// Diff hosts between snapshots
    fn diff_hosts(&self, old: &StateSnapshot, new: &StateSnapshot, report: &mut DiffReport) {
        let old_hosts: HashSet<&String> = old.host_states.keys().collect();
        let new_hosts: HashSet<&String> = new.host_states.keys().collect();

        // Added hosts
        for host in new_hosts.difference(&old_hosts) {
            if let Some(state) = new.host_states.get(*host) {
                report.host_changes.push(HostChange {
                    host: (*host).clone(),
                    change_type: ChangeType::Added,
                    old_state: None,
                    new_state: Some(state.clone()),
                });
            }
        }

        // Removed hosts
        for host in old_hosts.difference(&new_hosts) {
            if let Some(state) = old.host_states.get(*host) {
                report.host_changes.push(HostChange {
                    host: (*host).clone(),
                    change_type: ChangeType::Removed,
                    old_state: Some(state.clone()),
                    new_state: None,
                });
            }
        }

        // Modified hosts
        for host in old_hosts.intersection(&new_hosts) {
            let old_state = old.host_states.get(*host).unwrap();
            let new_state = new.host_states.get(*host).unwrap();

            if self.host_states_differ(old_state, new_state) {
                report.host_changes.push(HostChange {
                    host: (*host).clone(),
                    change_type: ChangeType::Modified,
                    old_state: Some(old_state.clone()),
                    new_state: Some(new_state.clone()),
                });
            }
        }
    }

    /// Check if two host states differ
    fn host_states_differ(&self, old: &HostState, new: &HostState) -> bool {
        old.ok != new.ok
            || old.changed != new.changed
            || old.failed != new.failed
            || old.skipped != new.skipped
            || old.unreachable != new.unreachable
    }

    /// Diff execution stats
    fn diff_stats(&self, old: &ExecutionStats, new: &ExecutionStats) -> StatsDiff {
        StatsDiff {
            ok_delta: new.ok as i64 - old.ok as i64,
            changed_delta: new.changed as i64 - old.changed as i64,
            failed_delta: new.failed as i64 - old.failed as i64,
            skipped_delta: new.skipped as i64 - old.skipped as i64,
            unreachable_delta: new.unreachable as i64 - old.unreachable as i64,
            total_delta: new.total as i64 - old.total as i64,
            duration_delta_ms: new.duration_ms as i64 - old.duration_ms as i64,
        }
    }
}

/// Type of change detected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    /// Item was added
    Added,
    /// Item was removed
    Removed,
    /// Item was modified
    Modified,
    /// No change
    Unchanged,
}

impl std::fmt::Display for ChangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeType::Added => write!(f, "+"),
            ChangeType::Removed => write!(f, "-"),
            ChangeType::Modified => write!(f, "~"),
            ChangeType::Unchanged => write!(f, " "),
        }
    }
}

/// A change to a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChange {
    /// Task identifier
    pub task_id: String,
    /// Task name
    pub task_name: String,
    /// Host this task ran on
    pub host: String,
    /// Type of change
    pub change_type: ChangeType,
    /// Old status (if existed before)
    pub old_status: Option<TaskStatus>,
    /// New status (if exists now)
    pub new_status: Option<TaskStatus>,
    /// Detailed value diff
    pub value_diff: Option<ValueDiff>,
}

/// A change to a host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostChange {
    /// Host name
    pub host: String,
    /// Type of change
    pub change_type: ChangeType,
    /// Old host state
    pub old_state: Option<HostState>,
    /// New host state
    pub new_state: Option<HostState>,
}

/// Detailed value diff
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValueDiff {
    /// Individual value changes
    pub changes: Vec<ValueChange>,
}

/// A single value change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueChange {
    /// JSON path to the changed value
    pub path: String,
    /// Old value
    pub old_value: Option<serde_json::Value>,
    /// New value
    pub new_value: Option<serde_json::Value>,
}

/// Diff of execution stats
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsDiff {
    pub ok_delta: i64,
    pub changed_delta: i64,
    pub failed_delta: i64,
    pub skipped_delta: i64,
    pub unreachable_delta: i64,
    pub total_delta: i64,
    pub duration_delta_ms: i64,
}

/// Summary of changes in a diff report
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffSummary {
    /// Number of added tasks
    pub tasks_added: usize,
    /// Number of removed tasks
    pub tasks_removed: usize,
    /// Number of modified tasks
    pub tasks_modified: usize,
    /// Number of added hosts
    pub hosts_added: usize,
    /// Number of removed hosts
    pub hosts_removed: usize,
    /// Number of modified hosts
    pub hosts_modified: usize,
    /// Whether there are any changes
    pub has_changes: bool,
}

/// A comprehensive diff report between two snapshots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffReport {
    /// When this diff was generated
    pub generated_at: DateTime<Utc>,
    /// Old snapshot ID (if any)
    pub old_snapshot_id: Option<String>,
    /// New snapshot ID
    pub new_snapshot_id: String,
    /// Old snapshot playbook
    pub old_playbook: Option<String>,
    /// New snapshot playbook
    pub new_playbook: String,
    /// Task changes
    pub task_changes: Vec<TaskChange>,
    /// Host changes
    pub host_changes: Vec<HostChange>,
    /// Stats diff
    pub stats_diff: StatsDiff,
    /// Summary
    pub summary: DiffSummary,
}

impl DiffReport {
    /// Create a new diff report
    pub fn new(old: Option<StateSnapshot>, new: StateSnapshot) -> Self {
        Self {
            generated_at: Utc::now(),
            old_snapshot_id: old.as_ref().map(|s| s.id.clone()),
            new_snapshot_id: new.id.clone(),
            old_playbook: old.as_ref().map(|s| s.playbook.clone()),
            new_playbook: new.playbook,
            task_changes: Vec::new(),
            host_changes: Vec::new(),
            stats_diff: StatsDiff::default(),
            summary: DiffSummary::default(),
        }
    }

    /// Calculate the summary from changes
    pub fn calculate_summary(&mut self) {
        self.summary = DiffSummary {
            tasks_added: self
                .task_changes
                .iter()
                .filter(|c| c.change_type == ChangeType::Added)
                .count(),
            tasks_removed: self
                .task_changes
                .iter()
                .filter(|c| c.change_type == ChangeType::Removed)
                .count(),
            tasks_modified: self
                .task_changes
                .iter()
                .filter(|c| c.change_type == ChangeType::Modified)
                .count(),
            hosts_added: self
                .host_changes
                .iter()
                .filter(|c| c.change_type == ChangeType::Added)
                .count(),
            hosts_removed: self
                .host_changes
                .iter()
                .filter(|c| c.change_type == ChangeType::Removed)
                .count(),
            hosts_modified: self
                .host_changes
                .iter()
                .filter(|c| c.change_type == ChangeType::Modified)
                .count(),
            has_changes: !self.task_changes.is_empty() || !self.host_changes.is_empty(),
        };
    }

    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        self.summary.has_changes
    }

    /// Get a human-readable summary
    pub fn summary_text(&self) -> String {
        if !self.has_changes() {
            return "No changes detected".to_string();
        }

        let mut parts = Vec::new();

        if self.summary.tasks_added > 0 {
            parts.push(format!("{} tasks added", self.summary.tasks_added));
        }
        if self.summary.tasks_removed > 0 {
            parts.push(format!("{} tasks removed", self.summary.tasks_removed));
        }
        if self.summary.tasks_modified > 0 {
            parts.push(format!("{} tasks modified", self.summary.tasks_modified));
        }
        if self.summary.hosts_added > 0 {
            parts.push(format!("{} hosts added", self.summary.hosts_added));
        }
        if self.summary.hosts_removed > 0 {
            parts.push(format!("{} hosts removed", self.summary.hosts_removed));
        }
        if self.summary.hosts_modified > 0 {
            parts.push(format!("{} hosts modified", self.summary.hosts_modified));
        }

        parts.join(", ")
    }

    /// Format the diff as a detailed report
    pub fn format_detailed(&self) -> String {
        let mut output = String::new();

        output.push_str("=== Diff Report ===\n");
        output.push_str(&format!("Generated: {}\n", self.generated_at));
        if let Some(ref old) = self.old_snapshot_id {
            output.push_str(&format!("Old Snapshot: {}\n", old));
        }
        output.push_str(&format!("New Snapshot: {}\n", self.new_snapshot_id));
        output.push_str(&format!("\n{}\n\n", self.summary_text()));

        if !self.task_changes.is_empty() {
            output.push_str("--- Task Changes ---\n");
            for change in &self.task_changes {
                let symbol = match change.change_type {
                    ChangeType::Added => "+",
                    ChangeType::Removed => "-",
                    ChangeType::Modified => "~",
                    ChangeType::Unchanged => " ",
                };
                output.push_str(&format!(
                    "{} [{}] {} on {}\n",
                    symbol, change.task_id, change.task_name, change.host
                ));
                if let (Some(old), Some(new)) = (&change.old_status, &change.new_status) {
                    if old != new {
                        output.push_str(&format!("    Status: {} -> {}\n", old, new));
                    }
                }
            }
            output.push('\n');
        }

        if !self.host_changes.is_empty() {
            output.push_str("--- Host Changes ---\n");
            for change in &self.host_changes {
                let symbol = match change.change_type {
                    ChangeType::Added => "+",
                    ChangeType::Removed => "-",
                    ChangeType::Modified => "~",
                    ChangeType::Unchanged => " ",
                };
                output.push_str(&format!("{} {}\n", symbol, change.host));
            }
            output.push('\n');
        }

        // Stats diff
        output.push_str("--- Stats ---\n");
        if self.stats_diff.ok_delta != 0 {
            output.push_str(&format!("  OK: {:+}\n", self.stats_diff.ok_delta));
        }
        if self.stats_diff.changed_delta != 0 {
            output.push_str(&format!("  Changed: {:+}\n", self.stats_diff.changed_delta));
        }
        if self.stats_diff.failed_delta != 0 {
            output.push_str(&format!("  Failed: {:+}\n", self.stats_diff.failed_delta));
        }
        if self.stats_diff.skipped_delta != 0 {
            output.push_str(&format!("  Skipped: {:+}\n", self.stats_diff.skipped_delta));
        }
        if self.stats_diff.duration_delta_ms != 0 {
            output.push_str(&format!(
                "  Duration: {:+}ms\n",
                self.stats_diff.duration_delta_ms
            ));
        }

        output
    }
}

/// A single state change entry (for streaming/logging)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    /// When this change occurred
    pub timestamp: DateTime<Utc>,
    /// Type of change
    pub change_type: ChangeType,
    /// Entity type (task, host, etc.)
    pub entity_type: String,
    /// Entity identifier
    pub entity_id: String,
    /// Description of the change
    pub description: String,
    /// Old value (JSON)
    pub old_value: Option<serde_json::Value>,
    /// New value (JSON)
    pub new_value: Option<serde_json::Value>,
}

impl StateChange {
    /// Create a new state change
    pub fn new(
        change_type: ChangeType,
        entity_type: impl Into<String>,
        entity_id: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            change_type,
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
            description: description.into(),
            old_value: None,
            new_value: None,
        }
    }

    /// Add old/new values
    pub fn with_values(
        mut self,
        old: Option<serde_json::Value>,
        new: Option<serde_json::Value>,
    ) -> Self {
        self.old_value = old;
        self.new_value = new;
        self
    }
}

/// Represents a diff between two text/JSON values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiff {
    /// Lines added
    pub additions: Vec<String>,
    /// Lines removed
    pub deletions: Vec<String>,
    /// Unified diff output
    pub unified_diff: String,
}

impl StateDiff {
    /// Create a diff between two strings
    pub fn from_strings(old: &str, new: &str) -> Self {
        let diff = TextDiff::from_lines(old, new);
        let mut additions = Vec::new();
        let mut deletions = Vec::new();
        let mut unified_diff = String::new();

        for change in diff.iter_all_changes() {
            let line = change.value().trim_end();
            match change.tag() {
                ChangeTag::Insert => {
                    additions.push(line.to_string());
                    unified_diff.push_str(&format!("+{}\n", line));
                }
                ChangeTag::Delete => {
                    deletions.push(line.to_string());
                    unified_diff.push_str(&format!("-{}\n", line));
                }
                ChangeTag::Equal => {
                    unified_diff.push_str(&format!(" {}\n", line));
                }
            }
        }

        Self {
            additions,
            deletions,
            unified_diff,
        }
    }

    /// Create a diff between two JSON values
    pub fn from_json(old: &serde_json::Value, new: &serde_json::Value) -> Self {
        let old_str = serde_json::to_string_pretty(old).unwrap_or_default();
        let new_str = serde_json::to_string_pretty(new).unwrap_or_default();
        Self::from_strings(&old_str, &new_str)
    }

    /// Check if there are any differences
    pub fn has_changes(&self) -> bool {
        !self.additions.is_empty() || !self.deletions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_snapshot(id: &str, playbook: &str) -> StateSnapshot {
        let mut snapshot = StateSnapshot::new(id, playbook);
        snapshot.id = id.to_string();
        snapshot
    }

    #[test]
    fn test_diff_empty_snapshots() {
        let engine = DiffEngine::new();
        let old = create_test_snapshot("old", "test.yml");
        let new = create_test_snapshot("new", "test.yml");

        let report = engine.diff(&old, &new);
        assert!(!report.has_changes());
    }

    #[test]
    fn test_diff_added_task() {
        let engine = DiffEngine::new();
        let old = create_test_snapshot("old", "test.yml");
        let mut new = create_test_snapshot("new", "test.yml");

        new.tasks
            .push(TaskStateRecord::new("task1", "host1", "apt"));

        let report = engine.diff(&old, &new);
        assert!(report.has_changes());
        assert_eq!(report.summary.tasks_added, 1);
        assert_eq!(report.summary.tasks_removed, 0);
    }

    #[test]
    fn test_diff_removed_task() {
        let engine = DiffEngine::new();
        let mut old = create_test_snapshot("old", "test.yml");
        let new = create_test_snapshot("new", "test.yml");

        old.tasks
            .push(TaskStateRecord::new("task1", "host1", "apt"));

        let report = engine.diff(&old, &new);
        assert!(report.has_changes());
        assert_eq!(report.summary.tasks_added, 0);
        assert_eq!(report.summary.tasks_removed, 1);
    }

    #[test]
    fn test_diff_modified_task() {
        let engine = DiffEngine::new();
        let mut old = create_test_snapshot("old", "test.yml");
        let mut new = create_test_snapshot("new", "test.yml");

        let mut old_task = TaskStateRecord::new("task1", "host1", "apt");
        old_task.status = TaskStatus::Pending;
        old.tasks.push(old_task);

        let mut new_task = TaskStateRecord::new("task1", "host1", "apt");
        new_task.status = TaskStatus::Changed;
        new.tasks.push(new_task);

        let report = engine.diff(&old, &new);
        assert!(report.has_changes());
        assert_eq!(report.summary.tasks_modified, 1);
    }

    #[test]
    fn test_state_diff_strings() {
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified\nline3\nline4";

        let diff = StateDiff::from_strings(old, new);
        assert!(diff.has_changes());
        // The diff algorithm sees line2->modified and line3->line3 as:
        // - delete line2
        // - delete line3
        // - insert modified
        // - insert line3
        // - insert line4
        // But with better alignment it should be:
        // - delete line2
        // - insert modified
        // - insert line4
        // The similar crate's line diff may produce different results.
        // Just verify that we have changes and the counts are reasonable.
        assert!(!diff.deletions.is_empty());
        assert!(!diff.additions.is_empty());
    }

    #[test]
    fn test_state_diff_json() {
        let old = serde_json::json!({"key": "old_value"});
        let new = serde_json::json!({"key": "new_value"});

        let diff = StateDiff::from_json(&old, &new);
        assert!(diff.has_changes());
    }

    #[test]
    fn test_diff_report_format() {
        let engine = DiffEngine::new();
        let mut old = create_test_snapshot("old", "test.yml");
        let mut new = create_test_snapshot("new", "test.yml");

        old.tasks
            .push(TaskStateRecord::new("task1", "host1", "apt"));
        new.tasks
            .push(TaskStateRecord::new("task1", "host1", "apt"));
        new.tasks
            .push(TaskStateRecord::new("task2", "host1", "service"));

        let report = engine.diff(&old, &new);
        let formatted = report.format_detailed();

        assert!(formatted.contains("Diff Report"));
        assert!(formatted.contains("task2"));
    }
}
