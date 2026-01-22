//! Audit event types and definitions
//!
//! This module defines the core audit event types for tracking privileged operations,
//! file modifications, and command executions in Rustible.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::time::SystemTime;

/// Severity level for audit events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AuditSeverity {
    /// Informational events (routine operations)
    #[default]
    Info,
    /// Warning events (potential issues)
    Warning,
    /// Error events (operation failures)
    Error,
    /// Critical events (security-relevant operations)
    Critical,
}

impl fmt::Display for AuditSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditSeverity::Info => write!(f, "INFO"),
            AuditSeverity::Warning => write!(f, "WARNING"),
            AuditSeverity::Error => write!(f, "ERROR"),
            AuditSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Category of audit events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditCategory {
    /// Command execution events
    CommandExecution,
    /// File modification events
    FileModification,
    /// Privilege escalation events
    PrivilegeEscalation,
    /// Authentication events
    Authentication,
    /// Configuration changes
    ConfigurationChange,
    /// Service management events
    ServiceManagement,
    /// User/group management events
    UserManagement,
    /// Package management events
    PackageManagement,
    /// Network operations
    NetworkOperation,
    /// System state changes
    SystemState,
}

impl fmt::Display for AuditCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditCategory::CommandExecution => write!(f, "COMMAND_EXECUTION"),
            AuditCategory::FileModification => write!(f, "FILE_MODIFICATION"),
            AuditCategory::PrivilegeEscalation => write!(f, "PRIVILEGE_ESCALATION"),
            AuditCategory::Authentication => write!(f, "AUTHENTICATION"),
            AuditCategory::ConfigurationChange => write!(f, "CONFIGURATION_CHANGE"),
            AuditCategory::ServiceManagement => write!(f, "SERVICE_MANAGEMENT"),
            AuditCategory::UserManagement => write!(f, "USER_MANAGEMENT"),
            AuditCategory::PackageManagement => write!(f, "PACKAGE_MANAGEMENT"),
            AuditCategory::NetworkOperation => write!(f, "NETWORK_OPERATION"),
            AuditCategory::SystemState => write!(f, "SYSTEM_STATE"),
        }
    }
}

/// Outcome of an audited operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AuditOutcome {
    /// Operation completed successfully
    Success,
    /// Operation failed
    Failure,
    /// Operation was denied
    Denied,
    /// Operation was skipped
    Skipped,
    /// Operation outcome is unknown
    #[default]
    Unknown,
}

impl fmt::Display for AuditOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuditOutcome::Success => write!(f, "SUCCESS"),
            AuditOutcome::Failure => write!(f, "FAILURE"),
            AuditOutcome::Denied => write!(f, "DENIED"),
            AuditOutcome::Skipped => write!(f, "SKIPPED"),
            AuditOutcome::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

/// An audit event representing a logged operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event identifier
    pub event_id: String,
    /// Event timestamp
    #[serde(with = "system_time_serde")]
    pub timestamp: SystemTime,
    /// Event category
    pub category: AuditCategory,
    /// Event severity
    pub severity: AuditSeverity,
    /// Event outcome
    pub outcome: AuditOutcome,
    /// Actor who initiated the operation (username or process)
    pub actor: String,
    /// Target host where the operation occurred
    pub target_host: Option<String>,
    /// Module that generated the event
    pub module: Option<String>,
    /// Task name (if applicable)
    pub task: Option<String>,
    /// Playbook name (if applicable)
    pub playbook: Option<String>,
    /// Short description of the event
    pub message: String,
    /// Detailed description
    pub details: Option<String>,
    /// Associated file paths (for file operations)
    pub paths: Vec<String>,
    /// Command executed (for command operations)
    pub command: Option<String>,
    /// Whether privilege escalation was used
    pub privileged: bool,
    /// Privilege escalation method (sudo, su, etc.)
    pub escalation_method: Option<String>,
    /// Target user for privilege escalation
    pub escalation_user: Option<String>,
    /// Exit code (for command operations)
    pub exit_code: Option<i32>,
    /// Additional metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Session ID for correlation
    pub session_id: Option<String>,
}

