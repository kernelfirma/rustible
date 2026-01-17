//! Context callback plugin for Rustible.
//!
//! This plugin displays variable context during task execution, including:
//! - Task-relevant variables
//! - Gathered facts
//! - Registered variables from previous tasks
//! - Sensitive data masking for passwords, keys, and tokens
//!
//! Useful for debugging variable resolution issues and understanding
//! what data is available to each task.
//!
//! # Features
//!
//! - Shows variables relevant to current task
//! - Displays gathered facts in organized format
//! - Shows registered variables from previous tasks
//! - Automatically masks sensitive data (passwords, API keys, tokens, secrets)
//! - Configurable verbosity levels
//! - Color-coded output for easy reading
//!
//! # Example Output
//!
//! ```text
//! TASK [Install nginx] ********************************************************
//! HOST: webserver1
//!
//! --- CONTEXT ---
//! Variables:
//!   http_port: 80
//!   admin_password: ********
//!   api_key: ********
//!
//! Facts:
//!   ansible_os_family: "Debian"
//!   ansible_distribution: "Ubuntu"
//!   ansible_distribution_version: "22.04"
//!
//! Registered:
//!   apt_update:
//!     changed: true
//!     rc: 0
//!     stdout: "Reading package lists..."
//! ---------------
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use colored::Colorize;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Patterns that indicate sensitive data that should be masked.
/// These are checked case-insensitively against variable names.
const SENSITIVE_PATTERNS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "auth_key",
    "authkey",
    "private_key",
    "privatekey",
    "access_key",
    "accesskey",
    "secret_key",
    "secretkey",
    "credential",
    "ssh_key",
    "sshkey",
    "cert",
    "certificate",
    "bearer",
    "authorization",
    "auth_token",
    "authtoken",
    "refresh_token",
    "access_token",
    "client_secret",
    "encryption_key",
    "decryption_key",
    "vault_password",
    "become_password",
    "become_pass",
    "ansible_password",
    "ansible_become_password",
    "ansible_ssh_pass",
    "mysql_password",
    "postgres_password",
    "db_password",
    "database_password",
    "redis_password",
    "aws_secret",
    "azure_secret",
    "gcp_secret",
];

/// Mask used to replace sensitive values.
const MASK: &str = "********";

/// Verbosity levels for context output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ContextVerbosity {
    /// Show only essential variables used in task args
    Minimal,
    /// Show task variables and relevant facts (default)
    Normal,
    /// Show all variables, facts, and registered vars
    Verbose,
    /// Show everything including internal magic variables
    Debug,
}

