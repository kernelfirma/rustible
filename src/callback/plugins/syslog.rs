//! Syslog Callback Plugin for Rustible.
//!
//! This plugin logs execution events to the system syslog daemon,
//! providing integration with centralized logging infrastructure.
//!
//! # Features
//!
//! - Configurable syslog facility (local0-local7, daemon, user, etc.)
//! - Configurable priority/severity levels per event type
//! - Structured log format with JSON support
//! - Works on Linux/Unix systems via the native syslog API
//! - Thread-safe design for concurrent task execution
//!
//! # Example Output (in /var/log/syslog or similar)
//!
//! ```text
//! Dec 25 10:30:15 server rustible[12345]: {"event":"playbook_start","name":"deploy.yml","timestamp":"2025-12-25T10:30:15Z"}
//! Dec 25 10:30:16 server rustible[12345]: {"event":"task_complete","task":"Install nginx","host":"web1","status":"changed","duration_ms":1523}
//! Dec 25 10:30:17 server rustible[12345]: {"event":"task_complete","task":"Configure nginx","host":"web1","status":"failed","error":"File not found"}
//! ```
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::{SyslogCallback, SyslogConfig, SyslogFacility};
//!
//! let config = SyslogConfig::builder()
//!     .facility(SyslogFacility::Local0)
//!     .ident("rustible")
//!     .include_host(true)
//!     .build();
//!
//! let callback = SyslogCallback::new(config)?;
//! # let _ = ();
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use serde::Serialize;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Syslog Facility and Priority Definitions
// ============================================================================

/// Syslog facility codes as defined in RFC 5424.
///
/// Facilities allow categorizing log messages by source type.
/// For application-specific logging, use `Local0` through `Local7`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[derive(Default)]
pub enum SyslogFacility {
    /// Kernel messages
    Kern = 0,
    /// User-level messages
    User = 1,
    /// Mail system
    Mail = 2,
    /// System daemons
    Daemon = 3,
    /// Security/authorization messages
    Auth = 4,
    /// Messages generated internally by syslogd
    Syslog = 5,
    /// Line printer subsystem
    Lpr = 6,
    /// Network news subsystem
    News = 7,
    /// UUCP subsystem
    Uucp = 8,
    /// Clock daemon
    Cron = 9,
    /// Security/authorization messages (private)
    AuthPriv = 10,
    /// FTP daemon
    Ftp = 11,
    /// NTP subsystem
    Ntp = 12,
    /// Log audit
    LogAudit = 13,
    /// Log alert
    LogAlert = 14,
    /// Clock daemon (note 2)
    Clock = 15,
    /// Local use 0 (recommended for applications)
    #[default]
    Local0 = 16,
    /// Local use 1
    Local1 = 17,
    /// Local use 2
    Local2 = 18,
    /// Local use 3
    Local3 = 19,
    /// Local use 4
    Local4 = 20,
    /// Local use 5
    Local5 = 21,
    /// Local use 6
    Local6 = 22,
    /// Local use 7
    Local7 = 23,
}


impl std::fmt::Display for SyslogFacility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyslogFacility::Kern => write!(f, "kern"),
            SyslogFacility::User => write!(f, "user"),
            SyslogFacility::Mail => write!(f, "mail"),
            SyslogFacility::Daemon => write!(f, "daemon"),
            SyslogFacility::Auth => write!(f, "auth"),
            SyslogFacility::Syslog => write!(f, "syslog"),
            SyslogFacility::Lpr => write!(f, "lpr"),
            SyslogFacility::News => write!(f, "news"),
            SyslogFacility::Uucp => write!(f, "uucp"),
            SyslogFacility::Cron => write!(f, "cron"),
            SyslogFacility::AuthPriv => write!(f, "authpriv"),
            SyslogFacility::Ftp => write!(f, "ftp"),
            SyslogFacility::Ntp => write!(f, "ntp"),
            SyslogFacility::LogAudit => write!(f, "logaudit"),
            SyslogFacility::LogAlert => write!(f, "logalert"),
            SyslogFacility::Clock => write!(f, "clock"),
            SyslogFacility::Local0 => write!(f, "local0"),
            SyslogFacility::Local1 => write!(f, "local1"),
            SyslogFacility::Local2 => write!(f, "local2"),
            SyslogFacility::Local3 => write!(f, "local3"),
            SyslogFacility::Local4 => write!(f, "local4"),
            SyslogFacility::Local5 => write!(f, "local5"),
            SyslogFacility::Local6 => write!(f, "local6"),
            SyslogFacility::Local7 => write!(f, "local7"),
        }
    }
}

impl std::str::FromStr for SyslogFacility {
    type Err = SyslogError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "kern" | "kernel" => Ok(SyslogFacility::Kern),
            "user" => Ok(SyslogFacility::User),
            "mail" => Ok(SyslogFacility::Mail),
            "daemon" => Ok(SyslogFacility::Daemon),
            "auth" => Ok(SyslogFacility::Auth),
            "syslog" => Ok(SyslogFacility::Syslog),
            "lpr" => Ok(SyslogFacility::Lpr),
            "news" => Ok(SyslogFacility::News),
            "uucp" => Ok(SyslogFacility::Uucp),
            "cron" => Ok(SyslogFacility::Cron),
            "authpriv" => Ok(SyslogFacility::AuthPriv),
            "ftp" => Ok(SyslogFacility::Ftp),
            "ntp" => Ok(SyslogFacility::Ntp),
            "logaudit" => Ok(SyslogFacility::LogAudit),
            "logalert" => Ok(SyslogFacility::LogAlert),
            "clock" => Ok(SyslogFacility::Clock),
            "local0" => Ok(SyslogFacility::Local0),
            "local1" => Ok(SyslogFacility::Local1),
            "local2" => Ok(SyslogFacility::Local2),
            "local3" => Ok(SyslogFacility::Local3),
            "local4" => Ok(SyslogFacility::Local4),
            "local5" => Ok(SyslogFacility::Local5),
            "local6" => Ok(SyslogFacility::Local6),
            "local7" => Ok(SyslogFacility::Local7),
            _ => Err(SyslogError::InvalidFacility(s.to_string())),
        }
    }
}

