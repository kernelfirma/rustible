//! Log File Callback Plugin for Rustible
//!
//! This plugin logs playbook execution to files with comprehensive features:
//!
//! - Separate log file per playbook run (timestamped filenames)
//! - Configurable log directory
//! - Log rotation support (by count and/or age)
//! - Both human-readable and machine-parseable (JSON) formats
//! - ISO 8601 timestamps on all log entries
//!
//! # Features
//!
//! - **Per-run logs**: Each playbook run creates a unique log file
//! - **Dual format**: Human-readable text with optional JSON sidecar
//! - **Rotation**: Automatic cleanup of old log files
//! - **Configurable**: Directory, retention, and format options
//! - **Thread-safe**: Safe for concurrent access
//!
//! # Example Output (Human-readable)
//!
//! ```text
//! [2024-01-15T10:30:45.123Z] PLAYBOOK START: deploy.yml
//! [2024-01-15T10:30:45.125Z] PLAY START: Deploy application | hosts: webservers
//! [2024-01-15T10:30:45.130Z] TASK START: Install nginx | host: web01
//! [2024-01-15T10:30:46.234Z] TASK RESULT: web01 | Install nginx | CHANGED (1.104s)
//! [2024-01-15T10:30:46.890Z] TASK RESULT: web02 | Install nginx | OK (0.656s)
//! [2024-01-15T10:31:00.000Z] PLAYBOOK END: deploy.yml | Duration: 14.877s
//! ```
//!
//! # Example Output (JSON, one entry per line)
//!
//! ```json
//! {"timestamp":"2024-01-15T10:30:45.123Z","event":"playbook_start","playbook":"deploy.yml"}
//! {"timestamp":"2024-01-15T10:30:45.125Z","event":"play_start","name":"Deploy application","hosts":"web01, web02"}
//! ```
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::{LogFileCallback, LogFileConfig};
//! use std::path::PathBuf;
//!
//! // Create with default configuration (logs to ./logs)
//! let callback = LogFileCallback::new(LogFileConfig::default())?;
//!
//! // Or with custom configuration
//! let config = LogFileConfig::builder()
//!     .log_directory("/var/log/rustible")
//!     .json_format(true)
//!     .max_log_files(30)
//!     .build();
//! let callback = LogFileCallback::new(config)?;
//!
//! # let _ = ();
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the log file callback plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFileConfig {
    /// Directory where log files are stored.
    /// Defaults to `./logs` if not specified.
    pub log_directory: PathBuf,

    /// Whether to also write JSON format logs.
    /// Creates a `.json` sidecar file alongside the human-readable log.
    #[serde(default)]
    pub json_format: bool,

    /// Maximum number of log files to retain.
    /// Older files are deleted when this limit is exceeded.
    /// Set to 0 for unlimited retention.
    #[serde(default = "default_max_log_files")]
    pub max_log_files: usize,

    /// Maximum age of log files in days.
    /// Files older than this are deleted during rotation.
    /// Set to 0 for unlimited age.
    #[serde(default)]
    pub max_log_age_days: u32,

    /// Prefix for log file names.
    /// Files are named: `{prefix}_{playbook}_{timestamp}.log`
    #[serde(default = "default_log_prefix")]
    pub log_prefix: String,

    /// Whether to include task arguments in logs.
    /// May expose sensitive data - disabled by default.
    #[serde(default)]
    pub include_task_args: bool,

    /// Whether to include command stdout/stderr in logs.
    #[serde(default = "default_include_output")]
    pub include_output: bool,

    /// Whether to include diffs in logs.
    #[serde(default)]
    pub include_diffs: bool,

    /// Whether to flush after each write (slower but safer).
    #[serde(default = "default_flush_immediately")]
    pub flush_immediately: bool,

    /// Whether to append to existing log file or create new one.
    #[serde(default)]
    pub append: bool,

    /// Log level filter: "all", "changes", "failures"
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_max_log_files() -> usize {
    50
}

fn default_log_prefix() -> String {
    "rustible".to_string()
}

fn default_include_output() -> bool {
    true
}

fn default_flush_immediately() -> bool {
    true
}

fn default_log_level() -> String {
    "all".to_string()
}

impl Default for LogFileConfig {
    fn default() -> Self {
        Self {
            log_directory: PathBuf::from("./logs"),
            json_format: false,
            max_log_files: default_max_log_files(),
            max_log_age_days: 0,
            log_prefix: default_log_prefix(),
            include_task_args: false,
            include_output: default_include_output(),
            include_diffs: false,
            flush_immediately: default_flush_immediately(),
            append: false,
            log_level: default_log_level(),
        }
    }
}

impl LogFileConfig {
    /// Creates a new configuration builder.
    #[must_use]
    pub fn builder() -> LogFileConfigBuilder {
        LogFileConfigBuilder::default()
    }

    /// Creates a configuration with the specified log directory.
    #[must_use]
    pub fn with_directory(directory: impl Into<PathBuf>) -> Self {
        Self {
            log_directory: directory.into(),
            ..Default::default()
        }
    }
}

/// Builder for `LogFileConfig`.
#[derive(Debug, Default)]
pub struct LogFileConfigBuilder {
    config: LogFileConfig,
}

impl LogFileConfigBuilder {
    /// Sets the log directory.
    #[must_use]
    pub fn log_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.log_directory = path.into();
        self
    }

    /// Enables or disables JSON format logging.
    #[must_use]
    pub fn json_format(mut self, enabled: bool) -> Self {
        self.config.json_format = enabled;
        self
    }

    /// Sets the maximum number of log files to retain.
    #[must_use]
    pub fn max_log_files(mut self, count: usize) -> Self {
        self.config.max_log_files = count;
        self
    }

    /// Sets the maximum age of log files in days.
    #[must_use]
    pub fn max_log_age_days(mut self, days: u32) -> Self {
        self.config.max_log_age_days = days;
        self
    }

    /// Sets the log file name prefix.
    #[must_use]
    pub fn log_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.config.log_prefix = prefix.into();
        self
    }

    /// Enables or disables including task arguments in logs.
    #[must_use]
    pub fn include_task_args(mut self, enabled: bool) -> Self {
        self.config.include_task_args = enabled;
        self
    }

    /// Enables or disables including command output in logs.
    #[must_use]
    pub fn include_output(mut self, enabled: bool) -> Self {
        self.config.include_output = enabled;
        self
    }

    /// Enables or disables including diffs in logs.
    #[must_use]
    pub fn include_diffs(mut self, enabled: bool) -> Self {
        self.config.include_diffs = enabled;
        self
    }

    /// Enables or disables immediate flushing.
    #[must_use]
    pub fn flush_immediately(mut self, enabled: bool) -> Self {
        self.config.flush_immediately = enabled;
        self
    }

    /// Enables or disables append mode.
    #[must_use]
    pub fn append(mut self, enabled: bool) -> Self {
        self.config.append = enabled;
        self
    }

    /// Sets the log level filter.
    #[must_use]
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.config.log_level = level.into();
        self
    }

    /// Builds the configuration.
    #[must_use]
    pub fn build(self) -> LogFileConfig {
        self.config
    }
}