impl Default for ContextVerbosity {
    fn default() -> Self {
        Self::Normal
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
}

/// Configuration for the context callback.
#[derive(Debug, Clone)]
pub struct ContextCallbackConfig {
    /// Verbosity level for context output
    pub verbosity: ContextVerbosity,
    /// Whether to show facts
    pub show_facts: bool,
    /// Whether to show registered variables
    pub show_registered: bool,
    /// Whether to mask sensitive data
    pub mask_sensitive: bool,
    /// Additional patterns to consider sensitive
    pub additional_sensitive_patterns: Vec<String>,
    /// Maximum depth for nested structures
    pub max_depth: usize,
    /// Maximum string length before truncation
    pub max_string_length: usize,
    /// Whether to show empty values
    pub show_empty: bool,
}

impl Default for ContextCallbackConfig {
    fn default() -> Self {
        Self {
            verbosity: ContextVerbosity::Normal,
            show_facts: true,
            show_registered: true,
            mask_sensitive: true,
            additional_sensitive_patterns: Vec::new(),
            max_depth: 4,
            max_string_length: 200,
            show_empty: false,
        }
    }
}

/// Context data captured for each task execution.
#[derive(Debug, Clone, Default)]
struct TaskContext {
    /// Variables available to the task
    variables: IndexMap<String, JsonValue>,
    /// Facts gathered from the host
    facts: IndexMap<String, JsonValue>,
    /// Registered variables from previous tasks
    registered: IndexMap<String, JsonValue>,
}

/// Context callback plugin that displays variable context during execution.
///
/// This callback is designed for debugging variable resolution issues by
/// showing what variables, facts, and registered results are available
/// to each task during execution.
///
/// # Design Principles
///
/// 1. **Contextual Awareness**: Show relevant data for current task
/// 2. **Security First**: Always mask sensitive data by default
/// 3. **Configurable Verbosity**: From minimal to debug output
/// 4. **Organized Output**: Group by category for readability
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::ContextCallback;
///
/// let callback = ContextCallback::new();
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ContextCallback {
    /// Configuration for the callback
    config: ContextCallbackConfig,
    /// Per-host execution statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Per-host context data
    host_contexts: Arc<RwLock<HashMap<String, TaskContext>>>,
    /// Playbook start time for duration tracking
    start_time: Arc<RwLock<Option<Instant>>>,
    /// Current playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Current task name
    current_task: Arc<RwLock<Option<String>>>,
    /// Compiled sensitive patterns (lowercase)
    sensitive_patterns: HashSet<String>,
}

impl ContextCallback {
    /// Creates a new context callback plugin with default configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = ContextCallback::new();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(ContextCallbackConfig::default())
    }

    /// Creates a new context callback plugin with custom configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let config = ContextCallbackConfig {
    ///     verbosity: ContextVerbosity::Verbose,
    ///     show_facts: true,
    ///     ..Default::default()
    /// };
    /// let callback = ContextCallback::with_config(config);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_config(config: ContextCallbackConfig) -> Self {
        let mut sensitive_patterns: HashSet<String> = SENSITIVE_PATTERNS
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        // Add custom sensitive patterns
        for pattern in &config.additional_sensitive_patterns {
            sensitive_patterns.insert(pattern.to_lowercase());
        }

