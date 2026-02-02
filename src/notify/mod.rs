//! # Notification System for Rustible
//!
//! This module provides a comprehensive notification system for sending alerts
//! about playbook execution to various backends including Slack, email, and webhooks.
//!
//! ## Features
//!
//! - **Multiple Backends**: Slack, Email (SMTP), and generic HTTP webhooks
//! - **Template Support**: Customize notification messages using Jinja2-style templates
//! - **Filtering**: Control which events trigger notifications with flexible rules
//! - **Async**: Non-blocking notification delivery
//! - **Resilient**: Notification failures don't affect playbook execution
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::notify::{NotificationConfig, NotificationEvent, NotificationManager};
//!
//! // Create from environment variables
//! let manager = NotificationManager::from_env();
//!
//! // Or with explicit configuration
//! let config = NotificationConfig::builder()
//!     .slack_webhook("https://hooks.slack.com/services/...")
//!     .notify_on_success(true)
//!     .notify_on_failure(true)
//!     .build()?;
//! let manager = NotificationManager::new(config);
//!
//! // Send a notification
//! manager.notify(&NotificationEvent::PlaybookComplete {
//!     playbook: "deploy.yml".to_string(),
//!     success: true,
//!     duration_secs: 45.2,
//!     host_stats: Default::default(),
//!     timestamp: "2024-01-01T00:00:00Z".to_string(),
//!     failures: None,
//! }).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Environment Variables
//!
//! ### Slack
//! - `RUSTIBLE_SLACK_WEBHOOK_URL`: Slack incoming webhook URL
//! - `RUSTIBLE_SLACK_CHANNEL`: Optional channel override
//! - `RUSTIBLE_SLACK_USERNAME`: Bot username (default: "Rustible")
//! - `RUSTIBLE_SLACK_ICON_EMOJI`: Bot icon emoji (default: ":gear:")
//!
//! ### Email (SMTP)
//! - `RUSTIBLE_SMTP_HOST`: SMTP server hostname
//! - `RUSTIBLE_SMTP_PORT`: SMTP server port (default: 587)
//! - `RUSTIBLE_SMTP_USER`: SMTP authentication username
//! - `RUSTIBLE_SMTP_PASSWORD`: SMTP authentication password
//! - `RUSTIBLE_SMTP_FROM`: Sender email address
//! - `RUSTIBLE_SMTP_TO`: Recipient email addresses (comma-separated)
//! - `RUSTIBLE_SMTP_TLS`: TLS mode (default: "starttls")
//!
//! ### Webhook
//! - `RUSTIBLE_WEBHOOK_URL`: HTTP endpoint URL
//! - `RUSTIBLE_WEBHOOK_METHOD`: HTTP method (default: POST)
//! - `RUSTIBLE_WEBHOOK_HEADERS`: Custom headers as JSON object
//! - `RUSTIBLE_WEBHOOK_AUTH_TOKEN`: Bearer token for Authorization header
//!
//! ### General
//! - `RUSTIBLE_NOTIFY_ON_SUCCESS`: Send notification on success (default: false)
//! - `RUSTIBLE_NOTIFY_ON_FAILURE`: Send notification on failure (default: true)
//! - `RUSTIBLE_NOTIFY_TEMPLATE`: Custom message template path

mod config;
mod email;
mod error;
mod filter;
mod manager;
mod slack;
mod template;
#[cfg(test)]
mod test_support;
mod webhook;

