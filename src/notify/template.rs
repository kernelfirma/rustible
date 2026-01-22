//! Notification message templating.

use std::collections::HashMap;
use std::path::Path;

use minijinja::{Environment, Value};
use serde::Serialize;

use super::error::{NotificationError, NotificationResult};
use super::{FailureInfo, HostStats, NotificationEvent, Severity};

/// Context for template rendering.
#[derive(Debug, Clone, Serialize)]
pub struct TemplateContext {
    /// Event type name
    pub event_type: String,
    /// Playbook name (if applicable)
    pub playbook: Option<String>,
    /// Whether this is a failure event
    pub is_failure: bool,
    /// Whether this is a success event
    pub is_success: bool,
    /// Severity level
    pub severity: String,
    /// Timestamp (ISO 8601)
    pub timestamp: String,
    /// Event-specific data
    pub data: serde_json::Value,
    /// Host statistics (for playbook complete events)
    pub host_stats: Option<HashMap<String, HostStats>>,
    /// Failure information (for failure events)
    pub failures: Option<Vec<FailureInfo>>,
    /// Duration in seconds (for complete events)
    pub duration_secs: Option<f64>,
    /// Formatted duration string
    pub duration: Option<String>,
    /// Total hosts count
    pub host_count: Option<usize>,
    /// Environment variables (filtered for safety)
    pub env: HashMap<String, String>,
}

impl TemplateContext {
    /// Creates a template context from a notification event.
    pub fn from_event(event: &NotificationEvent) -> Self {
        let severity = if event.is_failure() {
            Severity::Error
        } else {
            Severity::Info
        };

        let data = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);

        let mut ctx = Self {
            event_type: event.event_type().to_string(),
            playbook: event.playbook().map(String::from),
            is_failure: event.is_failure(),
            is_success: event.is_success(),
            severity: severity.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            data,
            host_stats: None,
            failures: None,
            duration_secs: None,
            duration: None,
            host_count: None,
            env: get_safe_env_vars(),
        };

        // Extract additional fields based on event type
        match event {
            NotificationEvent::PlaybookStart {
                hosts, timestamp, ..
            } => {
                ctx.host_count = Some(hosts.len());
                ctx.timestamp = timestamp.clone();
            }
            NotificationEvent::PlaybookComplete {
                host_stats,
                duration_secs,
                timestamp,
                failures,
                ..
            } => {
                ctx.host_stats = Some(host_stats.clone());
                ctx.duration_secs = Some(*duration_secs);
                ctx.duration = Some(format_duration(*duration_secs));
                ctx.host_count = Some(host_stats.len());
                ctx.timestamp = timestamp.clone();
                ctx.failures = failures.clone();
            }
            NotificationEvent::TaskFailed { timestamp, .. }
            | NotificationEvent::HostUnreachable { timestamp, .. }
            | NotificationEvent::Custom { timestamp, .. } => {
                ctx.timestamp = timestamp.clone();
            }
        }

        ctx
    }

    /// Adds a custom variable to the context.
    pub fn with_var(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        if let Ok(json_value) = serde_json::to_value(value) {
            if let serde_json::Value::Object(ref mut map) = self.data {
                map.insert(key.into(), json_value);
            }
        }
        self
    }
}

/// Notification template engine.
#[derive(Debug)]
pub struct NotificationTemplate {
    env: Environment<'static>,
    slack_template: Option<String>,
    email_template: Option<String>,
    webhook_template: Option<String>,
}

impl NotificationTemplate {
    /// Creates a new template engine with default templates.
    pub fn new() -> Self {
        let mut env = Environment::new();

        // Add custom filters
        env.add_filter("duration", format_duration_filter);
        env.add_filter("truncate", truncate_filter);
        env.add_filter("default", default_filter);
        env.add_filter("json", json_filter);

        Self {
            env,
            slack_template: None,
            email_template: None,
            webhook_template: None,
        }
    }