// ============================================================================
// Log Events
// ============================================================================

/// A log entry event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LogEvent {
    /// Playbook execution started.
    PlaybookStart { playbook: String },
    /// Playbook execution ended.
    PlaybookEnd {
        playbook: String,
        duration_secs: f64,
        success: bool,
        stats: HashMap<String, HostLogStats>,
    },
    /// Play execution started.
    PlayStart { name: String, hosts: String },
    /// Play execution ended.
    PlayEnd { name: String, success: bool },
    /// Task execution started.
    TaskStart {
        name: String,
        host: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        module: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
    },
    /// Task execution completed on a host.
    TaskResult {
        host: String,
        task_name: String,
        status: String,
        changed: bool,
        duration_secs: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stdout: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stderr: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
    },
    /// Handler was triggered.
    HandlerTriggered { name: String },
    /// Facts were gathered for a host.
    FactsGathered { host: String, fact_count: usize },
    /// Host became unreachable.
    HostUnreachable {
        host: String,
        task_name: String,
        error: String,
    },
    /// Warning message.
    Warning { message: String },
    /// Debug message.
    Debug {
        host: Option<String>,
        message: String,
    },
}

/// Per-host statistics for log summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostLogStats {
    pub ok: u32,
    pub changed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub unreachable: u32,
}

// ============================================================================
// Log Entry
// ============================================================================

/// A timestamped log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// The event data.
    #[serde(flatten)]
    pub event: LogEvent,
}

