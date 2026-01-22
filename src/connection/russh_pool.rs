//! Connection pool for Russh SSH connections
//!
//! This module provides a thread-safe, async connection pool for managing
//! multiple russh SSH connections. It supports:
//! - Connection reuse across tasks
//! - Configurable limits per host
//! - Idle timeout management
//! - Thread-safe access for async operations
//! - Graceful pool shutdown
//! - Connection pre-warming for reduced latency
//! - Background maintenance tasks

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};

use super::config::{ConnectionConfig, HostConfig};
use super::russh::RusshConnection;
use super::{Connection, ConnectionError, ConnectionResult};

/// Reference instant for computing elapsed time atomically
static POOL_START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Get nanoseconds since the pool start reference point
#[inline(always)]
fn nanos_since_start() -> u64 {
    let start = POOL_START_TIME.get_or_init(Instant::now);
    start.elapsed().as_nanos() as u64
}

/// Result of a pre-warming operation
#[derive(Debug, Clone, Default)]
pub struct PrewarmResult {
    /// Number of connections successfully created
    pub success: usize,
    /// Number of connections that failed to create
    pub failures: usize,
}

/// Result of a warmup operation
#[derive(Debug, Clone, Default)]
pub struct WarmupResult {
    /// Total number of hosts attempted
    pub total_hosts: usize,
    /// Number of hosts with at least one successful connection
    pub successful_hosts: usize,
    /// Number of hosts with no successful connections
    pub failed_hosts: usize,
    /// Total number of connection attempts
    pub total_connections: usize,
    /// Number of successful connections
    pub successful_connections: usize,
    /// Number of failed connections
    pub failed_connections: usize,
    /// Total warmup duration in milliseconds
    pub warmup_duration_ms: f64,
}

impl WarmupResult {
    /// Check if warmup was fully successful
    pub fn is_success(&self) -> bool {
        self.failed_hosts == 0 && self.failed_connections == 0
    }

    /// Get success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_connections == 0 {
            100.0
        } else {
            (self.successful_connections as f64 / self.total_connections as f64) * 100.0
        }
    }
}

/// Result of a health check operation
#[derive(Debug, Clone, Default)]
pub struct HealthCheckResult {
    /// Number of healthy connections
    pub healthy_connections: usize,
    /// Number of unhealthy connections found
    pub unhealthy_connections: usize,
    /// Number of connections removed from pool
    pub removed_connections: usize,
    /// Health check duration in milliseconds
    pub check_duration_ms: f64,
}

impl HealthCheckResult {
    /// Check if all connections are healthy
    pub fn all_healthy(&self) -> bool {
        self.unhealthy_connections == 0
    }

    /// Get health rate as a percentage
    pub fn health_rate(&self) -> f64 {
        let total = self.healthy_connections + self.unhealthy_connections;
        if total == 0 {
            100.0
        } else {
            (self.healthy_connections as f64 / total as f64) * 100.0
        }
    }
}

/// Internal result for per-host health check
#[derive(Debug, Default)]
struct HostHealthResult {
    healthy: usize,
    unhealthy: usize,
    removed: usize,
}

/// Pool utilization metrics
#[derive(Debug, Clone)]
pub struct PoolUtilizationMetrics {
    /// Total connections in pool
    pub total_connections: usize,
    /// Currently active connections
    pub active_connections: usize,
    /// Currently idle connections
    pub idle_connections: usize,
    /// Maximum allowed connections
    pub max_connections: usize,
    /// Current utilization percentage
    pub utilization_percent: f64,
    /// Hit rate percentage (cache hits / total requests)
    pub hit_rate_percent: f64,
    /// Average connection creation time in milliseconds
    pub avg_connection_time_ms: f64,
    /// Average wait time for connections in milliseconds
    pub avg_wait_time_ms: f64,
    /// Peak number of active connections observed
    pub peak_active: usize,
    /// Per-host utilization breakdown
    pub per_host: Vec<HostUtilization>,
}

/// Per-host utilization statistics
#[derive(Debug, Clone)]
pub struct HostUtilization {
    /// Connection key (ssh://user@host:port)
    pub key: String,
    /// Total connections for this host
    pub total: usize,
    /// Active connections for this host
    pub active: usize,
    /// Idle connections for this host
    pub idle: usize,
    /// Maximum allowed for this host
    pub max_allowed: usize,
}

impl HostUtilization {
    /// Get utilization percentage for this host
    pub fn utilization_percent(&self) -> f64 {
        if self.max_allowed == 0 {
            0.0
        } else {
            (self.total as f64 / self.max_allowed as f64) * 100.0
        }
    }
}

/// Configuration for the connection pool
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections per host
    pub max_connections_per_host: usize,
    /// Minimum number of connections to maintain per host (for pre-warming)
    pub min_connections_per_host: usize,
    /// Maximum total connections in the pool
    pub max_total_connections: usize,
    /// Connection idle timeout (connections unused for this long will be closed)
    pub idle_timeout: Duration,
    /// Health check interval (how often to check connection liveness)
    pub health_check_interval: Duration,
    /// Maximum number of reconnection attempts
    pub max_reconnect_attempts: u32,
    /// Delay between reconnection attempts (base delay for exponential backoff)
    pub reconnect_delay: Duration,
    /// Whether to enable connection health checks
    pub enable_health_checks: bool,
    /// Timeout for health check operations (keepalive/ping)
    pub health_check_timeout: Duration,
    /// Interval for pre-warm maintenance (how often to check and replenish connections)
    pub prewarm_maintenance_interval: Duration,
    /// Number of retry attempts for failed pre-warm connections
    pub prewarm_retry_attempts: u32,
    /// Delay between pre-warm retry attempts
    pub prewarm_retry_delay: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections_per_host: 5,
            min_connections_per_host: 0,
            max_total_connections: 50,
            idle_timeout: Duration::from_secs(300), // 5 minutes
            health_check_interval: Duration::from_secs(30),
            max_reconnect_attempts: 3,
            reconnect_delay: Duration::from_secs(1),
            enable_health_checks: true,
            health_check_timeout: Duration::from_secs(10),
            prewarm_maintenance_interval: Duration::from_secs(60),
            prewarm_retry_attempts: 3,
            prewarm_retry_delay: Duration::from_secs(2),
        }
    }
}

