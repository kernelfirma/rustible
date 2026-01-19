//! Full Skip Callback Plugin for Rustible.
//!
//! This callback plugin provides detailed information about why tasks were skipped,
//! including the exact `when` condition that evaluated to false. This is invaluable
//! for debugging conditional logic in playbooks.
//!
//! # Features
//!
//! - Shows the exact reason each task was skipped
//! - Displays the `when` condition that failed evaluation
//! - Groups skipped tasks by condition for pattern analysis
//! - Provides a summary of all skip patterns at playbook end
//! - Complements the `skippy` callback with deeper skip analysis
//!
//! # Example Output
//!
//! ```text
//! SKIPPED: webserver1 | Install nginx
//!   Condition: ansible_os_family == 'Debian'
//!   Reason: Condition evaluated to false (ansible_os_family = 'RedHat')
//!
//! SKIPPED: dbserver1 | Configure PostgreSQL
//!   Condition: postgresql_enabled is defined and postgresql_enabled
//!   Reason: Variable 'postgresql_enabled' is not defined
//!
//! === SKIP SUMMARY ===
//! Total skipped: 5 tasks across 3 hosts
//!
//! Skip Patterns:
//!   ansible_os_family == 'Debian'  (3 occurrences)
//!     - Install nginx (webserver1, webserver2, webserver3)
//!   postgresql_enabled is defined  (2 occurrences)
//!     - Configure PostgreSQL (dbserver1, dbserver2)
//! ```

use std::collections::HashMap;
use std::time::Instant;

use async_trait::async_trait;
use colored::Colorize;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Configuration for the full skip callback
#[derive(Debug, Clone)]
pub struct FullSkipConfig {
    /// Whether to show detailed variable values in skip reasons
    pub show_variable_values: bool,
    /// Whether to show per-task skip output immediately (vs only in summary)
    pub show_inline: bool,
    /// Whether to show the summary at playbook end
    pub show_summary: bool,
    /// Whether to group skips by condition in the summary
    pub group_by_condition: bool,
    /// Maximum number of hosts to show per condition in summary
    pub max_hosts_per_condition: usize,
    /// Whether to show the original when condition expression
    pub show_condition_expression: bool,
    /// Verbosity level (0=minimal, 1=normal, 2=verbose)
    pub verbosity: u8,
}

impl Default for FullSkipConfig {
    fn default() -> Self {
        Self {
            show_variable_values: true,
            show_inline: true,
            show_summary: true,
            group_by_condition: true,
            max_hosts_per_condition: 5,
            show_condition_expression: true,
            verbosity: 1,
        }
    }
}

/// Information about a single skipped task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedTask {
    /// Task name
    pub task_name: String,
    /// Host the task was skipped on
    pub host: String,
    /// The when condition that caused the skip (if available)
    pub when_condition: Option<String>,
    /// Human-readable reason for the skip
    pub skip_reason: String,
    /// Relevant variable values at time of skip
    pub variable_context: HashMap<String, String>,
    /// When the skip occurred
    #[serde(skip)]
    pub timestamp: Option<Instant>,
    /// Execution order
    pub order: u64,
}

impl SkippedTask {
    /// Create a new skipped task entry
    pub fn new(task_name: String, host: String, skip_reason: String, order: u64) -> Self {
        Self {
            task_name,
            host,
            when_condition: None,
            skip_reason,
            variable_context: HashMap::new(),
            timestamp: Some(Instant::now()),
            order,
        }
    }

    /// Set the when condition
    pub fn with_condition(mut self, condition: String) -> Self {
        self.when_condition = Some(condition);
        self
    }

    /// Add a variable to the context
    pub fn with_variable(mut self, name: String, value: String) -> Self {
        self.variable_context.insert(name, value);
        self
    }

    /// Extract the primary condition from the skip reason
    pub fn extract_condition(&self) -> String {
        self.when_condition.clone().unwrap_or_else(|| {
            // Try to extract condition from the skip reason message
            if let Some(start) = self.skip_reason.find("condition '") {
                if let Some(end) = self.skip_reason[start..].find("' was false") {
                    return self.skip_reason[start + 11..start + end].to_string();
                }
            }
            // Fallback to the raw message
            self.skip_reason.clone()
        })
    }
}

/// Statistics tracked per host
#[derive(Debug, Clone, Default)]
pub struct HostSkipStats {
    /// Total tasks on this host
    pub total_tasks: u32,
    /// Skipped tasks on this host
    pub skipped_tasks: u32,
    /// Conditions that caused skips
    pub skip_conditions: HashMap<String, u32>,
}

