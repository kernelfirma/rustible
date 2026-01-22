//! Timer Callback Plugin for Rustible
//!
//! This plugin tracks and displays execution timing information for tasks,
//! providing insights into performance and identifying slow operations.
//!
//! # Features
//!
//! - Tracks execution time for each task on each host
//! - Displays elapsed time after each task completion
//! - Provides summary of slowest tasks at playbook end
//! - Configurable display options (threshold, top N slowest, etc.)
//! - Thread-safe for parallel execution
//! - Can be combined with other stdout plugins
//!
//! # Example Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::{TimerCallback, TimerConfig};
//!
//! let timer = TimerCallback::new(TimerConfig {
//!     show_per_task: true,
//!     show_summary: true,
//!     top_slowest: 10,
//!     threshold_secs: 0.0,
//!     ..Default::default()
//! });
//!
//! // Use with playbook executor
//! # let _ = ();
//! # Ok(())
//! # }
//! ```
//!
//! # Example Output
//!
//! ```text
//! TASK [Install nginx] *******************************************************
//!   ok : [webserver1] Install nginx (2.345s)
//!   changed : [webserver2] Install nginx (3.456s)
//!
//! TIMING SUMMARY *************************************************************
//!
//! Total tasks executed: 15
//! Total execution time: 45.678s
//! Average task time:    3.045s
//!
//! Slowest tasks (top 10):
//!
//!   Duration  Task                            Host                  Status
//!   --------  --------------------------------  --------------------  --------
//!    12.345s  Install packages                 webserver1            changed
//!     8.234s  Configure nginx                  webserver2            changed
//!     5.123s  Deploy application               webserver1            ok
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Configuration options for the timer callback plugin
#[derive(Debug, Clone)]
pub struct TimerConfig {
    /// Show timing after each task completes
    pub show_per_task: bool,
    /// Show summary of slowest tasks at end
    pub show_summary: bool,
    /// Number of slowest tasks to show in summary
    pub top_slowest: usize,
    /// Only show tasks that took longer than this threshold (seconds)
    pub threshold_secs: f64,
    /// Show play-level timing
    pub show_play_timing: bool,
    /// Show playbook-level timing
    pub show_playbook_timing: bool,
    /// Use colors in output
    pub use_colors: bool,
    /// Show timing in human-readable format
    pub human_readable: bool,
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            show_per_task: true,
            show_summary: true,
            top_slowest: 10,
            threshold_secs: 0.0,
            show_play_timing: true,
            show_playbook_timing: true,
            use_colors: true,
            human_readable: true,
        }
    }
}

/// Entry for tracking a single task's timing
#[derive(Debug, Clone)]
pub struct TimerTaskTiming {
    /// Task name
    pub task_name: String,
    /// Host the task ran on
    pub host: String,
    /// Duration of the task execution
    pub duration: Duration,
    /// Whether the task succeeded
    pub success: bool,
    /// Whether the task made changes
    pub changed: bool,
}

/// Entry for tracking play timing
#[derive(Debug, Clone)]
struct PlayTiming {
    name: String,
    start: Instant,
    end: Option<Instant>,
    hosts: Vec<String>,
}

/// Internal state for tracking timing across execution
#[derive(Debug, Default)]
struct TimerState {
    /// All task timings collected during execution
    task_timings: Vec<TimerTaskTiming>,
    /// Current task start times (task_name:host -> start_time)
    task_starts: HashMap<String, Instant>,
    /// Play timings
    play_timings: Vec<PlayTiming>,
    /// Current play index
    current_play: Option<usize>,
    /// Playbook start time
    playbook_start: Option<Instant>,
    /// Playbook name
    playbook_name: Option<String>,
}

