//! Default Callback Plugin for Rustible
//!
//! This plugin produces Ansible-like colored terminal output including:
//! - Play headers with asterisk lines
//! - Task headers with asterisk lines
//! - Colored status per host (ok/changed/failed/skipped)
//! - Final recap with per-host statistics
//! - Support for verbosity levels (-v, -vv, -vvv, -vvvv)
//! - NO_COLOR environment variable support for CI environments
//!
//! # Features
//!
//! - **Ansible-compatible output**: Familiar format for Ansible users
//! - **Colored status indicators**: Green (ok), Yellow (changed), Red (failed), Cyan (skipped)
//! - **Verbosity levels**: Increasing detail with -v flags
//! - **CI-friendly**: Respects NO_COLOR environment variable
//! - **Diff support**: Optional diff output for file changes
//! - **Duration tracking**: Per-task and per-playbook timing
//!
//! # Example Output
//!
//! ```text
//! PLAY [webservers] **************************************************************
//!
//! TASK [Install nginx] ***********************************************************
//! changed: [web1]
//! changed: [web2]
//! ok: [web3]
//!
//! PLAY RECAP *********************************************************************
//! web1                       : ok=5    changed=2    unreachable=0    failed=0    skipped=1
//! web2                       : ok=5    changed=2    unreachable=0    failed=0    skipped=1
//! web3                       : ok=6    changed=1    unreachable=0    failed=0    skipped=0
//!
//! Playbook run took 12.34s
//! ```
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::DefaultCallback;
//!
//! // Create with default settings
//! let callback = DefaultCallback::new();
//!
//! // Create with specific verbosity and no-color mode
//! let callback = DefaultCallback::new()
//!     .with_verbosity(2)  // -vv
//!     .with_no_color(true);  // For CI
//!
//! // Using builder pattern
//! let callback = DefaultCallbackBuilder::new()
//!     .verbosity(3)
//!     .show_diff(true)
//!     .build();
//!
//! # let _ = ();
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::{Color, Colorize};
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Constants
// ============================================================================

/// The default width for output formatting (Ansible standard)
const OUTPUT_WIDTH: usize = 80;

// ============================================================================
// Verbosity Levels
// ============================================================================

/// Verbosity levels for output control
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    /// No verbose output (default)
    Normal = 0,
    /// -v: Show task results
    Verbose = 1,
    /// -vv: Show task input parameters
    MoreVerbose = 2,
    /// -vvv: Show connection debugging
    Debug = 3,
    /// -vvvv: Show full connection debugging
    ConnectionDebug = 4,
    /// -vvvvv: Internal debugging
    WinRMDebug = 5,
}

impl From<u8> for Verbosity {
    fn from(level: u8) -> Self {
        match level {
            0 => Verbosity::Normal,
            1 => Verbosity::Verbose,
            2 => Verbosity::MoreVerbose,
            3 => Verbosity::Debug,
            4 => Verbosity::ConnectionDebug,
            _ => Verbosity::WinRMDebug,
        }
    }
}

// ============================================================================
// Host Statistics
// ============================================================================

/// Per-host execution statistics
#[derive(Debug, Clone, Default)]
pub struct HostStats {
    /// Successfully completed tasks with no changes
    pub ok: u32,
    /// Tasks that made changes
    pub changed: u32,
    /// Tasks that failed
    pub failed: u32,
    /// Tasks that were skipped
    pub skipped: u32,
    /// Tasks where host was unreachable
    pub unreachable: u32,
    /// Tasks that were rescued from failure
    pub rescued: u32,
    /// Tasks whose failures were ignored
    pub ignored: u32,
}

impl HostStats {
    /// Create new empty statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there were any failures
    pub fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }

    /// Check if there were any changes
    pub fn has_changes(&self) -> bool {
        self.changed > 0
    }

    /// Get total task count
    pub fn total(&self) -> u32 {
        self.ok
            + self.changed
            + self.failed
            + self.skipped
            + self.unreachable
            + self.rescued
            + self.ignored
    }
}

// ============================================================================
// Default Callback Configuration
// ============================================================================

