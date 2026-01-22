//! Progress Bar Callback Plugin for Rustible
//!
//! This callback plugin provides visual progress bars for playbook execution,
//! showing overall playbook progress and current play/task status.
//!
//! # Features
//!
//! - Overall playbook progress bar with task counter
//! - Current play progress bar with host tracking
//! - Current task spinner with status updates
//! - Graceful fallback for non-TTY environments
//! - Respects NO_COLOR environment variable
//! - Thread-safe for concurrent task execution
//!
//! # Example Output (TTY)
//!
//! ```text
//! Playbook [site.yml] ━━━━━━━━━━━━━━━━━━━━━━ 15/30 tasks (50%)
//! Play [webservers] ━━━━━━━━━━━━━━━━━━━━━━━━ 5/10 hosts (50%)
//! ⠋ Installing nginx on web1...
//! ```
//!
//! # Example Output (non-TTY/CI)
//!
//! ```text
//! [PROGRESS] Playbook: site.yml - Starting (30 tasks)
//! [PROGRESS] Play: webservers - 0/10 hosts
//! [PROGRESS] Task: Install nginx (web1)
//! [PROGRESS] Task: Install nginx - completed on web1 (ok)
//! ```

use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the progress bar callback
#[derive(Debug, Clone)]
pub struct ProgressConfig {
    /// Whether to use colored output (overridden by NO_COLOR env var)
    pub use_color: bool,
    /// Template for the playbook progress bar
    pub playbook_template: String,
    /// Template for the play progress bar
    pub play_template: String,
    /// Template for the task spinner
    pub task_template: String,
    /// Spinner tick interval in milliseconds
    pub spinner_tick_ms: u64,
    /// Whether to show task spinners for each active task
    pub show_task_spinners: bool,
    /// Whether to show elapsed time
    pub show_elapsed: bool,
    /// Whether to show ETA (estimated time remaining)
    pub show_eta: bool,
    /// Maximum number of concurrent task spinners to display
    pub max_task_spinners: usize,
}

impl Default for ProgressConfig {
    fn default() -> Self {
        Self {
            use_color: true,
            playbook_template: "{spinner:.green} {prefix:.bold.white} {bar:40.cyan/blue} {pos}/{len} tasks ({percent}%) [{elapsed_precise}]".to_string(),
            play_template: "{spinner:.yellow} {prefix:.bold.white} {bar:40.green/dim} {pos}/{len} hosts ({percent}%)".to_string(),
            task_template: "{spinner:.cyan} {wide_msg}".to_string(),
            spinner_tick_ms: 80,
            show_task_spinners: true,
            show_elapsed: true,
            show_eta: false,
            max_task_spinners: 5,
        }
    }
}

// ============================================================================
// Play and Task State Tracking
// ============================================================================

/// State for tracking play progress
#[derive(Debug)]
#[allow(dead_code)]
struct PlayState {
    /// Play name
    name: String,
    /// Total number of hosts in this play
    total_hosts: usize,
    /// Hosts that have completed all tasks
    completed_hosts: HashSet<String>,
    /// Progress bar for this play
    progress_bar: Option<ProgressBar>,
    /// Start time
    start_time: Instant,
}

/// State for tracking task execution
#[derive(Debug)]
#[allow(dead_code)]
struct TaskState {
    /// Task name
    name: String,
    /// Host this task is running on
    host: String,
    /// Spinner for this task
    spinner: Option<ProgressBar>,
    /// Start time
    start_time: Instant,
}

// ============================================================================
// Progress Callback Implementation
// ============================================================================

