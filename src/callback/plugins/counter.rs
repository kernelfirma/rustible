//! Counter callback plugin for Rustible.
//!
//! This plugin provides detailed progress tracking with running counts,
//! percentages, and ETA estimation based on timing data.
//!
//! # Features
//!
//! - Task progress: "Task 5/20 (25%)"
//! - Host progress: "Host 3/10"
//! - Running success/failure/changed/skipped counts
//! - Percentage completion
//! - ETA based on average task duration
//!
//! # Example Output
//!
//! ```text
//! PLAY [Configure webservers] *************************************************
//!
//! TASK [Install nginx] (1/5) ************************************************
//! Host 1/3 | webserver1 | ok
//! Host 2/3 | webserver2 | changed
//! Host 3/3 | webserver3 | ok
//! Progress: 20% | ok: 2 | changed: 1 | failed: 0 | skipped: 0 | ETA: 00:01:24
//!
//! RECAP ********************************************************************
//! webserver1 : ok=5 changed=2 failed=0 skipped=0
//! webserver2 : ok=4 changed=3 failed=0 skipped=0
//! webserver3 : ok=5 changed=1 failed=1 skipped=0
//!
//! Total: 14 ok, 6 changed, 1 failed, 0 skipped in 2m 35s
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

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
    /// Total number of completed tasks for this host.
    #[allow(dead_code)]
    fn total(&self) -> u32 {
        self.ok + self.changed + self.failed + self.skipped + self.unreachable
    }
}

/// Global execution statistics for progress tracking.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
struct GlobalStats {
    /// Total ok count across all hosts
    ok: u32,
    /// Total changed count across all hosts
    changed: u32,
    /// Total failed count across all hosts
    failed: u32,
    /// Total skipped count across all hosts
    skipped: u32,
    /// Total unreachable count across all hosts
    unreachable: u32,
}

impl GlobalStats {
    /// Total number of completed task executions.
    #[allow(dead_code)]
    fn total(&self) -> u32 {
        self.ok + self.changed + self.failed + self.skipped + self.unreachable
    }
}

/// Task timing information for ETA calculation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TaskTiming {
    /// When the task started
    start_time: Instant,
    /// Duration of completed tasks for averaging
    completed_durations: Vec<Duration>,
}

impl Default for TaskTiming {
    fn default() -> Self {
        Self {
            start_time: Instant::now(),
            completed_durations: Vec::new(),
        }
    }
}

impl TaskTiming {
    /// Calculate average task duration.
    fn average_duration(&self) -> Option<Duration> {
        if self.completed_durations.is_empty() {
            return None;
        }
        let total: Duration = self.completed_durations.iter().sum();
        Some(total / self.completed_durations.len() as u32)
    }

    /// Record a completed task duration.
    fn record_duration(&mut self, duration: Duration) {
        self.completed_durations.push(duration);
    }
}

/// Progress state for tracking current execution position.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
struct ProgressState {
    /// Current task index (0-based)
    current_task: usize,
    /// Total number of tasks in current play
    total_tasks: usize,
    /// Current host index (0-based) for the current task
    current_host: usize,
    /// Total number of hosts in current play
    total_hosts: usize,
    /// Name of current task
    current_task_name: String,
    /// Name of current play
    current_play_name: String,
    /// Hosts completed for current task
    hosts_completed_for_task: usize,
}

impl ProgressState {
    /// Calculate overall percentage completion.
    fn percentage(&self) -> f64 {
        if self.total_tasks == 0 || self.total_hosts == 0 {
            return 0.0;
        }

        let total_executions = self.total_tasks * self.total_hosts;
        let completed_executions =
            (self.current_task * self.total_hosts) + self.hosts_completed_for_task;

        (completed_executions as f64 / total_executions as f64) * 100.0
    }

    /// Calculate ETA based on average task duration.
    fn eta(&self, avg_duration: Option<Duration>) -> Option<Duration> {
        let avg = avg_duration?;

        if self.total_tasks == 0 || self.total_hosts == 0 {
            return None;
        }

        let total_executions = self.total_tasks * self.total_hosts;
        let completed_executions =
            (self.current_task * self.total_hosts) + self.hosts_completed_for_task;
        let remaining = total_executions.saturating_sub(completed_executions);

        Some(avg * remaining as u32)
    }
}

