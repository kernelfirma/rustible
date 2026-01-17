//! Configuration for notification backends.

use std::collections::HashMap;
use std::env;
use std::time::Duration;

use super::error::{NotificationError, NotificationResult};

/// Configuration for the notification system.
#[derive(Debug, Clone, Default)]
pub struct NotificationConfig {
    /// Slack configuration
    pub slack: Option<SlackConfig>,
    /// Email configuration
    pub email: Option<EmailConfig>,
    /// Webhook configuration
    pub webhook: Option<WebhookConfig>,
    /// Send notification on success
    pub notify_on_success: bool,
    /// Send notification on failure
    pub notify_on_failure: bool,
    /// Custom template path
    pub template_path: Option<String>,
    /// Request timeout
    pub timeout: Duration,
    /// Number of retries for failed notifications
    pub retries: u32,
    /// Delay between retries
    pub retry_delay: Duration,
}

impl NotificationConfig {
    /// Creates a new configuration builder.
    pub fn builder() -> NotificationConfigBuilder {
        NotificationConfigBuilder::default()
    }

    /// Loads configuration from environment variables.
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

        let template_path = env::var("RUSTIBLE_NOTIFY_TEMPLATE").ok();

        let timeout = env::var("RUSTIBLE_NOTIFY_TIMEOUT")
            .ok()
            .and_then(|t| t.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(30));

        let retries = env::var("RUSTIBLE_NOTIFY_RETRIES")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(3);

        let retry_delay = env::var("RUSTIBLE_NOTIFY_RETRY_DELAY")
            .ok()
            .and_then(|d| d.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(1));

        Self {
            slack,
            email,
            webhook,
            notify_on_success,
            notify_on_failure,
            template_path,
            timeout,
            retries,
            retry_delay,
        }
    }

    /// Returns true if any backend is configured.
    pub fn has_backends(&self) -> bool {
        self.slack.is_some() || self.email.is_some() || self.webhook.is_some()
    }

    /// Returns a list of configured backend names.
    pub fn configured_backends(&self) -> Vec<&str> {
        let mut backends = Vec::new();
        if self.slack.is_some() {
            backends.push("Slack");
        }
        if self.email.is_some() {
            backends.push("Email");
        }
        if self.webhook.is_some() {
            backends.push("Webhook");
        }
        backends
    }

    /// Validates the configuration.
    pub fn validate(&self) -> NotificationResult<()> {
        if let Some(ref slack) = self.slack {
            slack.validate()?;
        }
        if let Some(ref email) = self.email {
            email.validate()?;
        }
        if let Some(ref webhook) = self.webhook {
            webhook.validate()?;
        }
        Ok(())
    }
}

/// Builder for [`NotificationConfig`].
#[derive(Debug, Clone, Default)]
pub struct NotificationConfigBuilder {
    config: NotificationConfig,
}

impl NotificationConfigBuilder {
    /// Sets the Slack webhook URL.
    pub fn slack_webhook(mut self, url: impl Into<String>) -> Self {
        let slack = self.config.slack.get_or_insert_with(SlackConfig::default);
        slack.webhook_url = url.into();
        self
    }

    /// Sets the Slack channel.
    pub fn slack_channel(mut self, channel: impl Into<String>) -> Self {
        let slack = self.config.slack.get_or_insert_with(SlackConfig::default);
        slack.channel = Some(channel.into());
        self
    }

    /// Sets the Slack username.
    pub fn slack_username(mut self, username: impl Into<String>) -> Self {
        let slack = self.config.slack.get_or_insert_with(SlackConfig::default);
        slack.username = username.into();
        self
    }

    /// Sets the full Slack configuration.
    pub fn slack(mut self, config: SlackConfig) -> Self {
        self.config.slack = Some(config);
        self
    }

    /// Sets the email configuration.
    pub fn email(mut self, config: EmailConfig) -> Self {
        self.config.email = Some(config);
        self
    }

    /// Sets the webhook configuration.
    pub fn webhook(mut self, config: WebhookConfig) -> Self {
        self.config.webhook = Some(config);
        self
    }

    /// Sets the webhook URL (convenience method).
    pub fn webhook_url(mut self, url: impl Into<String>) -> Self {
        let webhook = self.config.webhook.get_or_insert_with(WebhookConfig::default);
        webhook.url = url.into();
        self
    }

