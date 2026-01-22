//! Tiered Intelligent Fact Caching System
//!
//! This module implements a multi-tier caching system designed to address Ansible's
//! performance degradation at scale. The tiered approach ensures optimal memory usage
//! while maintaining fast access to frequently-used facts.
//!
//! ## Architecture
//!
//! ```text
//! +------------------+    +------------------+    +------------------+
//! |   L1: Hot Cache  |    |  L2: Warm Cache  |    |  L3: Cold Cache  |
//! |   (In-Memory)    | -> |   (Disk-Based)   | -> |   (Network/Redis)|
//! |   ~10ms access   |    |   ~100ms access  |    |   ~1-10ms access |
//! +------------------+    +------------------+    +------------------+
//!        ^                       ^                       ^
//!        |                       |                       |
//!   Hot facts              Warm facts              Shared facts
//!   (frequently           (less frequent,         (multi-node
//!    accessed)             still local)            deployments)
//! ```
//!
//! ## Fact Volatility Classification
//!
//! Facts are classified by how often they change, enabling intelligent TTL assignment:
//! - **Static**: Hardware, OS family, distribution (TTL: 1 hour+)
//! - **Semi-Static**: Network config, mounts, users (TTL: 5-15 minutes)
//! - **Dynamic**: Load, memory usage, processes (TTL: 30 seconds - 2 minutes)
//! - **Volatile**: Time, uptime counters (TTL: 0 - never cache)

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

/// Fact volatility classification for intelligent TTL assignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FactVolatility {
    /// Facts that rarely change (hardware, OS info)
    /// TTL: 1 hour or more
    Static,
    /// Facts that change occasionally (network config, mounts)
    /// TTL: 5-15 minutes
    SemiStatic,
    /// Facts that change frequently (load, memory usage)
    /// TTL: 30 seconds - 2 minutes
    Dynamic,
    /// Facts that are always changing (time, uptime)
    /// TTL: 0 - never cache
    Volatile,
}

impl FactVolatility {
    /// Get the recommended TTL for this volatility level
    pub fn recommended_ttl(&self) -> Duration {
        match self {
            FactVolatility::Static => Duration::from_secs(3600), // 1 hour
            FactVolatility::SemiStatic => Duration::from_secs(600), // 10 minutes
            FactVolatility::Dynamic => Duration::from_secs(60),  // 1 minute
            FactVolatility::Volatile => Duration::ZERO,          // Don't cache
        }
    }

    /// Check if this fact should be cached at all
    pub fn should_cache(&self) -> bool {
        !matches!(self, FactVolatility::Volatile)
    }

    /// Get the cache tier preference for this volatility
    pub fn preferred_tier(&self) -> CacheTier {
        match self {
            FactVolatility::Static => CacheTier::L3Cold, // Good for sharing
            FactVolatility::SemiStatic => CacheTier::L2Warm, // Disk is fine
            FactVolatility::Dynamic => CacheTier::L1Hot, // Needs fast access
            FactVolatility::Volatile => CacheTier::L1Hot, // Won't be cached anyway
        }
    }
}

/// Classify a fact key by its volatility
pub fn classify_fact_volatility(fact_key: &str) -> FactVolatility {
    // Static facts - hardware and OS fundamentals
    if fact_key.contains("architecture")
        || fact_key.contains("processor_count")
        || fact_key.contains("processor_cores")
        || fact_key.contains("memtotal")
        || fact_key.contains("os_family")
        || fact_key.contains("distribution")
        || fact_key.contains("kernel")
        || fact_key.contains("machine")
        || fact_key.contains("product_name")
        || fact_key.contains("product_serial")
        || fact_key.contains("bios_")
        || fact_key.contains("virtualization_type")
        || fact_key.contains("virtualization_role")
    {
        return FactVolatility::Static;
    }

    // Semi-static facts - configuration that changes with admin action
    if fact_key.contains("hostname")
        || fact_key.contains("fqdn")
        || fact_key.contains("domain")
        || fact_key.contains("interfaces")
        || fact_key.contains("default_ipv4")
        || fact_key.contains("default_ipv6")
        || fact_key.contains("mounts")
        || fact_key.contains("devices")
        || fact_key.contains("user_")
        || fact_key.contains("selinux")
        || fact_key.contains("apparmor")
        || fact_key.contains("pkg_mgr")
        || fact_key.contains("service_mgr")
        || fact_key.contains("python")
    {
        return FactVolatility::SemiStatic;
    }

    // Dynamic facts - change with system activity
    if fact_key.contains("memfree")
        || fact_key.contains("swapfree")
        || fact_key.contains("loadavg")
        || fact_key.contains("processor_threads")
        || fact_key.contains("local_")
    // local facts set during play
    {
        return FactVolatility::Dynamic;
    }

    // Volatile facts - always changing
    if fact_key.contains("date_time") || fact_key.contains("uptime") || fact_key.contains("epoch") {
        return FactVolatility::Volatile;
    }

    // Default to semi-static for unknown facts
    FactVolatility::SemiStatic
}