/// Progress bar callback plugin for visual execution feedback.
///
/// This callback provides real-time progress visualization using the indicatif
/// crate. It automatically detects TTY capabilities and falls back to simple
/// text output for CI/CD environments.
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::ProgressCallback;
///
/// // Create with default configuration
/// let callback = ProgressCallback::new();
///
/// // Create with custom configuration
/// let config = ProgressConfig {
///     show_task_spinners: false,
///     ..Default::default()
/// };
/// let callback = ProgressCallback::with_config(config);
///
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ProgressCallback {
    /// Configuration
    config: ProgressConfig,
    /// Whether we're running in a TTY
    is_tty: AtomicBool,
    /// Multi-progress container for managing multiple progress bars
    multi_progress: Arc<RwLock<Option<MultiProgress>>>,
    /// Current playbook name
    playbook_name: RwLock<Option<String>>,
    /// Playbook start time
    playbook_start: RwLock<Option<Instant>>,
    /// Total task count for the playbook (estimated)
    total_tasks: AtomicU64,
    /// Completed task count
    completed_tasks: AtomicU64,
    /// Playbook progress bar
    playbook_bar: RwLock<Option<ProgressBar>>,
    /// Current play state
    current_play: RwLock<Option<PlayState>>,
    /// Active task states (keyed by "task_name:host")
    active_tasks: RwLock<HashMap<String, TaskState>>,
    /// Task counter for unique task identification
    task_counter: AtomicU64,
    /// Whether any failures occurred
    has_failures: AtomicBool,
    /// Host statistics for recap
    host_stats: RwLock<HashMap<String, HostStats>>,
}

/// Per-host statistics
#[derive(Debug, Clone, Default)]
struct HostStats {
    ok: u32,
    changed: u32,
    failed: u32,
    skipped: u32,
    unreachable: u32,
}

impl ProgressCallback {
    /// Create a new progress callback with default configuration.
    pub fn new() -> Self {
        Self::with_config(ProgressConfig::default())
    }

    /// Create a new progress callback with custom configuration.
    pub fn with_config(config: ProgressConfig) -> Self {
        // Check if stdout is a TTY
        let is_tty = std::io::stdout().is_terminal();

        // Respect NO_COLOR environment variable
        let use_color = config.use_color && std::env::var("NO_COLOR").is_err();

        Self {
            config: ProgressConfig {
                use_color,
                ..config
            },
            is_tty: AtomicBool::new(is_tty),
            multi_progress: Arc::new(RwLock::new(None)),
            playbook_name: RwLock::new(None),
            playbook_start: RwLock::new(None),
            total_tasks: AtomicU64::new(0),
            completed_tasks: AtomicU64::new(0),
            playbook_bar: RwLock::new(None),
            current_play: RwLock::new(None),
            active_tasks: RwLock::new(HashMap::new()),
            task_counter: AtomicU64::new(0),
            has_failures: AtomicBool::new(false),
            host_stats: RwLock::new(HashMap::new()),
        }
    }

    /// Check if we're in TTY mode
    fn is_tty(&self) -> bool {
        self.is_tty.load(Ordering::Relaxed)
    }

    /// Get or create the multi-progress container
    fn get_multi_progress(&self) -> Option<MultiProgress> {
        if !self.is_tty() {
            return None;
        }

        let guard = self.multi_progress.read();
        if guard.is_some() {
            return guard.clone();
        }
        drop(guard);

        let mut guard = self.multi_progress.write();
        if guard.is_none() {
            *guard = Some(MultiProgress::new());
        }
        guard.clone()
    }

    /// Create the playbook progress bar style
    fn playbook_style(&self) -> ProgressStyle {
        ProgressStyle::default_bar()
            .template(&self.config.playbook_template)
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("━━─")
    }

    /// Create the play progress bar style
    fn play_style(&self) -> ProgressStyle {
        ProgressStyle::default_bar()
            .template(&self.config.play_template)
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("━━─")
    }

    /// Create the task spinner style
    fn task_style(&self) -> ProgressStyle {
        ProgressStyle::default_spinner()
            .template(&self.config.task_template)
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
    }

    /// Create a task key for the active_tasks map
    fn task_key(task_name: &str, host: &str) -> String {
        format!("{}:{}", task_name, host)
    }

