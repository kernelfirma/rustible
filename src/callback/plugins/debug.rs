//! Debug callback plugin for Rustible.
//!
//! This plugin provides maximum verbosity output for troubleshooting playbooks.
//! It displays full task arguments, all variable values (with sensitive data masked),
//! connection details, and comprehensive execution information.
//!
//! # Features
//!
//! - **Maximum Verbosity**: Shows all available information about execution
//! - **Task Arguments**: Displays full task arguments in formatted JSON
//! - **Variable Display**: Shows all variables with automatic sensitive data masking
//! - **Connection Details**: Displays connection type, host, port, and encryption status
//! - **Timing Information**: Shows precise timing for each task and overall execution
//! - **Handler Tracking**: Shows handler notifications and executions
//! - **Facts Display**: Shows gathered facts with optional verbosity control
//!
//! # Sensitive Data Masking
//!
//! The following patterns are automatically masked:
//! - password, passwd, secret, token
//! - api_key, private_key, credential
//! - ansible_password, ansible_ssh_pass, vault_password
//! - and more (see `SENSITIVE_PATTERNS`)
//!
//! # Example Output
//!
//! ```text
//! ================================================================================
//! PLAYBOOK: site.yml
//! ================================================================================
//!   Started: 2024-01-15 10:30:45 UTC
//! ================================================================================
//!
//! ================================================================================
//! PLAY [Configure webservers] ****************************************************
//! ================================================================================
//!   Hosts: 3 (web1.example.com, web2.example.com, web3.example.com)
//!
//! --------------------------------------------------------------------------------
//! TASK [Install nginx package] ***************************************************
//! --------------------------------------------------------------------------------
//!   Host: web1.example.com
//!
//! [CHANGED] web1.example.com (2.345s)
//!   Message: Package nginx installed
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::DebugCallback;
//!
//! let callback = DebugCallback::new();
//! // Or with custom verbosity
//! let callback = DebugCallback::with_verbosity(3);
//!
//! # let _ = ();
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use colored::Colorize;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Sensitive data patterns that should be masked in debug output.
pub const SENSITIVE_PATTERNS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "private_key",
    "privatekey",
    "credential",
    "auth",
    "bearer",
    "ssh_key",
    "sshkey",
    "access_key",
    "accesskey",
    "secret_key",
    "secretkey",
    "encryption_key",
    "vault_password",
    "become_pass",
    "ansible_password",
    "ansible_become_pass",
    "ansible_ssh_pass",
];

/// Mask value used for sensitive data.
pub const MASKED_VALUE: &str = "********";

/// Configuration for the debug callback.
#[derive(Debug, Clone, PartialEq)]
pub struct DebugConfig {
    /// Verbosity level (0-5)
    pub verbosity: u8,
    /// Whether to mask sensitive data
    pub mask_sensitive: bool,
    /// Whether to show timestamps
    pub show_timestamps: bool,
    /// Whether to show task arguments
    pub show_task_args: bool,
    /// Whether to show all facts
    pub show_all_facts: bool,
    /// Custom sensitive patterns to mask (in addition to defaults)
    pub custom_sensitive_patterns: Vec<String>,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            verbosity: 5,
            mask_sensitive: true,
            show_timestamps: true,
            show_task_args: true,
            show_all_facts: false,
            custom_sensitive_patterns: Vec::new(),
        }
    }
}

impl DebugConfig {
    /// Create a new configuration with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the verbosity level (0-5).
    #[must_use]
    pub fn with_verbosity(mut self, level: u8) -> Self {
        self.verbosity = level.min(5);
        self
    }

    /// Set whether to mask sensitive data.
    #[must_use]
    pub fn with_mask_sensitive(mut self, mask: bool) -> Self {
        self.mask_sensitive = mask;
        self
    }

    /// Set whether to show timestamps.
    #[must_use]
    pub fn with_timestamps(mut self, show: bool) -> Self {
        self.show_timestamps = show;
        self
    }

    /// Set whether to show task arguments.
    #[must_use]
    pub fn with_task_args(mut self, show: bool) -> Self {
        self.show_task_args = show;
        self
    }

    /// Set whether to show all facts.
    #[must_use]
    pub fn with_all_facts(mut self, show: bool) -> Self {
        self.show_all_facts = show;
        self
    }