impl LogEntry {
    /// Creates a new log entry with the current timestamp.
    fn new(event: LogEvent) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            event,
        }
    }

    /// Formats the entry as a human-readable log line.
    fn format_human(&self) -> String {
        let ts = &self.timestamp;
        match &self.event {
            LogEvent::PlaybookStart { playbook } => {
                format!("[{}] PLAYBOOK START: {}", ts, playbook)
            }
            LogEvent::PlaybookEnd {
                playbook,
                duration_secs,
                success,
                stats,
            } => {
                let status = if *success { "SUCCESS" } else { "FAILED" };
                let mut lines = vec![format!(
                    "[{}] PLAYBOOK END: {} | {} | Duration: {:.3}s",
                    ts, playbook, status, duration_secs
                )];

                // Add per-host stats in sorted order
                let mut hosts: Vec<_> = stats.keys().collect();
                hosts.sort();
                for host in hosts {
                    if let Some(s) = stats.get(host) {
                        lines.push(format!(
                            "[{}]   {}: ok={} changed={} failed={} skipped={} unreachable={}",
                            ts, host, s.ok, s.changed, s.failed, s.skipped, s.unreachable
                        ));
                    }
                }
                lines.join("\n")
            }
            LogEvent::PlayStart { name, hosts } => {
                format!("[{}] PLAY START: {} | hosts: {}", ts, name, hosts)
            }
            LogEvent::PlayEnd { name, success } => {
                let status = if *success { "SUCCESS" } else { "FAILED" };
                format!("[{}] PLAY END: {} | {}", ts, name, status)
            }
            LogEvent::TaskStart {
                name,
                host,
                module,
                args,
            } => {
                let module_str = module.as_deref().unwrap_or("unknown");
                if let Some(args) = args {
                    format!(
                        "[{}] TASK START: {} | host: {} | module: {} | args: {}",
                        ts,
                        name,
                        host,
                        module_str,
                        serde_json::to_string(args).unwrap_or_default()
                    )
                } else {
                    format!(
                        "[{}] TASK START: {} | host: {} | module: {}",
                        ts, name, host, module_str
                    )
                }
            }
            LogEvent::TaskResult {
                host,
                task_name,
                status,
                changed,
                duration_secs,
                message,
                stdout,
                stderr,
                diff,
            } => {
                let changed_str = if *changed { " (changed)" } else { "" };
                let duration_str = duration_secs
                    .map(|d| format!(" ({:.3}s)", d))
                    .unwrap_or_default();
                let mut line = format!(
                    "[{}] TASK RESULT: {} | {} | {}{}{}",
                    ts, host, task_name, status, changed_str, duration_str
                );

                if let Some(msg) = message {
                    if !msg.is_empty() {
                        line.push_str(&format!("\n[{}]   msg: {}", ts, msg));
                    }
                }
                if let Some(out) = stdout {
                    if !out.is_empty() {
                        line.push_str(&format!("\n[{}]   stdout: {}", ts, out.trim()));
                    }
                }
                if let Some(err) = stderr {
                    if !err.is_empty() {
                        line.push_str(&format!("\n[{}]   stderr: {}", ts, err.trim()));
                    }
                }
                if let Some(d) = diff {
                    if !d.is_empty() {
                        line.push_str(&format!("\n[{}]   diff:\n{}", ts, d));
                    }
                }
                line
            }
            LogEvent::HandlerTriggered { name } => {
                format!("[{}] HANDLER TRIGGERED: {}", ts, name)
            }
            LogEvent::FactsGathered { host, fact_count } => {
                format!("[{}] FACTS GATHERED: {} | {} facts", ts, host, fact_count)
            }
            LogEvent::HostUnreachable {
                host,
                task_name,
                error,
            } => {
                format!(
                    "[{}] HOST UNREACHABLE: {} | {} | {}",
                    ts, host, task_name, error
                )
            }
            LogEvent::Warning { message } => {
                format!("[{}] WARNING: {}", ts, message)
            }
            LogEvent::Debug { host, message } => {
                if let Some(h) = host {
                    format!("[{}] DEBUG: {} | {}", ts, h, message)
                } else {
                    format!("[{}] DEBUG: {}", ts, message)
                }
            }
        }
    }

    /// Formats the entry as a JSON line (JSONL format).
    fn format_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

// ============================================================================
// Internal State
// ============================================================================

/// Internal state for the log file callback.
///
/// Note: `Debug` is implemented manually since `BufWriter<File>` doesn't implement `Debug`.
struct LogFileState {
    /// Human-readable log file writer.
    text_writer: Option<BufWriter<File>>,
    /// JSON log file writer (if enabled).
    json_writer: Option<BufWriter<File>>,
    /// Path to the current text log file.
    text_log_path: Option<PathBuf>,
    /// Path to the current JSON log file.
    json_log_path: Option<PathBuf>,
    /// Playbook start time for duration tracking.
    start_time: Option<Instant>,
    /// Current playbook name.
    playbook_name: Option<String>,
    /// Per-host statistics.
    host_stats: HashMap<String, HostLogStats>,
    /// Whether any failures occurred.
    has_failures: bool,
    /// Current task name being executed.
    current_task: Option<String>,
}

