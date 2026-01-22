//! Connection tracing for verbose debugging.
//!
//! This module provides detailed connection event tracing, capturing
//! all SSH commands, file transfers, and connection lifecycle events.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Level of tracing detail
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceLevel {
    /// No tracing
    Off,
    /// Only errors
    Error,
    /// Warnings and errors
    Warn,
    /// Informational messages (default)
    #[default]
    Info,
    /// Debug-level detail
    Debug,
    /// Maximum detail including raw data
    Trace,
}

impl TraceLevel {
    /// Check if this level should emit messages at the given level
    pub fn should_trace(&self, level: TraceLevel) -> bool {
        *self >= level
    }
}

/// Type of connection event
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionEventType {
    /// Connection being established
    Connecting,
    /// Connection established successfully
    Connected,
    /// Connection failed
    ConnectionFailed,
    /// Connection closed
    Disconnected,
    /// Reconnection attempt
    Reconnecting,
    /// Command execution started
    CommandStart,
    /// Command execution completed
    CommandComplete,
    /// Command execution failed
    CommandFailed,
    /// File upload started
    UploadStart,
    /// File upload completed
    UploadComplete,
    /// File upload failed
    UploadFailed,
    /// File download started
    DownloadStart,
    /// File download completed
    DownloadComplete,
    /// File download failed
    DownloadFailed,
    /// Privilege escalation (sudo/su)
    PrivilegeEscalation,
    /// Authentication attempt
    Authentication,
    /// Authentication failed
    AuthenticationFailed,
    /// Channel opened
    ChannelOpen,
    /// Channel closed
    ChannelClose,
    /// Keepalive sent
    Keepalive,
    /// Timeout occurred
    Timeout,
    /// Data sent
    DataSent,
    /// Data received
    DataReceived,
}

/// A connection event for tracing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionEvent {
    /// Type of event
    pub event_type: ConnectionEventType,
    /// Host involved
    pub host: String,
    /// Port (if applicable)
    pub port: Option<u16>,
    /// Username (if applicable)
    pub username: Option<String>,
    /// Command being executed (if applicable)
    pub command: Option<String>,
    /// File path (if applicable)
    pub path: Option<PathBuf>,
    /// Duration (for completed events)
    pub duration: Option<Duration>,
    /// Exit code (for command events)
    pub exit_code: Option<i32>,
    /// Error message (if applicable)
    pub error: Option<String>,
    /// Bytes transferred (for data events)
    pub bytes: Option<usize>,
    /// Additional details
    pub details: Option<String>,
}

impl ConnectionEvent {
    /// Create a new connection event
    pub fn new(event_type: ConnectionEventType, host: impl Into<String>) -> Self {
        Self {
            event_type,
            host: host.into(),
            port: None,
            username: None,
            command: None,
            path: None,
            duration: None,
            exit_code: None,
            error: None,
            bytes: None,
            details: None,
        }
    }

    /// Set the port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set the username
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Set the command
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set the file path
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set the duration
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set the exit code
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// Set the error message
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Set the bytes transferred
    pub fn with_bytes(mut self, bytes: usize) -> Self {
        self.bytes = Some(bytes);
        self
    }

    /// Set additional details
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Get the trace level for this event type
    pub fn trace_level(&self) -> TraceLevel {
        match &self.event_type {
            ConnectionEventType::ConnectionFailed
            | ConnectionEventType::CommandFailed
            | ConnectionEventType::UploadFailed
            | ConnectionEventType::DownloadFailed
            | ConnectionEventType::AuthenticationFailed
            | ConnectionEventType::Timeout => TraceLevel::Error,

            ConnectionEventType::Reconnecting => TraceLevel::Warn,

            ConnectionEventType::Connected
            | ConnectionEventType::Disconnected
            | ConnectionEventType::CommandStart
            | ConnectionEventType::CommandComplete
            | ConnectionEventType::UploadComplete
            | ConnectionEventType::DownloadComplete => TraceLevel::Info,

            ConnectionEventType::Connecting
            | ConnectionEventType::UploadStart
            | ConnectionEventType::DownloadStart
            | ConnectionEventType::PrivilegeEscalation
            | ConnectionEventType::Authentication
            | ConnectionEventType::ChannelOpen
            | ConnectionEventType::ChannelClose => TraceLevel::Debug,

            ConnectionEventType::Keepalive
            | ConnectionEventType::DataSent
            | ConnectionEventType::DataReceived => TraceLevel::Trace,
        }
    }
}