/// Cache tier identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CacheTier {
    /// L1: Hot cache - in-memory, fastest access
    L1Hot,
    /// L2: Warm cache - disk-based, local persistence
    L2Warm,
    /// L3: Cold cache - network shared (Redis)
    L3Cold,
}

impl CacheTier {
    /// Get the typical access latency for this tier
    pub fn typical_latency(&self) -> Duration {
        match self {
            CacheTier::L1Hot => Duration::from_micros(100),
            CacheTier::L2Warm => Duration::from_millis(10),
            CacheTier::L3Cold => Duration::from_millis(5), // Redis is fast but network
        }
    }
}

/// A partitioned fact entry optimized for tiered caching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionedFacts {
    /// Static facts (hardware, OS) - rarely change
    pub static_facts: IndexMap<String, JsonValue>,
    /// Semi-static facts (network, config) - occasional changes
    pub semi_static_facts: IndexMap<String, JsonValue>,
    /// Dynamic facts (load, memory) - frequent changes
    pub dynamic_facts: IndexMap<String, JsonValue>,
    /// Volatile facts (time) - not cached
    pub volatile_facts: IndexMap<String, JsonValue>,
    /// Hostname for this fact set
    pub hostname: String,
    /// When facts were gathered
    pub gathered_at: chrono::DateTime<chrono::Utc>,
    /// Subsets that were gathered
    pub subsets: Vec<String>,
}

impl PartitionedFacts {
    /// Create partitioned facts from a flat fact map
    pub fn from_flat(
        hostname: &str,
        facts: IndexMap<String, JsonValue>,
        subsets: Vec<String>,
    ) -> Self {
        let mut static_facts = IndexMap::new();
        let mut semi_static_facts = IndexMap::new();
        let mut dynamic_facts = IndexMap::new();
        let mut volatile_facts = IndexMap::new();

        for (key, value) in facts {
            match classify_fact_volatility(&key) {
                FactVolatility::Static => {
                    static_facts.insert(key, value);
                }
                FactVolatility::SemiStatic => {
                    semi_static_facts.insert(key, value);
                }
                FactVolatility::Dynamic => {
                    dynamic_facts.insert(key, value);
                }
                FactVolatility::Volatile => {
                    volatile_facts.insert(key, value);
                }
            }
        }

        Self {
            static_facts,
            semi_static_facts,
            dynamic_facts,
            volatile_facts,
            hostname: hostname.to_string(),
            gathered_at: chrono::Utc::now(),
            subsets,
        }
    }

    /// Flatten partitioned facts back to a single map
    pub fn to_flat(&self) -> IndexMap<String, JsonValue> {
        let mut facts = IndexMap::new();
        facts.extend(self.static_facts.clone());
        facts.extend(self.semi_static_facts.clone());
        facts.extend(self.dynamic_facts.clone());
        facts.extend(self.volatile_facts.clone());
        facts
    }

    /// Get a specific fact by key
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.static_facts
            .get(key)
            .or_else(|| self.semi_static_facts.get(key))
            .or_else(|| self.dynamic_facts.get(key))
            .or_else(|| self.volatile_facts.get(key))
    }

    /// Get the size estimate in bytes
    pub fn size_bytes(&self) -> usize {
        serde_json::to_string(self).map(|s| s.len()).unwrap_or(1000)
    }

    /// Merge another partitioned facts into this one
    pub fn merge(&mut self, other: PartitionedFacts) {
        self.static_facts.extend(other.static_facts);
        self.semi_static_facts.extend(other.semi_static_facts);
        self.dynamic_facts.extend(other.dynamic_facts);
        self.volatile_facts.extend(other.volatile_facts);
        self.gathered_at = chrono::Utc::now();
    }
}

/// A cached fact entry in the tiered system
#[derive(Debug, Clone)]
pub struct TieredCacheEntry {
    /// The partitioned facts
    pub facts: PartitionedFacts,
    /// Current cache tier
    pub tier: CacheTier,
    /// When this entry was created
    pub created_at: Instant,
    /// Expiry times per volatility level
    pub expiry: TieredExpiry,
    /// Access count for promotion decisions
    pub access_count: u64,
    /// Last access time
    pub last_accessed: Instant,
    /// Size in bytes
    pub size_bytes: usize,
}

/// Expiry tracking for different fact volatility levels
#[derive(Debug, Clone)]
pub struct TieredExpiry {
    /// When static facts expire
    pub static_expires_at: Instant,
    /// When semi-static facts expire
    pub semi_static_expires_at: Instant,
    /// When dynamic facts expire
    pub dynamic_expires_at: Instant,
}