impl PoolConfig {
    /// Create a new pool configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// OPTIMIZATION: Create a lightweight pool config for small workloads (< 10 hosts)
    ///
    /// This configuration minimizes overhead by:
    /// - Disabling background health checks (relies on connection reuse fast-path)
    /// - Lower connection limits
    /// - Shorter timeouts
    /// - No pre-warming maintenance
    pub fn light() -> Self {
        Self {
            max_connections_per_host: 2,
            min_connections_per_host: 0,
            max_total_connections: 20,
            idle_timeout: Duration::from_secs(60),
            health_check_interval: Duration::from_secs(300), // Rarely run
            max_reconnect_attempts: 1,
            reconnect_delay: Duration::from_millis(500),
            enable_health_checks: false, // Rely on connection reuse fast-path
            health_check_timeout: Duration::from_secs(5),
            prewarm_maintenance_interval: Duration::from_secs(300), // Rarely run
            prewarm_retry_attempts: 1,
            prewarm_retry_delay: Duration::from_secs(1),
        }
    }

    /// Set maximum connections per host
    pub fn max_connections_per_host(mut self, max: usize) -> Self {
        self.max_connections_per_host = max;
        self
    }

    /// Set maximum total connections
    pub fn max_total_connections(mut self, max: usize) -> Self {
        self.max_total_connections = max;
        self
    }

    /// Set idle timeout
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Set health check interval
    pub fn health_check_interval(mut self, interval: Duration) -> Self {
        self.health_check_interval = interval;
        self
    }

    /// Enable or disable health checks
    pub fn enable_health_checks(mut self, enable: bool) -> Self {
        self.enable_health_checks = enable;
        self
    }

    /// Set health check timeout
    pub fn health_check_timeout(mut self, timeout: Duration) -> Self {
        self.health_check_timeout = timeout;
        self
    }

    /// Set maximum reconnection attempts
    pub fn max_reconnect_attempts(mut self, attempts: u32) -> Self {
        self.max_reconnect_attempts = attempts;
        self
    }

    /// Set base reconnection delay (for exponential backoff)
    pub fn reconnect_delay(mut self, delay: Duration) -> Self {
        self.reconnect_delay = delay;
        self
    }

    /// Set minimum connections per host (for pre-warming)
    pub fn min_connections_per_host(mut self, min: usize) -> Self {
        self.min_connections_per_host = min;
        self
    }

    /// Set pre-warm maintenance interval
    pub fn prewarm_maintenance_interval(mut self, interval: Duration) -> Self {
        self.prewarm_maintenance_interval = interval;
        self
    }

    /// Set pre-warm retry attempts
    pub fn prewarm_retry_attempts(mut self, attempts: u32) -> Self {
        self.prewarm_retry_attempts = attempts;
        self
    }

    /// Set pre-warm retry delay
    pub fn prewarm_retry_delay(mut self, delay: Duration) -> Self {
        self.prewarm_retry_delay = delay;
        self
    }
}

/// A pooled connection wrapper that tracks usage and health
struct PooledConnection {
    /// The actual connection
    connection: Arc<RusshConnection>,
    /// When the connection was created (as nanos since pool start for atomic ops)
    created_at_nanos: u64,
    /// When the connection was last used (as nanos since pool start for lock-free updates)
    last_used_nanos: AtomicU64,
    /// Number of times this connection has been borrowed
    borrow_count: AtomicUsize,
    /// Whether the connection is currently in use
    in_use: AtomicBool,
    /// Connection parameters for reconnection
    host: String,
    port: u16,
    user: String,
    host_config: Option<HostConfig>,
    /// Whether this connection was created via pre-warming
    is_prewarmed: bool,
}

impl PooledConnection {
    /// Create a new pooled connection
    #[allow(dead_code)]
    fn new(
        connection: RusshConnection,
        host: String,
        port: u16,
        user: String,
        host_config: Option<HostConfig>,
    ) -> Self {
        Self::with_prewarm_flag(connection, host, port, user, host_config, false)
    }

    /// Create a new pooled connection with pre-warm flag
    fn with_prewarm_flag(
        connection: RusshConnection,
        host: String,
        port: u16,
        user: String,
        host_config: Option<HostConfig>,
        is_prewarmed: bool,
    ) -> Self {
        let now_nanos = nanos_since_start();
        Self {
            connection: Arc::new(connection),
            created_at_nanos: now_nanos,
            last_used_nanos: AtomicU64::new(now_nanos),
            borrow_count: AtomicUsize::new(0),
            in_use: AtomicBool::new(false),
            host,
            port,
            user,
            host_config,
            is_prewarmed,
        }
    }

    /// Check if the connection is alive
    async fn is_alive(&self) -> bool {
        self.connection.is_alive().await
    }

    /// Check if the connection is alive with a timeout
    async fn is_alive_with_timeout(&self, timeout: Duration) -> bool {
        match tokio::time::timeout(timeout, self.connection.is_alive()).await {
            Ok(result) => result,
            Err(_) => {
                warn!(
                    host = %self.host,
                    port = %self.port,
                    "Health check timed out after {:?}",
                    timeout
                );
                false
            }
        }
    }

