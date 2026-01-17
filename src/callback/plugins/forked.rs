//! Forked callback plugin for Rustible.
//!
//! This plugin provides visual feedback for parallel task execution,
//! showing which hosts are running in parallel with individual progress bars.
//!
//! # Features
//!
//! - Visual fork lanes showing parallel execution
//! - Progress bars for each active host
//! - Completed/pending host counts
//! - Clear visual indication of parallelism level
//! - Works with the forks setting from executor config
//!
//! # Example Output
//!
//! ```text
//! TASK [Install nginx] ********************************************************
//! Parallel execution (forks=5):
//!
//!   [1] webserver1    [=========>          ] 45% | Installing...
//!   [2] webserver2    [============>       ] 60% | Configuring...
//!   [3] dbserver1     [===================>] 95% | Finalizing...
//!   [4] webserver3    [=====>              ] 25% | Downloading...
//!   [5] dbserver2     [=>                  ] 10% | Starting...
//!
//!   Completed: 3/10  |  Running: 5  |  Pending: 2
//! ```

use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// State of a host during parallel execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostState {
    /// Host is waiting to be processed
    Pending,
    /// Host is currently being processed in a fork slot
    Running {
        /// The fork slot index (0-based)
        fork_slot: usize,
    },
    /// Host completed successfully
    Completed,
    /// Host failed
    Failed,
    /// Host was skipped
    Skipped,
    /// Host was unreachable
    Unreachable,
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
    /// Current task name
    current_task: Option<String>,
    /// Task start time
    task_start: Option<Instant>,
}

/// Information about a fork slot.
#[derive(Debug)]
struct ForkSlot {
    /// Host currently occupying this slot (if any)
    host: Option<String>,
    /// Progress bar for this slot
    progress_bar: Option<ProgressBar>,
    /// Whether the slot is active
    active: bool,
}

impl Default for ForkSlot {
    fn default() -> Self {
        Self {
            host: None,
            progress_bar: None,
            active: false,
        }
    }
}

/// Configuration for the forked callback.
#[derive(Debug, Clone)]
pub struct ForkedConfig {
    /// Number of parallel fork slots
    pub forks: usize,
    /// Whether to use terminal UI (progress bars) or simple output
    pub use_terminal_ui: bool,
    /// Whether to show individual host progress bars
    pub show_host_progress: bool,
    /// Whether to show the summary line
    pub show_summary: bool,
    /// Progress bar template for fork slots
    pub slot_template: String,
}

impl Default for ForkedConfig {
    fn default() -> Self {
        Self {
            forks: 5,
            use_terminal_ui: std::io::stdout().is_terminal(),
            show_host_progress: true,
            show_summary: true,
            slot_template:
                "  {spinner:.green} [{prefix}] {msg:<15} [{bar:25.cyan/blue}] {percent:>3}%"
                    .to_string(),
        }
    }
}

/// Forked callback plugin that visualizes parallel execution.
///
/// This callback shows a live view of parallel task execution across
/// multiple hosts, with progress bars for each active fork slot.
///
/// # Design Principles
///
/// 1. **Visual Parallelism**: Clear indication of which hosts run simultaneously
/// 2. **Progress Tracking**: Individual progress bars per fork slot
/// 3. **Status Summary**: Running counts of completed/pending/running hosts
/// 4. **Minimal Flicker**: Smart terminal updates to reduce visual noise
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::ForkedCallback;
///
/// let callback = ForkedCallback::new(5); // 5 parallel forks
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ForkedCallback {
    /// Configuration
    config: ForkedConfig,
    /// Per-host execution statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Host states during execution
    host_states: Arc<RwLock<HashMap<String, HostState>>>,
    /// Fork slots for parallel visualization
    fork_slots: Arc<RwLock<Vec<ForkSlot>>>,
    /// Multi-progress container for progress bars
    multi_progress: Arc<RwLock<Option<MultiProgress>>>,
    /// Playbook start time for duration tracking
    start_time: Arc<RwLock<Option<Instant>>>,
    /// Current playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Current task name
    current_task: Arc<RwLock<Option<String>>>,
    /// Total hosts in current play
    total_hosts: Arc<AtomicU64>,
    /// Completed hosts count
    completed_hosts: Arc<AtomicU64>,
    /// Whether any failures occurred
    has_failures: Arc<RwLock<bool>>,
}

