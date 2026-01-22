//! Skippy Callback Plugin for Rustible.
//!
//! This plugin minimizes output noise by hiding skipped tasks completely
//! unless verbose mode is enabled. It focuses on showing only changed
//! and failed tasks, making it ideal for large playbooks.
//!
//! # Features
//!
//! - **Silent Skipped Tasks**: Skipped tasks produce no output unless verbose
//! - **Changed/Failed Focus**: Only shows tasks that made changes or failed
//! - **Compact Output**: Minimal formatting for cleaner logs
//! - **Summary Statistics**: Compact recap showing skip counts at end
//! - **Configurable Verbosity**: Three levels of detail
//!
//! # Example Output (Default Mode)
//!
//! ```text
//! PLAY [Configure webservers] *************************
//!
//! TASK [Install nginx] ********************************
//! changed: [web1]
//! changed: [web2]
//!
//! TASK [Deploy config] ********************************
//! changed: [web1]
//! failed: [web2] => Configuration file invalid
//!
//! PLAY RECAP ******************************************
//! web1: ok=3 changed=2 failed=0 skipped=5
//! web2: ok=2 changed=1 failed=1 skipped=5
//!
//! Skipped 10 tasks (use -v to show skipped)
//! ```
//!
//! # Verbosity Levels
//!
//! - **Level 0**: Hide all skipped tasks, show only changed/failed
//! - **Level 1**: Show skipped task names (no details)
//! - **Level 2+**: Show full output including skip reasons

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use tokio::sync::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Configuration for the Skippy callback plugin.
#[derive(Debug, Clone)]
pub struct SkippyConfig {
    /// Verbosity level (0 = hide skipped, 1 = show names, 2+ = full)
    pub verbosity: u8,
    /// Whether to show task timing information
    pub show_timing: bool,
    /// Whether to use colored output
    pub use_color: bool,
    /// Maximum task name length before truncation
    pub max_task_name_length: usize,
    /// Show host count instead of individual hosts for large inventories
    pub aggregate_hosts_threshold: usize,
    /// Whether to show the "skipped N tasks" summary
    pub show_skip_summary: bool,
}

impl Default for SkippyConfig {
    fn default() -> Self {
        Self {
            verbosity: 0,
            show_timing: false,
            use_color: true,
            max_task_name_length: 60,
            aggregate_hosts_threshold: 10,
            show_skip_summary: true,
        }
    }
}

impl SkippyConfig {
    /// Creates a configuration with specified verbosity.
    pub fn with_verbosity(verbosity: u8) -> Self {
        Self {
            verbosity,
            ..Default::default()
        }
    }

    /// Enables timing display.
    pub fn with_timing(mut self) -> Self {
        self.show_timing = true;
        self
    }

    /// Disables colored output.
    pub fn without_color(mut self) -> Self {
        self.use_color = false;
        self
    }
}

/// Statistics tracked per host during execution.
#[derive(Debug, Clone, Default)]
struct HostStats {
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

impl HostStats {
    /// Returns true if this host had any issues.
    fn has_issues(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }

    /// Returns true if this host had any changes.
    fn has_changes(&self) -> bool {
        self.changed > 0
    }
}

/// Information about the current task being executed.
#[derive(Debug, Clone)]
struct CurrentTask {
    /// Task name
    name: String,
    /// Results collected so far for this task
    results: Vec<TaskHostResult>,
    /// When the task started
    #[allow(dead_code)]
    start_time: Instant,
    /// Whether the header has been printed
    header_printed: bool,
}

/// Result for a single host in a task.
#[derive(Debug, Clone)]
struct TaskHostResult {
    host: String,
    status: TaskHostStatus,
    message: Option<String>,
    duration: Duration,
}

/// Status of a task on a single host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskHostStatus {
    Ok,
    Changed,
    Failed,
    Skipped,
    Unreachable,
}

/// Skippy Callback Plugin - minimizes skipped task output.
///
/// This callback is designed for large playbooks where the majority of tasks
/// are skipped due to conditions. It reduces noise by only showing tasks that
/// actually made changes or failed.
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::SkippyCallback;
///
/// // Default: hide all skipped tasks
/// let callback = SkippyCallback::new();
///
/// // With verbosity: show skipped task names
/// let callback = SkippyCallback::with_verbosity(1);
///
/// // Full configuration
/// let config = SkippyConfig {
///     verbosity: 1,
///     show_timing: true,
///     ..Default::default()
/// };
/// let callback = SkippyCallback::with_config(config);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SkippyCallback {
    /// Configuration
    config: SkippyConfig,
    /// Per-host execution statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Current task being executed
    current_task: Arc<RwLock<Option<CurrentTask>>>,
    /// Playbook start time
    start_time: Arc<RwLock<Option<Instant>>>,
    /// Current playbook name
    #[allow(dead_code)]
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Total count of skipped tasks
    total_skipped: Arc<RwLock<u32>>,
    /// Whether any failures occurred
    has_failures: Arc<RwLock<bool>>,
    /// Names of tasks that were entirely skipped (for summary)
    fully_skipped_tasks: Arc<RwLock<Vec<String>>>,
}

