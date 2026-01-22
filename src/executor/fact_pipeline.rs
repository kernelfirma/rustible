//! Async pipeline for fact gathering
//!
//! This module provides an asynchronous pipeline for gathering facts from
//! multiple hosts concurrently. Facts are gathered in parallel streams and
//! cached for efficient access.
//!
//! ## Architecture
//! ```text
//! [Host 1] ─┬─> [Fact Gatherer] ─┬─> [Cache] ─> [Variables]
//! [Host 2] ─┤                    │
//! [Host 3] ─┤   (parallel)       │   (deduplicated)
//! [Host N] ─┘                    ┴
//! ```

use indexmap::IndexMap;
use parking_lot::RwLock;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, trace, warn};

/// Configuration for the fact gathering pipeline
#[derive(Debug, Clone)]
pub struct FactPipelineConfig {
    /// Maximum concurrent fact gathering operations
    pub max_concurrent: usize,
    /// Timeout for individual fact gathering
    pub gather_timeout: Duration,
    /// Cache TTL for facts
    pub cache_ttl: Duration,
    /// Fact subsets to gather (empty = all)
    pub gather_subset: Vec<String>,
    /// Fact subsets to exclude
    pub gather_exclude: Vec<String>,
    /// Enable caching
    pub enable_cache: bool,
    /// Prefetch facts for hosts
    pub enable_prefetch: bool,
}

impl Default for FactPipelineConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            gather_timeout: Duration::from_secs(30),
            cache_ttl: Duration::from_secs(300), // 5 minutes
            gather_subset: Vec::new(),
            gather_exclude: Vec::new(),
            enable_cache: true,
            enable_prefetch: true,
        }
    }
}

/// Cached facts with expiry
#[derive(Debug, Clone)]
struct CachedFacts {
    facts: IndexMap<String, JsonValue>,
    gathered_at: Instant,
    ttl: Duration,
}

