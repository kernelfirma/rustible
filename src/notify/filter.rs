//! Notification filtering and rules.

use serde::{Deserialize, Serialize};

use super::NotificationEvent;

/// Filter for controlling which notifications are sent.
#[derive(Debug, Clone, Default)]
pub struct NotificationFilter {
    /// Rules for filtering notifications
    rules: Vec<NotificationRule>,
    /// Whether to notify on success events
    notify_on_success: bool,
    /// Whether to notify on failure events
    notify_on_failure: bool,
}

impl NotificationFilter {
    /// Creates a new notification filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a filter builder.
    pub fn builder() -> NotificationFilterBuilder {
        NotificationFilterBuilder::default()
    }

    /// Checks if a notification event should be sent.
    pub fn should_notify(&self, event: &NotificationEvent) -> bool {
        // Check success/failure settings first
        if event.is_success() && !self.notify_on_success {
            return false;
        }
        if event.is_failure() && !self.notify_on_failure {
            return false;
        }

        // If no rules, allow all
        if self.rules.is_empty() {
            return true;
        }

        // Check rules - any matching allow rule permits, any matching deny rule blocks
        let mut allowed = false;
        for rule in &self.rules {
            if rule.matches(event) {
                match rule.action {
                    RuleAction::Allow => allowed = true,
                    RuleAction::Deny => return false,
                }
            }
        }

        // If we have rules but none matched, deny by default
        if !self.rules.is_empty() && !allowed {
            return false;
        }

        true
    }

    /// Adds a rule to the filter.
    pub fn add_rule(&mut self, rule: NotificationRule) {
        self.rules.push(rule);
    }

    /// Sets whether to notify on success.
    pub fn set_notify_on_success(&mut self, enabled: bool) {
        self.notify_on_success = enabled;
    }

    /// Sets whether to notify on failure.
    pub fn set_notify_on_failure(&mut self, enabled: bool) {
        self.notify_on_failure = enabled;
    }
}

/// Builder for creating notification filters.
#[derive(Debug, Clone, Default)]
pub struct NotificationFilterBuilder {
    filter: NotificationFilter,
}

impl NotificationFilterBuilder {
    /// Enables notification on success.
    pub fn notify_on_success(mut self, enabled: bool) -> Self {
        self.filter.notify_on_success = enabled;
        self
    }

    /// Enables notification on failure.
    pub fn notify_on_failure(mut self, enabled: bool) -> Self {
        self.filter.notify_on_failure = enabled;
        self
    }

    /// Adds an allow rule for a specific event type.
    pub fn allow_event(mut self, event_type: impl Into<String>) -> Self {
        self.filter.rules.push(NotificationRule {
            event_type: Some(event_type.into()),
            playbook_pattern: None,
            action: RuleAction::Allow,
        });
        self
    }

    /// Adds a deny rule for a specific event type.
    pub fn deny_event(mut self, event_type: impl Into<String>) -> Self {
        self.filter.rules.push(NotificationRule {
            event_type: Some(event_type.into()),
            playbook_pattern: None,
            action: RuleAction::Deny,
        });
        self
    }

    /// Adds an allow rule for a playbook pattern.
    pub fn allow_playbook(mut self, pattern: impl Into<String>) -> Self {
        self.filter.rules.push(NotificationRule {
            event_type: None,
            playbook_pattern: Some(pattern.into()),
            action: RuleAction::Allow,
        });
        self
    }

    /// Adds a deny rule for a playbook pattern.
    pub fn deny_playbook(mut self, pattern: impl Into<String>) -> Self {
        self.filter.rules.push(NotificationRule {
            event_type: None,
            playbook_pattern: Some(pattern.into()),
            action: RuleAction::Deny,
        });
        self
    }

    /// Adds a custom rule.
    pub fn rule(mut self, rule: NotificationRule) -> Self {
        self.filter.rules.push(rule);
        self
    }

    /// Builds the filter.
    pub fn build(self) -> NotificationFilter {
        self.filter
    }
}

/// A rule for filtering notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRule {
    /// Event type to match (exact match)
    pub event_type: Option<String>,
    /// Playbook pattern to match (glob pattern)
    pub playbook_pattern: Option<String>,
    /// Action to take when rule matches
    pub action: RuleAction,
}

impl NotificationRule {
    /// Creates a new allow rule.
    pub fn allow() -> Self {
        Self {
            event_type: None,
            playbook_pattern: None,
            action: RuleAction::Allow,
        }
    }

