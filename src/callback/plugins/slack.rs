//! Slack Callback Plugin for Rustible.
//!
//! This plugin sends real-time notifications to Slack channels during playbook
//! execution. It provides rich, formatted messages with execution status,
//! task progress, and failure details.
//!
//! # Features
//!
//! - Real-time playbook status updates
//! - Rich message formatting with attachments
//! - Configurable notification triggers (start, end, failure)
//! - Thread support for detailed task updates
//! - Rate limiting to prevent Slack API throttling
//! - Retry logic for transient failures
//!
//! # Configuration
//!
//! Configuration via environment variables:
//!
//! - `RUSTIBLE_SLACK_WEBHOOK_URL`: Incoming webhook URL (required)
//! - `RUSTIBLE_SLACK_CHANNEL`: Channel override (optional)
//! - `RUSTIBLE_SLACK_USERNAME`: Bot username (default: "Rustible")
//! - `RUSTIBLE_SLACK_ICON_EMOJI`: Bot icon (default: ":gear:")
//! - `RUSTIBLE_SLACK_NOTIFY_START`: Notify on playbook start (default: false)
//! - `RUSTIBLE_SLACK_NOTIFY_END`: Notify on playbook end (default: true)
//! - `RUSTIBLE_SLACK_NOTIFY_FAILURE`: Notify on task failure (default: true)
//! - `RUSTIBLE_SLACK_THREAD_TASKS`: Post task updates in thread (default: false)
//!
//! # Example Output
//!
//! ```text
//! :gear: Rustible Playbook Started
//! Playbook: deploy-app.yml
//! Hosts: webserver1, webserver2
//!
//! :white_check_mark: Rustible Playbook Completed
//! Playbook: deploy-app.yml
//! Duration: 45.2s
//! Status: SUCCESS
//!
//! Host Summary:
//! webserver1: ok=5 changed=2 failed=0
//! webserver2: ok=5 changed=2 failed=0
//! ```
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::plugins::{SlackCallback, SlackCallbackConfig};
//!
//! // From environment variables
//! let callback = SlackCallback::from_env()?;
//!
//! // With custom configuration
//! let config = SlackCallbackConfig::builder()
//!     .webhook_url("https://hooks.slack.com/services/...")
//!     .channel("#deployments")
//!     .notify_on_start(true)
//!     .build();
//! let callback = SlackCallback::new(config)?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during Slack operations.
#[derive(Debug, thiserror::Error)]
pub enum SlackError {
    /// Missing required configuration
    #[error("Missing required configuration: {0}")]
    MissingConfig(String),

    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    /// Slack API returned an error
    #[error("Slack API error: {0}")]
    ApiError(String),

    /// Rate limited by Slack
    #[error("Rate limited by Slack, retry after {0} seconds")]
    RateLimited(u64),

    /// JSON serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Result type for Slack operations.
pub type SlackResult<T> = Result<T, SlackError>;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the Slack callback plugin.
#[derive(Debug, Clone)]
pub struct SlackCallbackConfig {
    /// Slack incoming webhook URL
    pub webhook_url: String,
    /// Optional channel override
    pub channel: Option<String>,
    /// Bot username (default: "Rustible")
    pub username: String,
    /// Bot icon emoji (default: ":gear:")
    pub icon_emoji: String,
    /// Notify on playbook start
    pub notify_on_start: bool,
    /// Notify on playbook end
    pub notify_on_end: bool,
    /// Notify on task failure
    pub notify_on_failure: bool,
    /// Post task updates in a thread
    pub thread_tasks: bool,
    /// Include host details in messages
    pub include_host_details: bool,
    /// Maximum failures to include in message
    pub max_failures_shown: usize,
    /// HTTP timeout in seconds
    pub timeout_secs: u64,
    /// Number of retry attempts
    pub retry_attempts: u32,
    /// Minimum delay between messages (rate limiting)
    pub min_message_interval_ms: u64,
}

impl Default for SlackCallbackConfig {
    fn default() -> Self {
        Self {
            webhook_url: String::new(),
            channel: None,
            username: "Rustible".to_string(),
            icon_emoji: ":gear:".to_string(),
            notify_on_start: false,
            notify_on_end: true,
            notify_on_failure: true,
            thread_tasks: false,
            include_host_details: true,
            max_failures_shown: 10,
            timeout_secs: 30,
            retry_attempts: 3,
            min_message_interval_ms: 1000,
        }
    }
}

impl SlackCallbackConfig {
    /// Create a new configuration builder.
    pub fn builder() -> SlackCallbackConfigBuilder {
        SlackCallbackConfigBuilder::default()
    }

