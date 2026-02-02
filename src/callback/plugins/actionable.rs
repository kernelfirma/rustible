//! Actionable Callback Plugin for Rustible
//!
//! A callback plugin focused on actionable output - only showing tasks that
//! did something (changed or failed), hiding ok and skipped tasks entirely.
//! Perfect for answering "what changed?" questions and getting clear action
//! items for failures.
//!
//! # Features
//!
//! - **Actionable Focus**: Only shows tasks that made changes or failed
//! - **Clean Output**: Hides ok and skipped tasks for noise-free logs
//! - **Action Suggestions**: Provides remediation hints for common failures
//! - **Change Summary**: Quick overview of what changed across all hosts
//! - **Duration Tracking**: Optional timing for changed/failed tasks
//!
//! # Use Cases
//!
//! - Reviewing what changed after a playbook run
//! - Auditing and change tracking
//! - Quick identification of failures needing attention
//! - Environments where most tasks are idempotent (no-op)
//!
//! # Example Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::{ActionableCallback, ActionableConfig};
//!
//! // Basic usage with defaults
//! let callback = ActionableCallback::new();
//!
//! // Custom configuration
//! let callback = ActionableCallback::with_config(ActionableConfig {
//!     show_action_suggestions: true,
//!     show_duration: true,
//!     ..Default::default()
//! });
//!
//! # let _ = ();
//! # Ok(())
//! # }
//! ```
//!
//! # Example Output
//!
//! ```text
//! PLAY [Configure web servers] *************************************************
//!
//! CHANGED: webserver1 | Install nginx | Package installed successfully
//! CHANGED: webserver1 | Configure nginx | Configuration updated
//! FAILED: webserver2 | Install nginx | Package installation failed: apt-get returned 100
//!   ACTION REQUIRED: Check package sources and network connectivity
//! CHANGED: dbserver1 | Update PostgreSQL config | Configuration modified
//!
//! PLAY RECAP ********************************************************************
//! webserver1                 : ok=5    changed=2    failed=0    skipped=1
//! webserver2                 : ok=3    changed=0    failed=1    skipped=0
//! dbserver1                  : ok=8    changed=1    failed=0    skipped=2
//!
//! SUMMARY: 2 hosts changed, 1 host failed
//! ```
//!
//! # Exit Code Mapping
//!
//! - `0` - No failures, changes were made successfully
//! - `1` - One or more tasks failed
//! - `2` - One or more hosts were unreachable
//! - `3` - Both failures and unreachable hosts

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use colored::Colorize;
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration options for the actionable callback.
#[derive(Debug, Clone)]
pub struct ActionableConfig {
    /// Whether to show play headers
    pub show_play_headers: bool,
    /// Whether to provide action suggestions for failures
    pub show_action_suggestions: bool,
    /// Whether to show a summary of changes at the end
    pub show_change_summary: bool,
    /// Whether to show task duration for changed/failed tasks
    pub show_duration: bool,
    /// Whether to use ANSI colors in output
    pub use_colors: bool,
    /// Minimum width for host name column in recap
    pub host_column_width: usize,
    /// Whether to show the recap at the end
    pub show_recap: bool,
}

impl Default for ActionableConfig {
    fn default() -> Self {
        Self {
            show_play_headers: true,
            show_action_suggestions: true,
            show_change_summary: true,
            show_duration: false,
            use_colors: true,
            host_column_width: 30,
            show_recap: true,
        }
    }
}

impl ActionableConfig {
    /// Creates a minimal configuration for truly quiet output.
    ///
    /// Only shows changed and failed tasks with no frills.
    pub fn minimal() -> Self {
        Self {
            show_play_headers: false,
            show_action_suggestions: false,
            show_change_summary: false,
            show_duration: false,
            use_colors: true,
            host_column_width: 30,
            show_recap: false,
        }
    }

    /// Creates a verbose configuration with all features enabled.
    pub fn verbose() -> Self {
        Self {
            show_play_headers: true,
            show_action_suggestions: true,
            show_change_summary: true,
            show_duration: true,
            use_colors: true,
            host_column_width: 30,
            show_recap: true,
        }
    }