impl TieredExpiry {
    /// Create new expiry times based on volatility TTLs
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            static_expires_at: now + FactVolatility::Static.recommended_ttl(),
            semi_static_expires_at: now + FactVolatility::SemiStatic.recommended_ttl(),
            dynamic_expires_at: now + FactVolatility::Dynamic.recommended_ttl(),
        }
    }

    /// Create with custom TTL multiplier
    pub fn with_multiplier(multiplier: f32) -> Self {
        let now = Instant::now();
        Self {
            static_expires_at: now
                + Duration::from_secs_f32(
                    FactVolatility::Static.recommended_ttl().as_secs_f32() * multiplier,
                ),
            semi_static_expires_at: now
                + Duration::from_secs_f32(
                    FactVolatility::SemiStatic.recommended_ttl().as_secs_f32() * multiplier,
                ),
            dynamic_expires_at: now
                + Duration::from_secs_f32(
                    FactVolatility::Dynamic.recommended_ttl().as_secs_f32() * multiplier,
                ),
        }
    }

    /// Check if static facts have expired
    pub fn static_expired(&self) -> bool {
        Instant::now() >= self.static_expires_at
    }

    /// Check if semi-static facts have expired
    pub fn semi_static_expired(&self) -> bool {
        Instant::now() >= self.semi_static_expires_at
    }

    /// Check if dynamic facts have expired
    pub fn dynamic_expired(&self) -> bool {
        Instant::now() >= self.dynamic_expires_at
    }

    /// Check if all facts have expired
    pub fn all_expired(&self) -> bool {
        self.static_expired() && self.semi_static_expired() && self.dynamic_expired()
    }

    /// Get which volatility levels need refresh
    pub fn needs_refresh(&self) -> Vec<FactVolatility> {
        let mut needs = Vec::new();
        if self.dynamic_expired() {
            needs.push(FactVolatility::Dynamic);
        }
        if self.semi_static_expired() {
            needs.push(FactVolatility::SemiStatic);
        }
        if self.static_expired() {
            needs.push(FactVolatility::Static);
        }
        needs
    }
}

impl Default for TieredExpiry {
    fn default() -> Self {
        Self::new()
    }
}

impl TieredCacheEntry {
    /// Create a new cache entry
    pub fn new(facts: PartitionedFacts, tier: CacheTier) -> Self {
        let size_bytes = facts.size_bytes();
        Self {
            facts,
            tier,
            created_at: Instant::now(),
            expiry: TieredExpiry::new(),
            access_count: 0,
            last_accessed: Instant::now(),
            size_bytes,
        }
    }

    /// Record an access
    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_accessed = Instant::now();
    }

    /// Check if this entry should be promoted to a faster tier
    pub fn should_promote(&self) -> bool {
        // Promote if accessed frequently and not already in L1
        self.tier != CacheTier::L1Hot && self.access_count >= 5
    }

    /// Check if this entry should be demoted to a slower tier
    pub fn should_demote(&self, idle_threshold: Duration) -> bool {
        // Demote if idle for too long and not already in L3
        self.tier != CacheTier::L3Cold && self.last_accessed.elapsed() > idle_threshold
    }

    /// Get valid (non-expired) facts
    pub fn get_valid_facts(&self) -> IndexMap<String, JsonValue> {
        let mut facts = IndexMap::new();

        if !self.expiry.static_expired() {
            facts.extend(self.facts.static_facts.clone());
        }
        if !self.expiry.semi_static_expired() {
            facts.extend(self.facts.semi_static_facts.clone());
        }
        if !self.expiry.dynamic_expired() {
            facts.extend(self.facts.dynamic_facts.clone());
        }
        // Volatile facts are never returned from cache

        facts
    }
}

/// Configuration for the tiered cache
#[derive(Debug, Clone)]
pub struct TieredCacheConfig {
    /// Maximum entries in L1 (hot) cache
    pub l1_max_entries: usize,
    /// Maximum memory for L1 cache in bytes
    pub l1_max_memory_bytes: usize,
    /// Maximum entries in L2 (warm) cache
    pub l2_max_entries: usize,
    /// Path for L2 disk cache
    pub l2_cache_path: PathBuf,
    /// Enable L3 (cold/network) cache
    pub l3_enabled: bool,
    /// Redis URL for L3 cache
    pub l3_redis_url: Option<String>,
    /// TTL multiplier (1.0 = default TTLs)
    pub ttl_multiplier: f32,
    /// Idle threshold for demotion
    pub demotion_idle_threshold: Duration,
    /// Access count threshold for promotion
    pub promotion_access_threshold: u64,
    /// Enable automatic tier management
    pub auto_tier_management: bool,
    /// Interval for tier management tasks
    pub tier_management_interval: Duration,
    /// Enable cache warming on startup
    pub enable_cache_warming: bool,
}