impl std::fmt::Debug for LogFileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogFileState")
            .field(
                "text_writer",
                &self.text_writer.as_ref().map(|_| "<BufWriter>"),
            )
            .field(
                "json_writer",
                &self.json_writer.as_ref().map(|_| "<BufWriter>"),
            )
            .field("text_log_path", &self.text_log_path)
            .field("json_log_path", &self.json_log_path)
            .field("start_time", &self.start_time)
            .field("playbook_name", &self.playbook_name)
            .field("host_stats", &self.host_stats)
            .field("has_failures", &self.has_failures)
            .field("current_task", &self.current_task)
            .finish()
    }
}

impl LogFileState {
    fn new() -> Self {
        Self {
            text_writer: None,
            json_writer: None,
            text_log_path: None,
            json_log_path: None,
            start_time: None,
            playbook_name: None,
            host_stats: HashMap::new(),
            has_failures: false,
            current_task: None,
        }
    }

    #[allow(dead_code)]
    fn reset(&mut self) {
        self.text_writer = None;
        self.json_writer = None;
        self.text_log_path = None;
        self.json_log_path = None;
        self.start_time = None;
        self.playbook_name = None;
        self.host_stats.clear();
        self.has_failures = false;
        self.current_task = None;
    }
}

// ============================================================================
// LogFileCallback Implementation
// ============================================================================

/// Log file callback plugin that writes execution logs to files.
///
/// This callback creates separate log files for each playbook run,
/// with support for both human-readable and JSON formats, plus
/// automatic log rotation.
///
/// # Thread Safety
///
/// This callback is safe for concurrent access from multiple tasks.
/// All state is protected by a `RwLock`.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::{LogFileCallback, LogFileConfig};
///
/// // Create with default config (logs to ./logs)
/// let callback = LogFileCallback::new(LogFileConfig::default())?;
///
/// // Or with custom config
/// let config = LogFileConfig::builder()
///     .log_directory("/var/log/rustible")
///     .json_format(true)
///     .max_log_files(30)
///     .build();
/// let callback = LogFileCallback::new(config)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct LogFileCallback {
    /// Configuration for the callback.
    config: LogFileConfig,
    /// Internal state protected by RwLock.
    state: RwLock<LogFileState>,
}

impl LogFileCallback {
    /// Creates a new log file callback with the given configuration.
    ///
    /// This will create the log directory if it doesn't exist and
    /// perform log rotation based on the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the log directory cannot be created.
    pub fn new(config: LogFileConfig) -> std::io::Result<Self> {
        // Ensure log directory exists
        fs::create_dir_all(&config.log_directory)?;

        let callback = Self {
            config,
            state: RwLock::new(LogFileState::new()),
        };

        // Perform initial rotation
        callback.rotate_logs()?;

        Ok(callback)
    }

    /// Creates a new log file callback with default configuration.
    ///
    /// Logs are written to `./logs` directory.
    pub fn with_defaults() -> std::io::Result<Self> {
        Self::new(LogFileConfig::default())
    }

    /// Returns the path to the current text log file, if any.
    pub fn current_log_path(&self) -> Option<PathBuf> {
        self.state.read().text_log_path.clone()
    }

    /// Returns the path to the current JSON log file, if any.
    pub fn current_json_log_path(&self) -> Option<PathBuf> {
        self.state.read().json_log_path.clone()
    }

    /// Returns whether any failures occurred during execution.
    pub fn has_failures(&self) -> bool {
        self.state.read().has_failures
    }

    /// Returns the current host statistics.
    pub fn get_stats(&self) -> HashMap<String, HostLogStats> {
        self.state.read().host_stats.clone()
    }