/// Configuration for the counter callback.
#[derive(Debug, Clone)]
pub struct CounterConfig {
    /// Whether to show verbose per-host output
    pub verbose: bool,
    /// Whether to show ETA estimates
    pub show_eta: bool,
    /// Whether to use colored output
    pub use_color: bool,
    /// Known total number of tasks (for accurate progress)
    pub total_tasks: Option<usize>,
}

impl Default for CounterConfig {
    fn default() -> Self {
        Self {
            verbose: true,
            show_eta: true,
            use_color: true,
            total_tasks: None,
        }
    }
}

/// Counter callback plugin that shows detailed progress tracking.
///
/// This callback provides real-time progress information including
/// task counts, host progress, success/failure rates, and ETA.
///
/// # Design Principles
///
/// 1. **Progress Visibility**: Show "Task X/Y" and "Host A/B" progress
/// 2. **Running Counts**: Display ok/changed/failed/skipped in real-time
/// 3. **ETA Estimation**: Calculate remaining time based on task averages
/// 4. **Percentage Tracking**: Overall completion percentage
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::CounterCallback;
///
/// let callback = CounterCallback::new();
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct CounterCallback {
    /// Configuration
    config: CounterConfig,
    /// Per-host execution statistics
    host_stats: RwLock<HashMap<String, HostStats>>,
    /// Global execution statistics
    global_stats: RwLock<GlobalStats>,
    /// Progress tracking state
    progress: RwLock<ProgressState>,
    /// Timing information for ETA calculation
    timing: RwLock<TaskTiming>,
    /// Playbook start time for duration tracking
    start_time: RwLock<Option<Instant>>,
    /// Current playbook name
    playbook_name: RwLock<Option<String>>,
    /// Whether any failures occurred (for exit code)
    has_failures: RwLock<bool>,
    /// Task start time for individual task duration tracking
    task_start_time: RwLock<Option<Instant>>,
}