/// Syslog severity/priority levels as defined in RFC 5424.
///
/// Lower numbers indicate higher severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
#[derive(Default)]
pub enum SyslogSeverity {
    /// System is unusable
    Emergency = 0,
    /// Action must be taken immediately
    Alert = 1,
    /// Critical conditions
    Critical = 2,
    /// Error conditions
    Error = 3,
    /// Warning conditions
    Warning = 4,
    /// Normal but significant condition
    Notice = 5,
    /// Informational messages
    #[default]
    Info = 6,
    /// Debug-level messages
    Debug = 7,
}


impl std::fmt::Display for SyslogSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyslogSeverity::Emergency => write!(f, "emerg"),
            SyslogSeverity::Alert => write!(f, "alert"),
            SyslogSeverity::Critical => write!(f, "crit"),
            SyslogSeverity::Error => write!(f, "err"),
            SyslogSeverity::Warning => write!(f, "warning"),
            SyslogSeverity::Notice => write!(f, "notice"),
            SyslogSeverity::Info => write!(f, "info"),
            SyslogSeverity::Debug => write!(f, "debug"),
        }
    }
}

impl std::str::FromStr for SyslogSeverity {
    type Err = SyslogError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "emerg" | "emergency" | "panic" => Ok(SyslogSeverity::Emergency),
            "alert" => Ok(SyslogSeverity::Alert),
            "crit" | "critical" => Ok(SyslogSeverity::Critical),
            "err" | "error" => Ok(SyslogSeverity::Error),
            "warn" | "warning" => Ok(SyslogSeverity::Warning),
            "notice" => Ok(SyslogSeverity::Notice),
            "info" | "informational" => Ok(SyslogSeverity::Info),
            "debug" => Ok(SyslogSeverity::Debug),
            _ => Err(SyslogError::InvalidSeverity(s.to_string())),
        }
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during syslog operations.
#[derive(Debug, thiserror::Error)]
pub enum SyslogError {
    /// Failed to open syslog connection
    #[error("Failed to open syslog: {0}")]
    OpenFailed(String),