    /// Creates a CI-friendly configuration (no colors).
    pub fn ci() -> Self {
        Self {
            show_play_headers: true,
            show_action_suggestions: true,
            show_change_summary: true,
            show_duration: false,
            use_colors: false,
            host_column_width: 30,
            show_recap: true,
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
    /// Returns true if any actionable events occurred (changes or failures)
    #[allow(dead_code)]
    fn has_actions(&self) -> bool {
        self.changed > 0 || self.failed > 0 || self.unreachable > 0
    }

    /// Returns true if any failures occurred
    fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }
}

// ============================================================================
// Actionable Callback Implementation
// ============================================================================

/// Actionable callback plugin that shows only tasks that made changes or failed.
///
/// This callback is designed for operators who want to quickly understand
/// what changed during a playbook run, without wading through "ok" messages.
///
/// # Design Principles
///
/// 1. **Actionable Output**: Only show things that require attention or review
/// 2. **Change Visibility**: Changes are highlighted so you know what was modified
/// 3. **Clear Failures**: Failed tasks include suggestions for remediation
/// 4. **Quiet Success**: Ok and skipped tasks produce no output
///
/// # Usage
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::ActionableCallback;
///
/// let callback = ActionableCallback::new();
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ActionableCallback {
    /// Configuration
    config: ActionableConfig,
    /// Per-host execution statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Playbook start time for duration tracking
    start_time: RwLock<Option<Instant>>,
    /// Current playbook name
    playbook_name: RwLock<Option<String>>,
    /// Current play name
    current_play: RwLock<Option<String>>,
    /// Whether any actionable events occurred
    has_actions: RwLock<bool>,
    /// Total task count
    task_count: RwLock<u32>,
}

impl ActionableCallback {
    /// Creates a new actionable callback plugin with default configuration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = ActionableCallback::new();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(ActionableConfig::default())
    }