    /// Mark the connection as in use (lock-free fast path)
    #[inline(always)]
    fn acquire(&self) -> bool {
        if self
            .in_use
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            self.borrow_count.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Release the connection back to the pool (lock-free)
    #[inline(always)]
    fn release(&self) {
        self.last_used_nanos
            .store(nanos_since_start(), Ordering::Relaxed);
        self.in_use.store(false, Ordering::Release);
    }

    /// Check if the connection has been idle for too long (lock-free)
    #[inline(always)]
    fn is_idle(&self, timeout: Duration) -> bool {
        let last_used = self.last_used_nanos.load(Ordering::Relaxed);
        let now = nanos_since_start();
        let elapsed_nanos = now.saturating_sub(last_used);
        elapsed_nanos > timeout.as_nanos() as u64
    }

    /// Get the underlying connection
    #[inline(always)]
    fn get_connection(&self) -> Arc<RusshConnection> {
        Arc::clone(&self.connection)
    }

    /// Get connection age
    fn age(&self) -> Duration {
        let now = nanos_since_start();
        let elapsed = now.saturating_sub(self.created_at_nanos);
        Duration::from_nanos(elapsed)
    }
}

impl Clone for PooledConnection {
    fn clone(&self) -> Self {
        Self {
            connection: Arc::clone(&self.connection),
            created_at_nanos: self.created_at_nanos,
            last_used_nanos: AtomicU64::new(nanos_since_start()),
            borrow_count: AtomicUsize::new(self.borrow_count.load(Ordering::SeqCst)),
            in_use: AtomicBool::new(self.in_use.load(Ordering::SeqCst)),
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            host_config: self.host_config.clone(),
            is_prewarmed: self.is_prewarmed,
        }
    }
}

/// Statistics for the connection pool
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total number of connections in the pool
    pub total_connections: usize,
    /// Number of connections currently in use
    pub active_connections: usize,
    /// Number of idle connections
    pub idle_connections: usize,
    /// Number of connection hits (reused connections)
    pub hits: usize,
    /// Number of connection misses (new connections created)
    pub misses: usize,
    /// Number of failed connection attempts
    pub failures: usize,
    /// Number of connections closed due to idle timeout
    pub idle_timeouts: usize,
    /// Number of connections closed due to health check failure
    pub health_check_failures: usize,
    /// Number of connections created via pre-warming
    pub prewarmed_connections: usize,
    /// Number of connections created on-demand
    pub ondemand_connections: usize,
    /// Number of failed pre-warm attempts
    pub prewarm_failures: usize,
    /// Total connection creation time in nanoseconds (for profiling)
    pub total_connection_time_ns: u64,
    /// Number of connection creations (for average calculation)
    pub connection_creation_count: usize,
    /// Maximum connection creation time observed (nanoseconds)
    pub max_connection_time_ns: u64,
    /// Minimum connection creation time observed (nanoseconds)
    pub min_connection_time_ns: u64,
    /// Number of successful health checks
    pub successful_health_checks: usize,
    /// Total wait time for connections (nanoseconds)
    pub total_wait_time_ns: u64,
    /// Number of times a connection was waited for
    pub wait_count: usize,
    /// Peak number of active connections observed
    pub peak_active_connections: usize,
    /// Number of connections warmed up during startup
    pub warmup_connections: usize,
}

impl PoolStats {
    /// Calculate the average connection creation time in milliseconds
    pub fn avg_connection_time_ms(&self) -> f64 {
        if self.connection_creation_count == 0 {
            0.0
        } else {
            (self.total_connection_time_ns as f64 / self.connection_creation_count as f64)
                / 1_000_000.0
        }
    }

    /// Calculate the average wait time in milliseconds
    pub fn avg_wait_time_ms(&self) -> f64 {
        if self.wait_count == 0 {
            0.0
        } else {
            (self.total_wait_time_ns as f64 / self.wait_count as f64) / 1_000_000.0
        }
    }

    /// Calculate the hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Calculate pool utilization (active / total)
    pub fn utilization(&self) -> f64 {
        if self.total_connections == 0 {
            0.0
        } else {
            self.active_connections as f64 / self.total_connections as f64
        }
    }
}

/// Thread-safe connection pool for Russh SSH connections
pub struct RusshConnectionPool {
    /// Pool configuration
    config: PoolConfig,
    /// Global connection configuration
    connection_config: Arc<ConnectionConfig>,
    /// Pooled connections by host key
    connections: Arc<RwLock<HashMap<String, Vec<Arc<PooledConnection>>>>>,
    /// Pool statistics
    stats: RwLock<PoolStats>,
    /// Whether the pool is shutting down
    shutdown: AtomicBool,
}

impl RusshConnectionPool {
    /// Create a new connection pool with default configuration
    pub fn new(connection_config: ConnectionConfig) -> Self {
        Self::with_config(connection_config, PoolConfig::default())
    }

    /// Create a new connection pool with custom configuration
    pub fn with_config(connection_config: ConnectionConfig, pool_config: PoolConfig) -> Self {
        debug!(
            max_per_host = %pool_config.max_connections_per_host,
            min_per_host = %pool_config.min_connections_per_host,
            max_total = %pool_config.max_total_connections,
            idle_timeout = ?pool_config.idle_timeout,
            "Creating new RusshConnectionPool"
        );
        Self {
            config: pool_config,
            connection_config: Arc::new(connection_config),
            connections: Arc::new(RwLock::new(HashMap::new())),
            stats: RwLock::new(PoolStats::default()),
            shutdown: AtomicBool::new(false),
        }
    }

    fn connection_key(host: &str, port: u16, user: &str) -> String {
        format!("ssh://{}@{}:{}", user, host, port)
    }

    pub async fn get(
        &self,
        host: &str,
        port: u16,
        user: &str,
    ) -> ConnectionResult<Arc<RusshConnection>> {
        self.get_with_config(host, port, user, None).await
    }

