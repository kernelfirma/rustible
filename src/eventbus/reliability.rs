//! Reliability primitives for the event bus system.
//!
//! Provides deduplication to prevent processing the same event twice,
//! retry policies for transient failures, and a dead-letter queue for
//! events that could not be successfully processed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

use super::event::Event;

/// Deduplicator that tracks seen event IDs to prevent duplicate processing.
pub struct Deduplicator {
    seen: HashMap<String, DateTime<Utc>>,
    max_entries: usize,
}

impl Deduplicator {
    /// Create a new deduplicator with a default capacity of 10,000 entries.
    pub fn new() -> Self {
        Self {
            seen: HashMap::new(),
            max_entries: 10_000,
        }
    }

    /// Create a new deduplicator with the given maximum capacity.
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            seen: HashMap::new(),
            max_entries,
        }
    }

    /// Check whether an event ID is new (not yet seen).
    ///
    /// Returns `true` if the event ID has not been seen before and records it.
    /// Returns `false` if it is a duplicate.
    pub fn check(&mut self, event_id: &str) -> bool {
        if self.seen.contains_key(event_id) {
            return false;
        }

        // Evict oldest entries if at capacity
        if self.seen.len() >= self.max_entries {
            // Find and remove the oldest entry
            if let Some(oldest_key) = self
                .seen
                .iter()
                .min_by_key(|(_, ts)| *ts)
                .map(|(k, _)| k.clone())
            {
                self.seen.remove(&oldest_key);
            }
        }

        self.seen.insert(event_id.to_string(), Utc::now());
        true
    }

    /// Returns the number of tracked event IDs.
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// Returns true if no event IDs have been tracked.
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

impl Default for Deduplicator {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for retry behaviour when action execution fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base backoff duration in milliseconds between retries
    pub backoff_ms: u64,
}

impl RetryPolicy {
    /// Create a new retry policy.
    pub fn new(max_retries: u32, backoff_ms: u64) -> Self {
        Self {
            max_retries,
            backoff_ms,
        }
    }

    /// Calculate the delay for a given attempt number using exponential backoff.
    pub fn delay_for_attempt(&self, attempt: u32) -> u64 {
        self.backoff_ms * 2u64.saturating_pow(attempt)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            backoff_ms: 1000,
        }
    }
}

/// An entry in the dead-letter queue, representing a failed event processing attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterEntry {
    /// Unique identifier for this dead-letter entry
    pub id: String,
    /// The event that failed processing
    pub event: Event,
    /// Description of the error that caused the failure
    pub error_message: String,
    /// Timestamp when the failure occurred
    pub failed_at: DateTime<Utc>,
    /// Number of times this event has been retried
    pub retry_count: u32,
}

/// A queue for events that failed processing after all retry attempts.
pub struct DeadLetterQueue {
    entries: VecDeque<DeadLetterEntry>,
}

impl DeadLetterQueue {
    /// Create a new empty dead-letter queue.
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    /// Push a failed event entry onto the dead-letter queue.
    pub fn push(&mut self, entry: DeadLetterEntry) {
        self.entries.push_back(entry);
    }

    /// List all entries currently in the dead-letter queue.
    pub fn list(&self) -> &VecDeque<DeadLetterEntry> {
        &self.entries
    }

    /// Remove and return the dead-letter entry with the given ID for retry.
    ///
    /// Returns `None` if no entry with that ID exists.
    pub fn retry(&mut self, id: &str) -> Option<DeadLetterEntry> {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            self.entries.remove(pos)
        } else {
            None
        }
    }

    /// Returns the number of entries in the dead-letter queue.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the dead-letter queue is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for DeadLetterQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventbus::event::{EventSource, EventType};

    #[test]
    fn test_deduplicator_new_event() {
        let mut dedup = Deduplicator::new();
        assert!(dedup.check("event-1"));
        assert!(!dedup.check("event-1")); // duplicate
        assert!(dedup.check("event-2")); // new
        assert_eq!(dedup.len(), 2);
    }

    #[test]
    fn test_deduplicator_capacity_eviction() {
        let mut dedup = Deduplicator::with_capacity(3);
        assert!(dedup.check("a"));
        assert!(dedup.check("b"));
        assert!(dedup.check("c"));
        assert_eq!(dedup.len(), 3);

        // Adding a 4th event should evict the oldest
        assert!(dedup.check("d"));
        assert_eq!(dedup.len(), 3);
    }

    #[test]
    fn test_dead_letter_queue_push_and_list() {
        let mut dlq = DeadLetterQueue::new();
        assert!(dlq.is_empty());

        let event = Event::new(EventType::TaskFailed, EventSource::new("executor"));
        let entry = DeadLetterEntry {
            id: "dlq-1".to_string(),
            event,
            error_message: "Action handler panicked".to_string(),
            failed_at: Utc::now(),
            retry_count: 3,
        };

        dlq.push(entry);
        assert_eq!(dlq.len(), 1);
        assert_eq!(dlq.list()[0].id, "dlq-1");
    }

    #[test]
    fn test_dead_letter_queue_retry() {
        let mut dlq = DeadLetterQueue::new();

        let event = Event::new(EventType::HostDown, EventSource::new("monitor"));
        dlq.push(DeadLetterEntry {
            id: "dlq-retry-1".to_string(),
            event,
            error_message: "Timeout".to_string(),
            failed_at: Utc::now(),
            retry_count: 1,
        });

        // Retry with correct ID
        let retried = dlq.retry("dlq-retry-1");
        assert!(retried.is_some());
        assert_eq!(retried.unwrap().id, "dlq-retry-1");
        assert!(dlq.is_empty());

        // Retry with non-existent ID
        let missing = dlq.retry("non-existent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_retry_policy_backoff() {
        let policy = RetryPolicy::new(5, 100);
        assert_eq!(policy.delay_for_attempt(0), 100); // 100 * 2^0
        assert_eq!(policy.delay_for_attempt(1), 200); // 100 * 2^1
        assert_eq!(policy.delay_for_attempt(2), 400); // 100 * 2^2
        assert_eq!(policy.delay_for_attempt(3), 800); // 100 * 2^3
    }

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.backoff_ms, 1000);
    }
}