/// Configuration for the DefaultCallback plugin
#[derive(Debug, Clone)]
pub struct DefaultCallbackConfig {
    /// Verbosity level (0-5, corresponding to -v flags)
    pub verbosity: u8,
    /// Whether to disable colored output
    pub no_color: bool,
    /// Whether to show diffs for changed files
    pub show_diff: bool,
    /// Whether to show task duration
    pub show_duration: bool,
    /// Whether to show skipped tasks
    pub show_skipped: bool,
    /// Whether to show ok tasks (can be noisy for large playbooks)
    pub show_ok: bool,
}

impl Default for DefaultCallbackConfig {
    fn default() -> Self {
        Self {
            verbosity: 0,
            no_color: false,
            show_diff: false,
            show_duration: true,
            show_skipped: true,
            show_ok: true,
        }
    }
}

// ============================================================================
// Default Callback Implementation
// ============================================================================

/// Default callback plugin producing Ansible-like colored output.
///
/// This is the primary callback plugin for Rustible, designed to produce
/// output that is familiar to Ansible users while respecting terminal
/// capabilities and CI environment constraints.
///
/// # Thread Safety
///
/// The callback is thread-safe and can be used with concurrent task execution.
/// All mutable state is protected by `RwLock` or atomic types.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::DefaultCallback;
///
/// let callback = DefaultCallback::new();
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct DefaultCallback {
    /// Configuration
    pub config: DefaultCallbackConfig,
    /// Whether to use colored output (computed from config and environment)
    use_color: AtomicBool,
    /// Current verbosity level
    verbosity: AtomicU8,
    /// Playbook start time
    playbook_start: RwLock<Option<Instant>>,
    /// Current playbook name
    playbook_name: RwLock<Option<String>>,
    /// Current play name
    current_play: RwLock<Option<String>>,
    /// Current task name (for header deduplication)
    current_task: RwLock<Option<String>>,
    /// Task start times for duration tracking
    task_starts: RwLock<HashMap<String, Instant>>,
    /// Per-host statistics
    host_stats: RwLock<HashMap<String, HostStats>>,
    /// Whether we've already printed the task header (to avoid duplicates)
    task_header_printed: AtomicBool,
}

impl DefaultCallback {
    /// Create a new DefaultCallback with default settings.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = DefaultCallback::new();
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> Self {
        Self::with_config(DefaultCallbackConfig::default())
    }