impl SkippyCallback {
    /// Creates a new Skippy callback with default configuration.
    ///
    /// Default configuration hides all skipped tasks and only shows
    /// changed/failed tasks.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(SkippyConfig::default())
    }

    /// Creates a new Skippy callback with specified verbosity.
    ///
    /// # Arguments
    ///
    /// * `verbosity` - 0 = hide skipped, 1 = show names, 2+ = full
    #[must_use]
    pub fn with_verbosity(verbosity: u8) -> Self {
        Self::with_config(SkippyConfig::with_verbosity(verbosity))
    }

    /// Creates a new Skippy callback with custom configuration.
    #[must_use]
    pub fn with_config(config: SkippyConfig) -> Self {
        Self {
            config,
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            current_task: Arc::new(RwLock::new(None)),
            start_time: Arc::new(RwLock::new(None)),
            playbook_name: Arc::new(RwLock::new(None)),
            total_skipped: Arc::new(RwLock::new(0)),
            has_failures: Arc::new(RwLock::new(false)),
            fully_skipped_tasks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Returns whether any failures occurred during execution.
    pub async fn has_failures(&self) -> bool {
        *self.has_failures.read().await
    }

    /// Returns the total number of skipped task executions.
    pub async fn total_skipped(&self) -> u32 {
        *self.total_skipped.read().await
    }

    /// Returns the list of fully skipped tasks.
    pub async fn fully_skipped_tasks(&self) -> Vec<String> {
        self.fully_skipped_tasks.read().await.clone()
    }

    /// Truncates a task name if it exceeds the maximum length.
    fn truncate_name(&self, name: &str) -> String {
        if name.len() <= self.config.max_task_name_length {
            name.to_string()
        } else {
            format!(
                "{}...",
                &name[..self.config.max_task_name_length.saturating_sub(3)]
            )
        }
    }

    /// Formats a task header line.
    fn format_task_header(&self, name: &str) -> String {
        let truncated = self.truncate_name(name);
        let padding_len = 70usize.saturating_sub(truncated.len() + 7);
        let padding = "*".repeat(padding_len);

        if self.config.use_color {
            format!(
                "\n{} [{}] {}",
                "TASK".bright_cyan().bold(),
                truncated.bright_white(),
                padding.bright_black()
            )
        } else {
            format!("\nTASK [{}] {}", truncated, padding)
        }
    }

    /// Formats a play header line.
    fn format_play_header(&self, name: &str, host_count: usize) -> String {
        let padding_len = 70usize.saturating_sub(name.len() + 7);
        let padding = "*".repeat(padding_len);

        if self.config.use_color {
            format!(
                "\n{} [{}] {} ({} hosts)",
                "PLAY".bright_magenta().bold(),
                name.bright_white().bold(),
                padding.bright_black(),
                host_count
            )
        } else {
            format!("\nPLAY [{}] {} ({} hosts)", name, padding, host_count)
        }
    }

    /// Formats a host result line.
    fn format_host_result(&self, result: &TaskHostResult) -> String {
        let status_str = match result.status {
            TaskHostStatus::Ok => {
                if self.config.use_color {
                    "ok".green().to_string()
                } else {
                    "ok".to_string()
                }
            }
            TaskHostStatus::Changed => {
                if self.config.use_color {
                    "changed".yellow().to_string()
                } else {
                    "changed".to_string()
                }
            }
            TaskHostStatus::Failed => {
                if self.config.use_color {
                    "failed".red().bold().to_string()
                } else {
                    "FAILED".to_string()
                }
            }
            TaskHostStatus::Skipped => {
                if self.config.use_color {
                    "skipping".cyan().to_string()
                } else {
                    "skipping".to_string()
                }
            }
            TaskHostStatus::Unreachable => {
                if self.config.use_color {
                    "unreachable".magenta().bold().to_string()
                } else {
                    "UNREACHABLE".to_string()
                }
            }
        };

        let host_str = if self.config.use_color {
            result.host.bright_white().to_string()
        } else {
            result.host.clone()
        };

        let timing_str = if self.config.show_timing {
            format!(" ({:.2}s)", result.duration.as_secs_f64())
        } else {
            String::new()
        };

        let message_str = match (&result.status, &result.message) {
            (TaskHostStatus::Failed | TaskHostStatus::Unreachable, Some(msg)) => {
                format!(" => {}", msg)
            }
            _ => String::new(),
        };

        format!(
            "{}: [{}]{}{}",
            status_str, host_str, timing_str, message_str
        )
    }

    /// Formats the recap line for a single host.
    fn format_recap_line(&self, host: &str, stats: &HostStats) -> String {
        let host_color = if stats.has_issues() {
            if self.config.use_color {
                host.red().bold().to_string()
            } else {
                host.to_uppercase()
            }
        } else if stats.has_changes() {
            if self.config.use_color {
                host.yellow().to_string()
            } else {
                host.to_string()
            }
        } else if self.config.use_color {
            host.green().to_string()
        } else {
            host.to_string()
        };

        let format_num = |n: u32, is_error: bool| -> String {
            if self.config.use_color {
                if is_error && n > 0 {
                    n.to_string().red().bold().to_string()
                } else if n > 0 {
                    n.to_string().yellow().to_string()
                } else {
                    n.to_string().bright_black().to_string()
                }
            } else {
                n.to_string()
            }
        };

        format!(
            "{:<20} : ok={} changed={} failed={} skipped={} unreachable={}",
            host_color,
            format_num(stats.ok, false),
            format_num(stats.changed, false),
            format_num(stats.failed, true),
            format_num(stats.skipped, false),
            format_num(stats.unreachable, true),
        )
    }

    /// Flushes the current task results if any should be shown.
    async fn flush_task(&self) {
        let mut current = self.current_task.write().await;

        if let Some(task) = current.take() {
            // Check if we have any non-skipped results
            let has_visible_results = task
                .results
                .iter()
                .any(|r| r.status != TaskHostStatus::Skipped || self.config.verbosity >= 1);

            let all_skipped = task
                .results
                .iter()
                .all(|r| r.status == TaskHostStatus::Skipped);

            if has_visible_results && !task.results.is_empty() {
                // Print header if we have results to show
                if !task.header_printed {
                    println!("{}", self.format_task_header(&task.name));
                }

                // Print each result based on verbosity
                for result in &task.results {
                    let should_print = match result.status {
                        TaskHostStatus::Ok => self.config.verbosity >= 2,
                        TaskHostStatus::Changed => true,
                        TaskHostStatus::Failed => true,
                        TaskHostStatus::Skipped => self.config.verbosity >= 1,
                        TaskHostStatus::Unreachable => true,
                    };

                    if should_print {
                        println!("{}", self.format_host_result(result));
                    }
                }
            }

            // Track fully skipped tasks for summary
            if all_skipped && !task.results.is_empty() {
                let mut skipped_tasks = self.fully_skipped_tasks.write().await;
                if !skipped_tasks.contains(&task.name) {
                    skipped_tasks.push(task.name.clone());
                }
            }
        }
    }

    /// Prints the skipped tasks summary if configured.
    async fn print_skip_summary(&self) {
        if !self.config.show_skip_summary {
            return;
        }

        let total = *self.total_skipped.read().await;
        let skipped_tasks = self.fully_skipped_tasks.read().await;

        if total == 0 {
            return;
        }

        let msg = if self.config.verbosity == 0 {
            format!(
                "Skipped {} task execution(s) across {} task(s) (use -v to show)",
                total,
                skipped_tasks.len()
            )
        } else {
            format!(
                "Skipped {} task execution(s) across {} task(s)",
                total,
                skipped_tasks.len()
            )
        };

        if self.config.use_color {
            println!("\n{}", msg.bright_black());
        } else {
            println!("\n{}", msg);
        }
    }
}

impl Default for SkippyCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SkippyCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            host_stats: Arc::clone(&self.host_stats),
            current_task: Arc::clone(&self.current_task),
            start_time: Arc::clone(&self.start_time),
            playbook_name: Arc::clone(&self.playbook_name),
            total_skipped: Arc::clone(&self.total_skipped),
            has_failures: Arc::clone(&self.has_failures),
            fully_skipped_tasks: Arc::clone(&self.fully_skipped_tasks),
        }
    }
}