impl CounterCallback {
    /// Creates a new counter callback plugin with default settings.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = CounterCallback::new();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(CounterConfig::default())
    }

    /// Creates a counter callback with custom configuration.
    #[must_use]
    pub fn with_config(config: CounterConfig) -> Self {
        // Respect NO_COLOR environment variable
        let use_color = config.use_color && std::env::var("NO_COLOR").is_err();

        let mut actual_config = config;
        actual_config.use_color = use_color;

        Self {
            config: actual_config,
            host_stats: RwLock::new(HashMap::new()),
            global_stats: RwLock::new(GlobalStats::default()),
            progress: RwLock::new(ProgressState::default()),
            timing: RwLock::new(TaskTiming::default()),
            start_time: RwLock::new(None),
            playbook_name: RwLock::new(None),
            has_failures: RwLock::new(false),
            task_start_time: RwLock::new(None),
        }
    }

    /// Creates a counter callback with custom verbosity settings.
    ///
    /// # Arguments
    ///
    /// * `verbose` - Whether to show per-host results
    /// * `show_eta` - Whether to show ETA estimates
    #[must_use]
    pub fn with_options(verbose: bool, show_eta: bool) -> Self {
        Self::with_config(CounterConfig {
            verbose,
            show_eta,
            ..Default::default()
        })
    }

    /// Returns whether any failures occurred during execution.
    ///
    /// Useful for determining exit codes in CI/CD.
    pub fn has_failures(&self) -> bool {
        *self.has_failures.read()
    }

    /// Format a duration as HH:MM:SS or MM:SS.
    fn format_duration(duration: Duration) -> String {
        let total_secs = duration.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;

        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{:02}:{:02}", minutes, seconds)
        }
    }

    /// Format a duration as human-readable string (e.g., "2m 35s").
    fn format_duration_human(duration: Duration) -> String {
        let total_secs = duration.as_secs();
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;

        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }

    /// Format the task header with progress.
    fn format_task_header(&self, name: &str, current: usize, total: usize) -> String {
        let progress = format!("({}/{})", current, total);
        let header = format!("TASK [{}] {}", name, progress);
        let stars = "*".repeat(80_usize.saturating_sub(header.len() + 1));

        if self.config.use_color {
            format!("{} {}", header.cyan().bold(), stars.bright_black())
        } else {
            format!("{} {}", header, stars)
        }
    }

    /// Format the play header.
    fn format_play_header(&self, name: &str) -> String {
        let header = format!("PLAY [{}]", name);
        let stars = "*".repeat(80_usize.saturating_sub(header.len() + 1));

        if self.config.use_color {
            format!("{} {}", header.magenta().bold(), stars.bright_black())
        } else {
            format!("{} {}", header, stars)
        }
    }

    /// Format host result with progress.
    fn format_host_result(
        &self,
        host: &str,
        current_host: usize,
        total_hosts: usize,
        status: &str,
        changed: bool,
    ) -> String {
        let host_progress = format!("Host {}/{}", current_host, total_hosts);

        if self.config.use_color {
            let status_colored = match status {
                "ok" if changed => "changed".yellow().to_string(),
                "ok" => "ok".green().to_string(),
                "failed" => "failed".red().bold().to_string(),
                "skipped" => "skipped".cyan().to_string(),
                "unreachable" => "unreachable".magenta().bold().to_string(),
                _ => status.to_string(),
            };

            format!(
                "{} | {} | {}",
                host_progress.bright_black(),
                host.bright_white(),
                status_colored
            )
        } else {
            let status_str = if status == "ok" && changed {
                "changed"
            } else {
                status
            };
            format!("{} | {} | {}", host_progress, host, status_str)
        }
    }

    /// Format the progress line with counts and ETA.
    fn format_progress_line(
        &self,
        percentage: f64,
        stats: &GlobalStats,
        eta: Option<Duration>,
    ) -> String {
        let pct_str = format!("{:.0}%", percentage);

        if self.config.use_color {
            let counts = format!(
                "ok: {} | changed: {} | failed: {} | skipped: {}",
                stats.ok.to_string().green(),
                stats.changed.to_string().yellow(),
                stats.failed.to_string().red(),
                stats.skipped.to_string().cyan(),
            );

            let eta_str = if self.config.show_eta {
                eta.map(|d| format!(" | ETA: {}", Self::format_duration(d)))
                    .unwrap_or_default()
            } else {
                String::new()
            };

            format!(
                "Progress: {} | {}{}",
                pct_str.bright_white().bold(),
                counts,
                eta_str.bright_black()
            )
        } else {
            let counts = format!(
                "ok: {} | changed: {} | failed: {} | skipped: {}",
                stats.ok, stats.changed, stats.failed, stats.skipped,
            );

            let eta_str = if self.config.show_eta {
                eta.map(|d| format!(" | ETA: {}", Self::format_duration(d)))
                    .unwrap_or_default()
            } else {
                String::new()
            };

            format!("Progress: {} | {}{}", pct_str, counts, eta_str)
        }
    }

    /// Format a single host's recap line.
    fn format_recap_line(&self, host: &str, stats: &HostStats) -> String {
        if self.config.use_color {
            let host_color = if stats.failed > 0 || stats.unreachable > 0 {
                host.red().bold()
            } else if stats.changed > 0 {
                host.yellow()
            } else {
                host.green()
            };

            format!(
                "{:<30} : ok={} changed={} failed={} skipped={}",
                host_color,
                stats.ok.to_string().green(),
                stats.changed.to_string().yellow(),
                stats.failed.to_string().red(),
                stats.skipped.to_string().cyan(),
            )
        } else {
            format!(
                "{:<30} : ok={} changed={} failed={} skipped={}",
                host, stats.ok, stats.changed, stats.failed, stats.skipped,
            )
        }
    }

    /// Format the final summary line.
    fn format_summary(&self, stats: &GlobalStats, duration: Duration) -> String {
        if self.config.use_color {
            format!(
                "\nTotal: {} ok, {} changed, {} failed, {} skipped in {}",
                stats.ok.to_string().green().bold(),
                stats.changed.to_string().yellow().bold(),
                stats.failed.to_string().red().bold(),
                stats.skipped.to_string().cyan().bold(),
                Self::format_duration_human(duration).bright_white().bold()
            )
        } else {
            format!(
                "\nTotal: {} ok, {} changed, {} failed, {} skipped in {}",
                stats.ok,
                stats.changed,
                stats.failed,
                stats.skipped,
                Self::format_duration_human(duration)
            )
        }
    }
}