    /// Failed to write to syslog
    #[error("Failed to write to syslog: {0}")]
    WriteFailed(#[from] io::Error),

    /// Invalid facility string
    #[error("Invalid syslog facility: {0}")]
    InvalidFacility(String),

    /// Invalid severity string
    #[error("Invalid syslog severity: {0}")]
    InvalidSeverity(String),

    /// JSON serialization error
    #[error("Failed to serialize log entry: {0}")]
    SerializationFailed(#[from] serde_json::Error),

    /// Syslog not available on this platform
    #[error("Syslog is not available on this platform")]
    NotAvailable,
}

/// Result type for syslog operations.
pub type SyslogResult<T> = Result<T, SyslogError>;

// ============================================================================
// Configuration
// ============================================================================

/// Log format for syslog messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyslogFormat {
    /// JSON structured format (recommended for log aggregation)
    #[default]
    Json,
    /// Human-readable text format
    Text,
    /// CEF (Common Event Format) for SIEM integration
    Cef,
}

/// Severity mapping for different event types.
#[derive(Debug, Clone)]
pub struct SeverityMapping {
    /// Severity for playbook start events
    pub playbook_start: SyslogSeverity,
    /// Severity for playbook end (success) events
    pub playbook_end_success: SyslogSeverity,
    /// Severity for playbook end (failure) events
    pub playbook_end_failure: SyslogSeverity,
    /// Severity for play start events
    pub play_start: SyslogSeverity,
    /// Severity for play end events
    pub play_end: SyslogSeverity,
    /// Severity for task start events
    pub task_start: SyslogSeverity,
    /// Severity for task ok events
    pub task_ok: SyslogSeverity,
    /// Severity for task changed events
    pub task_changed: SyslogSeverity,
    /// Severity for task failed events
    pub task_failed: SyslogSeverity,
    /// Severity for task skipped events
    pub task_skipped: SyslogSeverity,
    /// Severity for handler triggered events
    pub handler_triggered: SyslogSeverity,
    /// Severity for facts gathered events
    pub facts_gathered: SyslogSeverity,
}

impl Default for SeverityMapping {
    fn default() -> Self {
        Self {
            playbook_start: SyslogSeverity::Info,
            playbook_end_success: SyslogSeverity::Notice,
            playbook_end_failure: SyslogSeverity::Error,
            play_start: SyslogSeverity::Info,
            play_end: SyslogSeverity::Info,
            task_start: SyslogSeverity::Debug,
            task_ok: SyslogSeverity::Info,
            task_changed: SyslogSeverity::Notice,
            task_failed: SyslogSeverity::Error,
            task_skipped: SyslogSeverity::Debug,
            handler_triggered: SyslogSeverity::Debug,
            facts_gathered: SyslogSeverity::Debug,
        }
    }
}

/// Configuration for the syslog callback.
#[derive(Debug, Clone)]
pub struct SyslogConfig {
    /// Syslog facility to use
    pub facility: SyslogFacility,
    /// Program identifier (appears in log messages)
    pub ident: String,
    /// Log format
    pub format: SyslogFormat,
    /// Whether to include PID in syslog messages
    pub include_pid: bool,
    /// Whether to include hostname in log entries
    pub include_host: bool,
    /// Whether to log to console as well (LOG_CONS)
    pub log_to_console: bool,
    /// Whether to log task start events
    pub log_task_start: bool,
    /// Whether to log facts gathered events
    pub log_facts: bool,
    /// Minimum severity to log (less severe events are ignored)
    pub min_severity: SyslogSeverity,
    /// Severity mapping for different event types
    pub severity_mapping: SeverityMapping,
    /// Additional static fields to include in every log entry
    pub extra_fields: HashMap<String, String>,
    /// CEF vendor name (for CEF format)
    pub cef_vendor: String,
    /// CEF product name (for CEF format)
    pub cef_product: String,
    /// CEF product version (for CEF format)
    pub cef_version: String,
}

impl Default for SyslogConfig {
    fn default() -> Self {
        Self {
            facility: SyslogFacility::Local0,
            ident: "rustible".to_string(),
            format: SyslogFormat::Json,
            include_pid: true,
            include_host: true,
            log_to_console: false,
            log_task_start: false,
            log_facts: false,
            min_severity: SyslogSeverity::Debug,
            severity_mapping: SeverityMapping::default(),
            extra_fields: HashMap::new(),
            cef_vendor: "Rustible".to_string(),
            cef_product: "Rustible".to_string(),
            cef_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl SyslogConfig {
    /// Creates a new configuration builder.
    pub fn builder() -> SyslogConfigBuilder {
        SyslogConfigBuilder::default()
    }
}

/// Builder for [`SyslogConfig`].
#[derive(Debug, Clone, Default)]
pub struct SyslogConfigBuilder {
    config: SyslogConfig,
}

impl SyslogConfigBuilder {
    /// Sets the syslog facility.
    pub fn facility(mut self, facility: SyslogFacility) -> Self {
        self.config.facility = facility;
        self
    }

    /// Sets the program identifier.
    pub fn ident(mut self, ident: impl Into<String>) -> Self {
        self.config.ident = ident.into();
        self
    }

    /// Sets the log format.
    pub fn format(mut self, format: SyslogFormat) -> Self {
        self.config.format = format;
        self
    }

    /// Sets whether to include PID in messages.
    pub fn include_pid(mut self, include: bool) -> Self {
        self.config.include_pid = include;
        self
    }

    /// Sets whether to include hostname in entries.
    pub fn include_host(mut self, include: bool) -> Self {
        self.config.include_host = include;
        self
    }

    /// Sets whether to also log to console.
    pub fn log_to_console(mut self, log: bool) -> Self {
        self.config.log_to_console = log;
        self
    }

    /// Sets whether to log task start events.
    pub fn log_task_start(mut self, log: bool) -> Self {
        self.config.log_task_start = log;
        self
    }

    /// Sets whether to log facts gathered events.
    pub fn log_facts(mut self, log: bool) -> Self {
        self.config.log_facts = log;
        self
    }

    /// Sets the minimum severity level to log.
    pub fn min_severity(mut self, severity: SyslogSeverity) -> Self {
        self.config.min_severity = severity;
        self
    }

    /// Sets the severity mapping.
    pub fn severity_mapping(mut self, mapping: SeverityMapping) -> Self {
        self.config.severity_mapping = mapping;
        self
    }

    /// Adds an extra field to include in every log entry.
    pub fn extra_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.extra_fields.insert(key.into(), value.into());
        self
    }

    /// Sets CEF format metadata.
    pub fn cef_metadata(
        mut self,
        vendor: impl Into<String>,
        product: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        self.config.cef_vendor = vendor.into();
        self.config.cef_product = product.into();
        self.config.cef_version = version.into();
        self
    }

    /// Builds the configuration.
    pub fn build(self) -> SyslogConfig {
        self.config
    }
}

// ============================================================================
// Log Entry Structures
// ============================================================================

/// Structured log entry for JSON format.
#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct LogEntry {
    /// Event type
    event: String,
    /// ISO 8601 timestamp
    timestamp: String,
    /// Event-specific fields
    #[serde(flatten)]
    fields: serde_json::Value,
    /// Extra static fields from config
    #[serde(flatten)]
    extra: HashMap<String, String>,
}

// ============================================================================
// Syslog Writer Abstraction
// ============================================================================

/// Abstraction over syslog writing for testability and platform compatibility.
trait SyslogWriter: Send + Sync + std::fmt::Debug {
    /// Writes a message to syslog with the given priority.
    fn write(&self, priority: u8, message: &str) -> SyslogResult<()>;