    /// Creates a new deny rule.
    pub fn deny() -> Self {
        Self {
            event_type: None,
            playbook_pattern: None,
            action: RuleAction::Deny,
        }
    }

    /// Sets the event type filter.
    pub fn for_event(mut self, event_type: impl Into<String>) -> Self {
        self.event_type = Some(event_type.into());
        self
    }

    /// Sets the playbook pattern filter.
    pub fn for_playbook(mut self, pattern: impl Into<String>) -> Self {
        self.playbook_pattern = Some(pattern.into());
        self
    }

    /// Checks if this rule matches the given event.
    pub fn matches(&self, event: &NotificationEvent) -> bool {
        // Check event type
        if let Some(ref expected) = self.event_type {
            if event.event_type() != expected {
                return false;
            }
        }

        // Check playbook pattern
        if let Some(ref pattern) = self.playbook_pattern {
            if let Some(playbook) = event.playbook() {
                if !matches_glob(pattern, playbook) {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

/// Action to take when a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum RuleAction {
    /// Allow the notification
    #[default]
    Allow,
    /// Deny (block) the notification
    Deny,
}


/// Simple glob pattern matching.
fn matches_glob(pattern: &str, text: &str) -> bool {
    // Simple implementation: * matches any sequence
    if pattern == "*" {
        return true;
    }

    if !pattern.contains('*') {
        return pattern == text;
    }

    let parts: Vec<&str> = pattern.split('*').collect();

    // Must start with first part
    if !parts[0].is_empty() && !text.starts_with(parts[0]) {
        return false;
    }

    // Must end with last part
    if !parts[parts.len() - 1].is_empty() && !text.ends_with(parts[parts.len() - 1]) {
        return false;
    }

    // Check intermediate parts exist in order
    let mut pos = 0;
    for part in &parts {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = text[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_default_allows_all() {
        let filter = NotificationFilter::builder()
            .notify_on_success(true)
            .notify_on_failure(true)
            .build();

        let event = NotificationEvent::playbook_start("test.yml", vec![]);
        assert!(filter.should_notify(&event));
    }

    #[test]
    fn test_filter_deny_success() {
        let filter = NotificationFilter::builder()
            .notify_on_success(false)
            .notify_on_failure(true)
            .build();

        let success_event = NotificationEvent::playbook_complete(
            "test.yml",
            true,
            std::time::Duration::from_secs(10),
            std::collections::HashMap::new(),
            None,
        );
        assert!(!filter.should_notify(&success_event));

        let failure_event = NotificationEvent::playbook_complete(
            "test.yml",
            false,
            std::time::Duration::from_secs(10),
            std::collections::HashMap::new(),
            None,
        );
        assert!(filter.should_notify(&failure_event));
    }

    #[test]
    fn test_rule_matching() {
        let rule = NotificationRule::allow().for_event("task_failed");

        let event = NotificationEvent::task_failed("test.yml", "task1", "host1", "error");
        assert!(rule.matches(&event));

        let event = NotificationEvent::playbook_start("test.yml", vec![]);
        assert!(!rule.matches(&event));
    }

    #[test]
    fn test_playbook_pattern_matching() {
        let rule = NotificationRule::allow().for_playbook("deploy*.yml");

        let event = NotificationEvent::playbook_start("deploy_prod.yml", vec![]);
        assert!(rule.matches(&event));

        let event = NotificationEvent::playbook_start("test.yml", vec![]);
        assert!(!rule.matches(&event));
    }

    #[test]
    fn test_glob_matching() {
        assert!(matches_glob("*", "anything"));
        assert!(matches_glob("test", "test"));
        assert!(!matches_glob("test", "other"));
        assert!(matches_glob("*.yml", "deploy.yml"));
        assert!(matches_glob("deploy*", "deploy_prod"));
        assert!(matches_glob("*prod*", "deploy_prod.yml"));
    }

    #[test]
    fn test_filter_with_rules() {
        let filter = NotificationFilter::builder()
            .notify_on_success(true)
            .notify_on_failure(true)
            .allow_event("playbook_complete")
            .deny_playbook("internal/*")
            .build();

        let event = NotificationEvent::playbook_complete(
            "deploy.yml",
            true,
            std::time::Duration::from_secs(10),
            std::collections::HashMap::new(),
            None,
        );
        assert!(filter.should_notify(&event));
    }
}
