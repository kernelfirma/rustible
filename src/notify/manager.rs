//! Notification manager for coordinating multiple backends.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, error, info, warn};

use super::config::NotificationConfig;
use super::email::EmailNotifier;
use super::error::{NotificationError, NotificationResult};
use super::filter::NotificationFilter;
use super::slack::SlackNotifier;
use super::webhook::WebhookNotifier;
use super::{NotificationEvent, Notifier};

/// Manager for coordinating notifications across multiple backends.
#[derive(Debug)]
pub struct NotificationManager {
    /// Configured notification backends
    backends: Vec<Arc<dyn Notifier>>,
    /// Notification filter
    filter: NotificationFilter,
    /// Configuration
    config: NotificationConfig,
    /// Retry count
    retries: u32,
    /// Retry delay
    retry_delay: Duration,
}

impl NotificationManager {
    /// Creates a new notification manager with the given configuration.
    pub fn new(config: NotificationConfig) -> Self {
        let mut backends: Vec<Arc<dyn Notifier>> = Vec::new();

        // Create Slack notifier if configured
        if let Some(ref slack_config) = config.slack {
            match SlackNotifier::new(slack_config.clone(), config.timeout) {
                Ok(notifier) => {
                    info!("Slack notification backend configured");
                    backends.push(Arc::new(notifier));
                }
                Err(e) => {
                    warn!("Failed to configure Slack backend: {}", e);
                }
            }
        }

        // Create Email notifier if configured
        if let Some(ref email_config) = config.email {
            match EmailNotifier::new(email_config.clone(), config.timeout) {
                Ok(notifier) => {
                    info!("Email notification backend configured");
                    backends.push(Arc::new(notifier));
                }
                Err(e) => {
                    warn!("Failed to configure Email backend: {}", e);
                }
            }
        }

        // Create Webhook notifier if configured
        if let Some(ref webhook_config) = config.webhook {
            match WebhookNotifier::new(webhook_config.clone(), config.timeout) {
                Ok(notifier) => {
                    info!("Webhook notification backend configured");
                    backends.push(Arc::new(notifier));
                }
                Err(e) => {
                    warn!("Failed to configure Webhook backend: {}", e);
                }
            }
        }

        // Create filter from config
        let filter = NotificationFilter::builder()
            .notify_on_success(config.notify_on_success)
            .notify_on_failure(config.notify_on_failure)
            .build();

        Self {
            backends,
            filter,
            retries: config.retries,
            retry_delay: config.retry_delay,
            config,
        }
    }

    /// Creates a notification manager from environment variables.
    pub fn from_env() -> Self {
        Self::new(NotificationConfig::from_env())
    }

    /// Returns true if any backends are configured.
    pub fn has_backends(&self) -> bool {
        !self.backends.is_empty()
    }

    /// Returns the names of configured backends.
    pub fn backend_names(&self) -> Vec<&str> {
        self.backends.iter().map(|b| b.name()).collect()
    }

    /// Sets the notification filter.
    pub fn with_filter(mut self, filter: NotificationFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Sends a notification to all configured backends.
    ///
    /// This method is resilient - failures in one backend don't affect others.
    /// Returns Ok if at least one backend succeeded, or the last error if all failed.
    pub async fn notify(&self, event: &NotificationEvent) -> NotificationResult<()> {
        if self.backends.is_empty() {
            debug!("No notification backends configured, skipping notification");
            return Ok(());
        }

        // Check filter
        if !self.filter.should_notify(event) {
            debug!("Notification filtered out: {}", event.event_type());
            return Ok(());
        }

        let mut last_error: Option<NotificationError> = None;
        let mut success_count = 0;

        for backend in &self.backends {
            if !backend.is_configured() {
                continue;
            }

            match self.send_with_retry(backend.as_ref(), event).await {
                Ok(()) => {
                    success_count += 1;
                }
                Err(e) => {
                    error!("Failed to send notification via {}: {}", backend.name(), e);
                    last_error = Some(e);
                }
            }
        }

        if success_count > 0 {
            debug!(
                "Notification sent successfully to {}/{} backends",
                success_count,
                self.backends.len()
            );
            Ok(())
        } else if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(()) // No configured backends is not an error
        }
    }