    /// Closes the syslog connection.
    fn close(&self);
}

/// Native Unix syslog writer using libc.
#[cfg(unix)]
#[derive(Debug)]
struct UnixSyslogWriter {
    /// Program identifier (must live as long as syslog is open)
    _ident: std::ffi::CString,
}

#[cfg(unix)]
impl UnixSyslogWriter {
    fn new(ident: &str, options: libc::c_int, facility: SyslogFacility) -> SyslogResult<Self> {
        let c_ident =
            std::ffi::CString::new(ident).map_err(|e| SyslogError::OpenFailed(e.to_string()))?;

        unsafe {
            libc::openlog(c_ident.as_ptr(), options, (facility as libc::c_int) << 3);
        }

        Ok(Self { _ident: c_ident })
    }
}

#[cfg(unix)]
impl SyslogWriter for UnixSyslogWriter {
    fn write(&self, priority: u8, message: &str) -> SyslogResult<()> {
        let c_message = std::ffi::CString::new(message)
            .map_err(|e| SyslogError::WriteFailed(io::Error::new(io::ErrorKind::InvalidData, e)))?;

        unsafe {
            libc::syslog(
                priority as libc::c_int,
                c"%s".as_ptr(),
                c_message.as_ptr(),
            );
        }

        Ok(())
    }

    fn close(&self) {
        unsafe {
            libc::closelog();
        }
    }
}

#[cfg(unix)]
impl Drop for UnixSyslogWriter {
    fn drop(&mut self) {
        self.close();
    }
}

/// Fallback writer that writes to stderr (for non-Unix or testing).
#[derive(Debug)]
#[allow(dead_code)]
struct StderrSyslogWriter {
    ident: String,
    facility: SyslogFacility,
}

impl StderrSyslogWriter {
    #[allow(dead_code)]
    fn new(ident: &str, facility: SyslogFacility) -> Self {
        Self {
            ident: ident.to_string(),
            facility,
        }
    }
}

impl SyslogWriter for StderrSyslogWriter {
    fn write(&self, priority: u8, message: &str) -> SyslogResult<()> {
        let severity = priority & 0x07;
        let severity_str = match severity {
            0 => "EMERG",
            1 => "ALERT",
            2 => "CRIT",
            3 => "ERROR",
            4 => "WARN",
            5 => "NOTICE",
            6 => "INFO",
            7 => "DEBUG",
            _ => "UNKNOWN",
        };

        let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");

        writeln!(
            io::stderr(),
            "{} {} {} [{}]: {}",
            timestamp,
            self.facility,
            self.ident,
            severity_str,
            message
        )?;

        Ok(())
    }

    fn close(&self) {
        // No-op for stderr
    }
}

// ============================================================================
// Syslog Callback Implementation
// ============================================================================

/// Syslog callback plugin that logs execution events to syslog.
///
/// This callback integrates Rustible with system logging infrastructure,
/// enabling centralized log collection and monitoring.
///
/// # Platform Support
///
/// - **Linux/Unix**: Uses native syslog via libc
/// - **Other platforms**: Falls back to stderr logging
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::{SyslogCallback, SyslogConfig};
///
/// let callback = SyslogCallback::new(SyslogConfig::default())?;
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SyslogCallback {
    /// Configuration
    config: SyslogConfig,
    /// Syslog writer
    writer: Arc<dyn SyslogWriter>,
    /// Playbook start time for duration tracking
    playbook_start: RwLock<Option<Instant>>,
    /// Current playbook name
    current_playbook: RwLock<Option<String>>,
    /// Hostname (cached for performance)
    hostname: String,
    /// Statistics
    stats: RwLock<SyslogStats>,
}

/// Statistics tracked by the syslog callback.
#[derive(Debug, Default, Clone)]
pub struct SyslogStats {
    /// Total messages logged
    pub messages_logged: u64,
    /// Messages dropped due to min severity filter
    pub messages_filtered: u64,
    /// Write errors encountered
    pub write_errors: u64,
}

impl SyslogCallback {
    /// Creates a new syslog callback with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if syslog cannot be opened.
    pub fn new(config: SyslogConfig) -> SyslogResult<Self> {
        let writer = Self::create_writer(&config)?;
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        Ok(Self {
            config,
            writer,
            playbook_start: RwLock::new(None),
            current_playbook: RwLock::new(None),
            hostname,
            stats: RwLock::new(SyslogStats::default()),
        })
    }

    /// Creates a new syslog callback with default configuration.
    pub fn with_defaults() -> SyslogResult<Self> {
        Self::new(SyslogConfig::default())
    }

    /// Creates the appropriate syslog writer for the platform.
    #[cfg(unix)]
    fn create_writer(config: &SyslogConfig) -> SyslogResult<Arc<dyn SyslogWriter>> {
        let mut options = libc::LOG_NDELAY;
        if config.include_pid {
            options |= libc::LOG_PID;
        }
        if config.log_to_console {
            options |= libc::LOG_CONS;
        }

        let writer = UnixSyslogWriter::new(&config.ident, options, config.facility)?;
        Ok(Arc::new(writer))
    }

    #[cfg(not(unix))]
    fn create_writer(config: &SyslogConfig) -> SyslogResult<Arc<dyn SyslogWriter>> {
        Ok(Arc::new(StderrSyslogWriter::new(
            &config.ident,
            config.facility,
        )))
    }

    /// Returns the current statistics.
    pub fn stats(&self) -> SyslogStats {
        self.stats.read().clone()
    }

    /// Calculates the syslog priority value from facility and severity.
    fn calculate_priority(&self, severity: SyslogSeverity) -> u8 {
        ((self.config.facility as u8) << 3) | (severity as u8)
    }

