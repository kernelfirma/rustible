//! Selective callback plugin for Rustible.
//!
//! This plugin provides fine-grained filtering of execution output based on:
//! - Specific hosts (whitelist/blacklist)
//! - Specific tasks (whitelist/blacklist)
//! - Result status (failures only, changed only, etc.)
//! - Regex patterns for task names
//! - Tag-based filtering
//!
//! # Features
//!
//! - Host filtering: Include/exclude specific hosts by name or pattern
//! - Task filtering: Include/exclude specific tasks by name or regex
//! - Status filtering: Show only failures, changes, or specific status types
//! - Tag filtering: Filter tasks by their assigned tags
//! - Quiet mode: Suppress all output except matching criteria
//!
//! # Example Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::SelectiveCallback;
//!
//! // Show only failures from specific hosts
//! let callback = SelectiveCallback::builder()
//!     .hosts(&["webserver1", "webserver2"])
//!     .failures_only()
//!     .build();
//!
//! // Show tasks matching a regex pattern
//! let callback = SelectiveCallback::builder()
//!     .task_pattern(r"(?i)install.*nginx")?
//!     .build();
//!
//! // Filter by tags
//! let callback = SelectiveCallback::builder()
//!     .tags(&["deploy", "config"])
//!     .build();
//! # Ok(())
//! # }
//! ```
//!
//! # Example Output
//!
//! ```text
//! [MATCHED] webserver1 | Install nginx | changed
//! [MATCHED] webserver2 | Install nginx | failed: Package not found
//!
//! SELECTIVE RECAP (filtered 3 of 15 results):
//! webserver1: matched=2 ok=1 changed=1 failed=0
//! webserver2: matched=1 ok=0 changed=0 failed=1
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use colored::Colorize;
use parking_lot::RwLock;
use regex::Regex;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Filter mode for host and task matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    /// Include items that match the filter (whitelist)
    #[default]
    Include,
    /// Exclude items that match the filter (blacklist)
    Exclude,
}

/// Status filter configuration.
#[derive(Debug, Clone, Default)]
pub struct StatusFilter {
    /// Show only failures
    pub failures_only: bool,
    /// Show only changes
    pub changes_only: bool,
    /// Show only skipped
    pub skipped_only: bool,
    /// Show specific statuses (if empty, show all)
    pub statuses: HashSet<String>,
}

impl StatusFilter {
    /// Creates a filter that shows only failures.
    #[must_use]
    pub fn failures() -> Self {
        Self {
            failures_only: true,
            ..Default::default()
        }
    }

    /// Creates a filter that shows only changes.
    #[must_use]
    pub fn changes() -> Self {
        Self {
            changes_only: true,
            ..Default::default()
        }
    }

    /// Creates a filter that shows specific statuses.
    #[must_use]
    pub fn with_statuses(statuses: &[&str]) -> Self {
        Self {
            statuses: statuses.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// Check if a result matches this status filter.
    fn matches(&self, result: &ExecutionResult) -> bool {
        // If failures_only is set, only match failures
        if self.failures_only && result.result.success {
            return false;
        }

        // If changes_only is set, only match changes
        if self.changes_only && !result.result.changed {
            return false;
        }

        // If skipped_only is set, only match skipped
        if self.skipped_only && !result.result.skipped {
            return false;
        }

        // If specific statuses are set, check if the status matches
        if !self.statuses.is_empty() {
            let status = Self::result_to_status_str(result);
            if !self.statuses.contains(status) {
                return false;
            }
        }

        true
    }

    /// Convert ExecutionResult to status string for comparison.
    fn result_to_status_str(result: &ExecutionResult) -> &'static str {
        if result.result.skipped {
            "skipped"
        } else if !result.result.success {
            "failed"
        } else if result.result.changed {
            "changed"
        } else {
            "ok"
        }
    }
}

/// Configuration for the selective callback.
#[derive(Debug, Clone, Default)]
pub struct SelectiveConfig {
    /// Hosts to filter (whitelist or blacklist based on mode)
    pub hosts: HashSet<String>,
    /// Host filter mode
    pub host_mode: FilterMode,
    /// Host patterns (regex)
    pub host_patterns: Vec<String>,
    /// Compiled host regex patterns
    #[allow(dead_code)]
    compiled_host_patterns: Vec<Regex>,

