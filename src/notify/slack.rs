//! Slack notification backend.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use tracing::{debug, error, info};

use super::config::SlackConfig;
use super::error::{NotificationError, NotificationResult};
use super::{FailureInfo, HostStats, NotificationEvent, Notifier};

/// Slack notification backend.
#[derive(Debug)]
pub struct SlackNotifier {
    config: SlackConfig,
    client: Client,
}

impl SlackNotifier {
    /// Creates a new Slack notifier with the given configuration.
    pub fn new(config: SlackConfig, timeout: Duration) -> NotificationResult<Self> {
        config.validate()?;

        let client = Client::builder().timeout(timeout).build().map_err(|e| {
            NotificationError::internal(format!("Failed to create HTTP client: {}", e))
        })?;

        Ok(Self { config, client })
    }

    /// Creates a Slack notifier from environment variables.
    pub fn from_env(timeout: Duration) -> Option<Self> {
        let config = SlackConfig::from_env()?;
        Self::new(config, timeout).ok()
    }

    /// Formats a notification event as a Slack message.
    fn format_message(&self, event: &NotificationEvent) -> SlackMessage {
        match event {
            NotificationEvent::PlaybookStart {
                playbook, hosts, ..
            } => self.format_playbook_start(playbook, hosts),
            NotificationEvent::PlaybookComplete {
                playbook,
                success,
                duration_secs,
                host_stats,
                failures,
                ..
            } => self.format_playbook_complete(
                playbook,
                *success,
                *duration_secs,
                host_stats,
                failures.as_deref(),
            ),
            NotificationEvent::TaskFailed {
                playbook,
                task,
                host,
                error,
                ..
            } => self.format_task_failed(playbook, task, host, error),
            NotificationEvent::HostUnreachable {
                playbook,
                host,
                error,
                ..
            } => self.format_host_unreachable(playbook, host, error),
            NotificationEvent::Custom { name, data, .. } => self.format_custom(name, data),
        }
    }