    /// Create a new DefaultCallback with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration options
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let config = DefaultCallbackConfig {
    ///     verbosity: 2,
    ///     no_color: true,
    ///     ..Default::default()
    /// };
    /// let callback = DefaultCallback::with_config(config);
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_config(config: DefaultCallbackConfig) -> Self {
        // Respect NO_COLOR environment variable
        let use_color = !config.no_color && std::env::var("NO_COLOR").is_err();

        Self {
            verbosity: AtomicU8::new(config.verbosity),
            use_color: AtomicBool::new(use_color),
            config,
            playbook_start: RwLock::new(None),
            playbook_name: RwLock::new(None),
            current_play: RwLock::new(None),
            current_task: RwLock::new(None),
            task_starts: RwLock::new(HashMap::new()),
            host_stats: RwLock::new(HashMap::new()),
            task_header_printed: AtomicBool::new(false),
        }
    }

    /// Set the verbosity level.
    ///
    /// # Arguments
    ///
    /// * `level` - Verbosity level (0-5)
    pub fn with_verbosity(self, level: u8) -> Self {
        self.verbosity.store(level, Ordering::Relaxed);
        self
    }

    /// Enable or disable colored output.
    ///
    /// # Arguments
    ///
    /// * `no_color` - If true, disable colors
    pub fn with_no_color(self, no_color: bool) -> Self {
        let use_color = !no_color && std::env::var("NO_COLOR").is_err();
        self.use_color.store(use_color, Ordering::Relaxed);
        self
    }

    /// Get the builder for this callback.
    pub fn builder() -> DefaultCallbackBuilder {
        DefaultCallbackBuilder::new()
    }

    // ========================================================================
    // Output Helpers
    // ========================================================================

    /// Check if colors are enabled
    fn use_color(&self) -> bool {
        self.use_color.load(Ordering::Relaxed)
    }

    /// Get current verbosity level
    fn verbosity(&self) -> Verbosity {
        Verbosity::from(self.verbosity.load(Ordering::Relaxed))
    }

    /// Print a header line with asterisks (Ansible style).
    ///
    /// Format: `PREFIX [name] ******...`
    fn print_header(&self, prefix: &str, name: &str) {
        let header = format!("{} [{}]", prefix, name);
        let padding = OUTPUT_WIDTH.saturating_sub(header.len() + 1);
        let stars = "*".repeat(padding);

        if self.use_color() {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("\n{} {}", header, stars);
        }
    }

    /// Print the PLAY RECAP header
    fn print_recap_header(&self) {
        let header = "PLAY RECAP";
        let padding = OUTPUT_WIDTH.saturating_sub(header.len() + 1);
        let stars = "*".repeat(padding);

        if self.use_color() {
            println!(
                "\n{} {}",
                header.bright_white().bold(),
                stars.bright_black()
            );
        } else {
            println!("\n{} {}", header, stars);
        }
    }

    /// Format a status string with color
    fn format_status(&self, result: &ModuleResult) -> String {
        let (text, color) = if result.skipped {
            ("skipping", Color::Cyan)
        } else if !result.success {
            ("fatal", Color::Red)
        } else if result.changed {
            ("changed", Color::Yellow)
        } else {
            ("ok", Color::Green)
        };

        if self.use_color() {
            text.color(color).to_string()
        } else {
            text.to_string()
        }
    }

    /// Format a host name with color based on result
    fn format_host(&self, host: &str, result: &ModuleResult) -> String {
        if self.use_color() {
            if !result.success {
                host.red().bold().to_string()
            } else {
                host.bright_white().bold().to_string()
            }
        } else {
            host.to_string()
        }
    }

    /// Format host name for recap based on overall stats
    fn format_recap_host(&self, host: &str, stats: &HostStats) -> String {
        if self.use_color() {
            if stats.has_failures() {
                host.red().bold().to_string()
            } else if stats.has_changes() {
                host.yellow().to_string()
            } else {
                host.green().to_string()
            }
        } else {
            host.to_string()
        }
    }

    /// Format a stat value for recap (dimmed if zero)
    fn format_stat(&self, label: &str, value: u32, color: Color) -> String {
        if self.use_color() {
            if value > 0 {
                format!(
                    "{}={}",
                    label.color(color),
                    value.to_string().color(color).bold()
                )
            } else {
                format!("{}={}", label, value).dimmed().to_string()
            }
        } else {
            format!("{}={}", label, value)
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

    /// Print verbose result details (controlled by verbosity level)
    fn print_verbose_result(&self, result: &ExecutionResult) {
        if self.verbosity() < Verbosity::Verbose {
            return;
        }

        // -v: Show message if present
        if !result.result.message.is_empty() {
            if self.use_color() {
                println!("    {}: {}", "msg".bright_black(), result.result.message);
            } else {
                println!("    msg: {}", result.result.message);
            }
        }

        // -vv: Show result data
        if self.verbosity() >= Verbosity::MoreVerbose {
            if let Some(ref data) = result.result.data {
                let json_str = serde_json::to_string_pretty(data).unwrap_or_default();
                for line in json_str.lines() {
                    if self.use_color() {
                        println!("    {}", line.bright_black());
                    } else {
                        println!("    {}", line);
                    }
                }
            }
        }

        // Show duration if configured
        if self.config.show_duration {
            if self.use_color() {
                println!(
                    "    {}: {}",
                    "duration".bright_black(),
                    Self::format_duration(result.duration).bright_black()
                );
            } else {
                println!("    duration: {}", Self::format_duration(result.duration));
            }
        }
    }

    /// Create a task key for tracking
    fn task_key(task_name: &str, host: &str) -> String {
        format!("{}:{}", task_name, host)
    }
}

impl Default for DefaultCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for DefaultCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            use_color: AtomicBool::new(self.use_color.load(Ordering::Relaxed)),
            verbosity: AtomicU8::new(self.verbosity.load(Ordering::Relaxed)),
            playbook_start: RwLock::new(*self.playbook_start.read()),
            playbook_name: RwLock::new(self.playbook_name.read().clone()),
            current_play: RwLock::new(self.current_play.read().clone()),
            current_task: RwLock::new(self.current_task.read().clone()),
            task_starts: RwLock::new(self.task_starts.read().clone()),
            host_stats: RwLock::new(self.host_stats.read().clone()),
            task_header_printed: AtomicBool::new(self.task_header_printed.load(Ordering::Relaxed)),
        }
    }
}

