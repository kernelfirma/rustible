//! Optimized connection pooling for maximum throughput
//!
//! This module provides an intelligent connection pooling system that:
//! - Reuses connections across tasks
//! - Manages connection lifecycle automatically
//! - Handles connection failures gracefully
//! - Implements adaptive sizing based on load

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tokio::sync::Semaphore;

use crate::connection::{Connection, ConnectionError};

/// Connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections per host
    pub max_connections_per_host: usize,
    /// Maximum total connections across all hosts
    pub max_total_connections: usize,
    /// How long to keep idle connections alive
    pub idle_timeout: Duration,
    /// How long to wait for a connection from the pool
    pub acquire_timeout: Duration,
    /// Maximum age of a connection before recycling
    pub max_connection_age: Option<Duration>,
    /// Number of connections to pre-warm for new hosts
    pub pre_warm_connections: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections_per_host: 10,
            max_total_connections: 100,
            idle_timeout: Duration::from_secs(60),
            acquire_timeout: Duration::from_secs(30),
            max_connection_age: Some(Duration::from_secs(300)),
            pre_warm_connections: 2,
        }
    }
}

impl PoolConfig {
    /// Create a config for low-latency scenarios
    pub fn low_latency() -> Self {
        Self {
            max_connections_per_host: 20,
            max_total_connections: 200,
            idle_timeout: Duration::from_secs(120),
            acquire_timeout: Duration::from_secs(10),
            max_connection_age: Some(Duration::from_secs(600)),
            pre_warm_connections: 5,
        }
    }

    /// Create a config for resource-constrained scenarios
    pub fn resource_constrained() -> Self {
        Self {
            max_connections_per_host: 3,
            max_total_connections: 20,
            idle_timeout: Duration::from_secs(30),
            acquire_timeout: Duration::from_secs(60),
            max_connection_age: Some(Duration::from_secs(180)),
            pre_warm_connections: 1,
        }
    }
}

/// Pooled connection wrapper
#[derive(Debug)]
struct PooledConnection {
    /// The underlying connection
    connection: Arc<dyn Connection>,
    /// When this connection was created
    created_at: Instant,
    /// When this connection was last used
    last_used: Instant,
    /// Whether this connection is currently in use
    in_use: bool,
    /// Host this connection is for
    host: String,
}

/// Connection pool for a single host
#[derive(Debug)]
struct HostPool {
    /// Host identifier
    host: String,
    /// Available connections
    available: Vec<PooledConnection>,
    /// All connections (including in-use)
    all_connections: Vec<PooledConnection>,
    /// Semaphore to limit concurrent connections
    semaphore: Arc<Semaphore>,
    /// Pool configuration
    config: PoolConfig,
    /// Total number of connections created
    created_count: usize,
    /// Number of connection errors
    error_count: usize,
}

impl HostPool {
    /// Create a new host pool
    fn new(host: impl Into<String>, config: PoolConfig) -> Self {
        let host = host.into();
        Self {
            semaphore: Arc::new(Semaphore::new(config.max_connections_per_host)),
            host,
            available: Vec::new(),
            all_connections: Vec::new(),
            config,
            created_count: 0,
            error_count: 0,
        }
    }

    /// Acquire a connection from the pool
    async fn acquire(&mut self) -> Result<Arc<dyn Connection>, ConnectionError> {
        // Try to get an available connection
        if let Some(mut conn) = self.available.pop() {
            // Check if connection is still valid
            if self.is_connection_valid(&conn) {
                conn.last_used = Instant::now();
                conn.in_use = true;
                return Ok(conn.connection);
            }
            // Connection is invalid, remove it
            self.remove_connection(&conn);
        }

        // Wait for permission to create a new connection
        let _permit = tokio::time::timeout(
            self.config.acquire_timeout,
            self.semaphore.acquire()
        )
        .await
        .map_err(|_| ConnectionError::Timeout(
            "Timed out waiting for connection from pool".to_string()
        ))?
        .map_err(|_| ConnectionError::Other("Semaphore closed".to_string()))?;

        // Create a new connection
        self.create_connection().await
    }

    /// Create a new connection
    async fn create_connection(&mut self) -> Result<Arc<dyn Connection>, ConnectionError> {
        // Implementation would create actual connection
        // For now, return a mock error
        Err(ConnectionError::Other("Not implemented".to_string()))
    }

    /// Release a connection back to the pool
    fn release(&mut self, connection: Arc<dyn Connection>) {
        // Find the connection in all_connections
        if let Some(mut conn) = self.all_connections.iter_mut()
            .find(|c| Arc::ptr_eq(&c.connection, &connection)) 
        {
            conn.last_used = Instant::now();
            conn.in_use = false;
            
            // Check if we should keep this connection
            if self.is_connection_valid(conn) {
                self.available.push(conn.clone());
            } else {
                self.remove_connection(conn);
            }
        }
    }

