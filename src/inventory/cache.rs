//! Inventory Caching System for Rustible
//!
//! This module provides specialized inventory caching with features including:
//! - TTL-based expiration with automatic cleanup
//! - File dependency tracking for automatic invalidation
//! - Persistent cache storage to disk
//! - Cache statistics and monitoring
//! - Thread-safe concurrent access
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::inventory::Inventory;
//! use rustible::inventory::cache::{InventoryCache, InventoryCacheConfig};
//! use std::time::Duration;
//! # let inventory = Inventory::new();
//!
//! // Create a cache with 5-minute TTL
//! let config = InventoryCacheConfig::default()
//!     .with_ttl(Duration::from_secs(300));
//! let cache = InventoryCache::new(config);
//!
//! // Cache an inventory
//! cache.set("my-inventory", inventory, None).await;
//!
//! // Retrieve from cache
//! if let Some(cached) = cache.get("my-inventory").await {
//!     // Use cached inventory
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{parse_json_inventory_from_value, Inventory, InventoryResult};

// ============================================================================
// Cache Configuration
// ============================================================================

/// Configuration for the inventory cache
#[derive(Debug, Clone)]
pub struct InventoryCacheConfig {
    /// Default TTL for cache entries
    pub default_ttl: Duration,
    /// Maximum number of cached inventories
    pub max_entries: usize,
    /// Enable file dependency tracking
    pub track_file_dependencies: bool,
    /// Enable persistent storage to disk
    pub persistent_storage: bool,
    /// Path for persistent cache storage
    pub cache_dir: Option<PathBuf>,
    /// Enable cache metrics collection
    pub enable_metrics: bool,
    /// Interval for automatic cleanup of expired entries
    pub cleanup_interval: Duration,
}

impl Default for InventoryCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl: Duration::from_secs(300), // 5 minutes
            max_entries: 100,
            track_file_dependencies: true,
            persistent_storage: false,
            cache_dir: None,
            enable_metrics: true,
            cleanup_interval: Duration::from_secs(60),
        }
    }
}

impl InventoryCacheConfig {
    /// Create a new configuration with specified TTL
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = ttl;
        self
    }

    /// Enable persistent storage at the specified directory
    pub fn with_persistence(mut self, cache_dir: PathBuf) -> Self {
        self.persistent_storage = true;
        self.cache_dir = Some(cache_dir);
        self
    }

    /// Set maximum number of entries
    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    /// Disable file dependency tracking
    pub fn without_dependency_tracking(mut self) -> Self {
        self.track_file_dependencies = false;
        self
    }

    /// Create a configuration optimized for development
    pub fn development() -> Self {
        Self {
            default_ttl: Duration::from_secs(60),
            max_entries: 50,
            track_file_dependencies: true,
            persistent_storage: false,
            cache_dir: None,
            enable_metrics: true,
            cleanup_interval: Duration::from_secs(30),
        }
    }

    /// Create a configuration optimized for production
    pub fn production() -> Self {
        Self {
            default_ttl: Duration::from_secs(600),
            max_entries: 500,
            track_file_dependencies: true,
            persistent_storage: true,
            cache_dir: Some(PathBuf::from("/var/cache/rustible/inventory")),
            enable_metrics: true,
            cleanup_interval: Duration::from_secs(120),
        }
    }

    /// Create a disabled cache configuration
    pub fn disabled() -> Self {
        Self {
            default_ttl: Duration::ZERO,
            max_entries: 0,
            track_file_dependencies: false,
            persistent_storage: false,
            cache_dir: None,
            enable_metrics: false,
            cleanup_interval: Duration::from_secs(3600),
        }
    }
}

// ============================================================================
// Cache Entry
// ============================================================================