    /// Load configuration from environment variables.
    pub fn from_env() -> SlackResult<Self> {
        let webhook_url = env::var("RUSTIBLE_SLACK_WEBHOOK_URL")
            .map_err(|_| SlackError::MissingConfig("RUSTIBLE_SLACK_WEBHOOK_URL".to_string()))?;

        Ok(Self {
            webhook_url,
            channel: env::var("RUSTIBLE_SLACK_CHANNEL").ok(),
            username: env::var("RUSTIBLE_SLACK_USERNAME")
                .unwrap_or_else(|_| "Rustible".to_string()),
            icon_emoji: env::var("RUSTIBLE_SLACK_ICON_EMOJI")
                .unwrap_or_else(|_| ":gear:".to_string()),
            notify_on_start: env::var("RUSTIBLE_SLACK_NOTIFY_START")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            notify_on_end: env::var("RUSTIBLE_SLACK_NOTIFY_END")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(true),
            notify_on_failure: env::var("RUSTIBLE_SLACK_NOTIFY_FAILURE")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(true),
            thread_tasks: env::var("RUSTIBLE_SLACK_THREAD_TASKS")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            include_host_details: env::var("RUSTIBLE_SLACK_INCLUDE_HOSTS")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(true),
            max_failures_shown: env::var("RUSTIBLE_SLACK_MAX_FAILURES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            timeout_secs: env::var("RUSTIBLE_SLACK_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            retry_attempts: env::var("RUSTIBLE_SLACK_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            min_message_interval_ms: 1000,
        })
    }

    /// Validate the configuration.
    pub fn validate(&self) -> SlackResult<()> {
        if self.webhook_url.is_empty() {
            return Err(SlackError::MissingConfig("webhook_url".to_string()));
        }
        if !self.webhook_url.starts_with("https://hooks.slack.com/") {
            return Err(SlackError::MissingConfig(
                "webhook_url must be a valid Slack webhook URL".to_string(),
            ));
        }
        Ok(())
    }
}

/// Builder for SlackCallbackConfig.
#[derive(Debug, Default)]
pub struct SlackCallbackConfigBuilder {
    config: SlackCallbackConfig,
}

impl SlackCallbackConfigBuilder {
    /// Set the webhook URL.
    pub fn webhook_url(mut self, url: impl Into<String>) -> Self {
        self.config.webhook_url = url.into();
        self
    }

    /// Set the channel override.
    pub fn channel(mut self, channel: impl Into<String>) -> Self {
        self.config.channel = Some(channel.into());
        self
    }

    /// Set the bot username.
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.config.username = username.into();
        self
    }

    /// Set the bot icon emoji.
    pub fn icon_emoji(mut self, emoji: impl Into<String>) -> Self {
        self.config.icon_emoji = emoji.into();
        self
    }

    /// Set whether to notify on playbook start.
    pub fn notify_on_start(mut self, notify: bool) -> Self {
        self.config.notify_on_start = notify;
        self
    }

    /// Set whether to notify on playbook end.
    pub fn notify_on_end(mut self, notify: bool) -> Self {
        self.config.notify_on_end = notify;
        self
    }

    /// Set whether to notify on task failure.
    pub fn notify_on_failure(mut self, notify: bool) -> Self {
        self.config.notify_on_failure = notify;
        self
    }

    /// Set whether to post task updates in a thread.
    pub fn thread_tasks(mut self, thread: bool) -> Self {
        self.config.thread_tasks = thread;
        self
    }

    /// Set whether to include host details.
    pub fn include_host_details(mut self, include: bool) -> Self {
        self.config.include_host_details = include;
        self
    }

    /// Set the maximum number of failures to show.
    pub fn max_failures_shown(mut self, max: usize) -> Self {
        self.config.max_failures_shown = max;
        self
    }

    /// Set the HTTP timeout.
    pub fn timeout_secs(mut self, timeout: u64) -> Self {
        self.config.timeout_secs = timeout;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> SlackCallbackConfig {
        self.config
    }
}

// ============================================================================
// Slack Message Structures
// ============================================================================

/// Slack message payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    username: String,
    icon_emoji: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<SlackAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_ts: Option<String>,
}

/// Slack message attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackAttachment {
    fallback: String,
    color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pretext: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<SlackField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    footer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ts: Option<i64>,
}

/// Slack attachment field.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackField {
    title: String,
    value: String,
    short: bool,
}

