//! Configuration for the event bus system.

use serde::{Deserialize, Serialize};

use super::reliability::RetryPolicy;

/// Configuration for the event bus and reactor engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBusConfig {
    /// Maximum number of subscribers allowed on the bus
    pub max_subscribers: usize,
    /// Whether event deduplication is enabled
    pub enable_dedup: bool,
    /// Retry policy for failed actions
    pub retry_policy: RetryPolicy,
    /// Whether the dead-letter queue is enabled
    pub dead_letter_enabled: bool,
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self {
            max_subscribers: 64,
            enable_dedup: true,
            retry_policy: RetryPolicy::default(),
            dead_letter_enabled: true,
        }
    }
}

impl EventBusConfig {
    /// Create a minimal configuration suitable for testing.
    pub fn minimal() -> Self {
        Self {
            max_subscribers: 8,
            enable_dedup: false,
            retry_policy: RetryPolicy::new(1, 100),
            dead_letter_enabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EventBusConfig::default();
        assert_eq!(config.max_subscribers, 64);
        assert!(config.enable_dedup);
        assert_eq!(config.retry_policy.max_retries, 3);
        assert!(config.dead_letter_enabled);
    }

    #[test]
    fn test_minimal_config() {
        let config = EventBusConfig::minimal();
        assert_eq!(config.max_subscribers, 8);
        assert!(!config.enable_dedup);
        assert_eq!(config.retry_policy.max_retries, 1);
        assert!(!config.dead_letter_enabled);
    }
}