    /// Creates a template engine from a template directory.
    pub fn from_directory(path: &Path) -> NotificationResult<Self> {
        let mut template = Self::new();

        // Load Slack template
        let slack_path = path.join("slack.j2");
        if slack_path.exists() {
            template.slack_template = Some(std::fs::read_to_string(&slack_path).map_err(|e| {
                NotificationError::template(format!("Failed to read slack.j2: {}", e))
            })?);
        }

        // Load email template
        let email_path = path.join("email.j2");
        if email_path.exists() {
            template.email_template = Some(std::fs::read_to_string(&email_path).map_err(|e| {
                NotificationError::template(format!("Failed to read email.j2: {}", e))
            })?);
        }

        // Load webhook template
        let webhook_path = path.join("webhook.j2");
        if webhook_path.exists() {
            template.webhook_template =
                Some(std::fs::read_to_string(&webhook_path).map_err(|e| {
                    NotificationError::template(format!("Failed to read webhook.j2: {}", e))
                })?);
        }

        Ok(template)
    }

    /// Sets the Slack template.
    pub fn with_slack_template(mut self, template: impl Into<String>) -> Self {
        self.slack_template = Some(template.into());
        self
    }

    /// Sets the email template.
    pub fn with_email_template(mut self, template: impl Into<String>) -> Self {
        self.email_template = Some(template.into());
        self
    }

    /// Sets the webhook template.
    pub fn with_webhook_template(mut self, template: impl Into<String>) -> Self {
        self.webhook_template = Some(template.into());
        self
    }

    /// Renders the Slack template.
    pub fn render_slack(&self, ctx: &TemplateContext) -> NotificationResult<String> {
        let template = self
            .slack_template
            .as_deref()
            .unwrap_or(DEFAULT_SLACK_TEMPLATE);
        self.render(template, ctx)
    }

    /// Renders the email template.
    pub fn render_email(&self, ctx: &TemplateContext) -> NotificationResult<String> {
        let template = self
            .email_template
            .as_deref()
            .unwrap_or(DEFAULT_EMAIL_TEMPLATE);
        self.render(template, ctx)
    }

    /// Renders the webhook template.
    pub fn render_webhook(&self, ctx: &TemplateContext) -> NotificationResult<String> {
        let template = self
            .webhook_template
            .as_deref()
            .unwrap_or(DEFAULT_WEBHOOK_TEMPLATE);
        self.render(template, ctx)
    }

    /// Renders a template string with the given context.
    pub fn render(&self, template: &str, ctx: &TemplateContext) -> NotificationResult<String> {
        let tmpl = self.env.template_from_str(template)?;
        let result = tmpl.render(ctx)?;
        Ok(result)
    }

    /// Validates a template string.
    pub fn validate(&self, template: &str) -> NotificationResult<()> {
        self.env
            .template_from_str(template)
            .map(|_| ())
            .map_err(|e| NotificationError::template(e.to_string()))
    }
}

impl Default for NotificationTemplate {
    fn default() -> Self {
        Self::new()
    }
}

/// Gets safe environment variables for template context.
fn get_safe_env_vars() -> HashMap<String, String> {
    let safe_prefixes = ["RUSTIBLE_", "CI", "BUILD_", "GITHUB_"];
    let mut vars = HashMap::new();

    for (key, value) in std::env::vars() {
        for prefix in &safe_prefixes {
            if key.starts_with(prefix) {
                // Skip sensitive variables
                if key.contains("PASSWORD")
                    || key.contains("SECRET")
                    || key.contains("TOKEN")
                    || key.contains("KEY")
                {
                    continue;
                }
                vars.insert(key.clone(), value.clone());
                break;
            }
        }
    }

    vars
}

/// Formats a duration in seconds.
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

/// Template filter for formatting duration.
fn format_duration_filter(value: f64) -> String {
    format_duration(value)
}

/// Template filter for truncating strings.
fn truncate_filter(value: String, len: usize) -> String {
    if value.len() <= len {
        value
    } else {
        format!("{}...", &value[..len.saturating_sub(3)])
    }
}

/// Template filter for default values.
fn default_filter(value: Option<String>, default: String) -> String {
    value.unwrap_or(default)
}

/// Template filter for JSON serialization.
fn json_filter(value: Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
}

/// Default Slack message template.
const DEFAULT_SLACK_TEMPLATE: &str = r#"{
  "text": "{{ event_type | title }} - {{ playbook | default('N/A') }}",
  "attachments": [
    {
      "color": "{% if is_failure %}#dc3545{% elif is_success %}#36a64f{% else %}#2196F3{% endif %}",
      "title": "{{ playbook | default('Rustible Notification') }}",
      "fields": [
        {
          "title": "Event",
          "value": "{{ event_type }}",
          "short": true
        }{% if duration %},
        {
          "title": "Duration",
          "value": "{{ duration }}",
          "short": true
        }{% endif %}
      ],
      "footer": "Rustible",
      "ts": "{{ timestamp }}"
    }
  ]
}"#;