/// Timer callback plugin that tracks and reports execution timing
///
/// This plugin implements the `ExecutionCallback` trait to receive
/// notifications about task execution and track timing information.
///
/// # Thread Safety
///
/// The timer uses `parking_lot::RwLock` for state management and
/// `AtomicU64` for counters, making it safe for concurrent access
/// during parallel task execution.
///
/// # Combining with Other Plugins
///
/// The timer callback is designed to work alongside other callback
/// plugins. It only outputs timing-specific information and doesn't
/// interfere with task result output from other plugins.
#[derive(Debug)]
pub struct TimerCallback {
    /// Configuration for the timer
    pub config: TimerConfig,
    /// Internal state protected by RwLock for thread-safe access
    state: RwLock<TimerState>,
    /// Total tasks executed (atomic for lock-free counting)
    total_tasks: AtomicU64,
    /// Total duration in microseconds (atomic for aggregation)
    total_duration_us: AtomicU64,
}

impl TimerCallback {
    /// Create a new timer callback with the given configuration
    pub fn new(config: TimerConfig) -> Self {
        Self {
            config,
            state: RwLock::new(TimerState::default()),
            total_tasks: AtomicU64::new(0),
            total_duration_us: AtomicU64::new(0),
        }
    }

    /// Create a timer callback with default configuration
    pub fn default_config() -> Self {
        Self::new(TimerConfig::default())
    }

    /// Create a minimal timer that only shows summary
    pub fn summary_only() -> Self {
        Self::new(TimerConfig {
            show_per_task: false,
            show_summary: true,
            top_slowest: 10,
            ..Default::default()
        })
    }

    /// Create a verbose timer that shows everything
    pub fn verbose() -> Self {
        Self::new(TimerConfig {
            show_per_task: true,
            show_summary: true,
            top_slowest: 20,
            threshold_secs: 0.0,
            show_play_timing: true,
            show_playbook_timing: true,
            use_colors: true,
            human_readable: true,
        })
    }

    /// Get all collected task timings
    pub fn get_timings(&self) -> Vec<TimerTaskTiming> {
        self.state.read().task_timings.clone()
    }

    /// Get the top N slowest tasks
    pub fn get_slowest_tasks(&self, n: usize) -> Vec<TimerTaskTiming> {
        let state = self.state.read();
        let mut timings = state.task_timings.clone();
        timings.sort_by(|a, b| b.duration.cmp(&a.duration));
        timings.into_iter().take(n).collect()
    }

    /// Get total execution time across all tasks
    pub fn get_total_duration(&self) -> Duration {
        Duration::from_micros(self.total_duration_us.load(Ordering::Relaxed))
    }

    /// Get total number of tasks executed
    pub fn get_total_tasks(&self) -> u64 {
        self.total_tasks.load(Ordering::Relaxed)
    }

    /// Get average task duration
    pub fn get_average_duration(&self) -> Duration {
        let total = self.get_total_duration();
        let count = self.get_total_tasks();
        if count == 0 {
            Duration::ZERO
        } else {
            total / count as u32
        }
    }

    /// Format a duration in human-readable format
    fn format_duration(&self, duration: Duration) -> String {
        if self.config.human_readable {
            format_duration_human(duration)
        } else {
            format!("{:.3}s", duration.as_secs_f64())
        }
    }

    /// Print task timing (called after task completion)
    fn print_task_timing(&self, timing: &TimerTaskTiming) {
        if !self.config.show_per_task {
            return;
        }

        // Skip tasks below threshold
        if timing.duration.as_secs_f64() < self.config.threshold_secs {
            return;
        }

        let duration_str = self.format_duration(timing.duration);

        let status = if !timing.success {
            if self.config.use_colors {
                "FAILED".red().bold().to_string()
            } else {
                "FAILED".to_string()
            }
        } else if timing.changed {
            if self.config.use_colors {
                "changed".yellow().to_string()
            } else {
                "changed".to_string()
            }
        } else if self.config.use_colors {
            "ok".green().to_string()
        } else {
            "ok".to_string()
        };

        let time_display = if self.config.use_colors {
            colorize_duration(timing.duration, &duration_str)
        } else {
            duration_str
        };

        println!(
            "  {} : [{}] {} ({})",
            status, timing.host, timing.task_name, time_display
        );
    }