    /// Creates a new actionable callback plugin with custom configuration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let config = ActionableConfig {
    ///     show_duration: true,
    ///     ..Default::default()
    /// };
    /// let callback = ActionableCallback::with_config(config);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_config(config: ActionableConfig) -> Self {
        Self {
            config,
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            start_time: RwLock::new(None),
            playbook_name: RwLock::new(None),
            current_play: RwLock::new(None),
            has_actions: RwLock::new(false),
            task_count: RwLock::new(0),
        }
    }

    /// Returns whether any actionable events occurred during execution.
    ///
    /// Useful for determining if the playbook made any changes.
    pub fn has_actions(&self) -> bool {
        *self.has_actions.read()
    }

    /// Returns the count of hosts that had changes.
    pub fn hosts_with_changes(&self) -> u32 {
        self.host_stats
            .read()
            .values()
            .filter(|s| s.changed > 0)
            .count() as u32
    }

    /// Returns the count of hosts that had failures.
    pub fn hosts_with_failures(&self) -> u32 {
        self.host_stats
            .read()
            .values()
            .filter(|s| s.has_failures())
            .count() as u32
    }

    /// Get the suggested exit code based on execution results.
    pub fn exit_code(&self) -> i32 {
        let stats = self.host_stats.read();
        let has_failures = stats.values().any(|s| s.failed > 0);
        let has_unreachable = stats.values().any(|s| s.unreachable > 0);

        match (has_failures, has_unreachable) {
            (true, true) => 3,
            (false, true) => 2,
            (true, false) => 1,
            (false, false) => 0,
        }
    }

    /// Formats a changed task message.
    fn format_changed(
        &self,
        host: &str,
        task_name: &str,
        message: &str,
        duration_ms: Option<u128>,
    ) -> String {
        let duration_str = duration_ms
            .map(|d| format!(" ({:.2}s)", d as f64 / 1000.0))
            .unwrap_or_default();

        if self.config.use_colors {
            format!(
                "{}: {} | {} | {}{}",
                "CHANGED".yellow().bold(),
                host.bright_white().bold(),
                task_name.cyan(),
                message,
                duration_str.bright_black()
            )
        } else {
            format!(
                "CHANGED: {} | {} | {}{}",
                host, task_name, message, duration_str
            )
        }
    }

    /// Formats a failed task message.
    fn format_failed(&self, host: &str, task_name: &str, message: &str) -> String {
        if self.config.use_colors {
            format!(
                "{}: {} | {} | {}",
                "FAILED".red().bold(),
                host.bright_white().bold(),
                task_name.cyan(),
                message.red()
            )
        } else {
            format!("FAILED: {} | {} | {}", host, task_name, message)
        }
    }

    /// Formats an unreachable host message.
    fn format_unreachable(&self, host: &str, task_name: &str, message: &str) -> String {
        if self.config.use_colors {
            format!(
                "{}: {} | {} | {}",
                "UNREACHABLE".magenta().bold(),
                host.bright_white().bold(),
                task_name.cyan(),
                message
            )
        } else {
            format!("UNREACHABLE: {} | {} | {}", host, task_name, message)
        }
    }

    /// Generates action suggestions based on failure message.
    fn suggest_action(message: &str) -> Option<String> {
        let lower = message.to_lowercase();

        if lower.contains("permission denied") || lower.contains("access denied") {
            Some("Check file permissions and user privileges. Consider using become: true".into())
        } else if lower.contains("no such file") || lower.contains("file not found") {
            Some("Verify the file path exists. Check for typos in the path".into())
        } else if lower.contains("connection refused")
            || lower.contains("unreachable")
            || lower.contains("timed out")
        {
            Some("Check network connectivity and firewall rules. Verify the host is running".into())
        } else if lower.contains("package")
            && (lower.contains("not found") || lower.contains("failed"))
        {
            Some(
                "Check package sources and network connectivity. Verify package name is correct"
                    .into(),
            )
        } else if lower.contains("authentication") || lower.contains("credentials") {
            Some("Verify authentication credentials. Check SSH keys or passwords".into())
        } else if lower.contains("disk") && lower.contains("space") {
            Some("Free up disk space on the target host".into())
        } else if lower.contains("memory") || lower.contains("oom") {
            Some("Check available memory on the target host".into())
        } else if lower.contains("syntax error") || lower.contains("parse error") {
            Some("Review the task configuration for syntax errors".into())
        } else if lower.contains("service") && lower.contains("failed") {
            Some("Check service logs: journalctl -u <service> or /var/log".into())
        } else if lower.contains("command not found") || lower.contains("executable not found") {
            Some("Install the required command/package or check PATH".into())
        } else if lower.contains("timeout") {
            Some("Increase timeout value or check for slow operations".into())
        } else {
            None
        }
    }

    /// Formats a single host's recap line.
    fn format_recap_line(&self, host: &str, stats: &HostStats) -> String {
        let width = self.config.host_column_width;

        if self.config.use_colors {
            let host_color = if stats.failed > 0 || stats.unreachable > 0 {
                host.red().bold()
            } else if stats.changed > 0 {
                host.yellow()
            } else {
                host.green()
            };

            // Format each stat with appropriate color
            let ok_str = if stats.ok > 0 {
                format!("ok={}", stats.ok.to_string().green())
            } else {
                format!("ok={}", stats.ok)
            };

            let changed_str = if stats.changed > 0 {
                format!("changed={}", stats.changed.to_string().yellow().bold())
            } else {
                format!("changed={}", stats.changed)
            };

            let failed_str = if stats.failed > 0 {
                format!("failed={}", stats.failed.to_string().red().bold())
            } else {
                format!("failed={}", stats.failed)
            };

            let skipped_str = format!("skipped={}", stats.skipped);

            let unreachable_str = if stats.unreachable > 0 {
                format!(
                    "unreachable={}",
                    stats.unreachable.to_string().magenta().bold()
                )
            } else {
                format!("unreachable={}", stats.unreachable)
            };

            format!(
                "{:<width$} : {}    {}    {}    {}    {}",
                host_color, ok_str, changed_str, failed_str, skipped_str, unreachable_str
            )
        } else {
            format!(
                "{:<width$} : ok={}    changed={}    failed={}    skipped={}    unreachable={}",
                host, stats.ok, stats.changed, stats.failed, stats.skipped, stats.unreachable
            )
        }
    }

    /// Prints the action suggestion for a failure.
    fn print_action_suggestion(&self, message: &str) {
        if self.config.show_action_suggestions {
            if let Some(suggestion) = Self::suggest_action(message) {
                if self.config.use_colors {
                    println!(
                        "  {}: {}",
                        "ACTION REQUIRED".red().bold(),
                        suggestion.yellow()
                    );
                } else {
                    println!("  ACTION REQUIRED: {}", suggestion);
                }
            }
        }
    }
}

