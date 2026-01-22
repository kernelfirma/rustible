//! Oneline callback plugin for compact log output.
//!
//! This plugin outputs each task result on a single line, making it ideal for:
//! - Log files that need to be grep'd or filtered
//! - CI/CD pipelines with limited output space
//! - Machine parsing and monitoring
//!
//! # Output Format
//!
//! ```text
//! hostname | STATUS => result message
//! ```
//!
//! # Example Output
//!
//! ```text
//! webserver1 | OK => ok
//! webserver2 | CHANGED => File copied successfully
//! dbserver1 | FAILED => Connection refused
//! appserver1 | SKIPPED => Skipped: condition was false
//! ```
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::OnelineCallback;
//!
//! // Basic usage with defaults
//! let callback = OnelineCallback::new();
//! # let _ = ();
//!
//! // With custom configuration
//! let config = OnelineConfig::default()
//!     .with_task_name()
//!     .with_colors();
//! let callback = OnelineCallback::with_config(config);
//! # Ok(())
//! # }
//! ```
//!
//! # Grep-Friendly Output
//!
//! The oneline format is designed for easy filtering:
//!
//! ```bash
//! # Show only failures
//! rustible-playbook site.yml | grep FAILED
//!
//! # Show changes on specific host
//! rustible-playbook site.yml | grep "webserver1.*CHANGED"
//!
//! # Count skipped tasks
//! rustible-playbook site.yml | grep -c SKIPPED
//! ```

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the oneline callback plugin.
#[derive(Debug, Clone)]
pub struct OnelineConfig {
    /// Show task name in output (default: false for maximum compactness)
    pub show_task_name: bool,
    /// Show timestamps (default: false)
    pub show_timestamp: bool,
    /// Maximum message length before truncation (0 = no limit)
    pub max_message_length: usize,
    /// Use colors in output (default: false for log compatibility)
    pub use_colors: bool,
    /// Separator between hostname and status (default: " | ")
    pub separator: String,
    /// Separator between status and result (default: " => ")
    pub result_separator: String,
    /// Output to stderr instead of stdout
    pub use_stderr: bool,
    /// Show playbook/play headers (default: true)
    pub show_headers: bool,
    /// Show recap at end (default: true)
    pub show_recap: bool,
}

impl Default for OnelineConfig {
    fn default() -> Self {
        Self {
            show_task_name: false,
            show_timestamp: false,
            max_message_length: 0,
            use_colors: false,
            separator: " | ".to_string(),
            result_separator: " => ".to_string(),
            use_stderr: false,
            show_headers: true,
            show_recap: true,
        }
    }
}

impl OnelineConfig {
    /// Create a new configuration with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable task name in output.
    pub fn with_task_name(mut self) -> Self {
        self.show_task_name = true;
        self
    }

    /// Enable timestamps in output.
    pub fn with_timestamp(mut self) -> Self {
        self.show_timestamp = true;
        self
    }

    /// Set maximum message length.
    pub fn with_max_length(mut self, length: usize) -> Self {
        self.max_message_length = length;
        self
    }

    /// Enable colored output.
    pub fn with_colors(mut self) -> Self {
        self.use_colors = true;
        self
    }

    /// Use stderr for output.
    pub fn with_stderr(mut self) -> Self {
        self.use_stderr = true;
        self
    }

    /// Set custom separator.
    pub fn with_separator(mut self, sep: impl Into<String>) -> Self {
        self.separator = sep.into();
        self
    }

    /// Set custom result separator.
    pub fn with_result_separator(mut self, sep: impl Into<String>) -> Self {
        self.result_separator = sep.into();
        self
    }

    /// Disable headers.
    pub fn without_headers(mut self) -> Self {
        self.show_headers = false;
        self
    }

    /// Disable recap.
    pub fn without_recap(mut self) -> Self {
        self.show_recap = false;
        self
    }
}

// ============================================================================
// Host Statistics
// ============================================================================

/// Statistics for a single host during playbook execution.
#[derive(Debug, Clone, Default)]
struct HostStats {
    ok: u64,
    changed: u64,
    failed: u64,
    skipped: u64,
    unreachable: u64,
}