    /// Performs log rotation based on configuration.
    fn rotate_logs(&self) -> std::io::Result<()> {
        if self.config.max_log_files == 0 && self.config.max_log_age_days == 0 {
            return Ok(()); // No rotation configured
        }

        let log_dir = &self.config.log_directory;
        if !log_dir.exists() {
            return Ok(());
        }

        // Collect log files with their metadata
        let mut log_files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

        for entry in fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Only consider files matching our prefix
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(&self.config.log_prefix)
                    && (name.ends_with(".log") || name.ends_with(".json"))
                {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            log_files.push((path, modified));
                        }
                    }
                }
            }
        }

        // Sort by modification time (oldest first)
        log_files.sort_by(|a, b| a.1.cmp(&b.1));

        let now = std::time::SystemTime::now();

        // Remove files by age
        if self.config.max_log_age_days > 0 {
            let max_age = Duration::from_secs(self.config.max_log_age_days as u64 * 24 * 60 * 60);
            log_files.retain(|(path, modified)| {
                if let Ok(age) = now.duration_since(*modified) {
                    if age > max_age {
                        let _ = fs::remove_file(path);
                        return false;
                    }
                }
                true
            });
        }

        // Remove excess files by count (keep newest)
        // Note: We count .log and .json pairs as one file set
        if self.config.max_log_files > 0 {
            // Group by base name (without extension)
            let mut base_names: Vec<String> = log_files
                .iter()
                .filter_map(|(path, _)| path.file_stem().and_then(|s| s.to_str()).map(String::from))
                .collect();
            base_names.sort();
            base_names.dedup();

            if base_names.len() > self.config.max_log_files {
                let to_remove = base_names.len() - self.config.max_log_files;
                for base in base_names.iter().take(to_remove) {
                    let text_path = log_dir.join(format!("{}.log", base));
                    let json_path = log_dir.join(format!("{}.json", base));
                    let _ = fs::remove_file(text_path);
                    let _ = fs::remove_file(json_path);
                }
            }
        }

        Ok(())
    }

    /// Generates a unique log file name for the current run.
    fn generate_log_filename(&self, playbook_name: &str) -> String {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let sanitized_playbook = playbook_name
            .replace(['/', '\\', ' ', '.'], "_")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>();

        format!(
            "{}_{}_{}",
            self.config.log_prefix, sanitized_playbook, timestamp
        )
    }

    /// Opens log files for a new playbook run.
    fn open_log_files(&self, playbook_name: &str) -> std::io::Result<()> {
        let base_name = self.generate_log_filename(playbook_name);
        let text_path = self.config.log_directory.join(format!("{}.log", base_name));

        let text_file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(self.config.append)
            .truncate(!self.config.append)
            .open(&text_path)?;

        let mut state = self.state.write();
        state.text_writer = Some(BufWriter::new(text_file));
        state.text_log_path = Some(text_path);

        if self.config.json_format {
            let json_path = self
                .config
                .log_directory
                .join(format!("{}.json", base_name));
            let json_file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(self.config.append)
                .truncate(!self.config.append)
                .open(&json_path)?;

            state.json_writer = Some(BufWriter::new(json_file));
            state.json_log_path = Some(json_path);
        }

        Ok(())
    }

    /// Writes a log entry to the log files.
    fn write_entry(&self, entry: LogEntry) {
        let mut state = self.state.write();

        // Write human-readable format
        if let Some(ref mut writer) = state.text_writer {
            let line = entry.format_human();
            if writeln!(writer, "{}", line).is_ok() && self.config.flush_immediately {
                let _ = writer.flush();
            }
        }

        // Write JSON format
        if let Some(ref mut writer) = state.json_writer {
            let line = entry.format_json();
            if writeln!(writer, "{}", line).is_ok() && self.config.flush_immediately {
                let _ = writer.flush();
            }
        }
    }

    /// Flushes and closes the current log files.
    fn close_log_files(&self) {
        let mut state = self.state.write();

        if let Some(ref mut writer) = state.text_writer {
            let _ = writer.flush();
        }
        state.text_writer = None;

        if let Some(ref mut writer) = state.json_writer {
            let _ = writer.flush();
        }
        state.json_writer = None;
    }

    /// Checks if the given result should be logged based on log level.
    fn should_log_result(&self, result: &ExecutionResult) -> bool {
        match self.config.log_level.as_str() {
            "failures" => !result.result.success,
            "changes" => result.result.changed || !result.result.success,
            _ => true, // "all" or unknown
        }
    }

    /// Logs a warning message.
    pub fn log_warning(&self, message: &str) {
        self.write_entry(LogEntry::new(LogEvent::Warning {
            message: message.to_string(),
        }));
    }

    /// Logs a debug message.
    pub fn log_debug(&self, host: Option<&str>, message: &str) {
        self.write_entry(LogEntry::new(LogEvent::Debug {
            host: host.map(String::from),
            message: message.to_string(),
        }));
    }

    /// Logs a host unreachable event.
    pub fn log_host_unreachable(&self, host: &str, task_name: &str, error: &str) {
        // Update stats
        {
            let mut state = self.state.write();
            let host_stats = state.host_stats.entry(host.to_string()).or_default();
            host_stats.unreachable += 1;
            state.has_failures = true;
        }

        self.write_entry(LogEntry::new(LogEvent::HostUnreachable {
            host: host.to_string(),
            task_name: task_name.to_string(),
            error: error.to_string(),
        }));
    }
}

