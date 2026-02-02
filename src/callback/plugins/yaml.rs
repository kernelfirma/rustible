//! YAML Callback Plugin for Rustible
//!
//! This plugin outputs execution results in a human-readable YAML format that
//! is also machine-parseable. It provides clear structure with proper indentation
//! for nested data, making it easy to read while maintaining compatibility with
//! YAML parsers.
//!
//! # Features
//!
//! - Human-readable output with proper indentation
//! - Machine-parseable YAML format
//! - Colorized status indicators (optional)
//! - Structured task results with nested data
//! - Full diff support for file changes
//! - Execution timing information
//!
//! # Example Output
//!
//! ```yaml
//! ---
//! playbook: site.yml
//! plays:
//!   - name: Configure web servers
//!     hosts:
//!       - web01
//!       - web02
//!     tasks:
//!       - task: Install nginx
//!         host: web01
//!         status: changed
//!         changed: true
//!         msg: "Package nginx installed"
//!         duration: 2.45s
//! ...
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::{YamlCallback, YamlConfig};
//!
//! // Default configuration
//! let callback = YamlCallback::new();
//!
//! // Or with custom configuration
//! let config = YamlConfig::builder()
//!     .use_color(false)
//!     .indent_size(4)
//!     .show_duration(true)
//!     .build();
//! let callback = YamlCallback::with_config(config);
//!
//! # let _ = ();
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use parking_lot::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the YAML callback plugin.
///
/// Use [`YamlConfig::builder()`] for a fluent construction API.
#[derive(Debug, Clone)]
pub struct YamlConfig {
    /// Whether to use colored output (default: true)
    pub use_color: bool,
    /// Indentation size in spaces (default: 2)
    pub indent_size: usize,
    /// Whether to show task durations (default: true)
    pub show_duration: bool,
    /// Whether to show full result data (default: true)
    pub show_result_data: bool,
    /// Whether to show diffs (default: true)
    pub show_diff: bool,
    /// Whether to show stdout/stderr for command modules (default: true)
    pub show_command_output: bool,
    /// Maximum lines of stdout/stderr to show, 0 = unlimited (default: 50)
    pub max_output_lines: usize,
    /// Whether to display empty/null values (default: false)
    pub show_empty_values: bool,
    /// Whether to show warnings (default: true)
    pub show_warnings: bool,
    /// Verbosity level (default: 0)
    pub verbosity: u8,
}

impl Default for YamlConfig {
    fn default() -> Self {
        Self {
            use_color: true,
            indent_size: 2,
            show_duration: true,
            show_result_data: true,
            show_diff: true,
            show_command_output: true,
            max_output_lines: 50,
            show_empty_values: false,
            show_warnings: true,
            verbosity: 0,
        }
    }
}

impl YamlConfig {
    /// Create a new builder for YamlConfig.
    pub fn builder() -> YamlConfigBuilder {
        YamlConfigBuilder::default()
    }
}

/// Builder for [`YamlConfig`] with fluent API.
#[derive(Debug, Default)]
pub struct YamlConfigBuilder {
    config: YamlConfig,
}