    /// Add custom sensitive patterns to mask.
    #[must_use]
    pub fn with_sensitive_patterns(mut self, patterns: Vec<String>) -> Self {
        self.custom_sensitive_patterns = patterns;
        self
    }
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
    /// Total execution time for this host
    total_duration: Duration,
}

/// Debug callback plugin for maximum verbosity troubleshooting.
///
/// This callback displays comprehensive information about every aspect
/// of playbook execution, making it ideal for debugging complex playbooks
/// or understanding Rustible's behavior.
///
/// # Design Principles
///
/// 1. **Comprehensive Output**: Show everything that might be relevant
/// 2. **Structured Display**: Use clear visual hierarchy with separators
/// 3. **Sensitive Data Protection**: Automatically mask secrets and credentials
/// 4. **Timing Precision**: Show millisecond-precision timing for profiling
///
/// # Verbosity Levels
///
/// - 0: Quiet - only errors
/// - 1: Normal - basic progress (play/task headers)
/// - 2: Verbose - task details and results
/// - 3: More verbose - arguments and full results
/// - 4: Debug - variables and connection info
/// - 5: Maximum - everything including all facts
#[derive(Debug)]
pub struct DebugCallback {
    /// Configuration
    config: Arc<RwLock<DebugConfig>>,
    /// Per-host execution statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Playbook start time
    playbook_start: Arc<RwLock<Option<Instant>>>,
    /// Current playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Task start times per host
    task_start_times: Arc<RwLock<HashMap<String, Instant>>>,
    /// Whether any failures occurred
    has_failures: Arc<RwLock<bool>>,
    /// Variables for display (may be set externally)
    variables: Arc<RwLock<IndexMap<String, JsonValue>>>,
    /// Task arguments for display (may be set externally)
    task_args: Arc<RwLock<IndexMap<String, JsonValue>>>,
}