    pub async fn get_with_config(
        &self,
        host: &str,
        port: u16,
        user: &str,
        host_config: Option<HostConfig>,
    ) -> ConnectionResult<Arc<RusshConnection>> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(ConnectionError::ConnectionClosed);
        }

        let key = Self::connection_key(host, port, user);

        if let Some(conn) = self.try_get_existing(&key).await {
            return Ok(conn.connection());
        }

        let handle = self
            .create_new_connection(host, port, user, host_config, false)
            .await?;
        Ok(handle.connection())
    }

    pub async fn get_or_create(
        &self,
        host: &str,
        port: u16,
        user: &str,
        host_config: Option<HostConfig>,
    ) -> ConnectionResult<PooledConnectionHandle> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(ConnectionError::ConnectionClosed);
        }

        let key = Self::connection_key(host, port, user);

        if let Some(conn) = self.try_get_existing(&key).await {
            return Ok(conn);
        }

        self.create_new_connection(host, port, user, host_config, false)
            .await
    }

    /// OPTIMIZATION: Fast path for getting existing connections
    /// Skips health check for recently used connections (< 10 seconds old)
    async fn try_get_existing(&self, key: &str) -> Option<PooledConnectionHandle> {
        let connections = self.connections.read().await;

        if let Some(host_connections) = connections.get(key) {
            for pooled in host_connections {
                if pooled.acquire() {
                    // OPTIMIZATION: Skip health check for recently used connections
                    // This saves ~5-10ms per connection reuse for small workloads
                    let skip_health_check = !pooled.is_idle(std::time::Duration::from_secs(10));

                    if skip_health_check || pooled.is_alive().await {
                        {
                            let mut stats = self.stats.write().await;
                            stats.hits += 1;
                            stats.active_connections += 1;
                        }
                        debug!(key = %key, skip_health = %skip_health_check, "Reusing existing connection from pool");
                        return Some(PooledConnectionHandle::new(Arc::clone(pooled), self));
                    }
                    pooled.release();
                    warn!(key = %key, "Found dead connection in pool, will be cleaned up");
                }
            }
        }

        None
    }

    async fn create_new_connection(
        &self,
        host: &str,
        port: u16,
        user: &str,
        host_config: Option<HostConfig>,
        is_prewarmed: bool,
    ) -> ConnectionResult<PooledConnectionHandle> {
        let key = Self::connection_key(host, port, user);

        {
            let connections = self.connections.read().await;
            if let Some(host_connections) = connections.get(&key) {
                if host_connections.len() >= self.config.max_connections_per_host {
                    drop(connections);
                    return self.wait_for_connection(&key).await;
                }
            }
        }

        {
            let stats = self.stats.read().await;
            if stats.total_connections >= self.config.max_total_connections {
                drop(stats);
                return self.wait_for_connection(&key).await;
            }
        }

        debug!(host = %host, port = %port, user = %user, "Creating new SSH connection");

        // Profile connection creation time
        let connect_start = Instant::now();
        let connection = RusshConnection::connect(
            host,
            port,
            user,
            host_config.clone(),
            &self.connection_config,
        )
        .await?;
        let connect_duration_ns = connect_start.elapsed().as_nanos() as u64;

        let pooled = Arc::new(PooledConnection::with_prewarm_flag(
            connection,
            host.to_string(),
            port,
            user.to_string(),
            host_config,
            is_prewarmed,
        ));

        pooled.acquire();

        {
            let mut connections = self.connections.write().await;
            connections
                .entry(key.clone())
                .or_insert_with(Vec::new)
                .push(Arc::clone(&pooled));
        }

        {
            let mut stats = self.stats.write().await;
            stats.misses += 1;
            stats.total_connections += 1;
            stats.active_connections += 1;
            // Track peak active connections
            if stats.active_connections > stats.peak_active_connections {
                stats.peak_active_connections = stats.active_connections;
            }
            if is_prewarmed {
                stats.prewarmed_connections += 1;
            } else {
                stats.ondemand_connections += 1;
            }
            // Update connection creation timing stats
            stats.total_connection_time_ns += connect_duration_ns;
            stats.connection_creation_count += 1;
            if stats.min_connection_time_ns == 0
                || connect_duration_ns < stats.min_connection_time_ns
            {
                stats.min_connection_time_ns = connect_duration_ns;
            }
            if connect_duration_ns > stats.max_connection_time_ns {
                stats.max_connection_time_ns = connect_duration_ns;
            }
        }

        info!(key = %key, prewarmed = %is_prewarmed, connect_time_ms = %((connect_duration_ns as f64) / 1_000_000.0), "Created new connection and added to pool");
        Ok(PooledConnectionHandle::new(pooled, self))
    }

    async fn wait_for_connection(&self, key: &str) -> ConnectionResult<PooledConnectionHandle> {
        let timeout = Duration::from_secs(30);
        let start = Instant::now();
        let check_interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if let Some(conn) = self.try_get_existing(key).await {
                // Track wait time
                let wait_ns = start.elapsed().as_nanos() as u64;
                {
                    let mut stats = self.stats.write().await;
                    stats.total_wait_time_ns += wait_ns;
                    stats.wait_count += 1;
                }
                return Ok(conn);
            }
            tokio::time::sleep(check_interval).await;
        }

        Err(ConnectionError::Timeout(30))
    }

    pub async fn release(&self, host: &str, port: u16, user: &str) {
        let key = Self::connection_key(host, port, user);
        trace!(key = %key, "Releasing connection back to pool");

        let connections = self.connections.read().await;

        if let Some(host_connections) = connections.get(&key) {
            for pooled in host_connections {
                if pooled.in_use.load(Ordering::SeqCst) {
                    pooled.release();
                    {
                        let mut stats = self.stats.write().await;
                        stats.active_connections = stats.active_connections.saturating_sub(1);
                        stats.idle_connections += 1;
                    }
                    debug!(key = %key, "Connection released back to pool");
                    return;
                }
            }
        }

        trace!(key = %key, "Connection not found in pool during release");
    }

    pub async fn health_check(&self) {
        if !self.config.enable_health_checks {
            return;
        }

        let keys: Vec<String> = {
            let connections = self.connections.read().await;
            connections.keys().cloned().collect()
        };

        for key in keys {
            self.health_check_host(&key).await;
        }
    }

    async fn health_check_host(&self, key: &str) {
        let connections_to_check: Vec<Arc<PooledConnection>> = {
            let connections = self.connections.read().await;
            connections.get(key).cloned().unwrap_or_default()
        };

        let mut dead_connections = Vec::new();

        for pooled in connections_to_check {
            if !pooled.in_use.load(Ordering::SeqCst) && !pooled.is_alive().await {
                dead_connections.push(pooled);
            }
        }

        if !dead_connections.is_empty() {
            let mut connections = self.connections.write().await;
            if let Some(host_connections) = connections.get_mut(key) {
                let before_len = host_connections.len();
                host_connections
                    .retain(|c| !dead_connections.iter().any(|dead| Arc::ptr_eq(c, dead)));
                let removed = before_len - host_connections.len();

                if removed > 0 {
                    warn!(key = %key, count = %removed, "Removed dead connections from pool");
                    let mut stats = self.stats.write().await;
                    stats.health_check_failures += removed;
                    stats.total_connections = stats.total_connections.saturating_sub(removed);
                }
            }
        }
    }

    pub async fn cleanup_idle(&self) {
        let timeout = self.config.idle_timeout;
        let keys: Vec<String> = {
            let connections = self.connections.read().await;
            connections.keys().cloned().collect()
        };

        for key in keys {
            self.cleanup_idle_host(&key, timeout).await;
        }
    }

    async fn cleanup_idle_host(&self, key: &str, timeout: Duration) {
        let connections_to_check: Vec<Arc<PooledConnection>> = {
            let connections = self.connections.read().await;
            connections.get(key).cloned().unwrap_or_default()
        };

        let mut idle_connections = Vec::new();

        for pooled in connections_to_check {
            if !pooled.in_use.load(Ordering::SeqCst) && pooled.is_idle(timeout) {
                idle_connections.push(pooled);
            }
        }

        if !idle_connections.is_empty() {
            let mut connections = self.connections.write().await;
            if let Some(host_connections) = connections.get_mut(key) {
                let min_to_keep = self.config.min_connections_per_host.max(1);
                let max_to_remove = host_connections.len().saturating_sub(min_to_keep);
                let to_remove = idle_connections.len().min(max_to_remove);

                if to_remove > 0 {
                    let before_len = host_connections.len();
                    let mut removed = 0;

                    host_connections.retain(|c| {
                        if removed >= to_remove {
                            return true;
                        }
                        let should_remove =
                            idle_connections.iter().any(|idle| Arc::ptr_eq(c, idle));
                        if should_remove {
                            removed += 1;
                        }
                        !should_remove
                    });

                    let actual_removed = before_len - host_connections.len();
                    if actual_removed > 0 {
                        debug!(key = %key, count = %actual_removed, "Cleaned up idle connections");
                        let mut stats = self.stats.write().await;
                        stats.idle_timeouts += actual_removed;
                        stats.total_connections =
                            stats.total_connections.saturating_sub(actual_removed);
                        stats.idle_connections =
                            stats.idle_connections.saturating_sub(actual_removed);
                    }
                }
            }
        }
    }

    pub async fn stats(&self) -> PoolStats {
        self.stats.read().await.clone()
    }

    pub async fn connections_for_host(&self, host: &str, port: u16, user: &str) -> usize {
        let key = Self::connection_key(host, port, user);
        let connections = self.connections.read().await;
        connections.get(&key).map(|v| v.len()).unwrap_or(0)
    }

    pub async fn close_all(&self) -> ConnectionResult<()> {
        info!("Closing all connections in pool");
        self.shutdown.store(true, Ordering::SeqCst);

        let all_connections: Vec<(String, Vec<Arc<PooledConnection>>)> = {
            let mut connections = self.connections.write().await;
            connections.drain().collect()
        };

        let mut close_errors = Vec::new();

        for (key, pooled_conns) in all_connections {
            for pooled in pooled_conns {
                debug!(key = %key, "Closing pooled connection");
                if let Err(e) = pooled.connection.close().await {
                    warn!(key = %key, error = %e, "Error closing connection");
                    close_errors.push(e);
                }
            }
        }

        {
            let mut stats = self.stats.write().await;
            *stats = PoolStats::default();
        }

        if close_errors.is_empty() {
            info!("All pooled connections closed successfully");
            Ok(())
        } else {
            warn!("Connection pool closed with {} errors", close_errors.len());
            Err(close_errors.remove(0))
        }
    }

    pub async fn shutdown(&self) -> ConnectionResult<()> {
        self.close_all().await
    }

    /// Pre-warm connections for a specific host
    pub async fn prewarm(
        self: &Arc<Self>,
        host: &str,
        port: u16,
        user: &str,
        count: usize,
        host_config: Option<HostConfig>,
    ) -> PrewarmResult {
        if self.shutdown.load(Ordering::SeqCst) {
            return PrewarmResult {
                success: 0,
                failures: count,
            };
        }

        let key = Self::connection_key(host, port, user);
        let current_count = self.connections_for_host(host, port, user).await;
        let max_allowed = self
            .config
            .max_connections_per_host
            .saturating_sub(current_count);
        let to_create = count.min(max_allowed);

        if to_create == 0 {
            debug!(key = %key, current = %current_count, max = %self.config.max_connections_per_host, "Host already at maximum connections, skipping pre-warm");
            return PrewarmResult {
                success: 0,
                failures: 0,
            };
        }

        info!(key = %key, count = %to_create, "Pre-warming connections");

        let mut handles = Vec::with_capacity(to_create);

        for _ in 0..to_create {
            let pool = Arc::clone(self);
            let host = host.to_string();
            let user = user.to_string();
            let hc = host_config.clone();

            handles.push(tokio::spawn(async move {
                pool.create_prewarm_connection(&host, port, &user, hc).await
            }));
        }

        let mut success = 0;
        let mut failures = 0;

        for handle in handles {
            match handle.await {
                Ok(Ok(())) => success += 1,
                Ok(Err(e)) => {
                    warn!(key = %key, error = %e, "Pre-warm connection failed");
                    failures += 1;
                }
                Err(e) => {
                    warn!(key = %key, error = %e, "Pre-warm task panicked");
                    failures += 1;
                }
            }
        }

        {
            let mut stats = self.stats.write().await;
            stats.prewarm_failures += failures;
        }

        info!(key = %key, success = %success, failures = %failures, "Pre-warming complete");

        PrewarmResult { success, failures }
    }

    async fn create_prewarm_connection(
        &self,
        host: &str,
        port: u16,
        user: &str,
        host_config: Option<HostConfig>,
    ) -> ConnectionResult<()> {
        let key = Self::connection_key(host, port, user);
        let retry_attempts = self.config.prewarm_retry_attempts;
        let retry_delay = self.config.prewarm_retry_delay;

        let mut last_error = None;

        for attempt in 0..=retry_attempts {
            if attempt > 0 {
                tokio::time::sleep(retry_delay).await;
                debug!(key = %key, attempt = %attempt, "Retrying pre-warm connection");
            }

            {
                let connections = self.connections.read().await;
                if let Some(host_connections) = connections.get(&key) {
                    if host_connections.len() >= self.config.max_connections_per_host {
                        return Ok(());
                    }
                }
            }

            {
                let stats = self.stats.read().await;
                if stats.total_connections >= self.config.max_total_connections {
                    return Ok(());
                }
            }

            match RusshConnection::connect(
                host,
                port,
                user,
                host_config.clone(),
                &self.connection_config,
            )
            .await
            {
                Ok(connection) => {
                    let pooled = Arc::new(PooledConnection::with_prewarm_flag(
                        connection,
                        host.to_string(),
                        port,
                        user.to_string(),
                        host_config,
                        true,
                    ));

                    {
                        let mut connections = self.connections.write().await;
                        connections
                            .entry(key.clone())
                            .or_insert_with(Vec::new)
                            .push(pooled);
                    }

                    {
                        let mut stats = self.stats.write().await;
                        stats.total_connections += 1;
                        stats.idle_connections += 1;
                        stats.prewarmed_connections += 1;
                    }

                    debug!(key = %key, "Pre-warmed connection added to pool");
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ConnectionError::ConnectionFailed("Pre-warm failed after retries".to_string())
        }))
    }

    /// Perform a health-check ping on idle connections for a host
    pub async fn health_ping(&self, host: &str, port: u16, user: &str) -> bool {
        let key = Self::connection_key(host, port, user);
        let timeout = self.config.health_check_timeout;

        let connections = self.connections.read().await;
        if let Some(host_connections) = connections.get(&key) {
            for pooled in host_connections {
                if !pooled.in_use.load(Ordering::SeqCst)
                    && pooled.is_alive_with_timeout(timeout).await
                {
                    return true;
                }
            }
        }

        false
    }

    /// Warm up the connection pool for specified hosts
    ///
    /// This method pre-creates connections to reduce latency for the first requests.
    /// Call this during application startup to ensure connections are ready.
    ///
    /// # Arguments
    /// * `hosts` - List of (host, port, user, host_config) tuples to warm up
    /// * `connections_per_host` - Number of connections to create per host
    ///
    /// # Returns
    /// A `WarmupResult` containing success/failure counts
    pub async fn warmup(
        self: &Arc<Self>,
        hosts: &[(String, u16, String, Option<HostConfig>)],
        connections_per_host: usize,
    ) -> WarmupResult {
        if self.shutdown.load(Ordering::SeqCst) {
            return WarmupResult {
                total_hosts: hosts.len(),
                successful_hosts: 0,
                failed_hosts: hosts.len(),
                total_connections: 0,
                successful_connections: 0,
                failed_connections: 0,
                warmup_duration_ms: 0.0,
            };
        }

        let start = Instant::now();
        info!(
            hosts = %hosts.len(),
            connections_per_host = %connections_per_host,
            "Starting connection pool warmup"
        );

        let mut successful_hosts = 0;
        let mut failed_hosts = 0;
        let mut successful_connections = 0;
        let mut failed_connections = 0;

        // Warm up each host
        for (host, port, user, host_config) in hosts {
            let result = self
                .prewarm(host, *port, user, connections_per_host, host_config.clone())
                .await;

            successful_connections += result.success;
            failed_connections += result.failures;

            if result.success > 0 {
                successful_hosts += 1;
            } else {
                failed_hosts += 1;
            }
        }

        let warmup_duration = start.elapsed();

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.warmup_connections = successful_connections;
        }

        info!(
            successful_hosts = %successful_hosts,
            failed_hosts = %failed_hosts,
            successful_connections = %successful_connections,
            failed_connections = %failed_connections,
            duration_ms = %warmup_duration.as_millis(),
            "Connection pool warmup complete"
        );

        WarmupResult {
            total_hosts: hosts.len(),
            successful_hosts,
            failed_hosts,
            total_connections: successful_connections + failed_connections,
            successful_connections,
            failed_connections,
            warmup_duration_ms: warmup_duration.as_secs_f64() * 1000.0,
        }
    }

    /// Perform enhanced health check with connection validation
    ///
    /// This method goes beyond simple liveness checks by actually exercising
    /// the connections to ensure they're fully functional.
    pub async fn deep_health_check(&self) -> HealthCheckResult {
        if !self.config.enable_health_checks {
            return HealthCheckResult::default();
        }

        let start = Instant::now();
        let timeout = self.config.health_check_timeout;

        let keys: Vec<String> = {
            let connections = self.connections.read().await;
            connections.keys().cloned().collect()
        };

        let mut healthy = 0;
        let mut unhealthy = 0;
        let mut removed = 0;

        for key in keys {
            let result = self.deep_health_check_host(&key, timeout).await;
            healthy += result.healthy;
            unhealthy += result.unhealthy;
            removed += result.removed;
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.successful_health_checks += healthy;
            stats.health_check_failures += unhealthy;
        }

        HealthCheckResult {
            healthy_connections: healthy,
            unhealthy_connections: unhealthy,
            removed_connections: removed,
            check_duration_ms: start.elapsed().as_secs_f64() * 1000.0,
        }
    }

    /// Perform deep health check for a specific host
    async fn deep_health_check_host(&self, key: &str, timeout: Duration) -> HostHealthResult {
        let connections_to_check: Vec<Arc<PooledConnection>> = {
            let connections = self.connections.read().await;
            connections.get(key).cloned().unwrap_or_default()
        };

        let mut healthy = 0;
        let mut unhealthy = 0;
        let mut dead_connections = Vec::new();

        for pooled in connections_to_check {
            // Skip connections currently in use
            if pooled.in_use.load(Ordering::SeqCst) {
                continue;
            }

            // Check if connection is alive with timeout
            if pooled.is_alive_with_timeout(timeout).await {
                healthy += 1;
            } else {
                unhealthy += 1;
                dead_connections.push(pooled);
            }
        }

        // Remove dead connections
        let removed = if !dead_connections.is_empty() {
            let mut connections = self.connections.write().await;
            if let Some(host_connections) = connections.get_mut(key) {
                let before_len = host_connections.len();
                host_connections
                    .retain(|c| !dead_connections.iter().any(|dead| Arc::ptr_eq(c, dead)));
                let removed_count = before_len - host_connections.len();

                if removed_count > 0 {
                    warn!(key = %key, count = %removed_count, "Removed unhealthy connections");
                    let mut stats = self.stats.write().await;
                    stats.total_connections = stats.total_connections.saturating_sub(removed_count);
                }

                removed_count
            } else {
                0
            }
        } else {
            0
        };

        HostHealthResult {
            healthy,
            unhealthy,
            removed,
        }
    }

    /// Get current pool utilization metrics
    pub async fn get_utilization_metrics(&self) -> PoolUtilizationMetrics {
        let stats = self.stats.read().await;
        let connections = self.connections.read().await;

        let mut per_host_stats = Vec::new();
        for (key, conns) in connections.iter() {
            let active = conns
                .iter()
                .filter(|c| c.in_use.load(Ordering::Relaxed))
                .count();
            per_host_stats.push(HostUtilization {
                key: key.clone(),
                total: conns.len(),
                active,
                idle: conns.len() - active,
                max_allowed: self.config.max_connections_per_host,
            });
        }

        PoolUtilizationMetrics {
            total_connections: stats.total_connections,
            active_connections: stats.active_connections,
            idle_connections: stats.idle_connections,
            max_connections: self.config.max_total_connections,
            utilization_percent: stats.utilization() * 100.0,
            hit_rate_percent: stats.hit_rate() * 100.0,
            avg_connection_time_ms: stats.avg_connection_time_ms(),
            avg_wait_time_ms: stats.avg_wait_time_ms(),
            peak_active: stats.peak_active_connections,
            per_host: per_host_stats,
        }
    }

    async fn maintain_minimum_connections(self: &Arc<Self>) {
        if self.config.min_connections_per_host == 0 {
            return;
        }

        let hosts: Vec<(String, u16, String, Option<HostConfig>)> = {
            let connections = self.connections.read().await;
            connections
                .values()
                .filter_map(|conns| {
                    conns.first().map(|c| {
                        (
                            c.host.clone(),
                            c.port,
                            c.user.clone(),
                            c.host_config.clone(),
                        )
                    })
                })
                .collect()
        };

        for (host, port, user, host_config) in hosts {
            let current = self.connections_for_host(&host, port, &user).await;
            if current < self.config.min_connections_per_host {
                let needed = self.config.min_connections_per_host - current;
                debug!(host = %host, port = %port, user = %user, current = %current, needed = %needed, "Replenishing connections to maintain minimum");
                let _ = self.prewarm(&host, port, &user, needed, host_config).await;
            }
        }
    }

    /// Start background maintenance tasks
    pub fn start_maintenance(self: &Arc<Self>) {
        let pool = Arc::clone(self);
        let health_interval = pool.config.health_check_interval;
        let idle_timeout = pool.config.idle_timeout;
        let prewarm_interval = pool.config.prewarm_maintenance_interval;
        let min_connections = pool.config.min_connections_per_host;

        if pool.config.enable_health_checks {
            let pool_clone = Arc::clone(&pool);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(health_interval);
                loop {
                    interval.tick().await;
                    if pool_clone.shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    pool_clone.health_check().await;
                }
            });
        }

        let pool_clone = Arc::clone(&pool);
        tokio::spawn(async move {
            let cleanup_interval = idle_timeout / 2;
            let mut interval = tokio::time::interval(cleanup_interval);
            loop {
                interval.tick().await;
                if pool_clone.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                pool_clone.cleanup_idle().await;
            }
        });

        if min_connections > 0 {
            let pool_clone = Arc::clone(&pool);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(prewarm_interval);
                loop {
                    interval.tick().await;
                    if pool_clone.shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    pool_clone.maintain_minimum_connections().await;
                }
            });
        }
    }
}

