//! Minimal callback plugin for Rustible.
//!
//! This plugin provides minimal output - only failures and final recap.
//! Ideal for CI/CD pipelines where less noise is preferred.
//!
//! # Features
//!
//! - Silent on success: No output for ok/changed tasks
//! - Failure visibility: Shows task name and error only when failures occur
//! - Compact format: Single-line format for each host result
//! - Final recap: Summary of all hosts at the end
//!
//! # Example Output
//!
//! ```text
//! FAILED: webserver1 | Install nginx | Package installation failed: apt-get returned 100
//! FAILED: webserver2 | Configure nginx | File not found: /etc/nginx/nginx.conf
//!
//! RECAP: webserver1: ok=5 changed=2 failed=1 skipped=0
//! RECAP: webserver2: ok=3 changed=1 failed=1 skipped=1
//! RECAP: dbserver1: ok=8 changed=4 failed=0 skipped=0
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use colored::Colorize;
use tokio::sync::RwLock;

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

/// Minimal callback plugin that shows only failures and final recap.
///
/// This callback is designed for CI/CD environments where verbosity
/// should be minimized but failures must be clearly visible.
///
/// # Design Principles
///
/// 1. **Silent Success**: Ok and changed results produce no output
/// 2. **Loud Failures**: Failed tasks show immediately with context
/// 3. **Compact Summary**: Final recap uses single-line format per host
/// 4. **No Task Headers**: Task names only shown on failure
///
/// # Usage
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::MinimalCallback;
///
/// let callback = MinimalCallback::new();
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct MinimalCallback {
    /// Per-host execution statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Playbook start time for duration tracking
    start_time: Arc<RwLock<Option<Instant>>>,
    /// Current playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Whether any failures occurred (for exit code)
    has_failures: Arc<RwLock<bool>>,
}

impl MinimalCallback {
    /// Creates a new minimal callback plugin.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = MinimalCallback::new();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            start_time: Arc::new(RwLock::new(None)),
            playbook_name: Arc::new(RwLock::new(None)),
            has_failures: Arc::new(RwLock::new(false)),
        }
    }

    /// Returns whether any failures occurred during execution.
    ///
    /// Useful for determining exit codes in CI/CD.
    pub async fn has_failures(&self) -> bool {
        *self.has_failures.read().await
    }

    /// Formats a failure message in compact single-line format.
    fn format_failure(host: &str, task_name: &str, message: &str) -> String {
        format!(
            "{}: {} | {} | {}",
            "FAILED".red().bold(),
            host.bright_white().bold(),
            task_name.yellow(),
            message
        )
    }

    /// Formats an unreachable message in compact single-line format.
    fn format_unreachable(host: &str, task_name: &str, message: &str) -> String {
        format!(
            "{}: {} | {} | {}",
            "UNREACHABLE".magenta().bold(),
            host.bright_white().bold(),
            task_name.yellow(),
            message
        )
    }

    /// Formats a single host's recap line.
    fn format_recap_line(host: &str, stats: &HostStats) -> String {
        let host_color = if stats.failed > 0 || stats.unreachable > 0 {
            host.red().bold()
        } else if stats.changed > 0 {
            host.yellow()
        } else {
            host.green()
        };

        format!(
            "{}: {} ok={} changed={} failed={} skipped={} unreachable={}",
            "RECAP".bright_black(),
            host_color,
            stats.ok.to_string().green(),
            stats.changed.to_string().yellow(),
            stats.failed.to_string().red(),
            stats.skipped.to_string().cyan(),
            stats.unreachable.to_string().magenta(),
        )
    }
}

impl Default for MinimalCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MinimalCallback {
    fn clone(&self) -> Self {
        Self {
            host_stats: Arc::clone(&self.host_stats),
            start_time: Arc::clone(&self.start_time),
            playbook_name: Arc::clone(&self.playbook_name),
            has_failures: Arc::clone(&self.has_failures),
        }
    }
}

#[async_trait]
impl ExecutionCallback for MinimalCallback {
    /// Called when a playbook starts - records start time silently.
    async fn on_playbook_start(&self, name: &str) {
        let mut start_time = self.start_time.write().await;
        *start_time = Some(Instant::now());

        let mut playbook_name = self.playbook_name.write().await;
        *playbook_name = Some(name.to_string());

        // Clear stats from any previous run
        let mut stats = self.host_stats.write().await;
        stats.clear();

        let mut has_failures = self.has_failures.write().await;
        *has_failures = false;
    }

    /// Called when a playbook ends - prints the final recap.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let stats = self.host_stats.read().await;
        let start_time = self.start_time.read().await;

