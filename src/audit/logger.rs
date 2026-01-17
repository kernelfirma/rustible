//! Audit log output backends
//!
//! This module provides different backends for writing audit events:
//! - File logger: Write to local files with rotation support
//! - Syslog logger: Forward to local or remote syslog
//! - Journald logger: Write to systemd journal (Linux)

use super::event::{AuditEvent, AuditSeverity};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Errors that can occur during audit logging
#[derive(Error, Debug)]
pub enum AuditLogError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Syslog error: {0}")]
    Syslog(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Logger not available: {0}")]
    NotAvailable(String),
}

/// Result type for audit logging operations
pub type AuditLogResult<T> = Result<T, AuditLogError>;

/// Format for audit log output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditFormat {
    /// Single-line text format
    Text,
    /// JSON format (one event per line)
    Json,
    /// Common Event Format (CEF)
    Cef,
}

impl Default for AuditFormat {
    fn default() -> Self {
        AuditFormat::Text
    }
}

/// Trait for audit log backends
pub trait AuditLogger: Send + Sync {
    /// Write an audit event
    fn log(&self, event: &AuditEvent) -> AuditLogResult<()>;

    /// Flush any buffered events
    fn flush(&self) -> AuditLogResult<()>;

    /// Get the logger name for identification
    fn name(&self) -> &str;

    /// Check if the logger is available/healthy
    fn is_available(&self) -> bool {
        true
    }
}

/// File-based audit logger with optional rotation
pub struct FileLogger {
    /// Path to the log file
    path: PathBuf,
    /// Writer (buffered)
    writer: Arc<Mutex<BufWriter<File>>>,
    /// Output format
    format: AuditFormat,
    /// Maximum file size before rotation (bytes)
    max_size: Option<u64>,
    /// Number of rotated files to keep
    max_files: u32,
}

impl FileLogger {
    /// Create a new file logger
    pub fn new(path: impl AsRef<Path>) -> AuditLogResult<Self> {
        Self::with_options(path, AuditFormat::Text, None, 5)
    }

    /// Create a file logger with custom options
    pub fn with_options(
        path: impl AsRef<Path>,
        format: AuditFormat,
        max_size: Option<u64>,
        max_files: u32,
    ) -> AuditLogResult<Self> {
        let path = path.as_ref().to_path_buf();

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        Ok(Self {
            path,
            writer: Arc::new(Mutex::new(BufWriter::new(file))),
            format,
            max_size,
            max_files,
        })
    }

    /// Format an event according to the configured format
    fn format_event(&self, event: &AuditEvent) -> AuditLogResult<String> {
        match self.format {
            AuditFormat::Text => Ok(event.to_log_line()),
            AuditFormat::Json => serde_json::to_string(event)
                .map_err(|e| AuditLogError::Serialization(e.to_string())),
            AuditFormat::Cef => Ok(format_cef(event)),
        }
    }

    /// Check if rotation is needed and perform it
    fn maybe_rotate(&self) -> AuditLogResult<()> {
        let Some(max_size) = self.max_size else {
            return Ok(());
        };

        let metadata = std::fs::metadata(&self.path)?;
        if metadata.len() < max_size {
            return Ok(());
        }

        // Perform rotation
        self.rotate()?;
        Ok(())
    }

    /// Rotate log files
    fn rotate(&self) -> AuditLogResult<()> {
        // Close current file
        {
            let mut writer = self.writer.lock().unwrap();
            writer.flush()?;
        }

        // Rotate existing files
        for i in (1..self.max_files).rev() {
            let old_path = format!("{}.{}", self.path.display(), i);
            let new_path = format!("{}.{}", self.path.display(), i + 1);
            if Path::new(&old_path).exists() {
                std::fs::rename(&old_path, &new_path)?;
            }
        }

        // Move current file to .1
        let rotated = format!("{}.1", self.path.display());
        std::fs::rename(&self.path, &rotated)?;

        // Open new file
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let mut writer = self.writer.lock().unwrap();
        *writer = BufWriter::new(file);

        Ok(())
    }
}

impl AuditLogger for FileLogger {
    fn log(&self, event: &AuditEvent) -> AuditLogResult<()> {
        self.maybe_rotate()?;

        let line = self.format_event(event)?;
        let mut writer = self.writer.lock().unwrap();
        writeln!(writer, "{}", line)?;
        Ok(())
    }

    fn flush(&self) -> AuditLogResult<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.flush()?;
        Ok(())
    }

    fn name(&self) -> &str {
        "file"
    }
}

