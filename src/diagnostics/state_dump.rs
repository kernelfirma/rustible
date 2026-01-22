//! State dump functionality for failure analysis.
//!
//! This module provides the ability to capture and persist execution state
//! when failures occur, enabling post-mortem debugging and analysis.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Format for state dumps
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateDumpFormat {
    /// JSON format (default)
    #[default]
    Json,
    /// Pretty-printed JSON
    JsonPretty,
    /// YAML format
    Yaml,
    /// Compact binary format (MessagePack)
    Binary,
}

impl StateDumpFormat {
    /// Get the file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            StateDumpFormat::Json | StateDumpFormat::JsonPretty => "json",
            StateDumpFormat::Yaml => "yaml",
            StateDumpFormat::Binary => "msgpack",
        }
    }
}

/// Context information about a failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureContext {
    /// Timestamp of the failure
    pub timestamp: DateTime<Utc>,
    /// Error message
    pub error_message: String,
    /// Error type/category
    pub error_type: String,
    /// Playbook that was running
    pub playbook: Option<String>,
    /// Play that was executing
    pub play: Option<String>,
    /// Task that failed
    pub task: Option<String>,
    /// Host where failure occurred
    pub host: Option<String>,
    /// Module being executed
    pub module: Option<String>,
    /// Module arguments
    pub module_args: Option<JsonValue>,
    /// Command that was run (if applicable)
    pub command: Option<String>,
    /// Exit code (if applicable)
    pub exit_code: Option<i32>,
    /// Stdout from command
    pub stdout: Option<String>,
    /// Stderr from command
    pub stderr: Option<String>,
    /// Current variables at time of failure
    pub variables: HashMap<String, JsonValue>,
    /// Facts for the host
    pub facts: Option<JsonValue>,
    /// Stack trace (if available)
    pub stack_trace: Option<String>,
    /// Additional context data
    pub additional_data: HashMap<String, JsonValue>,
}

impl FailureContext {
    /// Create a new failure context
    pub fn new(error_message: impl Into<String>, error_type: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            error_message: error_message.into(),
            error_type: error_type.into(),
            playbook: None,
            play: None,
            task: None,
            host: None,
            module: None,
            module_args: None,
            command: None,
            exit_code: None,
            stdout: None,
            stderr: None,
            variables: HashMap::new(),
            facts: None,
            stack_trace: None,
            additional_data: HashMap::new(),
        }
    }

    /// Set the playbook
    pub fn with_playbook(mut self, playbook: impl Into<String>) -> Self {
        self.playbook = Some(playbook.into());
        self
    }

    /// Set the play
    pub fn with_play(mut self, play: impl Into<String>) -> Self {
        self.play = Some(play.into());
        self
    }

    /// Set the task
    pub fn with_task(mut self, task: impl Into<String>) -> Self {
        self.task = Some(task.into());
        self
    }

    /// Set the host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Set the module
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module = Some(module.into());
        self
    }

    /// Set module arguments
    pub fn with_module_args(mut self, args: JsonValue) -> Self {
        self.module_args = Some(args);
        self
    }

    /// Set the command
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set command output
    pub fn with_output(
        mut self,
        exit_code: i32,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
    ) -> Self {
        self.exit_code = Some(exit_code);
        self.stdout = Some(stdout.into());
        self.stderr = Some(stderr.into());
        self
    }

    /// Set variables
    pub fn with_variables(mut self, vars: HashMap<String, JsonValue>) -> Self {
        self.variables = vars;
        self
    }

    /// Set facts
    pub fn with_facts(mut self, facts: JsonValue) -> Self {
        self.facts = Some(facts);
        self
    }

    /// Set stack trace
    pub fn with_stack_trace(mut self, trace: impl Into<String>) -> Self {
        self.stack_trace = Some(trace.into());
        self
    }

    /// Add additional data
    pub fn with_data(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.additional_data.insert(key.into(), value);
        self
    }

    /// Get a summary of the failure for logging
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("[{}] {}", self.error_type, self.error_message)];

        if let Some(ref playbook) = self.playbook {
            parts.push(format!("Playbook: {}", playbook));
        }
        if let Some(ref task) = self.task {
            parts.push(format!("Task: {}", task));
        }
        if let Some(ref host) = self.host {
            parts.push(format!("Host: {}", host));
        }
        if let Some(code) = self.exit_code {
            parts.push(format!("Exit code: {}", code));
        }

        parts.join(" | ")
    }
}

impl Default for FailureContext {
    fn default() -> Self {
        Self::new("Unknown error", "unknown")
    }
}