        Self {
            config,
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            host_contexts: Arc::new(RwLock::new(HashMap::new())),
            start_time: Arc::new(RwLock::new(None)),
            playbook_name: Arc::new(RwLock::new(None)),
            current_task: Arc::new(RwLock::new(None)),
            sensitive_patterns,
        }
    }

    /// Creates a minimal verbosity callback.
    #[must_use]
    pub fn minimal() -> Self {
        Self::with_config(ContextCallbackConfig {
            verbosity: ContextVerbosity::Minimal,
            show_facts: false,
            show_registered: false,
            ..Default::default()
        })
    }

    /// Creates a verbose callback for detailed debugging.
    #[must_use]
    pub fn verbose() -> Self {
        Self::with_config(ContextCallbackConfig {
            verbosity: ContextVerbosity::Verbose,
            show_facts: true,
            show_registered: true,
            show_empty: true,
            ..Default::default()
        })
    }

    /// Creates a debug callback that shows everything.
    #[must_use]
    pub fn debug() -> Self {
        Self::with_config(ContextCallbackConfig {
            verbosity: ContextVerbosity::Debug,
            show_facts: true,
            show_registered: true,
            show_empty: true,
            max_depth: 8,
            max_string_length: 500,
            ..Default::default()
        })
    }

    /// Check if a variable name indicates sensitive data.
    fn is_sensitive(&self, name: &str) -> bool {
        if !self.config.mask_sensitive {
            return false;
        }

        let lower = name.to_lowercase();
        self.sensitive_patterns
            .iter()
            .any(|pattern| lower.contains(pattern))
    }

    /// Mask a value if it's sensitive, otherwise return formatted value.
    fn mask_value(&self, name: &str, value: &JsonValue) -> String {
        if self.is_sensitive(name) {
            MASK.to_string()
        } else {
            self.format_value(value, 0)
        }
    }

    /// Format a JSON value for display with depth limiting.
    fn format_value(&self, value: &JsonValue, depth: usize) -> String {
        if depth > self.config.max_depth {
            return "...".to_string();
        }

        match value {
            JsonValue::Null => "null".to_string(),
            JsonValue::Bool(b) => b.to_string(),
            JsonValue::Number(n) => n.to_string(),
            JsonValue::String(s) => {
                if s.len() > self.config.max_string_length {
                    format!(
                        "\"{}...\" ({} chars)",
                        &s[..self.config.max_string_length],
                        s.len()
                    )
                } else {
                    format!("\"{}\"", s)
                }
            }
            JsonValue::Array(arr) => {
                if arr.is_empty() {
                    "[]".to_string()
                } else if arr.len() > 5 && depth > 0 {
                    format!("[{} items]", arr.len())
                } else {
                    let items: Vec<String> = arr
                        .iter()
                        .take(10)
                        .map(|v| self.format_value(v, depth + 1))
                        .collect();
                    if arr.len() > 10 {
                        format!("[{}, ... +{} more]", items.join(", "), arr.len() - 10)
                    } else {
                        format!("[{}]", items.join(", "))
                    }
                }
            }
            JsonValue::Object(obj) => {
                if obj.is_empty() {
                    "{}".to_string()
                } else if obj.len() > 5 && depth > 0 {
                    format!("{{{} keys}}", obj.len())
                } else {
                    let items: Vec<String> = obj
                        .iter()
                        .take(10)
                        .map(|(k, v)| {
                            let formatted_value = if self.is_sensitive(k) {
                                MASK.to_string()
                            } else {
                                self.format_value(v, depth + 1)
                            };
                            format!("{}: {}", k, formatted_value)
                        })
                        .collect();
                    if obj.len() > 10 {
                        format!("{{{}}} +{} more", items.join(", "), obj.len() - 10)
                    } else {
                        format!("{{{}}}", items.join(", "))
                    }
                }
            }
        }
    }

    /// Print the context header.
    fn print_context_header(&self) {
        println!("\n{}", "--- CONTEXT ---".bright_blue().bold());
    }

    /// Print the context footer.
    fn print_context_footer(&self) {
        println!("{}\n", "---------------".bright_blue());
    }

    /// Print a section header.
    fn print_section(&self, title: &str) {
        println!("{}:", title.yellow().bold());
    }

    /// Print a variable entry.
    fn print_var(&self, name: &str, value: &str, indent: usize) {
        let spaces = "  ".repeat(indent);
        println!("{}{}: {}", spaces, name.cyan(), value);
    }

    /// Print variables section.
    fn print_variables(&self, vars: &IndexMap<String, JsonValue>) {
        if vars.is_empty() && !self.config.show_empty {
            return;
        }

        self.print_section("Variables");

        if vars.is_empty() {
            println!("  {}", "(none)".bright_black());
            return;
        }

        // Sort variables alphabetically for consistent output
        let mut sorted: Vec<_> = vars.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));

        for (name, value) in sorted {
            // Skip internal/magic variables in normal mode
            if self.config.verbosity < ContextVerbosity::Debug && name.starts_with("ansible_") {
                continue;
            }
            if self.config.verbosity < ContextVerbosity::Verbose && name.starts_with('_') {
                continue;
            }

            let formatted = self.mask_value(name, value);

            // Skip empty values unless configured to show them
            if !self.config.show_empty
                && (value.is_null() || formatted == "null" || formatted == "\"\"")
            {
                continue;
            }

            self.print_var(name, &formatted, 1);
        }
    }

    /// Print facts section.
    fn print_facts(&self, facts: &IndexMap<String, JsonValue>) {
        if !self.config.show_facts {
            return;
        }

        if facts.is_empty() && !self.config.show_empty {
            return;
        }

        println!();
        self.print_section("Facts");

        if facts.is_empty() {
            println!("  {}", "(none gathered)".bright_black());
            return;
        }

        // Group facts by category
        let mut system_facts = IndexMap::new();
        let mut network_facts = IndexMap::new();
        let mut other_facts = IndexMap::new();

        for (name, value) in facts {
            if name.starts_with("ansible_distribution")
                || name.starts_with("ansible_os")
                || name.starts_with("ansible_kernel")
                || name.starts_with("ansible_machine")
                || name.starts_with("ansible_architecture")
                || name == "ansible_hostname"
                || name == "ansible_fqdn"
            {
                system_facts.insert(name.clone(), value.clone());
            } else if name.starts_with("ansible_default_ipv")
                || name.starts_with("ansible_interfaces")
                || name.contains("_ip")
                || name.contains("_mac")
            {
                network_facts.insert(name.clone(), value.clone());
            } else {
                other_facts.insert(name.clone(), value.clone());
            }
        }

        // Print system facts first
        if !system_facts.is_empty() {
            println!("  {}:", "System".bright_black());
            for (name, value) in &system_facts {
                let short_name = name.strip_prefix("ansible_").unwrap_or(name);
                self.print_var(short_name, &self.format_value(value, 0), 2);
            }
        }

        // Print network facts in verbose mode
        if self.config.verbosity >= ContextVerbosity::Verbose && !network_facts.is_empty() {
            println!("  {}:", "Network".bright_black());
            for (name, value) in &network_facts {
                let short_name = name.strip_prefix("ansible_").unwrap_or(name);
                self.print_var(short_name, &self.format_value(value, 0), 2);
            }
        }

        // Print other facts in debug mode
        if self.config.verbosity >= ContextVerbosity::Debug && !other_facts.is_empty() {
            println!("  {}:", "Other".bright_black());
            for (name, value) in other_facts.iter().take(20) {
                let short_name = name.strip_prefix("ansible_").unwrap_or(name);
                self.print_var(short_name, &self.format_value(value, 0), 2);
            }
            if other_facts.len() > 20 {
                println!(
                    "    {} +{} more facts",
                    "...".bright_black(),
                    other_facts.len() - 20
                );
            }
        }
    }

    /// Print registered variables section.
    fn print_registered(&self, registered: &IndexMap<String, JsonValue>) {
        if !self.config.show_registered {
            return;
        }

        if registered.is_empty() && !self.config.show_empty {
            return;
        }

        println!();
        self.print_section("Registered");

        if registered.is_empty() {
            println!("  {}", "(none)".bright_black());
            return;
        }

        for (name, value) in registered {
            println!("  {}:", name.cyan().bold());

            // Extract common fields from registered results
            if let JsonValue::Object(obj) = value {
                if let Some(changed) = obj.get("changed") {
                    let changed_str = if changed.as_bool().unwrap_or(false) {
                        "true".yellow().to_string()
                    } else {
                        "false".to_string()
                    };
                    println!("    {}: {}", "changed".bright_black(), changed_str);
                }
                if let Some(failed) = obj.get("failed") {
                    if failed.as_bool().unwrap_or(false) {
                        println!("    {}: {}", "failed".bright_black(), "true".red());
                    }
                }
                if let Some(skipped) = obj.get("skipped") {
                    if skipped.as_bool().unwrap_or(false) {
                        println!("    {}: {}", "skipped".bright_black(), "true".cyan());
                    }
                }
                if let Some(rc) = obj.get("rc") {
                    println!("    {}: {}", "rc".bright_black(), self.format_value(rc, 0));
                }
                if let Some(stdout) = obj.get("stdout") {
                    if let JsonValue::String(s) = stdout {
                        if !s.is_empty() {
                            let truncated = if s.len() > 100 {
                                format!("{}...", &s[..100])
                            } else {
                                s.clone()
                            };
                            println!("    {}: \"{}\"", "stdout".bright_black(), truncated);
                        }
                    }
                }
                if let Some(msg) = obj.get("msg") {
                    println!(
                        "    {}: {}",
                        "msg".bright_black(),
                        self.format_value(msg, 0)
                    );
                }

                // Show other fields in verbose mode
                if self.config.verbosity >= ContextVerbosity::Verbose {
                    for (k, v) in obj {
                        if ![
                            "changed",
                            "failed",
                            "skipped",
                            "rc",
                            "stdout",
                            "stderr",
                            "msg",
                            "stdout_lines",
                            "stderr_lines",
                        ]
                        .contains(&k.as_str())
                        {
                            let formatted = self.mask_value(k, v);
                            println!("    {}: {}", k.bright_black(), formatted);
                        }
                    }
                }
            } else {
                println!("    {}", self.format_value(value, 1));
            }
        }
    }

    /// Format the task header line.
    fn format_task_header(task_name: &str) -> String {
        let padding = 70_usize.saturating_sub(task_name.len() + 8);
        format!(
            "{} [{}] {}",
            "TASK".green().bold(),
            task_name.bright_white().bold(),
            "*".repeat(padding)
        )
    }

    /// Format host line.
    fn format_host(host: &str) -> String {
        format!("{}: {}", "HOST".bright_black(), host.bright_white())
    }

    /// Formats a single host's recap line.
    fn format_recap_line(host: &str, stats: &HostStats) -> String {
        let host_color = if stats.failed > 0 {
            host.red().bold()
        } else if stats.changed > 0 {
            host.yellow()
        } else {
            host.green()
        };

        format!(
            "{}: {} ok={} changed={} failed={} skipped={}",
            "RECAP".bright_black(),
            host_color,
            stats.ok.to_string().green(),
            stats.changed.to_string().yellow(),
            stats.failed.to_string().red(),
            stats.skipped.to_string().cyan(),
        )
    }
}