    fn format_playbook_start(&self, playbook: &str, hosts: &[String]) -> SlackMessage {
        let text = format!(
            "Starting playbook `{}` on {} host(s)",
            playbook,
            hosts.len()
        );

        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            attachments: vec![SlackAttachment {
                fallback: text.clone(),
                color: "#2196F3".to_string(), // Blue
                title: format!("Playbook Started: {}", playbook),
                text: Some(text),
                fields: vec![SlackField {
                    title: "Hosts".to_string(),
                    value: if hosts.len() <= 5 {
                        hosts.join(", ")
                    } else {
                        format!("{} (and {} more)", hosts[..5].join(", "), hosts.len() - 5)
                    },
                    short: false,
                }],
                footer: Some("Rustible".to_string()),
                ts: Some(chrono::Utc::now().timestamp()),
            }],
        }
    }

    fn format_playbook_complete(
        &self,
        playbook: &str,
        success: bool,
        duration_secs: f64,
        host_stats: &std::collections::HashMap<String, HostStats>,
        failures: Option<&[FailureInfo]>,
    ) -> SlackMessage {
        let status = if success { "SUCCESS" } else { "FAILED" };
        let color = if success { "#36a64f" } else { "#dc3545" };

        let mut fields = vec![
            SlackField {
                title: "Status".to_string(),
                value: status.to_string(),
                short: true,
            },
            SlackField {
                title: "Duration".to_string(),
                value: format_duration(duration_secs),
                short: true,
            },
        ];

        // Add host summary if enabled
        if self.config.include_host_stats && !host_stats.is_empty() {
            let mut host_lines = Vec::new();
            let mut hosts: Vec<_> = host_stats.keys().collect();
            hosts.sort();

            for host in hosts.iter().take(10) {
                if let Some(stats) = host_stats.get(*host) {
                    host_lines.push(format!(
                        "`{}`: ok={} changed={} failed={}",
                        host, stats.ok, stats.changed, stats.failed
                    ));
                }
            }

            if host_stats.len() > 10 {
                host_lines.push(format!("... and {} more hosts", host_stats.len() - 10));
            }

            fields.push(SlackField {
                title: "Host Summary".to_string(),
                value: host_lines.join("\n"),
                short: false,
            });
        }

        // Add failure details if enabled
        if self.config.include_failures {
            if let Some(failures) = failures {
                if !failures.is_empty() {
                    let failure_lines: Vec<String> = failures
                        .iter()
                        .take(5)
                        .map(|f| format!("- `{}` on `{}`: {}", f.task, f.host, f.message))
                        .collect();

                    let mut value = failure_lines.join("\n");
                    if failures.len() > 5 {
                        value.push_str(&format!("\n... and {} more failures", failures.len() - 5));
                    }

                    fields.push(SlackField {
                        title: "Failures".to_string(),
                        value,
                        short: false,
                    });
                }
            }
        }

        // Add mention if configured and there's a failure
        let mut text = None;
        if !success {
            if let Some(ref mention) = self.config.mention_on_failure {
                text = Some(format!("{} Playbook failed!", mention));
            }
        }

        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            attachments: vec![SlackAttachment {
                fallback: format!("Rustible: {} - {}", playbook, status),
                color: color.to_string(),
                title: format!("Playbook: {}", playbook),
                text,
                fields,
                footer: Some("Rustible".to_string()),
                ts: Some(chrono::Utc::now().timestamp()),
            }],
        }
    }

    fn format_task_failed(
        &self,
        playbook: &str,
        task: &str,
        host: &str,
        error: &str,
    ) -> SlackMessage {
        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            attachments: vec![SlackAttachment {
                fallback: format!("Task '{}' failed on {}", task, host),
                color: "#dc3545".to_string(), // Red
                title: format!("Task Failed: {}", task),
                text: Some(format!("```{}```", truncate(error, 500))),
                fields: vec![
                    SlackField {
                        title: "Playbook".to_string(),
                        value: playbook.to_string(),
                        short: true,
                    },
                    SlackField {
                        title: "Host".to_string(),
                        value: host.to_string(),
                        short: true,
                    },
                ],
                footer: Some("Rustible".to_string()),
                ts: Some(chrono::Utc::now().timestamp()),
            }],
        }
    }

    fn format_host_unreachable(&self, playbook: &str, host: &str, error: &str) -> SlackMessage {
        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            attachments: vec![SlackAttachment {
                fallback: format!("Host '{}' is unreachable", host),
                color: "#ff9800".to_string(), // Orange
                title: format!("Host Unreachable: {}", host),
                text: Some(format!("```{}```", truncate(error, 300))),
                fields: vec![SlackField {
                    title: "Playbook".to_string(),
                    value: playbook.to_string(),
                    short: true,
                }],
                footer: Some("Rustible".to_string()),
                ts: Some(chrono::Utc::now().timestamp()),
            }],
        }
    }

    fn format_custom(&self, name: &str, data: &serde_json::Value) -> SlackMessage {
        let text = serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string());

        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            attachments: vec![SlackAttachment {
                fallback: format!("Custom event: {}", name),
                color: "#9c27b0".to_string(), // Purple
                title: format!("Event: {}", name),
                text: Some(format!("```{}```", truncate(&text, 1000))),
                fields: vec![],
                footer: Some("Rustible".to_string()),
                ts: Some(chrono::Utc::now().timestamp()),
            }],
        }
    }
}

#[async_trait]
impl Notifier for SlackNotifier {
    fn name(&self) -> &str {
        "Slack"
    }

    fn is_configured(&self) -> bool {
        !self.config.webhook_url.is_empty()
    }