impl YamlConfigBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable colored output.
    pub fn use_color(mut self, use_color: bool) -> Self {
        self.config.use_color = use_color;
        self
    }

    /// Set indentation size (number of spaces).
    pub fn indent_size(mut self, size: usize) -> Self {
        self.config.indent_size = size;
        self
    }

    /// Enable or disable duration display.
    pub fn show_duration(mut self, show: bool) -> Self {
        self.config.show_duration = show;
        self
    }

    /// Enable or disable full result data display.
    pub fn show_result_data(mut self, show: bool) -> Self {
        self.config.show_result_data = show;
        self
    }

    /// Enable or disable diff display.
    pub fn show_diff(mut self, show: bool) -> Self {
        self.config.show_diff = show;
        self
    }

    /// Enable or disable command output display.
    pub fn show_command_output(mut self, show: bool) -> Self {
        self.config.show_command_output = show;
        self
    }

    /// Set maximum output lines (0 = unlimited).
    pub fn max_output_lines(mut self, lines: usize) -> Self {
        self.config.max_output_lines = lines;
        self
    }

    /// Enable or disable empty value display.
    pub fn show_empty_values(mut self, show: bool) -> Self {
        self.config.show_empty_values = show;
        self
    }

    /// Enable or disable warnings display.
    pub fn show_warnings(mut self, show: bool) -> Self {
        self.config.show_warnings = show;
        self
    }

    /// Set verbosity level.
    pub fn verbosity(mut self, level: u8) -> Self {
        self.config.verbosity = level;
        self
    }

    /// Build the YamlConfig.
    pub fn build(self) -> YamlConfig {
        self.config
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

// ============================================================================
// YAML Callback Plugin
// ============================================================================

/// YAML callback plugin for human-readable, machine-parseable output.
///
/// This plugin outputs all execution events in YAML format, making it easy
/// to both read and parse programmatically. The output is structured with
/// proper indentation and includes all relevant execution details.
///
/// ## Key Features
///
/// - **Structured Output**: Proper YAML document structure with nested elements
/// - **Colorized Status**: Status indicators are colored for easy scanning
/// - **Duration Tracking**: Shows execution time for tasks and playbooks
/// - **Diff Support**: Shows before/after for changed resources
/// - **Command Output**: Includes stdout/stderr for shell commands
///
/// ## Thread Safety
///
/// The callback uses `RwLock` internally for state management, making it
/// safe to use across multiple threads during parallel task execution.
///
/// ## Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::{YamlCallback, YamlConfig};
///
/// // With default settings
/// let callback = YamlCallback::new();
///
/// // With custom settings
/// let config = YamlConfig::builder()
///     .use_color(false)
///     .indent_size(4)
///     .build();
/// let callback = YamlCallback::with_config(config);
/// # Ok(())
/// # }
/// ```
pub struct YamlCallback {
    /// Plugin configuration
    config: YamlConfig,
    /// Per-host execution statistics
    host_stats: RwLock<HashMap<String, HostStats>>,
    /// Playbook start time for duration tracking
    start_time: RwLock<Option<Instant>>,
    /// Current indentation level
    indent_level: RwLock<usize>,
    /// Current playbook name
    current_playbook: RwLock<Option<String>>,
    /// Whether we're in the tasks section
    in_tasks_section: RwLock<bool>,
}

impl fmt::Debug for YamlCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("YamlCallback")
            .field("config", &self.config)
            .field("current_playbook", &*self.current_playbook.read())
            .finish()
    }
}

impl YamlCallback {
    /// Create a new YAML callback plugin with default settings.
    pub fn new() -> Self {
        Self::with_config(YamlConfig::default())
    }

    /// Create a new YAML callback plugin with custom configuration.
    pub fn with_config(config: YamlConfig) -> Self {
        Self {
            config,
            host_stats: RwLock::new(HashMap::new()),
            start_time: RwLock::new(None),
            indent_level: RwLock::new(0),
            current_playbook: RwLock::new(None),
            in_tasks_section: RwLock::new(false),
        }
    }

    /// Create a YAML callback with colors disabled.
    pub fn without_colors() -> Self {
        let config = YamlConfig {
            use_color: false,
            ..Default::default()
        };
        Self::with_config(config)
    }

    // ========================================================================
    // Indentation Helpers
    // ========================================================================

    /// Get the current indentation string.
    fn indent(&self) -> String {
        let level = *self.indent_level.read();
        " ".repeat(level * self.config.indent_size)
    }

    /// Increase indentation level.
    fn push_indent(&self) {
        let mut level = self.indent_level.write();
        *level += 1;
    }

    /// Decrease indentation level.
    fn pop_indent(&self) {
        let mut level = self.indent_level.write();
        if *level > 0 {
            *level -= 1;
        }
    }