// ============================================================================
// ExecutionCallback Implementation
// ============================================================================

#[async_trait]
impl ExecutionCallback for DefaultCallback {
    async fn on_playbook_start(&self, name: &str) {
        // Reset state
        *self.playbook_start.write() = Some(Instant::now());
        *self.playbook_name.write() = Some(name.to_string());
        self.host_stats.write().clear();
        self.task_starts.write().clear();

        if self.verbosity() >= Verbosity::Verbose {
            if self.use_color() {
                println!(
                    "\n{} {}",
                    "PLAYBOOK:".bright_white().bold(),
                    name.bright_white()
                );
            } else {
                println!("\nPLAYBOOK: {}", name);
            }
        }
    }

    async fn on_playbook_end(&self, _name: &str, success: bool) {
        // Print recap
        self.print_recap_header();

        let stats = self.host_stats.read();

        // Sort hosts for consistent output
        let mut hosts: Vec<_> = stats.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_stats) = stats.get(host) {
                let host_colored = self.format_recap_host(host, host_stats);

                // Format each stat
                let ok = self.format_stat("ok", host_stats.ok, Color::Green);
                let changed = self.format_stat("changed", host_stats.changed, Color::Yellow);
                let unreachable =
                    self.format_stat("unreachable", host_stats.unreachable, Color::Red);
                let failed = self.format_stat("failed", host_stats.failed, Color::Red);
                let skipped = self.format_stat("skipped", host_stats.skipped, Color::Cyan);
                let rescued = self.format_stat("rescued", host_stats.rescued, Color::Magenta);
                let ignored = self.format_stat("ignored", host_stats.ignored, Color::Blue);

                println!(
                    "{:<30} : {}    {}    {}    {}    {}    {}    {}",
                    host_colored, ok, changed, unreachable, failed, skipped, rescued, ignored
                );
            }
        }

        // Print total duration
        if let Some(start) = *self.playbook_start.read() {
            let duration = start.elapsed();
            let duration_str = Self::format_duration(duration);

            let playbook_status = if success {
                if self.use_color() {
                    "completed".green().bold().to_string()
                } else {
                    "completed".to_string()
                }
            } else if self.use_color() {
                "failed".red().bold().to_string()
            } else {
                "failed".to_string()
            };

            println!();
            if self.use_color() {
                println!("Playbook {} in {}", playbook_status, duration_str.bright_white());
            } else {
                println!("Playbook {} in {}", playbook_status, duration_str);
            }
        }

        let _ = io::stdout().flush();
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        *self.current_play.write() = Some(name.to_string());

        // Initialize stats for all hosts
        {
            let mut stats = self.host_stats.write();
            for host in hosts {
                stats.entry(host.clone()).or_default();
            }
        }

        self.print_header("PLAY", name);
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {
        *self.current_play.write() = None;
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        let key = Self::task_key(name, host);
        self.task_starts.write().insert(key, Instant::now());

        // Only print the task header once (for the first host)
        let current = self.current_task.read().clone();
        if current.as_deref() != Some(name) {
            *self.current_task.write() = Some(name.to_string());
            self.task_header_printed.store(false, Ordering::Relaxed);
        }

        if !self.task_header_printed.swap(true, Ordering::Relaxed) {
            self.print_header("TASK", name);
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Update host stats
        {
            let mut stats = self.host_stats.write();
            let host_stats = stats.entry(result.host.clone()).or_default();

            if result.result.skipped {
                host_stats.skipped += 1;
            } else if !result.result.success {
                host_stats.failed += 1;
            } else if result.result.changed {
                host_stats.changed += 1;
            } else {
                host_stats.ok += 1;
            }
        }

        // Check if we should display this result
        if result.result.skipped && !self.config.show_skipped {
            return;
        }
        if result.result.success && !result.result.changed && !self.config.show_ok {
            return;
        }

        let status_str = self.format_status(&result.result);
        let host_str = self.format_host(&result.host, &result.result);

        // Format the result line
        if result.result.skipped || (result.result.success && !result.result.changed) {
            // ok/skipped: simple format
            // Check if this is a debug task - debug module always has data with a single key
            // (either "msg" for message mode or the variable name for var mode)
            let is_debug_task = result
                .result
                .data
                .as_ref()
                .and_then(|d| d.as_object())
                .is_some_and(|obj| obj.len() == 1);

            if is_debug_task && !result.result.message.is_empty() {
                // Debug module: always show the message
                print!("{}: [{}]", status_str, host_str);
                if self.use_color() {
                    println!(" => {}", result.result.message.bright_black());
                } else {
                    println!(" => {}", result.result.message);
                }
            } else {
                println!("{}: [{}]", status_str, host_str);
            }
        } else if result.result.changed {
            // changed: show message if brief
            print!("{}: [{}]", status_str, host_str);
            if self.verbosity() >= Verbosity::Verbose && !result.result.message.is_empty() {
                if self.use_color() {
                    print!(" => {}", result.result.message.bright_black());
                } else {
                    print!(" => {}", result.result.message);
                }
            }
            println!();
        } else {
            // failed: always show message and details
            print!("{}: [{}]", status_str, host_str);

            if !result.result.message.is_empty() {
                if self.use_color() {
                    print!(" => {}", format!("{{{}}}", result.result.message).red());
                } else {
                    print!(" => {{{}}}", result.result.message);
                }
            }

            println!();

            // Show full result for failures at any verbosity
            if let Some(ref data) = result.result.data {
                let json_str = serde_json::to_string_pretty(data).unwrap_or_default();
                for line in json_str.lines() {
                    if self.use_color() {
                        println!("    {}", line.red());
                    } else {
                        println!("    {}", line);
                    }
                }
            }
        }

        // Show verbose output
        self.print_verbose_result(result);

        let _ = io::stdout().flush();
    }

    async fn on_handler_triggered(&self, name: &str) {
        if self.verbosity() >= Verbosity::Verbose {
            if self.use_color() {
                println!(
                    "{} {}",
                    "RUNNING HANDLER".bright_white().bold(),
                    format!("[{}]", name).bright_white()
                );
            } else {
                println!("RUNNING HANDLER [{}]", name);
            }
        }
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        if self.verbosity() >= Verbosity::Debug {
            if self.use_color() {
                println!("{}: [{}]", "ok".green(), host.bright_white().bold());
            } else {
                println!("ok: [{}]", host);
            }
        }
    }
}

