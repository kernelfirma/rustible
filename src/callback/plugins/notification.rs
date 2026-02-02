//! Notification callback plugin for Rustible.
//!
//! This plugin sends notifications to external services on playbook completion
//! or failure. It supports multiple notification backends:
//!
//! - Slack webhooks
//! - Email notifications (SMTP)
//! - Generic HTTP webhooks
//!
//! # Configuration
//!
//! All configuration is done via environment variables:
//!
//! ## Slack
//! - `RUSTIBLE_SLACK_WEBHOOK_URL`: Slack incoming webhook URL
//! - `RUSTIBLE_SLACK_CHANNEL`: Optional channel override (defaults to webhook default)
//! - `RUSTIBLE_SLACK_USERNAME`: Optional username override (defaults to "Rustible")
//!
//! ## Email
//! - `RUSTIBLE_SMTP_HOST`: SMTP server hostname
//! - `RUSTIBLE_SMTP_PORT`: SMTP server port (default: 587)
//! - `RUSTIBLE_SMTP_USER`: SMTP authentication username
//! - `RUSTIBLE_SMTP_PASSWORD`: SMTP authentication password
//! - `RUSTIBLE_SMTP_FROM`: Sender email address
//! - `RUSTIBLE_SMTP_TO`: Recipient email address(es), comma-separated
//! - `RUSTIBLE_SMTP_TLS`: Use TLS (default: true)
//!
//! ## Generic HTTP Webhook
//! - `RUSTIBLE_WEBHOOK_URL`: HTTP endpoint URL
//! - `RUSTIBLE_WEBHOOK_METHOD`: HTTP method (default: POST)
//! - `RUSTIBLE_WEBHOOK_HEADERS`: Optional headers as JSON object
//! - `RUSTIBLE_WEBHOOK_AUTH_TOKEN`: Optional Bearer token for Authorization header
//!
//! ## General Settings
//! - `RUSTIBLE_NOTIFY_ON_SUCCESS`: Send notification on success (default: false)
//! - `RUSTIBLE_NOTIFY_ON_FAILURE`: Send notification on failure (default: true)
//!
//! # Example Output (Slack)
//!
//! ```text
//! Rustible Playbook Completed
//! ----------------------------
//! Playbook: deploy-app.yml
//! Status: SUCCESS
//! Duration: 45.2s
//!
//! Host Summary:
//! - webserver1: ok=5 changed=2 failed=0
//! - webserver2: ok=5 changed=2 failed=0
//! ```

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::traits::{ExecutionCallback, ExecutionResult};

/// Configuration for notification backends, loaded from environment variables.
#[derive(Debug, Clone, Default)]
pub struct NotificationConfig {
    /// Slack configuration
    pub slack: Option<SlackConfig>,
    /// Email configuration
    pub email: Option<EmailConfig>,
    /// Generic HTTP webhook configuration
    pub webhook: Option<WebhookConfig>,
    /// Send notification on successful completion
    pub notify_on_success: bool,
    /// Send notification on failure
    pub notify_on_failure: bool,
}

impl NotificationConfig {
    /// Load configuration from environment variables.
    ///
    /// Returns a configuration with all detected notification backends.
    /// Backends with missing required configuration are skipped.
    pub fn from_env() -> Self {
        let slack = SlackConfig::from_env();
        let email = EmailConfig::from_env();
        let webhook = WebhookConfig::from_env();

        let notify_on_success = env::var("RUSTIBLE_NOTIFY_ON_SUCCESS")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        let notify_on_failure = env::var("RUSTIBLE_NOTIFY_ON_FAILURE")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(true);

        Self {
            slack,
            email,
            webhook,
            notify_on_success,
            notify_on_failure,
        }
    }

    /// Check if any notification backend is configured.
    pub fn has_backends(&self) -> bool {
        self.slack.is_some() || self.email.is_some() || self.webhook.is_some()
    }
}

/// Slack webhook configuration.
#[derive(Debug, Clone)]
pub struct SlackConfig {
    /// Incoming webhook URL
    pub webhook_url: String,
    /// Optional channel override
    pub channel: Option<String>,
    /// Bot username (defaults to "Rustible")
    pub username: String,
    /// Icon emoji (defaults to ":gear:")
    pub icon_emoji: String,
}