impl AuditEvent {
    /// Create a new audit event with the given category and message
    pub fn new(category: AuditCategory, message: impl Into<String>) -> Self {
        Self {
            event_id: generate_event_id(),
            timestamp: SystemTime::now(),
            category,
            severity: AuditSeverity::Info,
            outcome: AuditOutcome::Unknown,
            actor: get_current_user(),
            target_host: None,
            module: None,
            task: None,
            playbook: None,
            message: message.into(),
            details: None,
            paths: Vec::new(),
            command: None,
            privileged: false,
            escalation_method: None,
            escalation_user: None,
            exit_code: None,
            metadata: HashMap::new(),
            session_id: None,
        }
    }

    /// Create a command execution event
    pub fn command_execution(command: impl Into<String>) -> Self {
        let cmd = command.into();
        let mut event = Self::new(
            AuditCategory::CommandExecution,
            format!("Command executed: {}", truncate_string(&cmd, 100)),
        );
        event.command = Some(cmd);
        event
    }

    /// Create a file modification event
    pub fn file_modification(path: impl Into<String>, operation: &str) -> Self {
        let path_str = path.into();
        let mut event = Self::new(
            AuditCategory::FileModification,
            format!("{} file: {}", operation, path_str),
        );
        event.paths.push(path_str);
        event
    }

    /// Create a privilege escalation event
    pub fn privilege_escalation(method: impl Into<String>, target_user: Option<String>) -> Self {
        let method_str = method.into();
        let mut event = Self::new(
            AuditCategory::PrivilegeEscalation,
            format!(
                "Privilege escalation via {}",
                if let Some(ref user) = target_user {
                    format!("{} to {}", method_str, user)
                } else {
                    method_str.clone()
                }
            ),
        );
        event.privileged = true;
        event.escalation_method = Some(method_str);
        event.escalation_user = target_user;
        event.severity = AuditSeverity::Critical;
        event
    }

    /// Create a service management event
    pub fn service_management(service: &str, action: &str) -> Self {
        Self::new(
            AuditCategory::ServiceManagement,
            format!("Service {}: {}", action, service),
        )
    }

    /// Create a user management event
    pub fn user_management(user: &str, action: &str) -> Self {
        let mut event = Self::new(
            AuditCategory::UserManagement,
            format!("User {}: {}", action, user),
        );
        event.severity = AuditSeverity::Warning;
        event
    }

    /// Create a package management event
    pub fn package_management(packages: &[String], action: &str) -> Self {
        Self::new(
            AuditCategory::PackageManagement,
            format!("Package {}: {}", action, packages.join(", ")),
        )
    }

    // Builder methods

    /// Set the severity level
    pub fn with_severity(mut self, severity: AuditSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Set the outcome
    pub fn with_outcome(mut self, outcome: AuditOutcome) -> Self {
        self.outcome = outcome;
        self
    }

    /// Set the target host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.target_host = Some(host.into());
        self
    }

    /// Set the module name
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module = Some(module.into());
        self
    }

    /// Set the task name
    pub fn with_task(mut self, task: impl Into<String>) -> Self {
        self.task = Some(task.into());
        self
    }

    /// Set the playbook name
    pub fn with_playbook(mut self, playbook: impl Into<String>) -> Self {
        self.playbook = Some(playbook.into());
        self
    }

    /// Add a file path
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.paths.push(path.into());
        self
    }

    /// Set the command
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Mark as privileged operation
    pub fn with_privilege(mut self, method: impl Into<String>, user: Option<String>) -> Self {
        self.privileged = true;
        self.escalation_method = Some(method.into());
        self.escalation_user = user;
        self
    }

    /// Set the exit code
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Set the session ID
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set details
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Mark as successful
    pub fn success(mut self) -> Self {
        self.outcome = AuditOutcome::Success;
        self
    }

    /// Mark as failed
    pub fn failure(mut self) -> Self {
        self.outcome = AuditOutcome::Failure;
        self
    }

    /// Format the event as a single log line
    pub fn to_log_line(&self) -> String {
        let timestamp = format_timestamp(&self.timestamp);
        let host = self.target_host.as_deref().unwrap_or("localhost");
        let module = self.module.as_deref().unwrap_or("-");

        format!(
            "{} {} {} [{}] {} actor={} host={} module={} outcome={} {}",
            timestamp,
            self.event_id,
            self.category,
            self.severity,
            self.message,
            self.actor,
            host,
            module,
            self.outcome,
            if self.privileged { "PRIVILEGED" } else { "" }
        )
        .trim_end()
        .to_string()
    }
}

