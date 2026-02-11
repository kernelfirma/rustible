//! Reactor engine for rule-based event processing.
//!
//! The reactor evaluates incoming events against a set of configurable rules
//! and returns the actions that should be triggered when conditions match.

use super::action::ReactorAction;
use super::event::{Event, EventType};
use serde::{Deserialize, Serialize};

/// The reactor engine that evaluates events against rules.
pub struct ReactorEngine {
    rules: Vec<ReactorRule>,
}

impl ReactorEngine {
    /// Create a new reactor engine with no rules.
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a rule to the reactor engine.
    pub fn add_rule(&mut self, rule: ReactorRule) {
        self.rules.push(rule);
    }

    /// Evaluate an event against all enabled rules and return matching actions.
    ///
    /// Only enabled rules are considered. Returns references to the actions
    /// from all rules whose conditions match the given event.
    pub fn evaluate(&self, event: &Event) -> Vec<&ReactorAction> {
        self.rules
            .iter()
            .filter(|rule| rule.enabled && rule.condition.matches(event))
            .map(|rule| &rule.action)
            .collect()
    }

    /// List all rules currently registered in the engine.
    pub fn list_rules(&self) -> &[ReactorRule] {
        &self.rules
    }
}

impl Default for ReactorEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// A reactor rule that maps a condition to an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactorRule {
    /// Human-readable name for this rule
    pub name: String,
    /// Condition that must be satisfied for the action to fire
    pub condition: ReactorCondition,
    /// Action to execute when the condition matches
    pub action: ReactorAction,
    /// Whether this rule is currently enabled
    pub enabled: bool,
}

impl ReactorRule {
    /// Create a new enabled rule.
    pub fn new(name: impl Into<String>, condition: ReactorCondition, action: ReactorAction) -> Self {
        Self {
            name: name.into(),
            condition,
            action,
            enabled: true,
        }
    }
}

/// Conditions that can be evaluated against events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReactorCondition {
    /// Matches events of a specific type
    EventTypeMatch(EventType),
    /// Matches events from a specific source component
    SourceMatch(String),
    /// Matches events whose payload contains a specific key-value pair
    PayloadContains(String, String),
    /// All sub-conditions must match (logical AND)
    All(Vec<ReactorCondition>),
    /// At least one sub-condition must match (logical OR)
    Any(Vec<ReactorCondition>),
}

impl ReactorCondition {
    /// Evaluate whether this condition matches the given event.
    pub fn matches(&self, event: &Event) -> bool {
        match self {
            ReactorCondition::EventTypeMatch(event_type) => event.event_type == *event_type,
            ReactorCondition::SourceMatch(source) => event.source.component == *source,
            ReactorCondition::PayloadContains(key, value) => event
                .payload
                .get(key)
                .and_then(|v| v.as_str())
                .map_or(false, |v| v == value),
            ReactorCondition::All(conditions) => {
                conditions.iter().all(|c| c.matches(event))
            }
            ReactorCondition::Any(conditions) => {
                conditions.iter().any(|c| c.matches(event))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventbus::event::EventSource;
    use std::collections::HashMap;

    #[test]
    fn test_reactor_evaluate_matching_rule() {
        let mut engine = ReactorEngine::new();

        engine.add_rule(ReactorRule::new(
            "restart-on-failure",
            ReactorCondition::EventTypeMatch(EventType::PlaybookFailed),
            ReactorAction::RunPlaybook {
                path: "recovery.yml".to_string(),
            },
        ));

        engine.add_rule(ReactorRule::new(
            "notify-on-complete",
            ReactorCondition::EventTypeMatch(EventType::PlaybookCompleted),
            ReactorAction::Notify {
                channel: "slack".to_string(),
                message: "Playbook completed".to_string(),
            },
        ));

        let failed_event = Event::new(EventType::PlaybookFailed, EventSource::new("executor"));
        let actions = engine.evaluate(&failed_event);

        assert_eq!(actions.len(), 1);
        match actions[0] {
            ReactorAction::RunPlaybook { path } => assert_eq!(path, "recovery.yml"),
            _ => panic!("Expected RunPlaybook action"),
        }
    }

    #[test]
    fn test_reactor_disabled_rules_skipped() {
        let mut engine = ReactorEngine::new();

        let mut rule = ReactorRule::new(
            "disabled-rule",
            ReactorCondition::EventTypeMatch(EventType::HostDown),
            ReactorAction::Notify {
                channel: "pager".to_string(),
                message: "Host is down!".to_string(),
            },
        );
        rule.enabled = false;
        engine.add_rule(rule);

        let event = Event::new(EventType::HostDown, EventSource::new("monitor"));
        let actions = engine.evaluate(&event);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_reactor_composite_conditions() {
        let mut engine = ReactorEngine::new();

        // Rule: match HostDown events from the "monitor" source
        engine.add_rule(ReactorRule::new(
            "monitor-host-down",
            ReactorCondition::All(vec![
                ReactorCondition::EventTypeMatch(EventType::HostDown),
                ReactorCondition::SourceMatch("monitor".to_string()),
            ]),
            ReactorAction::RunPlaybook {
                path: "failover.yml".to_string(),
            },
        ));

        // HostDown from monitor -- should match
        let matching = Event::new(EventType::HostDown, EventSource::new("monitor"));
        assert_eq!(engine.evaluate(&matching).len(), 1);

        // HostDown from executor -- should not match
        let non_matching = Event::new(EventType::HostDown, EventSource::new("executor"));
        assert_eq!(engine.evaluate(&non_matching).len(), 0);
    }

    #[test]
    fn test_reactor_any_condition() {
        let condition = ReactorCondition::Any(vec![
            ReactorCondition::EventTypeMatch(EventType::HostDown),
            ReactorCondition::EventTypeMatch(EventType::TaskFailed),
        ]);

        let host_down = Event::new(EventType::HostDown, EventSource::new("test"));
        let task_failed = Event::new(EventType::TaskFailed, EventSource::new("test"));
        let playbook_ok = Event::new(EventType::PlaybookCompleted, EventSource::new("test"));

        assert!(condition.matches(&host_down));
        assert!(condition.matches(&task_failed));
        assert!(!condition.matches(&playbook_ok));
    }

    #[test]
    fn test_reactor_payload_condition() {
        let condition =
            ReactorCondition::PayloadContains("env".to_string(), "production".to_string());

        let mut payload = HashMap::new();
        payload.insert(
            "env".to_string(),
            serde_json::Value::String("production".to_string()),
        );
        let matching = Event::with_payload(
            EventType::PlaybookStarted,
            EventSource::new("test"),
            payload,
        );
        assert!(condition.matches(&matching));

        let no_payload = Event::new(EventType::PlaybookStarted, EventSource::new("test"));
        assert!(!condition.matches(&no_payload));
    }

    #[test]
    fn test_reactor_list_rules() {
        let mut engine = ReactorEngine::new();
        assert!(engine.list_rules().is_empty());

        engine.add_rule(ReactorRule::new(
            "rule-1",
            ReactorCondition::EventTypeMatch(EventType::HostDown),
            ReactorAction::Notify {
                channel: "slack".to_string(),
                message: "Alert".to_string(),
            },
        ));

        assert_eq!(engine.list_rules().len(), 1);
        assert_eq!(engine.list_rules()[0].name, "rule-1");
    }
}