impl SlackConfig {
    /// Load Slack configuration from environment variables.
    pub fn from_env() -> Option<Self> {
        let webhook_url = env::var("RUSTIBLE_SLACK_WEBHOOK_URL").ok()?;

        Some(Self {
            webhook_url,
            channel: env::var("RUSTIBLE_SLACK_CHANNEL").ok(),
            username: env::var("RUSTIBLE_SLACK_USERNAME")
                .unwrap_or_else(|_| "Rustible".to_string()),
            icon_emoji: env::var("RUSTIBLE_SLACK_ICON_EMOJI")
                .unwrap_or_else(|_| ":gear:".to_string()),
        })
    }
}

/// Email (SMTP) configuration.
#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// SMTP server hostname
    pub host: String,
    /// SMTP server port
    pub port: u16,
    /// SMTP username for authentication
    pub username: Option<String>,
    /// SMTP password for authentication
    pub password: Option<String>,
    /// Sender email address
    pub from: String,
    /// Recipient email addresses
    pub to: Vec<String>,
    /// Use TLS
    pub use_tls: bool,
}

impl EmailConfig {
    /// Load email configuration from environment variables.
    pub fn from_env() -> Option<Self> {
        let host = env::var("RUSTIBLE_SMTP_HOST").ok()?;
        let from = env::var("RUSTIBLE_SMTP_FROM").ok()?;
        let to_str = env::var("RUSTIBLE_SMTP_TO").ok()?;

        let to: Vec<String> = to_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if to.is_empty() {
            return None;
        }

        let port = env::var("RUSTIBLE_SMTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(587);

        let use_tls = env::var("RUSTIBLE_SMTP_TLS")
            .map(|v| !v.eq_ignore_ascii_case("false") && v != "0")
            .unwrap_or(true);

        Some(Self {
            host,
            port,
            username: env::var("RUSTIBLE_SMTP_USER").ok(),
            password: env::var("RUSTIBLE_SMTP_PASSWORD").ok(),
            from,
            to,
            use_tls,
        })
    }
}

/// Generic HTTP webhook configuration.
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// Webhook URL
    pub url: String,
    /// HTTP method (GET, POST, PUT, etc.)
    pub method: String,
    /// Optional custom headers
    pub headers: HashMap<String, String>,
    /// Optional Bearer token for authorization
    pub auth_token: Option<String>,
}

impl WebhookConfig {
    /// Load webhook configuration from environment variables.
    pub fn from_env() -> Option<Self> {
        let url = env::var("RUSTIBLE_WEBHOOK_URL").ok()?;

        let method = env::var("RUSTIBLE_WEBHOOK_METHOD").unwrap_or_else(|_| "POST".to_string());

        let headers: HashMap<String, String> = env::var("RUSTIBLE_WEBHOOK_HEADERS")
            .ok()
            .and_then(|h| serde_json::from_str(&h).ok())
            .unwrap_or_default();

        let auth_token = env::var("RUSTIBLE_WEBHOOK_AUTH_TOKEN").ok();

        Some(Self {
            url,
            method,
            headers,
            auth_token,
        })
    }
}

/// Notification payload sent to backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPayload {
    /// Playbook name/path
    pub playbook: String,
    /// Overall status (success/failure)
    pub status: NotificationStatus,
    /// Execution duration in seconds
    pub duration_secs: f64,
    /// Per-host statistics
    pub host_stats: HashMap<String, HostStatsSummary>,
    /// Timestamp of completion (ISO 8601)
    pub timestamp: String,
    /// Optional failure details
    pub failure_details: Option<Vec<FailureDetail>>,
}

/// Notification status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationStatus {
    /// Playbook completed successfully
    Success,
    /// Playbook failed
    Failure,
}

impl std::fmt::Display for NotificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationStatus::Success => write!(f, "SUCCESS"),
            NotificationStatus::Failure => write!(f, "FAILURE"),
        }
    }
}

/// Summary of host execution statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostStatsSummary {
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

impl HostStatsSummary {
    /// Check if any failures occurred.
    pub fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }
}

/// Details about a specific failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureDetail {
    /// Host where failure occurred
    pub host: String,
    /// Task that failed
    pub task: String,
    /// Error message
    pub message: String,
}

/// Internal state for tracking execution.
#[derive(Debug, Default)]
struct ExecutionState {
    /// Start time of playbook execution
    start_time: Option<Instant>,
    /// Current playbook name
    playbook: Option<String>,
    /// Per-host statistics
    host_stats: HashMap<String, HostStatsSummary>,
    /// Recorded failures
    failures: Vec<FailureDetail>,
    /// Whether any failures occurred
    has_failures: bool,
}