/// A handle to a pooled connection that releases it back to the pool on drop
pub struct PooledConnectionHandle {
    pooled: Arc<PooledConnection>,
    host: String,
    port: u16,
    user: String,
    released: AtomicBool,
}

impl PooledConnectionHandle {
    fn new(pooled: Arc<PooledConnection>, _pool: &RusshConnectionPool) -> Self {
        Self {
            host: pooled.host.clone(),
            port: pooled.port,
            user: pooled.user.clone(),
            pooled,
            released: AtomicBool::new(false),
        }
    }

    pub fn connection(&self) -> Arc<RusshConnection> {
        self.pooled.get_connection()
    }

    pub fn as_connection(&self) -> Arc<dyn Connection + Send + Sync> {
        self.pooled.get_connection() as Arc<dyn Connection + Send + Sync>
    }

    pub fn age(&self) -> Duration {
        self.pooled.age()
    }

    pub fn borrow_count(&self) -> usize {
        self.pooled.borrow_count.load(Ordering::SeqCst)
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn is_prewarmed(&self) -> bool {
        self.pooled.is_prewarmed
    }

    pub fn mark_released(&self) {
        self.released.store(true, Ordering::SeqCst);
    }

    fn release_sync(&self) {
        if !self.released.swap(true, Ordering::SeqCst) {
            self.pooled.in_use.store(false, Ordering::SeqCst);
        }
    }
}

impl Drop for PooledConnectionHandle {
    fn drop(&mut self) {
        self.release_sync();
    }
}

/// Builder for creating a connection pool with custom settings
pub struct RusshConnectionPoolBuilder {
    pool_config: PoolConfig,
    connection_config: ConnectionConfig,
}

impl RusshConnectionPoolBuilder {
    pub fn new() -> Self {
        Self {
            pool_config: PoolConfig::default(),
            connection_config: ConnectionConfig::default(),
        }
    }

