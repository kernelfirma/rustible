//! Summary Callback Plugin for Rustible
//!
//! A silent-during-execution callback that only displays a final summary
//! at the end of playbook execution. Designed for batch jobs, cron tasks,
//! and scenarios where minimal output is required during runtime but a
//! comprehensive summary is needed at completion.
//!
//! # Features
//!
//! - **Silent Execution**: No output during task execution
//! - **Comprehensive Summary**: Displays aggregated stats at playbook end
//! - **Exit Code Info**: Includes success/failure status with exit code guidance
//! - **Timing Information**: Total execution duration and task counts
//! - **Log File Compatible**: Designed to work alongside log file callbacks
//! - **Host-Level Stats**: Per-host breakdown of ok/changed/failed/skipped/unreachable
//!
//! # Use Cases
//!
//! - Cron jobs where only final status matters
//! - Batch processing with log file output
//! - CI/CD pipelines with artifact-based logging
//! - Scheduled automation tasks
//! - Combining with `LogFileCallback` for detailed logs + summary
//!
//! # Example Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::{LogFileCallback, LogFileConfig, SummaryCallback, SummaryConfig};
//! use rustible::callback::manager::{CallbackManager, PluginPriority};
//!
//! // Basic usage with defaults
//! let callback = SummaryCallback::new();
//!
//! // Custom configuration
//! let callback = SummaryCallback::with_config(SummaryConfig {
//!     show_host_details: true,
//!     show_timing: true,
//!     show_exit_code_hint: true,
//!     compact_mode: false,
//!     use_colors: true,
//!     ..Default::default()
//! });
//!
//! // Use with executor
//! # let _ = ();
//!
//! // Combine with log file callback
//! let manager = CallbackManager::new();
//! let logfile = LogFileCallback::new(LogFileConfig::default())?;
//! manager
//!     .register("summary", Arc::new(SummaryCallback::new()), PluginPriority::NORMAL)
//!     .await;
//! manager
//!     .register("logfile", Arc::new(logfile), PluginPriority::LOGGING)
//!     .await;
//! # Ok(())
//! # }
//! ```
//!
//! # Example Output
//!
//! ```text
//! ================================================================================
//! PLAYBOOK SUMMARY: deploy-production.yml
//! ================================================================================
//!
//! Result: SUCCESS
//! Duration: 2m 35.421s
//! Total Tasks: 45
//! Total Hosts: 5
//!
//! Host Statistics:
//!   web01.example.com      ok=12  changed=3  failed=0  skipped=0  unreachable=0
//!   web02.example.com      ok=12  changed=3  failed=0  skipped=0  unreachable=0
//!   db01.example.com       ok=10  changed=2  failed=0  skipped=3  unreachable=0
//!   db02.example.com       ok=10  changed=2  failed=0  skipped=3  unreachable=0
//!   cache.example.com      ok=8   changed=1  failed=0  skipped=6  unreachable=0
//!
//! Totals: ok=52  changed=11  failed=0  skipped=12  unreachable=0
//!
//! Exit Code: 0 (success)
//! ================================================================================
//! ```
//!
//! # Exit Code Hints
//!
//! The callback provides exit code guidance:
//! - `0` - All tasks completed successfully
//! - `1` - One or more tasks failed
//! - `2` - One or more hosts were unreachable
//! - `3` - Both failures and unreachable hosts

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration options for the summary callback.
#[derive(Debug, Clone)]
pub struct SummaryConfig {
    /// Show per-host statistics in the summary
    pub show_host_details: bool,
    /// Show timing information (total duration)
    pub show_timing: bool,
    /// Show exit code hint at the end
    pub show_exit_code_hint: bool,
    /// Use compact single-line format instead of detailed
    pub compact_mode: bool,
    /// Use ANSI colors in output
    pub use_colors: bool,
    /// Minimum width for host name column
    pub host_column_width: usize,
    /// Show total aggregated stats
    pub show_totals: bool,
    /// Show task count
    pub show_task_count: bool,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            show_host_details: true,
            show_timing: true,
            show_exit_code_hint: true,
            compact_mode: false,
            use_colors: true,
            host_column_width: 25,
            show_totals: true,
            show_task_count: true,
        }
    }
}