impl Default for CounterCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CounterCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            host_stats: RwLock::new(self.host_stats.read().clone()),
            global_stats: RwLock::new(self.global_stats.read().clone()),
            progress: RwLock::new(self.progress.read().clone()),
            timing: RwLock::new(self.timing.read().clone()),
            start_time: RwLock::new(*self.start_time.read()),
            playbook_name: RwLock::new(self.playbook_name.read().clone()),
            has_failures: RwLock::new(*self.has_failures.read()),
            task_start_time: RwLock::new(*self.task_start_time.read()),
        }
    }
}

#[async_trait]
impl ExecutionCallback for CounterCallback {
    /// Called when a playbook starts - initializes timing and resets stats.
    async fn on_playbook_start(&self, name: &str) {
        *self.start_time.write() = Some(Instant::now());
        *self.playbook_name.write() = Some(name.to_string());

        // Clear stats from any previous run
        self.host_stats.write().clear();
        *self.global_stats.write() = GlobalStats::default();
        *self.timing.write() = TaskTiming::default();
        *self.progress.write() = ProgressState::default();
        *self.has_failures.write() = false;

        println!();
    }

    /// Called when a playbook ends - prints the final recap and summary.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let stats = self.host_stats.read();
        let global = self.global_stats.read();
        let start_time = *self.start_time.read();

        // Print recap header
        println!();
        let recap_header = "RECAP";
        let stars = "*".repeat(80_usize.saturating_sub(recap_header.len() + 1));

        if self.config.use_color {
            println!(
                "{} {}",
                recap_header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("{} {}", recap_header, stars);
        }

        // Print recap for each host in sorted order
        let mut hosts: Vec<_> = stats.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_stats) = stats.get(host) {
                println!("{}", self.format_recap_line(host, host_stats));
            }
        }

        // Print summary with duration
        if let Some(start) = start_time {
            let duration = start.elapsed();
            println!("{}", self.format_summary(&global, duration));

            if self.config.use_color {
                let status = if success {
                    "completed successfully".green().bold()
                } else {
                    "failed".red().bold()
                };

                println!("\nPlaybook '{}' {}", name.bright_white(), status);
            } else {
                let status = if success {
                    "completed successfully"
                } else {
                    "failed"
                };
                println!("\nPlaybook '{}' {}", name, status);
            }
        }
    }

    /// Called when a play starts - initializes host tracking.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Initialize stats for all hosts in this play
        {
            let mut stats = self.host_stats.write();
            for host in hosts {
                stats.entry(host.clone()).or_default();
            }
        }

        // Update progress state
        {
            let mut progress = self.progress.write();
            progress.current_play_name = name.to_string();
            progress.total_hosts = hosts.len();
            progress.current_task = 0;
            progress.hosts_completed_for_task = 0;
        }

        println!("{}", self.format_play_header(name));
        println!();
    }

    /// Called when a play ends.
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Progress line already shown after each task
        println!();
    }

    /// Called when a task starts - updates progress tracking.
    async fn on_task_start(&self, name: &str, _host: &str) {
        let mut progress = self.progress.write();

        // Only print task header when task name changes
        if progress.current_task_name != name {
            progress.current_task += 1;
            progress.current_task_name = name.to_string();
            progress.hosts_completed_for_task = 0;

            // Get total tasks if available (this would normally come from play metadata)
            // For now, we'll show running count
            let total = if progress.total_tasks > 0 {
                progress.total_tasks
            } else {
                progress.current_task
            };

            println!(
                "{}",
                self.format_task_header(name, progress.current_task, total)
            );
        }

        // Record task start time
        *self.task_start_time.write() = Some(Instant::now());
    }

    /// Called when a task completes - updates counts and shows progress.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Update host stats
        let mut stats = self.host_stats.write();
        let host_stats = stats.entry(result.host.clone()).or_default();

        // Update global stats
        let mut global = self.global_stats.write();

        // Determine status and update counts
        let status = if result.result.skipped {
            host_stats.skipped += 1;
            global.skipped += 1;
            "skipped"
        } else if !result.result.success {
            host_stats.failed += 1;
            global.failed += 1;

            // Mark that we have failures
            *self.has_failures.write() = true;

            "failed"
        } else if result.result.changed {
            host_stats.changed += 1;
            global.changed += 1;
            "ok"
        } else {
            host_stats.ok += 1;
            global.ok += 1;
            "ok"
        };

        // Update progress
        let mut progress = self.progress.write();
        progress.hosts_completed_for_task += 1;
        let current_host = progress.hosts_completed_for_task;
        let total_hosts = progress.total_hosts;
        let percentage = progress.percentage();

        // Record task duration
        let task_start = *self.task_start_time.read();
        if let Some(start) = task_start {
            self.timing.write().record_duration(start.elapsed());
        }

        // Calculate ETA
        let timing = self.timing.read();
        let eta = progress.eta(timing.average_duration());

        drop(stats);
        drop(timing);
        drop(progress);

        // Print host result if verbose
        if self.config.verbose {
            println!(
                "{}",
                self.format_host_result(
                    &result.host,
                    current_host,
                    total_hosts,
                    status,
                    result.result.changed
                )
            );

            // Print error message for failures
            if status == "failed" && !result.result.message.is_empty() {
                if self.config.use_color {
                    println!("  {} {}", "=>".red(), result.result.message.bright_red());
                } else {
                    println!("  => {}", result.result.message);
                }
            }
        }

        // Print progress line after all hosts complete the task
        if current_host == total_hosts {
            println!("{}", self.format_progress_line(percentage, &global, eta));
            println!();
        }
    }

    /// Called when a handler is triggered.
    async fn on_handler_triggered(&self, name: &str) {
        if self.config.use_color {
            println!("{}: {}", "HANDLER".yellow().bold(), name.bright_white());
        } else {
            println!("HANDLER: {}", name);
        }
    }

    /// Called when facts are gathered.
    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        if self.config.verbose {
            if self.config.use_color {
                println!(
                    "{} {} | {}",
                    "FACTS".bright_black(),
                    host.bright_white(),
                    "gathered".green()
                );
            } else {
                println!("FACTS {} | gathered", host);
            }
        }
    }
}