    /// Checks if a message with the given severity should be logged.
    fn should_log(&self, severity: SyslogSeverity) -> bool {
        severity <= self.config.min_severity
    }

    /// Logs an event with the given severity and fields.
    fn log_event(&self, event: &str, severity: SyslogSeverity, fields: serde_json::Value) {
        if !self.should_log(severity) {
            let mut stats = self.stats.write();
            stats.messages_filtered += 1;
            return;
        }

        let message = self.format_message(event, &fields);
        let priority = self.calculate_priority(severity);

        if let Err(e) = self.writer.write(priority, &message) {
            // Log error to stderr but don't fail
            eprintln!("Syslog write error: {}", e);
            let mut stats = self.stats.write();
            stats.write_errors += 1;
        } else {
            let mut stats = self.stats.write();
            stats.messages_logged += 1;
        }
    }

    /// Formats a message according to the configured format.
    fn format_message(&self, event: &str, fields: &serde_json::Value) -> String {
        match self.config.format {
            SyslogFormat::Json => self.format_json(event, fields),
            SyslogFormat::Text => self.format_text(event, fields),
            SyslogFormat::Cef => self.format_cef(event, fields),
        }
    }

    /// Formats a message as JSON.
    fn format_json(&self, event: &str, fields: &serde_json::Value) -> String {
        let mut entry = serde_json::json!({
            "event": event,
            "timestamp": Utc::now().to_rfc3339(),
        });

        // Add hostname if configured
        if self.config.include_host {
            entry["host"] = serde_json::json!(self.hostname);
        }

        // Merge in event-specific fields
        if let serde_json::Value::Object(ref map) = fields {
            if let serde_json::Value::Object(ref mut entry_map) = entry {
                for (k, v) in map {
                    entry_map.insert(k.clone(), v.clone());
                }
            }
        }

        // Add extra fields
        if let serde_json::Value::Object(ref mut entry_map) = entry {
            for (k, v) in &self.config.extra_fields {
                entry_map.insert(k.clone(), serde_json::json!(v));
            }
        }

        serde_json::to_string(&entry).unwrap_or_else(|_| format!("{{\"event\":\"{}\"}}", event))
    }

    /// Formats a message as human-readable text.
    fn format_text(&self, event: &str, fields: &serde_json::Value) -> String {
        let mut parts = vec![event.to_string()];

        if let serde_json::Value::Object(map) = fields {
            for (k, v) in map {
                let value_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    _ => v.to_string(),
                };
                parts.push(format!("{}={}", k, value_str));
            }
        }

        if self.config.include_host {
            parts.push(format!("host={}", self.hostname));
        }

        parts.join(" ")
    }

    /// Formats a message in CEF (Common Event Format).
    fn format_cef(&self, event: &str, fields: &serde_json::Value) -> String {
        // CEF format: CEF:Version|Device Vendor|Device Product|Device Version|Signature ID|Name|Severity|Extension
        let severity = self.event_to_cef_severity(event);
        let signature_id = self.event_to_signature_id(event);

        let mut extension = String::new();

        if let serde_json::Value::Object(map) = fields {
            for (k, v) in map {
                let value_str = match v {
                    serde_json::Value::String(s) => Self::escape_cef_value(s),
                    _ => Self::escape_cef_value(&v.to_string()),
                };
                if !extension.is_empty() {
                    extension.push(' ');
                }
                extension.push_str(&format!("{}={}", Self::escape_cef_key(k), value_str));
            }
        }

        if self.config.include_host {
            if !extension.is_empty() {
                extension.push(' ');
            }
            extension.push_str(&format!("dhost={}", self.hostname));
        }

        format!(
            "CEF:0|{}|{}|{}|{}|{}|{}|{}",
            Self::escape_cef_header(&self.config.cef_vendor),
            Self::escape_cef_header(&self.config.cef_product),
            Self::escape_cef_header(&self.config.cef_version),
            signature_id,
            Self::escape_cef_header(event),
            severity,
            extension
        )
    }

    /// Maps event type to CEF severity (0-10).
    fn event_to_cef_severity(&self, event: &str) -> u8 {
        match event {
            "task_failed" | "playbook_end_failure" => 7,
            "task_changed" => 3,
            "playbook_start" | "playbook_end_success" => 1,
            _ => 0,
        }
    }

    /// Maps event type to a signature ID for CEF.
    fn event_to_signature_id(&self, event: &str) -> u32 {
        match event {
            "playbook_start" => 1001,
            "playbook_end" => 1002,
            "play_start" => 2001,
            "play_end" => 2002,
            "task_start" => 3001,
            "task_ok" => 3002,
            "task_changed" => 3003,
            "task_failed" => 3004,
            "task_skipped" => 3005,
            "handler_triggered" => 4001,
            "facts_gathered" => 5001,
            _ => 9999,
        }
    }

    /// Escapes a CEF header field.
    fn escape_cef_header(s: &str) -> String {
        s.replace('\\', "\\\\").replace('|', "\\|")
    }

    /// Escapes a CEF extension key.
    fn escape_cef_key(s: &str) -> String {
        // CEF keys should be alphanumeric
        s.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect()
    }