impl SummaryConfig {
    /// Creates a minimal configuration for truly compact output.
    pub fn minimal() -> Self {
        Self {
            show_host_details: false,
            show_timing: true,
            show_exit_code_hint: true,
            compact_mode: true,
            use_colors: true,
            host_column_width: 20,
            show_totals: true,
            show_task_count: true,
        }
    }

    /// Creates a configuration for machine-parseable output (no colors).
    pub fn machine() -> Self {
        Self {
            show_host_details: true,
            show_timing: true,
            show_exit_code_hint: true,
            compact_mode: false,
            use_colors: false,
            host_column_width: 25,
            show_totals: true,
            show_task_count: true,
        }
    }
}

// ============================================================================
// Host Statistics
// ============================================================================

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
    /// Returns the total number of tasks executed for this host.
    #[allow(dead_code)]
    fn total_tasks(&self) -> u32 {
        self.ok + self.changed + self.failed + self.skipped
    }

    /// Returns true if any failures occurred.
    fn has_failures(&self) -> bool {
        self.failed > 0
    }

    /// Returns true if the host was unreachable.
    fn is_unreachable(&self) -> bool {
        self.unreachable > 0
    }
}

// ============================================================================
// Aggregated Statistics
// ============================================================================

/// Aggregated statistics across all hosts.
#[derive(Debug, Clone, Default)]
struct AggregateStats {
    /// Total ok count
    ok: u32,
    /// Total changed count
    changed: u32,
    /// Total failed count
    failed: u32,
    /// Total skipped count
    skipped: u32,
    /// Total unreachable count
    unreachable: u32,
    /// Total number of hosts
    host_count: usize,
}

impl AggregateStats {
    /// Compute aggregate stats from per-host stats.
    fn from_host_stats(stats: &HashMap<String, HostStats>) -> Self {
        let mut agg = Self {
            host_count: stats.len(),
            ..Default::default()
        };

        for host_stat in stats.values() {
            agg.ok += host_stat.ok;
            agg.changed += host_stat.changed;
            agg.failed += host_stat.failed;
            agg.skipped += host_stat.skipped;
            agg.unreachable += host_stat.unreachable;
        }

        agg
    }

    /// Returns the suggested exit code based on stats.
    fn suggested_exit_code(&self) -> i32 {
        match (self.failed > 0, self.unreachable > 0) {
            (true, true) => 3,   // Both failures and unreachable
            (false, true) => 2,  // Unreachable only
            (true, false) => 1,  // Failures only
            (false, false) => 0, // All success
        }
    }

    /// Returns the total task count.
    fn total_tasks(&self) -> u32 {
        self.ok + self.changed + self.failed + self.skipped
    }
}

// ============================================================================
// Internal State
// ============================================================================

/// Internal state for the summary callback.
#[derive(Debug, Default)]
struct SummaryState {
    /// Per-host statistics
    host_stats: HashMap<String, HostStats>,
    /// Playbook start time
    start_time: Option<Instant>,
    /// Current playbook name
    playbook_name: Option<String>,
    /// Playbook success status
    playbook_success: bool,
    /// Total number of plays
    play_count: u32,
    /// Current play name for context
    current_play: Option<String>,
}

impl SummaryState {
    /// Resets the state for a new playbook run.
    fn reset(&mut self) {
        self.host_stats.clear();
        self.start_time = None;
        self.playbook_name = None;
        self.playbook_success = true;
        self.play_count = 0;
        self.current_play = None;
    }
}

// ============================================================================
// Summary Callback
// ============================================================================

/// A callback that shows nothing during execution and only displays a summary at the end.
///
/// This callback is designed for batch jobs and cron tasks where:
/// - You don't need real-time feedback
/// - You want minimal output during execution
/// - You need a clear summary at completion
/// - You're combining with a log file callback
///
/// # Thread Safety
///
/// This callback is thread-safe and can be used with parallel task execution.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::SummaryCallback;
///
/// let callback = SummaryCallback::new();
///
/// // After playbook execution, the summary will be printed automatically
/// // No output occurs during task execution
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SummaryCallback {
    /// Configuration options
    config: SummaryConfig,
    /// Internal state protected by RwLock
    state: Arc<RwLock<SummaryState>>,
}