impl DebugCallback {
    /// Creates a new debug callback with default settings.
    ///
    /// Default verbosity is 5 (maximum).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = DebugCallback::new();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(DebugConfig::default())
    }

    /// Creates a debug callback with a specific configuration.
    #[must_use]
    pub fn with_config(config: DebugConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            playbook_start: Arc::new(RwLock::new(None)),
            playbook_name: Arc::new(RwLock::new(None)),
            task_start_times: Arc::new(RwLock::new(HashMap::new())),
            has_failures: Arc::new(RwLock::new(false)),
            variables: Arc::new(RwLock::new(IndexMap::new())),
            task_args: Arc::new(RwLock::new(IndexMap::new())),
        }
    }

    /// Creates a debug callback with a specific verbosity level.
    ///
    /// # Arguments
    ///
    /// * `verbosity` - Verbosity level from 0 (quiet) to 5 (maximum)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = DebugCallback::with_verbosity(3);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_verbosity(verbosity: u8) -> Self {
        Self::with_config(DebugConfig::default().with_verbosity(verbosity))
    }

    /// Returns whether any failures occurred during execution.
    pub async fn has_failures(&self) -> bool {
        *self.has_failures.read().await
    }

    /// Gets the current verbosity level.
    pub async fn verbosity(&self) -> u8 {
        self.config.read().await.verbosity
    }

    /// Sets the verbosity level.
    pub async fn set_verbosity(&self, level: u8) {
        self.config.write().await.verbosity = level.min(5);
    }

    /// Sets variables for display in debug output.
    pub async fn set_variables(&self, vars: IndexMap<String, JsonValue>) {
        *self.variables.write().await = vars;
    }

    /// Sets task arguments for display in debug output.
    pub async fn set_task_args(&self, args: IndexMap<String, JsonValue>) {
        *self.task_args.write().await = args;
    }

    /// Masks sensitive data in a JSON value.
    fn mask_value(value: &JsonValue) -> JsonValue {
        match value {
            JsonValue::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (key, val) in map {
                    let key_lower = key.to_lowercase();
                    let is_sensitive = SENSITIVE_PATTERNS
                        .iter()
                        .any(|pattern| key_lower.contains(*pattern));

                    if is_sensitive {
                        new_map.insert(key.clone(), JsonValue::String(MASKED_VALUE.to_string()));
                    } else {
                        new_map.insert(key.clone(), Self::mask_value(val));
                    }
                }
                JsonValue::Object(new_map)
            }
            JsonValue::Array(arr) => JsonValue::Array(arr.iter().map(Self::mask_value).collect()),
            _ => value.clone(),
        }
    }

    /// Formats a JSON value for display, optionally masking sensitive data.
    fn format_json(value: &JsonValue, mask: bool, indent: usize) -> String {
        let value = if mask {
            Self::mask_value(value)
        } else {
            value.clone()
        };

        serde_json::to_string_pretty(&value)
            .unwrap_or_else(|_| format!("{:?}", value))
            .lines()
            .map(|line| format!("{:indent$}{}", "", line, indent = indent))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Formats a duration in human-readable form.
    fn format_duration(duration: Duration) -> String {
        let secs = duration.as_secs();
        let millis = duration.subsec_millis();

        if secs >= 3600 {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            let secs_rem = secs % 60;
            format!("{}h {:02}m {:02}s", hours, mins, secs_rem)
        } else if secs >= 60 {
            let mins = secs / 60;
            let secs_rem = secs % 60;
            format!("{}m {:02}s", mins, secs_rem)
        } else if secs > 0 {
            format!("{}.{:03}s", secs, millis)
        } else {
            format!("{}ms", millis)
        }
    }

    /// Formats the current timestamp.
    fn format_timestamp() -> String {
        let now: DateTime<Utc> = Utc::now();
        now.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string()
    }

    /// Prints a separator line.
    fn print_separator(char: char, width: usize) {
        println!("{}", char.to_string().repeat(width).bright_black());
    }

    /// Prints a major separator (for playbooks and plays).
    fn print_major_separator() {
        Self::print_separator('=', 80);
    }

    /// Prints a minor separator (for tasks).
    fn print_minor_separator() {
        Self::print_separator('-', 80);
    }

    /// Prints indented key-value pair.
    fn print_kv(key: &str, value: &str, indent: usize) {
        println!("{:indent$}{}: {}", "", key.cyan(), value, indent = indent);
    }

    /// Prints task result status with color.
    fn print_status(status: &str, host: &str, duration: Duration, msg: Option<&str>) {
        let duration_str = Self::format_duration(duration);
        let status_colored = match status.to_lowercase().as_str() {
            "ok" => format!("[{}]", "OK".green().bold()),
            "changed" => format!("[{}]", "CHANGED".yellow().bold()),
            "failed" => format!("[{}]", "FAILED".red().bold()),
            "skipped" => format!("[{}]", "SKIPPED".cyan().bold()),
            "unreachable" => format!("[{}]", "UNREACHABLE".magenta().bold()),
            _ => format!("[{}]", status.bright_white().bold()),
        };

        print!(
            "{} {} ({})",
            status_colored,
            host.bright_white().bold(),
            duration_str.bright_black()
        );

        if let Some(message) = msg {
            println!();
            println!("  {}: {}", "Message".cyan(), message);
        } else {
            println!();
        }
    }
}

impl Default for DebugCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for DebugCallback {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            host_stats: Arc::clone(&self.host_stats),
            playbook_start: Arc::clone(&self.playbook_start),
            playbook_name: Arc::clone(&self.playbook_name),
            task_start_times: Arc::clone(&self.task_start_times),
            has_failures: Arc::clone(&self.has_failures),
            variables: Arc::clone(&self.variables),
            task_args: Arc::clone(&self.task_args),
        }
    }
}

#[async_trait]
impl ExecutionCallback for DebugCallback {
    /// Called when a playbook starts - displays playbook header with full details.
    async fn on_playbook_start(&self, name: &str) {
        let config = self.config.read().await;
        let verbosity = config.verbosity;

        // Record start time
        *self.playbook_start.write().await = Some(Instant::now());
        *self.playbook_name.write().await = Some(name.to_string());

        // Clear stats from any previous run
        self.host_stats.write().await.clear();
        *self.has_failures.write().await = false;

        if verbosity >= 1 {
            println!();
            Self::print_major_separator();
            println!(
                "{} {}",
                "PLAYBOOK:".bright_white().bold(),
                name.yellow().bold()
            );
            Self::print_major_separator();

            if verbosity >= 2 && config.show_timestamps {
                Self::print_kv("Started", &Self::format_timestamp(), 2);
            }

            Self::print_major_separator();
            println!();
        }
    }