        // Print empty line before recap for visual separation
        if !stats.is_empty() {
            println!();
        }

        // Print recap for each host in sorted order
        let mut hosts: Vec<_> = stats.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_stats) = stats.get(host) {
                println!("{}", Self::format_recap_line(host, host_stats));
            }
        }

        // Print duration if we have start time
        if let Some(start) = *start_time {
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

    /// Called when a play starts - silent in minimal mode.
    async fn on_play_start(&self, _name: &str, hosts: &[String]) {
        // Initialize stats for all hosts in this play
        let mut stats = self.host_stats.write().await;
        for host in hosts {
            stats.entry(host.clone()).or_default();
        }
    }

    /// Called when a play ends - silent in minimal mode.
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Silent - recap is shown at playbook end
    }

    /// Called when a task starts - silent in minimal mode.
    async fn on_task_start(&self, _name: &str, _host: &str) {
        // Silent - we only show output on failure
    }

    /// Called when a task completes - only shows output on failure.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let mut stats = self.host_stats.write().await;
        let host_stats = stats.entry(result.host.clone()).or_default();

        // Update statistics based on result
        if result.result.skipped {
            host_stats.skipped += 1;
        } else if !result.result.success {
            host_stats.failed += 1;

            // Mark that we have failures
            let mut has_failures = self.has_failures.write().await;
            *has_failures = true;

            // Print failure immediately
            let message = result.result.message.as_str();
            println!(
                "{}",
                Self::format_failure(&result.host, &result.task_name, message)
            );
        } else if result.result.changed {
            host_stats.changed += 1;
            // Silent on change
        } else {
            host_stats.ok += 1;
            // Silent on ok
        }
    }

    /// Called when a handler is triggered - silent in minimal mode.
    async fn on_handler_triggered(&self, _name: &str) {
        // Silent - handlers are internal details
    }

    /// Called when facts are gathered - silent in minimal mode.
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // Silent - fact gathering is internal
    }
}

/// Trait extension for handling unreachable hosts.
///
/// This is separate from the main callback trait to allow for
/// optional implementation by callbacks that care about unreachability.
#[async_trait]
pub trait UnreachableCallback: ExecutionCallback {
    /// Called when a host becomes unreachable.
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str);
}

#[async_trait]
impl UnreachableCallback for MinimalCallback {
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str) {
        let mut stats = self.host_stats.write().await;
        let host_stats = stats.entry(host.to_string()).or_default();
        host_stats.unreachable += 1;

        // Mark that we have failures
        let mut has_failures = self.has_failures.write().await;
        *has_failures = true;

        // Print unreachable message immediately
        println!("{}", Self::format_unreachable(host, task_name, error));
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
    async fn test_minimal_callback_tracks_stats() {
        let callback = MinimalCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        // Simulate some task completions
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
        let stats = callback.host_stats.read().await;

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

        assert!(callback.has_failures().await);
    }

    #[tokio::test]
    async fn test_minimal_callback_no_failures() {
        let callback = MinimalCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        assert!(!callback.has_failures().await);
    }

    #[tokio::test]
    async fn test_unreachable_callback() {
        let callback = MinimalCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback
            .on_host_unreachable("host1", "gather_facts", "Connection refused")
            .await;

        let stats = callback.host_stats.read().await;
        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.unreachable, 1);

        assert!(callback.has_failures().await);
    }

    #[test]
    fn test_format_failure() {
        // Just ensure it doesn't panic and produces output
        let output = MinimalCallback::format_failure("host1", "Install nginx", "Package not found");
        assert!(output.contains("host1"));
        assert!(output.contains("Install nginx"));
        assert!(output.contains("Package not found"));
    }

    #[test]
    fn test_format_recap_line() {
        let stats = HostStats {
            ok: 5,
            changed: 2,
            failed: 1,
            skipped: 0,
            unreachable: 0,
        };

        let output = MinimalCallback::format_recap_line("webserver1", &stats);
        let output_plain = console::strip_ansi_codes(&output);

        assert!(output_plain.contains("webserver1"));
        assert!(output_plain.contains("ok=5"));
        assert!(output_plain.contains("changed=2"));
        assert!(output_plain.contains("failed=1"));
    }

    #[test]
    fn test_default_trait() {
        let callback = MinimalCallback::default();
        // Just verify it creates successfully
        assert!(Arc::strong_count(&callback.host_stats) == 1);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = MinimalCallback::new();
        let callback2 = callback1.clone();

        // Both should share the same underlying state
        assert!(Arc::ptr_eq(&callback1.host_stats, &callback2.host_stats));
        assert!(Arc::ptr_eq(
            &callback1.has_failures,
            &callback2.has_failures
        ));
    }
}