impl SummaryCallback {
    /// Creates a new summary callback with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: SummaryConfig::default(),
            state: Arc::new(RwLock::new(SummaryState::default())),
        }
    }

    /// Creates a summary callback with custom configuration.
    #[must_use]
    pub fn with_config(config: SummaryConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(SummaryState::default())),
        }
    }

    /// Creates a minimal summary callback (compact output).
    #[must_use]
    pub fn minimal() -> Self {
        Self::with_config(SummaryConfig::minimal())
    }

    /// Creates a machine-readable summary callback (no colors).
    #[must_use]
    pub fn machine() -> Self {
        Self::with_config(SummaryConfig::machine())
    }

    /// Returns whether any failures occurred during execution.
    pub fn has_failures(&self) -> bool {
        let state = self.state.read();
        state.host_stats.values().any(|s| s.has_failures())
    }

    /// Returns whether any hosts were unreachable.
    pub fn has_unreachable(&self) -> bool {
        let state = self.state.read();
        state.host_stats.values().any(|s| s.is_unreachable())
    }

    /// Returns the suggested exit code based on execution results.
    ///
    /// - `0`: All tasks succeeded
    /// - `1`: One or more tasks failed
    /// - `2`: One or more hosts were unreachable
    /// - `3`: Both failures and unreachable hosts
    pub fn suggested_exit_code(&self) -> i32 {
        let state = self.state.read();
        let agg = AggregateStats::from_host_stats(&state.host_stats);
        agg.suggested_exit_code()
    }

    /// Returns the total execution duration.
    pub fn duration(&self) -> Option<Duration> {
        let state = self.state.read();
        state.start_time.map(|start| start.elapsed())
    }

    /// Returns the aggregate statistics.
    pub fn aggregate_stats(&self) -> (u32, u32, u32, u32, u32) {
        let state = self.state.read();
        let agg = AggregateStats::from_host_stats(&state.host_stats);
        (
            agg.ok,
            agg.changed,
            agg.failed,
            agg.skipped,
            agg.unreachable,
        )
    }

    // ========================================================================
    // Formatting Helpers
    // ========================================================================

    /// Formats a duration in human-readable format.
    fn format_duration(duration: Duration) -> String {
        let secs = duration.as_secs();
        let millis = duration.subsec_millis();

        if secs >= 3600 {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            let remaining_secs = secs % 60;
            format!("{}h {:02}m {:02}s", hours, mins, remaining_secs)
        } else if secs >= 60 {
            let mins = secs / 60;
            let remaining_secs = secs % 60;
            format!("{}m {:02}.{:03}s", mins, remaining_secs, millis)
        } else {
            format!("{}.{:03}s", secs, millis)
        }
    }

    /// Prints the separator line.
    fn print_separator(&self) {
        if self.config.use_colors {
            println!("{}", "=".repeat(80).bright_black());
        } else {
            println!("{}", "=".repeat(80));
        }
    }

    /// Prints the summary header.
    fn print_header(&self, playbook_name: &str) {
        self.print_separator();

        if self.config.use_colors {
            println!(
                "{}: {}",
                "PLAYBOOK SUMMARY".bright_white().bold(),
                playbook_name.bright_cyan()
            );
        } else {
            println!("PLAYBOOK SUMMARY: {}", playbook_name);
        }

        self.print_separator();
        println!();
    }

    /// Prints the result status line.
    fn print_result(&self, success: bool) {
        let status = if success { "SUCCESS" } else { "FAILED" };

        if self.config.use_colors {
            let colored_status = if success {
                status.green().bold()
            } else {
                status.red().bold()
            };
            println!("{}: {}", "Result".bright_white(), colored_status);
        } else {
            println!("Result: {}", status);
        }
    }

    /// Prints timing information.
    fn print_timing(&self, duration: Duration) {
        if !self.config.show_timing {
            return;
        }

        let duration_str = Self::format_duration(duration);

        if self.config.use_colors {
            println!(
                "{}: {}",
                "Duration".bright_white(),
                duration_str.bright_cyan()
            );
        } else {
            println!("Duration: {}", duration_str);
        }
    }

    /// Prints task count information.
    fn print_task_count(&self, agg: &AggregateStats) {
        if !self.config.show_task_count {
            return;
        }

        if self.config.use_colors {
            println!(
                "{}: {}",
                "Total Tasks".bright_white(),
                agg.total_tasks().to_string().bright_cyan()
            );
            println!(
                "{}: {}",
                "Total Hosts".bright_white(),
                agg.host_count.to_string().bright_cyan()
            );
        } else {
            println!("Total Tasks: {}", agg.total_tasks());
            println!("Total Hosts: {}", agg.host_count);
        }
    }

    /// Prints per-host statistics.
    fn print_host_details(&self, stats: &HashMap<String, HostStats>) {
        if !self.config.show_host_details || stats.is_empty() {
            return;
        }

        println!();
        if self.config.use_colors {
            println!("{}:", "Host Statistics".yellow().bold());
        } else {
            println!("Host Statistics:");
        }

        // Calculate max host name width
        let max_width = stats
            .keys()
            .map(|h| h.len())
            .max()
            .unwrap_or(0)
            .max(self.config.host_column_width);

        // Sort hosts for consistent output
        let mut hosts: Vec<_> = stats.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_stats) = stats.get(host) {
                self.print_host_line(host, host_stats, max_width);
            }
        }
    }

    /// Prints a single host's statistics line.
    fn print_host_line(&self, host: &str, stats: &HostStats, width: usize) {
        if self.config.use_colors {
            // Color the host name based on status
            let host_colored = if stats.has_failures() || stats.is_unreachable() {
                format!("{:width$}", host, width = width).red()
            } else if stats.changed > 0 {
                format!("{:width$}", host, width = width).yellow()
            } else {
                format!("{:width$}", host, width = width).green()
            };

            println!(
                "  {}  ok={}  changed={}  failed={}  skipped={}  unreachable={}",
                host_colored,
                stats.ok.to_string().green(),
                stats.changed.to_string().yellow(),
                if stats.failed > 0 {
                    stats.failed.to_string().red().bold().to_string()
                } else {
                    stats.failed.to_string()
                },
                stats.skipped.to_string().cyan(),
                if stats.unreachable > 0 {
                    stats.unreachable.to_string().magenta().bold().to_string()
                } else {
                    stats.unreachable.to_string()
                },
            );
        } else {
            println!(
                "  {:width$}  ok={}  changed={}  failed={}  skipped={}  unreachable={}",
                host,
                stats.ok,
                stats.changed,
                stats.failed,
                stats.skipped,
                stats.unreachable,
                width = width
            );
        }
    }

    /// Prints the aggregate totals.
    fn print_totals(&self, agg: &AggregateStats) {
        if !self.config.show_totals {
            return;
        }

        println!();
        if self.config.use_colors {
            println!(
                "{}: ok={}  changed={}  failed={}  skipped={}  unreachable={}",
                "Totals".bright_white().bold(),
                agg.ok.to_string().green(),
                agg.changed.to_string().yellow(),
                if agg.failed > 0 {
                    agg.failed.to_string().red().bold().to_string()
                } else {
                    agg.failed.to_string()
                },
                agg.skipped.to_string().cyan(),
                if agg.unreachable > 0 {
                    agg.unreachable.to_string().magenta().bold().to_string()
                } else {
                    agg.unreachable.to_string()
                },
            );
        } else {
            println!(
                "Totals: ok={}  changed={}  failed={}  skipped={}  unreachable={}",
                agg.ok, agg.changed, agg.failed, agg.skipped, agg.unreachable
            );
        }
    }

    /// Prints the exit code hint.
    fn print_exit_code_hint(&self, agg: &AggregateStats) {
        if !self.config.show_exit_code_hint {
            return;
        }

        println!();

        let exit_code = agg.suggested_exit_code();
        let description = match exit_code {
            0 => "success",
            1 => "task failures",
            2 => "unreachable hosts",
            3 => "failures and unreachable",
            _ => "unknown",
        };

        if self.config.use_colors {
            let code_colored = if exit_code == 0 {
                exit_code.to_string().green().bold()
            } else {
                exit_code.to_string().red().bold()
            };

            println!(
                "{}: {} ({})",
                "Exit Code".bright_white(),
                code_colored,
                description
            );
        } else {
            println!("Exit Code: {} ({})", exit_code, description);
        }
    }

    /// Prints the compact summary format.
    fn print_compact_summary(
        &self,
        playbook_name: &str,
        success: bool,
        duration: Option<Duration>,
    ) {
        let state = self.state.read();
        let agg = AggregateStats::from_host_stats(&state.host_stats);

        let status = if success { "OK" } else { "FAILED" };
        let duration_str = duration.map(Self::format_duration).unwrap_or_default();
        let exit_code = agg.suggested_exit_code();

        if self.config.use_colors {
            let status_colored = if success {
                status.green().bold()
            } else {
                status.red().bold()
            };

            println!(
                "[{}] {} | {} | ok={} changed={} failed={} skipped={} unreachable={} | exit={}",
                status_colored,
                playbook_name.bright_cyan(),
                duration_str.bright_white(),
                agg.ok.to_string().green(),
                agg.changed.to_string().yellow(),
                if agg.failed > 0 {
                    agg.failed.to_string().red().bold().to_string()
                } else {
                    agg.failed.to_string()
                },
                agg.skipped.to_string().cyan(),
                if agg.unreachable > 0 {
                    agg.unreachable.to_string().magenta().bold().to_string()
                } else {
                    agg.unreachable.to_string()
                },
                if exit_code == 0 {
                    exit_code.to_string().green().to_string()
                } else {
                    exit_code.to_string().red().bold().to_string()
                }
            );
        } else {
            println!(
                "[{}] {} | {} | ok={} changed={} failed={} skipped={} unreachable={} | exit={}",
                status,
                playbook_name,
                duration_str,
                agg.ok,
                agg.changed,
                agg.failed,
                agg.skipped,
                agg.unreachable,
                exit_code
            );
        }
    }

    /// Prints the full detailed summary.
    fn print_detailed_summary(
        &self,
        playbook_name: &str,
        success: bool,
        duration: Option<Duration>,
    ) {
        let state = self.state.read();
        let agg = AggregateStats::from_host_stats(&state.host_stats);

        self.print_header(playbook_name);
        self.print_result(success);

        if let Some(dur) = duration {
            self.print_timing(dur);
        }

        self.print_task_count(&agg);
        self.print_host_details(&state.host_stats);
        self.print_totals(&agg);
        self.print_exit_code_hint(&agg);

        self.print_separator();
    }

    /// Prints the summary (called at playbook end).
    fn print_summary(&self) {
        let (playbook_name, success, duration) = {
            let state = self.state.read();
            (
                state
                    .playbook_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                state.playbook_success,
                state.start_time.map(|s| s.elapsed()),
            )
        };

        if self.config.compact_mode {
            self.print_compact_summary(&playbook_name, success, duration);
        } else {
            self.print_detailed_summary(&playbook_name, success, duration);
        }
    }
}