/// A recorded trace entry with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// The connection event
    pub event: ConnectionEvent,
    /// Sequence number for ordering
    pub sequence: u64,
}

impl TraceEntry {
    /// Create a new trace entry
    pub fn new(event: ConnectionEvent, sequence: u64) -> Self {
        Self {
            timestamp: Utc::now(),
            event,
            sequence,
        }
    }

    /// Format this entry for display
    pub fn format(&self, show_timestamp: bool, color: bool) -> String {
        let mut parts = Vec::new();

        if show_timestamp {
            parts.push(format!(
                "[{}]",
                self.timestamp.format("%Y-%m-%d %H:%M:%S%.3f")
            ));
        }

        let event_str = format!("{:?}", self.event.event_type);
        let event_colored = if color {
            match self.event.trace_level() {
                TraceLevel::Error => format!("\x1b[31m{}\x1b[0m", event_str), // Red
                TraceLevel::Warn => format!("\x1b[33m{}\x1b[0m", event_str),  // Yellow
                TraceLevel::Info => format!("\x1b[32m{}\x1b[0m", event_str),  // Green
                TraceLevel::Debug => format!("\x1b[36m{}\x1b[0m", event_str), // Cyan
                TraceLevel::Trace => format!("\x1b[90m{}\x1b[0m", event_str), // Gray
                TraceLevel::Off => event_str,
            }
        } else {
            event_str
        };
        parts.push(event_colored);

        parts.push(format!("host={}", self.event.host));

        if let Some(port) = self.event.port {
            parts.push(format!("port={}", port));
        }

        if let Some(ref user) = self.event.username {
            parts.push(format!("user={}", user));
        }

        if let Some(ref cmd) = self.event.command {
            let truncated = if cmd.len() > 80 {
                format!("{}...", &cmd[..77])
            } else {
                cmd.clone()
            };
            parts.push(format!("cmd=\"{}\"", truncated));
        }

        if let Some(ref path) = self.event.path {
            parts.push(format!("path={}", path.display()));
        }

        if let Some(duration) = self.event.duration {
            parts.push(format!("duration={:?}", duration));
        }

        if let Some(code) = self.event.exit_code {
            parts.push(format!("exit_code={}", code));
        }

        if let Some(bytes) = self.event.bytes {
            parts.push(format!("bytes={}", bytes));
        }

        if let Some(ref err) = self.event.error {
            parts.push(format!("error=\"{}\"", err));
        }

        if let Some(ref details) = self.event.details {
            parts.push(format!("details=\"{}\"", details));
        }

        parts.join(" ")
    }
}

/// Sink for trace output
pub trait TraceSink: Send + Sync {
    /// Write a trace entry
    fn write(&self, entry: &TraceEntry);
    /// Flush any buffered output
    fn flush(&self);
}

/// Standard error trace sink
#[derive(Debug, Default)]
pub struct StderrSink {
    show_timestamp: bool,
    color: bool,
}

impl StderrSink {
    /// Create a new stderr sink
    pub fn new(show_timestamp: bool, color: bool) -> Self {
        Self {
            show_timestamp,
            color,
        }
    }
}

impl TraceSink for StderrSink {
    fn write(&self, entry: &TraceEntry) {
        eprintln!("{}", entry.format(self.show_timestamp, self.color));
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

/// File trace sink
pub struct FileSink {
    path: PathBuf,
    show_timestamp: bool,
}

impl FileSink {
    /// Create a new file sink
    pub fn new(path: impl Into<PathBuf>, show_timestamp: bool) -> Self {
        Self {
            path: path.into(),
            show_timestamp,
        }
    }
}

impl TraceSink for FileSink {
    fn write(&self, entry: &TraceEntry) {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(file, "{}", entry.format(self.show_timestamp, false));
        }
    }

    fn flush(&self) {
        // File writes are immediate with append mode
    }
}

/// Connection tracer for recording and reporting connection events
pub struct ConnectionTracer {
    /// Minimum trace level to record
    level: TraceLevel,
    /// Recorded trace entries
    entries: Arc<RwLock<Vec<TraceEntry>>>,
    /// Sequence counter
    sequence: Arc<RwLock<u64>>,
    /// Maximum entries to keep
    max_entries: usize,
    /// Output sinks
    sinks: Arc<RwLock<Vec<Box<dyn TraceSink>>>>,
    /// Whether real-time output is enabled
    realtime: bool,
}

impl std::fmt::Debug for ConnectionTracer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionTracer")
            .field("level", &self.level)
            .field("entries", &self.entries)
            .field("sequence", &self.sequence)
            .field("max_entries", &self.max_entries)
            .field("sinks", &format!("[{} sinks]", self.sinks.read().len()))
            .field("realtime", &self.realtime)
            .finish()
    }
}