// ============================================================================
// Execution State
// ============================================================================

/// Per-host execution statistics.
#[derive(Debug, Clone, Default)]
struct HostStats {
    ok: u32,
    changed: u32,
    failed: u32,
    skipped: u32,
}

/// Failure detail.
#[derive(Debug, Clone)]
struct FailureDetail {
    host: String,
    task: String,
    message: String,
}

/// Internal execution state.
#[derive(Debug, Default)]
struct ExecutionState {
    /// Playbook start time
    start_time: Option<Instant>,
    /// Current playbook name
    playbook: Option<String>,
    /// Per-host statistics
    host_stats: HashMap<String, HostStats>,
    /// Recorded failures
    failures: Vec<FailureDetail>,
    /// Thread timestamp for threading messages
    thread_ts: Option<String>,
    /// Last message sent time (for rate limiting)
    last_message_time: Option<Instant>,
    /// Target hosts
    hosts: Vec<String>,
}

// ============================================================================
// Slack Callback Implementation
// ============================================================================

/// Slack callback plugin for sending notifications to Slack.
///
/// This callback sends rich, formatted notifications to Slack channels
/// during playbook execution, providing real-time visibility into
/// automation runs.
#[derive(Debug)]
pub struct SlackCallback {
    /// Configuration
    config: SlackCallbackConfig,
    /// HTTP client
    client: Client,
    /// Execution state
    state: Arc<RwLock<ExecutionState>>,
}

impl SlackCallback {
    /// Create a new Slack callback with the given configuration.
    pub fn new(config: SlackCallbackConfig) -> SlackResult<Self> {
        config.validate()?;

        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        Ok(Self {
            config,
            client,
            state: Arc::new(RwLock::new(ExecutionState::default())),
        })
    }

    /// Create a Slack callback from environment variables.
    pub fn from_env() -> SlackResult<Self> {
        let config = SlackCallbackConfig::from_env()?;
        Self::new(config)
    }