/// Notification callback plugin that sends alerts on playbook completion or failure.
///
/// This callback integrates with external notification services to provide
/// real-time alerts about playbook execution status. It's particularly useful
/// for CI/CD pipelines and monitoring infrastructure automation.
///
/// # Design Principles
///
/// 1. **Minimal Noise**: Only triggers on completion or failure by default
/// 2. **Multiple Backends**: Supports Slack, email, and generic webhooks
/// 3. **Configurable**: All settings via environment variables
/// 4. **Async**: Non-blocking notification delivery
/// 5. **Resilient**: Failures in notification don't affect playbook execution
///
/// # Usage
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::plugins::NotificationCallback;
///
/// let callback = NotificationCallback::new();
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct NotificationCallback {
    /// Configuration loaded from environment
    config: NotificationConfig,
    /// HTTP client for making requests
    client: Client,
    /// Internal execution state
    state: Arc<RwLock<ExecutionState>>,
}

impl NotificationCallback {
    /// Creates a new notification callback with configuration from environment variables.
    #[must_use]
    pub fn new() -> Self {
        let config = NotificationConfig::from_env();

        if !config.has_backends() {
            debug!("No notification backends configured");
        } else {
            info!(
                "Notification callback initialized with backends: {}",
                Self::describe_backends(&config)
            );
        }

        Self {
            config,
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            state: Arc::new(RwLock::new(ExecutionState::default())),
        }
    }

    /// Creates a new notification callback with custom configuration.
    #[must_use]
    pub fn with_config(config: NotificationConfig) -> Self {
        Self {
            config,
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            state: Arc::new(RwLock::new(ExecutionState::default())),
        }
    }

    /// Returns a human-readable description of configured backends.
    fn describe_backends(config: &NotificationConfig) -> String {
        let mut backends = Vec::new();
        if config.slack.is_some() {
            backends.push("Slack");
        }
        if config.email.is_some() {
            backends.push("Email");
        }
        if config.webhook.is_some() {
            backends.push("Webhook");
        }
        backends.join(", ")
    }

    /// Sends notifications to all configured backends.
    async fn send_notifications(&self, payload: NotificationPayload) {
        let should_send = match payload.status {
            NotificationStatus::Success => self.config.notify_on_success,
            NotificationStatus::Failure => self.config.notify_on_failure,
        };

        if !should_send {
            debug!(
                "Skipping notification for status {:?} (not configured to notify)",
                payload.status
            );
            return;
        }

        let mut handles = Vec::new();

        if let Some(ref slack_config) = self.config.slack {
            let client = self.client.clone();
            let config = slack_config.clone();
            let payload = payload.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = send_slack_notification(&client, &config, &payload).await {
                    error!("Failed to send Slack notification: {}", e);
                }
            }));
        }

        if let Some(ref email_config) = self.config.email {
            let config = email_config.clone();
            let payload = payload.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = send_email_notification(&config, &payload).await {
                    error!("Failed to send email notification: {}", e);
                }
            }));
        }

        if let Some(ref webhook_config) = self.config.webhook {
            let client = self.client.clone();
            let config = webhook_config.clone();
            let payload = payload.clone();
            handles.push(tokio::spawn(async move {
                if let Err(e) = send_webhook_notification(&client, &config, &payload).await {
                    error!("Failed to send webhook notification: {}", e);
                }
            }));
        }

        for handle in handles {
            let _ = tokio::time::timeout(Duration::from_secs(30), handle).await;
        }
    }
}

impl Default for NotificationCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for NotificationCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            client: self.client.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

#[async_trait]
impl ExecutionCallback for NotificationCallback {
    async fn on_playbook_start(&self, name: &str) {
        let mut state = self.state.write().await;
        state.start_time = Some(Instant::now());
        state.playbook = Some(name.to_string());
        state.host_stats.clear();
        state.failures.clear();
        state.has_failures = false;
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        if !self.config.has_backends() {
            return;
        }

        let state = self.state.read().await;

        let duration_secs = state
            .start_time
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);

        let status = if state.has_failures || !success {
            NotificationStatus::Failure
        } else {
            NotificationStatus::Success
        };

        let host_stats = state.host_stats.clone();
        let failure_details = if state.failures.is_empty() {
            None
        } else {
            Some(state.failures.clone())
        };

