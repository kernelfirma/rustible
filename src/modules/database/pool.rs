//! Connection pooling for database operations
//!
//! This module provides a connection pool manager for MySQL databases,
//! offering efficient connection reuse and management.
//!
//! # Features
//!
//! - Configurable pool size (min/max connections)
//! - Connection health checking
//! - Automatic connection recovery
//! - Idle connection timeout
//! - Connection acquire timeout
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::modules::database::pool::{DatabasePool, PoolConfig};
//!
//! let config = PoolConfig::default()
//!     .with_max_connections(10)
//!     .with_min_connections(2)
//!     .with_acquire_timeout(30);
//!
//! let pool = DatabasePool::new("mysql://user:pass@localhost/db", config).await?;
//! let result = pool.execute("SELECT 1").await?;
//! # Ok(())
//! # }
//! ```

use parking_lot::RwLock;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions, MySqlRow};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::DatabaseError;

/// Configuration for the database connection pool
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool
    pub max_connections: u32,
    /// Minimum number of connections to maintain
    pub min_connections: u32,
    /// Timeout for acquiring a connection from the pool (seconds)
    pub acquire_timeout: u64,
    /// Maximum time a connection can be idle before being closed (seconds)
    pub idle_timeout: u64,
    /// Maximum lifetime of a connection (seconds)
    pub max_lifetime: u64,
    /// Whether to test connections before use
    pub test_before_acquire: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            acquire_timeout: 30,
            idle_timeout: 600,  // 10 minutes
            max_lifetime: 1800, // 30 minutes
            test_before_acquire: true,
        }
    }
}

impl PoolConfig {
    /// Create a new pool configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum number of connections
    pub fn with_max_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }

    /// Set minimum number of connections
    pub fn with_min_connections(mut self, min: u32) -> Self {
        self.min_connections = min;
        self
    }

    /// Set acquire timeout in seconds
    pub fn with_acquire_timeout(mut self, timeout: u64) -> Self {
        self.acquire_timeout = timeout;
        self
    }

    /// Set idle timeout in seconds
    pub fn with_idle_timeout(mut self, timeout: u64) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Set max lifetime in seconds
    pub fn with_max_lifetime(mut self, lifetime: u64) -> Self {
        self.max_lifetime = lifetime;
        self
    }

    /// Enable/disable connection testing before acquire
    pub fn with_test_before_acquire(mut self, test: bool) -> Self {
        self.test_before_acquire = test;
        self
    }

    /// Create a production-ready configuration
    pub fn production() -> Self {
        Self {
            max_connections: 20,
            min_connections: 5,
            acquire_timeout: 30,
            idle_timeout: 300,
            max_lifetime: 3600,
            test_before_acquire: true,
        }
    }

    /// Create a development configuration (smaller pool)
    pub fn development() -> Self {
        Self {
            max_connections: 5,
            min_connections: 1,
            acquire_timeout: 10,
            idle_timeout: 60,
            max_lifetime: 600,
            test_before_acquire: false,
        }
    }
}

/// A managed database connection pool
pub struct DatabasePool {
    pool: MySqlPool,
    config: PoolConfig,
}

impl DatabasePool {
    /// Create a new database pool with the given connection URL and configuration
    pub async fn new(connection_url: &str, config: PoolConfig) -> Result<Self, DatabaseError> {
        let pool = MySqlPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(Duration::from_secs(config.acquire_timeout))
            .idle_timeout(Some(Duration::from_secs(config.idle_timeout)))
            .max_lifetime(Some(Duration::from_secs(config.max_lifetime)))
            .test_before_acquire(config.test_before_acquire)
            .connect(connection_url)
            .await
            .map_err(|e| DatabaseError::ConnectionFailed(e.to_string()))?;

        Ok(Self { pool, config })
    }

    /// Get a reference to the underlying pool
    pub fn inner(&self) -> &MySqlPool {
        &self.pool
    }