pub use config::{
    EmailConfig, NotificationConfig, NotificationConfigBuilder, SlackConfig, WebhookConfig,
};
pub use email::EmailNotifier;
pub use error::{NotificationError, NotificationResult};
pub use filter::{NotificationFilter, NotificationFilterBuilder, NotificationRule};
pub use manager::NotificationManager;
pub use slack::SlackNotifier;
pub use template::{NotificationTemplate, TemplateContext};
pub use webhook::WebhookNotifier;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Events that can trigger notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationEvent {
    /// Playbook execution started.
    PlaybookStart {
        /// Playbook name/path
        playbook: String,
        /// Target hosts
        hosts: Vec<String>,
        /// Timestamp (ISO 8601)
        timestamp: String,
    },

    /// Playbook execution completed.
    PlaybookComplete {
        /// Playbook name/path
        playbook: String,
        /// Whether execution was successful
        success: bool,
        /// Execution duration in seconds
        duration_secs: f64,
        /// Per-host statistics
        host_stats: HashMap<String, HostStats>,
        /// Timestamp (ISO 8601)
        timestamp: String,
        /// Failure details (if any)
        #[serde(skip_serializing_if = "Option::is_none")]
        failures: Option<Vec<FailureInfo>>,
    },

    /// A task failed on a host.
    TaskFailed {
        /// Playbook name/path
        playbook: String,
        /// Task name
        task: String,
        /// Target host
        host: String,
        /// Error message
        error: String,
        /// Timestamp (ISO 8601)
        timestamp: String,
    },

    /// A host became unreachable.
    HostUnreachable {
        /// Playbook name/path
        playbook: String,
        /// Host that is unreachable
        host: String,
        /// Error message
        error: String,
        /// Timestamp (ISO 8601)
        timestamp: String,
    },

    /// Custom event with arbitrary data.
    Custom {
        /// Event name
        name: String,
        /// Event data
        data: serde_json::Value,
        /// Timestamp (ISO 8601)
        timestamp: String,
    },
}