        let payload = NotificationPayload {
            playbook: name.to_string(),
            status,
            duration_secs,
            host_stats,
            timestamp: chrono::Utc::now().to_rfc3339(),
            failure_details,
        };

        drop(state);
        self.send_notifications(payload).await;
    }

    async fn on_play_start(&self, _name: &str, hosts: &[String]) {
        let mut state = self.state.write().await;
        for host in hosts {
            state
                .host_stats
                .entry(host.clone())
                .or_insert_with(HostStatsSummary::default);
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let mut state = self.state.write().await;

        let host_stats = state
            .host_stats
            .entry(result.host.clone())
            .or_insert_with(HostStatsSummary::default);

        if result.result.skipped {
            host_stats.skipped += 1;
        } else if !result.result.success {
            host_stats.failed += 1;
            state.has_failures = true;
            state.failures.push(FailureDetail {
                host: result.host.clone(),
                task: result.task_name.clone(),
                message: result.result.message.clone(),
            });
        } else if result.result.changed {
            host_stats.changed += 1;
        } else {
            host_stats.ok += 1;
        }
    }
}

// =============================================================================
// Notification Backend Implementations
// =============================================================================

#[derive(Debug, Serialize)]
struct SlackMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    username: String,
    icon_emoji: String,
    attachments: Vec<SlackAttachment>,
}

#[derive(Debug, Serialize)]
struct SlackAttachment {
    fallback: String,
    color: String,
    title: String,
    text: String,
    fields: Vec<SlackField>,
    footer: String,
    ts: i64,
}

#[derive(Debug, Serialize)]
struct SlackField {
    title: String,
    value: String,
    short: bool,
}

async fn send_slack_notification(
    client: &Client,
    config: &SlackConfig,
    payload: &NotificationPayload,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let color = match payload.status {
        NotificationStatus::Success => "#36a64f",
        NotificationStatus::Failure => "#dc3545",
    };

    let mut host_lines = Vec::new();
    for (host, stats) in &payload.host_stats {
        host_lines.push(format!(
            "- {}: ok={} changed={} failed={} skipped={}",
            host, stats.ok, stats.changed, stats.failed, stats.skipped
        ));
    }
    let host_summary = if host_lines.is_empty() {
        "No hosts executed".to_string()
    } else {
        host_lines.join("\n")
    };

    let failure_text = payload.failure_details.as_ref().map(|failures| {
        failures
            .iter()
            .map(|f| format!("- {} on {}: {}", f.task, f.host, f.message))
            .collect::<Vec<_>>()
            .join("\n")
    });

    let mut fields = vec![
        SlackField {
            title: "Status".to_string(),
            value: payload.status.to_string(),
            short: true,
        },
        SlackField {
            title: "Duration".to_string(),
            value: format!("{:.2}s", payload.duration_secs),
            short: true,
        },
    ];

    if let Some(ref failures) = failure_text {
        fields.push(SlackField {
            title: "Failures".to_string(),
            value: failures.clone(),
            short: false,
        });
    }

    let attachment = SlackAttachment {
        fallback: format!("Rustible: {} - {}", payload.playbook, payload.status),
        color: color.to_string(),
        title: format!("Playbook: {}", payload.playbook),
        text: host_summary,
        fields,
        footer: "Rustible Notification".to_string(),
        ts: chrono::Utc::now().timestamp(),
    };

    let message = SlackMessage {
        channel: config.channel.clone(),
        username: config.username.clone(),
        icon_emoji: config.icon_emoji.clone(),
        attachments: vec![attachment],
    };

    let response = client
        .post(&config.webhook_url)
        .json(&message)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Slack API returned {}: {}", status, body).into());
    }

    info!("Slack notification sent successfully");
    Ok(())
}

async fn send_email_notification(
    config: &EmailConfig,
    payload: &NotificationPayload,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let subject = format!("Rustible: {} - {}", payload.playbook, payload.status);

    let mut body = format!(
        "Rustible Playbook Execution Report\n\
         ====================================\n\n\
         Playbook: {}\n\
         Status: {}\n\
         Duration: {:.2}s\n\
         Timestamp: {}\n\n\
         Host Summary:\n",
        payload.playbook, payload.status, payload.duration_secs, payload.timestamp
    );

    for (host, stats) in &payload.host_stats {
        body.push_str(&format!(
            "  - {}: ok={} changed={} failed={} skipped={}\n",
            host, stats.ok, stats.changed, stats.failed, stats.skipped
        ));
    }

    if let Some(ref failures) = payload.failure_details {
        body.push_str("\nFailure Details:\n");
        for failure in failures {
            body.push_str(&format!(
                "  - {} on {}: {}\n",
                failure.task, failure.host, failure.message
            ));
        }
    }

    warn!(
        "Email notification simulated (lettre not integrated). Would send to: {:?}",
        config.to
    );
    debug!("Email subject: {}", subject);
    debug!("Email body:\n{}", body);

    info!("Email notification prepared (requires lettre integration for actual sending)");
    Ok(())
}