impl Default for ActionableCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ActionableCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            host_stats: Arc::clone(&self.host_stats),
            start_time: RwLock::new(*self.start_time.read()),
            playbook_name: RwLock::new(self.playbook_name.read().clone()),
            current_play: RwLock::new(self.current_play.read().clone()),
            has_actions: RwLock::new(*self.has_actions.read()),
            task_count: RwLock::new(*self.task_count.read()),
        }
    }
}

#[async_trait]
impl ExecutionCallback for ActionableCallback {
    /// Called when a playbook starts - initializes tracking state.
    async fn on_playbook_start(&self, name: &str) {
        *self.start_time.write() = Some(Instant::now());
        *self.playbook_name.write() = Some(name.to_string());

        // Clear stats from any previous run
        self.host_stats.write().clear();
        *self.has_actions.write() = false;
        *self.task_count.write() = 0;
    }

    /// Called when a playbook ends - prints the final recap and summary.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let stats = self.host_stats.read();
        let start_time = *self.start_time.read();

        // Print recap if configured
        if self.config.show_recap && !stats.is_empty() {
            let separator = "*".repeat(62);
            if self.config.use_colors {
                println!(
                    "\n{} {}",
                    "PLAY RECAP".bright_white().bold(),
                    separator.bright_black()
                );
            } else {
                println!("\nPLAY RECAP {}", separator);
            }

            // Print recap for each host in sorted order
            let mut hosts: Vec<_> = stats.keys().collect();
            hosts.sort();

            for host in hosts {
                if let Some(host_stats) = stats.get(host) {
                    println!("{}", self.format_recap_line(host, host_stats));
                }
            }
        }

        // Print change summary if configured
        if self.config.show_change_summary {
            let total_changed = stats.values().filter(|s| s.changed > 0).count();
            let total_failed = stats.values().filter(|s| s.has_failures()).count();

            println!();

            let summary = if total_failed > 0 {
                if self.config.use_colors {
                    format!(
                        "{}: {} host(s) changed, {} host(s) failed",
                        "SUMMARY".bright_white().bold(),
                        total_changed.to_string().yellow(),
                        total_failed.to_string().red().bold()
                    )
                } else {
                    format!(
                        "SUMMARY: {} host(s) changed, {} host(s) failed",
                        total_changed, total_failed
                    )
                }
            } else if total_changed > 0 {
                if self.config.use_colors {
                    format!(
                        "{}: {} host(s) changed, all successful",
                        "SUMMARY".bright_white().bold(),
                        total_changed.to_string().yellow()
                    )
                } else {
                    format!("SUMMARY: {} host(s) changed, all successful", total_changed)
                }
            } else if self.config.use_colors {
                format!(
                    "{}: No changes made (all tasks ok or skipped)",
                    "SUMMARY".bright_white().bold()
                )
            } else {
                "SUMMARY: No changes made (all tasks ok or skipped)".to_string()
            };

            println!("{}", summary);
        }