    /// Print non-TTY progress message
    fn print_progress(&self, message: &str) {
        if self.config.use_color {
            println!("{} {}", "[PROGRESS]".bright_blue().bold(), message);
        } else {
            println!("[PROGRESS] {}", message);
        }
    }

    /// Print non-TTY status message with status indicator
    fn print_status(&self, status: &str, message: &str) {
        let status_colored = match status {
            "ok" => {
                if self.config.use_color {
                    "ok".green().to_string()
                } else {
                    "ok".to_string()
                }
            }
            "changed" => {
                if self.config.use_color {
                    "changed".yellow().to_string()
                } else {
                    "changed".to_string()
                }
            }
            "failed" => {
                if self.config.use_color {
                    "failed".red().bold().to_string()
                } else {
                    "failed".to_string()
                }
            }
            "skipped" => {
                if self.config.use_color {
                    "skipped".cyan().to_string()
                } else {
                    "skipped".to_string()
                }
            }
            _ => status.to_string(),
        };

        if self.config.use_color {
            println!(
                "{} {} - {}",
                "[PROGRESS]".bright_blue().bold(),
                message,
                status_colored
            );
        } else {
            println!("[PROGRESS] {} - {}", message, status_colored);
        }
    }

    /// Format duration for display
    fn format_duration(duration: Duration) -> String {
        let secs = duration.as_secs();
        let millis = duration.subsec_millis();

        if secs >= 3600 {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            let secs = secs % 60;
            format!("{}h {}m {}s", hours, mins, secs)
        } else if secs >= 60 {
            let mins = secs / 60;
            let secs = secs % 60;
            format!("{}m {}s", mins, secs)
        } else if secs > 0 {
            format!("{}.{:02}s", secs, millis / 10)
        } else {
            format!("{}ms", millis)
        }
    }

    /// Finish all progress bars and show completion message
    fn finish_all(&self, success: bool) {
        // Finish any active task spinners
        let mut active = self.active_tasks.write();
        for (_, state) in active.drain() {
            if let Some(spinner) = state.spinner {
                spinner.finish_and_clear();
            }
        }

        // Finish play progress bar
        if let Some(play_state) = self.current_play.write().take() {
            if let Some(bar) = play_state.progress_bar {
                bar.finish_and_clear();
            }
        }

        // Finish playbook progress bar
        if let Some(bar) = self.playbook_bar.write().take() {
            let completed = self.completed_tasks.load(Ordering::Relaxed);
            bar.set_position(completed);

            if success {
                bar.finish_with_message("completed");
            } else {
                bar.finish_with_message("failed");
            }
        }

        // Clear multi-progress
        *self.multi_progress.write() = None;
    }

    /// Print recap summary (non-TTY mode)
    fn print_recap(&self, playbook_name: &str, success: bool) {
        let stats = self.host_stats.read();
        let start = self.playbook_start.read();

        if !self.is_tty() {
            println!();
            if self.config.use_color {
                println!(
                    "{} {}",
                    "PLAY RECAP".bright_white().bold(),
                    "*".repeat(60).bright_black()
                );
            } else {
                println!("PLAY RECAP {}", "*".repeat(60));
            }

            // Sort hosts for consistent output
            let mut hosts: Vec<_> = stats.keys().collect();
            hosts.sort();

            for host in hosts {
                if let Some(host_stats) = stats.get(host) {
                    if self.config.use_color {
                        let host_colored = if host_stats.failed > 0 || host_stats.unreachable > 0 {
                            host.red().bold()
                        } else if host_stats.changed > 0 {
                            host.yellow()
                        } else {
                            host.green()
                        };

                        println!(
                            "{:<30} : ok={} changed={} failed={} skipped={} unreachable={}",
                            host_colored,
                            host_stats.ok.to_string().green(),
                            host_stats.changed.to_string().yellow(),
                            host_stats.failed.to_string().red(),
                            host_stats.skipped.to_string().cyan(),
                            host_stats.unreachable.to_string().magenta()
                        );
                    } else {
                        println!(
                            "{:<30} : ok={} changed={} failed={} skipped={} unreachable={}",
                            host,
                            host_stats.ok,
                            host_stats.changed,
                            host_stats.failed,
                            host_stats.skipped,
                            host_stats.unreachable
                        );
                    }
                }
            }
        }

        // Print duration
        if let Some(start_time) = *start {
            let duration = start_time.elapsed();
            let duration_str = Self::format_duration(duration);
            let playbook_status = if success {
                if self.config.use_color {
                    "completed".green().bold().to_string()
                } else {
                    "completed".to_string()
                }
            } else if self.config.use_color {
                "failed".red().bold().to_string()
            } else {
                "failed".to_string()
            };

            println!();
            if self.config.use_color {
                println!(
                    "Playbook {} {} in {}",
                    playbook_name.bright_white(),
                    playbook_status,
                    duration_str.bright_yellow()
                );
            } else {
                println!("Playbook {} {} in {}", playbook_name, playbook_status, duration_str);
            }
        }
    }
}

