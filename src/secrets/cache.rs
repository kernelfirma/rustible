//! Secret caching with TTL support.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::types::Secret;
use super::CacheStats;

/// Configuration for secret caching.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretCacheConfig {
    /// Enable caching
    pub enabled: bool,

    /// Default TTL for cached secrets
    #[serde(with = "humantime_serde")]
    pub default_ttl: Duration,

    /// Maximum number of secrets to cache
    pub max_size: usize,

    /// Whether to cache failed lookups (negative caching)
    pub cache_failures: bool,

    /// TTL for failed lookups
    #[serde(with = "humantime_serde")]
    pub failure_ttl: Duration,
}

impl Default for SecretCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_ttl: Duration::from_secs(300), // 5 minutes
            max_size: 1000,
            cache_failures: true,
            failure_ttl: Duration::from_secs(60), // 1 minute
        }
    }
}

impl SecretCacheConfig {
    /// Create a cache config with a custom TTL.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            default_ttl: ttl,
            ..Default::default()
        }
    }

    /// Disable caching.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Create an aggressive caching config (longer TTL, larger size).
    pub fn aggressive() -> Self {
        Self {
            enabled: true,
            default_ttl: Duration::from_secs(3600), // 1 hour
            max_size: 10000,
            cache_failures: true,
            failure_ttl: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// A cached secret entry.
#[derive(Clone)]
struct CacheEntry {
    /// The cached secret
    secret: Secret,
    /// When this entry was created
    created_at: Instant,
    /// TTL for this entry
    ttl: Duration,
    /// Number of times this entry has been accessed
    access_count: u64,
}

impl CacheEntry {
    fn new(secret: Secret, ttl: Duration) -> Self {
        Self {
            secret,
            created_at: Instant::now(),
            ttl,
            access_count: 0,
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }

    fn access(&mut self) -> &Secret {
        self.access_count += 1;
        &self.secret
    }
}

/// Secret cache implementation.
pub struct SecretCache {
    /// The cache entries
    entries: HashMap<String, CacheEntry>,

    /// Cache configuration
    config: SecretCacheConfig,

    /// Cache statistics
    hits: u64,
    misses: u64,
}

impl SecretCache {
    /// Create a new cache with the given configuration.
    pub fn new(config: SecretCacheConfig) -> Self {
        Self {
            entries: HashMap::new(),
            config,
            hits: 0,
            misses: 0,
        }
    }

    /// Get a secret from the cache.
    pub fn get(&self, path: &str) -> Option<Secret> {
        if !self.config.enabled {
            return None;
        }

        // Note: We can't increment access_count here without mutable access
        // The caller should use get_mut if they need to update stats
        self.entries.get(path).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some(entry.secret.clone())
            }
        })
    }

    /// Get a secret from the cache and update access statistics.
    pub fn get_mut(&mut self, path: &str) -> Option<Secret> {
        if !self.config.enabled {
            self.misses += 1;
            return None;
        }

        // Check for expired entry first
        if let Some(entry) = self.entries.get(path) {
            if entry.is_expired() {
                self.entries.remove(path);
                self.misses += 1;
                return None;
            }
        }

        // Get and update
        if let Some(entry) = self.entries.get_mut(path) {
            self.hits += 1;
            Some(entry.access().clone())
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert a secret into the cache.
    pub fn insert(&mut self, path: String, secret: Secret) {
        self.insert_with_ttl(path, secret, self.config.default_ttl);
    }

    /// Insert a secret with a custom TTL.
    pub fn insert_with_ttl(&mut self, path: String, secret: Secret, ttl: Duration) {
        if !self.config.enabled {
            return;
        }

        // Evict if at capacity
        if self.entries.len() >= self.config.max_size {
            self.evict_expired();

            // If still at capacity, evict least recently used
            if self.entries.len() >= self.config.max_size {
                self.evict_lru();
            }
        }

        self.entries.insert(path, CacheEntry::new(secret, ttl));
    }

    /// Invalidate a specific secret.
    pub fn invalidate(&mut self, path: &str) {
        self.entries.remove(path);
        tracing::debug!(path = %path, "Cache entry invalidated");
    }

    /// Invalidate all secrets matching a prefix.
    pub fn invalidate_prefix(&mut self, prefix: &str) {
        let to_remove: Vec<String> = self
            .entries
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();

        for key in &to_remove {
            self.entries.remove(key);
        }

        tracing::debug!(prefix = %prefix, count = to_remove.len(), "Cache entries invalidated by prefix");
    }

    /// Clear the entire cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        tracing::debug!("Cache cleared");
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let total = self.hits + self.misses;
        let hit_rate = if total > 0 {
            self.hits as f64 / total as f64
        } else {
            0.0
        };

        CacheStats {
            size: self.entries.len(),
            hits: self.hits,
            misses: self.misses,
            hit_rate,
        }
    }

    /// Get the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Evict all expired entries.
    fn evict_expired(&mut self) {
        let expired: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        for key in &expired {
            self.entries.remove(key);
        }

        if !expired.is_empty() {
            tracing::debug!(count = expired.len(), "Expired cache entries evicted");
        }
    }

    /// Evict the least recently used entry.
    fn evict_lru(&mut self) {
        // Find the entry with the lowest access count
        let lru = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.access_count)
            .map(|(k, _)| k.clone());

        if let Some(key) = lru {
            self.entries.remove(&key);
            tracing::debug!(path = %key, "LRU cache entry evicted");
        }
    }
}