impl Default for SummaryCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SummaryCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

// ============================================================================
// ExecutionCallback Implementation
// ============================================================================

#[async_trait]
impl ExecutionCallback for SummaryCallback {
    /// Called when a playbook starts - records start time silently.
    async fn on_playbook_start(&self, name: &str) {
        let mut state = self.state.write();
        state.reset();
        state.start_time = Some(Instant::now());
        state.playbook_name = Some(name.to_string());
        state.playbook_success = true;
        // No output - silent during execution
    }

    /// Called when a playbook ends - prints the summary.
    async fn on_playbook_end(&self, _name: &str, success: bool) {
        {
            let mut state = self.state.write();
            state.playbook_success = success;
        }

        // This is the only place we produce output
        self.print_summary();
    }

    /// Called when a play starts - silently tracks play count.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let mut state = self.state.write();
        state.play_count += 1;
        state.current_play = Some(name.to_string());

        // Initialize host stats for all hosts in this play
        for host in hosts {
            state.host_stats.entry(host.clone()).or_default();
        }
        // No output - silent during execution
    }

    /// Called when a play ends - silent.
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // No output - silent during execution
    }

    /// Called when a task starts - silent.
    async fn on_task_start(&self, _name: &str, _host: &str) {
        // No output - silent during execution
    }

    /// Called when a task completes - silently updates statistics.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let mut state = self.state.write();
        let host_stats = state.host_stats.entry(result.host.clone()).or_default();

        // Update statistics based on result
        if result.result.skipped {
            host_stats.skipped += 1;
        } else if !result.result.success {
            host_stats.failed += 1;
            state.playbook_success = false;
        } else if result.result.changed {
            host_stats.changed += 1;
        } else {
            host_stats.ok += 1;
        }
        // No output - silent during execution
    }

    /// Called when a handler is triggered - silent.
    async fn on_handler_triggered(&self, _name: &str) {
        // No output - silent during execution
    }

    /// Called when facts are gathered - silent.
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // No output - silent during execution
    }
}