/// Aggregated skip pattern information
#[derive(Debug, Clone)]
pub struct SkipPattern {
    /// The condition that caused skips
    pub condition: String,
    /// Number of occurrences
    pub count: usize,
    /// Tasks that were skipped due to this condition
    pub tasks: Vec<(String, String)>, // (task_name, host)
}

/// Full Skip Callback Plugin
///
/// Provides comprehensive information about skipped tasks, including
/// the exact conditions that caused tasks to be skipped. This is
/// essential for debugging conditional logic in complex playbooks.
#[derive(Debug)]
pub struct FullSkipCallback {
    /// Configuration
    config: FullSkipConfig,
    /// All skipped tasks
    skipped_tasks: RwLock<Vec<SkippedTask>>,
    /// Per-host statistics
    host_stats: RwLock<HashMap<String, HostSkipStats>>,
    /// Playbook start time
    playbook_start: RwLock<Option<Instant>>,
    /// Current playbook name
    current_playbook: RwLock<Option<String>>,
    /// Task counter for ordering
    task_counter: std::sync::atomic::AtomicU64,
    /// Total tasks executed
    total_tasks: std::sync::atomic::AtomicU64,
}

impl FullSkipCallback {
    /// Create a new full skip callback with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(FullSkipConfig::default())
    }

    /// Create a new full skip callback with custom configuration
    #[must_use]
    pub fn with_config(config: FullSkipConfig) -> Self {
        Self {
            config,
            skipped_tasks: RwLock::new(Vec::new()),
            host_stats: RwLock::new(HashMap::new()),
            playbook_start: RwLock::new(None),
            current_playbook: RwLock::new(None),
            task_counter: std::sync::atomic::AtomicU64::new(0),
            total_tasks: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Reset all tracked data
    pub fn reset(&self) {
        self.skipped_tasks.write().clear();
        self.host_stats.write().clear();
        *self.playbook_start.write() = None;
        *self.current_playbook.write() = None;
        self.task_counter
            .store(0, std::sync::atomic::Ordering::SeqCst);
        self.total_tasks
            .store(0, std::sync::atomic::Ordering::SeqCst);
    }

    /// Get all skipped tasks
    pub fn get_skipped_tasks(&self) -> Vec<SkippedTask> {
        self.skipped_tasks.read().clone()
    }

    /// Get the count of skipped tasks
    pub fn skip_count(&self) -> usize {
        self.skipped_tasks.read().len()
    }

    /// Get the total task count
    pub fn total_count(&self) -> u64 {
        self.total_tasks.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Calculate skip percentage
    pub fn skip_percentage(&self) -> f64 {
        let total = self.total_count();
        if total == 0 {
            return 0.0;
        }
        (self.skip_count() as f64 / total as f64) * 100.0
    }

    /// Get aggregated skip patterns
    pub fn get_skip_patterns(&self) -> Vec<SkipPattern> {
        let skipped = self.skipped_tasks.read();
        let mut patterns: HashMap<String, Vec<(String, String)>> = HashMap::new();

        for task in skipped.iter() {
            let condition = task.extract_condition();
            patterns
                .entry(condition)
                .or_default()
                .push((task.task_name.clone(), task.host.clone()));
        }

        let mut result: Vec<SkipPattern> = patterns
            .into_iter()
            .map(|(condition, tasks)| SkipPattern {
                count: tasks.len(),
                condition,
                tasks,
            })
            .collect();

        // Sort by count descending
        result.sort_by(|a, b| b.count.cmp(&a.count));
        result
    }

    /// Get hosts affected by skips
    pub fn get_affected_hosts(&self) -> Vec<String> {
        let stats = self.host_stats.read();
        stats
            .iter()
            .filter(|(_, s)| s.skipped_tasks > 0)
            .map(|(h, _)| h.clone())
            .collect()
    }

    /// Parse the skip reason to extract condition and details
    fn parse_skip_reason(message: &str) -> (Option<String>, String) {
        // Common skip message patterns:
        // "Skipped: condition 'X' was false"
        // "conditional check failed: X"
        // "when condition evaluated to false"

        if let Some(start) = message.find("condition '") {
            if let Some(end) = message[start + 11..].find('\'') {
                let condition = message[start + 11..start + 11 + end].to_string();
                let reason = format!("Condition '{}' evaluated to false", condition);
                return (Some(condition), reason);
            }
        }

        if let Some(reason) = message.strip_prefix("Skipped: ") {
            return (None, reason.to_string());
        }

        (None, message.to_string())
    }

    /// Format a single skipped task for inline output
    fn format_skip_inline(&self, task: &SkippedTask) -> String {
        let mut output = format!(
            "{}: {} | {}",
            "SKIPPED".cyan().bold(),
            task.host.bright_white().bold(),
            task.task_name.yellow()
        );

        if self.config.show_condition_expression {
            if let Some(ref condition) = task.when_condition {
                output.push_str(&format!(
                    "\n  {}: {}",
                    "Condition".bright_black(),
                    condition.bright_cyan()
                ));
            }
        }

        if self.config.verbosity > 0 {
            output.push_str(&format!(
                "\n  {}: {}",
                "Reason".bright_black(),
                task.skip_reason.italic()
            ));
        }

        if self.config.show_variable_values && !task.variable_context.is_empty() {
            output.push_str(&format!("\n  {}:", "Variables".bright_black()));
            for (name, value) in &task.variable_context {
                output.push_str(&format!("\n    {} = {}", name.green(), value.yellow()));
            }
        }

        output
    }

    /// Print the skip summary
    fn print_summary(&self) {
        let skipped = self.skipped_tasks.read();
        let total = self.total_count();
        let affected_hosts = self.get_affected_hosts();

        if skipped.is_empty() {
            if self.config.verbosity > 0 {
                println!(
                    "\n{} No tasks were skipped during playbook execution.",
                    "[FULL_SKIP]".cyan().bold()
                );
            }
            return;
        }

        // Header
        println!(
            "\n{} {}",
            "=== SKIP SUMMARY ===".bright_cyan().bold(),
            "=".repeat(50).bright_black()
        );

        // Overall statistics
        println!(
            "\n{}: {} tasks across {} hosts ({:.1}% of {} total tasks)",
            "Total skipped".bright_white().bold(),
            skipped.len().to_string().cyan().bold(),
            affected_hosts.len().to_string().yellow(),
            self.skip_percentage(),
            total
        );

        // Skip patterns
        if self.config.group_by_condition {
            let patterns = self.get_skip_patterns();

            if !patterns.is_empty() {
                println!("\n{}", "Skip Patterns:".bright_white().bold());

                for pattern in patterns.iter().take(10) {
                    println!(
                        "\n  {} ({} occurrence{})",
                        pattern.condition.bright_cyan(),
                        pattern.count,
                        if pattern.count == 1 { "" } else { "s" }
                    );

                    // Group tasks by name
                    let mut task_groups: HashMap<&str, Vec<&str>> = HashMap::new();
                    for (task_name, host) in &pattern.tasks {
                        task_groups
                            .entry(task_name.as_str())
                            .or_default()
                            .push(host.as_str());
                    }

                    for (task_name, hosts) in &task_groups {
                        let hosts_display: String =
                            if hosts.len() > self.config.max_hosts_per_condition {
                                let shown: Vec<_> = hosts
                                    .iter()
                                    .take(self.config.max_hosts_per_condition)
                                    .copied()
                                    .collect();
                                format!(
                                    "{} +{} more",
                                    shown.join(", "),
                                    hosts.len() - self.config.max_hosts_per_condition
                                )
                            } else {
                                hosts.join(", ")
                            };

                        println!(
                            "    {} {} ({})",
                            "-".bright_black(),
                            task_name.yellow(),
                            hosts_display.bright_white()
                        );
                    }
                }

                if patterns.len() > 10 {
                    println!(
                        "\n  {} more skip patterns...",
                        (patterns.len() - 10).to_string().bright_black()
                    );
                }
            }
        }

        // Per-host breakdown (if verbose)
        if self.config.verbosity > 1 {
            println!("\n{}", "Per-Host Skip Statistics:".bright_white().bold());

            let stats = self.host_stats.read();
            let mut sorted_hosts: Vec<_> = stats.iter().collect();
            sorted_hosts.sort_by(|a, b| b.1.skipped_tasks.cmp(&a.1.skipped_tasks));

            for (host, host_stats) in sorted_hosts.iter().take(10) {
                if host_stats.skipped_tasks > 0 {
                    let skip_rate = if host_stats.total_tasks > 0 {
                        (host_stats.skipped_tasks as f64 / host_stats.total_tasks as f64) * 100.0
                    } else {
                        0.0
                    };

                    println!(
                        "  {}: {} skipped / {} total ({:.1}%)",
                        host.bright_white().bold(),
                        host_stats.skipped_tasks.to_string().cyan(),
                        host_stats.total_tasks,
                        skip_rate
                    );
                }
            }
        }

        // Recommendations
        if self.skip_percentage() > 50.0 {
            println!(
                "\n{} High skip rate detected ({:.1}%). Consider reviewing your conditions:",
                "Warning:".yellow().bold(),
                self.skip_percentage()
            );
            println!(
                "  {} Check if conditions are too restrictive",
                "-".bright_black()
            );
            println!(
                "  {} Verify variables are defined correctly",
                "-".bright_black()
            );
            println!(
                "  {} Consider using 'default' filter for optional variables",
                "-".bright_black()
            );
        }

        println!();
    }

    /// Generate a JSON report of skip data
    pub fn to_json_report(&self) -> serde_json::Value {
        let skipped = self.skipped_tasks.read();
        let patterns = self.get_skip_patterns();

        serde_json::json!({
            "summary": {
                "total_tasks": self.total_count(),
                "skipped_tasks": skipped.len(),
                "skip_percentage": self.skip_percentage(),
                "affected_hosts": self.get_affected_hosts().len(),
            },
            "skipped_tasks": skipped.iter().map(|t| {
                serde_json::json!({
                    "task_name": t.task_name,
                    "host": t.host,
                    "when_condition": t.when_condition,
                    "skip_reason": t.skip_reason,
                    "variable_context": t.variable_context,
                    "order": t.order,
                })
            }).collect::<Vec<_>>(),
            "skip_patterns": patterns.iter().map(|p| {
                serde_json::json!({
                    "condition": p.condition,
                    "count": p.count,
                    "tasks": p.tasks.iter().map(|(t, h)| {
                        serde_json::json!({"task": t, "host": h})
                    }).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
        })
    }
}

impl Default for FullSkipCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for FullSkipCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            skipped_tasks: RwLock::new(self.skipped_tasks.read().clone()),
            host_stats: RwLock::new(self.host_stats.read().clone()),
            playbook_start: RwLock::new(*self.playbook_start.read()),
            current_playbook: RwLock::new(self.current_playbook.read().clone()),
            task_counter: std::sync::atomic::AtomicU64::new(
                self.task_counter.load(std::sync::atomic::Ordering::SeqCst),
            ),
            total_tasks: std::sync::atomic::AtomicU64::new(
                self.total_tasks.load(std::sync::atomic::Ordering::SeqCst),
            ),
        }
    }
}

#[async_trait]
impl ExecutionCallback for FullSkipCallback {
    /// Called when a playbook starts - initialize tracking
    async fn on_playbook_start(&self, name: &str) {
        *self.playbook_start.write() = Some(Instant::now());
        *self.current_playbook.write() = Some(name.to_string());

        // Clear stats from any previous run
        self.skipped_tasks.write().clear();
        self.host_stats.write().clear();
        self.task_counter
            .store(0, std::sync::atomic::Ordering::SeqCst);
        self.total_tasks
            .store(0, std::sync::atomic::Ordering::SeqCst);

        if self.config.verbosity > 0 {
            println!(
                "{} Skip tracking enabled for playbook: {}",
                "[FULL_SKIP]".cyan().bold(),
                name.bright_white()
            );
        }
    }

    /// Called when a playbook ends - print summary
    async fn on_playbook_end(&self, _name: &str, _success: bool) {
        if self.config.show_summary {
            self.print_summary();
        }
    }

    /// Called when a play starts - initialize host stats
    async fn on_play_start(&self, _name: &str, hosts: &[String]) {
        let mut stats = self.host_stats.write();
        for host in hosts {
            stats.entry(host.clone()).or_default();
        }
    }

    /// Called when a play ends - silent
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Nothing to do here
    }

    /// Called when a task starts - silent
    async fn on_task_start(&self, _name: &str, _host: &str) {
        // Nothing to do here
    }

    /// Called when a task completes - track skipped tasks
    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Increment total task counter
        self.total_tasks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        // Update host stats
        {
            let mut stats = self.host_stats.write();
            let host_stats = stats.entry(result.host.clone()).or_default();
            host_stats.total_tasks += 1;

            if result.result.skipped {
                host_stats.skipped_tasks += 1;
            }
        }

        // Only process skipped tasks
        if !result.result.skipped {
            return;
        }

        let order = self
            .task_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let message = result.result.message.as_str();

        // Parse the skip reason
        let (condition, reason) = Self::parse_skip_reason(message);

        let mut skipped_task =
            SkippedTask::new(result.task_name.clone(), result.host.clone(), reason, order);

        if let Some(cond) = condition {
            skipped_task = skipped_task.with_condition(cond.clone());

            // Update condition statistics
            let mut stats = self.host_stats.write();
            if let Some(host_stats) = stats.get_mut(&result.host) {
                *host_stats.skip_conditions.entry(cond).or_default() += 1;
            }
        }

        // Print inline if configured
        if self.config.show_inline {
            println!("{}", self.format_skip_inline(&skipped_task));
        }

        // Store the skipped task
        self.skipped_tasks.write().push(skipped_task);
    }

    /// Called when a handler is triggered - silent
    async fn on_handler_triggered(&self, _name: &str) {
        // Nothing to do here
    }

    /// Called when facts are gathered - silent
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // Nothing to do here
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;
    use std::time::Duration;

    fn create_execution_result(
        host: &str,
        task_name: &str,
        skipped: bool,
        message: &str,
    ) -> ExecutionResult {
        ExecutionResult {
            host: host.to_string(),
            task_name: task_name.to_string(),
            result: ModuleResult {
                success: true,
                changed: false,
                message: message.to_string(),
                skipped,
                data: None,
                warnings: Vec::new(),
            },
            duration: Duration::from_millis(10),
            notify: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_full_skip_callback_creation() {
        let callback = FullSkipCallback::new();
        assert!(callback.config.show_inline);
        assert!(callback.config.show_summary);
        assert_eq!(callback.skip_count(), 0);
    }

    #[tokio::test]
    async fn test_skip_tracking() {
        let callback = FullSkipCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // Simulate a skipped task
        let result = create_execution_result(
            "host1",
            "Install nginx",
            true,
            "Skipped: condition 'ansible_os_family == \"Debian\"' was false",
        );
        callback.on_task_complete(&result).await;

        assert_eq!(callback.skip_count(), 1);
        assert_eq!(callback.total_count(), 1);

        let skipped = callback.get_skipped_tasks();
        assert_eq!(skipped[0].task_name, "Install nginx");
        assert_eq!(skipped[0].host, "host1");
        assert!(skipped[0].when_condition.is_some());
    }

    #[tokio::test]
    async fn test_skip_patterns() {
        let callback = FullSkipCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start(
                "test-play",
                &[
                    "host1".to_string(),
                    "host2".to_string(),
                    "host3".to_string(),
                ],
            )
            .await;

        // Multiple tasks skipped with same condition
        for host in ["host1", "host2", "host3"] {
            let result = create_execution_result(
                host,
                "Install nginx",
                true,
                "Skipped: condition 'nginx_enabled' was false",
            );
            callback.on_task_complete(&result).await;
        }

        let patterns = callback.get_skip_patterns();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].count, 3);
        assert!(patterns[0].condition.contains("nginx_enabled"));
    }

    #[tokio::test]
    async fn test_non_skipped_tasks_not_tracked() {
        let callback = FullSkipCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // Non-skipped task
        let result = create_execution_result(
            "host1",
            "Install nginx",
            false,
            "Package installed successfully",
        );
        callback.on_task_complete(&result).await;

        assert_eq!(callback.skip_count(), 0);
        assert_eq!(callback.total_count(), 1);
    }

    #[tokio::test]
    async fn test_skip_percentage() {
        let callback = FullSkipCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // 2 normal tasks
        for _ in 0..2 {
            let result = create_execution_result("host1", "Normal task", false, "OK");
            callback.on_task_complete(&result).await;
        }

        // 2 skipped tasks
        for _ in 0..2 {
            let result = create_execution_result(
                "host1",
                "Skipped task",
                true,
                "Skipped: condition 'false' was false",
            );
            callback.on_task_complete(&result).await;
        }

        assert_eq!(callback.total_count(), 4);
        assert_eq!(callback.skip_count(), 2);
        assert!((callback.skip_percentage() - 50.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_affected_hosts() {
        let callback = FullSkipCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start(
                "test-play",
                &[
                    "host1".to_string(),
                    "host2".to_string(),
                    "host3".to_string(),
                ],
            )
            .await;

        // Skip on host1 and host2 only
        let result1 =
            create_execution_result("host1", "Task", true, "Skipped: condition 'X' was false");
        callback.on_task_complete(&result1).await;

        let result2 =
            create_execution_result("host2", "Task", true, "Skipped: condition 'X' was false");
        callback.on_task_complete(&result2).await;

        let result3 = create_execution_result("host3", "Task", false, "OK");
        callback.on_task_complete(&result3).await;

        let affected = callback.get_affected_hosts();
        assert_eq!(affected.len(), 2);
        assert!(affected.contains(&"host1".to_string()));
        assert!(affected.contains(&"host2".to_string()));
    }

    #[tokio::test]
    async fn test_parse_skip_reason() {
        let (cond, reason) = FullSkipCallback::parse_skip_reason(
            "Skipped: condition 'ansible_os_family == \"Debian\"' was false",
        );
        assert!(cond.is_some());
        assert!(cond.unwrap().contains("ansible_os_family"));
        assert!(reason.contains("evaluated to false"));

        let (cond2, reason2) = FullSkipCallback::parse_skip_reason("Skipped: some other reason");
        assert!(cond2.is_none());
        assert_eq!(reason2, "some other reason");
    }

    #[tokio::test]
    async fn test_json_report() {
        let callback = FullSkipCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let result =
            create_execution_result("host1", "Task", true, "Skipped: condition 'X' was false");
        callback.on_task_complete(&result).await;

        let report = callback.to_json_report();

        assert!(report.get("summary").is_some());
        assert!(report.get("skipped_tasks").is_some());
        assert!(report.get("skip_patterns").is_some());

        let summary = &report["summary"];
        assert_eq!(summary["skipped_tasks"], 1);
        assert_eq!(summary["total_tasks"], 1);
    }

    #[tokio::test]
    async fn test_reset() {
        let callback = FullSkipCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let result =
            create_execution_result("host1", "Task", true, "Skipped: condition 'X' was false");
        callback.on_task_complete(&result).await;

        assert_eq!(callback.skip_count(), 1);

        callback.reset();

        assert_eq!(callback.skip_count(), 0);
        assert_eq!(callback.total_count(), 0);
    }

    #[test]
    fn test_skipped_task_builder() {
        let task = SkippedTask::new(
            "Install nginx".to_string(),
            "host1".to_string(),
            "Condition was false".to_string(),
            0,
        )
        .with_condition("ansible_os_family == 'Debian'".to_string())
        .with_variable("ansible_os_family".to_string(), "RedHat".to_string());

        assert_eq!(task.task_name, "Install nginx");
        assert!(task.when_condition.is_some());
        assert_eq!(task.variable_context.len(), 1);
    }

    #[test]
    fn test_extract_condition() {
        let task = SkippedTask::new(
            "Task".to_string(),
            "host".to_string(),
            "Skipped: condition 'my_var == true' was false".to_string(),
            0,
        );

        let condition = task.extract_condition();
        assert!(condition.contains("my_var"));

        let task_with_condition = task.with_condition("explicit_condition".to_string());
        assert_eq!(
            task_with_condition.extract_condition(),
            "explicit_condition"
        );
    }

    #[test]
    fn test_config_defaults() {
        let config = FullSkipConfig::default();
        assert!(config.show_variable_values);
        assert!(config.show_inline);
        assert!(config.show_summary);
        assert!(config.group_by_condition);
        assert_eq!(config.max_hosts_per_condition, 5);
        assert!(config.show_condition_expression);
        assert_eq!(config.verbosity, 1);
    }

    #[tokio::test]
    async fn test_host_skip_stats() {
        let callback = FullSkipCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // 1 skipped, 2 normal
        let skipped =
            create_execution_result("host1", "Task1", true, "Skipped: condition 'X' was false");
        callback.on_task_complete(&skipped).await;

        let normal1 = create_execution_result("host1", "Task2", false, "OK");
        callback.on_task_complete(&normal1).await;

        let normal2 = create_execution_result("host1", "Task3", false, "OK");
        callback.on_task_complete(&normal2).await;

        let stats = callback.host_stats.read();
        let host_stats = stats.get("host1").unwrap();

        assert_eq!(host_stats.total_tasks, 3);
        assert_eq!(host_stats.skipped_tasks, 1);
    }

    #[test]
    fn test_clone() {
        let callback = FullSkipCallback::new();
        let cloned = callback.clone();

        assert!(cloned.config.show_inline);
        assert_eq!(cloned.skip_count(), 0);
    }
}