impl ForkedCallback {
    /// Creates a new forked callback plugin with the specified fork count.
    ///
    /// # Arguments
    ///
    /// * `forks` - Maximum number of parallel host executions
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = ForkedCallback::new(5);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(forks: usize) -> Self {
        let config = ForkedConfig {
            forks: forks.max(1),
            ..Default::default()
        };
        Self::with_config(config)
    }

    /// Creates a forked callback with custom configuration.
    #[must_use]
    pub fn with_config(config: ForkedConfig) -> Self {
        let forks = config.forks.max(1);
        Self {
            config: ForkedConfig { forks, ..config },
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            host_states: Arc::new(RwLock::new(HashMap::new())),
            fork_slots: Arc::new(RwLock::new(Vec::with_capacity(forks))),
            multi_progress: Arc::new(RwLock::new(None)),
            start_time: Arc::new(RwLock::new(None)),
            playbook_name: Arc::new(RwLock::new(None)),
            current_task: Arc::new(RwLock::new(None)),
            total_hosts: Arc::new(AtomicU64::new(0)),
            completed_hosts: Arc::new(AtomicU64::new(0)),
            has_failures: Arc::new(RwLock::new(false)),
        }
    }

    /// Returns whether any failures occurred during execution.
    pub fn has_failures(&self) -> bool {
        *self.has_failures.read()
    }

    /// Gets the number of configured forks.
    pub fn forks(&self) -> usize {
        self.config.forks
    }

    /// Allocates a fork slot for a host.
    fn allocate_fork_slot(&self, host: &str) -> Option<usize> {
        let mut slots = self.fork_slots.write();

        // Find an empty slot
        for (idx, slot) in slots.iter_mut().enumerate() {
            if !slot.active {
                slot.host = Some(host.to_string());
                slot.active = true;
                return Some(idx);
            }
        }

        // All slots full
        None
    }

    /// Releases a fork slot.
    fn release_fork_slot(&self, host: &str) {
        let mut slots = self.fork_slots.write();

        for slot in slots.iter_mut() {
            if slot.host.as_deref() == Some(host) {
                if let Some(pb) = slot.progress_bar.take() {
                    pb.finish_and_clear();
                }
                slot.host = None;
                slot.active = false;
                break;
            }
        }
    }

    /// Creates a progress bar for a fork slot.
    fn create_progress_bar(&self, slot_idx: usize, host: &str) -> ProgressBar {
        let style = ProgressStyle::default_bar()
            .template(&format!(
                "  {{spinner:.green}} [{}] {{prefix:<15}} [{{bar:25.cyan/blue}}] {{msg}}",
                slot_idx + 1
            ))
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-")
            .tick_chars("*+x-");

        let pb = ProgressBar::new(100);
        pb.set_style(style);
        pb.set_prefix(host.to_string());
        pb.set_message("Starting...");
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    }

    /// Updates the progress bar for a host.
    fn update_host_progress(&self, host: &str, progress: u8, message: &str) {
        let slots = self.fork_slots.read();

        for slot in slots.iter() {
            if slot.host.as_deref() == Some(host) {
                if let Some(ref pb) = slot.progress_bar {
                    pb.set_position(u64::from(progress));
                    pb.set_message(message.to_string());
                }
                break;
            }
        }
    }

    /// Prints the task header with parallelism info.
    fn print_task_header(&self, task_name: &str) {
        let header = format!(
            "\n{} [{}] {}",
            "TASK".bold().cyan(),
            task_name.bright_white().bold(),
            "*".repeat(60_usize.saturating_sub(task_name.len().min(55)))
        );
        println!("{header}");
        println!(
            "{}",
            format!("Parallel execution (forks={}):", self.config.forks).bright_black()
        );
        println!();
    }