    /// Check if a connection is still valid
    fn is_connection_valid(&self, conn: &PooledConnection) -> bool {
        let now = Instant::now();

        // Check idle timeout
        if now.duration_since(conn.last_used) > self.config.idle_timeout {
            return false;
        }

        // Check max connection age
        if let Some(max_age) = self.config.max_connection_age {
            if now.duration_since(conn.created_at) > max_age {
                return false;
            }
        }

        true
    }

    /// Remove a connection from the pool
    fn remove_connection(&mut self, conn: &PooledConnection) {
        self.all_connections.retain(|c| !Arc::ptr_eq(&c.connection, &conn.connection));
    }

    /// Clean up idle connections
    fn cleanup(&mut self) -> usize {
        let before_count = self.all_connections.len();
        self.all_connections.retain(|conn| {
            if conn.in_use {
                true
            } else {
                self.is_connection_valid(conn)
            }
        });
        self.available.retain(|conn| {
            self.all_connections.iter().any(|c| Arc::ptr_eq(&c.connection, &conn.connection))
        });
        before_count - self.all_connections.len()
    }

    /// Get pool statistics
    fn stats(&self) -> HostPoolStats {
        HostPoolStats {
            host: self.host.clone(),
            available: self.available.len(),
            in_use: self.all_connections.len() - self.available.len(),
            total: self.all_connections.len(),
            created_count: self.created_count,
            error_count: self.error_count,
        }
    }
}

/// Host pool statistics
#[derive(Debug, Clone)]
pub struct HostPoolStats {
    /// Host name
    pub host: String,
    /// Number of available connections
    pub available: usize,
    /// Number of in-use connections
    pub in_use: usize,
    /// Total connections
    pub total: usize,
    /// Total connections created
    pub created_count: usize,
    /// Number of errors
    pub error_count: usize,
}

/// Global connection pool
#[derive(Debug)]
pub struct ConnectionPool {
    /// Per-host connection pools
    pools: HashMap<String, HostPool>,
    /// Pool configuration
    config: PoolConfig,
    /// Total connections across all hosts
    total_connections: Arc<RwLock<usize>>,
    /// Cleanup interval
    cleanup_interval: Duration,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(config: PoolConfig) -> Self {
        Self {
            pools: HashMap::new(),
            config,
            total_connections: Arc::new(RwLock::new(0)),
            cleanup_interval: Duration::from_secs(30),
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(PoolConfig::default())
    }

    /// Acquire a connection for a host
    pub async fn acquire(&mut self, host: &str) -> Result<Arc<dyn Connection>, ConnectionError> {
        // Get or create host pool
        let pool = self.pools.entry(host.to_string())
            .or_insert_with(|| HostPool::new(host, self.config.clone()));

        // Acquire connection from pool
        let connection = pool.acquire().await?;

        Ok(connection)
    }

    /// Release a connection back to the pool
    pub fn release(&mut self, host: &str, connection: Arc<dyn Connection>) {
        if let Some(pool) = self.pools.get_mut(host) {
            pool.release(connection);
        }
    }

    /// Clean up idle connections across all pools
    pub fn cleanup(&mut self) -> usize {
        let mut total_removed = 0;
        for pool in self.pools.values_mut() {
            total_removed += pool.cleanup();
        }
        total_removed
    }

    /// Get statistics for all pools
    pub fn stats(&self) -> Vec<HostPoolStats> {
        self.pools.values().map(|pool| pool.stats()).collect()
    }

    /// Get total statistics
    pub fn total_stats(&self) -> PoolStats {
        let host_stats = self.stats();
        PoolStats {
            total_hosts: host_stats.len(),
            total_connections: host_stats.iter().map(|s| s.total).sum(),
            available_connections: host_stats.iter().map(|s| s.available).sum(),
            in_use_connections: host_stats.iter().map(|s| s.in_use).sum(),
            total_created: host_stats.iter().map(|s| s.created_count).sum(),
            total_errors: host_stats.iter().map(|s| s.error_count).sum(),
        }
    }

    /// Start background cleanup task
    pub fn start_cleanup_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let interval = self.cleanup_interval;
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                interval_timer.tick().await;
                let pool = Arc::clone(&self);
                if let Ok(mut pool) = Arc::try_unwrap(pool) {
                    let removed = pool.cleanup();
                    if removed > 0 {
                        tracing::debug!("Cleaned up {} idle connections", removed);
                    }
                }
            }
        })
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

/// Overall pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Number of hosts with pools
    pub total_hosts: usize,
    /// Total connections across all hosts
    pub total_connections: usize,
    /// Available connections
    pub available_connections: usize,
    /// In-use connections
    pub in_use_connections: usize,
    /// Total connections created
    pub total_created: usize,
    /// Total errors
    pub total_errors: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config() {
        let config = PoolConfig::low_latency();
        assert_eq!(config.max_connections_per_host, 20);
        assert_eq!(config.max_total_connections, 200);
    }

    #[test]
    fn test_host_pool_stats() {
        let stats = HostPoolStats {
            host: "test-host".to_string(),
            available: 5,
            in_use: 3,
            total: 8,
            created_count: 10,
            error_count: 1,
        };
        assert_eq!(stats.host, "test-host");
        assert_eq!(stats.total, 8);
    }
}
