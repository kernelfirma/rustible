//! Audit logging system for Rustible operations
//!
//! This module provides comprehensive audit logging capabilities for tracking
//! privileged operations, file modifications, command executions, and other
//! security-relevant events during playbook execution.
//!
//! # Features
//!
//! - **Event Types**: Command execution, file modifications, privilege escalation,
//!   authentication, service management, user management, and more
//! - **Multiple Backends**: File logging with rotation, syslog, journald, and console output
//! - **Flexible Formats**: Text, JSON, and Common Event Format (CEF) output
//! - **Session Correlation**: Track related events across a playbook run
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::audit::{AuditEvent, AuditManager, FileLogger};
//!
//! // Create an audit manager with file logging
//! let mut manager = AuditManager::new();
//! manager.add_logger(std::sync::Arc::new(FileLogger::new("/var/log/rustible/audit.log")?));
//!
//! // Log a command execution event
//! let event = AuditEvent::command_execution("apt-get update")
//!     .with_host("webserver01")
//!     .with_module("apt")
//!     .with_privilege("sudo", Some("root".to_string()))
//!     .success();
//!
//! manager.log(event)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Security Considerations
//!
//! Audit logs may contain sensitive information. Consider:
//! - Restricting file permissions on audit log files
//! - Using encrypted transport for remote syslog
//! - Implementing log retention policies
//! - Monitoring for audit log tampering

mod event;
pub mod hashchain;
pub mod immutable;
mod logger;
pub mod sink;
pub mod verify;

// Re-export main types
pub use event::{AuditCategory, AuditEvent, AuditOutcome, AuditSeverity};
pub use logger::{
    AuditFormat, AuditLogError, AuditLogResult, AuditLogger, ConsoleLogger, FileLogger,
    JournaldLogger, SyslogFacility, SyslogLogger, SyslogTransport,
};

use std::sync::Arc;

/// Manager for coordinating multiple audit loggers
#[derive(Default)]
pub struct AuditManager {
    /// Registered loggers
    loggers: Vec<Arc<dyn AuditLogger>>,
    /// Session ID for event correlation
    session_id: Option<String>,
    /// Whether to continue on logger errors
    fail_open: bool,
}

impl AuditManager {
    /// Create a new audit manager
    pub fn new() -> Self {
        Self {
            loggers: Vec::new(),
            session_id: None,
            fail_open: true,
        }
    }

    /// Add a logger to the manager
    pub fn add_logger(&mut self, logger: Arc<dyn AuditLogger>) {
        self.loggers.push(logger);
    }

    /// Set the session ID for event correlation
    pub fn set_session(&mut self, session_id: impl Into<String>) {
        self.session_id = Some(session_id.into());
    }

    /// Set whether to fail open (continue on logger errors)
    pub fn set_fail_open(&mut self, fail_open: bool) {
        self.fail_open = fail_open;
    }

    /// Log an event to all registered loggers
    pub fn log(&self, mut event: AuditEvent) -> AuditLogResult<()> {
        // Add session ID if set
        if let Some(ref session_id) = self.session_id {
            if event.session_id.is_none() {
                event.session_id = Some(session_id.clone());
            }
        }

        let mut last_error = None;

        for logger in &self.loggers {
            if let Err(e) = logger.log(&event) {
                if self.fail_open {
                    // Log the error but continue
                    eprintln!("Audit logger {} failed: {}", logger.name(), e);
                    last_error = Some(e);
                } else {
                    return Err(e);
                }
            }
        }

        // If all loggers failed and we're in fail-open mode, return the last error
        if self.loggers.is_empty() {
            return Ok(());
        }

        if !self.fail_open {
            if let Some(e) = last_error {
                return Err(e);
            }
        }

        Ok(())
    }

    /// Flush all loggers
    pub fn flush(&self) -> AuditLogResult<()> {
        for logger in &self.loggers {
            logger.flush()?;
        }
        Ok(())
    }

    /// Get the number of registered loggers
    pub fn logger_count(&self) -> usize {
        self.loggers.len()
    }

    /// Check if any loggers are available
    pub fn has_available_loggers(&self) -> bool {
        self.loggers.iter().any(|l| l.is_available())
    }
}

impl std::fmt::Debug for AuditManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditManager")
            .field("logger_count", &self.loggers.len())
            .field("session_id", &self.session_id)
            .field("fail_open", &self.fail_open)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_manager_creation() {
        let manager = AuditManager::new();
        assert_eq!(manager.logger_count(), 0);
    }

    #[test]
    fn test_audit_manager_with_console() {
        let mut manager = AuditManager::new();
        manager.add_logger(Arc::new(ConsoleLogger::new()));
        manager.set_session("test-session-123");

        assert_eq!(manager.logger_count(), 1);
        assert!(manager.has_available_loggers());

        let event = AuditEvent::command_execution("echo test").success();
        manager.log(event).unwrap();
    }

    #[test]
    fn test_audit_manager_fail_open() {
        let manager = AuditManager::new();
        // With no loggers, should succeed in fail-open mode
        let event = AuditEvent::command_execution("test");
        assert!(manager.log(event).is_ok());
    }
}
