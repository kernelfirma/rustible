//! Fact Caching
//!
//! This module provides caching for gathered facts from hosts.
//! Facts gathering is one of the most expensive operations (3-5s per host),
//! so caching provides significant performance improvements.

use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::{Cache, CacheConfig, CacheMetrics, CacheType};

/// Cached facts for a single host
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CachedFacts {
    /// The gathered facts
    pub facts: IndexMap<String, JsonValue>,
    /// Hostname
    pub hostname: String,
    /// When facts were gathered
    #[serde(skip)]
    pub gathered_at: Option<Instant>,
    /// Which fact subsets are included
    pub subsets: Vec<String>,
    /// Operating system family
    pub os_family: Option<String>,
    /// Distribution name
    pub distribution: Option<String>,
    /// Distribution version
    pub distribution_version: Option<String>,
}

impl CachedFacts {
    /// Create new cached facts
    pub fn new(hostname: impl Into<String>, facts: IndexMap<String, JsonValue>) -> Self {
        let hostname = hostname.into();
        let os_family = facts
            .get("ansible_os_family")
            .and_then(|v| v.as_str())
            .map(String::from);
        let distribution = facts
            .get("ansible_distribution")
            .and_then(|v| v.as_str())
            .map(String::from);
        let distribution_version = facts
            .get("ansible_distribution_version")
            .and_then(|v| v.as_str())
            .map(String::from);

        Self {
            facts,
            hostname,
            gathered_at: Some(Instant::now()),
            subsets: vec!["all".to_string()],
            os_family,
            distribution,
            distribution_version,
        }
    }

    /// Create with specific subsets
    pub fn with_subsets(mut self, subsets: Vec<String>) -> Self {
        self.subsets = subsets;
        self
    }

    /// Get a fact value
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.facts.get(key)
    }

    /// Get a fact value as a string
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.facts.get(key).and_then(|v| v.as_str())
    }

    /// Check if this cache covers the requested subsets
    pub fn covers_subsets(&self, requested: &[String]) -> bool {
        if self.subsets.contains(&"all".to_string()) {
            return true;
        }
        requested.iter().all(|s| self.subsets.contains(s))
    }

    /// Estimate memory size of the cached facts
    pub fn size_bytes(&self) -> usize {
        // Rough estimation: hostname + JSON serialized facts
        self.hostname.len()
            + serde_json::to_string(&self.facts)
                .map(|s| s.len())
                .unwrap_or(1000) // Default estimate if serialization fails
    }
}

/// Fact cache for storing gathered host facts
pub struct FactCache {
    pub(crate) cache: Cache<String, CachedFacts>,
    /// Map of IP to hostname for lookups
    ip_to_hostname: dashmap::DashMap<String, String>,
    /// Configuration
    config: FactCacheConfig,
}

/// Configuration specific to fact caching
#[derive(Debug, Clone)]
pub struct FactCacheConfig {
    /// TTL for fact cache entries (facts change infrequently)
    pub fact_ttl: Duration,
    /// Whether to cache fact subsets separately
    pub cache_subsets_separately: bool,
    /// Minimum facts to consider valid
    pub min_facts_count: usize,
}

impl Default for FactCacheConfig {
    fn default() -> Self {
        Self {
            fact_ttl: Duration::from_secs(600), // 10 minutes
            cache_subsets_separately: true,
            min_facts_count: 5, // At least 5 facts to be valid
        }
    }
}

impl FactCache {
    /// Create a new fact cache
    pub fn new(config: CacheConfig) -> Self {
        // Use the config's default_ttl as the fact_ttl to respect user configuration
        let fact_config = FactCacheConfig {
            fact_ttl: config.default_ttl,
            ..FactCacheConfig::default()
        };
        Self {
            cache: Cache::new(CacheType::Facts, config),
            ip_to_hostname: dashmap::DashMap::new(),
            config: fact_config,
        }
    }

    /// Create with custom fact cache configuration
    pub fn with_fact_config(config: CacheConfig, fact_config: FactCacheConfig) -> Self {
        Self {
            cache: Cache::new(CacheType::Facts, config),
            ip_to_hostname: dashmap::DashMap::new(),
            config: fact_config,
        }
    }

    /// Get cached facts for a host
    pub fn get(&self, hostname: &str) -> Option<CachedFacts> {
        self.cache.get(&hostname.to_string())
    }

    /// Get cached facts by IP address
    pub fn get_by_ip(&self, ip: &str) -> Option<CachedFacts> {
        self.ip_to_hostname
            .get(ip)
            .and_then(|hostname| self.cache.get(&hostname))
    }

    /// Get facts for a host with specific subsets
    pub fn get_with_subsets(&self, hostname: &str, subsets: &[String]) -> Option<CachedFacts> {
        self.get(hostname)
            .filter(|facts| facts.covers_subsets(subsets))
    }

    /// Store facts for a host
    pub fn insert(&self, hostname: &str, facts: CachedFacts) {
        let size = facts.size_bytes();

        // Store IP to hostname mapping if available
        if let Some(ip) = facts
            .facts
            .get("ansible_default_ipv4")
            .and_then(|v| v.get("address"))
            .and_then(|v| v.as_str())
        {
            self.ip_to_hostname
                .insert(ip.to_string(), hostname.to_string());
        }

        self.cache.insert_with_ttl(
            hostname.to_string(),
            facts,
            Some(self.config.fact_ttl),
            size,
        );
    }