impl HostStats {
    fn update(&mut self, result: &ModuleResult) {
        if !result.success {
            self.failed += 1;
        } else if result.skipped {
            self.skipped += 1;
        } else if result.changed {
            self.changed += 1;
        } else {
            self.ok += 1;
        }
    }

    fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }
}

// ============================================================================
// Oneline Callback
// ============================================================================

/// Oneline callback plugin for compact, grep-friendly output.
///
/// Each task result is output on a single line with the format:
/// ```text
/// hostname | STATUS => result
/// ```
///
/// This format is designed to be:
/// - Easy to grep and filter
/// - Suitable for log files
/// - Machine-parseable
/// - Compact for CI/CD pipelines
pub struct OnelineCallback {
    config: OnelineConfig,
    /// Track if we've printed anything (for recap formatting)
    has_output: AtomicBool,
    /// Playbook start time
    start_time: RwLock<Option<Instant>>,
    /// Per-host statistics
    host_stats: RwLock<std::collections::HashMap<String, HostStats>>,
    /// Total task count
    task_count: AtomicU64,
}

impl OnelineCallback {
    /// Create a new oneline callback with default configuration.
    pub fn new() -> Self {
        Self {
            config: OnelineConfig::default(),
            has_output: AtomicBool::new(false),
            start_time: RwLock::new(None),
            host_stats: RwLock::new(std::collections::HashMap::new()),
            task_count: AtomicU64::new(0),
        }
    }

    /// Create a new oneline callback with custom configuration.
    pub fn with_config(config: OnelineConfig) -> Self {
        Self {
            config,
            has_output: AtomicBool::new(false),
            start_time: RwLock::new(None),
            host_stats: RwLock::new(std::collections::HashMap::new()),
            task_count: AtomicU64::new(0),
        }
    }

    /// Format and output a line.
    fn output_line(&self, line: &str) {
        self.has_output.store(true, Ordering::SeqCst);

        if self.config.use_stderr {
            let _ = writeln!(io::stderr(), "{}", line);
        } else {
            println!("{}", line);
        }
    }

    /// Get the status string for a result.
    fn get_status(&self, result: &ModuleResult) -> &'static str {
        if !result.success {
            "FAILED"
        } else if result.skipped {
            "SKIPPED"
        } else if result.changed {
            "CHANGED"
        } else {
            "OK"
        }
    }

    /// Format the status string, optionally with colors.
    fn format_status(&self, result: &ModuleResult) -> String {
        let status = self.get_status(result);

        if self.config.use_colors {
            if !result.success {
                format!("\x1b[31m{}\x1b[0m", status) // Red
            } else if result.skipped {
                format!("\x1b[36m{}\x1b[0m", status) // Cyan
            } else if result.changed {
                format!("\x1b[33m{}\x1b[0m", status) // Yellow
            } else {
                format!("\x1b[32m{}\x1b[0m", status) // Green
            }
        } else {
            status.to_string()
        }
    }

    /// Truncate a message if configured.
    fn truncate_message(&self, msg: &str) -> String {
        if self.config.max_message_length > 0 && msg.len() > self.config.max_message_length {
            format!("{}...", &msg[..self.config.max_message_length - 3])
        } else {
            msg.to_string()
        }
    }

    /// Get the result message.
    fn get_result_message(&self, result: &ModuleResult) -> String {
        let msg = &result.message;
        if !msg.is_empty() {
            self.truncate_message(msg)
        } else {
            // Fall back to a default message based on status
            self.get_status(result).to_lowercase()
        }
    }

    /// Format a complete task result line.
    fn format_task_line(&self, exec_result: &ExecutionResult) -> String {
        let mut parts = Vec::new();

        // Optional timestamp
        if self.config.show_timestamp {
            let now = chrono::Utc::now();
            parts.push(now.format("%Y-%m-%d %H:%M:%S").to_string());
        }

        // Hostname
        parts.push(exec_result.host.clone());

        // Optional task name
        if self.config.show_task_name {
            parts.push(format!("[{}]", exec_result.task_name));
        }

        // Build the line
        let prefix = parts.join(" ");
        let status = self.format_status(&exec_result.result);
        let message = self.get_result_message(&exec_result.result);

        format!(
            "{}{}{}{}{}",
            prefix, self.config.separator, status, self.config.result_separator, message
        )
    }

    /// Update host statistics.
    fn update_stats(&self, host: &str, result: &ModuleResult) {
        let mut stats = self.host_stats.write();
        let host_stats = stats.entry(host.to_string()).or_default();
        host_stats.update(result);
        self.task_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl Default for OnelineCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OnelineCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnelineCallback")
            .field("config", &self.config)
            .finish()
    }
}