    /// Reset indentation to zero.
    fn reset_indent(&self) {
        let mut level = self.indent_level.write();
        *level = 0;
    }

    // ========================================================================
    // Formatting Helpers
    // ========================================================================

    /// Format a duration as a human-readable string.
    fn format_duration(&self, duration: Duration) -> String {
        let secs = duration.as_secs_f64();
        if secs < 0.001 {
            format!("{:.0}us", duration.as_micros())
        } else if secs < 1.0 {
            format!("{:.0}ms", duration.as_millis())
        } else if secs < 60.0 {
            format!("{:.2}s", secs)
        } else if secs < 3600.0 {
            let mins = (secs / 60.0).floor();
            let remaining_secs = secs % 60.0;
            format!("{:.0}m {:.0}s", mins, remaining_secs)
        } else {
            let hours = (secs / 3600.0).floor();
            let remaining_mins = ((secs % 3600.0) / 60.0).floor();
            format!("{:.0}h {:.0}m", hours, remaining_mins)
        }
    }

    /// Format a status with optional color.
    fn format_status(&self, success: bool, changed: bool, skipped: bool) -> String {
        let status_str = if skipped {
            "skipped"
        } else if !success {
            "failed"
        } else if changed {
            "changed"
        } else {
            "ok"
        };

        if self.config.use_color {
            match status_str {
                "skipped" => status_str.cyan().to_string(),
                "failed" => status_str.red().bold().to_string(),
                "changed" => status_str.yellow().to_string(),
                _ => status_str.green().to_string(),
            }
        } else {
            status_str.to_string()
        }
    }

    /// Escape a string for YAML output.
    fn yaml_escape(&self, s: &str) -> String {
        // Check if we need quoting
        let needs_quoting = s.is_empty()
            || s.contains(':')
            || s.contains('#')
            || s.contains('\n')
            || s.contains('\r')
            || s.contains('\t')
            || s.starts_with(' ')
            || s.ends_with(' ')
            || s.starts_with('\'')
            || s.starts_with('"')
            || s.starts_with('[')
            || s.starts_with('{')
            || s.starts_with('&')
            || s.starts_with('*')
            || s.starts_with('!')
            || s.starts_with('|')
            || s.starts_with('>')
            || s.starts_with('%')
            || s.starts_with('@')
            || s.starts_with('`')
            || s == "true"
            || s == "false"
            || s == "null"
            || s == "~"
            || s.parse::<f64>().is_ok();

        if needs_quoting {
            // Use double quotes and escape special chars
            let escaped = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            format!("\"{}\"", escaped)
        } else {
            s.to_string()
        }
    }

    /// Format a multiline string as a YAML literal block.
    fn format_multiline(&self, s: &str, base_indent: &str) -> String {
        if !s.contains('\n') {
            return self.yaml_escape(s);
        }

        let mut result = String::from("|\n");
        for line in s.lines() {
            result.push_str(base_indent);
            result.push_str("  ");
            result.push_str(line);
            result.push('\n');
        }
        // Remove trailing newline
        result.pop();
        result
    }

    /// Truncate output if it exceeds max lines.
    #[allow(dead_code)]
    fn truncate_output(&self, output: &str) -> String {
        if self.config.max_output_lines == 0 {
            return output.to_string();
        }

        let lines: Vec<&str> = output.lines().collect();
        if lines.len() <= self.config.max_output_lines {
            return output.to_string();
        }

        let half = self.config.max_output_lines / 2;
        let mut result = String::new();

        for line in lines.iter().take(half) {
            result.push_str(line);
            result.push('\n');
        }

        result.push_str(&format!(
            "... ({} lines omitted) ...\n",
            lines.len() - self.config.max_output_lines
        ));

        for line in lines.iter().skip(lines.len() - half) {
            result.push_str(line);
            result.push('\n');
        }

        result
    }

    // ========================================================================
    // Output Helpers
    // ========================================================================