impl fmt::Debug for FileLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileLogger")
            .field("path", &self.path)
            .field("format", &self.format)
            .field("max_size", &self.max_size)
            .field("max_files", &self.max_files)
            .finish()
    }
}

/// Syslog facility for audit messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyslogFacility {
    /// Security/authorization messages
    Auth,
    /// Security/authorization messages (private)
    AuthPriv,
    /// System daemons
    Daemon,
    /// Local use 0-7
    Local0,
    Local1,
    Local2,
    Local3,
    Local4,
    Local5,
    Local6,
    Local7,
}

impl SyslogFacility {
    fn to_code(self) -> u8 {
        match self {
            SyslogFacility::Auth => 4,
            SyslogFacility::AuthPriv => 10,
            SyslogFacility::Daemon => 3,
            SyslogFacility::Local0 => 16,
            SyslogFacility::Local1 => 17,
            SyslogFacility::Local2 => 18,
            SyslogFacility::Local3 => 19,
            SyslogFacility::Local4 => 20,
            SyslogFacility::Local5 => 21,
            SyslogFacility::Local6 => 22,
            SyslogFacility::Local7 => 23,
        }
    }
}

impl Default for SyslogFacility {
    fn default() -> Self {
        SyslogFacility::AuthPriv
    }
}

/// Syslog transport protocol
#[derive(Debug, Clone)]
pub enum SyslogTransport {
    /// Unix domain socket (local syslog)
    Unix(PathBuf),
    /// UDP transport
    Udp { host: String, port: u16 },
    /// TCP transport
    Tcp { host: String, port: u16 },
}

impl Default for SyslogTransport {
    fn default() -> Self {
        SyslogTransport::Unix(PathBuf::from("/dev/log"))
    }
}

/// Syslog-based audit logger
pub struct SyslogLogger {
    /// Application identifier
    ident: String,
    /// Syslog facility
    facility: SyslogFacility,
    /// Transport configuration
    transport: SyslogTransport,
    /// Output format
    format: AuditFormat,
    /// Socket (for Unix/UDP)
    socket: Arc<Mutex<Option<std::os::unix::net::UnixDatagram>>>,
}

impl SyslogLogger {
    /// Create a new syslog logger with default settings
    pub fn new(ident: impl Into<String>) -> AuditLogResult<Self> {
        Self::with_options(
            ident,
            SyslogFacility::default(),
            SyslogTransport::default(),
            AuditFormat::Text,
        )
    }

    /// Create a syslog logger with custom options
    pub fn with_options(
        ident: impl Into<String>,
        facility: SyslogFacility,
        transport: SyslogTransport,
        format: AuditFormat,
    ) -> AuditLogResult<Self> {
        let socket = match &transport {
            SyslogTransport::Unix(path) => {
                if path.exists() {
                    let sock = std::os::unix::net::UnixDatagram::unbound()?;
                    sock.connect(path)?;
                    Some(sock)
                } else {
                    None
                }
            }
            SyslogTransport::Udp { .. } | SyslogTransport::Tcp { .. } => {
                // Network syslog not yet implemented
                None
            }
        };

        Ok(Self {
            ident: ident.into(),
            facility,
            transport,
            format,
            socket: Arc::new(Mutex::new(socket)),
        })
    }

    /// Create a syslog logger for remote server
    pub fn remote(ident: impl Into<String>, host: impl Into<String>, port: u16) -> AuditLogResult<Self> {
        Self::with_options(
            ident,
            SyslogFacility::AuthPriv,
            SyslogTransport::Udp { host: host.into(), port },
            AuditFormat::Text,
        )
    }

    /// Convert severity to syslog priority
    fn severity_to_priority(severity: AuditSeverity) -> u8 {
        match severity {
            AuditSeverity::Info => 6,     // informational
            AuditSeverity::Warning => 4,  // warning
            AuditSeverity::Error => 3,    // error
            AuditSeverity::Critical => 2, // critical
        }
    }

    /// Format a syslog message
    fn format_message(&self, event: &AuditEvent) -> String {
        let priority = (self.facility.to_code() << 3) | Self::severity_to_priority(event.severity);
        let content = match self.format {
            AuditFormat::Text => event.to_log_line(),
            AuditFormat::Json => serde_json::to_string(event).unwrap_or_else(|_| event.to_log_line()),
            AuditFormat::Cef => format_cef(event),
        };

        format!("<{}>{}: {}", priority, self.ident, content)
    }