impl Default for TieredCacheConfig {
    fn default() -> Self {
        Self {
            l1_max_entries: 1000,
            l1_max_memory_bytes: 256 * 1024 * 1024, // 256 MB
            l2_max_entries: 10_000,
            l2_cache_path: PathBuf::from("/var/cache/rustible/facts"),
            l3_enabled: false,
            l3_redis_url: None,
            ttl_multiplier: 1.0,
            demotion_idle_threshold: Duration::from_secs(300), // 5 minutes
            promotion_access_threshold: 5,
            auto_tier_management: true,
            tier_management_interval: Duration::from_secs(60),
            enable_cache_warming: true,
        }
    }
}

impl TieredCacheConfig {
    /// Create a development configuration
    pub fn development() -> Self {
        Self {
            l1_max_entries: 100,
            l1_max_memory_bytes: 32 * 1024 * 1024, // 32 MB
            l2_max_entries: 500,
            l2_cache_path: PathBuf::from("/tmp/rustible-facts"),
            l3_enabled: false,
            l3_redis_url: None,
            ttl_multiplier: 0.5, // Shorter TTLs for dev
            demotion_idle_threshold: Duration::from_secs(60),
            promotion_access_threshold: 3,
            auto_tier_management: true,
            tier_management_interval: Duration::from_secs(30),
            enable_cache_warming: false,
        }
    }

    /// Create a production configuration
    pub fn production() -> Self {
        Self {
            l1_max_entries: 5000,
            l1_max_memory_bytes: 512 * 1024 * 1024, // 512 MB
            l2_max_entries: 50_000,
            l2_cache_path: PathBuf::from("/var/cache/rustible/facts"),
            l3_enabled: true,
            l3_redis_url: None,  // Set via environment
            ttl_multiplier: 1.5, // Longer TTLs for production
            demotion_idle_threshold: Duration::from_secs(600), // 10 minutes
            promotion_access_threshold: 10,
            auto_tier_management: true,
            tier_management_interval: Duration::from_secs(120),
            enable_cache_warming: true,
        }
    }
}

/// Metrics for the tiered cache
#[derive(Debug, Default)]
pub struct TieredCacheMetrics {
    /// L1 cache hits
    pub l1_hits: AtomicU64,
    /// L1 cache misses (checked L2)
    pub l1_misses: AtomicU64,
    /// L2 cache hits
    pub l2_hits: AtomicU64,
    /// L2 cache misses (checked L3)
    pub l2_misses: AtomicU64,
    /// L3 cache hits
    pub l3_hits: AtomicU64,
    /// L3 cache misses (total miss)
    pub l3_misses: AtomicU64,
    /// Entries promoted to faster tier
    pub promotions: AtomicU64,
    /// Entries demoted to slower tier
    pub demotions: AtomicU64,
    /// Entries evicted due to capacity
    pub evictions: AtomicU64,
    /// Entries expired by TTL
    pub expirations: AtomicU64,
    /// Current L1 entries
    pub l1_entries: AtomicUsize,
    /// Current L2 entries
    pub l2_entries: AtomicUsize,
    /// Current L3 entries
    pub l3_entries: AtomicUsize,
    /// Current L1 memory usage
    pub l1_memory_bytes: AtomicUsize,
    /// Total bytes saved by caching
    pub bytes_saved: AtomicU64,
    /// Average access latency in microseconds
    pub avg_latency_us: AtomicU64,
}