/// A cached inventory entry with metadata
#[derive(Debug, Clone)]
pub struct InventoryCacheEntry {
    /// The cached inventory
    pub inventory: Inventory,
    /// When this entry was created
    pub created_at: Instant,
    /// When this entry expires
    pub expires_at: Option<Instant>,
    /// Source file paths that this inventory depends on
    pub source_files: Vec<FileDependency>,
    /// Number of times this entry has been accessed
    pub access_count: u64,
    /// Last access time
    pub last_accessed: Instant,
    /// Estimated size in bytes
    pub size_bytes: usize,
}

impl InventoryCacheEntry {
    /// Create a new cache entry
    pub fn new(inventory: Inventory, ttl: Option<Duration>) -> Self {
        let now = Instant::now();
        let size = estimate_inventory_size(&inventory);

        Self {
            inventory,
            created_at: now,
            expires_at: ttl.map(|d| now + d),
            source_files: Vec::new(),
            access_count: 0,
            last_accessed: now,
            size_bytes: size,
        }
    }

    /// Create an entry with file dependencies
    pub fn with_dependencies(mut self, files: Vec<PathBuf>) -> Self {
        self.source_files = files
            .into_iter()
            .filter_map(|path| FileDependency::from_path(&path))
            .collect();
        self
    }

    /// Check if this entry has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Instant::now() >= expires_at
        } else {
            false
        }
    }

    /// Check if any file dependencies have changed
    pub fn is_dependency_invalidated(&self) -> bool {
        self.source_files.iter().any(|dep| dep.is_modified())
    }

    /// Check if this entry is still valid
    pub fn is_valid(&self) -> bool {
        !self.is_expired() && !self.is_dependency_invalidated()
    }

    /// Record an access to this entry
    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_accessed = Instant::now();
    }

    /// Get the age of this entry
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get time until expiration (None if already expired or no TTL)
    pub fn time_to_live(&self) -> Option<Duration> {
        self.expires_at.and_then(|expires_at| {
            let now = Instant::now();
            if now < expires_at {
                Some(expires_at - now)
            } else {
                None
            }
        })
    }
}

/// A file dependency for cache invalidation
#[derive(Debug, Clone)]
pub struct FileDependency {
    /// Path to the file
    pub path: PathBuf,
    /// Modification time when cached
    pub modified_at: SystemTime,
    /// File size when cached
    pub size: u64,
}

impl FileDependency {
    /// Create a file dependency from a path
    pub fn from_path(path: &Path) -> Option<Self> {
        std::fs::metadata(path).ok().and_then(|metadata| {
            metadata.modified().ok().map(|modified_at| Self {
                path: path.to_path_buf(),
                modified_at,
                size: metadata.len(),
            })
        })
    }

    /// Check if the file has been modified since caching
    pub fn is_modified(&self) -> bool {
        std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .map(|current_mtime| current_mtime != self.modified_at)
            .unwrap_or(true) // If we can't read the file, assume it's modified
    }
}

// ============================================================================
// Cache Metrics
// ============================================================================

/// Metrics for monitoring cache performance
#[derive(Debug, Default)]
pub struct InventoryCacheMetrics {
    /// Number of cache hits
    pub hits: AtomicU64,
    /// Number of cache misses
    pub misses: AtomicU64,
    /// Number of evictions
    pub evictions: AtomicU64,
    /// Number of invalidations
    pub invalidations: AtomicU64,
    /// Current number of entries
    pub entries: AtomicUsize,
    /// Total memory usage in bytes
    pub memory_bytes: AtomicUsize,
}

impl InventoryCacheMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a cache hit
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a cache miss
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an eviction
    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an invalidation
    pub fn record_invalidation(&self) {
        self.invalidations.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the cache hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed) as f64;
        let misses = self.misses.load(Ordering::Relaxed) as f64;
        let total = hits + misses;
        if total > 0.0 {
            hits / total
        } else {
            0.0
        }
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
        self.invalidations.store(0, Ordering::Relaxed);
    }

    /// Get a snapshot of current statistics
    pub fn snapshot(&self) -> CacheStatsSnapshot {
        CacheStatsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            invalidations: self.invalidations.load(Ordering::Relaxed),
            entries: self.entries.load(Ordering::Relaxed),
            memory_bytes: self.memory_bytes.load(Ordering::Relaxed),
            hit_rate: self.hit_rate(),
        }
    }
}