    /// Store facts from a raw IndexMap
    pub fn insert_raw(&self, hostname: &str, facts: IndexMap<String, JsonValue>) {
        let cached = CachedFacts::new(hostname, facts);
        self.insert(hostname, cached);
    }

    /// Store facts with specific subsets
    pub fn insert_with_subsets(
        &self,
        hostname: &str,
        facts: IndexMap<String, JsonValue>,
        subsets: Vec<String>,
    ) {
        let cached = CachedFacts::new(hostname, facts).with_subsets(subsets);
        self.insert(hostname, cached);
    }

    /// Invalidate cached facts for a host
    pub fn invalidate_host(&self, hostname: &str) {
        self.cache.remove(&hostname.to_string());

        // Also remove from IP mapping
        let ips_to_remove: Vec<_> = self
            .ip_to_hostname
            .iter()
            .filter(|entry| entry.value() == hostname)
            .map(|entry| entry.key().clone())
            .collect();

        for ip in ips_to_remove {
            self.ip_to_hostname.remove(&ip);
        }
    }

    /// Invalidate all cached facts
    pub fn clear(&self) {
        self.cache.clear();
        self.ip_to_hostname.clear();
    }

    /// Get the number of cached fact entries
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Get cache metrics
    pub fn metrics(&self) -> Arc<CacheMetrics> {
        self.cache.metrics()
    }

    /// Cleanup expired entries
    pub fn cleanup_expired(&self) -> usize {
        self.cache.cleanup_expired()
    }

    /// Get all cached hostnames
    pub fn hostnames(&self) -> Vec<String> {
        self.cache.entries.iter().map(|e| e.key().clone()).collect()
    }

    /// Merge additional facts into existing cache entry
    pub fn merge_facts(&self, hostname: &str, additional_facts: IndexMap<String, JsonValue>) {
        if let Some(mut existing) = self.get(hostname) {
            for (key, value) in additional_facts {
                existing.facts.insert(key, value);
            }
            self.insert(hostname, existing);
        } else {
            self.insert_raw(hostname, additional_facts);
        }
    }

    /// Check if facts need to be refreshed based on age
    pub fn needs_refresh(&self, hostname: &str, max_age: Duration) -> bool {
        match self.get(hostname) {
            Some(facts) => facts
                .gathered_at
                .map(|t| t.elapsed() > max_age)
                .unwrap_or(true),
            None => true,
        }
    }

    /// Get a list of hosts that need fact refresh
    pub fn hosts_needing_refresh(&self, max_age: Duration) -> Vec<String> {
        self.cache
            .entries
            .iter()
            .filter(|entry| {
                entry
                    .value()
                    .value
                    .gathered_at
                    .map(|t| t.elapsed() > max_age)
                    .unwrap_or(true)
            })
            .map(|entry| entry.key().clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_facts() -> IndexMap<String, JsonValue> {
        let mut facts = IndexMap::new();
        facts.insert(
            "ansible_os_family".to_string(),
            JsonValue::String("Debian".to_string()),
        );
        facts.insert(
            "ansible_distribution".to_string(),
            JsonValue::String("Ubuntu".to_string()),
        );
        facts.insert(
            "ansible_distribution_version".to_string(),
            JsonValue::String("22.04".to_string()),
        );
        facts.insert(
            "ansible_hostname".to_string(),
            JsonValue::String("test-host".to_string()),
        );
        facts.insert(
            "ansible_fqdn".to_string(),
            JsonValue::String("test-host.example.com".to_string()),
        );
        facts
    }

    #[test]
    fn test_fact_cache_basic() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());

        let cached = cache.get("host1").unwrap();
        assert_eq!(cached.os_family, Some("Debian".to_string()));
        assert_eq!(cached.distribution, Some("Ubuntu".to_string()));
    }

    #[test]
    fn test_fact_cache_subsets() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_with_subsets(
            "host1",
            sample_facts(),
            vec!["network".to_string(), "hardware".to_string()],
        );

        let cached = cache.get("host1").unwrap();
        assert!(cached.covers_subsets(&["network".to_string()]));
        assert!(!cached.covers_subsets(&["all".to_string()]));
    }

    #[test]
    fn test_fact_cache_invalidation() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());
        cache.insert_raw("host2", sample_facts());

        cache.invalidate_host("host1");

        assert!(cache.get("host1").is_none());
        assert!(cache.get("host2").is_some());
    }

    #[test]
    fn test_fact_cache_merge() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());

        let mut additional = IndexMap::new();
        additional.insert(
            "custom_fact".to_string(),
            JsonValue::String("custom_value".to_string()),
        );

        cache.merge_facts("host1", additional);

        let cached = cache.get("host1").unwrap();
        assert_eq!(cached.get_str("custom_fact"), Some("custom_value"));
        assert_eq!(cached.os_family, Some("Debian".to_string())); // Original facts preserved
    }

    #[test]
    fn test_fact_cache_needs_refresh() {
        let cache = FactCache::new(CacheConfig::default());

        cache.insert_raw("host1", sample_facts());

        // Should not need refresh immediately
        assert!(!cache.needs_refresh("host1", Duration::from_secs(60)));

        // Unknown host should need refresh
        assert!(cache.needs_refresh("unknown", Duration::from_secs(60)));
    }
}