    /// Print the timing summary at the end of execution
    fn print_summary(&self) {
        if !self.config.show_summary {
            return;
        }

        let state = self.state.read();

        if state.task_timings.is_empty() {
            return;
        }

        // Calculate statistics
        let total_duration = self.get_total_duration();
        let total_tasks = self.get_total_tasks();
        let avg_duration = self.get_average_duration();

        // Get slowest tasks
        let mut timings = state.task_timings.clone();
        timings.sort_by(|a, b| b.duration.cmp(&a.duration));
        let slowest: Vec<_> = timings.into_iter().take(self.config.top_slowest).collect();

        // Print summary header
        println!();
        if self.config.use_colors {
            println!(
                "{} {}",
                "TIMING SUMMARY".bright_white().bold(),
                "*".repeat(65).bright_black()
            );
        } else {
            println!("TIMING SUMMARY {}", "*".repeat(65));
        }
        println!();

        // Overall statistics
        println!(
            "Total tasks executed: {}",
            if self.config.use_colors {
                total_tasks.to_string().bright_white().bold().to_string()
            } else {
                total_tasks.to_string()
            }
        );
        println!(
            "Total execution time: {}",
            if self.config.use_colors {
                self.format_duration(total_duration)
                    .bright_cyan()
                    .to_string()
            } else {
                self.format_duration(total_duration)
            }
        );
        println!(
            "Average task time:    {}",
            self.format_duration(avg_duration)
        );
        println!();

        // Slowest tasks
        if !slowest.is_empty() {
            if self.config.use_colors {
                println!(
                    "{} (top {}):",
                    "Slowest tasks".yellow().bold(),
                    self.config.top_slowest
                );
            } else {
                println!("Slowest tasks (top {}):", self.config.top_slowest);
            }
            println!();

            // Table header
            println!(
                "  {:>8}  {:30}  {:20}  {:8}",
                "Duration", "Task", "Host", "Status"
            );
            if self.config.use_colors {
                println!("  {}", "-".repeat(72).bright_black());
            } else {
                println!("  {}", "-".repeat(72));
            }

            for timing in &slowest {
                let duration_str = self.format_duration(timing.duration);
                let status = if !timing.success {
                    if self.config.use_colors {
                        "failed".red().to_string()
                    } else {
                        "failed".to_string()
                    }
                } else if timing.changed {
                    if self.config.use_colors {
                        "changed".yellow().to_string()
                    } else {
                        "changed".to_string()
                    }
                } else if self.config.use_colors {
                    "ok".green().to_string()
                } else {
                    "ok".to_string()
                };

                let time_display = if self.config.use_colors {
                    colorize_duration(timing.duration, &duration_str)
                } else {
                    duration_str
                };

                // Truncate long names
                let task_name = truncate_string(&timing.task_name, 30);
                let host = truncate_string(&timing.host, 20);

                println!(
                    "  {:>8}  {:30}  {:20}  {:8}",
                    time_display, task_name, host, status
                );
            }
            println!();
        }

        // Playbook timing
        if self.config.show_playbook_timing {
            if let Some(start) = state.playbook_start {
                let playbook_duration = start.elapsed();
                let name = state.playbook_name.as_deref().unwrap_or("playbook");
                if self.config.use_colors {
                    println!(
                        "{} '{}' completed in {}",
                        "Playbook".bright_white().bold(),
                        name.bright_cyan(),
                        self.format_duration(playbook_duration).bright_green()
                    );
                } else {
                    println!(
                        "Playbook '{}' completed in {}",
                        name,
                        self.format_duration(playbook_duration)
                    );
                }
            }
        }

        // Play timings
        if self.config.show_play_timing && !state.play_timings.is_empty() {
            println!();
            if self.config.use_colors {
                println!("{}:", "Play timings".yellow().bold());
            } else {
                println!("Play timings:");
            }

            for play in &state.play_timings {
                if let Some(end) = play.end {
                    let duration = end.duration_since(play.start);
                    let host_count = play.hosts.len();
                    println!(
                        "  {} - {} ({} host{})",
                        play.name,
                        self.format_duration(duration),
                        host_count,
                        if host_count == 1 { "" } else { "s" }
                    );
                }
            }
        }
    }