impl TieredCacheMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Calculate overall hit rate
    pub fn overall_hit_rate(&self) -> f64 {
        let total_hits = self.l1_hits.load(Ordering::Relaxed)
            + self.l2_hits.load(Ordering::Relaxed)
            + self.l3_hits.load(Ordering::Relaxed);
        let total_misses = self.l3_misses.load(Ordering::Relaxed);
        let total = total_hits + total_misses;
        if total > 0 {
            total_hits as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Calculate L1 hit rate
    pub fn l1_hit_rate(&self) -> f64 {
        let hits = self.l1_hits.load(Ordering::Relaxed);
        let misses = self.l1_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total > 0 {
            hits as f64 / total as f64
        } else {
            0.0
        }
    }

    /// Get a summary report
    pub fn summary(&self) -> String {
        format!(
            "L1: {}/{} ({:.1}%), L2: {}/{}, L3: {}/{}, Overall: {:.1}%, Promotions: {}, Demotions: {}, Evictions: {}",
            self.l1_hits.load(Ordering::Relaxed),
            self.l1_hits.load(Ordering::Relaxed) + self.l1_misses.load(Ordering::Relaxed),
            self.l1_hit_rate() * 100.0,
            self.l2_hits.load(Ordering::Relaxed),
            self.l2_misses.load(Ordering::Relaxed),
            self.l3_hits.load(Ordering::Relaxed),
            self.l3_misses.load(Ordering::Relaxed),
            self.overall_hit_rate() * 100.0,
            self.promotions.load(Ordering::Relaxed),
            self.demotions.load(Ordering::Relaxed),
            self.evictions.load(Ordering::Relaxed),
        )
    }
}

/// Cache invalidation event
#[derive(Debug, Clone)]
pub enum InvalidationEvent {
    /// Invalidate a specific host
    Host(String),
    /// Invalidate hosts matching a pattern
    Pattern(String),
    /// Invalidate all entries
    All,
    /// Invalidate by volatility level
    Volatility(FactVolatility),
}

/// The tiered fact cache implementation
pub struct TieredFactCache {
    /// L1: Hot cache (in-memory)
    l1_cache: DashMap<String, TieredCacheEntry>,
    /// L2: Warm cache (disk-based with memory index)
    l2_cache: DashMap<String, TieredCacheEntry>,
    /// L2 disk persistence
    l2_disk_path: PathBuf,
    /// L3: Cold cache backend (placeholder for Redis)
    l3_enabled: bool,
    /// Configuration
    config: TieredCacheConfig,
    /// Metrics
    metrics: Arc<TieredCacheMetrics>,
    /// Invalidation broadcast channel
    invalidation_tx: broadcast::Sender<InvalidationEvent>,
    /// Background task handle
    _background_handle: Option<tokio::task::JoinHandle<()>>,
}

impl TieredFactCache {
    /// Create a new tiered cache
    pub fn new(config: TieredCacheConfig) -> Self {
        let (invalidation_tx, _) = broadcast::channel(100);

        // Ensure L2 cache directory exists
        if let Err(e) = std::fs::create_dir_all(&config.l2_cache_path) {
            warn!("Failed to create L2 cache directory: {}", e);
        }

        let l2_disk_path = config.l2_cache_path.clone();

        Self {
            l1_cache: DashMap::with_capacity(config.l1_max_entries.min(1000)),
            l2_cache: DashMap::with_capacity(config.l2_max_entries.min(5000)),
            l2_disk_path,
            l3_enabled: config.l3_enabled,
            config,
            metrics: Arc::new(TieredCacheMetrics::new()),
            invalidation_tx,
            _background_handle: None,
        }
    }

    /// Get cached facts for a host
    pub fn get(&self, hostname: &str) -> Option<IndexMap<String, JsonValue>> {
        let start = Instant::now();

        // Try L1 first
        if let Some(mut entry) = self.l1_cache.get_mut(hostname) {
            entry.record_access();
            let facts = entry.get_valid_facts();
            if !facts.is_empty() {
                self.metrics.l1_hits.fetch_add(1, Ordering::Relaxed);
                self.record_latency(start.elapsed());
                return Some(facts);
            }
        }
        self.metrics.l1_misses.fetch_add(1, Ordering::Relaxed);

        // Try L2
        if let Some(mut entry) = self.l2_cache.get_mut(hostname) {
            entry.record_access();
            let facts = entry.get_valid_facts();
            if !facts.is_empty() {
                self.metrics.l2_hits.fetch_add(1, Ordering::Relaxed);
                self.record_latency(start.elapsed());

                // Promote to L1 if accessed frequently
                if entry.should_promote() {
                    let entry_clone = entry.clone();
                    drop(entry);
                    self.promote_to_l1(hostname, entry_clone);
                }

                return Some(facts);
            }
        }
        self.metrics.l2_misses.fetch_add(1, Ordering::Relaxed);

        // Try L3 if enabled
        if self.l3_enabled {
            if let Some(facts) = self.get_from_l3(hostname) {
                self.metrics.l3_hits.fetch_add(1, Ordering::Relaxed);
                self.record_latency(start.elapsed());
                return Some(facts);
            }
        }
        self.metrics.l3_misses.fetch_add(1, Ordering::Relaxed);

        self.record_latency(start.elapsed());
        None
    }

    /// Get facts with specific volatility levels
    pub fn get_by_volatility(
        &self,
        hostname: &str,
        volatilities: &[FactVolatility],
    ) -> Option<IndexMap<String, JsonValue>> {
        if let Some(entry) = self.l1_cache.get(hostname) {
            let mut facts = IndexMap::new();
            for volatility in volatilities {
                match volatility {
                    FactVolatility::Static if !entry.expiry.static_expired() => {
                        facts.extend(entry.facts.static_facts.clone());
                    }
                    FactVolatility::SemiStatic if !entry.expiry.semi_static_expired() => {
                        facts.extend(entry.facts.semi_static_facts.clone());
                    }
                    FactVolatility::Dynamic if !entry.expiry.dynamic_expired() => {
                        facts.extend(entry.facts.dynamic_facts.clone());
                    }
                    _ => {}
                }
            }
            if !facts.is_empty() {
                return Some(facts);
            }
        }
        None
    }

    /// Insert facts for a host
    pub fn insert(&self, hostname: &str, facts: IndexMap<String, JsonValue>, subsets: Vec<String>) {
        let partitioned = PartitionedFacts::from_flat(hostname, facts, subsets);
        let entry = TieredCacheEntry::new(partitioned, CacheTier::L1Hot);
        let size = entry.size_bytes;

        // Check if we need to evict from L1
        self.maybe_evict_l1(size);

        self.l1_cache.insert(hostname.to_string(), entry);
        self.metrics
            .l1_entries
            .store(self.l1_cache.len(), Ordering::Relaxed);
        self.metrics
            .l1_memory_bytes
            .fetch_add(size, Ordering::Relaxed);
    }

    /// Insert with custom tier
    pub fn insert_to_tier(
        &self,
        hostname: &str,
        facts: IndexMap<String, JsonValue>,
        subsets: Vec<String>,
        tier: CacheTier,
    ) {
        let partitioned = PartitionedFacts::from_flat(hostname, facts, subsets);
        let entry = TieredCacheEntry::new(partitioned, tier);

        match tier {
            CacheTier::L1Hot => {
                self.l1_cache.insert(hostname.to_string(), entry);
                self.metrics
                    .l1_entries
                    .store(self.l1_cache.len(), Ordering::Relaxed);
            }
            CacheTier::L2Warm => {
                self.l2_cache.insert(hostname.to_string(), entry.clone());
                self.persist_to_disk(hostname, &entry);
                self.metrics
                    .l2_entries
                    .store(self.l2_cache.len(), Ordering::Relaxed);
            }
            CacheTier::L3Cold => {
                if self.l3_enabled {
                    self.insert_to_l3(hostname, entry);
                }
            }
        }
    }

    /// Invalidate cached facts for a host
    pub fn invalidate(&self, hostname: &str) {
        self.l1_cache.remove(hostname);
        self.l2_cache.remove(hostname);
        self.remove_from_disk(hostname);
        if self.l3_enabled {
            self.invalidate_l3(hostname);
        }

        // Broadcast invalidation event
        let _ = self
            .invalidation_tx
            .send(InvalidationEvent::Host(hostname.to_string()));

        self.update_entry_counts();
    }

    /// Invalidate all entries
    pub fn clear(&self) {
        self.l1_cache.clear();
        self.l2_cache.clear();
        self.clear_disk_cache();

        let _ = self.invalidation_tx.send(InvalidationEvent::All);

        self.update_entry_counts();
    }

    /// Get the number of entries in each tier
    pub fn tier_counts(&self) -> (usize, usize, usize) {
        (self.l1_cache.len(), self.l2_cache.len(), 0)
    }

    /// Get total entry count
    pub fn len(&self) -> usize {
        self.l1_cache.len() + self.l2_cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.l1_cache.is_empty() && self.l2_cache.is_empty()
    }

    /// Get metrics
    pub fn metrics(&self) -> Arc<TieredCacheMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Subscribe to invalidation events
    pub fn subscribe_invalidations(&self) -> broadcast::Receiver<InvalidationEvent> {
        self.invalidation_tx.subscribe()
    }

    /// Cleanup expired entries
    pub fn cleanup_expired(&self) -> usize {
        let mut removed = 0;

        // Cleanup L1
        let l1_to_remove: Vec<String> = self
            .l1_cache
            .iter()
            .filter(|e| e.value().expiry.all_expired())
            .map(|e| e.key().clone())
            .collect();

        for key in l1_to_remove {
            if let Some((_, entry)) = self.l1_cache.remove(&key) {
                removed += 1;
                self.metrics
                    .l1_memory_bytes
                    .fetch_sub(entry.size_bytes, Ordering::Relaxed);
                self.metrics.expirations.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Cleanup L2
        let l2_to_remove: Vec<String> = self
            .l2_cache
            .iter()
            .filter(|e| e.value().expiry.all_expired())
            .map(|e| e.key().clone())
            .collect();

        for key in l2_to_remove {
            if self.l2_cache.remove(&key).is_some() {
                self.remove_from_disk(&key);
                removed += 1;
                self.metrics.expirations.fetch_add(1, Ordering::Relaxed);
            }
        }

        self.update_entry_counts();
        removed
    }

    /// Run tier management (promotions/demotions)
    pub fn manage_tiers(&self) {
        // Demote idle L1 entries to L2
        let to_demote: Vec<(String, TieredCacheEntry)> = self
            .l1_cache
            .iter()
            .filter(|e| e.value().should_demote(self.config.demotion_idle_threshold))
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        for (hostname, entry) in to_demote {
            self.demote_to_l2(&hostname, entry);
        }

        // Promote frequently accessed L2 entries to L1
        let to_promote: Vec<(String, TieredCacheEntry)> = self
            .l2_cache
            .iter()
            .filter(|e| e.value().access_count >= self.config.promotion_access_threshold)
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        for (hostname, entry) in to_promote {
            self.promote_to_l1(&hostname, entry);
        }

        self.update_entry_counts();
    }

    /// Warm the cache from disk
    pub fn warm_from_disk(&self) -> usize {
        let mut loaded = 0;

        if let Ok(entries) = std::fs::read_dir(&self.l2_disk_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(facts) = serde_json::from_str::<PartitionedFacts>(&content) {
                            let cache_entry =
                                TieredCacheEntry::new(facts.clone(), CacheTier::L2Warm);
                            if !cache_entry.expiry.all_expired() {
                                self.l2_cache.insert(facts.hostname.clone(), cache_entry);
                                loaded += 1;
                            } else {
                                let _ = std::fs::remove_file(&path);
                            }
                        }
                    }
                }
            }
        }

        self.metrics
            .l2_entries
            .store(self.l2_cache.len(), Ordering::Relaxed);

        debug!("Warmed cache with {} entries from disk", loaded);
        loaded
    }

    // --- Private helper methods ---

    fn promote_to_l1(&self, hostname: &str, mut entry: TieredCacheEntry) {
        entry.tier = CacheTier::L1Hot;
        entry.access_count = 0; // Reset after promotion

        // Remove from L2
        self.l2_cache.remove(hostname);

        // Add to L1 (with eviction if needed)
        self.maybe_evict_l1(entry.size_bytes);
        self.l1_cache.insert(hostname.to_string(), entry);

        self.metrics.promotions.fetch_add(1, Ordering::Relaxed);
        trace!("Promoted {} to L1", hostname);
    }

    fn demote_to_l2(&self, hostname: &str, mut entry: TieredCacheEntry) {
        entry.tier = CacheTier::L2Warm;
        entry.access_count = 0; // Reset after demotion

        // Remove from L1
        if let Some((_, old)) = self.l1_cache.remove(hostname) {
            self.metrics
                .l1_memory_bytes
                .fetch_sub(old.size_bytes, Ordering::Relaxed);
        }

        // Add to L2 and persist
        self.l2_cache.insert(hostname.to_string(), entry.clone());
        self.persist_to_disk(hostname, &entry);

        self.metrics.demotions.fetch_add(1, Ordering::Relaxed);
        trace!("Demoted {} to L2", hostname);
    }

    fn maybe_evict_l1(&self, needed_bytes: usize) {
        // Check entry count
        while self.l1_cache.len() >= self.config.l1_max_entries {
            self.evict_lru_from_l1();
        }

        // Check memory
        if self.config.l1_max_memory_bytes > 0 {
            loop {
                let current_memory = self.metrics.l1_memory_bytes.load(Ordering::Relaxed);
                if current_memory + needed_bytes <= self.config.l1_max_memory_bytes {
                    break;
                }
                if !self.evict_lru_from_l1() {
                    break;
                }
            }
        }
    }

    fn evict_lru_from_l1(&self) -> bool {
        let mut oldest: Option<(String, Instant)> = None;

        for entry in self.l1_cache.iter() {
            if oldest.is_none() || entry.value().last_accessed < oldest.as_ref().unwrap().1 {
                oldest = Some((entry.key().clone(), entry.value().last_accessed));
            }
        }

        if let Some((hostname, _)) = oldest {
            if let Some((_, entry)) = self.l1_cache.remove(&hostname) {
                self.metrics
                    .l1_memory_bytes
                    .fetch_sub(entry.size_bytes, Ordering::Relaxed);
                self.metrics.evictions.fetch_add(1, Ordering::Relaxed);

                // Demote to L2 instead of discarding
                self.l2_cache.insert(
                    hostname.clone(),
                    TieredCacheEntry {
                        tier: CacheTier::L2Warm,
                        ..entry
                    },
                );

                return true;
            }
        }

        false
    }

    fn persist_to_disk(&self, hostname: &str, entry: &TieredCacheEntry) {
        let safe_name: String = hostname
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = self.l2_disk_path.join(format!("{}.json", safe_name));

        if let Ok(json) = serde_json::to_string_pretty(&entry.facts) {
            let _ = std::fs::write(path, json);
        }
    }

    fn remove_from_disk(&self, hostname: &str) {
        let safe_name: String = hostname
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = self.l2_disk_path.join(format!("{}.json", safe_name));
        let _ = std::fs::remove_file(path);
    }

    fn clear_disk_cache(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.l2_disk_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }

    fn get_from_l3(&self, _hostname: &str) -> Option<IndexMap<String, JsonValue>> {
        // Placeholder for Redis implementation
        // In a full implementation, this would:
        // 1. Connect to Redis using the configured URL
        // 2. Get the cached facts using the hostname as key
        // 3. Deserialize and return
        None
    }

    fn insert_to_l3(&self, _hostname: &str, _entry: TieredCacheEntry) {
        // Placeholder for Redis implementation
    }

    fn invalidate_l3(&self, _hostname: &str) {
        // Placeholder for Redis implementation
    }

    fn record_latency(&self, latency: Duration) {
        let latency_us = latency.as_micros() as u64;
        let current = self.metrics.avg_latency_us.load(Ordering::Relaxed);
        // Exponential moving average
        let new_avg = if current == 0 {
            latency_us
        } else {
            (current * 9 + latency_us) / 10
        };
        self.metrics
            .avg_latency_us
            .store(new_avg, Ordering::Relaxed);
    }

    fn update_entry_counts(&self) {
        self.metrics
            .l1_entries
            .store(self.l1_cache.len(), Ordering::Relaxed);
        self.metrics
            .l2_entries
            .store(self.l2_cache.len(), Ordering::Relaxed);
    }
}