/// A snapshot of cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStatsSnapshot {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of evictions
    pub evictions: u64,
    /// Number of invalidations
    pub invalidations: u64,
    /// Current number of entries
    pub entries: usize,
    /// Total memory usage in bytes
    pub memory_bytes: usize,
    /// Cache hit rate
    pub hit_rate: f64,
}

impl std::fmt::Display for CacheStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Hits: {}, Misses: {}, Hit Rate: {:.1}%, Entries: {}, Memory: {} KB",
            self.hits,
            self.misses,
            self.hit_rate * 100.0,
            self.entries,
            self.memory_bytes / 1024
        )
    }
}

// ============================================================================
// Inventory Cache
// ============================================================================

/// Thread-safe inventory cache with TTL and dependency tracking
pub struct InventoryCache {
    /// Cached entries
    entries: RwLock<HashMap<String, InventoryCacheEntry>>,
    /// Configuration
    config: InventoryCacheConfig,
    /// Metrics
    metrics: Arc<InventoryCacheMetrics>,
}

impl InventoryCache {
    /// Create a new inventory cache with the given configuration
    pub fn new(config: InventoryCacheConfig) -> Self {
        Self {
            entries: RwLock::new(HashMap::with_capacity(config.max_entries.min(100))),
            metrics: Arc::new(InventoryCacheMetrics::new()),
            config,
        }
    }

    /// Create a cache with default configuration
    pub fn with_default_config() -> Self {
        Self::new(InventoryCacheConfig::default())
    }

    /// Get an inventory from the cache
    pub async fn get(&self, key: &str) -> Option<Inventory> {
        if self.config.max_entries == 0 {
            self.metrics.record_miss();
            return None;
        }

        let mut entries = self.entries.write().await;

        if let Some(entry) = entries.get_mut(key) {
            if entry.is_valid() {
                entry.record_access();
                if self.config.enable_metrics {
                    self.metrics.record_hit();
                }
                return Some(entry.inventory.clone());
            }
            // Entry is invalid, remove it
            let removed = entries.remove(key);
            if let Some(removed_entry) = removed {
                self.metrics.memory_bytes.fetch_sub(
                    removed_entry
                        .size_bytes
                        .min(self.metrics.memory_bytes.load(Ordering::Relaxed)),
                    Ordering::Relaxed,
                );
            }
            self.metrics.entries.store(entries.len(), Ordering::Relaxed);
            if self.config.enable_metrics {
                self.metrics.record_miss();
                self.metrics.record_eviction();
            }
        } else if self.config.enable_metrics {
            self.metrics.record_miss();
        }

        None
    }

    /// Store an inventory in the cache
    pub async fn set(&self, key: &str, inventory: Inventory, ttl: Option<Duration>) {
        if self.config.max_entries == 0 {
            return;
        }

        let mut entries = self.entries.write().await;

        // Evict entries if we're at capacity
        if entries.len() >= self.config.max_entries {
            self.evict_lru_entry(&mut entries);
        }

        let entry = InventoryCacheEntry::new(inventory, ttl.or(Some(self.config.default_ttl)));
        let size = entry.size_bytes;

        entries.insert(key.to_string(), entry);
        self.metrics.entries.store(entries.len(), Ordering::Relaxed);
        self.metrics.memory_bytes.fetch_add(size, Ordering::Relaxed);

        // Persist to disk if configured
        if self.config.persistent_storage {
            if let Some(cache_dir) = &self.config.cache_dir {
                if let Some(entry) = entries.get(key) {
                    let _ = self.persist_entry(cache_dir, key, &entry.inventory).await;
                }
            }
        }
    }