    /// Record a task start time
    pub fn record_task_start(&self, task_name: &str, host: &str) {
        let key = format!("{}:{}", task_name, host);
        self.state.write().task_starts.insert(key, Instant::now());
    }

    /// Record a task completion and calculate duration
    pub fn record_task_complete(
        &self,
        task_name: &str,
        host: &str,
        success: bool,
        changed: bool,
        explicit_duration: Option<Duration>,
    ) {
        let key = format!("{}:{}", task_name, host);

        let duration = if let Some(d) = explicit_duration {
            d
        } else {
            let mut state = self.state.write();
            if let Some(start) = state.task_starts.remove(&key) {
                start.elapsed()
            } else {
                // If we don't have a start time, use zero duration
                Duration::ZERO
            }
        };

        let timing = TimerTaskTiming {
            task_name: task_name.to_string(),
            host: host.to_string(),
            duration,
            success,
            changed,
        };

        // Update atomic counters
        self.total_tasks.fetch_add(1, Ordering::Relaxed);
        self.total_duration_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);

        // Print timing if configured
        self.print_task_timing(&timing);

        // Store timing for summary
        self.state.write().task_timings.push(timing);
    }

    /// Reset all timing data (useful for reuse)
    pub fn reset(&self) {
        let mut state = self.state.write();
        state.task_timings.clear();
        state.task_starts.clear();
        state.play_timings.clear();
        state.current_play = None;
        state.playbook_start = None;
        state.playbook_name = None;
        self.total_tasks.store(0, Ordering::Relaxed);
        self.total_duration_us.store(0, Ordering::Relaxed);
    }
}

#[async_trait]
impl ExecutionCallback for TimerCallback {
    async fn on_playbook_start(&self, name: &str) {
        let mut state = self.state.write();
        state.playbook_start = Some(Instant::now());
        state.playbook_name = Some(name.to_string());
    }

    async fn on_playbook_end(&self, _name: &str, _success: bool) {
        // Print the summary at the end of the playbook
        self.print_summary();
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let mut state = self.state.write();
        let play_timing = PlayTiming {
            name: name.to_string(),
            start: Instant::now(),
            end: None,
            hosts: hosts.to_vec(),
        };
        state.play_timings.push(play_timing);
        state.current_play = Some(state.play_timings.len() - 1);
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {
        let mut state = self.state.write();
        if let Some(idx) = state.current_play {
            if let Some(play) = state.play_timings.get_mut(idx) {
                play.end = Some(Instant::now());
            }
        }
        state.current_play = None;
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        self.record_task_start(name, host);
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let success = result.result.success;
        let changed = result.result.changed;

        // Use the duration from ExecutionResult even if it is zero.
        let duration = Some(result.duration);

        self.record_task_complete(&result.task_name, &result.host, success, changed, duration);
    }

    async fn on_handler_triggered(&self, _name: &str) {
        // Handlers are tracked as tasks, so we just note when they're triggered
    }

    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // Could track fact gathering time if needed
    }
}

impl Clone for TimerCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: RwLock::new(TimerState::default()),
            total_tasks: AtomicU64::new(0),
            total_duration_us: AtomicU64::new(0),
        }
    }
}

impl Default for TimerCallback {
    fn default() -> Self {
        Self::default_config()
    }
}

// ============================================================================
// Builder Pattern for Configuration
// ============================================================================

/// Builder for creating TimerCallback with custom configuration
#[derive(Debug, Default)]
pub struct TimerCallbackBuilder {
    config: TimerConfig,
}