    /// Tasks to filter (whitelist or blacklist based on mode)
    pub tasks: HashSet<String>,
    /// Task filter mode
    pub task_mode: FilterMode,
    /// Task patterns (regex)
    pub task_patterns: Vec<String>,
    /// Compiled task regex patterns
    #[allow(dead_code)]
    compiled_task_patterns: Vec<Regex>,

    /// Status filter
    pub status_filter: StatusFilter,

    /// Tags to filter by (tasks must have at least one matching tag)
    pub tags: HashSet<String>,
    /// Tag filter mode
    pub tag_mode: FilterMode,

    /// Whether to show verbose output for matches
    pub verbose: bool,
    /// Whether to show the recap even if no matches
    pub always_recap: bool,
    /// Custom prefix for matched output
    pub match_prefix: Option<String>,
}

impl SelectiveConfig {
    /// Compile regex patterns from string patterns.
    fn compile_patterns(&mut self) {
        self.compiled_host_patterns = self
            .host_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
        self.compiled_task_patterns = self
            .task_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
    }
}

/// Statistics tracked per host during execution.
#[derive(Debug, Clone, Default)]
struct HostStats {
    /// Count of matched results
    matched: u32,
    /// Count of total results
    total: u32,
    /// Count of successful tasks (no changes)
    ok: u32,
    /// Count of tasks that made changes
    changed: u32,
    /// Count of failed tasks
    failed: u32,
    /// Count of skipped tasks
    skipped: u32,
    /// Count of unreachable attempts
    unreachable: u32,
}

/// Selective callback plugin that filters output based on configurable criteria.
///
/// This callback allows fine-grained control over which execution results
/// are displayed, making it easier to focus on specific hosts, tasks, or
/// failure scenarios.
///
/// # Design Principles
///
/// 1. **Flexible Filtering**: Multiple filter types can be combined
/// 2. **Pattern Matching**: Regex support for host and task names
/// 3. **Tag Support**: Filter by task tags for organized output
/// 4. **Clear Output**: Matched results are clearly indicated
/// 5. **Filtered Recap**: Summary shows filter statistics
///
/// # Usage
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::{SelectiveCallback, SelectiveBuilder};
///
/// let callback = SelectiveCallback::builder()
///     .hosts(&["prod-web-*"])
///     .failures_only()
///     .verbose()
///     .build();
///
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SelectiveCallback {
    /// Configuration for filtering
    config: SelectiveConfig,
    /// Compiled host regex patterns
    host_patterns: Vec<Regex>,
    /// Compiled task regex patterns
    task_patterns: Vec<Regex>,
    /// Per-host execution statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Playbook start time for duration tracking
    start_time: Arc<RwLock<Option<Instant>>>,
    /// Current playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Whether any failures occurred (for exit code)
    has_failures: Arc<RwLock<bool>>,
    /// Task tags cache (task_name -> tags)
    task_tags: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Count of matched results
    match_count: Arc<RwLock<u32>>,
    /// Count of total results
    total_count: Arc<RwLock<u32>>,
}

impl SelectiveCallback {
    /// Creates a new selective callback with the given configuration.
    #[must_use]
    pub fn new(mut config: SelectiveConfig) -> Self {
        config.compile_patterns();

        let host_patterns: Vec<Regex> = config
            .host_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();

        let task_patterns: Vec<Regex> = config
            .task_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();

        Self {
            config,
            host_patterns,
            task_patterns,
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            start_time: Arc::new(RwLock::new(None)),
            playbook_name: Arc::new(RwLock::new(None)),
            has_failures: Arc::new(RwLock::new(false)),
            task_tags: Arc::new(RwLock::new(HashMap::new())),
            match_count: Arc::new(RwLock::new(0)),
            total_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Creates a new builder for configuring the callback.
    #[must_use]
    pub fn builder() -> SelectiveBuilder {
        SelectiveBuilder::new()
    }

    /// Creates a callback that only shows failures.
    #[must_use]
    pub fn failures_only() -> Self {
        Self::builder().failures_only().build()
    }

    /// Creates a callback that only shows changes.
    #[must_use]
    pub fn changes_only() -> Self {
        Self::builder().changes_only().build()
    }

    /// Creates a callback that filters by specific hosts.
    #[must_use]
    pub fn for_hosts(hosts: &[&str]) -> Self {
        Self::builder().hosts(hosts).build()
    }

    /// Creates a callback that filters by task name pattern.
    #[must_use]
    pub fn for_task_pattern(pattern: &str) -> Self {
        Self::builder()
            .task_pattern(pattern)
            .unwrap_or_else(|_| SelectiveBuilder::new())
            .build()
    }

    /// Returns whether any failures occurred during execution.
    pub fn has_failures(&self) -> bool {
        *self.has_failures.read()
    }

    /// Returns the match statistics.
    pub fn match_stats(&self) -> (u32, u32) {
        let matched = *self.match_count.read();
        let total = *self.total_count.read();
        (matched, total)
    }

    /// Register task tags for later filtering.
    pub fn register_task_tags(&self, task_name: &str, tags: Vec<String>) {
        let mut task_tags = self.task_tags.write();
        task_tags.insert(task_name.to_string(), tags);
    }

    /// Check if a host matches the filter criteria.
    fn host_matches(&self, host: &str) -> bool {
        // Check explicit host list
        let in_list = self.config.hosts.contains(host);

        // Check host patterns
        let pattern_match = self.host_patterns.iter().any(|p| p.is_match(host));

        let matches = in_list || pattern_match;

        // If no filters are set, match all
        if self.config.hosts.is_empty() && self.host_patterns.is_empty() {
            return true;
        }

        // Apply filter mode
        match self.config.host_mode {
            FilterMode::Include => matches,
            FilterMode::Exclude => !matches,
        }
    }

    /// Check if a task matches the filter criteria.
    fn task_matches(&self, task_name: &str) -> bool {
        // Check explicit task list
        let in_list = self.config.tasks.contains(task_name);

        // Check task patterns
        let pattern_match = self.task_patterns.iter().any(|p| p.is_match(task_name));

        let matches = in_list || pattern_match;

        // If no filters are set, match all
        if self.config.tasks.is_empty() && self.task_patterns.is_empty() {
            return true;
        }

        // Apply filter mode
        match self.config.task_mode {
            FilterMode::Include => matches,
            FilterMode::Exclude => !matches,
        }
    }

    /// Check if task tags match the filter criteria.
    fn tags_match(&self, task_name: &str) -> bool {
        // If no tag filters are set, match all
        if self.config.tags.is_empty() {
            return true;
        }

        let task_tags = self.task_tags.read();
        let tags = task_tags.get(task_name);

        let has_matching_tag =
            tags.is_some_and(|task_tags| task_tags.iter().any(|t| self.config.tags.contains(t)));

        // Apply filter mode
        match self.config.tag_mode {
            FilterMode::Include => has_matching_tag,
            FilterMode::Exclude => !has_matching_tag,
        }
    }

    /// Check if a result matches all filter criteria.
    fn result_matches(&self, result: &ExecutionResult) -> bool {
        // Check host filter
        if !self.host_matches(&result.host) {
            return false;
        }

        // Check task filter
        if !self.task_matches(&result.task_name) {
            return false;
        }

        // Check tag filter
        if !self.tags_match(&result.task_name) {
            return false;
        }

        // Check status filter
        if !self.config.status_filter.matches(result) {
            return false;
        }

        true
    }

    /// Format a matched result for output.
    fn format_match(&self, result: &ExecutionResult) -> String {
        let prefix = self.config.match_prefix.as_deref().unwrap_or("[MATCHED]");

        let status_str = if result.result.skipped {
            "skipped".cyan()
        } else if !result.result.success {
            "failed".red().bold()
        } else if result.result.changed {
            "changed".yellow()
        } else {
            "ok".green()
        };

        let base = format!(
            "{} {} | {} | {}",
            prefix.bright_magenta().bold(),
            result.host.bright_white().bold(),
            result.task_name.bright_cyan(),
            status_str
        );

        if !result.result.success && !result.result.message.is_empty() {
            format!("{}: {}", base, result.result.message)
        } else {
            base
        }
    }

    /// Format verbose match output with additional details.
    fn format_verbose_match(&self, result: &ExecutionResult) -> String {
        let mut output = self.format_match(result);

        if self.config.verbose {
            output.push_str(&format!("\n  Duration: {:?}", result.duration));

            if !result.notify.is_empty() {
                output.push_str(&format!("\n  Notified: {}", result.notify.join(", ")));
            }

            if let Some(ref data) = result.result.data {
                if let Ok(formatted) = serde_json::to_string_pretty(data) {
                    let indented = formatted
                        .lines()
                        .map(|l| format!("    {}", l))
                        .collect::<Vec<_>>()
                        .join("\n");
                    output.push_str(&format!("\n  Data:\n{}", indented));
                }
            }
        }

        output
    }

    /// Format the filtered recap line for a host.
    fn format_recap_line(host: &str, stats: &HostStats) -> String {
        let host_color = if stats.failed > 0 || stats.unreachable > 0 {
            host.red().bold()
        } else if stats.changed > 0 {
            host.yellow()
        } else {
            host.green()
        };

        format!(
            "{}: matched={} ok={} changed={} failed={} skipped={} unreachable={}",
            host_color,
            stats.matched.to_string().bright_magenta(),
            stats.ok.to_string().green(),
            stats.changed.to_string().yellow(),
            stats.failed.to_string().red(),
            stats.skipped.to_string().cyan(),
            stats.unreachable.to_string().magenta(),
        )
    }
}

impl Default for SelectiveCallback {
    fn default() -> Self {
        Self::new(SelectiveConfig::default())
    }
}

impl Clone for SelectiveCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            host_patterns: self.host_patterns.clone(),
            task_patterns: self.task_patterns.clone(),
            host_stats: Arc::clone(&self.host_stats),
            start_time: Arc::clone(&self.start_time),
            playbook_name: Arc::clone(&self.playbook_name),
            has_failures: Arc::clone(&self.has_failures),
            task_tags: Arc::clone(&self.task_tags),
            match_count: Arc::clone(&self.match_count),
            total_count: Arc::clone(&self.total_count),
        }
    }
}