    /// Store an inventory with file dependencies
    pub async fn set_with_dependencies(
        &self,
        key: &str,
        inventory: Inventory,
        source_files: Vec<PathBuf>,
        ttl: Option<Duration>,
    ) {
        if self.config.max_entries == 0 {
            return;
        }

        let mut entries = self.entries.write().await;

        // Evict entries if we're at capacity
        if entries.len() >= self.config.max_entries {
            self.evict_lru_entry(&mut entries);
        }

        let entry = InventoryCacheEntry::new(inventory, ttl.or(Some(self.config.default_ttl)))
            .with_dependencies(source_files);
        let size = entry.size_bytes;

        entries.insert(key.to_string(), entry);
        self.metrics.entries.store(entries.len(), Ordering::Relaxed);
        self.metrics.memory_bytes.fetch_add(size, Ordering::Relaxed);
    }

    /// Get or compute an inventory
    pub async fn get_or_compute<F, Fut>(
        &self,
        key: &str,
        compute: F,
        ttl: Option<Duration>,
    ) -> InventoryResult<Inventory>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = InventoryResult<Inventory>>,
    {
        // Check cache first
        if let Some(inventory) = self.get(key).await {
            return Ok(inventory);
        }

        // Compute and cache
        let inventory = compute().await?;
        self.set(key, inventory.clone(), ttl).await;
        Ok(inventory)
    }