impl TimerCallbackBuilder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether to show timing after each task
    pub fn show_per_task(mut self, enabled: bool) -> Self {
        self.config.show_per_task = enabled;
        self
    }

    /// Set whether to show summary at end
    pub fn show_summary(mut self, enabled: bool) -> Self {
        self.config.show_summary = enabled;
        self
    }

    /// Set number of slowest tasks to show
    pub fn top_slowest(mut self, count: usize) -> Self {
        self.config.top_slowest = count;
        self
    }

    /// Set minimum threshold for showing task timing (seconds)
    pub fn threshold_secs(mut self, seconds: f64) -> Self {
        self.config.threshold_secs = seconds;
        self
    }

    /// Set whether to show play timing
    pub fn show_play_timing(mut self, enabled: bool) -> Self {
        self.config.show_play_timing = enabled;
        self
    }

    /// Set whether to show playbook timing
    pub fn show_playbook_timing(mut self, enabled: bool) -> Self {
        self.config.show_playbook_timing = enabled;
        self
    }

    /// Set whether to use colors
    pub fn use_colors(mut self, enabled: bool) -> Self {
        self.config.use_colors = enabled;
        self
    }

    /// Set whether to use human-readable format
    pub fn human_readable(mut self, enabled: bool) -> Self {
        self.config.human_readable = enabled;
        self
    }

    /// Build the TimerCallback
    pub fn build(self) -> TimerCallback {
        TimerCallback::new(self.config)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Format a duration in human-readable format
fn format_duration_human(duration: Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let micros = duration.subsec_micros() % 1000;

    if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let remaining_secs = secs % 60;
        format!("{}h {:02}m {:02}s", hours, mins, remaining_secs)
    } else if secs >= 60 {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        format!("{}m {:02}s", mins, remaining_secs)
    } else if secs > 0 {
        format!("{}.{:03}s", secs, millis)
    } else if millis > 0 {
        format!("{}.{:03}ms", millis, micros)
    } else {
        format!("{}us", duration.as_micros())
    }
}

/// Colorize duration based on how long it took
fn colorize_duration(duration: Duration, text: &str) -> String {
    let secs = duration.as_secs_f64();

    if secs >= 30.0 {
        text.red().bold().to_string()
    } else if secs >= 10.0 {
        text.red().to_string()
    } else if secs >= 5.0 {
        text.yellow().to_string()
    } else if secs >= 1.0 {
        text.bright_yellow().to_string()
    } else {
        text.green().to_string()
    }
}