/// Generate a unique event ID
fn generate_event_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    format!("{:x}-{:04x}", timestamp, counter as u16)
}

/// Get the current username
fn get_current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Truncate a string to a maximum length
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Format a SystemTime as ISO 8601
fn format_timestamp(time: &SystemTime) -> String {
    let duration = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    // Convert to components (simplified, doesn't handle leap seconds)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Simple date calculation from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, millis
    )
}

/// Convert days since Unix epoch to year/month/day
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Simplified algorithm for date conversion
    let mut remaining = days as i64;
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let days_in_months: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u32;
    for days_in_month in &days_in_months {
        if remaining < *days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }

    (year, month, (remaining + 1) as u32)
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Serde module for SystemTime serialization
mod system_time_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn test_command_execution_event() {
        let event = AuditEvent::command_execution("ls -la /etc")
            .with_host("server1")
            .with_module("command")
            .success();

        assert_eq!(event.category, AuditCategory::CommandExecution);
        assert_eq!(event.outcome, AuditOutcome::Success);
        assert_eq!(event.target_host, Some("server1".to_string()));
        assert!(event.command.as_ref().unwrap().contains("ls -la"));
    }

    #[test]
    fn test_file_modification_event() {
        let event = AuditEvent::file_modification("/etc/passwd", "Modified")
            .with_module("lineinfile")
            .success();

        assert_eq!(event.category, AuditCategory::FileModification);
        assert!(event.paths.contains(&"/etc/passwd".to_string()));
    }

    #[test]
    fn test_privilege_escalation_event() {
        let event =
            AuditEvent::privilege_escalation("sudo", Some("root".to_string())).with_host("server1");

        assert_eq!(event.category, AuditCategory::PrivilegeEscalation);
        assert_eq!(event.severity, AuditSeverity::Critical);
        assert!(event.privileged);
        assert_eq!(event.escalation_method, Some("sudo".to_string()));
        assert_eq!(event.escalation_user, Some("root".to_string()));
    }

    #[test]
    fn test_event_to_log_line() {
        let event = AuditEvent::command_execution("echo test")
            .with_host("server1")
            .with_module("shell")
            .success();

        let line = event.to_log_line();
        assert!(line.contains("COMMAND_EXECUTION"));
        assert!(line.contains("server1"));
        assert!(line.contains("SUCCESS"));
    }

    #[test]
    fn test_event_serialization() {
        let event = AuditEvent::file_modification("/tmp/test.txt", "Created")
            .with_metadata("size", serde_json::json!(1024));

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"category\":\"file_modification\""));

        let deserialized: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.category, AuditCategory::FileModification);
    }

    #[test]
    fn test_truncate_string() {
        let short = "short";
        assert_eq!(truncate_string(short, 10), "short");

        let long = "abcdefghijklmnopqrstuvwxyz";
        assert_eq!(truncate_string(long, 10), "abcdefg...");
    }

    #[test]
    fn test_format_timestamp_epoch() {
        let formatted = format_timestamp(&UNIX_EPOCH);
        assert_eq!(formatted, "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn test_format_timestamp_offset() {
        let time = UNIX_EPOCH + Duration::from_secs(3661);
        let formatted = format_timestamp(&time);
        assert!(formatted.contains("T01:01:01.000Z"));
    }

    #[test]
    fn test_days_to_ymd_basic() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        assert_eq!(days_to_ymd(365), (1971, 1, 1));
    }

    #[test]
    fn test_is_leap_year_rules() {
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2024));
    }

    #[test]
    fn test_log_line_privileged_marker() {
        let event = AuditEvent::command_execution("echo hello")
            .with_host("server1")
            .with_module("command")
            .with_privilege("sudo", Some("root".to_string()))
            .success();

        let line = event.to_log_line();
        assert!(line.contains("PRIVILEGED"));
    }
}