    /// Sets whether to notify on success.
    pub fn notify_on_success(mut self, enabled: bool) -> Self {
        self.config.notify_on_success = enabled;
        self
    }

    /// Sets whether to notify on failure.
    pub fn notify_on_failure(mut self, enabled: bool) -> Self {
        self.config.notify_on_failure = enabled;
        self
    }

    /// Sets the template path.
    pub fn template_path(mut self, path: impl Into<String>) -> Self {
        self.config.template_path = Some(path.into());
        self
    }

    /// Sets the request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Sets the number of retries.
    pub fn retries(mut self, retries: u32) -> Self {
        self.config.retries = retries;
        self
    }

    /// Sets the retry delay.
    pub fn retry_delay(mut self, delay: Duration) -> Self {
        self.config.retry_delay = delay;
        self
    }

    /// Builds the configuration.
    pub fn build(self) -> NotificationResult<NotificationConfig> {
        self.config.validate()?;
        Ok(self.config)
    }

    /// Builds the configuration without validation.
    pub fn build_unchecked(self) -> NotificationConfig {
        self.config
    }
}

/// Slack notification configuration.
#[derive(Debug, Clone)]
pub struct SlackConfig {
    /// Incoming webhook URL
    pub webhook_url: String,
    /// Optional channel override
    pub channel: Option<String>,
    /// Bot username
    pub username: String,
    /// Icon emoji
    pub icon_emoji: String,
    /// Mention users on failure
    pub mention_on_failure: Option<String>,
    /// Include detailed host stats
    pub include_host_stats: bool,
    /// Include failure details
    pub include_failures: bool,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            webhook_url: String::new(),
            channel: None,
            username: "Rustible".to_string(),
            icon_emoji: ":gear:".to_string(),
            mention_on_failure: None,
            include_host_stats: true,
            include_failures: true,
        }
    }
}

impl SlackConfig {
    /// Creates a new Slack configuration with the given webhook URL.
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
            ..Default::default()
        }
    }

    /// Loads configuration from environment variables.
    pub fn from_env() -> Option<Self> {
        let webhook_url = env::var("RUSTIBLE_SLACK_WEBHOOK_URL").ok()?;

        Some(Self {
            webhook_url,
            channel: env::var("RUSTIBLE_SLACK_CHANNEL").ok(),
            username: env::var("RUSTIBLE_SLACK_USERNAME").unwrap_or_else(|_| "Rustible".to_string()),
            icon_emoji: env::var("RUSTIBLE_SLACK_ICON_EMOJI")
                .unwrap_or_else(|_| ":gear:".to_string()),
            mention_on_failure: env::var("RUSTIBLE_SLACK_MENTION_ON_FAILURE").ok(),
            include_host_stats: env::var("RUSTIBLE_SLACK_INCLUDE_HOST_STATS")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(true),
            include_failures: env::var("RUSTIBLE_SLACK_INCLUDE_FAILURES")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(true),
        })
    }

    /// Validates the configuration.
    pub fn validate(&self) -> NotificationResult<()> {
        if self.webhook_url.is_empty() {
            return Err(NotificationError::config("Slack webhook URL is required"));
        }
        if !self.webhook_url.starts_with("https://") {
            return Err(NotificationError::config(
                "Slack webhook URL must use HTTPS",
            ));
        }
        Ok(())
    }

    /// Sets the channel.
    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
        self
    }

    /// Sets the username.
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = username.into();
        self
    }

    /// Sets the icon emoji.
    pub fn with_icon_emoji(mut self, emoji: impl Into<String>) -> Self {
        self.icon_emoji = emoji.into();
        self
    }
}

/// Email notification configuration.
#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// SMTP server hostname
    pub host: String,
    /// SMTP server port
    pub port: u16,
    /// SMTP username
    pub username: Option<String>,
    /// SMTP password
    pub password: Option<String>,
    /// Sender email address
    pub from: String,
    /// Recipient email addresses
    pub to: Vec<String>,
    /// CC recipients
    pub cc: Vec<String>,
    /// BCC recipients
    pub bcc: Vec<String>,
    /// Use TLS
    pub use_tls: bool,
    /// Use STARTTLS
    pub use_starttls: bool,
    /// Subject prefix
    pub subject_prefix: String,
    /// Include HTML content
    pub html_enabled: bool,
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 587,
            username: None,
            password: None,
            from: String::new(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            use_tls: false,
            use_starttls: true,
            subject_prefix: "[Rustible]".to_string(),
            html_enabled: false,
        }
    }
}