    /// Get the pool configuration
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// Execute a query that returns rows
    pub async fn fetch_all(&self, query: &str) -> Result<Vec<MySqlRow>, DatabaseError> {
        sqlx::query(query)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryFailed(e.to_string()))
    }

    /// Execute a query that returns a single row
    pub async fn fetch_one(&self, query: &str) -> Result<MySqlRow, DatabaseError> {
        sqlx::query(query)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryFailed(e.to_string()))
    }

    /// Execute a query that returns an optional row
    pub async fn fetch_optional(&self, query: &str) -> Result<Option<MySqlRow>, DatabaseError> {
        sqlx::query(query)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryFailed(e.to_string()))
    }

    /// Execute a query that doesn't return rows
    pub async fn execute(&self, query: &str) -> Result<u64, DatabaseError> {
        let result = sqlx::query(query)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryFailed(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Check if the connection is healthy
    pub async fn is_healthy(&self) -> bool {
        self.execute("SELECT 1").await.is_ok()
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            size: self.pool.size(),
            num_idle: self.pool.num_idle(),
            max_connections: self.config.max_connections,
            min_connections: self.config.min_connections,
        }
    }

    /// Close the pool
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Statistics about the connection pool
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Current number of connections in the pool
    pub size: u32,
    /// Number of idle connections
    pub num_idle: usize,
    /// Maximum connections allowed
    pub max_connections: u32,
    /// Minimum connections to maintain
    pub min_connections: u32,
}

/// Global pool manager for caching and reusing connection pools
pub struct PoolManager {
    pools: RwLock<HashMap<String, Arc<DatabasePool>>>,
    default_config: PoolConfig,
}

impl PoolManager {
    /// Create a new pool manager
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            default_config: PoolConfig::default(),
        }
    }

    /// Create a pool manager with custom default configuration
    pub fn with_config(config: PoolConfig) -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            default_config: config,
        }
    }

    /// Get or create a pool for the given connection URL
    pub async fn get_or_create(
        &self,
        connection_url: &str,
    ) -> Result<Arc<DatabasePool>, DatabaseError> {
        // First, try to get from cache with read lock
        {
            let pools = self.pools.read();
            if let Some(pool) = pools.get(connection_url) {
                return Ok(Arc::clone(pool));
            }
        }

        // Create new pool
        let pool = DatabasePool::new(connection_url, self.default_config.clone()).await?;
        let pool = Arc::new(pool);

        // Insert into cache with write lock
        {
            let mut pools = self.pools.write();
            // Double-check in case another thread created it
            if let Some(existing) = pools.get(connection_url) {
                return Ok(Arc::clone(existing));
            }
            pools.insert(connection_url.to_string(), Arc::clone(&pool));
        }

        Ok(pool)
    }

    /// Get a pool for the given connection URL with custom configuration
    pub async fn get_with_config(
        &self,
        connection_url: &str,
        config: PoolConfig,
    ) -> Result<Arc<DatabasePool>, DatabaseError> {
        let pool = DatabasePool::new(connection_url, config).await?;
        Ok(Arc::new(pool))
    }

    /// Remove a pool from the cache
    pub async fn remove(&self, connection_url: &str) -> Option<Arc<DatabasePool>> {
        let pool = {
            let mut pools = self.pools.write();
            pools.remove(connection_url)
        };

        if let Some(ref p) = pool {
            p.close().await;
        }

        pool
    }

    /// Close all pools
    pub async fn close_all(&self) {
        let pools: Vec<Arc<DatabasePool>> = {
            let mut pools_guard = self.pools.write();
            pools_guard.drain().map(|(_, p)| p).collect()
        };

        for pool in pools {
            pool.close().await;
        }
    }

    /// Get statistics for all pools
    pub fn all_stats(&self) -> HashMap<String, PoolStats> {
        let pools = self.pools.read();
        pools
            .iter()
            .map(|(url, pool)| (url.clone(), pool.stats()))
            .collect()
    }
}

impl Default for PoolManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global pool manager instance
static POOL_MANAGER: once_cell::sync::Lazy<PoolManager> =
    once_cell::sync::Lazy::new(PoolManager::new);

/// Get the global pool manager
pub fn global_pool_manager() -> &'static PoolManager {
    &POOL_MANAGER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 1);
        assert_eq!(config.acquire_timeout, 30);
    }

    #[test]
    fn test_pool_config_builder() {
        let config = PoolConfig::new()
            .with_max_connections(20)
            .with_min_connections(5)
            .with_acquire_timeout(60)
            .with_idle_timeout(300)
            .with_test_before_acquire(false);

        assert_eq!(config.max_connections, 20);
        assert_eq!(config.min_connections, 5);
        assert_eq!(config.acquire_timeout, 60);
        assert_eq!(config.idle_timeout, 300);
        assert!(!config.test_before_acquire);
    }

    #[test]
    fn test_pool_config_production() {
        let config = PoolConfig::production();
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.min_connections, 5);
        assert!(config.test_before_acquire);
    }

    #[test]
    fn test_pool_config_development() {
        let config = PoolConfig::development();
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.min_connections, 1);
        assert!(!config.test_before_acquire);
    }
}