impl ConnectionTracer {
    /// Create a new connection tracer
    pub fn new(level: TraceLevel) -> Self {
        Self {
            level,
            entries: Arc::new(RwLock::new(Vec::new())),
            sequence: Arc::new(RwLock::new(0)),
            max_entries: 10000,
            sinks: Arc::new(RwLock::new(Vec::new())),
            realtime: false,
        }
    }

    /// Create a tracer with maximum detail
    pub fn verbose() -> Self {
        Self::new(TraceLevel::Trace)
    }

    /// Set maximum entries to keep
    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    /// Enable real-time output to stderr
    pub fn with_realtime_output(mut self, color: bool) -> Self {
        self.realtime = true;
        {
            let mut sinks = self.sinks.write();
            sinks.push(Box::new(StderrSink::new(true, color)));
        }
        self
    }

    /// Add a trace sink
    pub fn add_sink(&self, sink: Box<dyn TraceSink>) {
        let mut sinks = self.sinks.write();
        sinks.push(sink);
    }

    /// Record a connection event
    pub fn trace(&self, event: ConnectionEvent) {
        // Check if we should trace this event
        if !self.level.should_trace(event.trace_level()) {
            return;
        }

        let sequence = {
            let mut seq = self.sequence.write();
            *seq += 1;
            *seq
        };

        let entry = TraceEntry::new(event, sequence);

        // Output to sinks in real-time if enabled
        if self.realtime {
            let sinks = self.sinks.read();
            for sink in sinks.iter() {
                sink.write(&entry);
            }
        }

        // Record the entry
        let mut entries = self.entries.write();
        entries.push(entry);

        // Trim if necessary
        if entries.len() > self.max_entries {
            let drain_count = entries.len() - self.max_entries;
            entries.drain(0..drain_count);
        }
    }

    /// Get all recorded entries
    pub fn get_entries(&self) -> Vec<TraceEntry> {
        self.entries.read().clone()
    }

    /// Get entries filtered by host
    pub fn get_entries_for_host(&self, host: &str) -> Vec<TraceEntry> {
        self.entries
            .read()
            .iter()
            .filter(|e| e.event.host == host)
            .cloned()
            .collect()
    }

    /// Get entries filtered by event type
    pub fn get_entries_by_type(&self, event_type: ConnectionEventType) -> Vec<TraceEntry> {
        self.entries
            .read()
            .iter()
            .filter(|e| e.event.event_type == event_type)
            .cloned()
            .collect()
    }

    /// Get error entries only
    pub fn get_errors(&self) -> Vec<TraceEntry> {
        self.entries
            .read()
            .iter()
            .filter(|e| e.event.error.is_some())
            .cloned()
            .collect()
    }

    /// Clear all recorded entries
    pub fn clear(&self) {
        self.entries.write().clear();
    }

    /// Get the current trace level
    pub fn level(&self) -> TraceLevel {
        self.level
    }

    /// Get entry count
    pub fn entry_count(&self) -> usize {
        self.entries.read().len()
    }

    /// Export entries as JSON
    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        let entries = self.entries.read();
        serde_json::to_string_pretty(&*entries)
    }

    /// Generate a summary report
    pub fn summary(&self) -> TraceSummary {
        let entries = self.entries.read();

        let mut summary = TraceSummary {
            total_events: entries.len(),
            ..Default::default()
        };

        for entry in entries.iter() {
            match entry.event.event_type {
                ConnectionEventType::Connected => summary.connections += 1,
                ConnectionEventType::ConnectionFailed => summary.connection_failures += 1,
                ConnectionEventType::CommandComplete => {
                    summary.commands_executed += 1;
                    if let Some(duration) = entry.event.duration {
                        summary.total_command_time += duration;
                    }
                }
                ConnectionEventType::CommandFailed => summary.command_failures += 1,
                ConnectionEventType::UploadComplete | ConnectionEventType::DownloadComplete => {
                    summary.file_transfers += 1;
                    if let Some(bytes) = entry.event.bytes {
                        summary.bytes_transferred += bytes;
                    }
                }
                ConnectionEventType::UploadFailed | ConnectionEventType::DownloadFailed => {
                    summary.transfer_failures += 1;
                }
                _ => {}
            }

            if entry.event.error.is_some() {
                summary.errors += 1;
            }
        }

        summary
    }
}

