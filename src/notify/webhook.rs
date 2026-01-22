//! Generic webhook notification backend.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method};
use tracing::{debug, error, info};

use super::config::WebhookConfig;
use super::error::{NotificationError, NotificationResult};
use super::{NotificationEvent, Notifier};

/// Webhook notification backend.
#[derive(Debug)]
pub struct WebhookNotifier {
    config: WebhookConfig,
    client: Client,
}

impl WebhookNotifier {
    /// Creates a new webhook notifier with the given configuration.
    pub fn new(config: WebhookConfig, timeout: Duration) -> NotificationResult<Self> {
        config.validate()?;

        let client = Client::builder()
            .timeout(timeout)
            .danger_accept_invalid_certs(!config.verify_ssl)
            .build()
            .map_err(|e| NotificationError::internal(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Creates a webhook notifier from environment variables.
    pub fn from_env(timeout: Duration) -> Option<Self> {
        let config = WebhookConfig::from_env()?;
        Self::new(config, timeout).ok()
    }

    /// Parses the HTTP method from configuration.
    fn parse_method(&self) -> Method {
        match self.config.method.to_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "PATCH" => Method::PATCH,
            "DELETE" => Method::DELETE,
            _ => Method::POST,
        }
    }

    /// Builds the payload for the webhook.
    fn build_payload(&self, event: &NotificationEvent) -> NotificationResult<serde_json::Value> {
        // If a custom template is provided, we'd render it here
        // For now, we just serialize the event as-is
        if self.config.include_full_event {
            Ok(serde_json::to_value(event)?)
        } else {
            // Build a minimal payload
            Ok(serde_json::json!({
                "type": event.event_type(),
                "playbook": event.playbook(),
                "is_failure": event.is_failure(),
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }))
        }
    }
}

#[async_trait]
impl Notifier for WebhookNotifier {
    fn name(&self) -> &str {
        "Webhook"
    }

    fn is_configured(&self) -> bool {
        !self.config.url.is_empty()
    }

    async fn send(&self, event: &NotificationEvent) -> NotificationResult<()> {
        if !self.is_configured() {
            return Err(NotificationError::not_configured("Webhook"));
        }

        let method = self.parse_method();
        let payload = self.build_payload(event)?;

        debug!(
            "Sending webhook notification to {} for event: {}",
            self.config.url,
            event.event_type()
        );

        let mut request = self.client.request(method, &self.config.url);

        // Add custom headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        // Add authorization
        if let Some(ref token) = self.config.auth_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        // Add basic auth
        if let Some((ref user, ref pass)) = self.config.basic_auth {
            request = request.basic_auth(user, Some(pass));
        }

        // Set content type and body
        request = request
            .header("Content-Type", "application/json")
            .header("User-Agent", "Rustible-Webhook/1.0")
            .json(&payload);

        let response = request.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(
                "Webhook returned error: {} - {}",
                status,
                truncate(&body, 200)
            );
            return Err(NotificationError::http(
                Some(status.as_u16()),
                format!("Webhook error: {}", truncate(&body, 200)),
            ));
        }

        info!(
            "Webhook notification sent successfully to {}",
            self.config.url
        );
        Ok(())
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

    #[test]
    fn test_parse_method() {
        let config = WebhookConfig::new("https://example.com/hook").with_method("POST");
        let notifier = WebhookNotifier {
            config,
            client: Client::new(),
        };
        assert_eq!(notifier.parse_method(), Method::POST);

        let config = WebhookConfig::new("https://example.com/hook").with_method("get");
        let notifier = WebhookNotifier {
            config,
            client: Client::new(),
        };
        assert_eq!(notifier.parse_method(), Method::GET);

        let config = WebhookConfig::new("https://example.com/hook").with_method("PUT");
        let notifier = WebhookNotifier {
            config,
            client: Client::new(),
        };
        assert_eq!(notifier.parse_method(), Method::PUT);
    }

    #[test]
    fn test_build_payload_full_event() {
        let config = WebhookConfig::new("https://example.com/hook");
        let notifier = WebhookNotifier {
            config,
            client: Client::new(),
        };

        let event = NotificationEvent::task_failed("deploy.yml", "Copy files", "host1", "Timeout");

        let payload = notifier.build_payload(&event).unwrap();

        assert!(payload.is_object());
        assert_eq!(payload["type"], "task_failed");
    }

    #[test]
    fn test_build_payload_minimal() {
        let mut config = WebhookConfig::new("https://example.com/hook");
        config.include_full_event = false;
        let notifier = WebhookNotifier {
            config,
            client: Client::new(),
        };

        let event = NotificationEvent::task_failed("deploy.yml", "Copy files", "host1", "Timeout");

        let payload = notifier.build_payload(&event).unwrap();

        assert!(payload.is_object());
        assert_eq!(payload["type"], "task_failed");
        assert_eq!(payload["playbook"], "deploy.yml");
        assert_eq!(payload["is_failure"], true);
    }

    #[test]
    fn test_notifier_is_configured() {
        let config = WebhookConfig::new("https://example.com/hook");
        let notifier = WebhookNotifier {
            config,
            client: Client::new(),
        };
        assert!(notifier.is_configured());

        let config = WebhookConfig::default();
        let notifier = WebhookNotifier {
            config,
            client: Client::new(),
        };
        assert!(!notifier.is_configured());
    }

    #[test]
    fn test_webhook_with_headers() {
        let config = WebhookConfig::new("https://example.com/hook")
            .with_header("X-Custom-Header", "custom-value")
            .with_header("X-API-Key", "secret");

        assert_eq!(config.headers.len(), 2);
        assert_eq!(config.headers.get("X-Custom-Header"), Some(&"custom-value".to_string()));
    }

    #[test]
    fn test_webhook_with_auth() {
        let config = WebhookConfig::new("https://example.com/hook")
            .with_auth_token("my-bearer-token");

        assert_eq!(config.auth_token, Some("my-bearer-token".to_string()));

        let config = WebhookConfig::new("https://example.com/hook")
            .with_basic_auth("user", "pass");

        assert_eq!(config.basic_auth, Some(("user".to_string(), "pass".to_string())));
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is longer", 10), "this is...");
    }
}