    /// Prints the summary line showing completed/running/pending counts.
    #[allow(dead_code)]
    fn print_summary_line(&self) {
        let total = self.total_hosts.load(Ordering::SeqCst);
        let completed = self.completed_hosts.load(Ordering::SeqCst);
        let states = self.host_states.read();

        let running = states
            .values()
            .filter(|s| matches!(s, HostState::Running { .. }))
            .count();
        let pending = states
            .values()
            .filter(|s| matches!(s, HostState::Pending))
            .count();

        println!(
            "\n  {} {}/{}  |  {} {}  |  {} {}",
            "Completed:".bright_black(),
            completed.to_string().green(),
            total,
            "Running:".bright_black(),
            running.to_string().yellow(),
            "Pending:".bright_black(),
            pending.to_string().cyan(),
        );
    }

    /// Formats a simple status line for non-terminal output.
    fn format_simple_status(host: &str, task_name: &str, status: &str) -> String {
        format!(
            "  {} | {} | {}",
            host.bright_white().bold(),
            task_name.yellow(),
            status
        )
    }

    /// Formats the final recap line for a host.
    fn format_recap_line(host: &str, stats: &HostStats, state: &HostState) -> String {
        let host_display = match state {
            HostState::Failed | HostState::Unreachable => host.red().bold(),
            HostState::Completed if stats.changed > 0 => host.yellow(),
            HostState::Completed => host.green(),
            HostState::Skipped => host.cyan(),
            _ => host.white(),
        };

        let state_indicator = match state {
            HostState::Failed => "[FAILED]".red().bold(),
            HostState::Unreachable => "[UNREACHABLE]".magenta().bold(),
            HostState::Skipped => "[SKIPPED]".cyan(),
            HostState::Completed => "[OK]".green(),
            _ => "[?]".white(),
        };

        format!(
            "{host_display}: {state_indicator} ok={} changed={} failed={} skipped={} unreachable={}",
            stats.ok.to_string().green(),
            stats.changed.to_string().yellow(),
            stats.failed.to_string().red(),
            stats.skipped.to_string().cyan(),
            stats.unreachable.to_string().magenta(),
        )
    }
}

impl Default for ForkedCallback {
    fn default() -> Self {
        Self::new(5)
    }
}

impl Clone for ForkedCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            host_stats: Arc::clone(&self.host_stats),
            host_states: Arc::clone(&self.host_states),
            fork_slots: Arc::clone(&self.fork_slots),
            multi_progress: Arc::clone(&self.multi_progress),
            start_time: Arc::clone(&self.start_time),
            playbook_name: Arc::clone(&self.playbook_name),
            current_task: Arc::clone(&self.current_task),
            total_hosts: Arc::clone(&self.total_hosts),
            completed_hosts: Arc::clone(&self.completed_hosts),
            has_failures: Arc::clone(&self.has_failures),
        }
    }
}

#[async_trait]
impl ExecutionCallback for ForkedCallback {
    /// Called when a playbook starts - initializes tracking state.
    async fn on_playbook_start(&self, name: &str) {
        *self.start_time.write() = Some(Instant::now());
        *self.playbook_name.write() = Some(name.to_string());

        // Clear stats from any previous run
        self.host_stats.write().clear();
        self.host_states.write().clear();
        *self.has_failures.write() = false;

        // Reset counters
        self.total_hosts.store(0, Ordering::SeqCst);
        self.completed_hosts.store(0, Ordering::SeqCst);

        // Initialize fork slots
        let mut slots = self.fork_slots.write();
        slots.clear();
        for _ in 0..self.config.forks {
            slots.push(ForkSlot::default());
        }

        // Print playbook header
        println!(
            "\n{} {}",
            "PLAYBOOK:".bold().magenta(),
            name.bright_white().bold()
        );
        println!(
            "{}",
            format!("Parallelism: {} forks", self.config.forks).bright_black()
        );
    }

    /// Called when a playbook ends - prints the final recap.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let stats = self.host_stats.read();
        let states = self.host_states.read();
        let start_time = *self.start_time.read();

        // Clear any remaining progress bars
        if let Some(mp) = self.multi_progress.write().take() {
            let _ = mp.clear();
        }

        // Print recap header
        println!("\n{}\n", "=".repeat(70).bright_black());
        println!("{}", "PLAY RECAP".bold().cyan());
        println!("{}", "-".repeat(70).bright_black());