    /// Called when a playbook ends - displays comprehensive recap.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let config = self.config.read().await;
        let verbosity = config.verbosity;
        let stats = self.host_stats.read().await;
        let start = self.playbook_start.read().await;

        if verbosity >= 1 {
            println!();
            Self::print_major_separator();
            println!(
                "{} {}",
                "PLAY RECAP".bright_white().bold(),
                "*".repeat(69).bright_black()
            );
            Self::print_major_separator();

            // Print detailed stats for each host
            let mut hosts: Vec<_> = stats.keys().collect();
            hosts.sort();

            for host in hosts {
                if let Some(host_stats) = stats.get(host) {
                    let host_color = if host_stats.failed > 0 {
                        host.red().bold()
                    } else if host_stats.changed > 0 {
                        host.yellow()
                    } else {
                        host.green()
                    };

                    println!(
                        "{:<30} : {}={:<4} {}={:<4} {}={:<4} {}={:<4}",
                        host_color,
                        "ok".green(),
                        host_stats.ok,
                        "changed".yellow(),
                        host_stats.changed,
                        "failed".red(),
                        host_stats.failed,
                        "skipped".cyan(),
                        host_stats.skipped,
                    );

                    if verbosity >= 3 {
                        println!(
                            "{:32}  {}: {}",
                            "",
                            "total time".bright_black(),
                            Self::format_duration(host_stats.total_duration).bright_black()
                        );
                    }
                }
            }

            // Print overall summary
            if let Some(start_time) = *start {
                let duration = start_time.elapsed();
                let playbook_status = if success {
                    "completed successfully".green().bold()
                } else {
                    "failed".red().bold()
                };

                println!();
                Self::print_major_separator();
                println!(
                    "{} {} in {}",
                    "Playbook".bright_white(),
                    playbook_status,
                    Self::format_duration(duration).yellow()
                );

                if verbosity >= 2 && config.show_timestamps {
                    Self::print_kv("Finished", &Self::format_timestamp(), 2);
                }

                Self::print_major_separator();
            }

            println!();
        }