impl NotificationEvent {
    /// Creates a playbook start event.
    pub fn playbook_start(playbook: impl Into<String>, hosts: Vec<String>) -> Self {
        Self::PlaybookStart {
            playbook: playbook.into(),
            hosts,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Creates a playbook complete event.
    pub fn playbook_complete(
        playbook: impl Into<String>,
        success: bool,
        duration: Duration,
        host_stats: HashMap<String, HostStats>,
        failures: Option<Vec<FailureInfo>>,
    ) -> Self {
        Self::PlaybookComplete {
            playbook: playbook.into(),
            success,
            duration_secs: duration.as_secs_f64(),
            host_stats,
            timestamp: chrono::Utc::now().to_rfc3339(),
            failures,
        }
    }

    /// Creates a task failed event.
    pub fn task_failed(
        playbook: impl Into<String>,
        task: impl Into<String>,
        host: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self::TaskFailed {
            playbook: playbook.into(),
            task: task.into(),
            host: host.into(),
            error: error.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Creates a host unreachable event.
    pub fn host_unreachable(
        playbook: impl Into<String>,
        host: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self::HostUnreachable {
            playbook: playbook.into(),
            host: host.into(),
            error: error.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Creates a custom event.
    pub fn custom(name: impl Into<String>, data: serde_json::Value) -> Self {
        Self::Custom {
            name: name.into(),
            data,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Returns the event type name.
    pub fn event_type(&self) -> &str {
        match self {
            Self::PlaybookStart { .. } => "playbook_start",
            Self::PlaybookComplete { .. } => "playbook_complete",
            Self::TaskFailed { .. } => "task_failed",
            Self::HostUnreachable { .. } => "host_unreachable",
            Self::Custom { name, .. } => name,
        }
    }

    /// Returns the playbook name (if applicable).
    pub fn playbook(&self) -> Option<&str> {
        match self {
            Self::PlaybookStart { playbook, .. }
            | Self::PlaybookComplete { playbook, .. }
            | Self::TaskFailed { playbook, .. }
            | Self::HostUnreachable { playbook, .. } => Some(playbook),
            Self::Custom { .. } => None,
        }
    }

    /// Returns whether this is a failure event.
    pub fn is_failure(&self) -> bool {
        match self {
            Self::PlaybookComplete { success, .. } => !success,
            Self::TaskFailed { .. } | Self::HostUnreachable { .. } => true,
            _ => false,
        }
    }

    /// Returns whether this is a success event.
    pub fn is_success(&self) -> bool {
        match self {
            Self::PlaybookComplete { success, .. } => *success,
            _ => false,
        }
    }
}

/// Statistics for a single host execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostStats {
    /// Tasks completed successfully without changes
    pub ok: u32,
    /// Tasks that made changes
    pub changed: u32,
    /// Tasks that failed
    pub failed: u32,
    /// Tasks that were skipped
    pub skipped: u32,
    /// Unreachable attempts
    pub unreachable: u32,
}

impl HostStats {
    /// Creates new host stats.
    pub fn new(ok: u32, changed: u32, failed: u32, skipped: u32, unreachable: u32) -> Self {
        Self {
            ok,
            changed,
            failed,
            skipped,
            unreachable,
        }
    }

    /// Returns the total number of tasks.
    pub fn total(&self) -> u32 {
        self.ok + self.changed + self.failed + self.skipped
    }

    /// Returns true if there are any failures.
    pub fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }
}

/// Information about a failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureInfo {
    /// Host where failure occurred
    pub host: String,
    /// Task that failed
    pub task: String,
    /// Error message
    pub message: String,
}

impl FailureInfo {
    /// Creates new failure info.
    pub fn new(
        host: impl Into<String>,
        task: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            task: task.into(),
            message: message.into(),
        }
    }
}

/// Notification severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Severity {
    /// Informational notification
    #[default]
    Info,
    /// Warning notification
    Warning,
    /// Error/failure notification
    Error,
    /// Critical notification
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Trait for notification backends.
#[async_trait::async_trait]
pub trait Notifier: Send + Sync + std::fmt::Debug {
    /// Returns the name of this notifier backend.
    fn name(&self) -> &str;

    /// Returns whether this notifier is configured and ready to send.
    fn is_configured(&self) -> bool;

    /// Sends a notification.
    async fn send(&self, event: &NotificationEvent) -> NotificationResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_event_playbook_start() {
        let event = NotificationEvent::playbook_start(
            "deploy.yml",
            vec!["host1".to_string(), "host2".to_string()],
        );
        assert_eq!(event.event_type(), "playbook_start");
        assert_eq!(event.playbook(), Some("deploy.yml"));
        assert!(!event.is_failure());
        assert!(!event.is_success());
    }

    #[test]
    fn test_notification_event_playbook_complete_success() {
        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            true,
            Duration::from_secs(45),
            HashMap::new(),
            None,
        );
        assert_eq!(event.event_type(), "playbook_complete");
        assert!(event.is_success());
        assert!(!event.is_failure());
    }

    #[test]
    fn test_notification_event_playbook_complete_failure() {
        let failures = vec![FailureInfo::new("host1", "task1", "Connection refused")];
        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            false,
            Duration::from_secs(10),
            HashMap::new(),
            Some(failures),
        );
        assert!(!event.is_success());
        assert!(event.is_failure());
    }

    #[test]
    fn test_notification_event_task_failed() {
        let event = NotificationEvent::task_failed("deploy.yml", "Copy file", "host1", "Timeout");
        assert_eq!(event.event_type(), "task_failed");
        assert!(event.is_failure());
    }

    #[test]
    fn test_host_stats() {
        let stats = HostStats::new(5, 2, 1, 0, 0);
        assert_eq!(stats.total(), 8);
        assert!(stats.has_failures());

        let stats_ok = HostStats::new(5, 2, 0, 1, 0);
        assert!(!stats_ok.has_failures());
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(Severity::Info.to_string(), "info");
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Critical.to_string(), "critical");
    }

    #[test]
    fn test_notification_event_serialization() {
        let event = NotificationEvent::task_failed("test.yml", "task1", "host1", "error");
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("task_failed"));
        assert!(json.contains("test.yml"));
    }
}