    pub fn connection_config(mut self, config: ConnectionConfig) -> Self {
        self.connection_config = config;
        self
    }

    pub fn max_connections_per_host(mut self, max: usize) -> Self {
        self.pool_config.max_connections_per_host = max;
        self
    }

    pub fn min_connections_per_host(mut self, min: usize) -> Self {
        self.pool_config.min_connections_per_host = min;
        self
    }

    pub fn max_total_connections(mut self, max: usize) -> Self {
        self.pool_config.max_total_connections = max;
        self
    }

    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.pool_config.idle_timeout = timeout;
        self
    }

    pub fn health_check_interval(mut self, interval: Duration) -> Self {
        self.pool_config.health_check_interval = interval;
        self
    }

    pub fn enable_health_checks(mut self, enable: bool) -> Self {
        self.pool_config.enable_health_checks = enable;
        self
    }

    pub fn max_reconnect_attempts(mut self, attempts: u32) -> Self {
        self.pool_config.max_reconnect_attempts = attempts;
        self
    }

    pub fn reconnect_delay(mut self, delay: Duration) -> Self {
        self.pool_config.reconnect_delay = delay;
        self
    }

    pub fn prewarm_maintenance_interval(mut self, interval: Duration) -> Self {
        self.pool_config.prewarm_maintenance_interval = interval;
        self
    }