impl EmailConfig {
    /// Creates a new email configuration.
    pub fn new(host: impl Into<String>, from: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            from: from.into(),
            ..Default::default()
        }
    }

    /// Loads configuration from environment variables.
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
            .map(|v| v.eq_ignore_ascii_case("implicit") || v.eq_ignore_ascii_case("tls"))
            .unwrap_or(false);

        let use_starttls = env::var("RUSTIBLE_SMTP_TLS")
            .map(|v| v.eq_ignore_ascii_case("starttls") || v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(!use_tls);

        let cc: Vec<String> = env::var("RUSTIBLE_SMTP_CC")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|e| e.trim().to_string())
                    .filter(|e| !e.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        Some(Self {
            host,
            port,
            username: env::var("RUSTIBLE_SMTP_USER").ok(),
            password: env::var("RUSTIBLE_SMTP_PASSWORD").ok(),
            from,
            to,
            cc,
            bcc: Vec::new(),
            use_tls,
            use_starttls,
            subject_prefix: env::var("RUSTIBLE_MAIL_SUBJECT_PREFIX")
                .unwrap_or_else(|_| "[Rustible]".to_string()),
            html_enabled: env::var("RUSTIBLE_MAIL_HTML")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
        })
    }

    /// Validates the configuration.
    pub fn validate(&self) -> NotificationResult<()> {
        if self.host.is_empty() {
            return Err(NotificationError::config("SMTP host is required"));
        }
        if self.from.is_empty() {
            return Err(NotificationError::config("From address is required"));
        }
        if !self.from.contains('@') {
            return Err(NotificationError::config(format!(
                "Invalid from address: {}",
                self.from
            )));
        }
        if self.to.is_empty() {
            return Err(NotificationError::config(
                "At least one recipient is required",
            ));
        }
        for addr in &self.to {
            if !addr.contains('@') {
                return Err(NotificationError::config(format!(
                    "Invalid recipient address: {}",
                    addr
                )));
            }
        }
        Ok(())
    }

    /// Adds a recipient.
    pub fn add_recipient(mut self, email: impl Into<String>) -> Self {
        self.to.push(email.into());
        self
    }

    /// Sets the port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Sets authentication credentials.
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }
}

/// Webhook notification configuration.
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// Webhook URL
    pub url: String,
    /// HTTP method
    pub method: String,
    /// Custom headers
    pub headers: HashMap<String, String>,
    /// Bearer token for authorization
    pub auth_token: Option<String>,
    /// Basic auth credentials
    pub basic_auth: Option<(String, String)>,
    /// Custom payload template
    pub payload_template: Option<String>,
    /// Include full event data
    pub include_full_event: bool,
    /// Verify SSL certificates
    pub verify_ssl: bool,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            auth_token: None,
            basic_auth: None,
            payload_template: None,
            include_full_event: true,
            verify_ssl: true,
        }
    }
}