impl Default for SecretCache {
    fn default() -> Self {
        Self::new(SecretCacheConfig::default())
    }
}

impl std::fmt::Debug for SecretCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretCache")
            .field("size", &self.entries.len())
            .field("hits", &self.hits)
            .field("misses", &self.misses)
            .field("config", &self.config)
            .finish()
    }
}

/// Duration serialization using humantime.
mod humantime_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&humantime::format_duration(*duration).to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        humantime::parse_duration(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::types::SecretValue;
    use std::collections::HashMap;

    fn create_test_secret(path: &str) -> Secret {
        let mut data = HashMap::new();
        data.insert("key".to_string(), SecretValue::String("value".to_string()));
        Secret::new(path, data)
    }

    #[test]
    fn test_cache_basic_operations() {
        let mut cache = SecretCache::default();

        let secret = create_test_secret("test/secret");
        cache.insert("test/secret".to_string(), secret.clone());

        assert_eq!(cache.len(), 1);

        let retrieved = cache.get("test/secret");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().path(), "test/secret");
    }

    #[test]
    fn test_cache_miss() {
        let cache = SecretCache::default();
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_cache_invalidation() {
        let mut cache = SecretCache::default();

        cache.insert(
            "test/secret1".to_string(),
            create_test_secret("test/secret1"),
        );
        cache.insert(
            "test/secret2".to_string(),
            create_test_secret("test/secret2"),
        );

        assert_eq!(cache.len(), 2);

        cache.invalidate("test/secret1");
        assert_eq!(cache.len(), 1);
        assert!(cache.get("test/secret1").is_none());
        assert!(cache.get("test/secret2").is_some());
    }

    #[test]
    fn test_cache_invalidate_prefix() {
        let mut cache = SecretCache::default();

        cache.insert(
            "prod/db/password".to_string(),
            create_test_secret("prod/db/password"),
        );
        cache.insert(
            "prod/api/key".to_string(),
            create_test_secret("prod/api/key"),
        );
        cache.insert(
            "dev/db/password".to_string(),
            create_test_secret("dev/db/password"),
        );

        assert_eq!(cache.len(), 3);

        cache.invalidate_prefix("prod/");
        assert_eq!(cache.len(), 1);
        assert!(cache.get("dev/db/password").is_some());
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = SecretCache::default();

        cache.insert("secret1".to_string(), create_test_secret("secret1"));
        cache.insert("secret2".to_string(), create_test_secret("secret2"));

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_stats() {
        let mut cache = SecretCache::default();

        cache.insert("test".to_string(), create_test_secret("test"));

        // Hit
        cache.get_mut("test");
        // Miss
        cache.get_mut("nonexistent");

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_cache_disabled() {
        let config = SecretCacheConfig::disabled();
        let mut cache = SecretCache::new(config);

        cache.insert("test".to_string(), create_test_secret("test"));
        assert!(cache.is_empty()); // Should not store when disabled
        assert!(cache.get("test").is_none());
    }

    #[test]
    fn test_cache_expiration() {
        let config = SecretCacheConfig {
            default_ttl: Duration::from_millis(1),
            ..Default::default()
        };
        let mut cache = SecretCache::new(config);

        cache.insert("test".to_string(), create_test_secret("test"));

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(10));

        assert!(cache.get("test").is_none());
    }
}
