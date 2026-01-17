//! Password Caching with Timeout
//!
//! This module provides secure password caching for privilege escalation
//! operations, with configurable TTL and automatic expiration.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{SecurityError, SecurityResult};

/// Configuration for the password cache
#[derive(Debug, Clone)]
pub struct PasswordCacheConfig {
    /// Default TTL for cached passwords
    pub default_ttl: Duration,
    /// Maximum TTL (even if longer is requested)
    pub max_ttl: Duration,
    /// Whether to cache passwords at all
    pub enabled: bool,
    /// Maximum number of cached entries
    pub max_entries: usize,
    /// Whether to clear password from memory when retrieved
    pub clear_on_retrieve: bool,
}

impl Default for PasswordCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl: Duration::from_secs(300), // 5 minutes
            max_ttl: Duration::from_secs(3600),    // 1 hour max
            enabled: true,
            max_entries: 100,
            clear_on_retrieve: false,
        }
    }
}

impl PasswordCacheConfig {
    /// Create a config for high-security environments
    pub fn high_security() -> Self {
        Self {
            default_ttl: Duration::from_secs(60),  // 1 minute
            max_ttl: Duration::from_secs(300),     // 5 minutes max
            enabled: true,
            max_entries: 50,
            clear_on_retrieve: true,
        }
    }

    /// Create a config with caching disabled
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }
}

/// A cached password entry
#[derive(Clone)]
pub struct CachedPassword {
    /// The password (stored as bytes for potential zeroing)
    password: Vec<u8>,
    /// When this entry was created
    created_at: Instant,
    /// When this entry expires
    expires_at: Instant,
    /// Number of times this password has been used
    use_count: u32,
    /// Host this password is for
    host: String,
    /// User this password is for
    user: String,
}

impl CachedPassword {
    /// Create a new cached password
    fn new(host: String, user: String, password: &str, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            password: password.as_bytes().to_vec(),
            created_at: now,
            expires_at: now + ttl,
            use_count: 0,
            host,
            user,
        }
    }

    /// Check if this entry is expired
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    /// Get the password as a string
    pub fn password(&self) -> Option<String> {
        String::from_utf8(self.password.clone()).ok()
    }

    /// Get remaining TTL
    pub fn remaining_ttl(&self) -> Duration {
        let now = Instant::now();
        if now >= self.expires_at {
            Duration::ZERO
        } else {
            self.expires_at - now
        }
    }

    /// Get age of this entry
    pub fn age(&self) -> Duration {
        Instant::now() - self.created_at
    }

    /// Clear the password from memory
    fn clear(&mut self) {
        for byte in &mut self.password {
            *byte = 0;
        }
        self.password.clear();
    }
}

impl Drop for CachedPassword {
    fn drop(&mut self) {
        self.clear();
    }
}

// Debug implementation that redacts the password
impl std::fmt::Debug for CachedPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedPassword")
            .field("host", &self.host)
            .field("user", &self.user)
            .field("password", &"[REDACTED]")
            .field("expires_at", &format!("{:?} remaining", self.remaining_ttl()))
            .field("use_count", &self.use_count)
            .finish()
    }
}

/// Key for password cache entries
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    host: String,
    user: String,
}

impl CacheKey {
    fn new(host: &str, user: &str) -> Self {
        Self {
            host: host.to_string(),
            user: user.to_string(),
        }
    }
}

/// Thread-safe password cache with TTL
pub struct PasswordCache {
    /// Cache configuration
    config: PasswordCacheConfig,
    /// Cached passwords (host+user -> password)
    entries: Arc<RwLock<HashMap<CacheKey, CachedPassword>>>,
    /// Statistics
    stats: Arc<RwLock<CacheStats>>,
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total cache hits
    pub hits: u64,
    /// Total cache misses
    pub misses: u64,
    /// Expired entries evicted
    pub expirations: u64,
    /// Manual clears
    pub clears: u64,
}

impl CacheStats {
    /// Get hit rate as a percentage
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

impl PasswordCache {
    /// Create a new password cache with default config
    pub fn new() -> Self {
        Self::with_config(PasswordCacheConfig::default())
    }

    /// Create a new password cache with custom config
    pub fn with_config(config: PasswordCacheConfig) -> Self {
        Self {
            config,
            entries: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(CacheStats::default())),
        }
    }