/// A complete state dump
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDump {
    /// Dump format version
    pub version: String,
    /// When the dump was created
    pub created_at: DateTime<Utc>,
    /// Failure context
    pub failure: FailureContext,
    /// Execution history (recent events)
    pub history: Vec<super::ExecutionEvent>,
    /// All current variables
    pub all_variables: HashMap<String, JsonValue>,
    /// Debug state at time of failure
    pub debug_state: Option<super::DebugState>,
    /// Connection trace entries (recent)
    pub trace_entries: Vec<super::tracer::TraceEntry>,
    /// Watched variable values
    pub watched_variables: HashMap<String, JsonValue>,
    /// Active breakpoints
    pub breakpoints: Vec<super::breakpoint::Breakpoint>,
    /// System information
    pub system_info: SystemInfo,
}

impl StateDump {
    /// Create a new state dump
    pub fn new(failure: FailureContext) -> Self {
        Self {
            version: "1.0.0".to_string(),
            created_at: Utc::now(),
            failure,
            history: Vec::new(),
            all_variables: HashMap::new(),
            debug_state: None,
            trace_entries: Vec::new(),
            watched_variables: HashMap::new(),
            breakpoints: Vec::new(),
            system_info: SystemInfo::gather(),
        }
    }

    /// Set execution history
    pub fn with_history(mut self, history: Vec<super::ExecutionEvent>) -> Self {
        self.history = history;
        self
    }

    /// Set all variables
    pub fn with_variables(mut self, vars: HashMap<String, JsonValue>) -> Self {
        self.all_variables = vars;
        self
    }

    /// Set debug state
    pub fn with_debug_state(mut self, state: super::DebugState) -> Self {
        self.debug_state = Some(state);
        self
    }

    /// Set trace entries
    pub fn with_trace_entries(mut self, entries: Vec<super::tracer::TraceEntry>) -> Self {
        self.trace_entries = entries;
        self
    }

    /// Set watched variables
    pub fn with_watched_variables(mut self, vars: HashMap<String, JsonValue>) -> Self {
        self.watched_variables = vars;
        self
    }

    /// Set breakpoints
    pub fn with_breakpoints(mut self, breakpoints: Vec<super::breakpoint::Breakpoint>) -> Self {
        self.breakpoints = breakpoints;
        self
    }