    /// Invalidate a cache entry
    pub async fn invalidate(&self, key: &str) {
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.remove(key) {
            self.metrics.memory_bytes.fetch_sub(
                entry
                    .size_bytes
                    .min(self.metrics.memory_bytes.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
            self.metrics.entries.store(entries.len(), Ordering::Relaxed);
            if self.config.enable_metrics {
                self.metrics.record_invalidation();
            }
        }

        // Remove from persistent storage if configured
        if self.config.persistent_storage {
            if let Some(cache_dir) = &self.config.cache_dir {
                let cache_file = cache_dir.join(format!("{}.json", key));
                let _ = tokio::fs::remove_file(cache_file).await;
            }
        }
    }

    /// Invalidate all entries with a matching prefix
    pub async fn invalidate_prefix(&self, prefix: &str) {
        let mut entries = self.entries.write().await;
        let keys_to_remove: Vec<String> = entries
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();

        for key in keys_to_remove {
            if let Some(entry) = entries.remove(&key) {
                self.metrics.memory_bytes.fetch_sub(
                    entry
                        .size_bytes
                        .min(self.metrics.memory_bytes.load(Ordering::Relaxed)),
                    Ordering::Relaxed,
                );
                if self.config.enable_metrics {
                    self.metrics.record_invalidation();
                }
            }
        }
        self.metrics.entries.store(entries.len(), Ordering::Relaxed);
    }

    /// Clear all cache entries
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        let count = entries.len();
        entries.clear();
        self.metrics.entries.store(0, Ordering::Relaxed);
        self.metrics.memory_bytes.store(0, Ordering::Relaxed);

        if self.config.enable_metrics {
            for _ in 0..count {
                self.metrics.record_invalidation();
            }
        }

        // Clear persistent storage if configured
        if self.config.persistent_storage {
            if let Some(cache_dir) = &self.config.cache_dir {
                if let Ok(mut read_dir) = tokio::fs::read_dir(cache_dir).await {
                    while let Ok(Some(entry)) = read_dir.next_entry().await {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    }

    /// Clean up expired entries
    pub async fn cleanup_expired(&self) -> usize {
        let mut entries = self.entries.write().await;
        let mut removed = 0;

        let keys_to_remove: Vec<String> = entries
            .iter()
            .filter(|(_, entry)| !entry.is_valid())
            .map(|(key, _)| key.clone())
            .collect();

        for key in keys_to_remove {
            if let Some(entry) = entries.remove(&key) {
                self.metrics.memory_bytes.fetch_sub(
                    entry
                        .size_bytes
                        .min(self.metrics.memory_bytes.load(Ordering::Relaxed)),
                    Ordering::Relaxed,
                );
                removed += 1;
                if self.config.enable_metrics {
                    self.metrics.record_eviction();
                }
            }
        }

        self.metrics.entries.store(entries.len(), Ordering::Relaxed);
        removed
    }

    /// Get the number of cached entries
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Check if the cache is empty
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStatsSnapshot {
        self.metrics.snapshot()
    }

    /// Get a reference to the metrics
    pub fn metrics(&self) -> Arc<InventoryCacheMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Get information about all cached entries
    pub async fn entries_info(&self) -> Vec<CacheEntryInfo> {
        let entries = self.entries.read().await;
        entries
            .iter()
            .map(|(key, entry)| CacheEntryInfo {
                key: key.clone(),
                age: entry.age(),
                ttl: entry.time_to_live(),
                access_count: entry.access_count,
                size_bytes: entry.size_bytes,
                is_valid: entry.is_valid(),
                dependency_count: entry.source_files.len(),
            })
            .collect()
    }

    /// Evict the least recently used entry
    fn evict_lru_entry(&self, entries: &mut HashMap<String, InventoryCacheEntry>) {
        if let Some((lru_key, _)) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(k, v)| (k.clone(), v.last_accessed))
        {
            if let Some(entry) = entries.remove(&lru_key) {
                self.metrics.memory_bytes.fetch_sub(
                    entry
                        .size_bytes
                        .min(self.metrics.memory_bytes.load(Ordering::Relaxed)),
                    Ordering::Relaxed,
                );
                if self.config.enable_metrics {
                    self.metrics.record_eviction();
                }
            }
        }
    }

    /// Persist a cache entry to disk
    async fn persist_entry(
        &self,
        cache_dir: &Path,
        key: &str,
        inventory: &Inventory,
    ) -> std::io::Result<()> {
        tokio::fs::create_dir_all(cache_dir).await?;

        // Sanitize key for filename
        let safe_key = key.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let cache_file = cache_dir.join(format!("{}.json", safe_key));

        let json = super::plugin::inventory_to_json(inventory)
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        tokio::fs::write(cache_file, serde_json::to_string_pretty(&json)?).await
    }

    /// Load a cache entry from disk
    #[allow(dead_code)]
    async fn load_entry(&self, cache_dir: &Path, key: &str) -> Option<Inventory> {
        let safe_key = key.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let cache_file = cache_dir.join(format!("{}.json", safe_key));

        if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                // Parse the JSON into an inventory
                let mut inventory = Inventory::new();
                if parse_json_inventory_from_value(&mut inventory, &json).is_ok() {
                    return Some(inventory);
                }
            }
        }

        None
    }
}

impl Default for InventoryCache {
    fn default() -> Self {
        Self::with_default_config()
    }
}

impl std::fmt::Debug for InventoryCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InventoryCache")
            .field("config", &self.config)
            .field("metrics", &self.metrics.snapshot())
            .finish()
    }
}

/// Information about a cache entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntryInfo {
    /// Cache key
    pub key: String,
    /// Age of the entry
    pub age: Duration,
    /// Time until expiration
    pub ttl: Option<Duration>,
    /// Number of times accessed
    pub access_count: u64,
    /// Size in bytes
    pub size_bytes: usize,
    /// Whether the entry is still valid
    pub is_valid: bool,
    /// Number of file dependencies
    pub dependency_count: usize,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Estimate the memory size of an inventory