    /// Sends a notification with retry logic.
    async fn send_with_retry(
        &self,
        backend: &dyn Notifier,
        event: &NotificationEvent,
    ) -> NotificationResult<()> {
        let mut last_error = None;

        for attempt in 0..=self.retries {
            if attempt > 0 {
                debug!(
                    "Retrying {} notification (attempt {}/{})",
                    backend.name(),
                    attempt + 1,
                    self.retries + 1
                );
                tokio::time::sleep(self.retry_delay).await;
            }

            match backend.send(event).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if !e.is_recoverable() {
                        return Err(e);
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| NotificationError::internal("Unknown error")))
    }

    /// Sends a notification asynchronously without waiting for completion.
    ///
    /// This is useful when you don't want notification delivery to block
    /// the main execution flow.
    pub fn notify_async(
        &self,
        event: NotificationEvent,
    ) -> tokio::task::JoinHandle<NotificationResult<()>>
    where
        Self: 'static,
    {
        let backends = self.backends.clone();
        let filter = self.filter.clone();
        let retries = self.retries;
        let retry_delay = self.retry_delay;

        tokio::spawn(async move {
            if backends.is_empty() {
                return Ok(());
            }

            if !filter.should_notify(&event) {
                return Ok(());
            }

            let mut last_error: Option<NotificationError> = None;
            let mut success_count = 0;

            for backend in &backends {
                if !backend.is_configured() {
                    continue;
                }

                let result =
                    send_with_retry_static(backend.as_ref(), &event, retries, retry_delay).await;

                match result {
                    Ok(()) => success_count += 1,
                    Err(e) => last_error = Some(e),
                }
            }

            if success_count > 0 {
                Ok(())
            } else if let Some(err) = last_error {
                Err(err)
            } else {
                Ok(())
            }
        })
    }

    /// Sends a playbook start notification.
    pub async fn playbook_started(
        &self,
        playbook: impl Into<String>,
        hosts: Vec<String>,
    ) -> NotificationResult<()> {
        let event = NotificationEvent::playbook_start(playbook, hosts);
        self.notify(&event).await
    }

    /// Sends a playbook complete notification.
    pub async fn playbook_completed(
        &self,
        playbook: impl Into<String>,
        success: bool,
        duration: Duration,
        host_stats: std::collections::HashMap<String, super::HostStats>,
        failures: Option<Vec<super::FailureInfo>>,
    ) -> NotificationResult<()> {
        let event =
            NotificationEvent::playbook_complete(playbook, success, duration, host_stats, failures);
        self.notify(&event).await
    }

    /// Sends a task failed notification.
    pub async fn task_failed(
        &self,
        playbook: impl Into<String>,
        task: impl Into<String>,
        host: impl Into<String>,
        error: impl Into<String>,
    ) -> NotificationResult<()> {
        let event = NotificationEvent::task_failed(playbook, task, host, error);
        self.notify(&event).await
    }

    /// Sends a host unreachable notification.
    pub async fn host_unreachable(
        &self,
        playbook: impl Into<String>,
        host: impl Into<String>,
        error: impl Into<String>,
    ) -> NotificationResult<()> {
        let event = NotificationEvent::host_unreachable(playbook, host, error);
        self.notify(&event).await
    }

    /// Sends a custom notification.
    pub async fn custom(
        &self,
        name: impl Into<String>,
        data: serde_json::Value,
    ) -> NotificationResult<()> {
        let event = NotificationEvent::custom(name, data);
        self.notify(&event).await
    }
}