impl CachedFacts {
    fn new(facts: IndexMap<String, JsonValue>, ttl: Duration) -> Self {
        Self {
            facts,
            gathered_at: Instant::now(),
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        self.gathered_at.elapsed() > self.ttl
    }
}

/// Result of a fact gathering operation
#[derive(Debug, Clone)]
pub struct FactResult {
    /// Host for which facts were gathered
    pub host: String,
    /// Gathered facts
    pub facts: IndexMap<String, JsonValue>,
    /// Whether facts came from cache
    pub from_cache: bool,
    /// Time taken to gather
    pub gather_time: Duration,
    /// Any error message
    pub error: Option<String>,
}

/// Statistics about the fact pipeline
#[derive(Debug, Clone, Default)]
pub struct FactPipelineStats {
    /// Total gather operations
    pub total_gathers: usize,
    /// Cache hits
    pub cache_hits: usize,
    /// Cache misses
    pub cache_misses: usize,
    /// Failed gathers
    pub failed_gathers: usize,
    /// Average gather time (ms)
    pub avg_gather_time_ms: u64,
    /// Total bytes of facts gathered
    pub total_bytes: usize,
}

impl FactPipelineStats {
    /// Calculate cache hit ratio
    pub fn cache_hit_ratio(&self) -> f64 {
        if self.total_gathers == 0 {
            return 0.0;
        }
        self.cache_hits as f64 / self.total_gathers as f64
    }
}

/// Message types for the pipeline
enum PipelineMessage {
    GatherFacts {
        host: String,
        subset: Vec<String>,
        response_tx: tokio::sync::oneshot::Sender<FactResult>,
    },
    Invalidate {
        host: String,
    },
    InvalidateAll,
    Shutdown,
}

/// Async pipeline for fact gathering
pub struct FactPipeline {
    /// Configuration
    config: FactPipelineConfig,
    /// Fact cache
    cache: Arc<RwLock<HashMap<String, CachedFacts>>>,
    /// Concurrency limiter
    semaphore: Arc<Semaphore>,
    /// Statistics
    stats: Arc<RwLock<FactPipelineStats>>,
    /// Channel for pipeline messages
    message_tx: Option<mpsc::Sender<PipelineMessage>>,
}

impl FactPipeline {
    /// Create a new fact pipeline
    pub fn new(config: FactPipelineConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent));

        Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            semaphore,
            stats: Arc::new(RwLock::new(FactPipelineStats::default())),
            message_tx: None,
        }
    }

    /// Gather facts for a single host
    pub async fn gather_facts(&self, host: &str, gather_fn: impl AsyncFactGatherer) -> FactResult {
        let start = Instant::now();

        // Check cache first
        if self.config.enable_cache {
            if let Some(cached) = self.get_cached_facts(host) {
                self.stats.write().cache_hits += 1;
                self.stats.write().total_gathers += 1;
                return FactResult {
                    host: host.to_string(),
                    facts: cached,
                    from_cache: true,
                    gather_time: start.elapsed(),
                    error: None,
                };
            }
            self.stats.write().cache_misses += 1;
        }

        // Acquire semaphore permit for concurrency control
        let _permit = self
            .semaphore
            .acquire()
            .await
            .expect("Semaphore should not be closed");

        // Gather facts with timeout
        let subset = self.config.gather_subset.clone();
        let gather_result = tokio::time::timeout(
            self.config.gather_timeout,
            gather_fn.gather_facts(host, &subset),
        )
        .await;

        let result = match gather_result {
            Ok(Ok(facts)) => {
                // Cache the results
                if self.config.enable_cache {
                    self.cache_facts(host, facts.clone());
                }

                // Update stats
                {
                    let mut stats = self.stats.write();
                    stats.total_gathers += 1;
                    let gather_time_ms = start.elapsed().as_millis() as u64;
                    stats.avg_gather_time_ms = (stats.avg_gather_time_ms
                        * (stats.total_gathers - 1) as u64
                        + gather_time_ms)
                        / stats.total_gathers as u64;
                }

                FactResult {
                    host: host.to_string(),
                    facts,
                    from_cache: false,
                    gather_time: start.elapsed(),
                    error: None,
                }
            }
            Ok(Err(e)) => {
                self.stats.write().failed_gathers += 1;
                self.stats.write().total_gathers += 1;
                warn!("Failed to gather facts for {}: {}", host, e);
                FactResult {
                    host: host.to_string(),
                    facts: IndexMap::new(),
                    from_cache: false,
                    gather_time: start.elapsed(),
                    error: Some(e),
                }
            }
            Err(_) => {
                self.stats.write().failed_gathers += 1;
                self.stats.write().total_gathers += 1;
                warn!("Timeout gathering facts for {}", host);
                FactResult {
                    host: host.to_string(),
                    facts: IndexMap::new(),
                    from_cache: false,
                    gather_time: start.elapsed(),
                    error: Some(format!("Timeout after {:?}", self.config.gather_timeout)),
                }
            }
        };

        result
    }

    /// Gather facts for multiple hosts concurrently
    pub async fn gather_facts_parallel<F>(&self, hosts: &[String], gather_fn: F) -> Vec<FactResult>
    where
        F: AsyncFactGathererFactory,
    {
        use futures::stream::{self, StreamExt};

        let results: Vec<FactResult> = stream::iter(hosts)
            .map(|host| {
                let gatherer = gather_fn.create();
                async move { self.gather_facts(host, gatherer).await }
            })
            .buffer_unordered(self.config.max_concurrent)
            .collect()
            .await;

        debug!("Gathered facts for {} hosts in parallel", results.len());

        results
    }

    /// Gather facts with streaming results
    pub fn gather_facts_stream<'a, F>(
        &'a self,
        hosts: &'a [String],
        gather_fn: F,
    ) -> impl futures::Stream<Item = FactResult> + 'a
    where
        F: AsyncFactGathererFactory + 'a,
    {
        use futures::stream::{self, StreamExt};

        stream::iter(hosts)
            .map(move |host| {
                let gatherer = gather_fn.create();
                async move { self.gather_facts(host, gatherer).await }
            })
            .buffer_unordered(self.config.max_concurrent)
    }

    /// Prefetch facts for hosts (fire and forget)
    pub fn prefetch_facts<F>(&self, hosts: Vec<String>, gather_fn: F)
    where
        F: AsyncFactGathererFactory + Send + 'static,
    {
        if !self.config.enable_prefetch {
            return;
        }

        let cache = self.cache.clone();
        let semaphore = self.semaphore.clone();
        let timeout = self.config.gather_timeout;
        let ttl = self.config.cache_ttl;
        let subset = self.config.gather_subset.clone();

        tokio::spawn(async move {
            for host in hosts {
                // Check if already cached
                {
                    let cache_read = cache.read();
                    if let Some(cached) = cache_read.get(&host) {
                        if !cached.is_expired() {
                            continue; // Already have fresh facts
                        }
                    }
                }

                // Acquire permit
                let permit = match semaphore.try_acquire() {
                    Ok(p) => p,
                    Err(_) => continue, // Too busy, skip prefetch
                };

                let gatherer = gather_fn.create();
                let result =
                    tokio::time::timeout(timeout, gatherer.gather_facts(&host, &subset)).await;

                drop(permit);

                if let Ok(Ok(facts)) = result {
                    let cached = CachedFacts::new(facts, ttl);
                    cache.write().insert(host, cached);
                    trace!("Prefetched facts for host");
                }
            }
        });
    }

    /// Get cached facts for a host
    fn get_cached_facts(&self, host: &str) -> Option<IndexMap<String, JsonValue>> {
        let cache = self.cache.read();
        if let Some(cached) = cache.get(host) {
            if !cached.is_expired() {
                return Some(cached.facts.clone());
            }
        }
        None
    }

    /// Cache facts for a host
    fn cache_facts(&self, host: &str, facts: IndexMap<String, JsonValue>) {
        let cached = CachedFacts::new(facts, self.config.cache_ttl);
        self.cache.write().insert(host.to_string(), cached);
    }

    /// Invalidate cached facts for a host
    pub fn invalidate(&self, host: &str) {
        self.cache.write().remove(host);
    }

    /// Invalidate all cached facts
    pub fn invalidate_all(&self) {
        self.cache.write().clear();
    }

    /// Clean up expired cache entries
    pub fn cleanup_cache(&self) {
        let mut cache = self.cache.write();
        cache.retain(|_, v| !v.is_expired());
    }

    /// Get statistics
    pub fn stats(&self) -> FactPipelineStats {
        self.stats.read().clone()
    }

    /// Get cache size
    pub fn cache_size(&self) -> usize {
        self.cache.read().len()
    }
}