        // Print recap for each host in sorted order
        let mut hosts: Vec<_> = stats.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_stats) = stats.get(host) {
                let state = states.get(host).unwrap_or(&HostState::Completed);
                println!("{}", Self::format_recap_line(host, host_stats, state));
            }
        }

        // Print duration and status
        if let Some(start) = start_time {
            let duration = start.elapsed();
            let status = if success {
                "completed successfully".green().bold()
            } else {
                "failed".red().bold()
            };

            println!(
                "\n{} {status} in {:.2}s",
                name.bright_white().bold(),
                duration.as_secs_f64()
            );
        }

        // Print parallelism summary
        let total = self.total_hosts.load(Ordering::SeqCst);
        let completed = self.completed_hosts.load(Ordering::SeqCst);
        println!(
            "{}",
            format!(
                "Parallelism: {} forks, {}/{} hosts processed",
                self.config.forks, completed, total
            )
            .bright_black()
        );
    }

    /// Called when a play starts - initializes host tracking.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Initialize stats and states for all hosts in this play
        let mut stats = self.host_stats.write();
        let mut states = self.host_states.write();

        for host in hosts {
            stats.entry(host.clone()).or_default();
            states.insert(host.clone(), HostState::Pending);
        }

        // Update total hosts count
        self.total_hosts.store(hosts.len() as u64, Ordering::SeqCst);
        self.completed_hosts.store(0, Ordering::SeqCst);

        println!(
            "\n{} [{}] {} hosts",
            "PLAY".bold().cyan(),
            name.bright_white().bold(),
            hosts.len()
        );
    }

    /// Called when a play ends.
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Release all fork slots
        let mut slots = self.fork_slots.write();
        for slot in slots.iter_mut() {
            if let Some(pb) = slot.progress_bar.take() {
                pb.finish_and_clear();
            }
            slot.host = None;
            slot.active = false;
        }
    }

    /// Called when a task starts on a host.
    async fn on_task_start(&self, name: &str, host: &str) {
        // Update current task
        {
            let mut current_task = self.current_task.write();
            if current_task.as_deref() != Some(name) {
                *current_task = Some(name.to_string());

                if self.config.use_terminal_ui {
                    self.print_task_header(name);
                } else {
                    println!(
                        "\n{} [{}]",
                        "TASK".bold().cyan(),
                        name.bright_white().bold()
                    );
                }
            }
        }

        // Update host state
        {
            let mut states = self.host_states.write();
            if let Some(slot) = self.allocate_fork_slot(host) {
                states.insert(host.to_string(), HostState::Running { fork_slot: slot });
            }
        }

        // Update host stats with current task
        {
            let mut stats = self.host_stats.write();
            if let Some(host_stat) = stats.get_mut(host) {
                host_stat.current_task = Some(name.to_string());
                host_stat.task_start = Some(Instant::now());
            }
        }

        // Setup progress bar if using terminal UI
        if self.config.use_terminal_ui && self.config.show_host_progress {
            let mut slots = self.fork_slots.write();
            for (idx, slot) in slots.iter_mut().enumerate() {
                if slot.host.as_deref() == Some(host) && slot.progress_bar.is_none() {
                    let mp_guard = self.multi_progress.read();
                    if let Some(ref mp) = *mp_guard {
                        let pb = self.create_progress_bar(idx, host);
                        slot.progress_bar = Some(mp.add(pb));
                    } else {
                        drop(mp_guard);
                        let mut mp_guard = self.multi_progress.write();
                        let mp = MultiProgress::new();
                        let pb = self.create_progress_bar(idx, host);
                        slot.progress_bar = Some(mp.add(pb));
                        *mp_guard = Some(mp);
                    }
                    break;
                }
            }
        } else {
            // Simple output mode
            println!("{}", Self::format_simple_status(host, name, "starting..."));
        }
    }

    /// Called when a task completes.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let host = &result.host;

        // Update host statistics
        {
            let mut stats = self.host_stats.write();
            let host_stats = stats.entry(host.clone()).or_default();

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

            host_stats.current_task = None;
            host_stats.task_start = None;
        }

        // Update host state
        {
            let mut states = self.host_states.write();
            let new_state = if result.result.skipped {
                HostState::Skipped
            } else if !result.result.success {
                HostState::Failed
            } else {
                HostState::Completed
            };

            // Only update if moving to a terminal state
            if let Some(current_state) = states.get(host) {
                if matches!(current_state, HostState::Running { .. }) {
                    states.insert(host.clone(), new_state);
                }
            }
        }

        // Update progress bar or print status
        if self.config.use_terminal_ui {
            let status_msg = if result.result.skipped {
                "skipped".cyan().to_string()
            } else if !result.result.success {
                format!("FAILED: {}", result.result.message)
                    .red()
                    .to_string()
            } else if result.result.changed {
                "changed".yellow().to_string()
            } else {
                "ok".green().to_string()
            };

            self.update_host_progress(host, 100, &status_msg);
        } else {
            // Simple output mode
            let status = if result.result.skipped {
                "skipped".cyan().to_string()
            } else if !result.result.success {
                format!("FAILED: {}", result.result.message)
                    .red()
                    .to_string()
            } else if result.result.changed {
                "changed".yellow().to_string()
            } else {
                "ok".green().to_string()
            };

            println!(
                "{}",
                Self::format_simple_status(host, &result.task_name, &status)
            );
        }

        // Release the fork slot
        self.release_fork_slot(host);

        // Increment completed count
        self.completed_hosts.fetch_add(1, Ordering::SeqCst);
    }

    /// Called when a handler is triggered.
    async fn on_handler_triggered(&self, name: &str) {
        println!("  {} {}", "HANDLER:".bright_magenta(), name.bright_white());
    }

    /// Called when facts are gathered.
    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        if self.config.use_terminal_ui {
            self.update_host_progress(host, 10, "Facts gathered");
        } else {
            println!(
                "{}",
                Self::format_simple_status(host, "gather_facts", &"ok".green().to_string())
            );
        }
    }
}