    /// Send message via Unix socket
    fn send_unix(&self, message: &str) -> AuditLogResult<()> {
        let socket = self.socket.lock().unwrap();
        if let Some(ref sock) = *socket {
            sock.send(message.as_bytes())?;
            Ok(())
        } else {
            Err(AuditLogError::NotAvailable("Unix socket not connected".into()))
        }
    }

    /// Send message via UDP
    fn send_udp(&self, message: &str, host: &str, port: u16) -> AuditLogResult<()> {
        use std::net::UdpSocket;

        let socket = UdpSocket::bind("0.0.0.0:0")?;
        let addr = format!("{}:{}", host, port);
        socket.send_to(message.as_bytes(), &addr)?;
        Ok(())
    }
}

impl AuditLogger for SyslogLogger {
    fn log(&self, event: &AuditEvent) -> AuditLogResult<()> {
        let message = self.format_message(event);

        match &self.transport {
            SyslogTransport::Unix(_) => self.send_unix(&message),
            SyslogTransport::Udp { host, port } => self.send_udp(&message, host, *port),
            SyslogTransport::Tcp { .. } => {
                Err(AuditLogError::NotAvailable("TCP syslog not implemented".into()))
            }
        }
    }

    fn flush(&self) -> AuditLogResult<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "syslog"
    }

    fn is_available(&self) -> bool {
        match &self.transport {
            SyslogTransport::Unix(path) => path.exists(),
            SyslogTransport::Udp { .. } => true,
            SyslogTransport::Tcp { .. } => false, // Not implemented
        }
    }
}

impl fmt::Debug for SyslogLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SyslogLogger")
            .field("ident", &self.ident)
            .field("facility", &self.facility)
            .field("transport", &self.transport)
            .field("format", &self.format)
            .finish()
    }
}

/// Journald-based audit logger (Linux systemd)
#[cfg(target_os = "linux")]
pub struct JournaldLogger {
    /// Application identifier
    ident: String,
    /// Whether journald is available
    available: bool,
}

#[cfg(target_os = "linux")]
impl JournaldLogger {
    /// Create a new journald logger
    pub fn new(ident: impl Into<String>) -> Self {
        let available = Path::new("/run/systemd/journal/socket").exists();
        Self {
            ident: ident.into(),
            available,
        }
    }

    /// Send to journald using the logger command (fallback)
    fn send_via_logger(&self, event: &AuditEvent) -> AuditLogResult<()> {
        use std::process::Command;

        let priority = match event.severity {
            AuditSeverity::Info => "info",
            AuditSeverity::Warning => "warning",
            AuditSeverity::Error => "err",
            AuditSeverity::Critical => "crit",
        };

        let message = event.to_log_line();

        let output = Command::new("logger")
            .arg("-t")
            .arg(&self.ident)
            .arg("-p")
            .arg(format!("auth.{}", priority))
            .arg(&message)
            .output()?;

        if !output.status.success() {
            return Err(AuditLogError::Syslog(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl AuditLogger for JournaldLogger {
    fn log(&self, event: &AuditEvent) -> AuditLogResult<()> {
        if !self.available {
            return Err(AuditLogError::NotAvailable("journald not available".into()));
        }

        self.send_via_logger(event)
    }

    fn flush(&self) -> AuditLogResult<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "journald"
    }

    fn is_available(&self) -> bool {
        self.available
    }
}

#[cfg(target_os = "linux")]
impl fmt::Debug for JournaldLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JournaldLogger")
            .field("ident", &self.ident)
            .field("available", &self.available)
            .finish()
    }
}

/// Stub journald logger for non-Linux systems
#[cfg(not(target_os = "linux"))]
pub struct JournaldLogger {
    ident: String,
}

#[cfg(not(target_os = "linux"))]
impl JournaldLogger {
    pub fn new(ident: impl Into<String>) -> Self {
        Self { ident: ident.into() }
    }
}

#[cfg(not(target_os = "linux"))]
impl AuditLogger for JournaldLogger {
    fn log(&self, _event: &AuditEvent) -> AuditLogResult<()> {
        Err(AuditLogError::NotAvailable("journald only available on Linux".into()))
    }

    fn flush(&self) -> AuditLogResult<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "journald"
    }

    fn is_available(&self) -> bool {
        false
    }
}

#[cfg(not(target_os = "linux"))]
impl fmt::Debug for JournaldLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JournaldLogger")
            .field("ident", &self.ident)
            .field("available", &false)
            .finish()
    }
}

/// Console logger for debugging/development
pub struct ConsoleLogger {
    /// Output format
    format: AuditFormat,
    /// Output to stderr instead of stdout
    use_stderr: bool,
}