// ============================================================================
// ExecutionCallback Implementation
// ============================================================================

#[async_trait]
impl ExecutionCallback for LogFileCallback {
    async fn on_playbook_start(&self, name: &str) {
        // Perform rotation before starting new log
        let _ = self.rotate_logs();

        // Open new log files
        if let Err(e) = self.open_log_files(name) {
            eprintln!("Warning: Failed to open log files: {}", e);
            return;
        }

        // Initialize state
        {
            let mut state = self.state.write();
            state.start_time = Some(Instant::now());
            state.playbook_name = Some(name.to_string());
            state.host_stats.clear();
            state.has_failures = false;
        }

        // Write playbook start entry
        self.write_entry(LogEntry::new(LogEvent::PlaybookStart {
            playbook: name.to_string(),
        }));
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let (duration, stats) = {
            let state = self.state.read();
            let duration = state
                .start_time
                .map(|s| s.elapsed().as_secs_f64())
                .unwrap_or(0.0);
            (duration, state.host_stats.clone())
        };

        self.write_entry(LogEntry::new(LogEvent::PlaybookEnd {
            playbook: name.to_string(),
            duration_secs: duration,
            success,
            stats,
        }));

        self.close_log_files();
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Initialize host stats for all hosts in this play
        {
            let mut state = self.state.write();
            for host in hosts {
                state.host_stats.entry(host.clone()).or_default();
            }
        }

        self.write_entry(LogEntry::new(LogEvent::PlayStart {
            name: name.to_string(),
            hosts: hosts.join(", "),
        }));
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        self.write_entry(LogEntry::new(LogEvent::PlayEnd {
            name: name.to_string(),
            success,
        }));
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        // Store current task name for later reference
        {
            let mut state = self.state.write();
            state.current_task = Some(name.to_string());
        }

        self.write_entry(LogEntry::new(LogEvent::TaskStart {
            name: name.to_string(),
            host: host.to_string(),
            module: None,
            args: None,
        }));
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Check log level filter
        if !self.should_log_result(result) {
            // Still update stats even if not logging
            let mut state = self.state.write();
            let host_stats = state.host_stats.entry(result.host.clone()).or_default();

            if result.result.skipped {
                host_stats.skipped += 1;
            } else if !result.result.success {
                host_stats.failed += 1;
                state.has_failures = true;
            } else if result.result.changed {
                host_stats.changed += 1;
            } else {
                host_stats.ok += 1;
            }
            return;
        }

        let status = if result.result.skipped {
            "SKIPPED"
        } else if !result.result.success {
            "FAILED"
        } else if result.result.changed {
            "CHANGED"
        } else {
            "OK"
        };

        // Update stats
        {
            let mut state = self.state.write();
            let host_stats = state.host_stats.entry(result.host.clone()).or_default();

            if result.result.skipped {
                host_stats.skipped += 1;
            } else if !result.result.success {
                host_stats.failed += 1;
                state.has_failures = true;
            } else if result.result.changed {
                host_stats.changed += 1;
            } else {
                host_stats.ok += 1;
            }
        }

        // Build the log entry
        let message = if result.result.message.is_empty() {
            None
        } else {
            Some(result.result.message.clone())
        };

        // Extract stdout/stderr from data if available and configured
        let (stdout, stderr) = if self.config.include_output {
            if let Some(ref data) = result.result.data {
                let stdout = data
                    .get("stdout")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let stderr = data
                    .get("stderr")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                (stdout, stderr)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        // Extract diff if available and configured
        let diff = if self.config.include_diffs {
            if let Some(ref data) = result.result.data {
                data.get("diff")
                    .and_then(|v| serde_json::to_string_pretty(v).ok())
            } else {
                None
            }
        } else {
            None
        };

        self.write_entry(LogEntry::new(LogEvent::TaskResult {
            host: result.host.clone(),
            task_name: result.task_name.clone(),
            status: status.to_string(),
            changed: result.result.changed,
            duration_secs: Some(result.duration.as_secs_f64()),
            message,
            stdout,
            stderr,
            diff,
        }));
    }

    async fn on_handler_triggered(&self, name: &str) {
        self.write_entry(LogEntry::new(LogEvent::HandlerTriggered {
            name: name.to_string(),
        }));
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let fact_count = facts.all().len();

        self.write_entry(LogEntry::new(LogEvent::FactsGathered {
            host: host.to_string(),
            fact_count,
        }));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::TempDir;

    fn create_test_config(dir: &Path) -> LogFileConfig {
        LogFileConfig {
            log_directory: dir.to_path_buf(),
            json_format: true,
            max_log_files: 5,
            max_log_age_days: 0,
            log_prefix: "test".to_string(),
            include_task_args: false,
            include_output: true,
            include_diffs: false,
            flush_immediately: true,
            append: false,
            log_level: "all".to_string(),
        }
    }

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
    fn test_config_builder() {
        let config = LogFileConfig::builder()
            .log_directory("/var/log/test")
            .json_format(true)
            .max_log_files(10)
            .log_prefix("custom")
            .include_task_args(true)
            .log_level("failures")
            .build();

        assert_eq!(config.log_directory, PathBuf::from("/var/log/test"));
        assert!(config.json_format);
        assert_eq!(config.max_log_files, 10);
        assert_eq!(config.log_prefix, "custom");
        assert!(config.include_task_args);
        assert_eq!(config.log_level, "failures");
    }

    #[test]
    fn test_config_default() {
        let config = LogFileConfig::default();

        assert_eq!(config.log_directory, PathBuf::from("./logs"));
        assert!(!config.json_format);
        assert_eq!(config.max_log_files, 50);
        assert_eq!(config.log_prefix, "rustible");
        assert_eq!(config.log_level, "all");
    }

    #[test]
    fn test_log_entry_format_human() {
        let entry = LogEntry::new(LogEvent::PlaybookStart {
            playbook: "test.yml".to_string(),
        });

        let formatted = entry.format_human();
        assert!(formatted.contains("PLAYBOOK START"));
        assert!(formatted.contains("test.yml"));
    }

    #[test]
    fn test_log_entry_format_json() {
        let entry = LogEntry::new(LogEvent::PlaybookStart {
            playbook: "test.yml".to_string(),
        });

        let formatted = entry.format_json();
        let parsed: serde_json::Value = serde_json::from_str(&formatted).unwrap();

        assert_eq!(parsed["event"], "playbook_start");
        assert_eq!(parsed["playbook"], "test.yml");
        assert!(parsed["timestamp"].is_string());
    }

    #[test]
    fn test_log_entry_task_result_format() {
        let entry = LogEntry::new(LogEvent::TaskResult {
            host: "webserver1".to_string(),
            task_name: "Install nginx".to_string(),
            status: "CHANGED".to_string(),
            changed: true,
            duration_secs: Some(1.234),
            message: Some("Package installed".to_string()),
            stdout: None,
            stderr: None,
            diff: None,
        });

        let formatted = entry.format_human();
        assert!(formatted.contains("webserver1"));
        assert!(formatted.contains("Install nginx"));
        assert!(formatted.contains("CHANGED"));
        assert!(formatted.contains("1.234s"));
    }

    #[tokio::test]
    async fn test_callback_creates_log_files() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(temp_dir.path());

        let callback = LogFileCallback::new(config).unwrap();

        callback.on_playbook_start("test-playbook").await;

        // Verify log files were created
        assert!(callback.current_log_path().is_some());
        assert!(callback.current_json_log_path().is_some());

        // Verify files exist
        let log_path = callback.current_log_path().unwrap();
        let json_path = callback.current_json_log_path().unwrap();
        assert!(log_path.exists());
        assert!(json_path.exists());

        callback.on_playbook_end("test-playbook", true).await;
    }

    #[tokio::test]
    async fn test_callback_writes_entries() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(temp_dir.path());

        let callback = LogFileCallback::new(config).unwrap();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("Test Play", &["host1".to_string()])
            .await;
        callback.on_task_start("Install package", "host1").await;

        let result = create_execution_result("host1", "Install package", true, true, false, "ok");
        callback.on_task_complete(&result).await;

        callback.on_play_end("Test Play", true).await;
        callback.on_playbook_end("test-playbook", true).await;

        // Read and verify log content
        let log_path = callback.current_log_path().unwrap();
        let content = fs::read_to_string(&log_path).unwrap();

        assert!(content.contains("PLAYBOOK START"));
        assert!(content.contains("PLAY START"));
        assert!(content.contains("TASK START"));
        assert!(content.contains("TASK RESULT"));
        assert!(content.contains("PLAYBOOK END"));
    }

    #[tokio::test]
    async fn test_callback_tracks_stats() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(temp_dir.path());

        let callback = LogFileCallback::new(config).unwrap();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("Test Play", &["host1".to_string(), "host2".to_string()])
            .await;

        // Simulate various results
        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        let changed_result =
            create_execution_result("host1", "task2", true, true, false, "changed");
        callback.on_task_complete(&changed_result).await;

        let failed_result = create_execution_result("host2", "task1", false, false, false, "error");
        callback.on_task_complete(&failed_result).await;

        let skipped_result =
            create_execution_result("host2", "task2", true, false, true, "skipped");
        callback.on_task_complete(&skipped_result).await;

        // Verify stats
        let stats = callback.get_stats();

        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.ok, 1);
        assert_eq!(host1_stats.changed, 1);
        assert_eq!(host1_stats.failed, 0);