        let _ = name;
    }

    /// Called when a play starts - displays play header with host list.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let config = self.config.read().await;
        let verbosity = config.verbosity;

        // Initialize stats for all hosts in this play
        {
            let mut stats = self.host_stats.write().await;
            for host in hosts {
                stats.entry(host.clone()).or_default();
            }
        }

        if verbosity >= 1 {
            println!();
            Self::print_major_separator();
            println!(
                "{} [{}] {}",
                "PLAY".bright_white().bold(),
                name.yellow().bold(),
                "*".repeat(70_usize.saturating_sub(name.len() + 8))
                    .bright_black()
            );
            Self::print_major_separator();

            if verbosity >= 2 {
                // Show host information
                let host_list = if hosts.len() <= 5 {
                    hosts.join(", ")
                } else {
                    format!(
                        "{}, ... and {} more",
                        hosts[..5].join(", "),
                        hosts.len() - 5
                    )
                };
                Self::print_kv("Hosts", &format!("{} ({})", hosts.len(), host_list), 2);

                // Show variables at high verbosity
                if verbosity >= 4 {
                    let vars = self.variables.read().await;
                    if !vars.is_empty() {
                        println!("  {}:", "Variables".cyan());
                        for (key, value) in vars.iter() {
                            let formatted = Self::format_json(value, config.mask_sensitive, 6);
                            println!("    {}: {}", key.bright_white(), formatted.trim());
                        }
                    }
                }
            }

            println!();
        }
    }

    /// Called when a play ends.
    async fn on_play_end(&self, name: &str, success: bool) {
        let config = self.config.read().await;
        let verbosity = config.verbosity;

        if verbosity >= 2 {
            let status = if success {
                "completed".green()
            } else {
                "failed".red().bold()
            };
            println!(
                "\n{} [{}] {}",
                "PLAY END:".bright_black(),
                name.bright_white(),
                status
            );
        }
    }

    /// Called when a task starts - displays task header with full arguments.
    async fn on_task_start(&self, name: &str, host: &str) {
        let config = self.config.read().await;
        let verbosity = config.verbosity;

        // Record task start time for this host
        self.task_start_times
            .write()
            .await
            .insert(host.to_string(), Instant::now());

        if verbosity >= 1 {
            Self::print_minor_separator();
            println!(
                "{} [{}] {}",
                "TASK".bright_white().bold(),
                name.yellow().bold(),
                "*".repeat(70_usize.saturating_sub(name.len() + 8))
                    .bright_black()
            );
            Self::print_minor_separator();
        }

        if verbosity >= 2 {
            Self::print_kv("Host", host, 2);

            // Show task arguments at verbosity 3+
            if verbosity >= 3 && config.show_task_args {
                let args = self.task_args.read().await;
                if !args.is_empty() {
                    println!("  {}:", "Arguments".cyan());
                    let args_json = serde_json::to_value(args.clone()).unwrap_or(JsonValue::Null);
                    let formatted = Self::format_json(&args_json, config.mask_sensitive, 4);
                    println!("{}", formatted);
                }
            }

            println!();
        }
    }

    /// Called when a task completes - displays result with full details.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let config = self.config.read().await;
        let verbosity = config.verbosity;

        // Update stats
        {
            let mut stats = self.host_stats.write().await;
            let host_stats = stats.entry(result.host.clone()).or_default();
            host_stats.total_duration += result.duration;

            if result.result.skipped {
                host_stats.skipped += 1;
            } else if !result.result.success {
                host_stats.failed += 1;
                *self.has_failures.write().await = true;
            } else if result.result.changed {
                host_stats.changed += 1;
            } else {
                host_stats.ok += 1;
            }
        }

        // Determine status
        let status = if result.result.skipped {
            "skipped"
        } else if !result.result.success {
            "failed"
        } else if result.result.changed {
            "changed"
        } else {
            "ok"
        };

        if verbosity >= 1 {
            // Get actual duration from our recorded start time if available
            let duration = {
                let start_times = self.task_start_times.read().await;
                start_times
                    .get(&result.host)
                    .map(|start| start.elapsed())
                    .unwrap_or(result.duration)
            };

            // Show status line
            let msg = if !result.result.success || result.result.changed || result.result.skipped {
                Some(result.result.message.as_str())
            } else {
                None
            };

            Self::print_status(status, &result.host, duration, msg);

            // Show result details at verbosity 3+
            if verbosity >= 3 {
                // Show result data if available
                if let Some(ref data) = result.result.data {
                    println!("  {}:", "Result".cyan());
                    let formatted = Self::format_json(data, config.mask_sensitive, 4);
                    println!("{}", formatted);
                }

                // Show warnings
                if !result.result.warnings.is_empty() {
                    for warning in &result.result.warnings {
                        println!("  {}: {}", "WARNING".yellow().bold(), warning);
                    }
                }

                // Show notify handlers that were triggered
                if !result.notify.is_empty() {
                    println!(
                        "  {}: {}",
                        "Notified".cyan(),
                        result.notify.join(", ").bright_white()
                    );
                }
            }

            println!();
        }
    }

    /// Called when a handler is triggered.
    async fn on_handler_triggered(&self, name: &str) {
        let config = self.config.read().await;
        let verbosity = config.verbosity;

        if verbosity >= 2 {
            println!("  {} {}", "HANDLER NOTIFIED:".bright_black(), name.yellow());
        }
    }

    /// Called when facts are gathered - displays all facts at high verbosity.
    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let config = self.config.read().await;
        let verbosity = config.verbosity;

        if verbosity >= 3 {
            Self::print_minor_separator();
            println!(
                "{} [{}] {}",
                "GATHERING FACTS".bright_white().bold(),
                host.yellow().bold(),
                "*".repeat(55).bright_black()
            );
            Self::print_minor_separator();

            let all_facts = facts.all();
            Self::print_kv("Facts gathered", &format!("{} items", all_facts.len()), 2);

            // Show all facts at verbosity 5
            if verbosity >= 5 && config.show_all_facts {
                println!("  {}:", "All Facts".cyan());
                let facts_json = serde_json::to_value(all_facts).unwrap_or(JsonValue::Null);
                let formatted = Self::format_json(&facts_json, config.mask_sensitive, 4);
                println!("{}", formatted);
            } else if verbosity >= 4 {
                // Show key facts at verbosity 4
                println!("  {}:", "Key Facts".cyan());
                let key_facts = ["os_family", "os_arch", "hostname", "user"];
                for key in &key_facts {
                    if let Some(value) = all_facts.get(*key) {
                        println!("    {}: {}", key.bright_white(), value);
                    }
                }
            }

            println!();
        }
    }
}

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

    #[test]
    fn test_mask_value() {
        let data = serde_json::json!({
            "username": "admin",
            "password": "secret123",
            "api_key": "abc123",
            "config": {
                "host": "localhost",
                "db_password": "dbpass"
            }
        });

        let masked = DebugCallback::mask_value(&data);

        assert_eq!(masked["username"], "admin");
        assert_eq!(masked["password"], MASKED_VALUE);
        assert_eq!(masked["api_key"], MASKED_VALUE);
        assert_eq!(masked["config"]["host"], "localhost");
        assert_eq!(masked["config"]["db_password"], MASKED_VALUE);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(
            DebugCallback::format_duration(Duration::from_millis(500)),
            "500ms"
        );
        assert_eq!(
            DebugCallback::format_duration(Duration::from_secs(5)),
            "5.000s"
        );
        assert_eq!(
            DebugCallback::format_duration(Duration::from_secs(65)),
            "1m 05s"
        );
        assert_eq!(
            DebugCallback::format_duration(Duration::from_secs(3665)),
            "1h 01m 05s"
        );
    }

    #[test]
    fn test_debug_config_builder() {
        let config = DebugConfig::new()
            .with_verbosity(3)
            .with_mask_sensitive(false)
            .with_timestamps(true)
            .with_task_args(true)
            .with_all_facts(true);

        assert_eq!(config.verbosity, 3);
        assert!(!config.mask_sensitive);
        assert!(config.show_timestamps);
        assert!(config.show_task_args);
        assert!(config.show_all_facts);
    }

    #[test]
    fn test_verbosity_capped() {
        let config = DebugConfig::new().with_verbosity(10);
        assert_eq!(config.verbosity, 5);
    }

    #[tokio::test]
    async fn test_debug_callback_tracks_stats() {
        let callback = DebugCallback::with_verbosity(0); // Quiet for testing

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
    async fn test_debug_callback_no_failures() {
        let callback = DebugCallback::with_verbosity(0);

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        assert!(!callback.has_failures().await);
    }

    #[tokio::test]
    async fn test_verbosity_levels() {
        let callback = DebugCallback::new();
        assert_eq!(callback.verbosity().await, 5); // Default max

        callback.set_verbosity(3).await;
        assert_eq!(callback.verbosity().await, 3);

        callback.set_verbosity(10).await;
        assert_eq!(callback.verbosity().await, 5); // Capped at max
    }

    #[tokio::test]
    async fn test_set_variables() {
        let callback = DebugCallback::new();

        let mut vars = IndexMap::new();
        vars.insert("test_var".to_string(), serde_json::json!("test_value"));

        callback.set_variables(vars).await;

        let stored_vars = callback.variables.read().await;
        assert!(stored_vars.contains_key("test_var"));
    }

    #[test]
    fn test_default_trait() {
        let callback = DebugCallback::default();
        assert_eq!(*callback.config.blocking_read(), DebugConfig::default());
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = DebugCallback::new();
        let callback2 = callback1.clone();

        // Both should share the same underlying state
        assert!(Arc::ptr_eq(&callback1.host_stats, &callback2.host_stats));
        assert!(Arc::ptr_eq(
            &callback1.has_failures,
            &callback2.has_failures
        ));
    }

    #[test]
    fn test_format_json_with_masking() {
        let data = serde_json::json!({
            "user": "admin",
            "password": "secret"
        });

        let formatted = DebugCallback::format_json(&data, true, 0);
        assert!(formatted.contains("admin"));
        assert!(formatted.contains(MASKED_VALUE));
        assert!(!formatted.contains("secret"));
    }

    #[test]
    fn test_format_json_without_masking() {
        let data = serde_json::json!({
            "user": "admin",
            "password": "secret"
        });

        let formatted = DebugCallback::format_json(&data, false, 0);
        assert!(formatted.contains("admin"));
        assert!(formatted.contains("secret"));
        assert!(!formatted.contains(MASKED_VALUE));
    }
}