/// Truncate a string to fit within a maximum width
fn truncate_string(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        ".".repeat(max_width)
    } else {
        format!("{}...", &s[..max_width - 3])
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;

    #[test]
    fn test_timer_callback_creation() {
        let timer = TimerCallback::default();
        assert!(timer.config.show_per_task);
        assert!(timer.config.show_summary);
        assert_eq!(timer.config.top_slowest, 10);
    }

    #[test]
    fn test_timer_callback_builder() {
        let timer = TimerCallbackBuilder::new()
            .show_per_task(false)
            .show_summary(true)
            .top_slowest(5)
            .threshold_secs(1.0)
            .use_colors(false)
            .build();

        assert!(!timer.config.show_per_task);
        assert!(timer.config.show_summary);
        assert_eq!(timer.config.top_slowest, 5);
        assert_eq!(timer.config.threshold_secs, 1.0);
        assert!(!timer.config.use_colors);
    }

    #[test]
    fn test_format_duration_human() {
        assert_eq!(format_duration_human(Duration::from_micros(500)), "500us");
        assert_eq!(format_duration_human(Duration::from_millis(50)), "50.000ms");
        assert_eq!(format_duration_human(Duration::from_millis(1500)), "1.500s");
        assert_eq!(format_duration_human(Duration::from_secs(90)), "1m 30s");
        assert_eq!(
            format_duration_human(Duration::from_secs(3700)),
            "1h 01m 40s"
        );
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("this is a long string", 10), "this is...");
        assert_eq!(truncate_string("ab", 2), "ab");
        assert_eq!(truncate_string("abcd", 3), "...");
    }

    #[test]
    fn test_timer_timing_collection() {
        let timer = TimerCallback::default();

        // Simulate task execution
        timer.record_task_start("task1", "host1");
        std::thread::sleep(Duration::from_millis(10));
        timer.record_task_complete("task1", "host1", true, false, None);

        timer.record_task_start("task2", "host1");
        std::thread::sleep(Duration::from_millis(20));
        timer.record_task_complete("task2", "host1", true, true, None);

        let timings = timer.get_timings();
        assert_eq!(timings.len(), 2);
        assert_eq!(timer.get_total_tasks(), 2);
    }

    #[test]
    fn test_timer_slowest_tasks() {
        let timer = TimerCallback::default();

        // Add timings with explicit durations
        timer.record_task_complete(
            "fast",
            "host1",
            true,
            false,
            Some(Duration::from_millis(10)),
        );
        timer.record_task_complete(
            "slow",
            "host1",
            true,
            false,
            Some(Duration::from_millis(100)),
        );
        timer.record_task_complete(
            "medium",
            "host1",
            true,
            false,
            Some(Duration::from_millis(50)),
        );

        let slowest = timer.get_slowest_tasks(2);
        assert_eq!(slowest.len(), 2);
        assert_eq!(slowest[0].task_name, "slow");
        assert_eq!(slowest[1].task_name, "medium");
    }

    #[tokio::test]
    async fn test_timer_async_callbacks() {
        let timer = TimerCallback::default();

        timer.on_playbook_start("test-playbook").await;
        timer
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;
        timer.on_task_start("task1", "host1").await;

        // Create a mock execution result
        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "task1".to_string(),
            result: ModuleResult::ok("done"),
            duration: Duration::from_millis(50),
            notify: vec![],
        };

        timer.on_task_complete(&result).await;
        timer.on_play_end("test-play", true).await;
        timer.on_playbook_end("test-playbook", true).await;

        let timings = timer.get_timings();
        assert_eq!(timings.len(), 1);
        assert_eq!(timings[0].task_name, "task1");
        assert_eq!(timings[0].duration, Duration::from_millis(50));
    }

    #[test]
    fn test_timer_clone() {
        let timer = TimerCallback::default();
        timer.record_task_complete("task1", "host1", true, false, Some(Duration::from_secs(1)));

        // Clone should start fresh
        let cloned = timer.clone();
        assert_eq!(cloned.get_total_tasks(), 0);
        assert_eq!(cloned.get_timings().len(), 0);
    }

    #[test]
    fn test_timer_average_duration() {
        let timer = TimerCallback::default();

        timer.record_task_complete("t1", "h1", true, false, Some(Duration::from_secs(1)));
        timer.record_task_complete("t2", "h1", true, false, Some(Duration::from_secs(3)));

        let avg = timer.get_average_duration();
        assert_eq!(avg, Duration::from_secs(2));
    }

    #[test]
    fn test_timer_summary_only() {
        let timer = TimerCallback::summary_only();
        assert!(!timer.config.show_per_task);
        assert!(timer.config.show_summary);
    }

    #[test]
    fn test_timer_verbose() {
        let timer = TimerCallback::verbose();
        assert!(timer.config.show_per_task);
        assert!(timer.config.show_summary);
        assert_eq!(timer.config.top_slowest, 20);
    }

    #[test]
    fn test_timer_reset() {
        let timer = TimerCallback::default();
        timer.record_task_complete("task1", "host1", true, false, Some(Duration::from_secs(1)));

        assert_eq!(timer.get_total_tasks(), 1);

        timer.reset();

        assert_eq!(timer.get_total_tasks(), 0);
        assert_eq!(timer.get_timings().len(), 0);
    }
}