fn estimate_inventory_size(inventory: &Inventory) -> usize {
    let host_count = inventory.host_count();
    let group_count = inventory.group_count();

    // Rough estimate: 500 bytes per host + 200 bytes per group + base overhead
    const HOST_SIZE: usize = 500;
    const GROUP_SIZE: usize = 200;
    const BASE_OVERHEAD: usize = 1000;

    BASE_OVERHEAD + (host_count * HOST_SIZE) + (group_count * GROUP_SIZE)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_basic_operations() {
        let cache = InventoryCache::new(InventoryCacheConfig::default());

        let inventory = Inventory::new();
        cache.set("test", inventory, None).await;

        let cached = cache.get("test").await;
        assert!(cached.is_some());

        cache.invalidate("test").await;
        let cached = cache.get("test").await;
        assert!(cached.is_none());
    }

    #[cfg_attr(tarpaulin, ignore)]
    #[tokio::test]
    async fn test_cache_ttl_expiration() {
        let config = InventoryCacheConfig::default().with_ttl(Duration::from_millis(50));
        let cache = InventoryCache::new(config);

        let inventory = Inventory::new();
        cache.set("test", inventory, None).await;

        // Should be available immediately
        assert!(cache.get("test").await.is_some());

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should be expired now
        assert!(cache.get("test").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_metrics() {
        let cache = InventoryCache::new(InventoryCacheConfig::default());

        let inventory = Inventory::new();
        cache.set("test", inventory, None).await;

        // Hit
        cache.get("test").await;
        // Miss
        cache.get("nonexistent").await;

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_cache_lru_eviction() {
        let config = InventoryCacheConfig::default().with_max_entries(2);
        let cache = InventoryCache::new(config);

        cache.set("key1", Inventory::new(), None).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        cache.set("key2", Inventory::new(), None).await;

        // Access key1 to make it recently used
        cache.get("key1").await;

        // Insert another entry, should evict key2 (least recently used)
        cache.set("key3", Inventory::new(), None).await;

        assert!(cache.get("key1").await.is_some());
        assert!(cache.get("key3").await.is_some());
        assert_eq!(cache.len().await, 2);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = InventoryCache::new(InventoryCacheConfig::default());

        cache.set("test1", Inventory::new(), None).await;
        cache.set("test2", Inventory::new(), None).await;

        assert_eq!(cache.len().await, 2);

        cache.clear().await;

        assert!(cache.is_empty().await);
    }

    #[tokio::test]
    async fn test_disabled_cache() {
        let cache = InventoryCache::new(InventoryCacheConfig::disabled());

        cache.set("test", Inventory::new(), None).await;
        assert!(cache.get("test").await.is_none());
    }

    #[tokio::test]
    async fn test_invalidate_prefix() {
        let cache = InventoryCache::new(InventoryCacheConfig::default());

        cache.set("prefix_1", Inventory::new(), None).await;
        cache.set("prefix_2", Inventory::new(), None).await;
        cache.set("other_1", Inventory::new(), None).await;

        cache.invalidate_prefix("prefix_").await;

        assert!(cache.get("prefix_1").await.is_none());
        assert!(cache.get("prefix_2").await.is_none());
        assert!(cache.get("other_1").await.is_some());
    }

    #[test]
    fn test_file_dependency() {
        // Create a temp file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_dep_file.txt");
        std::fs::write(&temp_file, "test").unwrap();

        let dep = FileDependency::from_path(&temp_file);
        assert!(dep.is_some());

        let dep = dep.unwrap();
        assert!(!dep.is_modified());

        // Sleep to ensure filesystem timestamp granularity is exceeded
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Modify the file
        std::fs::write(&temp_file, "modified").unwrap();
        assert!(dep.is_modified());

        // Cleanup
        let _ = std::fs::remove_file(temp_file);
    }

    #[test]
    fn test_cache_entry_validity() {
        let inventory = Inventory::new();
        let entry = InventoryCacheEntry::new(inventory, Some(Duration::from_secs(60)));

        assert!(!entry.is_expired());
        assert!(entry.is_valid());
        assert!(entry.time_to_live().is_some());
    }

    #[test]
    fn test_config_builders() {
        let dev = InventoryCacheConfig::development();
        assert_eq!(dev.default_ttl, Duration::from_secs(60));
        assert!(!dev.persistent_storage);

        let prod = InventoryCacheConfig::production();
        assert_eq!(prod.default_ttl, Duration::from_secs(600));
        assert!(prod.persistent_storage);

        let disabled = InventoryCacheConfig::disabled();
        assert_eq!(disabled.max_entries, 0);
    }
}