/// Trait extension for handling unreachable hosts in forked execution.
#[async_trait]
pub trait ForkedUnreachableCallback: ExecutionCallback {
    /// Called when a host becomes unreachable during parallel execution.
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str);
}

#[async_trait]
impl ForkedUnreachableCallback for ForkedCallback {
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str) {
        // Update host state
        self.host_states
            .write()
            .insert(host.to_string(), HostState::Unreachable);

        // Update host statistics
        {
            let mut stats = self.host_stats.write();
            let host_stats = stats.entry(host.to_string()).or_default();
            host_stats.unreachable += 1;
        }

        // Mark as having failures
        *self.has_failures.write() = true;

        // Release fork slot
        self.release_fork_slot(host);

        // Print unreachable message
        println!(
            "  {} {} | {} | {}",
            "UNREACHABLE".magenta().bold(),
            host.bright_white().bold(),
            task_name.yellow(),
            error.red()
        );
    }
}

/// Builder for creating `ForkedCallback` with custom options.
#[derive(Debug, Clone)]
pub struct ForkedCallbackBuilder {
    config: ForkedConfig,
}

impl ForkedCallbackBuilder {
    /// Creates a new builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: ForkedConfig::default(),
        }
    }

    /// Sets the number of parallel forks.
    #[must_use]
    pub fn forks(mut self, forks: usize) -> Self {
        self.config.forks = forks;
        self
    }

    /// Forces terminal UI on or off.
    #[must_use]
    pub fn terminal_ui(mut self, enabled: bool) -> Self {
        self.config.use_terminal_ui = enabled;
        self
    }

    /// Enables or disables host progress bars.
    #[must_use]
    pub fn show_host_progress(mut self, show: bool) -> Self {
        self.config.show_host_progress = show;
        self
    }

    /// Enables or disables the summary line.
    #[must_use]
    pub fn show_summary(mut self, show: bool) -> Self {
        self.config.show_summary = show;
        self
    }

    /// Builds the `ForkedCallback`.
    #[must_use]
    pub fn build(self) -> ForkedCallback {
        ForkedCallback::with_config(self.config)
    }
}