    /// Output a YAML key-value pair.
    fn output_kv(&self, key: &str, value: &str) {
        let indent = self.indent();
        println!("{}{}: {}", indent, key, value);
    }

    /// Output a YAML key with a literal value (no escaping).
    fn output_kv_literal(&self, key: &str, value: &str) {
        let indent = self.indent();
        println!("{}{}: {}", indent, key, value);
    }

    /// Output a YAML list item.
    fn output_list_item(&self, value: &str) {
        let indent = self.indent();
        println!("{}- {}", indent, value);
    }

    /// Output the start of a YAML list item object.
    fn output_list_item_key(&self, key: &str, value: &str) {
        let indent = self.indent();
        println!("{}- {}: {}", indent, key, value);
    }

    /// Output a JSON value as YAML.
    fn output_json_value(&self, key: &str, value: &serde_json::Value) {
        let indent = self.indent();

        match value {
            serde_json::Value::Null => {
                if self.config.show_empty_values {
                    println!("{}{}: null", indent, key);
                }
            }
            serde_json::Value::Bool(b) => {
                println!("{}{}: {}", indent, key, b);
            }
            serde_json::Value::Number(n) => {
                println!("{}{}: {}", indent, key, n);
            }
            serde_json::Value::String(s) => {
                if s.is_empty() && !self.config.show_empty_values {
                    return;
                }
                if s.contains('\n') {
                    println!("{}{}: {}", indent, key, self.format_multiline(s, &indent));
                } else {
                    println!("{}{}: {}", indent, key, self.yaml_escape(s));
                }
            }
            serde_json::Value::Array(arr) => {
                if arr.is_empty() && !self.config.show_empty_values {
                    return;
                }
                println!("{}{}:", indent, key);
                self.push_indent();
                for item in arr {
                    self.output_json_array_item(item);
                }
                self.pop_indent();
            }
            serde_json::Value::Object(obj) => {
                if obj.is_empty() && !self.config.show_empty_values {
                    return;
                }
                println!("{}{}:", indent, key);
                self.push_indent();
                for (k, v) in obj {
                    self.output_json_value(k, v);
                }
                self.pop_indent();
            }
        }
    }

    /// Output a JSON array item.
    fn output_json_array_item(&self, value: &serde_json::Value) {
        let indent = self.indent();

        match value {
            serde_json::Value::Null => {
                println!("{}- null", indent);
            }
            serde_json::Value::Bool(b) => {
                println!("{}- {}", indent, b);
            }
            serde_json::Value::Number(n) => {
                println!("{}- {}", indent, n);
            }
            serde_json::Value::String(s) => {
                println!("{}- {}", indent, self.yaml_escape(s));
            }
            serde_json::Value::Array(arr) => {
                println!("{}-", indent);
                self.push_indent();
                for item in arr {
                    self.output_json_array_item(item);
                }
                self.pop_indent();
            }
            serde_json::Value::Object(obj) => {
                let mut first = true;
                for (k, v) in obj {
                    if first {
                        // First item uses list marker
                        print!("{}- ", indent);
                        self.output_inline_kv(k, v);
                        first = false;
                    } else {
                        // Subsequent items are indented
                        print!("{}  ", indent);
                        self.output_inline_kv(k, v);
                    }
                }
            }
        }
    }

    /// Output an inline key-value (without indent prefix).
    fn output_inline_kv(&self, key: &str, value: &serde_json::Value) {
        match value {
            serde_json::Value::Null => {
                println!("{}: null", key);
            }
            serde_json::Value::Bool(b) => {
                println!("{}: {}", key, b);
            }
            serde_json::Value::Number(n) => {
                println!("{}: {}", key, n);
            }
            serde_json::Value::String(s) => {
                println!("{}: {}", key, self.yaml_escape(s));
            }
            _ => {
                // For complex types, serialize to JSON string
                println!(
                    "{}: {}",
                    key,
                    serde_json::to_string(value).unwrap_or_default()
                );
            }
        }
    }
}