    /// Send a message to Slack with retry logic.
    async fn send_message(&self, message: SlackMessage) -> SlackResult<Option<String>> {
        // Rate limiting - calculate sleep time while holding the lock, then sleep after releasing it
        let sleep_time = {
            let state = self.state.read();
            if let Some(last_time) = state.last_message_time {
                let elapsed = last_time.elapsed().as_millis() as u64;
                if elapsed < self.config.min_message_interval_ms {
                    Some(self.config.min_message_interval_ms - elapsed)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(delay) = sleep_time {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }

        let mut last_error = None;

        for attempt in 0..self.config.retry_attempts {
            if attempt > 0 {
                let backoff = Duration::from_millis(100 * 2u64.pow(attempt));
                tokio::time::sleep(backoff).await;
            }

            match self.send_message_once(&message).await {
                Ok(ts) => {
                    // Update last message time
                    let mut state = self.state.write();
                    state.last_message_time = Some(Instant::now());
                    return Ok(ts);
                }
                Err(e) => {
                    warn!("Slack message attempt {} failed: {}", attempt + 1, e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| SlackError::ApiError("Unknown error".to_string())))
    }

    /// Send a single message attempt.
    async fn send_message_once(&self, message: &SlackMessage) -> SlackResult<Option<String>> {
        let response = self
            .client
            .post(&self.config.webhook_url)
            .json(message)
            .send()
            .await?;

        let status = response.status();

        if status.is_success() {
            debug!("Slack message sent successfully");
            Ok(None) // Webhooks don't return thread_ts
        } else if status.as_u16() == 429 {
            // Rate limited
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(60);
            Err(SlackError::RateLimited(retry_after))
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(SlackError::ApiError(format!("{}: {}", status, body)))
        }
    }

    /// Create a playbook start message.
    fn create_start_message(&self, playbook: &str, hosts: &[String]) -> SlackMessage {
        let host_list = if hosts.len() > 5 {
            format!("{} and {} more", hosts[..5].join(", "), hosts.len() - 5)
        } else {
            hosts.join(", ")
        };

        let attachment = SlackAttachment {
            fallback: format!("Rustible playbook started: {}", playbook),
            color: "#2196F3".to_string(), // Blue
            pretext: None,
            title: Some(format!(":rocket: Playbook Started: {}", playbook)),
            text: None,
            fields: vec![
                SlackField {
                    title: "Hosts".to_string(),
                    value: host_list,
                    short: false,
                },
                SlackField {
                    title: "Started".to_string(),
                    value: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    short: true,
                },
            ],
            footer: Some("Rustible Automation".to_string()),
            ts: Some(Utc::now().timestamp()),
        };

        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            text: None,
            attachments: vec![attachment],
            thread_ts: None,
        }
    }

    /// Create a playbook end message.
    fn create_end_message(
        &self,
        playbook: &str,
        success: bool,
        duration_secs: f64,
        host_stats: &HashMap<String, HostStats>,
        failures: &[FailureDetail],
    ) -> SlackMessage {
        let (icon, color, status_text) = if success {
            (":white_check_mark:", "#36a64f", "SUCCESS")
        } else {
            (":x:", "#dc3545", "FAILED")
        };

        let mut fields = vec![
            SlackField {
                title: "Status".to_string(),
                value: status_text.to_string(),
                short: true,
            },
            SlackField {
                title: "Duration".to_string(),
                value: format!("{:.1}s", duration_secs),
                short: true,
            },
        ];

        // Add host summary
        if self.config.include_host_details && !host_stats.is_empty() {
            let mut host_lines = Vec::new();
            for (host, stats) in host_stats {
                host_lines.push(format!(
                    "`{}`: ok={} changed={} failed={} skipped={}",
                    host, stats.ok, stats.changed, stats.failed, stats.skipped
                ));
            }
            fields.push(SlackField {
                title: "Host Summary".to_string(),
                value: host_lines.join("\n"),
                short: false,
            });
        }

        // Add failure details
        if !failures.is_empty() {
            let failure_count = failures.len();
            let shown = failures.iter().take(self.config.max_failures_shown);
            let mut failure_lines: Vec<String> = shown
                .map(|f| format!("`{}` on `{}`: {}", f.task, f.host, f.message))
                .collect();

            if failure_count > self.config.max_failures_shown {
                failure_lines.push(format!(
                    "... and {} more failures",
                    failure_count - self.config.max_failures_shown
                ));
            }

            fields.push(SlackField {
                title: format!(":warning: Failures ({})", failure_count),
                value: failure_lines.join("\n"),
                short: false,
            });
        }

        let attachment = SlackAttachment {
            fallback: format!(
                "Rustible playbook {}: {} ({duration_secs:.1}s)",
                status_text.to_lowercase(),
                playbook
            ),
            color: color.to_string(),
            pretext: None,
            title: Some(format!("{} Playbook Completed: {}", icon, playbook)),
            text: None,
            fields,
            footer: Some("Rustible Automation".to_string()),
            ts: Some(Utc::now().timestamp()),
        };

        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            text: None,
            attachments: vec![attachment],
            thread_ts: self.state.read().thread_ts.clone(),
        }
    }

    /// Create a failure notification message.
    fn create_failure_message(&self, failure: &FailureDetail, playbook: &str) -> SlackMessage {
        let attachment = SlackAttachment {
            fallback: format!(
                "Task failed: {} on {} - {}",
                failure.task, failure.host, failure.message
            ),
            color: "#dc3545".to_string(), // Red
            pretext: None,
            title: Some(format!(":x: Task Failed: {}", failure.task)),
            text: None,
            fields: vec![
                SlackField {
                    title: "Host".to_string(),
                    value: format!("`{}`", failure.host),
                    short: true,
                },
                SlackField {
                    title: "Playbook".to_string(),
                    value: playbook.to_string(),
                    short: true,
                },
                SlackField {
                    title: "Error".to_string(),
                    value: format!("```{}```", failure.message),
                    short: false,
                },
            ],
            footer: Some("Rustible Automation".to_string()),
            ts: Some(Utc::now().timestamp()),
        };

        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            text: None,
            attachments: vec![attachment],
            thread_ts: self.state.read().thread_ts.clone(),
        }
    }
}

impl Clone for SlackCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            client: self.client.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

#[async_trait]
impl ExecutionCallback for SlackCallback {
    async fn on_playbook_start(&self, name: &str) {
        {
            let mut state = self.state.write();
            state.start_time = Some(Instant::now());
            state.playbook = Some(name.to_string());
            state.host_stats.clear();
            state.failures.clear();
            state.thread_ts = None;
        }

        if self.config.notify_on_start {
            let hosts = self.state.read().hosts.clone();
            let message = self.create_start_message(name, &hosts);
            if let Err(e) = self.send_message(message).await {
                error!("Failed to send Slack start notification: {}", e);
            } else {
                info!("Slack start notification sent for playbook: {}", name);
            }
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        if !self.config.notify_on_end {
            return;
        }

        let (duration_secs, host_stats, failures) = {
            let state = self.state.read();
            let duration = state
                .start_time
                .map(|t| t.elapsed().as_secs_f64())
                .unwrap_or(0.0);
            (duration, state.host_stats.clone(), state.failures.clone())
        };

        let message = self.create_end_message(name, success, duration_secs, &host_stats, &failures);

        if let Err(e) = self.send_message(message).await {
            error!("Failed to send Slack end notification: {}", e);
        } else {
            info!(
                "Slack end notification sent for playbook: {} ({})",
                name,
                if success { "success" } else { "failed" }
            );
        }
    }

    async fn on_play_start(&self, _name: &str, hosts: &[String]) {
        let mut state = self.state.write();
        state.hosts = hosts.to_vec();
        for host in hosts {
            state.host_stats.entry(host.clone()).or_default();
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let (playbook, failure_to_send) = {
            let mut state = self.state.write();
            let playbook = state.playbook.clone();

            // Update statistics
            let host_stats = state.host_stats.entry(result.host.clone()).or_default();
            let failure = if result.result.skipped {
                host_stats.skipped += 1;
                None
            } else if !result.result.success {
                host_stats.failed += 1;
                let failure = FailureDetail {
                    host: result.host.clone(),
                    task: result.task_name.clone(),
                    message: result.result.message.clone(),
                };
                state.failures.push(failure.clone());
                Some(failure)
            } else if result.result.changed {
                host_stats.changed += 1;
                None
            } else {
                host_stats.ok += 1;
                None
            };

            (playbook, failure)
        };

        // Send immediate failure notification if configured
        if let Some(failure) = failure_to_send {
            if self.config.notify_on_failure {
                let message =
                    self.create_failure_message(&failure, playbook.as_deref().unwrap_or("unknown"));
                if let Err(e) = self.send_message(message).await {
                    error!("Failed to send Slack failure notification: {}", e);
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = SlackCallbackConfig::builder()
            .webhook_url("https://hooks.slack.com/services/test")
            .channel("#test")
            .username("TestBot")
            .icon_emoji(":robot:")
            .notify_on_start(true)
            .notify_on_end(true)
            .notify_on_failure(false)
            .thread_tasks(true)
            .include_host_details(false)
            .max_failures_shown(5)
            .timeout_secs(60)
            .build();

        assert_eq!(config.webhook_url, "https://hooks.slack.com/services/test");
        assert_eq!(config.channel, Some("#test".to_string()));
        assert_eq!(config.username, "TestBot");
        assert_eq!(config.icon_emoji, ":robot:");
        assert!(config.notify_on_start);
        assert!(config.notify_on_end);
        assert!(!config.notify_on_failure);
        assert!(config.thread_tasks);
        assert!(!config.include_host_details);
        assert_eq!(config.max_failures_shown, 5);
        assert_eq!(config.timeout_secs, 60);
    }

    #[test]
    fn test_config_validation() {
        let config = SlackCallbackConfig::default();
        assert!(config.validate().is_err());

        let mut config = SlackCallbackConfig::default();
        config.webhook_url = "https://example.com".to_string();
        assert!(config.validate().is_err());

        let mut config = SlackCallbackConfig::default();
        config.webhook_url = "https://hooks.slack.com/services/test".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_default_config() {
        let config = SlackCallbackConfig::default();
        assert!(config.webhook_url.is_empty());
        assert!(config.channel.is_none());
        assert_eq!(config.username, "Rustible");
        assert_eq!(config.icon_emoji, ":gear:");
        assert!(!config.notify_on_start);
        assert!(config.notify_on_end);
        assert!(config.notify_on_failure);
        assert!(!config.thread_tasks);
        assert!(config.include_host_details);
        assert_eq!(config.max_failures_shown, 10);
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.retry_attempts, 3);
    }

    #[test]
    fn test_slack_message_serialization() {
        let message = SlackMessage {
            channel: Some("#test".to_string()),
            username: "TestBot".to_string(),
            icon_emoji: ":gear:".to_string(),
            text: Some("Test message".to_string()),
            attachments: vec![],
            thread_ts: None,
        };

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("\"channel\":\"#test\""));
        assert!(json.contains("\"username\":\"TestBot\""));
        assert!(json.contains("\"text\":\"Test message\""));
    }

    #[test]
    fn test_slack_attachment_serialization() {
        let attachment = SlackAttachment {
            fallback: "Test fallback".to_string(),
            color: "#ff0000".to_string(),
            pretext: None,
            title: Some("Test Title".to_string()),
            text: Some("Test text".to_string()),
            fields: vec![SlackField {
                title: "Field".to_string(),
                value: "Value".to_string(),
                short: true,
            }],
            footer: Some("Test Footer".to_string()),
            ts: Some(1234567890),
        };

        let json = serde_json::to_string(&attachment).unwrap();
        assert!(json.contains("\"fallback\":\"Test fallback\""));
        assert!(json.contains("\"color\":\"#ff0000\""));
        assert!(json.contains("\"title\":\"Test Title\""));
    }

    #[test]
    fn test_host_stats_default() {
        let stats = HostStats::default();
        assert_eq!(stats.ok, 0);
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn test_slack_error_display() {
        let err = SlackError::MissingConfig("test".to_string());
        assert!(err.to_string().contains("Missing required configuration"));

        let err = SlackError::ApiError("Bad request".to_string());
        assert!(err.to_string().contains("Slack API error"));

        let err = SlackError::RateLimited(60);
        assert!(err.to_string().contains("Rate limited"));
        assert!(err.to_string().contains("60"));
    }
}