impl ConsoleLogger {
    /// Create a new console logger
    pub fn new() -> Self {
        Self {
            format: AuditFormat::Text,
            use_stderr: true,
        }
    }

    /// Create with custom format
    pub fn with_format(format: AuditFormat) -> Self {
        Self {
            format,
            use_stderr: true,
        }
    }
}

impl Default for ConsoleLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLogger for ConsoleLogger {
    fn log(&self, event: &AuditEvent) -> AuditLogResult<()> {
        let output = match self.format {
            AuditFormat::Text => event.to_log_line(),
            AuditFormat::Json => serde_json::to_string(event)
                .map_err(|e| AuditLogError::Serialization(e.to_string()))?,
            AuditFormat::Cef => format_cef(event),
        };

        if self.use_stderr {
            eprintln!("[AUDIT] {}", output);
        } else {
            println!("[AUDIT] {}", output);
        }
        Ok(())
    }

    fn flush(&self) -> AuditLogResult<()> {
        if self.use_stderr {
            io::stderr().flush()?;
        } else {
            io::stdout().flush()?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "console"
    }
}

impl fmt::Debug for ConsoleLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConsoleLogger")
            .field("format", &self.format)
            .field("use_stderr", &self.use_stderr)
            .finish()
    }
}

/// Format an event in Common Event Format (CEF)
fn format_cef(event: &AuditEvent) -> String {
    // CEF format: CEF:Version|Device Vendor|Device Product|Device Version|Signature ID|Name|Severity|Extension
    let severity = match event.severity {
        AuditSeverity::Info => 3,
        AuditSeverity::Warning => 5,
        AuditSeverity::Error => 7,
        AuditSeverity::Critical => 10,
    };

    let extensions = format!(
        "act={} src={} dst={} outcome={} msg={}",
        event.category,
        event.actor,
        event.target_host.as_deref().unwrap_or("localhost"),
        event.outcome,
        cef_escape(&event.message),
    );

    format!(
        "CEF:0|Rustible|Audit|1.0|{}|{}|{}|{}",
        event.category,
        cef_escape(&event.message),
        severity,
        extensions
    )
}

/// Escape special characters for CEF format
fn cef_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('=', "\\=")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::path::PathBuf;

    #[test]
    fn test_file_logger() {
        let temp = TempDir::new().unwrap();
        let log_path = temp.path().join("audit.log");

        let logger = FileLogger::new(&log_path).unwrap();

        let event = AuditEvent::command_execution("ls -la")
            .with_host("server1")
            .success();

        logger.log(&event).unwrap();
        logger.flush().unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("COMMAND_EXECUTION"));
        assert!(content.contains("server1"));
    }

    #[test]
    fn test_file_logger_json_format() {
        let temp = TempDir::new().unwrap();
        let log_path = temp.path().join("audit.json");

        let logger = FileLogger::with_options(&log_path, AuditFormat::Json, None, 5).unwrap();

        let event = AuditEvent::file_modification("/etc/passwd", "Modified")
            .success();

        logger.log(&event).unwrap();
        logger.flush().unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content.trim()).unwrap();
        assert_eq!(parsed["category"], "file_modification");
    }

    #[test]
    fn test_console_logger() {
        let logger = ConsoleLogger::new();
        let event = AuditEvent::privilege_escalation("sudo", Some("root".to_string()));

        // Should not panic
        logger.log(&event).unwrap();
    }

    #[test]
    fn test_cef_format() {
        let event = AuditEvent::command_execution("rm -rf /tmp/test")
            .with_host("server1")
            .success();

        let cef = format_cef(&event);
        assert!(cef.starts_with("CEF:0|Rustible|Audit|"));
        assert!(cef.contains("COMMAND_EXECUTION"));
    }

    #[test]
    fn test_cef_escape() {
        assert_eq!(cef_escape("a|b=c\\d"), "a\\|b\\=c\\\\d");
    }

    #[test]
    fn test_file_logger_rotation() {
        let temp = TempDir::new().unwrap();
        let log_path = temp.path().join("audit.log");

        std::fs::write(&log_path, "x".repeat(64)).unwrap();

        let logger = FileLogger::with_options(&log_path, AuditFormat::Text, Some(10), 3).unwrap();
        let event = AuditEvent::command_execution("echo test").success();
        logger.log(&event).unwrap();
        logger.flush().unwrap();

        let rotated = PathBuf::from(format!("{}.1", log_path.display()));
        assert!(rotated.exists());
        assert!(log_path.exists());
    }
}