    /// Escapes a CEF extension value.
    fn escape_cef_value(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('=', "\\=")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }
}

#[async_trait]
impl ExecutionCallback for SyslogCallback {
    async fn on_playbook_start(&self, name: &str) {
        *self.playbook_start.write() = Some(Instant::now());
        *self.current_playbook.write() = Some(name.to_string());

        self.log_event(
            "playbook_start",
            self.config.severity_mapping.playbook_start,
            serde_json::json!({
                "playbook": name,
            }),
        );
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let duration_ms = self
            .playbook_start
            .read()
            .map(|start| start.elapsed().as_millis() as u64);

        let severity = if success {
            self.config.severity_mapping.playbook_end_success
        } else {
            self.config.severity_mapping.playbook_end_failure
        };

        let mut fields = serde_json::json!({
            "playbook": name,
            "success": success,
            "status": if success { "success" } else { "failed" },
        });

        if let Some(ms) = duration_ms {
            fields["duration_ms"] = serde_json::json!(ms);
        }

        self.log_event("playbook_end", severity, fields);

        // Clear state
        *self.playbook_start.write() = None;
        *self.current_playbook.write() = None;
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.log_event(
            "play_start",
            self.config.severity_mapping.play_start,
            serde_json::json!({
                "play": name,
                "host_count": hosts.len(),
                "hosts": hosts,
            }),
        );
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        self.log_event(
            "play_end",
            self.config.severity_mapping.play_end,
            serde_json::json!({
                "play": name,
                "success": success,
                "status": if success { "success" } else { "failed" },
            }),
        );
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        if !self.config.log_task_start {
            return;
        }

        self.log_event(
            "task_start",
            self.config.severity_mapping.task_start,
            serde_json::json!({
                "task": name,
                "target_host": host,
            }),
        );
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let (event, severity) = if result.result.skipped {
            ("task_skipped", self.config.severity_mapping.task_skipped)
        } else if !result.result.success {
            ("task_failed", self.config.severity_mapping.task_failed)
        } else if result.result.changed {
            ("task_changed", self.config.severity_mapping.task_changed)
        } else {
            ("task_ok", self.config.severity_mapping.task_ok)
        };

        let status = if result.result.skipped {
            "skipped"
        } else if !result.result.success {
            "failed"
        } else if result.result.changed {
            "changed"
        } else {
            "ok"
        };

        let mut fields = serde_json::json!({
            "task": result.task_name,
            "target_host": result.host,
            "status": status,
            "success": result.result.success,
            "changed": result.result.changed,
            "skipped": result.result.skipped,
            "duration_ms": result.duration.as_millis() as u64,
        });

        // Add message for failures
        if !result.result.success {
            fields["error"] = serde_json::json!(result.result.message);
        }

        // Add warnings if present
        if !result.result.warnings.is_empty() {
            fields["warnings"] = serde_json::json!(result.result.warnings);
        }

        // Add handlers to notify if present
        if !result.notify.is_empty() {
            fields["notify"] = serde_json::json!(result.notify);
        }

        self.log_event(event, severity, fields);
    }