impl Default for ContextCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ContextCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            host_stats: Arc::clone(&self.host_stats),
            host_contexts: Arc::clone(&self.host_contexts),
            start_time: Arc::clone(&self.start_time),
            playbook_name: Arc::clone(&self.playbook_name),
            current_task: Arc::clone(&self.current_task),
            sensitive_patterns: self.sensitive_patterns.clone(),
        }
    }
}

#[async_trait]
impl ExecutionCallback for ContextCallback {
    /// Called when a playbook starts - records start time.
    async fn on_playbook_start(&self, name: &str) {
        let mut start_time = self.start_time.write().await;
        *start_time = Some(Instant::now());

        let mut playbook_name = self.playbook_name.write().await;
        *playbook_name = Some(name.to_string());

        // Clear stats from any previous run
        let mut stats = self.host_stats.write().await;
        stats.clear();

        let mut contexts = self.host_contexts.write().await;
        contexts.clear();

        println!(
            "\n{} [{}]",
            "PLAYBOOK".green().bold(),
            name.bright_white().bold()
        );
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
            let status = if success {
                "completed".green()
            } else {
                "failed".red().bold()
            };

            println!(
                "\n{} {} in {:.2}s",
                name.bright_white().bold(),
                status,
                duration.as_secs_f64()
            );
        }
    }

    /// Called when a play starts - initializes host tracking.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        println!(
            "\n{} [{}] {}",
            "PLAY".cyan().bold(),
            name.bright_white().bold(),
            format!("({} hosts)", hosts.len()).bright_black()
        );

        // Initialize stats and context for all hosts in this play
        let mut stats = self.host_stats.write().await;
        let mut contexts = self.host_contexts.write().await;

        for host in hosts {
            stats.entry(host.clone()).or_default();
            contexts.entry(host.clone()).or_default();
        }
    }

    /// Called when a play ends.
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Nothing special needed here
    }

    /// Called when a task starts - shows the context if verbosity allows.
    async fn on_task_start(&self, name: &str, host: &str) {
        let mut current_task = self.current_task.write().await;
        *current_task = Some(name.to_string());

        println!("\n{}", Self::format_task_header(name));
        println!("{}", Self::format_host(host));

        // Show context based on verbosity
        if self.config.verbosity >= ContextVerbosity::Normal {
            let contexts = self.host_contexts.read().await;
            if let Some(ctx) = contexts.get(host) {
                if !ctx.variables.is_empty() || !ctx.facts.is_empty() || !ctx.registered.is_empty()
                {
                    self.print_context_header();
                    self.print_variables(&ctx.variables);
                    self.print_facts(&ctx.facts);
                    self.print_registered(&ctx.registered);
                    self.print_context_footer();
                }
            }
        }
    }

    /// Called when a task completes - updates statistics and context.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let mut stats = self.host_stats.write().await;
        let host_stats = stats.entry(result.host.clone()).or_default();

        // Update statistics based on result
        if result.result.skipped {
            host_stats.skipped += 1;
            println!(
                "{}: {} | {}",
                "SKIPPED".cyan(),
                result.host.bright_white(),
                result.result.message.bright_black()
            );
        } else if !result.result.success {
            host_stats.failed += 1;
            println!(
                "{}: {} | {} | {}",
                "FAILED".red().bold(),
                result.host.bright_white(),
                result.task_name.yellow(),
                result.result.message
            );
        } else if result.result.changed {
            host_stats.changed += 1;
            println!(
                "{}: {} | {}",
                "CHANGED".yellow(),
                result.host.bright_white(),
                result.result.message.bright_black()
            );
        } else {
            host_stats.ok += 1;
            println!(
                "{}: {} | {}",
                "OK".green(),
                result.host.bright_white(),
                result.result.message.bright_black()
            );
        }

        // Update registered variables in context if this task registered something
        if let Some(data) = &result.result.data {
            let mut contexts = self.host_contexts.write().await;
            let ctx = contexts.entry(result.host.clone()).or_default();

            // Store the result under the task name for display purposes
            ctx.registered
                .insert(result.task_name.clone(), data.clone());
        }
    }

    /// Called when a handler is triggered.
    async fn on_handler_triggered(&self, name: &str) {
        if self.config.verbosity >= ContextVerbosity::Verbose {
            println!("{}: {}", "HANDLER NOTIFIED".magenta(), name.bright_white());
        }
    }

    /// Called when facts are gathered - stores them for context display.
    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let mut contexts = self.host_contexts.write().await;
        let ctx = contexts.entry(host.to_string()).or_default();

        // Convert facts to IndexMap<String, JsonValue>
        for (key, value) in facts.all() {
            ctx.facts.insert(key.clone(), value.clone());
        }

        if self.config.verbosity >= ContextVerbosity::Verbose {
            println!(
                "{}: {} ({} facts)",
                "FACTS GATHERED".bright_black(),
                host.bright_white(),
                facts.all().len()
            );
        }
    }
}