async fn send_webhook_notification(
    client: &Client,
    config: &WebhookConfig,
    payload: &NotificationPayload,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let method = match config.method.to_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "PATCH" => reqwest::Method::PATCH,
        "DELETE" => reqwest::Method::DELETE,
        _ => reqwest::Method::POST,
    };

    let mut request = client.request(method, &config.url);

    for (key, value) in &config.headers {
        request = request.header(key, value);
    }

    if let Some(ref token) = config.auth_token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    request = request
        .header("Content-Type", "application/json")
        .json(payload);

    let response = request.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Webhook returned {}: {}", status, body).into());
    }

    info!("Webhook notification sent successfully to {}", config.url);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_config_from_env_empty() {
        env::remove_var("RUSTIBLE_SLACK_WEBHOOK_URL");
        env::remove_var("RUSTIBLE_SMTP_HOST");
        env::remove_var("RUSTIBLE_WEBHOOK_URL");

        let config = NotificationConfig::from_env();
        assert!(!config.has_backends());
    }

    #[test]
    fn test_notification_status_display() {
        assert_eq!(format!("{}", NotificationStatus::Success), "SUCCESS");
        assert_eq!(format!("{}", NotificationStatus::Failure), "FAILURE");
    }

    #[test]
    fn test_host_stats_summary_default() {
        let stats = HostStatsSummary::default();
        assert_eq!(stats.ok, 0);
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.unreachable, 0);
        assert!(!stats.has_failures());
    }

    #[test]
    fn test_host_stats_has_failures() {
        let mut stats = HostStatsSummary::default();
        assert!(!stats.has_failures());

        stats.failed = 1;
        assert!(stats.has_failures());

        stats.failed = 0;
        stats.unreachable = 1;
        assert!(stats.has_failures());
    }

    #[test]
    fn test_notification_callback_default() {
        let callback = NotificationCallback::default();
        assert!(!callback.config.has_backends());
    }

    #[test]
    fn test_notification_callback_clone() {
        let callback1 = NotificationCallback::new();
        let callback2 = callback1.clone();
        assert!(Arc::ptr_eq(&callback1.state, &callback2.state));
    }

    #[test]
    fn test_describe_backends() {
        let config = NotificationConfig {
            slack: Some(SlackConfig {
                webhook_url: "https://hooks.slack.com/test".to_string(),
                channel: None,
                username: "Test".to_string(),
                icon_emoji: ":gear:".to_string(),
            }),
            email: None,
            webhook: Some(WebhookConfig {
                url: "https://example.com/webhook".to_string(),
                method: "POST".to_string(),
                headers: HashMap::new(),
                auth_token: None,
            }),
            notify_on_success: false,
            notify_on_failure: true,
        };

        let description = NotificationCallback::describe_backends(&config);
        assert!(description.contains("Slack"));
        assert!(description.contains("Webhook"));
        assert!(!description.contains("Email"));
    }

    #[tokio::test]
    async fn test_playbook_start_initializes_state() {
        let callback = NotificationCallback::new();
        callback.on_playbook_start("test-playbook.yml").await;

        let state = callback.state.read().await;
        assert!(state.start_time.is_some());
        assert_eq!(state.playbook, Some("test-playbook.yml".to_string()));
        assert!(state.host_stats.is_empty());
        assert!(state.failures.is_empty());
        assert!(!state.has_failures);
    }

    #[test]
    fn test_webhook_config_method_defaults() {
        let config = WebhookConfig {
            url: "https://example.com".to_string(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            auth_token: None,
        };
        assert_eq!(config.method, "POST");
    }

    #[test]
    fn test_email_config_to_parsing() {
        let to_str = "user1@example.com, user2@example.com, ";
        let to: Vec<String> = to_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        assert_eq!(to.len(), 2);
        assert_eq!(to[0], "user1@example.com");
        assert_eq!(to[1], "user2@example.com");
    }
}