        // Print duration if we have start time
        if let Some(start) = start_time {
            let duration = start.elapsed();
            if self.config.use_colors {
                let playbook_status = if success {
                    "completed successfully".green()
                } else {
                    "failed".red().bold()
                };
                println!(
                    "\n{} {} in {:.2}s",
                    name.bright_white().bold(),
                    playbook_status,
                    duration.as_secs_f64()
                );
            } else {
                let playbook_status = if success {
                    "completed successfully"
                } else {
                    "failed"
                };
                println!(
                    "\n{} {} in {:.2}s",
                    name,
                    playbook_status,
                    duration.as_secs_f64()
                );
            }
        }
    }

    /// Called when a play starts - shows play header if configured.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Initialize stats for all hosts in this play
        let mut stats = self.host_stats.write();
        for host in hosts {
            stats.entry(host.clone()).or_default();
        }

        // Store current play name
        *self.current_play.write() = Some(name.to_string());

        // Show play header if configured
        if self.config.show_play_headers {
            let separator = "*".repeat(50);
            if self.config.use_colors {
                println!(
                    "\n{} [{}] {}",
                    "PLAY".bright_white().bold(),
                    name.cyan(),
                    separator.bright_black()
                );
            } else {
                println!("\nPLAY [{}] {}", name, separator);
            }
        }
    }

    /// Called when a play ends - silent in actionable mode.
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Silent - recap is shown at playbook end
    }

    /// Called when a task starts - silent in actionable mode.
    async fn on_task_start(&self, _name: &str, _host: &str) {
        // Silent - we only show output on change or failure
        *self.task_count.write() += 1;
    }

    /// Called when a task completes - only shows output on change or failure.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let mut stats = self.host_stats.write();
        let host_stats = stats.entry(result.host.clone()).or_default();

        // Update statistics based on result
        if result.result.skipped {
            host_stats.skipped += 1;
            // Silent on skip - this is the core behavior of actionable callback
        } else if !result.result.success {
            host_stats.failed += 1;

            // Mark that we have actions
            *self.has_actions.write() = true;

            // Print failure immediately
            let message = result.result.message.as_str();
            println!(
                "{}",
                self.format_failed(&result.host, &result.task_name, message)
            );

            // Show action suggestion
            self.print_action_suggestion(message);
        } else if result.result.changed {
            host_stats.changed += 1;

            // Mark that we have actions
            *self.has_actions.write() = true;

            // Print change immediately
            let duration_ms = if self.config.show_duration {
                Some(result.duration.as_millis())
            } else {
                None
            };

            println!(
                "{}",
                self.format_changed(
                    &result.host,
                    &result.task_name,
                    &result.result.message,
                    duration_ms
                )
            );
        } else {
            host_stats.ok += 1;
            // Silent on ok - this is the core behavior of actionable callback
        }
    }

    /// Called when a handler is triggered - silent in actionable mode.
    async fn on_handler_triggered(&self, _name: &str) {
        // Silent - handlers are internal details
    }

    /// Called when facts are gathered - silent in actionable mode.
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // Silent - fact gathering is internal
    }
}

// ============================================================================
// Unreachable Host Extension
// ============================================================================

/// Trait extension for handling unreachable hosts.
#[async_trait]
pub trait ActionableUnreachableCallback: ExecutionCallback {
    /// Called when a host becomes unreachable.
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str);
}