#[async_trait]
impl ExecutionCallback for SkippyCallback {
    async fn on_playbook_start(&self, name: &str) {
        // Initialize state
        *self.start_time.write().await = Some(Instant::now());
        *self.playbook_name.write().await = Some(name.to_string());
        self.host_stats.write().await.clear();
        *self.total_skipped.write().await = 0;
        *self.has_failures.write().await = false;
        self.fully_skipped_tasks.write().await.clear();
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        // Flush any pending task
        self.flush_task().await;

        let stats = self.host_stats.read().await;
        let start_time = self.start_time.read().await;

        // Print recap header
        let header = if self.config.use_color {
            format!(
                "\n{} {}",
                "PLAY RECAP".bright_white().bold(),
                "*".repeat(60).bright_black()
            )
        } else {
            format!("\nPLAY RECAP {}", "*".repeat(60))
        };
        println!("{}", header);

        // Print recap for each host in sorted order
        let mut hosts: Vec<_> = stats.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_stats) = stats.get(host) {
                println!("{}", self.format_recap_line(host, host_stats));
            }
        }

        // Print skip summary
        self.print_skip_summary().await;

        // Print duration
        if let Some(start) = *start_time {
            let duration = start.elapsed();
            let playbook_status = if success {
                if self.config.use_color {
                    "ok".green().bold().to_string()
                } else {
                    "ok".to_string()
                }
            } else if self.config.use_color {
                "failed".red().bold().to_string()
            } else {
                "FAILED".to_string()
            };

            let playbook_display = if self.config.use_color {
                name.bright_white().bold().to_string()
            } else {
                name.to_string()
            };

            println!(
                "\nPlaybook '{}' finished: {} in {:.2}s",
                playbook_display,
                playbook_status,
                duration.as_secs_f64()
            );
        }
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Flush any pending task from previous play
        self.flush_task().await;

        // Initialize stats for all hosts
        let mut stats = self.host_stats.write().await;
        for host in hosts {
            stats.entry(host.clone()).or_default();
        }

        // Print play header
        println!("{}", self.format_play_header(name, hosts.len()));
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Flush any pending task
        self.flush_task().await;
    }

    async fn on_task_start(&self, name: &str, _host: &str) {
        // Check if this is a new task
        let current = self.current_task.write().await;

        let is_new_task = current.as_ref().map(|t| t.name != name).unwrap_or(true);

        if is_new_task {
            // Flush previous task if exists
            drop(current);
            self.flush_task().await;

            // Start new task
            let mut current = self.current_task.write().await;
            *current = Some(CurrentTask {
                name: name.to_string(),
                results: Vec::new(),
                start_time: Instant::now(),
                header_printed: false,
            });
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Update host stats
        let mut stats = self.host_stats.write().await;
        let host_stats = stats.entry(result.host.clone()).or_default();

        let task_status = if result.result.skipped {
            host_stats.skipped += 1;
            let mut total = self.total_skipped.write().await;
            *total += 1;
            TaskHostStatus::Skipped
        } else if !result.result.success {
            host_stats.failed += 1;
            *self.has_failures.write().await = true;
            TaskHostStatus::Failed
        } else if result.result.changed {
            host_stats.changed += 1;
            TaskHostStatus::Changed
        } else {
            host_stats.ok += 1;
            TaskHostStatus::Ok
        };

        drop(stats);

        // Add result to current task
        let mut current = self.current_task.write().await;

        if let Some(task) = current.as_mut() {
            // Check if we need to print header immediately for failures
            let should_print_header =
                matches!(task_status, TaskHostStatus::Failed | TaskHostStatus::Unreachable)
                    && !task.header_printed;

            if should_print_header {
                println!("{}", self.format_task_header(&task.name));
                task.header_printed = true;
            }

            let host_result = TaskHostResult {
                host: result.host.clone(),
                status: task_status,
                message: if !result.result.success || task_status == TaskHostStatus::Unreachable {
                    Some(result.result.message.clone())
                } else {
                    None
                },
                duration: result.duration,
            };

            // For failures, print immediately
            if matches!(task_status, TaskHostStatus::Failed | TaskHostStatus::Unreachable) {
                println!("{}", self.format_host_result(&host_result));
            }

            task.results.push(host_result);
        }
    }

    async fn on_handler_triggered(&self, _name: &str) {
        // Silent - handlers are internal details
    }

    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // Silent - fact gathering is internal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;

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

    #[tokio::test]
    async fn test_skippy_callback_default() {
        let callback = SkippyCallback::new();
        assert_eq!(callback.config.verbosity, 0);
        assert!(callback.config.show_skip_summary);
    }

    #[tokio::test]
    async fn test_skippy_callback_with_verbosity() {
        let callback = SkippyCallback::with_verbosity(2);
        assert_eq!(callback.config.verbosity, 2);
    }

    #[tokio::test]
    async fn test_skippy_tracks_skipped() {
        let callback = SkippyCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // Skipped task
        callback.on_task_start("skipped-task", "host1").await;
        let skipped =
            create_execution_result("host1", "skipped-task", true, false, true, "skipped");
        callback.on_task_complete(&skipped).await;

        assert_eq!(callback.total_skipped().await, 1);

        let stats = callback.host_stats.read().await;
        let host_stats = stats.get("host1").unwrap();
        assert_eq!(host_stats.skipped, 1);
    }

    #[tokio::test]
    async fn test_skippy_tracks_changed() {
        let callback = SkippyCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback.on_task_start("change-task", "host1").await;
        let changed = create_execution_result("host1", "change-task", true, true, false, "changed");
        callback.on_task_complete(&changed).await;

        let stats = callback.host_stats.read().await;
        let host_stats = stats.get("host1").unwrap();
        assert_eq!(host_stats.changed, 1);
        assert!(host_stats.has_changes());
    }

    #[tokio::test]
    async fn test_skippy_tracks_failures() {
        let callback = SkippyCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback.on_task_start("failed-task", "host1").await;
        let failed = create_execution_result("host1", "failed-task", false, false, false, "error");
        callback.on_task_complete(&failed).await;

        assert!(callback.has_failures().await);

        let stats = callback.host_stats.read().await;
        let host_stats = stats.get("host1").unwrap();
        assert_eq!(host_stats.failed, 1);
        assert!(host_stats.has_issues());
    }

    #[tokio::test]
    async fn test_skippy_fully_skipped_tasks() {
        let callback = SkippyCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        // Task skipped on all hosts
        callback.on_task_start("all-skipped", "host1").await;
        let skipped1 =
            create_execution_result("host1", "all-skipped", true, false, true, "skipped");
        callback.on_task_complete(&skipped1).await;

        callback.on_task_start("all-skipped", "host2").await;
        let skipped2 =
            create_execution_result("host2", "all-skipped", true, false, true, "skipped");
        callback.on_task_complete(&skipped2).await;

        // Flush to process
        callback.flush_task().await;

        let skipped_tasks = callback.fully_skipped_tasks().await;
        assert!(skipped_tasks.contains(&"all-skipped".to_string()));
    }

    #[tokio::test]
    async fn test_skippy_mixed_results() {
        let callback = SkippyCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // Mix of results
        callback.on_task_start("task1", "host1").await;
        let ok = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok).await;

        callback.on_task_start("task2", "host1").await;
        let changed = create_execution_result("host1", "task2", true, true, false, "changed");
        callback.on_task_complete(&changed).await;

        callback.on_task_start("task3", "host1").await;
        let skipped = create_execution_result("host1", "task3", true, false, true, "skipped");
        callback.on_task_complete(&skipped).await;

        let stats = callback.host_stats.read().await;
        let host_stats = stats.get("host1").unwrap();
        assert_eq!(host_stats.ok, 1);
        assert_eq!(host_stats.changed, 1);
        assert_eq!(host_stats.skipped, 1);
        assert_eq!(host_stats.failed, 0);
    }

    #[test]
    fn test_truncate_name() {
        let config = SkippyConfig {
            max_task_name_length: 20,
            ..Default::default()
        };
        let callback = SkippyCallback::with_config(config);

        assert_eq!(callback.truncate_name("short"), "short");
        assert_eq!(
            callback.truncate_name("this is a very long task name that exceeds the limit"),
            "this is a very lo..."
        );
    }

    #[test]
    fn test_host_stats_has_issues() {
        let mut stats = HostStats::default();
        assert!(!stats.has_issues());

        stats.failed = 1;
        assert!(stats.has_issues());

        stats.failed = 0;
        stats.unreachable = 1;
        assert!(stats.has_issues());
    }

    #[test]
    fn test_host_stats_has_changes() {
        let mut stats = HostStats::default();
        assert!(!stats.has_changes());

        stats.changed = 1;
        assert!(stats.has_changes());
    }

    #[test]
    fn test_config_builder() {
        let config = SkippyConfig::with_verbosity(1)
            .with_timing()
            .without_color();

        assert_eq!(config.verbosity, 1);
        assert!(config.show_timing);
        assert!(!config.use_color);
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let callback1 = SkippyCallback::new();
        let callback2 = callback1.clone();

        callback1.on_playbook_start("test").await;

        // Both should share the same state
        assert!(callback2.start_time.read().await.is_some());
    }

    #[test]
    fn test_default_trait() {
        let callback = SkippyCallback::default();
        assert_eq!(callback.config.verbosity, 0);
    }
}