#[async_trait]
impl ExecutionCallback for OnelineCallback {
    async fn on_playbook_start(&self, name: &str) {
        *self.start_time.write() = Some(Instant::now());

        if self.config.show_headers {
            self.output_line(&format!("PLAYBOOK: {}", name));
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        if !self.config.show_recap {
            return;
        }

        // Calculate duration
        let duration = self
            .start_time
            .read()
            .map(|t| t.elapsed())
            .unwrap_or_default();

        let playbook_status = if success { "SUCCESS" } else { "FAILED" };

        // Print recap header
        self.output_line("");
        self.output_line(&format!(
            "PLAYBOOK RECAP: {}{}{}  ({:.2}s)",
            name,
            self.config.separator,
            playbook_status,
            duration.as_secs_f64()
        ));

        // Print per-host statistics
        let stats = self.host_stats.read();
        let mut hosts: Vec<_> = stats.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_stats) = stats.get(host) {
                let host_status = if host_stats.has_failures() {
                    "FAILED"
                } else if host_stats.changed > 0 {
                    "CHANGED"
                } else {
                    "OK"
                };

                self.output_line(&format!(
                    "RECAP: {}{}{}{}ok={} changed={} failed={} skipped={} unreachable={}",
                    host,
                    self.config.separator,
                    host_status,
                    self.config.separator,
                    host_stats.ok,
                    host_stats.changed,
                    host_stats.failed,
                    host_stats.skipped,
                    host_stats.unreachable
                ));
            }
        }
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        if self.config.show_headers {
            self.output_line(&format!("PLAY [{}] hosts={}", name, hosts.len()));
        }
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Play end is handled by recap
    }

    async fn on_task_start(&self, _name: &str, _host: &str) {
        // No output on task start for oneline format - maximum compactness
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Update statistics
        self.update_stats(&result.host, &result.result);

        // Output the oneline format
        let line = self.format_task_line(result);
        self.output_line(&line);
    }

    async fn on_handler_triggered(&self, name: &str) {
        if self.config.show_headers {
            self.output_line(&format!("HANDLER: {}", name));
        }
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        self.output_line(&format!(
            "{}{}FACTS{}gathered",
            host, self.config.separator, self.config.result_separator
        ));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_result(
        host: &str,
        success: bool,
        changed: bool,
        skipped: bool,
        msg: &str,
    ) -> ExecutionResult {
        ExecutionResult {
            host: host.to_string(),
            task_name: "test task".to_string(),
            result: ModuleResult {
                success,
                changed,
                message: msg.to_string(),
                skipped,
                data: None,
                warnings: Vec::new(),
            },
            duration: std::time::Duration::from_millis(100),
            notify: Vec::new(),
        }
    }

    #[test]
    fn test_default_format_ok() {
        let callback = OnelineCallback::new();
        let result = create_test_result("webserver1", true, false, false, "ok");
        let line = callback.format_task_line(&result);
        assert_eq!(line, "webserver1 | OK => ok");
    }

    #[test]
    fn test_default_format_changed() {
        let callback = OnelineCallback::new();
        let result = create_test_result("dbserver1", true, true, false, "file updated");
        let line = callback.format_task_line(&result);
        assert_eq!(line, "dbserver1 | CHANGED => file updated");
    }

    #[test]
    fn test_default_format_failed() {
        let callback = OnelineCallback::new();
        let result = create_test_result("appserver1", false, false, false, "connection refused");
        let line = callback.format_task_line(&result);
        assert_eq!(line, "appserver1 | FAILED => connection refused");
    }

    #[test]
    fn test_default_format_skipped() {
        let callback = OnelineCallback::new();
        let result = create_test_result("host1", true, false, true, "condition was false");
        let line = callback.format_task_line(&result);
        assert_eq!(line, "host1 | SKIPPED => condition was false");
    }

    #[test]
    fn test_with_task_name() {
        let config = OnelineConfig::new().with_task_name();
        let callback = OnelineCallback::with_config(config);
        let result = create_test_result("webserver1", true, false, false, "ok");
        let line = callback.format_task_line(&result);
        assert_eq!(line, "webserver1 [test task] | OK => ok");
    }

    #[test]
    fn test_truncation() {
        let config = OnelineConfig::new().with_max_length(20);
        let callback = OnelineCallback::with_config(config);
        let result = create_test_result(
            "host1",
            true,
            false,
            false,
            "This is a very long message that should be truncated",
        );
        let line = callback.format_task_line(&result);
        assert!(line.contains("..."));
        assert!(line.len() < 80);
    }

    #[test]
    fn test_custom_separators() {
        let config = OnelineConfig::new()
            .with_separator(" :: ")
            .with_result_separator(" -> ");
        let callback = OnelineCallback::with_config(config);
        let result = create_test_result("host1", true, false, false, "ok");
        let line = callback.format_task_line(&result);
        assert_eq!(line, "host1 :: OK -> ok");
    }

    #[test]
    fn test_empty_message_fallback() {
        let callback = OnelineCallback::new();
        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "test task".to_string(),
            result: ModuleResult {
                success: true,
                changed: false,
                message: "".to_string(),
                skipped: false,
                data: None,
                warnings: Vec::new(),
            },
            duration: std::time::Duration::from_millis(100),
            notify: Vec::new(),
        };
        let line = callback.format_task_line(&result);
        assert!(line.contains("ok")); // Falls back to status
    }

    #[test]
    fn test_status_formatting_no_colors() {
        let callback = OnelineCallback::new();

        let ok_result = ModuleResult::ok("ok");
        assert_eq!(callback.format_status(&ok_result), "OK");

        let changed_result = ModuleResult::changed("changed");
        assert_eq!(callback.format_status(&changed_result), "CHANGED");

        let failed_result = ModuleResult::failed("failed");
        assert_eq!(callback.format_status(&failed_result), "FAILED");

        let skipped_result = ModuleResult::skipped("skipped");
        assert_eq!(callback.format_status(&skipped_result), "SKIPPED");
    }

    #[test]
    fn test_colored_status() {
        let config = OnelineConfig::new().with_colors();
        let callback = OnelineCallback::with_config(config);

        let ok_result = ModuleResult::ok("ok");
        let status = callback.format_status(&ok_result);
        assert!(status.contains("\x1b[32m")); // Green
        assert!(status.contains("\x1b[0m")); // Reset
    }

    #[test]
    fn test_host_stats() {
        let callback = OnelineCallback::new();

        // Simulate task completions
        let ok = ModuleResult::ok("ok");
        let changed = ModuleResult::changed("changed");
        let failed = ModuleResult::failed("failed");
        let skipped = ModuleResult::skipped("skipped");

        callback.update_stats("host1", &ok);
        callback.update_stats("host1", &changed);
        callback.update_stats("host1", &failed);
        callback.update_stats("host1", &skipped);

        let stats = callback.host_stats.read();
        let host_stats = stats.get("host1").unwrap();

        assert_eq!(host_stats.ok, 1);
        assert_eq!(host_stats.changed, 1);
        assert_eq!(host_stats.failed, 1);
        assert_eq!(host_stats.skipped, 1);
        assert!(host_stats.has_failures());
    }

    #[test]
    fn test_config_builder() {
        let config = OnelineConfig::new()
            .with_task_name()
            .with_timestamp()
            .with_max_length(100)
            .with_colors()
            .with_separator(" :: ")
            .with_result_separator(" -> ")
            .with_stderr()
            .without_headers()
            .without_recap();

        assert!(config.show_task_name);
        assert!(config.show_timestamp);
        assert_eq!(config.max_message_length, 100);
        assert!(config.use_colors);
        assert_eq!(config.separator, " :: ");
        assert_eq!(config.result_separator, " -> ");
        assert!(config.use_stderr);
        assert!(!config.show_headers);
        assert!(!config.show_recap);
    }
}