impl Default for ProgressCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ProgressCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            is_tty: AtomicBool::new(self.is_tty.load(Ordering::Relaxed)),
            multi_progress: Arc::new(RwLock::new(None)),
            playbook_name: RwLock::new(self.playbook_name.read().clone()),
            playbook_start: RwLock::new(*self.playbook_start.read()),
            total_tasks: AtomicU64::new(self.total_tasks.load(Ordering::Relaxed)),
            completed_tasks: AtomicU64::new(self.completed_tasks.load(Ordering::Relaxed)),
            playbook_bar: RwLock::new(None),
            current_play: RwLock::new(None),
            active_tasks: RwLock::new(HashMap::new()),
            task_counter: AtomicU64::new(self.task_counter.load(Ordering::Relaxed)),
            has_failures: AtomicBool::new(self.has_failures.load(Ordering::Relaxed)),
            host_stats: RwLock::new(self.host_stats.read().clone()),
        }
    }
}

#[async_trait]
impl ExecutionCallback for ProgressCallback {
    async fn on_playbook_start(&self, name: &str) {
        // Reset state
        *self.playbook_name.write() = Some(name.to_string());
        *self.playbook_start.write() = Some(Instant::now());
        self.total_tasks.store(0, Ordering::Relaxed);
        self.completed_tasks.store(0, Ordering::Relaxed);
        self.has_failures.store(false, Ordering::Relaxed);
        self.host_stats.write().clear();
        self.active_tasks.write().clear();
        self.task_counter.store(0, Ordering::Relaxed);

        if self.is_tty() {
            // Create multi-progress and playbook progress bar
            if let Some(mp) = self.get_multi_progress() {
                let bar = mp.add(ProgressBar::new(100)); // Will be updated when we know task count
                bar.set_style(self.playbook_style());
                bar.set_prefix(format!("Playbook [{}]", name));
                bar.enable_steady_tick(Duration::from_millis(self.config.spinner_tick_ms));
                *self.playbook_bar.write() = Some(bar);
            }
        } else {
            self.print_progress(&format!("Playbook: {} - Starting", name));
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        self.finish_all(success);
        self.print_recap(name, success);
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Initialize host stats for all hosts in this play
        {
            let mut stats = self.host_stats.write();
            for host in hosts {
                stats.entry(host.clone()).or_default();
            }
        }

        // Finish previous play progress bar if any
        if let Some(prev_play) = self.current_play.write().take() {
            if let Some(bar) = prev_play.progress_bar {
                bar.finish_and_clear();
            }
        }

        if self.is_tty() {
            // Create play progress bar
            let bar = if let Some(mp) = self.get_multi_progress() {
                let bar = mp.add(ProgressBar::new(hosts.len() as u64));
                bar.set_style(self.play_style());
                bar.set_prefix(format!("Play [{}]", name));
                bar.enable_steady_tick(Duration::from_millis(self.config.spinner_tick_ms));
                Some(bar)
            } else {
                None
            };

            *self.current_play.write() = Some(PlayState {
                name: name.to_string(),
                total_hosts: hosts.len(),
                completed_hosts: HashSet::new(),
                progress_bar: bar,
                start_time: Instant::now(),
            });
        } else {
            self.print_progress(&format!("Play: {} - 0/{} hosts", name, hosts.len()));

            *self.current_play.write() = Some(PlayState {
                name: name.to_string(),
                total_hosts: hosts.len(),
                completed_hosts: HashSet::new(),
                progress_bar: None,
                start_time: Instant::now(),
            });
        }
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        if let Some(play_state) = self.current_play.write().take() {
            if let Some(bar) = play_state.progress_bar {
                if success {
                    bar.finish_with_message("done");
                } else {
                    bar.finish_with_message("failed");
                }
            } else if !self.is_tty() {
                let duration = play_state.start_time.elapsed();
                let status = if success { "completed" } else { "failed" };
                self.print_progress(&format!(
                    "Play: {} - {} in {}",
                    name,
                    status,
                    Self::format_duration(duration)
                ));
            }
        }
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        let _task_id = self.task_counter.fetch_add(1, Ordering::Relaxed);
        let key = Self::task_key(name, host);

        if self.is_tty() && self.config.show_task_spinners {
            // Check if we're under the limit for task spinners
            let active_count = self.active_tasks.read().len();

            let spinner = if active_count < self.config.max_task_spinners {
                if let Some(mp) = self.get_multi_progress() {
                    let spinner = mp.add(ProgressBar::new_spinner());
                    spinner.set_style(self.task_style());
                    spinner.set_message(format!("{} on {}...", name, host));
                    spinner.enable_steady_tick(Duration::from_millis(self.config.spinner_tick_ms));
                    Some(spinner)
                } else {
                    None
                }
            } else {
                None
            };

            self.active_tasks.write().insert(
                key,
                TaskState {
                    name: name.to_string(),
                    host: host.to_string(),
                    spinner,
                    start_time: Instant::now(),
                },
            );
        } else if !self.is_tty() {
            self.print_progress(&format!("Task: {} ({})", name, host));

            self.active_tasks.write().insert(
                key,
                TaskState {
                    name: name.to_string(),
                    host: host.to_string(),
                    spinner: None,
                    start_time: Instant::now(),
                },
            );
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let key = Self::task_key(&result.task_name, &result.host);

        // Increment completed tasks counter
        let completed = self.completed_tasks.fetch_add(1, Ordering::Relaxed) + 1;

        // Update playbook progress bar
        if let Some(bar) = self.playbook_bar.read().as_ref() {
            let total = self.total_tasks.load(Ordering::Relaxed);
            if total > 0 {
                bar.set_length(total);
                bar.set_position(completed);
            } else {
                // If we don't know total, just show completed count
                bar.set_length(completed + 10); // Add buffer
                bar.set_position(completed);
            }
        }

        // Update host stats
        {
            let mut stats = self.host_stats.write();
            let host_stats = stats.entry(result.host.clone()).or_default();

            if result.result.skipped {
                host_stats.skipped += 1;
            } else if !result.result.success {
                host_stats.failed += 1;
                self.has_failures.store(true, Ordering::Relaxed);
            } else if result.result.changed {
                host_stats.changed += 1;
            } else {
                host_stats.ok += 1;
            }
        }

        // Remove and finish task spinner
        if let Some(task_state) = self.active_tasks.write().remove(&key) {
            if let Some(spinner) = task_state.spinner {
                let status = if result.result.skipped {
                    "skipped"
                } else if !result.result.success {
                    "failed"
                } else if result.result.changed {
                    "changed"
                } else {
                    "ok"
                };

                let msg = format!("{} on {} - {}", result.task_name, result.host, status);
                spinner.finish_with_message(msg);
            } else if !self.is_tty() {
                let status = if result.result.skipped {
                    "skipped"
                } else if !result.result.success {
                    "failed"
                } else if result.result.changed {
                    "changed"
                } else {
                    "ok"
                };

                self.print_status(
                    status,
                    &format!("Task: {} ({})", result.task_name, result.host),
                );
            }
        }

        // Update play progress bar - mark host as having completed this task
        // (Note: actual "host complete" tracking would need more context about
        // total tasks per host, but we can show progress based on task completions)
        if let Some(play_state) = self.current_play.write().as_mut() {
            if let Some(bar) = &play_state.progress_bar {
                // Count unique completed hosts for this update
                // This is a simplified approach - in practice you'd track all tasks per host
                let hosts_with_tasks = self.host_stats.read().len();
                bar.set_position(hosts_with_tasks as u64);
            }
        }
    }

    async fn on_handler_triggered(&self, name: &str) {
        if !self.is_tty() {
            self.print_progress(&format!("Handler: {} triggered", name));
        }
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        if !self.is_tty() {
            self.print_progress(&format!("Facts gathered for {}", host));
        }
    }
}

// ============================================================================
// Builder Pattern
// ============================================================================

/// Builder for ProgressCallback with fluent configuration.
#[derive(Debug, Clone)]
pub struct ProgressCallbackBuilder {
    config: ProgressConfig,
}

impl ProgressCallbackBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: ProgressConfig::default(),
        }
    }

    /// Enable or disable colored output.
    pub fn use_color(mut self, use_color: bool) -> Self {
        self.config.use_color = use_color;
        self
    }

    /// Set the playbook progress bar template.
    pub fn playbook_template(mut self, template: impl Into<String>) -> Self {
        self.config.playbook_template = template.into();
        self
    }

    /// Set the play progress bar template.
    pub fn play_template(mut self, template: impl Into<String>) -> Self {
        self.config.play_template = template.into();
        self
    }

    /// Set the task spinner template.
    pub fn task_template(mut self, template: impl Into<String>) -> Self {
        self.config.task_template = template.into();
        self
    }

    /// Set the spinner tick interval in milliseconds.
    pub fn spinner_tick_ms(mut self, ms: u64) -> Self {
        self.config.spinner_tick_ms = ms;
        self
    }

    /// Enable or disable task spinners.
    pub fn show_task_spinners(mut self, show: bool) -> Self {
        self.config.show_task_spinners = show;
        self
    }

    /// Enable or disable elapsed time display.
    pub fn show_elapsed(mut self, show: bool) -> Self {
        self.config.show_elapsed = show;
        self
    }

    /// Enable or disable ETA display.
    pub fn show_eta(mut self, show: bool) -> Self {
        self.config.show_eta = show;
        self
    }

    /// Set the maximum number of concurrent task spinners.
    pub fn max_task_spinners(mut self, max: usize) -> Self {
        self.config.max_task_spinners = max;
        self
    }

    /// Build the ProgressCallback.
    pub fn build(self) -> ProgressCallback {
        ProgressCallback::with_config(self.config)
    }
}