/// Builder for configuring CounterCallback with custom settings.
#[derive(Debug, Default, Clone)]
pub struct CounterCallbackBuilder {
    config: CounterConfig,
}

impl CounterCallbackBuilder {
    /// Create a new builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: CounterConfig::default(),
        }
    }

    /// Set whether to show verbose per-host output.
    #[must_use]
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.config.verbose = verbose;
        self
    }

    /// Set whether to show ETA estimates.
    #[must_use]
    pub fn show_eta(mut self, show_eta: bool) -> Self {
        self.config.show_eta = show_eta;
        self
    }

    /// Set whether to use colored output.
    #[must_use]
    pub fn use_color(mut self, use_color: bool) -> Self {
        self.config.use_color = use_color;
        self
    }

    /// Set the known total number of tasks (for accurate progress).
    #[must_use]
    pub fn total_tasks(mut self, total: usize) -> Self {
        self.config.total_tasks = Some(total);
        self
    }

    /// Build the CounterCallback with configured settings.
    #[must_use]
    pub fn build(self) -> CounterCallback {
        let callback = CounterCallback::with_config(self.config.clone());

        // If total tasks is known, set it in progress state
        if let Some(total) = self.config.total_tasks {
            callback.progress.write().total_tasks = total;
        }

        callback
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

    #[tokio::test]
    async fn test_counter_callback_tracks_stats() {
        let callback = CounterCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        // Simulate task start
        callback.on_task_start("task1", "host1").await;

        // Simulate some task completions
        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        callback.on_task_start("task1", "host2").await;
        let changed_result =
            create_execution_result("host2", "task1", true, true, false, "changed");
        callback.on_task_complete(&changed_result).await;

        // Verify global stats
        let global = callback.global_stats.read();
        assert_eq!(global.ok, 1);
        assert_eq!(global.changed, 1);
        assert_eq!(global.failed, 0);
        assert_eq!(global.total(), 2);

        // Verify host stats
        let stats = callback.host_stats.read();
        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.ok, 1);
        assert_eq!(host1_stats.changed, 0);

        let host2_stats = stats.get("host2").unwrap();
        assert_eq!(host2_stats.changed, 1);
    }

    #[tokio::test]
    async fn test_counter_callback_tracks_failures() {
        let callback = CounterCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback.on_task_start("task1", "host1").await;
        let failed_result =
            create_execution_result("host1", "task1", false, false, false, "error occurred");
        callback.on_task_complete(&failed_result).await;

        assert!(callback.has_failures());

        let global = callback.global_stats.read();
        assert_eq!(global.failed, 1);
    }

    #[test]
    fn test_progress_percentage() {
        let mut progress = ProgressState {
            current_task: 2,
            total_tasks: 4,
            current_host: 0,
            total_hosts: 2,
            hosts_completed_for_task: 1,
            current_task_name: "task2".to_string(),
            current_play_name: "play".to_string(),
        };

        // 2 complete tasks * 2 hosts = 4 + 1 host on current task = 5 of 8 total
        assert!((progress.percentage() - 62.5).abs() < 0.1);

        progress.hosts_completed_for_task = 2;
        // 2 complete tasks * 2 hosts = 4 + 2 hosts on current task = 6 of 8 total = 75%
        assert!((progress.percentage() - 75.0).abs() < 0.1);
    }

    #[test]
    fn test_eta_calculation() {
        let mut timing = TaskTiming::default();
        timing.record_duration(Duration::from_secs(10));
        timing.record_duration(Duration::from_secs(20));
        timing.record_duration(Duration::from_secs(15));

        let avg = timing.average_duration().unwrap();
        assert_eq!(avg, Duration::from_secs(15));

        let progress = ProgressState {
            current_task: 1,
            total_tasks: 3,
            current_host: 0,
            total_hosts: 2,
            hosts_completed_for_task: 2,
            current_task_name: "task1".to_string(),
            current_play_name: "play".to_string(),
        };

        // total_executions = 3 * 2 = 6
        // completed_executions = (1 * 2) + 2 = 4
        // remaining = 6 - 4 = 2
        // ETA = 2 * 15s = 30s
        let eta = progress.eta(Some(avg)).unwrap();
        assert_eq!(eta, Duration::from_secs(30));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(
            CounterCallback::format_duration(Duration::from_secs(65)),
            "01:05"
        );
        assert_eq!(
            CounterCallback::format_duration(Duration::from_secs(3665)),
            "01:01:05"
        );
        assert_eq!(
            CounterCallback::format_duration(Duration::from_secs(0)),
            "00:00"
        );
    }

    #[test]
    fn test_format_duration_human() {
        assert_eq!(
            CounterCallback::format_duration_human(Duration::from_secs(65)),
            "1m 5s"
        );
        assert_eq!(
            CounterCallback::format_duration_human(Duration::from_secs(3665)),
            "1h 1m 5s"
        );
        assert_eq!(
            CounterCallback::format_duration_human(Duration::from_secs(30)),
            "30s"
        );
    }

    #[test]
    fn test_host_stats_total() {
        let stats = HostStats {
            ok: 5,
            changed: 3,
            failed: 1,
            skipped: 2,
            unreachable: 0,
        };
        assert_eq!(stats.total(), 11);
    }

    #[test]
    fn test_global_stats_total() {
        let stats = GlobalStats {
            ok: 10,
            changed: 5,
            failed: 2,
            skipped: 3,
            unreachable: 1,
        };
        assert_eq!(stats.total(), 21);
    }

    #[test]
    fn test_builder_pattern() {
        let callback = CounterCallbackBuilder::new()
            .verbose(false)
            .show_eta(false)
            .use_color(false)
            .total_tasks(10)
            .build();

        assert!(!callback.config.verbose);
        assert!(!callback.config.show_eta);
        assert!(!callback.config.use_color);
    }

    #[test]
    fn test_default_trait() {
        let callback = CounterCallback::default();
        assert!(callback.config.verbose);
        // show_eta depends on default config
    }

    #[test]
    fn test_clone() {
        let callback1 = CounterCallback::new();
        {
            let mut stats = callback1.host_stats.write();
            stats.insert(
                "host1".to_string(),
                HostStats {
                    ok: 5,
                    ..Default::default()
                },
            );
        }

        let callback2 = callback1.clone();

        // Cloned callback should have same data
        let stats = callback2.host_stats.read();
        assert_eq!(stats.get("host1").map(|s| s.ok), Some(5));
    }
}