#[async_trait]
impl ExecutionCallback for SelectiveCallback {
    /// Called when a playbook starts - records start time.
    async fn on_playbook_start(&self, name: &str) {
        *self.start_time.write() = Some(Instant::now());
        *self.playbook_name.write() = Some(name.to_string());

        // Clear stats from any previous run
        self.host_stats.write().clear();
        *self.has_failures.write() = false;
        *self.match_count.write() = 0;
        *self.total_count.write() = 0;

        // Clear task tags cache
        self.task_tags.write().clear();
    }

    /// Called when a playbook ends - prints the filtered recap.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let stats = self.host_stats.read();
        let start_time = *self.start_time.read();
        let match_count = *self.match_count.read();
        let total_count = *self.total_count.read();

        // Skip recap if no matches and always_recap is false
        if match_count == 0 && !self.config.always_recap {
            return;
        }

        // Print empty line before recap for visual separation
        if !stats.is_empty() {
            println!();
        }

        // Print selective recap header
        println!(
            "{} (filtered {} of {} results):",
            "SELECTIVE RECAP".bright_magenta().bold(),
            match_count.to_string().bright_white(),
            total_count.to_string().bright_black()
        );

        // Print recap for each host in sorted order
        let mut hosts: Vec<_> = stats.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_stats) = stats.get(host) {
                // Only show hosts with matches unless always_recap is true
                if host_stats.matched > 0 || self.config.always_recap {
                    println!("{}", Self::format_recap_line(host, host_stats));
                }
            }
        }

        // Print duration if we have start time
        if let Some(start) = start_time {
            let duration = start.elapsed();
            let playbook_status = if success {
                "completed".green()
            } else {
                "failed".red().bold()
            };

            println!(
                "\n{} {} in {:.2}s",
                name.bright_white().bold(),
                playbook_status,
                duration.as_secs_f64()
            );
        }
    }

    /// Called when a play starts - initializes host stats.
    async fn on_play_start(&self, _name: &str, hosts: &[String]) {
        let mut stats = self.host_stats.write();
        for host in hosts {
            stats.entry(host.clone()).or_default();
        }
    }

    /// Called when a play ends.
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Silent - recap is shown at playbook end
    }

    /// Called when a task starts.
    async fn on_task_start(&self, _name: &str, _host: &str) {
        // Silent - we only show output on match
    }

    /// Called when a task completes - shows output only for matches.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Update total count
        *self.total_count.write() += 1;

        // Update host stats
        {
            let mut stats = self.host_stats.write();
            let host_stats = stats.entry(result.host.clone()).or_default();
            host_stats.total += 1;

            // Update status counts
            if result.result.skipped {
                host_stats.skipped += 1;
            } else if !result.result.success {
                host_stats.failed += 1;
                *self.has_failures.write() = true;
            } else if result.result.changed {
                host_stats.changed += 1;
            } else {
                host_stats.ok += 1;
            }
        }

        // Check if result matches filter criteria
        if self.result_matches(result) {
            // Update match counts
            *self.match_count.write() += 1;

            {
                let mut stats = self.host_stats.write();
                let host_stats = stats.entry(result.host.clone()).or_default();
                host_stats.matched += 1;
            }

            // Print matched result
            if self.config.verbose {
                println!("{}", self.format_verbose_match(result));
            } else {
                println!("{}", self.format_match(result));
            }
        }
    }

    /// Called when a handler is triggered.
    async fn on_handler_triggered(&self, _name: &str) {
        // Silent - handlers are internal details
    }

    /// Called when facts are gathered.
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // Silent - fact gathering is internal
    }
}

