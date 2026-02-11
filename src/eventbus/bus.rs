//! Event bus for publishing and subscribing to events.
//!
//! The event bus provides a pub/sub mechanism where subscribers register
//! interest in events and receive notifications when matching events are
//! published.

use super::event::{Event, EventType};

/// Trait for event subscribers that receive published events.
pub trait EventSubscriber: Send + Sync {
    /// Called when an event matching the subscriber's filter is published.
    fn on_event(&self, event: &Event);

    /// Returns the name of this subscriber for identification and logging.
    fn name(&self) -> &str;

    /// Returns an optional filter to restrict which events this subscriber receives.
    /// If `None`, the subscriber receives all events.
    fn filter(&self) -> Option<EventFilter> {
        None
    }
}

/// Filter criteria for controlling which events a subscriber receives.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// If set, only events with these types will be delivered.
    pub event_types: Option<Vec<EventType>>,
    /// If set, only events from these source components will be delivered.
    pub sources: Option<Vec<String>>,
}

impl EventFilter {
    /// Create a new empty filter that matches all events.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a filter that matches specific event types.
    pub fn with_event_types(event_types: Vec<EventType>) -> Self {
        Self {
            event_types: Some(event_types),
            sources: None,
        }
    }

    /// Create a filter that matches specific source components.
    pub fn with_sources(sources: Vec<String>) -> Self {
        Self {
            event_types: None,
            sources: Some(sources),
        }
    }

    /// Check whether a given event matches this filter.
    pub fn matches(&self, event: &Event) -> bool {
        let type_match = match &self.event_types {
            Some(types) => types.contains(&event.event_type),
            None => true,
        };

        let source_match = match &self.sources {
            Some(sources) => sources.contains(&event.source.component),
            None => true,
        };

        type_match && source_match
    }
}

/// The central event bus that manages subscribers and event distribution.
pub struct EventBus {
    subscribers: Vec<Box<dyn EventSubscriber>>,
}

impl EventBus {
    /// Create a new empty event bus with no subscribers.
    pub fn new() -> Self {
        Self {
            subscribers: Vec::new(),
        }
    }

    /// Register a subscriber to receive events.
    pub fn subscribe(&mut self, subscriber: Box<dyn EventSubscriber>) {
        self.subscribers.push(subscriber);
    }

    /// Publish an event to all matching subscribers.
    ///
    /// Each subscriber's filter is checked; if it matches (or if the subscriber
    /// has no filter), the subscriber's `on_event` method is called.
    pub fn publish(&self, event: &Event) {
        for subscriber in &self.subscribers {
            let should_deliver = match subscriber.filter() {
                Some(filter) => filter.matches(event),
                None => true,
            };

            if should_deliver {
                subscriber.on_event(event);
            }
        }
    }

    /// Returns the number of registered subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventbus::event::EventSource;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A simple test subscriber that counts received events.
    struct CountingSubscriber {
        name: String,
        count: Arc<AtomicUsize>,
        filter: Option<EventFilter>,
    }

    impl CountingSubscriber {
        fn new(name: &str, count: Arc<AtomicUsize>) -> Self {
            Self {
                name: name.to_string(),
                count,
                filter: None,
            }
        }

        fn with_filter(name: &str, count: Arc<AtomicUsize>, filter: EventFilter) -> Self {
            Self {
                name: name.to_string(),
                count,
                filter: Some(filter),
            }
        }
    }

    impl EventSubscriber for CountingSubscriber {
        fn on_event(&self, _event: &Event) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn filter(&self) -> Option<EventFilter> {
            self.filter.clone()
        }
    }

    #[test]
    fn test_event_bus_publish_to_all() {
        let mut bus = EventBus::new();
        let count1 = Arc::new(AtomicUsize::new(0));
        let count2 = Arc::new(AtomicUsize::new(0));

        bus.subscribe(Box::new(CountingSubscriber::new("sub1", count1.clone())));
        bus.subscribe(Box::new(CountingSubscriber::new("sub2", count2.clone())));

        assert_eq!(bus.subscriber_count(), 2);

        let event = Event::new(EventType::PlaybookStarted, EventSource::new("test"));
        bus.publish(&event);

        assert_eq!(count1.load(Ordering::SeqCst), 1);
        assert_eq!(count2.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_event_bus_filter_by_type() {
        let mut bus = EventBus::new();
        let all_count = Arc::new(AtomicUsize::new(0));
        let filtered_count = Arc::new(AtomicUsize::new(0));

        // Subscriber with no filter receives everything
        bus.subscribe(Box::new(CountingSubscriber::new(
            "all",
            all_count.clone(),
        )));

        // Subscriber only interested in HostDown events
        bus.subscribe(Box::new(CountingSubscriber::with_filter(
            "host-monitor",
            filtered_count.clone(),
            EventFilter::with_event_types(vec![EventType::HostDown]),
        )));

        let playbook_event = Event::new(EventType::PlaybookStarted, EventSource::new("test"));
        bus.publish(&playbook_event);

        let host_event = Event::new(EventType::HostDown, EventSource::new("monitor"));
        bus.publish(&host_event);

        // "all" subscriber received both events
        assert_eq!(all_count.load(Ordering::SeqCst), 2);
        // "host-monitor" subscriber received only the HostDown event
        assert_eq!(filtered_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_event_bus_filter_by_source() {
        let mut bus = EventBus::new();
        let count = Arc::new(AtomicUsize::new(0));

        bus.subscribe(Box::new(CountingSubscriber::with_filter(
            "executor-only",
            count.clone(),
            EventFilter::with_sources(vec!["executor".to_string()]),
        )));

        // Event from executor -- should match
        let event1 = Event::new(EventType::TaskChanged, EventSource::new("executor"));
        bus.publish(&event1);

        // Event from monitor -- should not match
        let event2 = Event::new(EventType::HostDown, EventSource::new("monitor"));
        bus.publish(&event2);

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_event_filter_matches() {
        let filter = EventFilter {
            event_types: Some(vec![EventType::PlaybookStarted, EventType::PlaybookCompleted]),
            sources: Some(vec!["executor".to_string()]),
        };

        let matching = Event::new(EventType::PlaybookStarted, EventSource::new("executor"));
        assert!(filter.matches(&matching));

        let wrong_type = Event::new(EventType::HostDown, EventSource::new("executor"));
        assert!(!filter.matches(&wrong_type));

        let wrong_source = Event::new(EventType::PlaybookStarted, EventSource::new("monitor"));
        assert!(!filter.matches(&wrong_source));
    }

    #[test]
    fn test_empty_bus() {
        let bus = EventBus::new();
        assert_eq!(bus.subscriber_count(), 0);

        // Publishing to an empty bus should not panic
        let event = Event::new(EventType::PlaybookStarted, EventSource::new("test"));
        bus.publish(&event);
    }
}