/// Static retry function for use in spawned tasks.
async fn send_with_retry_static(
    backend: &dyn Notifier,
    event: &NotificationEvent,
    retries: u32,
    retry_delay: Duration,
) -> NotificationResult<()> {
    let mut last_error = None;

    for attempt in 0..=retries {
        if attempt > 0 {
            tokio::time::sleep(retry_delay).await;
        }

        match backend.send(event).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                if !e.is_recoverable() {
                    return Err(e);
                }
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| NotificationError::internal("Unknown error")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::test_support::{EnvGuard, ENV_LOCK};
    use std::sync::atomic::{AtomicU32, Ordering};

    const NOTIFY_ENV_KEYS: &[&str] = &[
        "RUSTIBLE_NOTIFY_ON_SUCCESS",
        "RUSTIBLE_NOTIFY_ON_FAILURE",
        "RUSTIBLE_NOTIFY_TEMPLATE",
        "RUSTIBLE_NOTIFY_TIMEOUT",
        "RUSTIBLE_NOTIFY_RETRIES",
        "RUSTIBLE_NOTIFY_RETRY_DELAY",
        "RUSTIBLE_SLACK_WEBHOOK_URL",
        "RUSTIBLE_SLACK_CHANNEL",
        "RUSTIBLE_SLACK_USERNAME",
        "RUSTIBLE_SLACK_ICON_EMOJI",
        "RUSTIBLE_SLACK_MENTION_ON_FAILURE",
        "RUSTIBLE_SLACK_INCLUDE_HOST_STATS",
        "RUSTIBLE_SLACK_INCLUDE_FAILURES",
        "RUSTIBLE_SMTP_HOST",
        "RUSTIBLE_SMTP_FROM",
        "RUSTIBLE_SMTP_TO",
        "RUSTIBLE_SMTP_PORT",
        "RUSTIBLE_SMTP_TLS",
        "RUSTIBLE_SMTP_CC",
        "RUSTIBLE_SMTP_USER",
        "RUSTIBLE_SMTP_PASSWORD",
        "RUSTIBLE_MAIL_SUBJECT_PREFIX",
        "RUSTIBLE_MAIL_HTML",
        "RUSTIBLE_WEBHOOK_URL",
        "RUSTIBLE_WEBHOOK_METHOD",
        "RUSTIBLE_WEBHOOK_HEADERS",
        "RUSTIBLE_WEBHOOK_AUTH_TOKEN",
        "RUSTIBLE_WEBHOOK_BASIC_AUTH",
        "RUSTIBLE_WEBHOOK_VERIFY_SSL",
        "RUSTIBLE_WEBHOOK_TEMPLATE",
    ];

    fn clear_notify_env(guard: &mut EnvGuard) {
        for key in NOTIFY_ENV_KEYS {
            guard.remove(key);
        }
    }

    /// Mock notifier for testing.
    #[derive(Debug)]
    struct MockNotifier {
        name: String,
        configured: bool,
        send_count: AtomicU32,
        should_fail: bool,
    }

    impl MockNotifier {
        fn new(name: &str, configured: bool) -> Self {
            Self {
                name: name.to_string(),
                configured,
                send_count: AtomicU32::new(0),
                should_fail: false,
            }
        }

        fn failing(name: &str) -> Self {
            Self {
                name: name.to_string(),
                configured: true,
                send_count: AtomicU32::new(0),
                should_fail: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl Notifier for MockNotifier {
        fn name(&self) -> &str {
            &self.name
        }

        fn is_configured(&self) -> bool {
            self.configured
        }

        async fn send(&self, _event: &NotificationEvent) -> NotificationResult<()> {
            self.send_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                Err(NotificationError::network("Mock failure"))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_manager_no_backends() {
        let config = NotificationConfig::default();
        let manager = NotificationManager::new(config);

        let event = NotificationEvent::playbook_start("test.yml", vec![]);
        let result = manager.notify(&event).await;

        assert!(result.is_ok());
        assert!(!manager.has_backends());
    }

    #[tokio::test]
    async fn test_manager_from_env() {
        // Without env vars set, should create manager with no backends
        let _lock = ENV_LOCK.lock().unwrap();
        let mut guard = EnvGuard::new();
        clear_notify_env(&mut guard);
        let manager = NotificationManager::from_env();
        assert!(!manager.has_backends());
    }
}