/// Builder for configuring SelectiveCallback.
#[derive(Debug, Clone, Default)]
pub struct SelectiveBuilder {
    config: SelectiveConfig,
}

impl SelectiveBuilder {
    /// Creates a new builder with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add hosts to the filter list.
    #[must_use]
    pub fn hosts(mut self, hosts: &[&str]) -> Self {
        for host in hosts {
            self.config.hosts.insert((*host).to_string());
        }
        self
    }

    /// Add a host pattern (regex) to the filter.
    pub fn host_pattern(mut self, pattern: &str) -> Result<Self, regex::Error> {
        // Validate the regex
        Regex::new(pattern)?;
        self.config.host_patterns.push(pattern.to_string());
        Ok(self)
    }

    /// Set host filter mode to exclude.
    #[must_use]
    pub fn exclude_hosts(mut self) -> Self {
        self.config.host_mode = FilterMode::Exclude;
        self
    }

    /// Add tasks to the filter list.
    #[must_use]
    pub fn tasks(mut self, tasks: &[&str]) -> Self {
        for task in tasks {
            self.config.tasks.insert((*task).to_string());
        }
        self
    }

    /// Add a task pattern (regex) to the filter.
    pub fn task_pattern(mut self, pattern: &str) -> Result<Self, regex::Error> {
        // Validate the regex
        Regex::new(pattern)?;
        self.config.task_patterns.push(pattern.to_string());
        Ok(self)
    }