impl Default for ForkedCallbackBuilder {
    fn default() -> Self {
        Self::new()
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
    async fn test_forked_callback_creation() {
        let callback = ForkedCallback::new(5);
        assert_eq!(callback.forks(), 5);
    }

    #[tokio::test]
    async fn test_forked_callback_minimum_forks() {
        let callback = ForkedCallback::new(0);
        assert_eq!(callback.forks(), 1); // Should be at least 1
    }

    #[tokio::test]
    async fn test_forked_callback_builder() {
        let callback = ForkedCallbackBuilder::new()
            .forks(10)
            .terminal_ui(false)
            .build();

        assert_eq!(callback.forks(), 10);
        assert!(!callback.config.use_terminal_ui);
    }

    #[tokio::test]
    async fn test_forked_callback_tracks_stats() {
        let callback = ForkedCallbackBuilder::new()
            .forks(5)
            .terminal_ui(false)
            .build();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        // Simulate task execution
        callback.on_task_start("Install nginx", "host1").await;
        callback.on_task_start("Install nginx", "host2").await;

        let ok_result = create_execution_result("host1", "Install nginx", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        let changed_result =
            create_execution_result("host2", "Install nginx", true, true, false, "changed");
        callback.on_task_complete(&changed_result).await;

        // Verify stats
        let stats = callback.host_stats.read();

        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.ok, 1);
        assert_eq!(host1_stats.changed, 0);

        let host2_stats = stats.get("host2").unwrap();
        assert_eq!(host2_stats.ok, 0);
        assert_eq!(host2_stats.changed, 1);

        assert!(!callback.has_failures());
    }

    #[tokio::test]
    async fn test_forked_callback_tracks_failures() {
        let callback = ForkedCallbackBuilder::new()
            .forks(5)
            .terminal_ui(false)
            .build();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback.on_task_start("Install nginx", "host1").await;

        let failed_result = create_execution_result(
            "host1",
            "Install nginx",
            false,
            false,
            false,
            "Package not found",
        );
        callback.on_task_complete(&failed_result).await;

        let stats = callback.host_stats.read();
        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.failed, 1);

        assert!(callback.has_failures());
    }

    #[tokio::test]
    async fn test_forked_callback_unreachable() {
        let callback = ForkedCallbackBuilder::new()
            .forks(5)
            .terminal_ui(false)
            .build();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback
            .on_host_unreachable("host1", "gather_facts", "Connection refused")
            .await;

        let stats = callback.host_stats.read();
        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.unreachable, 1);

        let states = callback.host_states.read();
        assert!(matches!(states.get("host1"), Some(HostState::Unreachable)));

        assert!(callback.has_failures());
    }

    #[tokio::test]
    async fn test_fork_slot_allocation() {
        let callback = ForkedCallbackBuilder::new()
            .forks(2)
            .terminal_ui(false)
            .build();

        callback.on_playbook_start("test").await;

        // Allocate first slot
        let slot1 = callback.allocate_fork_slot("host1");
        assert_eq!(slot1, Some(0));

        // Allocate second slot
        let slot2 = callback.allocate_fork_slot("host2");
        assert_eq!(slot2, Some(1));

        // No more slots available
        let slot3 = callback.allocate_fork_slot("host3");
        assert_eq!(slot3, None);

        // Release a slot
        callback.release_fork_slot("host1");

        // Now we can allocate again
        let slot4 = callback.allocate_fork_slot("host3");
        assert_eq!(slot4, Some(0));
    }

    #[test]
    fn test_default_trait() {
        let callback = ForkedCallback::default();
        assert_eq!(callback.forks(), 5);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = ForkedCallback::new(5);
        let callback2 = callback1.clone();

        assert!(Arc::ptr_eq(&callback1.host_stats, &callback2.host_stats));
        assert!(Arc::ptr_eq(&callback1.host_states, &callback2.host_states));
        assert!(Arc::ptr_eq(&callback1.fork_slots, &callback2.fork_slots));
    }

    #[test]
    fn test_host_state_equality() {
        assert_eq!(HostState::Pending, HostState::Pending);
        assert_eq!(
            HostState::Running { fork_slot: 0 },
            HostState::Running { fork_slot: 0 }
        );
        assert_ne!(
            HostState::Running { fork_slot: 0 },
            HostState::Running { fork_slot: 1 }
        );
        assert_ne!(HostState::Pending, HostState::Completed);
    }

    #[test]
    fn test_forked_config_default() {
        let config = ForkedConfig::default();
        assert_eq!(config.forks, 5);
        assert!(config.show_host_progress);
        assert!(config.show_summary);
    }
}