    pub fn build(self) -> Arc<RusshConnectionPool> {
        let pool = RusshConnectionPool::with_config(self.connection_config, self.pool_config);
        Arc::new(pool)
    }

    pub fn build_with_maintenance(self) -> Arc<RusshConnectionPool> {
        let pool = self.build();
        pool.start_maintenance();
        pool
    }
}

impl Default for RusshConnectionPoolBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections_per_host, 5);
        assert_eq!(config.min_connections_per_host, 0);
        assert_eq!(config.max_total_connections, 50);
        assert_eq!(config.idle_timeout, Duration::from_secs(300));
        assert!(config.enable_health_checks);
    }

    #[test]
    fn test_pool_config_builder() {
        let config = PoolConfig::new()
            .max_connections_per_host(10)
            .min_connections_per_host(2)
            .max_total_connections(100)
            .idle_timeout(Duration::from_secs(600))
            .enable_health_checks(false);

        assert_eq!(config.max_connections_per_host, 10);
        assert_eq!(config.min_connections_per_host, 2);
        assert_eq!(config.max_total_connections, 100);
        assert_eq!(config.idle_timeout, Duration::from_secs(600));
        assert!(!config.enable_health_checks);
    }

    #[test]
    fn test_connection_key() {
        let key = RusshConnectionPool::connection_key("example.com", 22, "admin");
        assert_eq!(key, "ssh://admin@example.com:22");
    }

    #[test]
    fn test_pool_stats_default() {
        let stats = PoolStats::default();
        assert_eq!(stats.total_connections, 0);
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 0);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.prewarmed_connections, 0);
        assert_eq!(stats.ondemand_connections, 0);
        assert_eq!(stats.prewarm_failures, 0);
    }

    #[test]
    fn test_prewarm_result_default() {
        let result = PrewarmResult::default();
        assert_eq!(result.success, 0);
        assert_eq!(result.failures, 0);
    }

    #[tokio::test]
    async fn test_pool_creation() {
        let pool = RusshConnectionPool::new(ConnectionConfig::default());
        let stats = pool.stats().await;
        assert_eq!(stats.total_connections, 0);
    }

    #[test]
    fn test_pool_builder() {
        let pool = RusshConnectionPoolBuilder::new()
            .max_connections_per_host(3)
            .min_connections_per_host(1)
            .max_total_connections(30)
            .idle_timeout(Duration::from_secs(120))
            .enable_health_checks(true)
            .build();

        assert!(!pool.shutdown.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_pool_close_all() {
        let pool = RusshConnectionPool::new(ConnectionConfig::default());
        let result = pool.close_all().await;
        assert!(result.is_ok());
        assert!(pool.shutdown.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_pool_release_nonexistent() {
        let pool = RusshConnectionPool::new(ConnectionConfig::default());
        pool.release("nonexistent.com", 22, "user").await;
        let stats = pool.stats().await;
        assert_eq!(stats.total_connections, 0);
    }

    #[tokio::test]
    async fn test_pool_connections_for_host() {
        let pool = RusshConnectionPool::new(ConnectionConfig::default());
        let count = pool.connections_for_host("example.com", 22, "user").await;
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_pool_health_check_empty() {
        let pool = RusshConnectionPool::new(ConnectionConfig::default());
        pool.health_check().await;
        let stats = pool.stats().await;
        assert_eq!(stats.health_check_failures, 0);
    }

    #[tokio::test]
    async fn test_pool_cleanup_idle_empty() {
        let pool = RusshConnectionPool::new(ConnectionConfig::default());
        pool.cleanup_idle().await;
        let stats = pool.stats().await;
        assert_eq!(stats.idle_timeouts, 0);
    }

    #[tokio::test]
    async fn test_pool_get_after_shutdown() {
        let pool = RusshConnectionPool::new(ConnectionConfig::default());
        pool.close_all().await.unwrap();
        let result = pool.get("example.com", 22, "user").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pool_health_ping_empty() {
        let pool = RusshConnectionPool::new(ConnectionConfig::default());
        let result = pool.health_ping("example.com", 22, "user").await;
        assert!(!result);
    }

    #[tokio::test]
    async fn test_prewarm_after_shutdown() {
        let pool = Arc::new(RusshConnectionPool::new(ConnectionConfig::default()));
        pool.close_all().await.unwrap();
        let result = pool.prewarm("example.com", 22, "user", 3, None).await;
        assert_eq!(result.success, 0);
        assert_eq!(result.failures, 3);
    }
}