// ============================================================================
// Unreachable Host Extension
// ============================================================================

/// Trait extension for handling unreachable hosts in summary callback.
#[async_trait]
pub trait SummaryUnreachableCallback: ExecutionCallback {
    /// Called when a host becomes unreachable.
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str);
}

#[async_trait]
impl SummaryUnreachableCallback for SummaryCallback {
    async fn on_host_unreachable(&self, host: &str, _task_name: &str, _error: &str) {
        let mut state = self.state.write();
        let host_stats = state.host_stats.entry(host.to_string()).or_default();
        host_stats.unreachable += 1;
        state.playbook_success = false;
        // No output - silent during execution
    }
}

// ============================================================================
// Builder Pattern
// ============================================================================

/// Builder for creating `SummaryCallback` with custom configuration.
#[derive(Debug, Default)]
pub struct SummaryCallbackBuilder {
    config: SummaryConfig,
}

impl SummaryCallbackBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether to show per-host details.
    pub fn show_host_details(mut self, show: bool) -> Self {
        self.config.show_host_details = show;
        self
    }

    /// Set whether to show timing information.
    pub fn show_timing(mut self, show: bool) -> Self {
        self.config.show_timing = show;
        self
    }

    /// Set whether to show exit code hint.
    pub fn show_exit_code_hint(mut self, show: bool) -> Self {
        self.config.show_exit_code_hint = show;
        self
    }

    /// Set whether to use compact mode.
    pub fn compact_mode(mut self, compact: bool) -> Self {
        self.config.compact_mode = compact;
        self
    }

    /// Set whether to use colors.
    pub fn use_colors(mut self, use_colors: bool) -> Self {
        self.config.use_colors = use_colors;
        self
    }

    /// Set the host column width.
    pub fn host_column_width(mut self, width: usize) -> Self {
        self.config.host_column_width = width;
        self
    }

    /// Set whether to show totals.
    pub fn show_totals(mut self, show: bool) -> Self {
        self.config.show_totals = show;
        self
    }

    /// Set whether to show task count.
    pub fn show_task_count(mut self, show: bool) -> Self {
        self.config.show_task_count = show;
        self
    }

    /// Build the `SummaryCallback`.
    pub fn build(self) -> SummaryCallback {
        SummaryCallback::with_config(self.config)
    }
}

