//! Audit Trail for Privileged Operations
//!
//! This module provides comprehensive audit logging for all privilege
//! escalation operations, enabling security monitoring and compliance.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;

use super::SecurityResult;

/// Severity level for audit entries
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditSeverity {
    /// Informational (routine operations)
    Info,
    /// Warning (unusual but allowed operations)
    Warning,
    /// Alert (security-relevant operations)
    Alert,
    /// Critical (potential security incident)
    Critical,
}

impl std::fmt::Display for AuditSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditSeverity::Info => write!(f, "INFO"),
            AuditSeverity::Warning => write!(f, "WARN"),
            AuditSeverity::Alert => write!(f, "ALERT"),
            AuditSeverity::Critical => write!(f, "CRIT"),
        }
    }
}

/// Type of audit event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// Privilege escalation started
    EscalationStart,
    /// Privilege escalation completed successfully
    EscalationSuccess,
    /// Privilege escalation failed
    EscalationFailed,
    /// Password authentication attempt
    PasswordAuth,
    /// Password authentication failed
    PasswordAuthFailed,
    /// Policy violation detected
    PolicyViolation,
    /// Suspicious input detected
    SuspiciousInput,
    /// Command executed with elevated privileges
    ElevatedCommand,
    /// File operation with elevated privileges
    ElevatedFileOp,
    /// Configuration change
    ConfigChange,
    /// Session start
    SessionStart,
    /// Session end
    SessionEnd,
}

impl std::fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditEventType::EscalationStart => write!(f, "ESCALATION_START"),
            AuditEventType::EscalationSuccess => write!(f, "ESCALATION_SUCCESS"),
            AuditEventType::EscalationFailed => write!(f, "ESCALATION_FAILED"),
            AuditEventType::PasswordAuth => write!(f, "PASSWORD_AUTH"),
            AuditEventType::PasswordAuthFailed => write!(f, "PASSWORD_AUTH_FAILED"),
            AuditEventType::PolicyViolation => write!(f, "POLICY_VIOLATION"),
            AuditEventType::SuspiciousInput => write!(f, "SUSPICIOUS_INPUT"),
            AuditEventType::ElevatedCommand => write!(f, "ELEVATED_COMMAND"),
            AuditEventType::ElevatedFileOp => write!(f, "ELEVATED_FILE_OP"),
            AuditEventType::ConfigChange => write!(f, "CONFIG_CHANGE"),
            AuditEventType::SessionStart => write!(f, "SESSION_START"),
            AuditEventType::SessionEnd => write!(f, "SESSION_END"),
        }
    }
}

/// A single audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique entry ID
    pub id: String,
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// Event type
    pub event_type: AuditEventType,
    /// Severity level
    pub severity: AuditSeverity,
    /// Host where the operation occurred
    pub host: String,
    /// Source user (who initiated)
    pub source_user: String,
    /// Target user (who we're becoming)
    pub target_user: Option<String>,
    /// Escalation method used
    pub method: Option<String>,
    /// Command executed (truncated/sanitized)
    pub command: Option<String>,
    /// Result/outcome
    pub result: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Additional context
    #[serde(default)]
    pub context: std::collections::HashMap<String, String>,
    /// Duration of the operation (if applicable)
    pub duration_ms: Option<u64>,
    /// Session ID for correlating entries
    pub session_id: Option<String>,
    /// Process ID
    pub pid: u32,
}