    async fn send(&self, event: &NotificationEvent) -> NotificationResult<()> {
        if !self.is_configured() {
            return Err(NotificationError::not_configured("Slack"));
        }

        let message = self.format_message(event);

        debug!(
            "Sending Slack notification for event: {}",
            event.event_type()
        );

        let response = self
            .client
            .post(&self.config.webhook_url)
            .json(&message)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Slack API returned error: {} - {}", status, body);
            return Err(NotificationError::http(
                Some(status.as_u16()),
                format!("Slack API error: {}", body),
            ));
        }

        info!(
            "Slack notification sent successfully for event: {}",
            event.event_type()
        );
        Ok(())
    }
}

/// Slack message structure.
#[derive(Debug, Serialize)]
struct SlackMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    username: String,
    icon_emoji: String,
    attachments: Vec<SlackAttachment>,
}

/// Slack message attachment.
#[derive(Debug, Serialize)]
struct SlackAttachment {
    fallback: String,
    color: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    fields: Vec<SlackField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    footer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ts: Option<i64>,
}

/// Slack attachment field.
#[derive(Debug, Serialize)]
struct SlackField {
    title: String,
    value: String,
    short: bool,
}

/// Formats a duration in seconds to a human-readable string.
fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.1}s", secs)
    } else if secs < 3600.0 {
        let mins = (secs / 60.0).floor();
        let remaining = secs % 60.0;
        format!("{:.0}m {:.1}s", mins, remaining)
    } else {
        let hours = (secs / 3600.0).floor();
        let mins = ((secs % 3600.0) / 60.0).floor();
        format!("{:.0}h {:.0}m", hours, mins)
    }
}

/// Truncates a string to the specified length.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30.5), "30.5s");
        assert_eq!(format_duration(90.0), "1m 30.0s");
        assert_eq!(format_duration(3700.0), "1h 1m");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a longer string", 10), "this is...");
    }

    #[test]
    fn test_format_playbook_start() {
        let config = SlackConfig::new("https://hooks.slack.com/test");
        let notifier = SlackNotifier {
            config,
            client: Client::new(),
        };

        let event = NotificationEvent::playbook_start(
            "deploy.yml",
            vec!["host1".to_string(), "host2".to_string()],
        );
        let message = notifier.format_message(&event);

        assert_eq!(message.attachments.len(), 1);
        assert!(message.attachments[0].title.contains("deploy.yml"));
        assert_eq!(message.attachments[0].color, "#2196F3");
    }

    #[test]
    fn test_format_playbook_complete_success() {
        let config = SlackConfig::new("https://hooks.slack.com/test");
        let notifier = SlackNotifier {
            config,
            client: Client::new(),
        };

        let mut host_stats = HashMap::new();
        host_stats.insert("host1".to_string(), HostStats::new(5, 2, 0, 1, 0));

        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            true,
            std::time::Duration::from_secs(45),
            host_stats,
            None,
        );
        let message = notifier.format_message(&event);

        assert_eq!(message.attachments[0].color, "#36a64f"); // Green
    }

    #[test]
    fn test_format_playbook_complete_failure() {
        let config = SlackConfig::new("https://hooks.slack.com/test").with_channel("#alerts");
        let notifier = SlackNotifier {
            config,
            client: Client::new(),
        };

        let failures = vec![FailureInfo::new("host1", "task1", "Connection timeout")];
        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            false,
            std::time::Duration::from_secs(10),
            HashMap::new(),
            Some(failures),
        );
        let message = notifier.format_message(&event);

        assert_eq!(message.attachments[0].color, "#dc3545"); // Red
        assert_eq!(message.channel, Some("#alerts".to_string()));
    }

    #[test]
    fn test_notifier_is_configured() {
        let config = SlackConfig::new("https://hooks.slack.com/test");
        let notifier = SlackNotifier {
            config,
            client: Client::new(),
        };
        assert!(notifier.is_configured());

        let config = SlackConfig::default();
        let notifier = SlackNotifier {
            config,
            client: Client::new(),
        };
        assert!(!notifier.is_configured());
    }
}