    async fn on_handler_triggered(&self, name: &str) {
        self.log_event(
            "handler_triggered",
            self.config.severity_mapping.handler_triggered,
            serde_json::json!({
                "handler": name,
            }),
        );
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        if !self.config.log_facts {
            return;
        }

        // Only log minimal fact info to avoid bloating logs
        let fact_count = facts.all().len();

        self.log_event(
            "facts_gathered",
            self.config.severity_mapping.facts_gathered,
            serde_json::json!({
                "target_host": host,
                "fact_count": fact_count,
            }),
        );
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

    /// Test syslog writer that captures messages for verification.
    #[derive(Debug)]
    struct TestSyslogWriter {
        messages: Arc<RwLock<Vec<(u8, String)>>>,
    }

    impl TestSyslogWriter {
        fn new() -> Self {
            Self {
                messages: Arc::new(RwLock::new(Vec::new())),
            }
        }

        fn get_messages(&self) -> Vec<(u8, String)> {
            self.messages.read().clone()
        }
    }

    impl SyslogWriter for TestSyslogWriter {
        fn write(&self, priority: u8, message: &str) -> SyslogResult<()> {
            self.messages.write().push((priority, message.to_string()));
            Ok(())
        }

        fn close(&self) {}
    }

    fn create_test_callback() -> (SyslogCallback, Arc<TestSyslogWriter>) {
        let writer = Arc::new(TestSyslogWriter::new());
        let callback = SyslogCallback {
            config: SyslogConfig::default(),
            writer: Arc::clone(&writer) as Arc<dyn SyslogWriter>,
            playbook_start: RwLock::new(None),
            current_playbook: RwLock::new(None),
            hostname: "testhost".to_string(),
            stats: RwLock::new(SyslogStats::default()),
        };
        (callback, writer)
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
    fn test_facility_parsing() {
        assert_eq!(
            "local0".parse::<SyslogFacility>().unwrap(),
            SyslogFacility::Local0
        );
        assert_eq!(
            "daemon".parse::<SyslogFacility>().unwrap(),
            SyslogFacility::Daemon
        );
        assert_eq!(
            "user".parse::<SyslogFacility>().unwrap(),
            SyslogFacility::User
        );
        assert!("invalid".parse::<SyslogFacility>().is_err());
    }

    #[test]
    fn test_severity_parsing() {
        assert_eq!(
            "info".parse::<SyslogSeverity>().unwrap(),
            SyslogSeverity::Info
        );
        assert_eq!(
            "error".parse::<SyslogSeverity>().unwrap(),
            SyslogSeverity::Error
        );
        assert_eq!(
            "warn".parse::<SyslogSeverity>().unwrap(),
            SyslogSeverity::Warning
        );
        assert!("invalid".parse::<SyslogSeverity>().is_err());
    }

    #[test]
    fn test_priority_calculation() {
        let (callback, _) = create_test_callback();

        // local0 (16) << 3 = 128, + info (6) = 134
        assert_eq!(callback.calculate_priority(SyslogSeverity::Info), 134);

        // local0 (16) << 3 = 128, + error (3) = 131
        assert_eq!(callback.calculate_priority(SyslogSeverity::Error), 131);
    }

    #[test]
    fn test_severity_filtering() {
        let mut config = SyslogConfig::default();
        config.min_severity = SyslogSeverity::Warning;

        let callback = SyslogCallback {
            config,
            writer: Arc::new(TestSyslogWriter::new()),
            playbook_start: RwLock::new(None),
            current_playbook: RwLock::new(None),
            hostname: "testhost".to_string(),
            stats: RwLock::new(SyslogStats::default()),
        };

        // Should log
        assert!(callback.should_log(SyslogSeverity::Emergency));
        assert!(callback.should_log(SyslogSeverity::Error));
        assert!(callback.should_log(SyslogSeverity::Warning));

        // Should not log
        assert!(!callback.should_log(SyslogSeverity::Notice));
        assert!(!callback.should_log(SyslogSeverity::Info));
        assert!(!callback.should_log(SyslogSeverity::Debug));
    }

    #[test]
    fn test_json_format() {
        let (callback, _) = create_test_callback();

        let fields = serde_json::json!({
            "task": "test_task",
            "host": "host1"
        });

        let message = callback.format_json("task_complete", &fields);
        let parsed: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(parsed["event"], "task_complete");
        assert_eq!(parsed["task"], "test_task");
        assert!(parsed["timestamp"].is_string());
    }

    #[test]
    fn test_text_format() {
        let mut config = SyslogConfig::default();
        config.format = SyslogFormat::Text;
        config.include_host = false;

        let callback = SyslogCallback {
            config,
            writer: Arc::new(TestSyslogWriter::new()),
            playbook_start: RwLock::new(None),
            current_playbook: RwLock::new(None),
            hostname: "testhost".to_string(),
            stats: RwLock::new(SyslogStats::default()),
        };

        let fields = serde_json::json!({
            "task": "test_task",
            "status": "changed"
        });

        let message = callback.format_text("task_complete", &fields);

        assert!(message.contains("task_complete"));
        assert!(message.contains("task=test_task"));
        assert!(message.contains("status=changed"));
    }

    #[test]
    fn test_cef_format() {
        let mut config = SyslogConfig::default();
        config.format = SyslogFormat::Cef;

        let callback = SyslogCallback {
            config,
            writer: Arc::new(TestSyslogWriter::new()),
            playbook_start: RwLock::new(None),
            current_playbook: RwLock::new(None),
            hostname: "testhost".to_string(),
            stats: RwLock::new(SyslogStats::default()),
        };

        let fields = serde_json::json!({
            "task": "test_task"
        });

        let message = callback.format_cef("task_failed", &fields);

        assert!(message.starts_with("CEF:0|"));
        assert!(message.contains("Rustible"));
        assert!(message.contains("task=test_task"));
    }

    #[test]
    fn test_cef_escaping() {
        assert_eq!(
            SyslogCallback::escape_cef_header("test|value"),
            "test\\|value"
        );
        assert_eq!(SyslogCallback::escape_cef_value("key=value"), "key\\=value");
        assert_eq!(
            SyslogCallback::escape_cef_value("line1\nline2"),
            "line1\\nline2"
        );
    }

    #[tokio::test]
    async fn test_playbook_lifecycle() {
        let (callback, writer) = create_test_callback();

        callback.on_playbook_start("test-playbook").await;
        callback.on_playbook_end("test-playbook", true).await;

        let messages = writer.get_messages();
        assert_eq!(messages.len(), 2);

        // Check playbook_start event
        let start_msg: serde_json::Value = serde_json::from_str(&messages[0].1).unwrap();
        assert_eq!(start_msg["event"], "playbook_start");
        assert_eq!(start_msg["playbook"], "test-playbook");

        // Check playbook_end event
        let end_msg: serde_json::Value = serde_json::from_str(&messages[1].1).unwrap();
        assert_eq!(end_msg["event"], "playbook_end");
        assert_eq!(end_msg["success"], true);
    }

    #[tokio::test]
    async fn test_task_complete_ok() {
        let (callback, writer) = create_test_callback();

        let result = create_execution_result("host1", "Install package", true, false, false, "ok");
        callback.on_task_complete(&result).await;

        let messages = writer.get_messages();
        assert_eq!(messages.len(), 1);

        let msg: serde_json::Value = serde_json::from_str(&messages[0].1).unwrap();
        assert_eq!(msg["event"], "task_ok");
        assert_eq!(msg["status"], "ok");
        assert_eq!(msg["success"], true);
        assert_eq!(msg["changed"], false);
    }

    #[tokio::test]
    async fn test_task_complete_changed() {
        let (callback, writer) = create_test_callback();

        let result = create_execution_result(
            "host1",
            "Configure service",
            true,
            true,
            false,
            "configuration updated",
        );
        callback.on_task_complete(&result).await;

        let messages = writer.get_messages();
        assert_eq!(messages.len(), 1);

        let msg: serde_json::Value = serde_json::from_str(&messages[0].1).unwrap();
        assert_eq!(msg["event"], "task_changed");
        assert_eq!(msg["status"], "changed");
        assert_eq!(msg["changed"], true);
    }

    #[tokio::test]
    async fn test_task_complete_failed() {
        let (callback, writer) = create_test_callback();

        let result = create_execution_result(
            "host1",
            "Install package",
            false,
            false,
            false,
            "Package not found",
        );
        callback.on_task_complete(&result).await;

        let messages = writer.get_messages();
        assert_eq!(messages.len(), 1);

        let msg: serde_json::Value = serde_json::from_str(&messages[0].1).unwrap();
        assert_eq!(msg["event"], "task_failed");
        assert_eq!(msg["status"], "failed");
        assert_eq!(msg["error"], "Package not found");
    }

    #[tokio::test]
    async fn test_task_complete_skipped() {
        let (callback, writer) = create_test_callback();

        let result =
            create_execution_result("host1", "Conditional task", true, false, true, "skipped");
        callback.on_task_complete(&result).await;

        let messages = writer.get_messages();
        assert_eq!(messages.len(), 1);

        let msg: serde_json::Value = serde_json::from_str(&messages[0].1).unwrap();
        assert_eq!(msg["event"], "task_skipped");
        assert_eq!(msg["skipped"], true);
    }

    #[tokio::test]
    async fn test_task_start_disabled_by_default() {
        let (callback, writer) = create_test_callback();

        callback.on_task_start("Test task", "host1").await;

        let messages = writer.get_messages();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_task_start_enabled() {
        let mut config = SyslogConfig::default();
        config.log_task_start = true;

        let writer = Arc::new(TestSyslogWriter::new());
        let callback = SyslogCallback {
            config,
            writer: Arc::clone(&writer) as Arc<dyn SyslogWriter>,
            playbook_start: RwLock::new(None),
            current_playbook: RwLock::new(None),
            hostname: "testhost".to_string(),
            stats: RwLock::new(SyslogStats::default()),
        };

        callback.on_task_start("Test task", "host1").await;

        let messages = writer.get_messages();
        assert_eq!(messages.len(), 1);

        let msg: serde_json::Value = serde_json::from_str(&messages[0].1).unwrap();
        assert_eq!(msg["event"], "task_start");
    }

    #[tokio::test]
    async fn test_facts_disabled_by_default() {
        let (callback, writer) = create_test_callback();

        let facts = Facts::default();
        callback.on_facts_gathered("host1", &facts).await;

        let messages = writer.get_messages();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_handler_triggered() {
        let (callback, writer) = create_test_callback();

        callback.on_handler_triggered("restart nginx").await;

        let messages = writer.get_messages();
        assert_eq!(messages.len(), 1);

        let msg: serde_json::Value = serde_json::from_str(&messages[0].1).unwrap();
        assert_eq!(msg["event"], "handler_triggered");
        assert_eq!(msg["handler"], "restart nginx");
    }

    #[test]
    fn test_config_builder() {
        let config = SyslogConfig::builder()
            .facility(SyslogFacility::Local3)
            .ident("myapp")
            .format(SyslogFormat::Text)
            .include_pid(false)
            .log_task_start(true)
            .min_severity(SyslogSeverity::Warning)
            .extra_field("environment", "production")
            .build();

        assert_eq!(config.facility, SyslogFacility::Local3);
        assert_eq!(config.ident, "myapp");
        assert_eq!(config.format, SyslogFormat::Text);
        assert!(!config.include_pid);
        assert!(config.log_task_start);
        assert_eq!(config.min_severity, SyslogSeverity::Warning);
        assert_eq!(
            config.extra_fields.get("environment"),
            Some(&"production".to_string())
        );
    }

    #[test]
    fn test_stats_tracking() {
        let (callback, _) = create_test_callback();

        // Log a message
        callback.log_event("test", SyslogSeverity::Info, serde_json::json!({}));

        let stats = callback.stats();
        assert_eq!(stats.messages_logged, 1);
        assert_eq!(stats.messages_filtered, 0);
    }

    #[test]
    fn test_stats_filtered() {
        let mut config = SyslogConfig::default();
        config.min_severity = SyslogSeverity::Error;

        let callback = SyslogCallback {
            config,
            writer: Arc::new(TestSyslogWriter::new()),
            playbook_start: RwLock::new(None),
            current_playbook: RwLock::new(None),
            hostname: "testhost".to_string(),
            stats: RwLock::new(SyslogStats::default()),
        };

        // Log a debug message (should be filtered)
        callback.log_event("test", SyslogSeverity::Debug, serde_json::json!({}));

        let stats = callback.stats();
        assert_eq!(stats.messages_logged, 0);
        assert_eq!(stats.messages_filtered, 1);
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(SyslogSeverity::Emergency.to_string(), "emerg");
        assert_eq!(SyslogSeverity::Error.to_string(), "err");
        assert_eq!(SyslogSeverity::Warning.to_string(), "warning");
        assert_eq!(SyslogSeverity::Info.to_string(), "info");
    }

    #[test]
    fn test_facility_display() {
        assert_eq!(SyslogFacility::Local0.to_string(), "local0");
        assert_eq!(SyslogFacility::Daemon.to_string(), "daemon");
        assert_eq!(SyslogFacility::User.to_string(), "user");
    }
}