impl AuditEntry {
    /// Create a new audit entry
    pub fn new(event_type: AuditEventType, severity: AuditSeverity, host: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type,
            severity,
            host: host.to_string(),
            source_user: current_username(),
            target_user: None,
            method: None,
            command: None,
            result: None,
            error: None,
            context: std::collections::HashMap::new(),
            duration_ms: None,
            session_id: None,
            pid: std::process::id(),
        }
    }

    /// Set target user
    pub fn with_target_user(mut self, user: impl Into<String>) -> Self {
        self.target_user = Some(user.into());
        self
    }

    /// Set escalation method
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    /// Set command (will be sanitized)
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(sanitize_command(&command.into()));
        self
    }

    /// Set result
    pub fn with_result(mut self, result: impl Into<String>) -> Self {
        self.result = Some(result.into());
        self
    }

    /// Set error
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Add context
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Set duration
    pub fn with_duration_ms(mut self, duration: u64) -> Self {
        self.duration_ms = Some(duration);
        self
    }

    /// Set session ID
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Format as a log line
    pub fn format_log_line(&self) -> String {
        let mut parts = vec![
            format!("[{}]", self.timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ")),
            format!("[{}]", self.severity),
            format!("[{}]", self.event_type),
            format!("host={}", self.host),
            format!("src_user={}", self.source_user),
        ];

        if let Some(ref user) = self.target_user {
            parts.push(format!("target_user={}", user));
        }

        if let Some(ref method) = self.method {
            parts.push(format!("method={}", method));
        }

        if let Some(ref cmd) = self.command {
            parts.push(format!("cmd=\"{}\"", cmd));
        }

        if let Some(ref result) = self.result {
            parts.push(format!("result={}", result));
        }

        if let Some(ref error) = self.error {
            parts.push(format!("error=\"{}\"", error));
        }

        if let Some(duration) = self.duration_ms {
            parts.push(format!("duration_ms={}", duration));
        }

        parts.join(" ")
    }
}

fn current_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Sanitize a command for logging (remove sensitive data)
fn sanitize_command(command: &str) -> String {
    // Truncate long commands
    let max_len = 200;
    let cmd = if command.len() > max_len {
        format!("{}...", &command[..max_len])
    } else {
        command.to_string()
    };

    // Remove potential passwords and secrets
    // This is a heuristic - we look for common patterns
    let patterns = [
        (r"(-p\s*)\S+", "$1[REDACTED]"),
        (r"(--password[=\s]*)\S+", "$1[REDACTED]"),
        (r"(PASSWORD[=:])\S+", "$1[REDACTED]"),
        (r"(SECRET[=:])\S+", "$1[REDACTED]"),
        (r"(TOKEN[=:])\S+", "$1[REDACTED]"),
        (r"(API_KEY[=:])\S+", "$1[REDACTED]"),
    ];

    let mut result = cmd;
    for (pattern, replacement) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            result = re.replace_all(&result, replacement).to_string();
        }
    }

    result
}

/// Configuration for audit logging
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Whether auditing is enabled
    pub enabled: bool,
    /// Minimum severity to log
    pub min_severity: AuditSeverity,
    /// Path to audit log file (None for memory-only)
    pub log_file: Option<PathBuf>,
    /// Maximum entries to keep in memory
    pub max_memory_entries: usize,
    /// Whether to log to syslog
    pub use_syslog: bool,
    /// Whether to log to tracing
    pub use_tracing: bool,
    /// Whether to include full commands (vs truncated)
    pub log_full_commands: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_severity: AuditSeverity::Info,
            log_file: None,
            max_memory_entries: 1000,
            use_syslog: false,
            use_tracing: true,
            log_full_commands: false,
        }
    }
}

impl AuditConfig {
    /// Create config for compliance-focused logging
    pub fn compliance() -> Self {
        Self {
            enabled: true,
            min_severity: AuditSeverity::Info,
            log_file: Some(PathBuf::from("/var/log/rustible/audit.log")),
            max_memory_entries: 10000,
            use_syslog: true,
            use_tracing: true,
            log_full_commands: true,
        }
    }

    /// Create minimal config
    pub fn minimal() -> Self {
        Self {
            enabled: true,
            min_severity: AuditSeverity::Alert,
            log_file: None,
            max_memory_entries: 100,
            use_syslog: false,
            use_tracing: true,
            log_full_commands: false,
        }
    }
}

/// Audit logger for privilege escalation operations
pub struct AuditLogger {
    /// Configuration
    config: AuditConfig,
    /// In-memory log entries (ring buffer)
    entries: Arc<RwLock<VecDeque<AuditEntry>>>,
    /// File writer (if configured)
    file_writer: Option<Arc<RwLock<BufWriter<File>>>>,
    /// Current session ID
    session_id: String,
}

impl AuditLogger {
    /// Create a new audit logger
    pub fn new() -> Self {
        Self::with_config(AuditConfig::default())
    }