impl Default for YamlCallback {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ExecutionCallback Implementation
// ============================================================================

#[async_trait]
impl ExecutionCallback for YamlCallback {
    /// Called when a playbook starts.
    async fn on_playbook_start(&self, name: &str) {
        self.reset_indent();

        // Store start time and playbook name
        *self.start_time.write() = Some(Instant::now());
        *self.current_playbook.write() = Some(name.to_string());

        // Clear stats from any previous run
        self.host_stats.write().clear();

        // YAML document start
        println!("---");
        self.output_kv("playbook", &self.yaml_escape(name));
        println!("{}plays:", self.indent());
    }

    /// Called when a playbook ends.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let start_time = *self.start_time.read();
        let duration = start_time.map(|s| s.elapsed()).unwrap_or_default();

        // Close any open sections
        *self.in_tasks_section.write() = false;

        // Output summary
        println!();
        println!("---");
        println!("summary:");
        self.push_indent();

        self.output_kv("playbook", &self.yaml_escape(name));
        self.output_kv_literal("duration", &self.format_duration(duration));

        let playbook_status = if success { "success" } else { "failed" };
        let playbook_status_str = if self.config.use_color {
            if success {
                playbook_status.green().to_string()
            } else {
                playbook_status.red().bold().to_string()
            }
        } else {
            playbook_status.to_string()
        };
        self.output_kv_literal("status", &playbook_status_str);

        // Output per-host stats
        let stats = self.host_stats.read();
        if !stats.is_empty() {
            println!("{}hosts:", self.indent());
            self.push_indent();

            let mut hosts: Vec<_> = stats.keys().collect();
            hosts.sort();

            for host in hosts {
                if let Some(host_stats) = stats.get(host) {
                    self.output_list_item_key("host", &self.yaml_escape(host));
                    self.push_indent();

                    // Use colored status summary
                    let ok_str = if self.config.use_color {
                        host_stats.ok.to_string().green().to_string()
                    } else {
                        host_stats.ok.to_string()
                    };
                    let changed_str = if self.config.use_color {
                        host_stats.changed.to_string().yellow().to_string()
                    } else {
                        host_stats.changed.to_string()
                    };
                    let failed_str = if self.config.use_color {
                        host_stats.failed.to_string().red().to_string()
                    } else {
                        host_stats.failed.to_string()
                    };
                    let skipped_str = if self.config.use_color {
                        host_stats.skipped.to_string().cyan().to_string()
                    } else {
                        host_stats.skipped.to_string()
                    };

                    self.output_kv_literal("ok", &ok_str);
                    self.output_kv_literal("changed", &changed_str);
                    self.output_kv_literal("failed", &failed_str);
                    self.output_kv_literal("skipped", &skipped_str);

                    if host_stats.unreachable > 0 {
                        let unreachable_str = if self.config.use_color {
                            host_stats.unreachable.to_string().magenta().to_string()
                        } else {
                            host_stats.unreachable.to_string()
                        };
                        self.output_kv_literal("unreachable", &unreachable_str);
                    }

                    self.pop_indent();
                }
            }

            self.pop_indent();
        }

        self.pop_indent();

        // YAML document end
        println!("...");

        // Flush output
        let _ = io::stdout().flush();
    }

    /// Called when a play starts.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.push_indent();

        self.output_list_item_key("name", &self.yaml_escape(name));
        self.push_indent();

        // Output hosts list
        println!("{}hosts:", self.indent());
        self.push_indent();
        for host in hosts {
            self.output_list_item(&self.yaml_escape(host));
            // Initialize host stats
            self.host_stats.write().entry(host.clone()).or_default();
        }
        self.pop_indent();