// ============================================================================
// Tests
// ============================================================================

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
    async fn test_summary_callback_tracks_stats() {
        let callback = SummaryCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        // Simulate task completions
        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        let changed_result =
            create_execution_result("host1", "task2", true, true, false, "changed");
        callback.on_task_complete(&changed_result).await;

        let failed_result =
            create_execution_result("host2", "task1", false, false, false, "failed");
        callback.on_task_complete(&failed_result).await;

        let skipped_result =
            create_execution_result("host2", "task2", true, false, true, "skipped");
        callback.on_task_complete(&skipped_result).await;

        // Verify stats
        let (ok, changed, failed, skipped, unreachable) = callback.aggregate_stats();
        assert_eq!(ok, 1);
        assert_eq!(changed, 1);
        assert_eq!(failed, 1);
        assert_eq!(skipped, 1);
        assert_eq!(unreachable, 0);

        assert!(callback.has_failures());
        assert!(!callback.has_unreachable());
        assert_eq!(callback.suggested_exit_code(), 1);
    }

    #[tokio::test]
    async fn test_summary_callback_no_failures() {
        let callback = SummaryCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        assert!(!callback.has_failures());
        assert_eq!(callback.suggested_exit_code(), 0);
    }

    #[tokio::test]
    async fn test_summary_callback_unreachable() {
        let callback = SummaryCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback
            .on_host_unreachable("host1", "gather_facts", "Connection refused")
            .await;

        assert!(callback.has_unreachable());
        assert_eq!(callback.suggested_exit_code(), 2);
    }

    #[tokio::test]
    async fn test_summary_callback_failures_and_unreachable() {
        let callback = SummaryCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        let failed_result =
            create_execution_result("host1", "task1", false, false, false, "failed");
        callback.on_task_complete(&failed_result).await;

        callback
            .on_host_unreachable("host2", "gather_facts", "Connection refused")
            .await;

        assert!(callback.has_failures());
        assert!(callback.has_unreachable());
        assert_eq!(callback.suggested_exit_code(), 3);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(
            SummaryCallback::format_duration(Duration::from_secs(30)),
            "30.000s"
        );
        assert_eq!(
            SummaryCallback::format_duration(Duration::from_secs(90)),
            "1m 30.000s"
        );
        assert_eq!(
            SummaryCallback::format_duration(Duration::from_secs(3700)),
            "1h 01m 40s"
        );
    }

    #[test]
    fn test_aggregate_stats_from_host_stats() {
        let mut stats = HashMap::new();
        stats.insert(
            "host1".to_string(),
            HostStats {
                ok: 5,
                changed: 2,
                failed: 1,
                skipped: 0,
                unreachable: 0,
            },
        );
        stats.insert(
            "host2".to_string(),
            HostStats {
                ok: 3,
                changed: 1,
                failed: 0,
                skipped: 2,
                unreachable: 1,
            },
        );

        let agg = AggregateStats::from_host_stats(&stats);

        assert_eq!(agg.ok, 8);
        assert_eq!(agg.changed, 3);
        assert_eq!(agg.failed, 1);
        assert_eq!(agg.skipped, 2);
        assert_eq!(agg.unreachable, 1);
        assert_eq!(agg.host_count, 2);
        assert_eq!(agg.suggested_exit_code(), 3); // Both failures and unreachable
    }

    #[test]
    fn test_builder() {
        let callback = SummaryCallbackBuilder::new()
            .show_host_details(false)
            .show_timing(false)
            .compact_mode(true)
            .use_colors(false)
            .build();

        assert!(!callback.config.show_host_details);
        assert!(!callback.config.show_timing);
        assert!(callback.config.compact_mode);
        assert!(!callback.config.use_colors);
    }

    #[test]
    fn test_config_presets() {
        let minimal = SummaryConfig::minimal();
        assert!(!minimal.show_host_details);
        assert!(minimal.compact_mode);

        let machine = SummaryConfig::machine();
        assert!(!machine.use_colors);
        assert!(machine.show_host_details);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = SummaryCallback::new();
        let callback2 = callback1.clone();

        // Both should share the same underlying state
        assert!(Arc::ptr_eq(&callback1.state, &callback2.state));
    }

    #[test]
    fn test_default_config() {
        let config = SummaryConfig::default();

        assert!(config.show_host_details);
        assert!(config.show_timing);
        assert!(config.show_exit_code_hint);
        assert!(!config.compact_mode);
        assert!(config.use_colors);
        assert!(config.show_totals);
        assert!(config.show_task_count);
    }

    #[tokio::test]
    async fn test_playbook_resets_state() {
        let callback = SummaryCallback::new();

        // First playbook
        callback.on_playbook_start("playbook1").await;
        callback
            .on_play_start("play1", &["host1".to_string()])
            .await;
        let result = create_execution_result("host1", "task1", true, true, false, "ok");
        callback.on_task_complete(&result).await;

        // Second playbook should reset state
        callback.on_playbook_start("playbook2").await;

        let (ok, changed, failed, skipped, unreachable) = callback.aggregate_stats();
        assert_eq!(ok, 0);
        assert_eq!(changed, 0);
        assert_eq!(failed, 0);
        assert_eq!(skipped, 0);
        assert_eq!(unreachable, 0);
    }
}