impl WebhookConfig {
    /// Creates a new webhook configuration with the given URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
    }

    /// Loads configuration from environment variables.
    pub fn from_env() -> Option<Self> {
        let url = env::var("RUSTIBLE_WEBHOOK_URL").ok()?;

        let method = env::var("RUSTIBLE_WEBHOOK_METHOD").unwrap_or_else(|_| "POST".to_string());

        let headers: HashMap<String, String> = env::var("RUSTIBLE_WEBHOOK_HEADERS")
            .ok()
            .and_then(|h| serde_json::from_str(&h).ok())
            .unwrap_or_default();

        let auth_token = env::var("RUSTIBLE_WEBHOOK_AUTH_TOKEN").ok();

        let basic_auth = env::var("RUSTIBLE_WEBHOOK_BASIC_AUTH")
            .ok()
            .and_then(|auth| {
                let parts: Vec<&str> = auth.splitn(2, ':').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            });

        let verify_ssl = env::var("RUSTIBLE_WEBHOOK_VERIFY_SSL")
            .map(|v| !v.eq_ignore_ascii_case("false") && v != "0")
            .unwrap_or(true);

        Some(Self {
            url,
            method,
            headers,
            auth_token,
            basic_auth,
            payload_template: env::var("RUSTIBLE_WEBHOOK_TEMPLATE").ok(),
            include_full_event: true,
            verify_ssl,
        })
    }

    /// Validates the configuration.
    pub fn validate(&self) -> NotificationResult<()> {
        if self.url.is_empty() {
            return Err(NotificationError::config("Webhook URL is required"));
        }
        if !self.url.starts_with("http://") && !self.url.starts_with("https://") {
            return Err(NotificationError::config(
                "Webhook URL must start with http:// or https://",
            ));
        }
        let valid_methods = ["GET", "POST", "PUT", "PATCH", "DELETE"];
        if !valid_methods.contains(&self.method.to_uppercase().as_str()) {
            return Err(NotificationError::config(format!(
                "Invalid HTTP method: {}",
                self.method
            )));
        }
        Ok(())
    }

    /// Sets the HTTP method.
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = method.into();
        self
    }

    /// Adds a header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Sets the auth token.
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    /// Sets basic authentication.
    pub fn with_basic_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.basic_auth = Some((username.into(), password.into()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        saved: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn new() -> Self {
            Self { saved: Vec::new() }
        }

        fn set(&mut self, key: &str, value: &str) {
            let prev = env::var(key).ok();
            self.saved.push((key.to_string(), prev));
            env::set_var(key, value);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(val) = value {
                    env::set_var(key, val);
                } else {
                    env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn test_notification_config_builder() {
        let config = NotificationConfig::builder()
            .slack_webhook("https://hooks.slack.com/test")
            .notify_on_success(true)
            .notify_on_failure(false)
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();

        assert!(config.slack.is_some());
        assert!(config.notify_on_success);
        assert!(!config.notify_on_failure);
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_slack_config_validation() {
        let config = SlackConfig::default();
        assert!(config.validate().is_err());

        let config = SlackConfig::new("https://hooks.slack.com/services/test");
        assert!(config.validate().is_ok());

        let config = SlackConfig::new("http://hooks.slack.com/services/test");
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_email_config_validation() {
        let mut config = EmailConfig::new("smtp.example.com", "from@example.com");
        assert!(config.validate().is_err()); // No recipients

        config = config.add_recipient("to@example.com");
        assert!(config.validate().is_ok());

        config.from = "invalid".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_webhook_config_validation() {
        let config = WebhookConfig::default();
        assert!(config.validate().is_err());

        let config = WebhookConfig::new("https://example.com/webhook");
        assert!(config.validate().is_ok());

        let config = WebhookConfig::new("ftp://example.com/webhook");
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_configured_backends() {
        let config = NotificationConfig::builder()
            .slack_webhook("https://hooks.slack.com/test")
            .webhook_url("https://example.com/hook")
            .build_unchecked();

        let backends = config.configured_backends();
        assert!(backends.contains(&"Slack"));
        assert!(backends.contains(&"Webhook"));
        assert!(!backends.contains(&"Email"));
    }

    #[test]
    fn test_notification_config_from_env() {
        let _lock = ENV_LOCK.lock().unwrap();
        let mut guard = EnvGuard::new();

        guard.set(
            "RUSTIBLE_SLACK_WEBHOOK_URL",
            "https://hooks.slack.com/services/test",
        );
        guard.set("RUSTIBLE_SMTP_HOST", "smtp.example.com");
        guard.set("RUSTIBLE_SMTP_FROM", "from@example.com");
        guard.set("RUSTIBLE_SMTP_TO", "to@example.com");
        guard.set("RUSTIBLE_WEBHOOK_URL", "https://example.com/hook");
        guard.set("RUSTIBLE_NOTIFY_ON_SUCCESS", "true");
        guard.set("RUSTIBLE_NOTIFY_ON_FAILURE", "0");
        guard.set("RUSTIBLE_NOTIFY_TEMPLATE", "/tmp/template");
        guard.set("RUSTIBLE_NOTIFY_TIMEOUT", "45");
        guard.set("RUSTIBLE_NOTIFY_RETRIES", "5");
        guard.set("RUSTIBLE_NOTIFY_RETRY_DELAY", "2");

        let config = NotificationConfig::from_env();
        assert!(config.slack.is_some());
        assert!(config.email.is_some());
        assert!(config.webhook.is_some());
        assert!(config.notify_on_success);
        assert!(!config.notify_on_failure);
        assert_eq!(config.template_path.as_deref(), Some("/tmp/template"));
        assert_eq!(config.timeout, Duration::from_secs(45));
        assert_eq!(config.retries, 5);
        assert_eq!(config.retry_delay, Duration::from_secs(2));
    }
}