        let host2_stats = stats.get("host2").unwrap();
        assert_eq!(host2_stats.ok, 0);
        assert_eq!(host2_stats.failed, 1);
        assert_eq!(host2_stats.skipped, 1);

        assert!(callback.has_failures());

        callback.on_playbook_end("test-playbook", false).await;
    }

    #[test]
    fn test_generate_log_filename() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(temp_dir.path());
        let callback = LogFileCallback::new(config).unwrap();

        let filename = callback.generate_log_filename("deploy.yml");
        assert!(filename.starts_with("test_deploy_yml_"));
        assert!(!filename.contains('/'));
        assert!(!filename.contains('\\'));
    }

    #[test]
    fn test_log_rotation() {
        let temp_dir = TempDir::new().unwrap();
        let log_dir = temp_dir.path();

        // Create some old log files
        for i in 0..10 {
            let path = log_dir.join(format!("test_playbook_{:02}.log", i));
            fs::write(&path, "test content").unwrap();
            let json_path = log_dir.join(format!("test_playbook_{:02}.json", i));
            fs::write(&json_path, "{}").unwrap();
        }

        // Create callback with max 5 files
        let config = LogFileConfig {
            log_directory: log_dir.to_path_buf(),
            max_log_files: 5,
            log_prefix: "test".to_string(),
            ..Default::default()
        };

        let _callback = LogFileCallback::new(config).unwrap();

        // Count remaining files
        let count = fs::read_dir(log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "log")
                    .unwrap_or(false)
            })
            .count();

        // Should have at most 5 log files (rotation happened)
        assert!(count <= 5, "Expected at most 5 log files, found {}", count);
    }

    #[test]
    fn test_host_log_stats_default() {
        let stats = HostLogStats::default();
        assert_eq!(stats.ok, 0);
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.unreachable, 0);
    }

    #[tokio::test]
    async fn test_log_level_filter() {
        let temp_dir = TempDir::new().unwrap();
        let config = LogFileConfig {
            log_directory: temp_dir.path().to_path_buf(),
            log_level: "failures".to_string(),
            json_format: false,
            ..Default::default()
        };

        let callback = LogFileCallback::new(config).unwrap();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("Test Play", &["host1".to_string()])
            .await;

        // OK result should not be logged
        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        // Changed result should not be logged (only failures)
        let changed_result =
            create_execution_result("host1", "task2", true, true, false, "changed");
        callback.on_task_complete(&changed_result).await;

        // Failed result should be logged
        let failed_result = create_execution_result("host1", "task3", false, false, false, "error");
        callback.on_task_complete(&failed_result).await;

        callback.on_playbook_end("test-playbook", false).await;

        // Read and verify - only failure should be in task results
        let log_path = callback.current_log_path().unwrap();
        let content = fs::read_to_string(&log_path).unwrap();

        // Count TASK RESULT entries
        let task_results: Vec<_> = content
            .lines()
            .filter(|l| l.contains("TASK RESULT"))
            .collect();
        assert_eq!(task_results.len(), 1, "Only failed task should be logged");
        assert!(task_results[0].contains("FAILED"));

        // But stats should still track all
        let stats = callback.get_stats();
        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.ok, 1);
        assert_eq!(host1_stats.changed, 1);
        assert_eq!(host1_stats.failed, 1);
    }

    #[test]
    fn test_log_warning_and_debug() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(temp_dir.path());

        let callback = LogFileCallback::new(config).unwrap();

        // These should not panic even without active log session
        callback.log_warning("This is a warning");
        callback.log_debug(Some("host1"), "Debug message");
        callback.log_debug(None, "Global debug");
    }

    #[tokio::test]
    async fn test_host_unreachable() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(temp_dir.path());

        let callback = LogFileCallback::new(config).unwrap();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("Test Play", &["host1".to_string()])
            .await;

        callback.log_host_unreachable("host1", "gather_facts", "Connection refused");

        // Verify stats
        let stats = callback.get_stats();
        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.unreachable, 1);

        assert!(callback.has_failures());

        callback.on_playbook_end("test-playbook", false).await;

        // Verify log content
        let log_path = callback.current_log_path().unwrap();
        let content = fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("HOST UNREACHABLE"));
        assert!(content.contains("Connection refused"));
    }
}