/// Start background tier management task
pub fn start_tier_management(
    cache: Arc<TieredFactCache>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            cache.cleanup_expired();
            if cache.config.auto_tier_management {
                cache.manage_tiers();
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fact_volatility_classification() {
        assert_eq!(
            classify_fact_volatility("ansible_architecture"),
            FactVolatility::Static
        );
        assert_eq!(
            classify_fact_volatility("ansible_os_family"),
            FactVolatility::Static
        );
        assert_eq!(
            classify_fact_volatility("ansible_hostname"),
            FactVolatility::SemiStatic
        );
        assert_eq!(
            classify_fact_volatility("ansible_default_ipv4"),
            FactVolatility::SemiStatic
        );
        assert_eq!(
            classify_fact_volatility("ansible_memfree_mb"),
            FactVolatility::Dynamic
        );
        assert_eq!(
            classify_fact_volatility("ansible_date_time"),
            FactVolatility::Volatile
        );
    }

    #[test]
    fn test_volatility_ttl() {
        assert!(FactVolatility::Static.recommended_ttl() > Duration::from_secs(1800));
        assert!(FactVolatility::SemiStatic.recommended_ttl() >= Duration::from_secs(300));
        assert!(FactVolatility::Dynamic.recommended_ttl() >= Duration::from_secs(30));
        assert_eq!(FactVolatility::Volatile.recommended_ttl(), Duration::ZERO);
    }

    #[test]
    fn test_partitioned_facts() {
        let mut facts = IndexMap::new();
        facts.insert(
            "ansible_os_family".to_string(),
            JsonValue::String("Debian".to_string()),
        );
        facts.insert(
            "ansible_hostname".to_string(),
            JsonValue::String("server1".to_string()),
        );
        facts.insert(
            "ansible_memfree_mb".to_string(),
            JsonValue::Number(1024.into()),
        );
        facts.insert(
            "ansible_date_time".to_string(),
            JsonValue::String("2024-01-01".to_string()),
        );

        let partitioned = PartitionedFacts::from_flat("server1", facts, vec!["all".to_string()]);

        assert!(partitioned.static_facts.contains_key("ansible_os_family"));
        assert!(partitioned
            .semi_static_facts
            .contains_key("ansible_hostname"));
        assert!(partitioned.dynamic_facts.contains_key("ansible_memfree_mb"));
        assert!(partitioned.volatile_facts.contains_key("ansible_date_time"));
    }

    #[test]
    fn test_tiered_cache_basic() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        let mut facts = IndexMap::new();
        facts.insert(
            "ansible_os_family".to_string(),
            JsonValue::String("Debian".to_string()),
        );

        cache.insert("host1", facts.clone(), vec!["all".to_string()]);

        let cached = cache.get("host1").unwrap();
        assert!(cached.contains_key("ansible_os_family"));
    }

    #[test]
    fn test_tiered_cache_invalidation() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        let mut facts = IndexMap::new();
        facts.insert("test".to_string(), JsonValue::String("value".to_string()));

        cache.insert("host1", facts.clone(), vec![]);
        cache.insert("host2", facts, vec![]);

        assert_eq!(cache.len(), 2);

        cache.invalidate("host1");

        assert!(cache.get("host1").is_none());
        assert!(cache.get("host2").is_some());
    }

    #[test]
    fn test_tiered_expiry() {
        let expiry = TieredExpiry::new();

        // Nothing should be expired immediately
        assert!(!expiry.static_expired());
        assert!(!expiry.semi_static_expired());
        assert!(!expiry.dynamic_expired());
        assert!(!expiry.all_expired());
    }

    #[test]
    fn test_cache_metrics() {
        let config = TieredCacheConfig::development();
        let cache = TieredFactCache::new(config);

        // Miss
        assert!(cache.get("nonexistent").is_none());

        let metrics = cache.metrics();
        assert_eq!(metrics.l1_misses.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.l3_misses.load(Ordering::Relaxed), 1);

        // Insert and hit
        let mut facts = IndexMap::new();
        facts.insert("test".to_string(), JsonValue::String("value".to_string()));
        cache.insert("host1", facts, vec![]);

        assert!(cache.get("host1").is_some());
        assert_eq!(metrics.l1_hits.load(Ordering::Relaxed), 1);
    }
}