/// Default email template.
const DEFAULT_EMAIL_TEMPLATE: &str = r"Rustible Notification
======================

Event: {{ event_type }}
{% if playbook %}Playbook: {{ playbook }}{% endif %}
Severity: {{ severity }}
Time: {{ timestamp }}

{% if duration %}Duration: {{ duration }}{% endif %}
{% if host_count %}Hosts: {{ host_count }}{% endif %}

{% if host_stats %}
Host Summary:
{% for host, stats in host_stats %}
  {{ host }}: ok={{ stats.ok }} changed={{ stats.changed }} failed={{ stats.failed }}
{% endfor %}
{% endif %}

{% if failures %}
Failures:
{% for f in failures %}
  - {{ f.task }} on {{ f.host }}: {{ f.message }}
{% endfor %}
{% endif %}

---
Generated by Rustible
";

/// Default webhook JSON template.
const DEFAULT_WEBHOOK_TEMPLATE: &str = r#"{
  "event": "{{ event_type }}",
  "playbook": {{ playbook | json | default('null') }},
  "is_failure": {{ is_failure }},
  "is_success": {{ is_success }},
  "severity": "{{ severity }}",
  "timestamp": "{{ timestamp }}"{% if duration_secs %},
  "duration_secs": {{ duration_secs }}{% endif %}{% if host_count %},
  "host_count": {{ host_count }}{% endif %}
}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_template_context_from_event() {
        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            true,
            Duration::from_secs(120),
            HashMap::new(),
            None,
        );

        let ctx = TemplateContext::from_event(&event);

        assert_eq!(ctx.event_type, "playbook_complete");
        assert_eq!(ctx.playbook, Some("deploy.yml".to_string()));
        assert!(ctx.is_success);
        assert!(!ctx.is_failure);
        assert!(ctx.duration.is_some());
    }

    #[test]
    fn test_template_context_with_var() {
        let event = NotificationEvent::playbook_start("test.yml", vec![]);
        let ctx = TemplateContext::from_event(&event).with_var("custom_field", "custom_value");

        assert!(ctx.data.is_object());
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30.5), "30.5s");
        assert_eq!(format_duration(90.0), "1m 30.0s");
        assert_eq!(format_duration(3700.0), "1h 1m");
    }

    #[test]
    fn test_template_render() {
        let template = NotificationTemplate::new();
        let event = NotificationEvent::task_failed("test.yml", "task1", "host1", "error");
        let ctx = TemplateContext::from_event(&event);

        let result = template.render("Event: {{ event_type }}", &ctx);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Event: task_failed");
    }

    #[test]
    fn test_template_validate() {
        let template = NotificationTemplate::new();

        assert!(template.validate("Hello {{ name }}").is_ok());
        assert!(template.validate("{% if x %}y{% endif %}").is_ok());
        assert!(template.validate("{% if x %}").is_err()); // Unclosed if
    }

    #[test]
    fn test_render_slack_template() {
        let template = NotificationTemplate::new();
        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            true,
            Duration::from_secs(45),
            HashMap::new(),
            None,
        );
        let ctx = TemplateContext::from_event(&event);

        let result = template.render_slack(&ctx);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("deploy.yml"));
        assert!(rendered.contains("#36a64f")); // Green for success
    }

    #[test]
    fn test_render_email_template() {
        let template = NotificationTemplate::new();
        let event = NotificationEvent::task_failed("test.yml", "Copy file", "host1", "Timeout");
        let ctx = TemplateContext::from_event(&event);

        let result = template.render_email(&ctx);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("Rustible Notification"));
        assert!(rendered.contains("task_failed"));
    }

    #[test]
    fn test_custom_template() {
        let template = NotificationTemplate::new().with_slack_template("Custom: {{ playbook }}");

        let event = NotificationEvent::playbook_start("custom.yml", vec![]);
        let ctx = TemplateContext::from_event(&event);

        let result = template.render_slack(&ctx);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Custom: custom.yml");
    }
}