/// Trait for async fact gathering
#[async_trait::async_trait]
pub trait AsyncFactGatherer: Send + Sync {
    /// Gather facts for a host
    async fn gather_facts(
        self,
        host: &str,
        subset: &[String],
    ) -> Result<IndexMap<String, JsonValue>, String>;
}

/// Factory trait for creating fact gatherers
pub trait AsyncFactGathererFactory: Send + Sync {
    type Gatherer: AsyncFactGatherer;
    fn create(&self) -> Self::Gatherer;
}

/// Simple fact gatherer for testing
pub struct SimpleFactGatherer;

#[async_trait::async_trait]
impl AsyncFactGatherer for SimpleFactGatherer {
    async fn gather_facts(
        self,
        host: &str,
        _subset: &[String],
    ) -> Result<IndexMap<String, JsonValue>, String> {
        // Simulate network delay
        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut facts = IndexMap::new();
        facts.insert(
            "ansible_hostname".to_string(),
            JsonValue::String(host.to_string()),
        );
        facts.insert(
            "ansible_os_family".to_string(),
            JsonValue::String("Debian".to_string()),
        );
        Ok(facts)
    }
}

/// Factory for simple fact gatherer
pub struct SimpleFactGathererFactory;

impl AsyncFactGathererFactory for SimpleFactGathererFactory {
    type Gatherer = SimpleFactGatherer;

    fn create(&self) -> Self::Gatherer {
        SimpleFactGatherer
    }
}

/// Streaming fact results for processing as they arrive
pub struct FactStream {
    rx: mpsc::Receiver<FactResult>,
}

impl FactStream {
    /// Create a new fact stream
    pub fn new(rx: mpsc::Receiver<FactResult>) -> Self {
        Self { rx }
    }

    /// Receive the next fact result
    pub async fn recv(&mut self) -> Option<FactResult> {
        self.rx.recv().await
    }

    /// Collect all remaining results
    pub async fn collect(mut self) -> Vec<FactResult> {
        let mut results = Vec::new();
        while let Some(result) = self.rx.recv().await {
            results.push(result);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fact_pipeline_basic() {
        let config = FactPipelineConfig {
            enable_cache: false,
            ..Default::default()
        };
        let pipeline = FactPipeline::new(config);

        let result = pipeline.gather_facts("test-host", SimpleFactGatherer).await;

        assert!(!result.from_cache);
        assert!(result.error.is_none());
        assert!(result.facts.contains_key("ansible_hostname"));
    }

    #[tokio::test]
    async fn test_fact_pipeline_caching() {
        let config = FactPipelineConfig {
            enable_cache: true,
            cache_ttl: Duration::from_secs(60),
            ..Default::default()
        };
        let pipeline = FactPipeline::new(config);

        // First gather
        let result1 = pipeline.gather_facts("test-host", SimpleFactGatherer).await;
        assert!(!result1.from_cache);

        // Second gather should come from cache
        let result2 = pipeline.gather_facts("test-host", SimpleFactGatherer).await;
        assert!(result2.from_cache);

        let stats = pipeline.stats();
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 1);
    }

    #[cfg_attr(tarpaulin, ignore)]
    #[tokio::test]
    async fn test_fact_pipeline_parallel() {
        let config = FactPipelineConfig {
            max_concurrent: 5,
            enable_cache: false,
            ..Default::default()
        };
        let pipeline = FactPipeline::new(config);

        let hosts: Vec<String> = (0..10).map(|i| format!("host-{}", i)).collect();

        let start = Instant::now();
        let results = pipeline
            .gather_facts_parallel(&hosts, SimpleFactGathererFactory)
            .await;
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 10);

        // With 5 concurrent and 10ms per host, should take ~20-30ms, not 100ms
        // (allowing some overhead)
        assert!(
            elapsed < Duration::from_millis(100),
            "Parallel should be faster: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_fact_pipeline_invalidation() {
        let config = FactPipelineConfig {
            enable_cache: true,
            ..Default::default()
        };
        let pipeline = FactPipeline::new(config);

        // Gather and cache
        let _ = pipeline.gather_facts("test-host", SimpleFactGatherer).await;
        assert_eq!(pipeline.cache_size(), 1);

        // Invalidate
        pipeline.invalidate("test-host");
        assert_eq!(pipeline.cache_size(), 0);
    }

    #[test]
    fn test_stats_cache_hit_ratio() {
        let stats = FactPipelineStats {
            total_gathers: 100,
            cache_hits: 75,
            cache_misses: 25,
            ..Default::default()
        };

        assert!((stats.cache_hit_ratio() - 0.75).abs() < 0.001);
    }
}