    /// Serialize to bytes in the specified format
    pub fn to_bytes(&self, format: StateDumpFormat) -> Result<Vec<u8>, io::Error> {
        match format {
            StateDumpFormat::Json => {
                serde_json::to_vec(self).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
            StateDumpFormat::JsonPretty => serde_json::to_vec_pretty(self)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            StateDumpFormat::Yaml => {
                let yaml = serde_yaml::to_string(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Ok(yaml.into_bytes())
            }
            StateDumpFormat::Binary => {
                // For now, fall back to JSON for binary format
                // In a real implementation, this would use MessagePack or similar
                serde_json::to_vec(self).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
        }
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8], format: StateDumpFormat) -> Result<Self, io::Error> {
        match format {
            StateDumpFormat::Json | StateDumpFormat::JsonPretty => serde_json::from_slice(bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            StateDumpFormat::Yaml => {
                let s = std::str::from_utf8(bytes)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                serde_yaml::from_str(s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
            StateDumpFormat::Binary => serde_json::from_slice(bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        }
    }
}

/// System information for context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    /// Operating system
    pub os: String,
    /// OS version
    pub os_version: String,
    /// Architecture
    pub arch: String,
    /// Hostname
    pub hostname: String,
    /// Current working directory
    pub cwd: String,
    /// Rustible version
    pub rustible_version: String,
}

impl SystemInfo {
    /// Gather system information
    pub fn gather() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            os_version: std::env::consts::FAMILY.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            hostname: hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            rustible_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self::gather()
    }
}

/// State dumper for persisting failure state
#[derive(Debug)]
pub struct StateDumper {
    /// Whether dumping is enabled
    enabled: bool,
    /// Directory to write dumps to
    dump_dir: Option<PathBuf>,
    /// Format for dumps
    format: StateDumpFormat,
    /// Maximum number of dumps to keep
    max_dumps: usize,
    /// Whether to include sensitive data
    include_sensitive: bool,
}

impl StateDumper {
    /// Create a new state dumper
    pub fn new(enabled: bool, dump_dir: Option<PathBuf>) -> Self {
        Self {
            enabled,
            dump_dir,
            format: StateDumpFormat::JsonPretty,
            max_dumps: 10,
            include_sensitive: false,
        }
    }

    /// Set the dump format
    pub fn with_format(mut self, format: StateDumpFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the maximum number of dumps to keep
    pub fn with_max_dumps(mut self, max: usize) -> Self {
        self.max_dumps = max;
        self
    }

    /// Set whether to include sensitive data
    pub fn with_sensitive(mut self, include: bool) -> Self {
        self.include_sensitive = include;
        self
    }

    /// Check if dumping is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable dumping
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Dump failure state to a file
    pub fn dump(&self, context: FailureContext) -> Result<PathBuf, io::Error> {
        if !self.enabled {
            return Err(io::Error::other(
                "State dumping is disabled",
            ));
        }

        let dump = StateDump::new(context);
        self.write_dump(&dump)
    }

    /// Dump a complete state dump
    pub fn dump_full(&self, dump: &StateDump) -> Result<PathBuf, io::Error> {
        if !self.enabled {
            return Err(io::Error::other(
                "State dumping is disabled",
            ));
        }

        self.write_dump(dump)
    }

    /// Write a state dump to disk
    fn write_dump(&self, dump: &StateDump) -> Result<PathBuf, io::Error> {
        let dir = self.get_dump_directory()?;
        let filename = self.generate_filename(dump);
        let path = dir.join(filename);

        let bytes = dump.to_bytes(self.format)?;
        let mut file = File::create(&path)?;
        file.write_all(&bytes)?;

        // Cleanup old dumps
        self.cleanup_old_dumps(&dir)?;

        Ok(path)
    }

    /// Get or create the dump directory
    fn get_dump_directory(&self) -> Result<PathBuf, io::Error> {
        let dir = match &self.dump_dir {
            Some(path) => path.clone(),
            None => {
                // Default to ~/.rustible/dumps or /tmp/rustible-dumps
                dirs::home_dir()
                    .map(|h| h.join(".rustible").join("dumps"))
                    .unwrap_or_else(|| PathBuf::from("/tmp/rustible-dumps"))
            }
        };

        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Generate a filename for a dump
    fn generate_filename(&self, dump: &StateDump) -> String {
        let timestamp = dump.created_at.format("%Y%m%d_%H%M%S");
        let host = dump
            .failure
            .host
            .as_deref()
            .unwrap_or("unknown")
            .replace(['/', '\\', ':'], "_");
        format!(
            "rustible_dump_{}_{}.{}",
            timestamp,
            host,
            self.format.extension()
        )
    }

    /// Cleanup old dump files
    fn cleanup_old_dumps(&self, dir: &PathBuf) -> Result<(), io::Error> {
        let mut dumps: Vec<_> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("rustible_dump_")
            })
            .collect();

        if dumps.len() <= self.max_dumps {
            return Ok(());
        }

        // Sort by modification time (oldest first)
        dumps.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });

        // Remove oldest dumps
        let to_remove = dumps.len() - self.max_dumps;
        for entry in dumps.into_iter().take(to_remove) {
            let _ = fs::remove_file(entry.path());
        }

        Ok(())
    }
}

impl Default for StateDumper {
    fn default() -> Self {
        Self::new(false, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_failure_context() {
        let context = FailureContext::new("Connection refused", "ConnectionError")
            .with_host("web1")
            .with_task("Install nginx")
            .with_output(1, "stdout", "stderr");

        assert_eq!(context.error_message, "Connection refused");
        assert_eq!(context.host, Some("web1".to_string()));
        assert_eq!(context.exit_code, Some(1));
    }

    #[test]
    fn test_state_dump_serialization() {
        let context = FailureContext::new("Test error", "TestError");
        let dump = StateDump::new(context);

        let json = dump.to_bytes(StateDumpFormat::Json).unwrap();
        let restored = StateDump::from_bytes(&json, StateDumpFormat::Json).unwrap();

        assert_eq!(restored.failure.error_message, "Test error");
    }

    #[test]
    fn test_state_dumper() {
        let dir = tempdir().unwrap();
        let dumper = StateDumper::new(true, Some(dir.path().to_path_buf()));

        let context = FailureContext::new("Test failure", "TestError").with_host("testhost");

        let path = dumper.dump(context).unwrap();
        assert!(path.exists());

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("Test failure"));
    }

    #[test]
    fn test_dumper_disabled() {
        let dumper = StateDumper::new(false, None);
        let context = FailureContext::new("Error", "Error");

        let result = dumper.dump(context);
        assert!(result.is_err());
    }

    #[test]
    fn test_system_info() {
        let info = SystemInfo::gather();
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
    }
}