#[async_trait]
impl ActionableUnreachableCallback for ActionableCallback {
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str) {
        let mut stats = self.host_stats.write();
        let host_stats = stats.entry(host.to_string()).or_default();
        host_stats.unreachable += 1;

        // Mark that we have actions
        *self.has_actions.write() = true;

        // Print unreachable message immediately
        println!("{}", self.format_unreachable(host, task_name, error));

        // Show action suggestion
        if self.config.show_action_suggestions {
            let suggestion = "Check network connectivity, SSH configuration, and firewall rules";
            if self.config.use_colors {
                println!(
                    "  {}: {}",
                    "ACTION REQUIRED".magenta().bold(),
                    suggestion.yellow()
                );
            } else {
                println!("  ACTION REQUIRED: {}", suggestion);
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

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
    async fn test_actionable_callback_tracks_stats() {
        let callback = ActionableCallback::new();

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
            create_execution_result("host2", "task1", false, false, false, "error occurred");
        callback.on_task_complete(&failed_result).await;

        let skipped_result =
            create_execution_result("host2", "task2", true, false, true, "skipped");
        callback.on_task_complete(&skipped_result).await;

        // Verify stats
        let stats = callback.host_stats.read();

        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.ok, 1);
        assert_eq!(host1_stats.changed, 1);
        assert_eq!(host1_stats.failed, 0);
        assert_eq!(host1_stats.skipped, 0);

        let host2_stats = stats.get("host2").unwrap();
        assert_eq!(host2_stats.ok, 0);
        assert_eq!(host2_stats.changed, 0);
        assert_eq!(host2_stats.failed, 1);
        assert_eq!(host2_stats.skipped, 1);

        assert!(callback.has_actions());
    }

    #[tokio::test]
    async fn test_actionable_callback_no_actions_when_all_ok() {
        let callback = ActionableCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        assert!(!callback.has_actions());
    }

    #[tokio::test]
    async fn test_actionable_callback_has_actions_on_change() {
        let callback = ActionableCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let changed_result =
            create_execution_result("host1", "task1", true, true, false, "file modified");
        callback.on_task_complete(&changed_result).await;

        assert!(callback.has_actions());
    }

    #[tokio::test]
    async fn test_unreachable_callback() {
        let callback = ActionableCallback::new();

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

        assert!(callback.has_actions());
    }

    #[tokio::test]
    async fn test_hosts_with_changes() {
        let callback = ActionableCallback::new();

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

        // host1: changed
        let changed1 = create_execution_result("host1", "task1", true, true, false, "changed");
        callback.on_task_complete(&changed1).await;

        // host2: ok
        let ok = create_execution_result("host2", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok).await;

        // host3: changed
        let changed2 = create_execution_result("host3", "task1", true, true, false, "changed");
        callback.on_task_complete(&changed2).await;

        assert_eq!(callback.hosts_with_changes(), 2);
        assert_eq!(callback.hosts_with_failures(), 0);
    }

    #[tokio::test]
    async fn test_exit_code() {
        let callback = ActionableCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // No failures = exit code 0
        let ok = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok).await;
        assert_eq!(callback.exit_code(), 0);

        // Failed = exit code 1
        let failed = create_execution_result("host1", "task2", false, false, false, "failed");
        callback.on_task_complete(&failed).await;
        assert_eq!(callback.exit_code(), 1);
    }

    #[test]
    fn test_suggest_action_permission_denied() {
        let suggestion =
            ActionableCallback::suggest_action("Permission denied: /etc/nginx/nginx.conf");
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("permission"));
    }

    #[test]
    fn test_suggest_action_file_not_found() {
        let suggestion =
            ActionableCallback::suggest_action("No such file or directory: /tmp/missing");
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("file path"));
    }

    #[test]
    fn test_suggest_action_connection_refused() {
        let suggestion = ActionableCallback::suggest_action("Connection refused");
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("network"));
    }

    #[test]
    fn test_suggest_action_package_error() {
        let suggestion = ActionableCallback::suggest_action("Package nginx-core not found");
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("package"));
    }

    #[test]
    fn test_suggest_action_command_not_found() {
        let suggestion = ActionableCallback::suggest_action("command not found: docker");
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("Install"));
    }

    #[test]
    fn test_suggest_action_unknown_error() {
        let suggestion = ActionableCallback::suggest_action("Some random error message");
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_host_stats_has_actions() {
        let mut stats = HostStats::default();
        assert!(!stats.has_actions());

        stats.changed = 1;
        assert!(stats.has_actions());

        stats = HostStats::default();
        stats.failed = 1;
        assert!(stats.has_actions());

        stats = HostStats::default();
        stats.unreachable = 1;
        assert!(stats.has_actions());

        stats = HostStats::default();
        stats.ok = 5;
        stats.skipped = 2;
        assert!(!stats.has_actions());
    }

    #[test]
    fn test_default_config() {
        let config = ActionableConfig::default();
        assert!(config.show_play_headers);
        assert!(config.show_action_suggestions);
        assert!(config.show_change_summary);
        assert!(!config.show_duration);
        assert!(config.use_colors);
    }

    #[test]
    fn test_minimal_config() {
        let config = ActionableConfig::minimal();
        assert!(!config.show_play_headers);
        assert!(!config.show_action_suggestions);
        assert!(!config.show_change_summary);
        assert!(!config.show_recap);
    }

    #[test]
    fn test_ci_config() {
        let config = ActionableConfig::ci();
        assert!(!config.use_colors);
        assert!(config.show_action_suggestions);
    }

    #[test]
    fn test_verbose_config() {
        let config = ActionableConfig::verbose();
        assert!(config.show_duration);
        assert!(config.show_play_headers);
        assert!(config.show_action_suggestions);
    }

    #[test]
    fn test_clone_shares_stats() {
        let callback1 = ActionableCallback::new();
        let callback2 = callback1.clone();

        // Both should share the same underlying host_stats
        assert!(Arc::ptr_eq(&callback1.host_stats, &callback2.host_stats));
    }
}