/// Summary of trace activity
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceSummary {
    /// Total events recorded
    pub total_events: usize,
    /// Successful connections
    pub connections: usize,
    /// Failed connections
    pub connection_failures: usize,
    /// Commands executed
    pub commands_executed: usize,
    /// Command failures
    pub command_failures: usize,
    /// File transfers completed
    pub file_transfers: usize,
    /// File transfer failures
    pub transfer_failures: usize,
    /// Total bytes transferred
    pub bytes_transferred: usize,
    /// Total command execution time
    pub total_command_time: Duration,
    /// Total errors
    pub errors: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_level_ordering() {
        assert!(TraceLevel::Trace > TraceLevel::Debug);
        assert!(TraceLevel::Debug > TraceLevel::Info);
        assert!(TraceLevel::Info > TraceLevel::Warn);
        assert!(TraceLevel::Warn > TraceLevel::Error);
        assert!(TraceLevel::Error > TraceLevel::Off);
    }

    #[test]
    fn test_trace_level_should_trace() {
        let level = TraceLevel::Info;
        assert!(level.should_trace(TraceLevel::Error));
        assert!(level.should_trace(TraceLevel::Warn));
        assert!(level.should_trace(TraceLevel::Info));
        assert!(!level.should_trace(TraceLevel::Debug));
        assert!(!level.should_trace(TraceLevel::Trace));
    }

    #[test]
    fn test_connection_event_creation() {
        let event = ConnectionEvent::new(ConnectionEventType::Connected, "host1")
            .with_port(22)
            .with_username("admin")
            .with_duration(Duration::from_millis(150));

        assert_eq!(event.host, "host1");
        assert_eq!(event.port, Some(22));
        assert_eq!(event.username, Some("admin".to_string()));
        assert!(event.duration.is_some());
    }

    #[test]
    fn test_tracer_records_events() {
        let tracer = ConnectionTracer::new(TraceLevel::Info);

        tracer.trace(ConnectionEvent::new(
            ConnectionEventType::Connected,
            "host1",
        ));
        tracer.trace(ConnectionEvent::new(
            ConnectionEventType::CommandComplete,
            "host1",
        ));

        assert_eq!(tracer.entry_count(), 2);
    }

    #[test]
    fn test_tracer_filters_by_level() {
        let tracer = ConnectionTracer::new(TraceLevel::Warn);

        // Debug event should not be recorded
        tracer.trace(ConnectionEvent::new(
            ConnectionEventType::Connecting,
            "host1",
        ));
        // Error event should be recorded
        tracer.trace(ConnectionEvent::new(
            ConnectionEventType::ConnectionFailed,
            "host1",
        ));

        assert_eq!(tracer.entry_count(), 1);
    }

    #[test]
    fn test_tracer_max_entries() {
        let tracer = ConnectionTracer::new(TraceLevel::Info).with_max_entries(5);

        for i in 0..10 {
            tracer.trace(
                ConnectionEvent::new(ConnectionEventType::Connected, format!("host{}", i))
                    .with_port(22),
            );
        }

        assert_eq!(tracer.entry_count(), 5);
    }

    #[test]
    fn test_tracer_summary() {
        let tracer = ConnectionTracer::new(TraceLevel::Info);

        tracer.trace(ConnectionEvent::new(
            ConnectionEventType::Connected,
            "host1",
        ));
        tracer.trace(ConnectionEvent::new(
            ConnectionEventType::CommandComplete,
            "host1",
        ));
        tracer.trace(ConnectionEvent::new(
            ConnectionEventType::CommandFailed,
            "host1",
        ));

        let summary = tracer.summary();
        assert_eq!(summary.connections, 1);
        assert_eq!(summary.commands_executed, 1);
        assert_eq!(summary.command_failures, 1);
    }
}