        // Start tasks section
        println!("{}tasks:", self.indent());
        *self.in_tasks_section.write() = true;
    }

    /// Called when a play ends.
    async fn on_play_end(&self, _name: &str, _success: bool) {
        *self.in_tasks_section.write() = false;
        self.pop_indent();
        self.pop_indent();
    }

    /// Called when a task starts.
    async fn on_task_start(&self, _name: &str, _host: &str) {
        // We output task info with results, not on start
        // This provides better grouping in the output
    }

    /// Called when a task completes.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Update statistics
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

        // Output task result
        self.push_indent();

        self.output_list_item_key("task", &self.yaml_escape(&result.task_name));
        self.push_indent();

        self.output_kv("host", &self.yaml_escape(&result.host));

        let status = self.format_status(
            result.result.success,
            result.result.changed,
            result.result.skipped,
        );
        self.output_kv_literal("status", &status);
        self.output_kv_literal("changed", &result.result.changed.to_string());

        // Message
        if !result.result.message.is_empty() {
            let indent = self.indent();
            if result.result.message.contains('\n') {
                println!(
                    "{}msg: {}",
                    indent,
                    self.format_multiline(&result.result.message, &indent)
                );
            } else {
                self.output_kv("msg", &self.yaml_escape(&result.result.message));
            }
        }

        // Duration
        if self.config.show_duration {
            self.output_kv_literal("duration", &self.format_duration(result.duration));
        }

        // Warnings
        if self.config.show_warnings && !result.result.warnings.is_empty() {
            println!("{}warnings:", self.indent());
            self.push_indent();
            for warning in &result.result.warnings {
                let warning_str = if self.config.use_color {
                    self.yaml_escape(warning).yellow().to_string()
                } else {
                    self.yaml_escape(warning)
                };
                self.output_list_item(&warning_str);
            }
            self.pop_indent();
        }

        // Result data
        if self.config.show_result_data {
            if let Some(ref data) = result.result.data {
                if !data.is_null() && data != &serde_json::Value::Object(serde_json::Map::new()) {
                    self.output_json_value("result", data);
                }
            }
        }

        self.pop_indent();
        self.pop_indent();
    }

    /// Called when a handler is triggered.
    async fn on_handler_triggered(&self, name: &str) {
        let indent = self.indent();
        if self.config.use_color {
            println!("{}# Handler triggered: {}", indent, name.bright_blue());
        } else {
            println!("{}# Handler triggered: {}", indent, name);
        }
    }

    /// Called when facts are gathered.
    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        let indent = self.indent();
        if self.config.use_color {
            println!("{}# Facts gathered for: {}", indent, host.bright_green());
        } else {
            println!("{}# Facts gathered for: {}", indent, host);
        }
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
    fn test_yaml_callback_creation() {
        let callback = YamlCallback::new();
        assert!(callback.config.use_color);
        assert_eq!(callback.config.indent_size, 2);
    }

    #[test]
    fn test_yaml_callback_without_colors() {
        let callback = YamlCallback::without_colors();
        assert!(!callback.config.use_color);
    }

    #[test]
    fn test_yaml_config_builder() {
        let config = YamlConfig::builder()
            .use_color(false)
            .indent_size(4)
            .show_duration(false)
            .show_result_data(true)
            .max_output_lines(100)
            .verbosity(2)
            .build();

        assert!(!config.use_color);
        assert_eq!(config.indent_size, 4);
        assert!(!config.show_duration);
        assert!(config.show_result_data);
        assert_eq!(config.max_output_lines, 100);
        assert_eq!(config.verbosity, 2);
    }

    #[test]
    fn test_yaml_escape() {
        let callback = YamlCallback::new();

        // Simple strings don't need escaping
        assert_eq!(callback.yaml_escape("hello"), "hello");

        // Strings with colons need quoting
        assert_eq!(callback.yaml_escape("key: value"), "\"key: value\"");

        // Strings with newlines need quoting
        assert_eq!(callback.yaml_escape("line1\nline2"), "\"line1\\nline2\"");

        // Reserved words need quoting
        assert_eq!(callback.yaml_escape("true"), "\"true\"");
        assert_eq!(callback.yaml_escape("false"), "\"false\"");
        assert_eq!(callback.yaml_escape("null"), "\"null\"");

        // Numbers need quoting
        assert_eq!(callback.yaml_escape("123"), "\"123\"");
        assert_eq!(callback.yaml_escape("3.14"), "\"3.14\"");
    }

    #[test]
    fn test_format_duration() {
        let callback = YamlCallback::new();

        // Microseconds
        assert_eq!(
            callback.format_duration(Duration::from_micros(500)),
            "500us"
        );

        // Milliseconds
        assert_eq!(
            callback.format_duration(Duration::from_millis(100)),
            "100ms"
        );

        // Seconds
        assert!(callback
            .format_duration(Duration::from_secs(5))
            .contains("5.00s"));

        // Minutes
        let dur = callback.format_duration(Duration::from_secs(125));
        assert!(dur.contains("2m") && dur.contains("5s"));

        // Hours
        let dur = callback.format_duration(Duration::from_secs(3700));
        assert!(dur.contains("1h") && dur.contains("1m"));
    }

    #[test]
    fn test_format_status() {
        let callback = YamlCallback::without_colors();

        assert_eq!(callback.format_status(true, false, false), "ok");
        assert_eq!(callback.format_status(true, true, false), "changed");
        assert_eq!(callback.format_status(false, false, false), "failed");
        assert_eq!(callback.format_status(true, false, true), "skipped");
    }

    #[test]
    fn test_truncate_output() {
        let config = YamlConfig {
            max_output_lines: 4,
            ..Default::default()
        };
        let callback = YamlCallback::with_config(config);

        let long_output = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8";
        let truncated = callback.truncate_output(long_output);

        assert!(truncated.contains("line1"));
        assert!(truncated.contains("line2"));
        assert!(truncated.contains("omitted"));
        assert!(truncated.contains("line7"));
        assert!(truncated.contains("line8"));
    }

    #[test]
    fn test_indentation() {
        let callback = YamlCallback::new();

        assert_eq!(callback.indent(), "");

        callback.push_indent();
        assert_eq!(callback.indent(), "  ");

        callback.push_indent();
        assert_eq!(callback.indent(), "    ");

        callback.pop_indent();
        assert_eq!(callback.indent(), "  ");

        callback.reset_indent();
        assert_eq!(callback.indent(), "");
    }

    #[test]
    fn test_multiline_format() {
        let callback = YamlCallback::new();

        // Single line should not use block format
        let single = callback.format_multiline("single line", "");
        assert!(!single.starts_with('|'));

        // Multi line should use block format
        let multi = callback.format_multiline("line1\nline2\nline3", "");
        assert!(multi.starts_with('|'));
        assert!(multi.contains("line1"));
        assert!(multi.contains("line2"));
        assert!(multi.contains("line3"));
    }

    #[tokio::test]
    async fn test_host_stats_tracking() {
        let callback = YamlCallback::without_colors();

        callback.on_playbook_start("test.yml").await;
        callback.on_play_start("test", &["host1".to_string()]).await;

        // Simulate task completions
        let ok_result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "task1".to_string(),
            result: ModuleResult::ok("Success"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&ok_result).await;

        let changed_result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "task2".to_string(),
            result: ModuleResult::changed("Changed"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&changed_result).await;

        let failed_result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "task3".to_string(),
            result: ModuleResult::failed("Failed"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&failed_result).await;

        // Check stats
        let stats = callback.host_stats.read();
        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.ok, 1);
        assert_eq!(host1_stats.changed, 1);
        assert_eq!(host1_stats.failed, 1);
    }

    #[test]
    fn test_default_trait() {
        let callback = YamlCallback::default();
        assert!(callback.config.use_color);
    }

    #[test]
    fn test_debug_trait() {
        let callback = YamlCallback::new();
        let debug_str = format!("{:?}", callback);
        assert!(debug_str.contains("YamlCallback"));
    }
}