    /// Set task filter mode to exclude.
    #[must_use]
    pub fn exclude_tasks(mut self) -> Self {
        self.config.task_mode = FilterMode::Exclude;
        self
    }

    /// Filter to show only failures.
    #[must_use]
    pub fn failures_only(mut self) -> Self {
        self.config.status_filter.failures_only = true;
        self
    }

    /// Filter to show only changes.
    #[must_use]
    pub fn changes_only(mut self) -> Self {
        self.config.status_filter.changes_only = true;
        self
    }

    /// Filter to show only skipped tasks.
    #[must_use]
    pub fn skipped_only(mut self) -> Self {
        self.config.status_filter.skipped_only = true;
        self
    }

    /// Filter to show specific statuses.
    #[must_use]
    pub fn statuses(mut self, statuses: &[&str]) -> Self {
        self.config.status_filter.statuses = statuses.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Add tags to the filter list.
    #[must_use]
    pub fn tags(mut self, tags: &[&str]) -> Self {
        for tag in tags {
            self.config.tags.insert((*tag).to_string());
        }
        self
    }

    /// Set tag filter mode to exclude.
    #[must_use]
    pub fn exclude_tags(mut self) -> Self {
        self.config.tag_mode = FilterMode::Exclude;
        self
    }

    /// Enable verbose output for matches.
    #[must_use]
    pub fn verbose(mut self) -> Self {
        self.config.verbose = true;
        self
    }

    /// Always show recap even if no matches.
    #[must_use]
    pub fn always_recap(mut self) -> Self {
        self.config.always_recap = true;
        self
    }

    /// Set a custom prefix for matched output.
    #[must_use]
    pub fn match_prefix(mut self, prefix: &str) -> Self {
        self.config.match_prefix = Some(prefix.to_string());
        self
    }

    /// Build the SelectiveCallback with the configured options.
    #[must_use]
    pub fn build(self) -> SelectiveCallback {
        SelectiveCallback::new(self.config)
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
        success: bool,
        changed: bool,
        skipped: bool,
        message: &str,
    ) -> ExecutionResult {
        ExecutionResult {
            host: host.to_string(),
            task_name: task_name.to_string(),
            result: ModuleResult {
                success,
                changed,
                message: message.to_string(),
                skipped,
                data: None,
                warnings: Vec::new(),
            },
            duration: Duration::from_millis(100),
            notify: Vec::new(),
        }
    }

    #[test]
    fn test_selective_callback_host_filter() {
        let callback = SelectiveCallback::builder()
            .hosts(&["host1", "host2"])
            .build();

        assert!(callback.host_matches("host1"));
        assert!(callback.host_matches("host2"));
        assert!(!callback.host_matches("host3"));
    }

    #[test]
    fn test_selective_callback_host_exclude() {
        let callback = SelectiveCallback::builder()
            .hosts(&["host1"])
            .exclude_hosts()
            .build();

        assert!(!callback.host_matches("host1"));
        assert!(callback.host_matches("host2"));
    }

    #[test]
    fn test_selective_callback_host_pattern() {
        let callback = SelectiveCallback::builder()
            .host_pattern(r"web-\d+")
            .unwrap()
            .build();

        assert!(callback.host_matches("web-01"));
        assert!(callback.host_matches("web-99"));
        assert!(!callback.host_matches("db-01"));
    }

    #[test]
    fn test_selective_callback_task_filter() {
        let callback = SelectiveCallback::builder()
            .tasks(&["Install nginx"])
            .build();

        assert!(callback.task_matches("Install nginx"));
        assert!(!callback.task_matches("Configure nginx"));
    }

    #[test]
    fn test_selective_callback_task_pattern() {
        let callback = SelectiveCallback::builder()
            .task_pattern(r"(?i)install.*nginx")
            .unwrap()
            .build();

        assert!(callback.task_matches("Install nginx"));
        assert!(callback.task_matches("INSTALL Nginx server"));
        assert!(!callback.task_matches("Configure nginx"));
    }

    #[test]
    fn test_selective_callback_failures_only() {
        let callback = SelectiveCallback::failures_only();

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        let failed_result = create_execution_result("host1", "task2", false, false, false, "error");

        assert!(!callback.result_matches(&ok_result));
        assert!(callback.result_matches(&failed_result));
    }

    #[test]
    fn test_selective_callback_changes_only() {
        let callback = SelectiveCallback::changes_only();

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        let changed_result =
            create_execution_result("host1", "task2", true, true, false, "changed");

        assert!(!callback.result_matches(&ok_result));
        assert!(callback.result_matches(&changed_result));
    }

    #[test]
    fn test_selective_callback_tag_filter() {
        let callback = SelectiveCallback::builder().tags(&["deploy"]).build();

        // Register tags for tasks
        callback.register_task_tags("Install nginx", vec!["deploy".to_string()]);
        callback.register_task_tags("Configure nginx", vec!["config".to_string()]);

        assert!(callback.tags_match("Install nginx"));
        assert!(!callback.tags_match("Configure nginx"));
    }

    #[test]
    fn test_selective_callback_combined_filters() {
        let callback = SelectiveCallback::builder()
            .hosts(&["host1"])
            .failures_only()
            .build();

        let host1_ok = create_execution_result("host1", "task1", true, false, false, "ok");
        let host1_fail = create_execution_result("host1", "task2", false, false, false, "error");
        let host2_fail = create_execution_result("host2", "task1", false, false, false, "error");

        assert!(!callback.result_matches(&host1_ok));
        assert!(callback.result_matches(&host1_fail));
        assert!(!callback.result_matches(&host2_fail));
    }

    #[test]
    fn test_selective_callback_no_filters_matches_all() {
        let callback = SelectiveCallback::default();

        let result = create_execution_result("any-host", "any-task", true, false, false, "ok");

        assert!(callback.result_matches(&result));
    }

    #[test]
    fn test_status_filter_matches() {
        let failures_filter = StatusFilter::failures();
        let changes_filter = StatusFilter::changes();

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        let changed_result =
            create_execution_result("host1", "task2", true, true, false, "changed");
        let failed_result = create_execution_result("host1", "task3", false, false, false, "error");

        assert!(!failures_filter.matches(&ok_result));
        assert!(!failures_filter.matches(&changed_result));
        assert!(failures_filter.matches(&failed_result));

        assert!(!changes_filter.matches(&ok_result));
        assert!(changes_filter.matches(&changed_result));
        assert!(!changes_filter.matches(&failed_result));
    }

    #[test]
    fn test_builder_chaining() {
        let callback = SelectiveCallback::builder()
            .hosts(&["host1", "host2"])
            .tasks(&["task1"])
            .failures_only()
            .verbose()
            .always_recap()
            .match_prefix("[FILTER]")
            .build();

        assert!(callback.config.hosts.contains("host1"));
        assert!(callback.config.hosts.contains("host2"));
        assert!(callback.config.tasks.contains("task1"));
        assert!(callback.config.status_filter.failures_only);
        assert!(callback.config.verbose);
        assert!(callback.config.always_recap);
        assert_eq!(callback.config.match_prefix, Some("[FILTER]".to_string()));
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = SelectiveCallback::new(SelectiveConfig::default());
        let callback2 = callback1.clone();

        // Both should share the same underlying state
        assert!(Arc::ptr_eq(&callback1.host_stats, &callback2.host_stats));
        assert!(Arc::ptr_eq(
            &callback1.has_failures,
            &callback2.has_failures
        ));
        assert!(Arc::ptr_eq(&callback1.match_count, &callback2.match_count));
    }

    #[test]
    fn test_format_match_output() {
        let callback = SelectiveCallback::default();

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        let output = callback.format_match(&ok_result);
        assert!(output.contains("host1"));
        assert!(output.contains("task1"));
        assert!(output.contains("MATCHED"));

        let failed_result =
            create_execution_result("host1", "task1", false, false, false, "error msg");
        let output = callback.format_match(&failed_result);
        assert!(output.contains("error msg"));
    }
}