    /// Create a high-security cache
    pub fn high_security() -> Self {
        Self::with_config(PasswordCacheConfig::high_security())
    }

    /// Store a password in the cache
    pub fn store(&self, host: &str, user: &str, password: &str) {
        self.store_with_ttl(host, user, password, self.config.default_ttl);
    }

    /// Store a password with custom TTL
    pub fn store_with_ttl(&self, host: &str, user: &str, password: &str, ttl: Duration) {
        if !self.config.enabled {
            return;
        }

        // Cap TTL at max
        let ttl = std::cmp::min(ttl, self.config.max_ttl);

        let key = CacheKey::new(host, user);
        let entry = CachedPassword::new(host.to_string(), user.to_string(), password, ttl);

        let mut entries = self.entries.write();

        // Evict if at capacity
        if entries.len() >= self.config.max_entries {
            self.evict_expired_locked(&mut entries);

            // If still at capacity, remove oldest entry
            if entries.len() >= self.config.max_entries {
                if let Some(oldest_key) = entries
                    .iter()
                    .min_by_key(|(_, v)| v.created_at)
                    .map(|(k, _)| k.clone())
                {
                    entries.remove(&oldest_key);
                }
            }
        }

        entries.insert(key, entry);

        tracing::debug!(
            host = %host,
            user = %user,
            ttl_secs = %ttl.as_secs(),
            "Stored password in cache"
        );
    }

    /// Retrieve a password from the cache
    pub fn get(&self, host: &str, user: &str) -> SecurityResult<String> {
        if !self.config.enabled {
            return Err(SecurityError::PasswordExpired(format!(
                "Password cache disabled for {}@{}",
                user, host
            )));
        }

        let key = CacheKey::new(host, user);

        // First try with read lock
        {
            let entries = self.entries.read();
            if let Some(entry) = entries.get(&key) {
                if !entry.is_expired() {
                    self.stats.write().hits += 1;
                    if let Some(pwd) = entry.password() {
                        return Ok(pwd);
                    }
                }
            }
        }

        // Cache miss or expired
        self.stats.write().misses += 1;

        // If clear_on_retrieve, we need write lock anyway
        if self.config.clear_on_retrieve {
            let mut entries = self.entries.write();
            if let Some(mut entry) = entries.remove(&key) {
                if !entry.is_expired() {
                    if let Some(pwd) = entry.password() {
                        entry.clear();
                        return Ok(pwd);
                    }
                }
            }
        }

        Err(SecurityError::PasswordExpired(format!(
            "No valid cached password for {}@{}",
            user, host
        )))
    }

    /// Check if a password is cached (without retrieving)
    pub fn has(&self, host: &str, user: &str) -> bool {
        if !self.config.enabled {
            return false;
        }

        let key = CacheKey::new(host, user);
        let entries = self.entries.read();

        entries
            .get(&key)
            .map(|e| !e.is_expired())
            .unwrap_or(false)
    }

    /// Remove a password from the cache
    pub fn remove(&self, host: &str, user: &str) {
        let key = CacheKey::new(host, user);
        let mut entries = self.entries.write();

        if let Some(mut entry) = entries.remove(&key) {
            entry.clear();
            tracing::debug!(
                host = %host,
                user = %user,
                "Removed password from cache"
            );
        }
    }

    /// Clear all passwords for a host
    pub fn clear_host(&self, host: &str) {
        let mut entries = self.entries.write();

        let keys_to_remove: Vec<_> = entries
            .keys()
            .filter(|k| k.host == host)
            .cloned()
            .collect();

        for key in keys_to_remove {
            if let Some(mut entry) = entries.remove(&key) {
                entry.clear();
            }
        }

        self.stats.write().clears += 1;
        tracing::debug!(host = %host, "Cleared all passwords for host");
    }

    /// Clear all cached passwords
    pub fn clear_all(&self) {
        let mut entries = self.entries.write();

        for (_, mut entry) in entries.drain() {
            entry.clear();
        }

        self.stats.write().clears += 1;
        tracing::debug!("Cleared all cached passwords");
    }

    /// Evict expired entries
    pub fn evict_expired(&self) {
        let mut entries = self.entries.write();
        self.evict_expired_locked(&mut entries);
    }

