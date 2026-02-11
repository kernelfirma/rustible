//! Event types and structures for the event bus system.
//!
//! Events represent significant occurrences within the Rustible automation
//! pipeline, such as playbook lifecycle events, host status changes, and
//! configuration drift detection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A system event that can be published to the event bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique identifier for this event
    pub id: String,
    /// The type of event
    pub event_type: EventType,
    /// Source component that generated the event
    pub source: EventSource,
    /// Timestamp when the event was created
    pub timestamp: DateTime<Utc>,
    /// Arbitrary key-value payload attached to the event
    pub payload: HashMap<String, serde_json::Value>,
}

impl Event {
    /// Create a new event with the given type and source.
    ///
    /// The event ID is automatically generated as a UUID v4 and the
    /// timestamp is set to the current UTC time.
    pub fn new(event_type: EventType, source: EventSource) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            event_type,
            source,
            timestamp: Utc::now(),
            payload: HashMap::new(),
        }
    }

    /// Create a new event with an explicit payload.
    pub fn with_payload(
        event_type: EventType,
        source: EventSource,
        payload: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            event_type,
            source,
            timestamp: Utc::now(),
            payload,
        }
    }

    /// Add a key-value pair to the event payload.
    pub fn set_payload(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.payload.insert(key.into(), value);
    }
}

/// Enumeration of event types that the system can produce.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventType {
    /// A playbook execution has started
    PlaybookStarted,
    /// A playbook execution completed successfully
    PlaybookCompleted,
    /// A playbook execution failed
    PlaybookFailed,
    /// A task's state changed (e.g., ok, changed, skipped)
    TaskChanged,
    /// A task failed during execution
    TaskFailed,
    /// A host became unreachable
    HostDown,
    /// A previously unreachable host is now reachable
    HostUp,
    /// Configuration drift was detected on a host
    DriftDetected,
    /// Previously detected drift has been resolved
    DriftResolved,
    /// Infrastructure provisioning completed
    ProvisionComplete,
    /// A custom/user-defined event type
    Custom(String),
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::PlaybookStarted => write!(f, "playbook.started"),
            EventType::PlaybookCompleted => write!(f, "playbook.completed"),
            EventType::PlaybookFailed => write!(f, "playbook.failed"),
            EventType::TaskChanged => write!(f, "task.changed"),
            EventType::TaskFailed => write!(f, "task.failed"),
            EventType::HostDown => write!(f, "host.down"),
            EventType::HostUp => write!(f, "host.up"),
            EventType::DriftDetected => write!(f, "drift.detected"),
            EventType::DriftResolved => write!(f, "drift.resolved"),
            EventType::ProvisionComplete => write!(f, "provision.complete"),
            EventType::Custom(name) => write!(f, "custom.{}", name),
        }
    }
}

impl EventType {
    /// Parse an event type from a string representation.
    pub fn from_str_loose(s: &str) -> Self {
        match s {
            "playbook.started" | "PlaybookStarted" => EventType::PlaybookStarted,
            "playbook.completed" | "PlaybookCompleted" => EventType::PlaybookCompleted,
            "playbook.failed" | "PlaybookFailed" => EventType::PlaybookFailed,
            "task.changed" | "TaskChanged" => EventType::TaskChanged,
            "task.failed" | "TaskFailed" => EventType::TaskFailed,
            "host.down" | "HostDown" => EventType::HostDown,
            "host.up" | "HostUp" => EventType::HostUp,
            "drift.detected" | "DriftDetected" => EventType::DriftDetected,
            "drift.resolved" | "DriftResolved" => EventType::DriftResolved,
            "provision.complete" | "ProvisionComplete" => EventType::ProvisionComplete,
            other => EventType::Custom(other.to_string()),
        }
    }
}

/// The source component that generated an event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSource {
    /// Name of the component (e.g., "executor", "drift-detector", "provisioner")
    pub component: String,
    /// Optional host associated with the event
    pub host: Option<String>,
}

impl EventSource {
    /// Create a new event source with the given component name.
    pub fn new(component: impl Into<String>) -> Self {
        Self {
            component: component.into(),
            host: None,
        }
    }

    /// Create a new event source with a component name and host.
    pub fn with_host(component: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            component: component.into(),
            host: Some(host.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let source = EventSource::new("executor");
        let event = Event::new(EventType::PlaybookStarted, source);

        assert!(!event.id.is_empty());
        assert_eq!(event.event_type, EventType::PlaybookStarted);
        assert_eq!(event.source.component, "executor");
        assert!(event.source.host.is_none());
        assert!(event.payload.is_empty());
    }

    #[test]
    fn test_event_with_payload() {
        let source = EventSource::with_host("executor", "web01");
        let mut payload = HashMap::new();
        payload.insert(
            "playbook".to_string(),
            serde_json::Value::String("site.yml".to_string()),
        );

        let event = Event::with_payload(EventType::PlaybookCompleted, source, payload);

        assert_eq!(event.event_type, EventType::PlaybookCompleted);
        assert_eq!(event.source.host.as_deref(), Some("web01"));
        assert_eq!(
            event.payload.get("playbook"),
            Some(&serde_json::Value::String("site.yml".to_string()))
        );
    }

    #[test]
    fn test_event_type_display() {
        assert_eq!(EventType::PlaybookStarted.to_string(), "playbook.started");
        assert_eq!(EventType::HostDown.to_string(), "host.down");
        assert_eq!(
            EventType::Custom("my_event".to_string()).to_string(),
            "custom.my_event"
        );
    }

    #[test]
    fn test_event_type_from_str_loose() {
        assert_eq!(
            EventType::from_str_loose("playbook.started"),
            EventType::PlaybookStarted
        );
        assert_eq!(
            EventType::from_str_loose("PlaybookStarted"),
            EventType::PlaybookStarted
        );
        assert_eq!(
            EventType::from_str_loose("host.down"),
            EventType::HostDown
        );
        assert_eq!(
            EventType::from_str_loose("unknown_event"),
            EventType::Custom("unknown_event".to_string())
        );
    }

    #[test]
    fn test_event_set_payload() {
        let mut event = Event::new(EventType::TaskChanged, EventSource::new("runner"));
        event.set_payload("task_name", serde_json::json!("Install nginx"));
        event.set_payload("changed", serde_json::json!(true));

        assert_eq!(event.payload.len(), 2);
        assert_eq!(
            event.payload.get("task_name"),
            Some(&serde_json::json!("Install nginx"))
        );
    }

    #[test]
    fn test_event_source_with_host() {
        let source = EventSource::with_host("provisioner", "db01.example.com");
        assert_eq!(source.component, "provisioner");
        assert_eq!(source.host.as_deref(), Some("db01.example.com"));
    }
}