impl Default for ProgressCallbackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;

    #[test]
    fn test_progress_callback_creation() {
        let callback = ProgressCallback::new();
        assert!(callback.config.use_color || std::env::var("NO_COLOR").is_ok());
    }

    #[test]
    fn test_progress_config_default() {
        let config = ProgressConfig::default();
        assert!(config.use_color);
        assert!(config.show_task_spinners);
        assert!(config.show_elapsed);
        assert!(!config.show_eta);
        assert_eq!(config.max_task_spinners, 5);
        assert_eq!(config.spinner_tick_ms, 80);
    }

    #[test]
    fn test_builder_pattern() {
        let callback = ProgressCallbackBuilder::new()
            .use_color(false)
            .show_task_spinners(false)
            .max_task_spinners(10)
            .spinner_tick_ms(100)
            .build();

        assert!(!callback.config.use_color);
        assert!(!callback.config.show_task_spinners);
        assert_eq!(callback.config.max_task_spinners, 10);
        assert_eq!(callback.config.spinner_tick_ms, 100);
    }

    #[test]
    fn test_task_key_generation() {
        let key = ProgressCallback::task_key("Install nginx", "webserver1");
        assert_eq!(key, "Install nginx:webserver1");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(
            ProgressCallback::format_duration(Duration::from_millis(500)),
            "500ms"
        );
        assert_eq!(
            ProgressCallback::format_duration(Duration::from_secs(5)),
            "5.00s"
        );
        assert_eq!(
            ProgressCallback::format_duration(Duration::from_secs(65)),
            "1m 5s"
        );
        assert_eq!(
            ProgressCallback::format_duration(Duration::from_secs(3665)),
            "1h 1m 5s"
        );
    }

    #[tokio::test]
    async fn test_callback_lifecycle() {
        let callback = ProgressCallback::new();

        // Simulate playbook execution
        callback.on_playbook_start("test_playbook").await;

        assert_eq!(
            callback.playbook_name.read().as_ref().map(|s| s.as_str()),
            Some("test_playbook")
        );
        assert!(callback.playbook_start.read().is_some());

        callback
            .on_play_start("test_play", &["host1".to_string(), "host2".to_string()])
            .await;

        assert!(callback.current_play.read().is_some());

        callback.on_task_start("Install nginx", "host1").await;

        // Verify task is tracked
        let key = ProgressCallback::task_key("Install nginx", "host1");
        assert!(callback.active_tasks.read().contains_key(&key));

        // Complete task
        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "Install nginx".to_string(),
            result: ModuleResult::changed("Installed"),
            duration: Duration::from_millis(500),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;

        // Verify task is no longer active
        assert!(!callback.active_tasks.read().contains_key(&key));

        // Verify stats updated
        let stats = callback.host_stats.read();
        assert_eq!(stats.get("host1").map(|s| s.changed), Some(1));

        callback.on_play_end("test_play", true).await;
        callback.on_playbook_end("test_playbook", true).await;
    }

    #[tokio::test]
    async fn test_host_stats_tracking() {
        let callback = ProgressCallback::new();

        callback.on_playbook_start("test").await;
        callback
            .on_play_start("test_play", &["host1".to_string()])
            .await;

        // OK result
        let ok_result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "task1".to_string(),
            result: ModuleResult::ok("OK"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&ok_result).await;

        // Changed result
        let changed_result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "task2".to_string(),
            result: ModuleResult::changed("Changed"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&changed_result).await;

        // Failed result
        let failed_result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "task3".to_string(),
            result: ModuleResult {
                success: false,
                changed: false,
                message: "Failed".to_string(),
                skipped: false,
                data: None,
                warnings: vec![],
            },
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&failed_result).await;

        // Skipped result
        let skipped_result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "task4".to_string(),
            result: ModuleResult::skipped("Skipped"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&skipped_result).await;

        let stats = callback.host_stats.read();
        let host_stats = stats.get("host1").unwrap();

        assert_eq!(host_stats.ok, 1);
        assert_eq!(host_stats.changed, 1);
        assert_eq!(host_stats.failed, 1);
        assert_eq!(host_stats.skipped, 1);

        assert!(callback.has_failures.load(Ordering::Relaxed));
    }

    #[test]
    fn test_clone() {
        let callback1 = ProgressCallback::new();
        callback1.total_tasks.store(10, Ordering::Relaxed);
        callback1.completed_tasks.store(5, Ordering::Relaxed);

        let callback2 = callback1.clone();

        assert_eq!(callback2.total_tasks.load(Ordering::Relaxed), 10);
        assert_eq!(callback2.completed_tasks.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn test_default() {
        let callback = ProgressCallback::default();
        assert!(callback.playbook_name.read().is_none());
        assert_eq!(callback.completed_tasks.load(Ordering::Relaxed), 0);
    }
}