/// Builder for ContextCallback configuration.
#[derive(Debug, Default)]
pub struct ContextCallbackBuilder {
    config: ContextCallbackConfig,
}

impl ContextCallbackBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the verbosity level.
    pub fn verbosity(mut self, verbosity: ContextVerbosity) -> Self {
        self.config.verbosity = verbosity;
        self
    }

    /// Enable or disable facts display.
    pub fn show_facts(mut self, show: bool) -> Self {
        self.config.show_facts = show;
        self
    }

    /// Enable or disable registered variables display.
    pub fn show_registered(mut self, show: bool) -> Self {
        self.config.show_registered = show;
        self
    }

    /// Enable or disable sensitive data masking.
    pub fn mask_sensitive(mut self, mask: bool) -> Self {
        self.config.mask_sensitive = mask;
        self
    }

    /// Add additional sensitive patterns.
    pub fn sensitive_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.config
            .additional_sensitive_patterns
            .push(pattern.into());
        self
    }

    /// Set maximum nesting depth for display.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.config.max_depth = depth;
        self
    }

    /// Set maximum string length before truncation.
    pub fn max_string_length(mut self, length: usize) -> Self {
        self.config.max_string_length = length;
        self
    }

    /// Enable or disable showing empty values.
    pub fn show_empty(mut self, show: bool) -> Self {
        self.config.show_empty = show;
        self
    }

    /// Build the ContextCallback.
    pub fn build(self) -> ContextCallback {
        ContextCallback::with_config(self.config)
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

    #[test]
    fn test_sensitive_detection() {
        let callback = ContextCallback::new();

        // Should detect sensitive patterns
        assert!(callback.is_sensitive("password"));
        assert!(callback.is_sensitive("db_password"));
        assert!(callback.is_sensitive("api_key"));
        assert!(callback.is_sensitive("secret_token"));
        assert!(callback.is_sensitive("AWS_SECRET_KEY"));
        assert!(callback.is_sensitive("mysql_PASSWORD"));

        // Should not detect non-sensitive patterns
        assert!(!callback.is_sensitive("username"));
        assert!(!callback.is_sensitive("hostname"));
        assert!(!callback.is_sensitive("port"));
        assert!(!callback.is_sensitive("enabled"));
    }

    #[test]
    fn test_sensitive_masking_disabled() {
        let callback = ContextCallback::with_config(ContextCallbackConfig {
            mask_sensitive: false,
            ..Default::default()
        });

        assert!(!callback.is_sensitive("password"));
        assert!(!callback.is_sensitive("api_key"));
    }

    #[test]
    fn test_custom_sensitive_patterns() {
        let callback = ContextCallback::with_config(ContextCallbackConfig {
            additional_sensitive_patterns: vec!["my_custom_secret".to_string()],
            ..Default::default()
        });

        assert!(callback.is_sensitive("my_custom_secret"));
        assert!(callback.is_sensitive("MY_CUSTOM_SECRET_VALUE"));
    }

    #[test]
    fn test_value_formatting() {
        let callback = ContextCallback::new();

        // Simple values
        assert_eq!(callback.format_value(&JsonValue::Null, 0), "null");
        assert_eq!(callback.format_value(&JsonValue::Bool(true), 0), "true");
        assert_eq!(callback.format_value(&serde_json::json!(42), 0), "42");
        assert_eq!(
            callback.format_value(&serde_json::json!("hello"), 0),
            "\"hello\""
        );

        // Arrays
        assert_eq!(callback.format_value(&serde_json::json!([]), 0), "[]");
        assert_eq!(
            callback.format_value(&serde_json::json!([1, 2, 3]), 0),
            "[1, 2, 3]"
        );

        // Objects
        assert_eq!(callback.format_value(&serde_json::json!({}), 0), "{}");
    }

    #[test]
    fn test_value_masking() {
        let callback = ContextCallback::new();

        let secret = serde_json::json!("super_secret_value");
        assert_eq!(callback.mask_value("password", &secret), MASK);
        assert_eq!(callback.mask_value("api_key", &secret), MASK);

        let normal = serde_json::json!("normal_value");
        assert_eq!(callback.mask_value("hostname", &normal), "\"normal_value\"");
    }

    #[test]
    fn test_string_truncation() {
        let config = ContextCallbackConfig {
            max_string_length: 10,
            ..Default::default()
        };
        let callback = ContextCallback::with_config(config);

        let short = serde_json::json!("short");
        assert_eq!(callback.format_value(&short, 0), "\"short\"");

        let long = serde_json::json!("this is a very long string that should be truncated");
        let formatted = callback.format_value(&long, 0);
        assert!(formatted.contains("..."));
        assert!(formatted.contains("chars"));
    }

    #[test]
    fn test_depth_limiting() {
        let config = ContextCallbackConfig {
            max_depth: 2,
            ..Default::default()
        };
        let callback = ContextCallback::with_config(config);

        let deep = serde_json::json!({
            "level1": {
                "level2": {
                    "level3": "value"
                }
            }
        });

        let formatted = callback.format_value(&deep, 0);
        // At depth 2, the level2 object is formatted, and when level3's VALUE
        // is accessed at depth 3, it returns "...". The key "level3" still appears
        // but its value is truncated. The output is: {level1: {level2: {level3: ...}}}
        // So level3 key IS present but its value is "..."
        assert!(formatted.contains("..."));
        // The value "value" should NOT appear since it's beyond max_depth
        assert!(!formatted.contains("\"value\""));
    }

    #[tokio::test]
    async fn test_context_callback_stats() {
        let callback = ContextCallback::new();

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
    }

    #[tokio::test]
    async fn test_facts_gathering() {
        let callback = ContextCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let mut facts = Facts::new();
        facts.set("os_family", serde_json::json!("Debian"));
        facts.set("distribution", serde_json::json!("Ubuntu"));

        callback.on_facts_gathered("host1", &facts).await;

        // Verify facts are stored in context
        let contexts = callback.host_contexts.read().await;
        let ctx = contexts.get("host1").unwrap();
        assert!(ctx.facts.contains_key("os_family"));
        assert!(ctx.facts.contains_key("distribution"));
    }

    #[test]
    fn test_builder_pattern() {
        let callback = ContextCallbackBuilder::new()
            .verbosity(ContextVerbosity::Verbose)
            .show_facts(true)
            .show_registered(true)
            .mask_sensitive(true)
            .sensitive_pattern("custom_secret")
            .max_depth(5)
            .max_string_length(100)
            .show_empty(false)
            .build();

        assert_eq!(callback.config.verbosity, ContextVerbosity::Verbose);
        assert!(callback.config.show_facts);
        assert!(callback.config.show_registered);
        assert!(callback.config.mask_sensitive);
        assert_eq!(callback.config.max_depth, 5);
        assert_eq!(callback.config.max_string_length, 100);
        assert!(!callback.config.show_empty);
        assert!(callback.is_sensitive("custom_secret"));
    }

    #[test]
    fn test_preset_constructors() {
        let minimal = ContextCallback::minimal();
        assert_eq!(minimal.config.verbosity, ContextVerbosity::Minimal);
        assert!(!minimal.config.show_facts);
        assert!(!minimal.config.show_registered);

        let verbose = ContextCallback::verbose();
        assert_eq!(verbose.config.verbosity, ContextVerbosity::Verbose);
        assert!(verbose.config.show_facts);
        assert!(verbose.config.show_registered);

        let debug = ContextCallback::debug();
        assert_eq!(debug.config.verbosity, ContextVerbosity::Debug);
        assert_eq!(debug.config.max_depth, 8);
        assert_eq!(debug.config.max_string_length, 500);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = ContextCallback::new();
        let callback2 = callback1.clone();

        // Both should share the same underlying state
        assert!(Arc::ptr_eq(&callback1.host_stats, &callback2.host_stats));
        assert!(Arc::ptr_eq(
            &callback1.host_contexts,
            &callback2.host_contexts
        ));
    }

    #[test]
    fn test_format_task_header() {
        let header = ContextCallback::format_task_header("Install nginx");
        assert!(header.contains("TASK"));
        assert!(header.contains("Install nginx"));
    }

    #[test]
    fn test_format_recap_line() {
        let stats = HostStats {
            ok: 5,
            changed: 2,
            failed: 1,
            skipped: 0,
        };

        let output = ContextCallback::format_recap_line("webserver1", &stats);
        let output_plain = console::strip_ansi_codes(&output);

        assert!(output_plain.contains("webserver1"));
        assert!(output_plain.contains("ok=5"));
        assert!(output_plain.contains("changed=2"));
        assert!(output_plain.contains("failed=1"));
    }
}