    /// Create an audit logger with config
    pub fn with_config(config: AuditConfig) -> Self {
        let file_writer = config.log_file.as_ref().and_then(|path| {
            // Create parent directories if needed
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(|f| Arc::new(RwLock::new(BufWriter::new(f))))
        });

        Self {
            config,
            entries: Arc::new(RwLock::new(VecDeque::new())),
            file_writer,
            session_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Log an audit entry
    pub fn log(&self, entry: AuditEntry) {
        if !self.config.enabled {
            return;
        }

        if entry.severity < self.config.min_severity {
            return;
        }

        // Add session ID
        let mut entry = entry;
        if entry.session_id.is_none() {
            entry.session_id = Some(self.session_id.clone());
        }

        // Log to tracing
        if self.config.use_tracing {
            let log_line = entry.format_log_line();
            match entry.severity {
                AuditSeverity::Info => tracing::info!(target: "rustible::audit", "{}", log_line),
                AuditSeverity::Warning => {
                    tracing::warn!(target: "rustible::audit", "{}", log_line)
                }
                AuditSeverity::Alert => tracing::warn!(target: "rustible::audit", "{}", log_line),
                AuditSeverity::Critical => {
                    tracing::error!(target: "rustible::audit", "{}", log_line)
                }
            }
        }

        // Log to file
        if let Some(ref writer) = self.file_writer {
            let mut w = writer.write();
            let json = serde_json::to_string(&entry).unwrap_or_default();
            let _ = writeln!(w, "{}", json);
            let _ = w.flush();
        }

        // Store in memory
        let mut entries = self.entries.write();
        if entries.len() >= self.config.max_memory_entries {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Log an escalation start event
    pub fn log_escalation_start(
        &self,
        host: &str,
        target_user: &str,
        method: &str,
        command: &str,
    ) {
        let entry = AuditEntry::new(AuditEventType::EscalationStart, AuditSeverity::Info, host)
            .with_target_user(target_user)
            .with_method(method)
            .with_command(command);

        self.log(entry);
    }

    /// Log an escalation success event
    pub fn log_escalation_success(
        &self,
        host: &str,
        target_user: &str,
        method: &str,
        command: &str,
        duration_ms: u64,
    ) {
        let entry = AuditEntry::new(AuditEventType::EscalationSuccess, AuditSeverity::Info, host)
            .with_target_user(target_user)
            .with_method(method)
            .with_command(command)
            .with_result("success")
            .with_duration_ms(duration_ms);

        self.log(entry);
    }

    /// Log an escalation failure event
    pub fn log_escalation_failure(
        &self,
        host: &str,
        target_user: &str,
        method: &str,
        error: &str,
    ) {
        let entry = AuditEntry::new(AuditEventType::EscalationFailed, AuditSeverity::Warning, host)
            .with_target_user(target_user)
            .with_method(method)
            .with_error(error);

        self.log(entry);
    }

    /// Log a policy violation
    pub fn log_policy_violation(
        &self,
        host: &str,
        target_user: &str,
        reason: &str,
    ) {
        let entry = AuditEntry::new(AuditEventType::PolicyViolation, AuditSeverity::Alert, host)
            .with_target_user(target_user)
            .with_error(reason);

        self.log(entry);
    }

    /// Log suspicious input detection
    pub fn log_suspicious_input(
        &self,
        host: &str,
        input_type: &str,
        value: &str,
    ) {
        let entry = AuditEntry::new(AuditEventType::SuspiciousInput, AuditSeverity::Alert, host)
            .with_context("input_type", input_type)
            .with_context("value", sanitize_command(value));

        self.log(entry);
    }

    /// Log an elevated command execution
    pub fn log_elevated_command(
        &self,
        host: &str,
        target_user: &str,
        command: &str,
        exit_code: i32,
    ) {
        let severity = if exit_code == 0 {
            AuditSeverity::Info
        } else {
            AuditSeverity::Warning
        };

        let entry = AuditEntry::new(AuditEventType::ElevatedCommand, severity, host)
            .with_target_user(target_user)
            .with_command(command)
            .with_result(format!("exit_code={}", exit_code));

        self.log(entry);
    }

    /// Get recent audit entries
    pub fn recent_entries(&self, count: usize) -> Vec<AuditEntry> {
        let entries = self.entries.read();
        entries.iter().rev().take(count).cloned().collect()
    }

    /// Get entries by severity
    pub fn entries_by_severity(&self, severity: AuditSeverity) -> Vec<AuditEntry> {
        let entries = self.entries.read();
        entries
            .iter()
            .filter(|e| e.severity >= severity)
            .cloned()
            .collect()
    }

    /// Get entries for a specific host
    pub fn entries_for_host(&self, host: &str) -> Vec<AuditEntry> {
        let entries = self.entries.read();
        entries.iter().filter(|e| e.host == host).cloned().collect()
    }

    /// Get current session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Export entries as JSON
    pub fn export_json(&self) -> SecurityResult<String> {
        let entries = self.entries.read();
        serde_json::to_string_pretty(&*entries).map_err(|e| {
            super::SecurityError::AuditFailed(format!("Failed to export audit log: {}", e))
        })
    }

    /// Get entry count
    pub fn entry_count(&self) -> usize {
        self.entries.read().len()
    }

    /// Clear all entries (for testing)
    #[cfg(test)]
    pub fn clear(&self) {
        self.entries.write().clear();
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_entry_creation() {
        let entry = AuditEntry::new(AuditEventType::EscalationStart, AuditSeverity::Info, "host1")
            .with_target_user("root")
            .with_method("sudo")
            .with_command("apt install nginx");

        assert_eq!(entry.host, "host1");
        assert_eq!(entry.target_user, Some("root".to_string()));
        assert_eq!(entry.method, Some("sudo".to_string()));
        assert!(entry.command.is_some());
    }

    #[test]
    fn test_command_sanitization() {
        // Test password redaction
        let cmd = "mysql -p secretpassword";
        let sanitized = sanitize_command(cmd);
        assert!(sanitized.contains("[REDACTED]") || !sanitized.contains("secretpassword"));

        // Test truncation
        let long_cmd = "echo ".to_string() + &"a".repeat(300);
        let sanitized = sanitize_command(&long_cmd);
        assert!(sanitized.len() < 250);
        assert!(sanitized.ends_with("..."));
    }

    #[test]
    fn test_audit_logger_basic() {
        let logger = AuditLogger::new();

        logger.log_escalation_start("host1", "root", "sudo", "apt update");
        logger.log_escalation_success("host1", "root", "sudo", "apt update", 1500);

        assert_eq!(logger.entry_count(), 2);

        let entries = logger.recent_entries(10);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].event_type, AuditEventType::EscalationSuccess);
    }

    #[test]
    fn test_severity_filtering() {
        let config = AuditConfig {
            min_severity: AuditSeverity::Warning,
            ..Default::default()
        };
        let logger = AuditLogger::with_config(config);

        // Info should be filtered
        let entry = AuditEntry::new(AuditEventType::EscalationStart, AuditSeverity::Info, "host1");
        logger.log(entry);

        // Warning should pass
        let entry =
            AuditEntry::new(AuditEventType::PolicyViolation, AuditSeverity::Warning, "host1");
        logger.log(entry);

        assert_eq!(logger.entry_count(), 1);
    }

    #[test]
    fn test_max_entries_limit() {
        let config = AuditConfig {
            max_memory_entries: 5,
            ..Default::default()
        };
        let logger = AuditLogger::with_config(config);

        for i in 0..10 {
            let entry = AuditEntry::new(
                AuditEventType::EscalationStart,
                AuditSeverity::Info,
                &format!("host{}", i),
            );
            logger.log(entry);
        }

        assert_eq!(logger.entry_count(), 5);
        // Should have hosts 5-9 (oldest dropped)
        let entries = logger.recent_entries(10);
        assert!(entries.iter().any(|e| e.host == "host9"));
        assert!(!entries.iter().any(|e| e.host == "host0"));
    }

    #[test]
    fn test_entries_by_host() {
        let logger = AuditLogger::new();

        logger.log_escalation_start("host1", "root", "sudo", "cmd1");
        logger.log_escalation_start("host2", "root", "sudo", "cmd2");
        logger.log_escalation_start("host1", "admin", "sudo", "cmd3");

        let host1_entries = logger.entries_for_host("host1");
        assert_eq!(host1_entries.len(), 2);
    }

    #[test]
    fn test_audit_entry_format() {
        let entry = AuditEntry::new(AuditEventType::EscalationSuccess, AuditSeverity::Info, "host1")
            .with_target_user("root")
            .with_method("sudo")
            .with_command("apt update")
            .with_duration_ms(1500);

        let log_line = entry.format_log_line();

        assert!(log_line.contains("[INFO]"));
        assert!(log_line.contains("[ESCALATION_SUCCESS]"));
        assert!(log_line.contains("host=host1"));
        assert!(log_line.contains("target_user=root"));
        assert!(log_line.contains("duration_ms=1500"));
    }
}