    /// Internal eviction with lock already held
    fn evict_expired_locked(&self, entries: &mut HashMap<CacheKey, CachedPassword>) {
        let expired_keys: Vec<_> = entries
            .iter()
            .filter(|(_, v)| v.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        let count = expired_keys.len();

        for key in expired_keys {
            if let Some(mut entry) = entries.remove(&key) {
                entry.clear();
            }
        }

        if count > 0 {
            self.stats.write().expirations += count as u64;
            tracing::debug!(count = %count, "Evicted expired password entries");
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        self.stats.read().clone()
    }

    /// Get current entry count
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    /// Get information about cached entries (without passwords)
    pub fn entries_info(&self) -> Vec<PasswordEntryInfo> {
        let entries = self.entries.read();
        entries
            .values()
            .map(|e| PasswordEntryInfo {
                host: e.host.clone(),
                user: e.user.clone(),
                age: e.age(),
                remaining_ttl: e.remaining_ttl(),
                use_count: e.use_count,
                expired: e.is_expired(),
            })
            .collect()
    }
}

impl Default for PasswordCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PasswordCache {
    fn drop(&mut self) {
        self.clear_all();
    }
}

/// Information about a cached password entry (without the password)
#[derive(Debug, Clone)]
pub struct PasswordEntryInfo {
    /// Host this password is for
    pub host: String,
    /// User this password is for
    pub user: String,
    /// Age of the entry
    pub age: Duration,
    /// Remaining TTL
    pub remaining_ttl: Duration,
    /// Number of times used
    pub use_count: u32,
    /// Whether the entry is expired
    pub expired: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_password_cache_basic() {
        let cache = PasswordCache::new();

        cache.store("host1", "root", "secret123");

        assert!(cache.has("host1", "root"));
        assert!(!cache.has("host1", "admin"));
        assert!(!cache.has("host2", "root"));

        let pwd = cache.get("host1", "root").unwrap();
        assert_eq!(pwd, "secret123");
    }

    #[cfg_attr(tarpaulin, ignore)]
    #[test]
    fn test_password_cache_expiration() {
        let config = PasswordCacheConfig {
            default_ttl: Duration::from_millis(50),
            ..Default::default()
        };
        let cache = PasswordCache::with_config(config);

        cache.store("host1", "root", "secret");

        assert!(cache.has("host1", "root"));

        // Wait for expiration
        thread::sleep(Duration::from_millis(100));

        assert!(!cache.has("host1", "root"));
        assert!(cache.get("host1", "root").is_err());
    }

    #[test]
    fn test_password_cache_clear() {
        let cache = PasswordCache::new();

        cache.store("host1", "root", "secret1");
        cache.store("host1", "admin", "secret2");
        cache.store("host2", "root", "secret3");

        assert_eq!(cache.len(), 3);

        cache.clear_host("host1");

        assert_eq!(cache.len(), 1);
        assert!(!cache.has("host1", "root"));
        assert!(!cache.has("host1", "admin"));
        assert!(cache.has("host2", "root"));
    }

    #[test]
    fn test_password_cache_stats() {
        let cache = PasswordCache::new();

        cache.store("host1", "root", "secret");

        // Hit
        let _ = cache.get("host1", "root");
        // Miss
        let _ = cache.get("host1", "admin");
        let _ = cache.get("host2", "root");

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 2);
    }

    #[test]
    fn test_password_debug_redaction() {
        let pwd = CachedPassword::new(
            "host1".to_string(),
            "root".to_string(),
            "super_secret",
            Duration::from_secs(300),
        );

        let debug_output = format!("{:?}", pwd);
        assert!(debug_output.contains("[REDACTED]"));
        assert!(!debug_output.contains("super_secret"));
    }

    #[test]
    fn test_disabled_cache() {
        let cache = PasswordCache::with_config(PasswordCacheConfig::disabled());

        cache.store("host1", "root", "secret");

        assert!(!cache.has("host1", "root"));
        assert!(cache.get("host1", "root").is_err());
    }

    #[test]
    fn test_max_entries_eviction() {
        let config = PasswordCacheConfig {
            max_entries: 3,
            ..Default::default()
        };
        let cache = PasswordCache::with_config(config);

        cache.store("host1", "root", "secret1");
        cache.store("host2", "root", "secret2");
        cache.store("host3", "root", "secret3");

        assert_eq!(cache.len(), 3);

        // Adding 4th should evict oldest
        cache.store("host4", "root", "secret4");

        assert_eq!(cache.len(), 3);
    }
}