// ============================================================================
// Builder Pattern
// ============================================================================

/// Builder for DefaultCallback with fluent configuration.
#[derive(Debug, Clone, Default)]
pub struct DefaultCallbackBuilder {
    config: DefaultCallbackConfig,
}

impl DefaultCallbackBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self {
            config: DefaultCallbackConfig::default(),
        }
    }

    /// Set the verbosity level (0-5).
    pub fn verbosity(mut self, level: u8) -> Self {
        self.config.verbosity = level;
        self
    }

    /// Disable colored output.
    pub fn no_color(mut self, no_color: bool) -> Self {
        self.config.no_color = no_color;
        self
    }

    /// Enable diff mode for showing file changes.
    pub fn show_diff(mut self, show_diff: bool) -> Self {
        self.config.show_diff = show_diff;
        self
    }

    /// Enable or disable duration display.
    pub fn show_duration(mut self, show: bool) -> Self {
        self.config.show_duration = show;
        self
    }

    /// Enable or disable showing skipped tasks.
    pub fn show_skipped(mut self, show: bool) -> Self {
        self.config.show_skipped = show;
        self
    }

    /// Enable or disable showing ok tasks.
    pub fn show_ok(mut self, show: bool) -> Self {
        self.config.show_ok = show;
        self
    }

    /// Build the DefaultCallback.
    pub fn build(self) -> DefaultCallback {
        DefaultCallback::with_config(self.config)
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_callback_creation() {
        let callback = DefaultCallback::new();
        assert_eq!(callback.verbosity(), Verbosity::Normal);
    }

    #[test]
    fn test_default_callback_with_verbosity() {
        let callback = DefaultCallback::new().with_verbosity(2);
        assert_eq!(callback.verbosity(), Verbosity::MoreVerbose);
    }

    #[test]
    fn test_default_callback_no_color() {
        let callback = DefaultCallback::new().with_no_color(true);
        assert!(!callback.use_color());
    }

    #[test]
    fn test_builder_pattern() {
        let callback = DefaultCallbackBuilder::new()
            .verbosity(2)
            .no_color(true)
            .show_diff(true)
            .show_duration(false)
            .build();

        assert_eq!(callback.verbosity(), Verbosity::MoreVerbose);
        assert!(!callback.use_color());
        assert!(callback.config.show_diff);
        assert!(!callback.config.show_duration);
    }

    #[test]
    fn test_verbosity_from_u8() {
        assert_eq!(Verbosity::from(0), Verbosity::Normal);
        assert_eq!(Verbosity::from(1), Verbosity::Verbose);
        assert_eq!(Verbosity::from(2), Verbosity::MoreVerbose);
        assert_eq!(Verbosity::from(3), Verbosity::Debug);
        assert_eq!(Verbosity::from(4), Verbosity::ConnectionDebug);
        assert_eq!(Verbosity::from(5), Verbosity::WinRMDebug);
        assert_eq!(Verbosity::from(10), Verbosity::WinRMDebug); // Clamps to max
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(
            DefaultCallback::format_duration(Duration::from_millis(500)),
            "500ms"
        );
        assert_eq!(
            DefaultCallback::format_duration(Duration::from_secs(5)),
            "5.00s"
        );
        assert_eq!(
            DefaultCallback::format_duration(Duration::from_secs(65)),
            "1m 5s"
        );
        assert_eq!(
            DefaultCallback::format_duration(Duration::from_secs(3665)),
            "1h 1m 5s"
        );
    }

    #[test]
    fn test_host_stats() {
        let mut stats = HostStats::new();

        assert_eq!(stats.total(), 0);
        assert!(!stats.has_failures());
        assert!(!stats.has_changes());

        stats.ok = 5;
        stats.changed = 2;
        stats.failed = 1;

        assert_eq!(stats.total(), 8);
        assert!(stats.has_failures());
        assert!(stats.has_changes());
    }

    #[test]
    fn test_task_key() {
        let key = DefaultCallback::task_key("Install nginx", "webserver1");
        assert_eq!(key, "Install nginx:webserver1");
    }

    #[test]
    fn test_clone() {
        let callback1 = DefaultCallback::new().with_verbosity(3);
        let callback2 = callback1.clone();

        assert_eq!(callback2.verbosity(), Verbosity::Debug);
    }

    #[test]
    fn test_default() {
        let callback = DefaultCallback::default();
        assert_eq!(callback.verbosity(), Verbosity::Normal);
        assert!(callback.playbook_name.read().is_none());
    }

    #[tokio::test]
    async fn test_callback_lifecycle() {
        let callback = DefaultCallback::builder()
            .no_color(true)
            .show_ok(false)
            .show_skipped(false)
            .build();

        // Start playbook
        callback.on_playbook_start("test_playbook").await;
        assert!(callback.playbook_start.read().is_some());
        assert_eq!(
            callback.playbook_name.read().as_ref().map(|s| s.as_str()),
            Some("test_playbook")
        );

        // Start play
        callback
            .on_play_start("test_play", &["host1".to_string(), "host2".to_string()])
            .await;
        assert_eq!(
            callback.current_play.read().as_ref().map(|s| s.as_str()),
            Some("test_play")
        );

        // Verify hosts are initialized
        let stats = callback.host_stats.read();
        assert!(stats.contains_key("host1"));
        assert!(stats.contains_key("host2"));
        drop(stats);

        // Start task
        callback.on_task_start("Install nginx", "host1").await;
        assert_eq!(
            callback.current_task.read().as_ref().map(|s| s.as_str()),
            Some("Install nginx")
        );

        // Complete task
        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "Install nginx".to_string(),
            result: ModuleResult::changed("Installed"),
            duration: Duration::from_millis(500),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;

        // Verify stats updated
        let stats = callback.host_stats.read();
        assert_eq!(stats.get("host1").map(|s| s.changed), Some(1));
        drop(stats);

        // End play
        callback.on_play_end("test_play", true).await;
        assert!(callback.current_play.read().is_none());

        // End playbook
        callback.on_playbook_end("test_playbook", true).await;
    }

    #[tokio::test]
    async fn test_host_stats_tracking() {
        let callback = DefaultCallback::builder()
            .no_color(true)
            .show_ok(false)
            .show_skipped(false)
            .build();

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
            result: ModuleResult::failed("Failed"),
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
        assert!(host_stats.has_failures());
        assert!(host_stats.has_changes());
    }
}
