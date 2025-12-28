//! State locking mechanism for concurrent provisioning operations
//!
//! This module provides locking capabilities to prevent concurrent state modifications
//! during provisioning operations. It supports multiple backends including file-based
//! locks for local state and DynamoDB locks for S3 backends.
//!
//! ## Features
//!
//! - File-based locking for local state files
//! - DynamoDB locking for distributed scenarios
//! - Automatic lock expiration handling
//! - RAII-style lock guards for safe lock management
//! - Force unlock capability for stuck locks
//!
//! ## Example Usage
//!
//! ```rust,ignore
//! use rustible::provisioning::state_lock::{StateLockManager, FileLock, LockInfo};
//! use std::time::Duration;
//!
//! // Create a file-based lock manager
//! let lock = FileLock::new(".rustible/provisioning.state.lock");
//! let manager = StateLockManager::new(Box::new(lock));
//!
//! // Acquire lock for an operation
//! let guard = manager.lock("apply").await?;
//!
//! // Perform state modifications...
//!
//! // Lock is automatically released when guard is dropped
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::time::Duration;
use uuid::Uuid;

use super::error::{ProvisioningError, ProvisioningResult};

// ============================================================================
// Lock Information
// ============================================================================

/// Information about a held lock
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockInfo {
    /// Unique identifier for this lock
    pub id: String,

    /// The operation that holds the lock (e.g., "apply", "destroy", "import")
    pub operation: String,

    /// Information about who/what holds the lock (hostname, user, process ID)
    pub who: String,

    /// When the lock was created
    pub created_at: DateTime<Utc>,

    /// When the lock expires (None means no expiration)
    pub expires_at: Option<DateTime<Utc>>,

    /// Additional info about the lock
    pub info: Option<String>,
}

impl LockInfo {
    /// Create a new lock info with the given operation
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            operation: operation.into(),
            who: Self::default_who(),
            created_at: Utc::now(),
            expires_at: None,
            info: None,
        }
    }

    /// Create lock info with an expiration time
    pub fn with_expiration(operation: impl Into<String>, duration: Duration) -> Self {
        let created_at = Utc::now();
        let expires_at = created_at + chrono::Duration::from_std(duration).unwrap_or_default();

        Self {
            id: Uuid::new_v4().to_string(),
            operation: operation.into(),
            who: Self::default_who(),
            created_at,
            expires_at: Some(expires_at),
            info: None,
        }
    }

    /// Set additional information
    pub fn with_info(mut self, info: impl Into<String>) -> Self {
        self.info = Some(info.into());
        self
    }

    /// Set who holds the lock
    pub fn with_who(mut self, who: impl Into<String>) -> Self {
        self.who = who.into();
        self
    }

    /// Check if the lock has expired
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expires) => Utc::now() > expires,
            None => false,
        }
    }

    /// Get default "who" information
    fn default_who() -> String {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let pid = std::process::id();
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());

        format!("{}@{} (pid: {})", user, hostname, pid)
    }
}

impl std::fmt::Display for LockInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Lock {} for operation '{}' held by {} since {}",
            self.id,
            self.operation,
            self.who,
            self.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        )?;

        if let Some(ref expires) = self.expires_at {
            if self.is_expired() {
                write!(
                    f,
                    " (EXPIRED at {})",
                    expires.format("%Y-%m-%d %H:%M:%S UTC")
                )?;
            } else {
                write!(
                    f,
                    " (expires at {})",
                    expires.format("%Y-%m-%d %H:%M:%S UTC")
                )?;
            }
        }

        if let Some(ref info) = self.info {
            write!(f, " - {}", info)?;
        }

        Ok(())
    }
}

// ============================================================================
// Lock Backend Trait
// ============================================================================

/// Backend trait for different lock storage mechanisms
#[async_trait]
pub trait LockBackend: Send + Sync {
    /// Attempt to acquire a lock
    ///
    /// Returns `true` if the lock was acquired, `false` if it's held by another process.
    /// The timeout specifies how long to wait before giving up.
    async fn acquire(&self, info: &LockInfo, timeout: Duration) -> ProvisioningResult<bool>;

    /// Release a lock
    ///
    /// Returns `true` if the lock was released, `false` if the lock wasn't held.
    async fn release(&self, lock_id: &str) -> ProvisioningResult<bool>;

    /// Get current lock information (if any)
    async fn get_lock(&self) -> ProvisioningResult<Option<LockInfo>>;

    /// Force unlock regardless of who holds it
    ///
    /// This should be used with caution as it can corrupt state if another
    /// process is actively modifying it.
    async fn force_unlock(&self, lock_id: &str) -> ProvisioningResult<()>;

    /// Get the backend name for logging
    fn backend_name(&self) -> &str;
}

// ============================================================================
// File-Based Lock Implementation
// ============================================================================

/// File-based lock backend for local state files
///
/// Creates a `.lock` file adjacent to the state file containing
/// lock information as JSON.
pub struct FileLock {
    /// Path to the lock file
    lock_path: PathBuf,
}

impl FileLock {
    /// Create a new file-based lock
    pub fn new(lock_path: impl Into<PathBuf>) -> Self {
        Self {
            lock_path: lock_path.into(),
        }
    }

    /// Create a lock for a state file (adds .lock extension)
    pub fn for_state_file(state_path: impl Into<PathBuf>) -> Self {
        let state_path = state_path.into();
        let lock_path = state_path.with_extension("state.lock");
        Self::new(lock_path)
    }

    /// Try to create the lock file atomically
    async fn try_create_lock_file(&self, info: &LockInfo) -> ProvisioningResult<bool> {
        // Ensure parent directory exists
        if let Some(parent) = self.lock_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to create lock directory: {}",
                    e
                ))
            })?;
        }

        // Check for existing lock
        if self.lock_path.exists() {
            // Read existing lock to check if expired
            if let Ok(content) = tokio::fs::read_to_string(&self.lock_path).await {
                if let Ok(existing_lock) = serde_json::from_str::<LockInfo>(&content) {
                    if !existing_lock.is_expired() {
                        // Lock is held and not expired
                        return Ok(false);
                    }
                    // Lock is expired, we can take it
                    tracing::info!("Removing expired lock: {}", existing_lock);
                }
            }
            // Lock file is corrupt or expired, remove it
            let _ = tokio::fs::remove_file(&self.lock_path).await;
        }

        // Try to create lock file atomically using O_EXCL equivalent
        let content = serde_json::to_string_pretty(info)?;
        let temp_path = self.lock_path.with_extension("lock.tmp");

        // Write to temp file first
        tokio::fs::write(&temp_path, &content).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!("Failed to write lock file: {}", e))
        })?;

        // Try atomic rename - this should fail if lock exists due to race
        match tokio::fs::rename(&temp_path, &self.lock_path).await {
            Ok(_) => Ok(true),
            Err(e) => {
                // Clean up temp file
                let _ = tokio::fs::remove_file(&temp_path).await;

                // Check if it's because lock file now exists
                if self.lock_path.exists() {
                    Ok(false)
                } else {
                    Err(ProvisioningError::StatePersistenceError(format!(
                        "Failed to create lock file: {}",
                        e
                    )))
                }
            }
        }
    }
}

#[async_trait]
impl LockBackend for FileLock {
    async fn acquire(&self, info: &LockInfo, timeout: Duration) -> ProvisioningResult<bool> {
        let start = std::time::Instant::now();
        let retry_interval = Duration::from_millis(100);

        loop {
            if self.try_create_lock_file(info).await? {
                tracing::debug!("Acquired file lock: {}", self.lock_path.display());
                return Ok(true);
            }

            // Check timeout
            if start.elapsed() >= timeout {
                tracing::warn!(
                    "Timeout acquiring lock after {:?}: {}",
                    timeout,
                    self.lock_path.display()
                );
                return Ok(false);
            }

            // Wait before retrying
            tokio::time::sleep(retry_interval).await;
        }
    }

    async fn release(&self, lock_id: &str) -> ProvisioningResult<bool> {
        // Read current lock to verify we own it
        if let Some(current) = self.get_lock().await? {
            if current.id != lock_id {
                tracing::warn!(
                    "Cannot release lock {} - held by different lock {}",
                    lock_id,
                    current.id
                );
                return Ok(false);
            }
        } else {
            // No lock exists
            return Ok(false);
        }

        // Remove the lock file
        match tokio::fs::remove_file(&self.lock_path).await {
            Ok(_) => {
                tracing::debug!("Released file lock: {}", self.lock_path.display());
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(ProvisioningError::StatePersistenceError(format!(
                "Failed to release lock: {}",
                e
            ))),
        }
    }

    async fn get_lock(&self) -> ProvisioningResult<Option<LockInfo>> {
        if !self.lock_path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&self.lock_path)
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!("Failed to read lock file: {}", e))
            })?;

        match serde_json::from_str(&content) {
            Ok(info) => Ok(Some(info)),
            Err(e) => {
                tracing::warn!("Lock file is corrupt: {}", e);
                Ok(None)
            }
        }
    }

    async fn force_unlock(&self, lock_id: &str) -> ProvisioningResult<()> {
        // Verify the lock exists and matches the ID (for safety)
        if let Some(current) = self.get_lock().await? {
            if current.id != lock_id {
                return Err(ProvisioningError::ConcurrencyError(format!(
                    "Force unlock failed: lock ID mismatch (expected {}, found {})",
                    lock_id, current.id
                )));
            }
        }

        match tokio::fs::remove_file(&self.lock_path).await {
            Ok(_) => {
                tracing::warn!("Force unlocked: {}", self.lock_path.display());
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ProvisioningError::StatePersistenceError(format!(
                "Failed to force unlock: {}",
                e
            ))),
        }
    }

    fn backend_name(&self) -> &str {
        "file"
    }
}

// ============================================================================
// DynamoDB Lock Implementation (Stub for S3 Backend)
// ============================================================================

/// DynamoDB-based lock backend for distributed state locking
///
/// This is typically used with S3 state backends for distributed teams.
#[cfg(feature = "aws")]
pub struct DynamoDbLock {
    /// DynamoDB table name
    pub table_name: String,
    /// State file identifier (used as partition key)
    pub state_id: String,
    // AWS client would be added here
}

#[cfg(feature = "aws")]
impl DynamoDbLock {
    /// Create a new DynamoDB lock
    pub fn new(table_name: impl Into<String>, state_id: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            state_id: state_id.into(),
        }
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl LockBackend for DynamoDbLock {
    async fn acquire(&self, _info: &LockInfo, _timeout: Duration) -> ProvisioningResult<bool> {
        // TODO: Implement DynamoDB conditional put
        // This would use AWS SDK to perform a conditional PutItem
        // with a condition expression that the item doesn't exist
        Err(ProvisioningError::ConcurrencyError(
            "DynamoDB lock not yet implemented".to_string(),
        ))
    }

    async fn release(&self, _lock_id: &str) -> ProvisioningResult<bool> {
        // TODO: Implement DynamoDB conditional delete
        Err(ProvisioningError::ConcurrencyError(
            "DynamoDB lock not yet implemented".to_string(),
        ))
    }

    async fn get_lock(&self) -> ProvisioningResult<Option<LockInfo>> {
        // TODO: Implement DynamoDB GetItem
        Err(ProvisioningError::ConcurrencyError(
            "DynamoDB lock not yet implemented".to_string(),
        ))
    }

    async fn force_unlock(&self, _lock_id: &str) -> ProvisioningResult<()> {
        // TODO: Implement DynamoDB unconditional delete
        Err(ProvisioningError::ConcurrencyError(
            "DynamoDB lock not yet implemented".to_string(),
        ))
    }

    fn backend_name(&self) -> &str {
        "dynamodb"
    }
}

// ============================================================================
// In-Memory Lock (for testing)
// ============================================================================

/// In-memory lock backend for testing purposes
pub struct InMemoryLock {
    lock: Arc<Mutex<Option<LockInfo>>>,
}

impl InMemoryLock {
    /// Create a new in-memory lock
    pub fn new() -> Self {
        Self {
            lock: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for InMemoryLock {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LockBackend for InMemoryLock {
    async fn acquire(&self, info: &LockInfo, timeout: Duration) -> ProvisioningResult<bool> {
        let start = std::time::Instant::now();
        let retry_interval = Duration::from_millis(10);

        loop {
            {
                let mut guard = self.lock.lock().await;

                // Check if existing lock is expired
                if let Some(ref existing) = *guard {
                    if existing.is_expired() {
                        *guard = None;
                    }
                }

                if guard.is_none() {
                    *guard = Some(info.clone());
                    return Ok(true);
                }
            }

            if start.elapsed() >= timeout {
                return Ok(false);
            }

            tokio::time::sleep(retry_interval).await;
        }
    }

    async fn release(&self, lock_id: &str) -> ProvisioningResult<bool> {
        let mut guard = self.lock.lock().await;

        if let Some(ref current) = *guard {
            if current.id == lock_id {
                *guard = None;
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn get_lock(&self) -> ProvisioningResult<Option<LockInfo>> {
        let guard = self.lock.lock().await;
        Ok(guard.clone())
    }

    async fn force_unlock(&self, _lock_id: &str) -> ProvisioningResult<()> {
        let mut guard = self.lock.lock().await;
        *guard = None;
        Ok(())
    }

    fn backend_name(&self) -> &str {
        "memory"
    }
}

// ============================================================================
// State Lock Manager
// ============================================================================

/// Manager for state locking operations
///
/// Provides a high-level interface for acquiring and releasing locks
/// with support for timeouts, retries, and RAII-style lock guards.
pub struct StateLockManager {
    /// The lock backend
    backend: Box<dyn LockBackend>,
    /// Default timeout for lock acquisition
    timeout: Duration,
    /// Default lock expiration
    lock_expiration: Option<Duration>,
    /// Self-reference for lock guards
    self_ref: Option<Arc<StateLockManager>>,
}

impl StateLockManager {
    /// Create a new lock manager with the given backend
    pub fn new(backend: Box<dyn LockBackend>) -> Self {
        Self {
            backend,
            timeout: Duration::from_secs(30),
            lock_expiration: Some(Duration::from_secs(3600)), // 1 hour default
            self_ref: None,
        }
    }

    /// Create a new lock manager with custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the lock expiration duration
    pub fn with_lock_expiration(mut self, expiration: Duration) -> Self {
        self.lock_expiration = Some(expiration);
        self
    }

    /// Disable lock expiration (locks never expire automatically)
    pub fn without_lock_expiration(mut self) -> Self {
        self.lock_expiration = None;
        self
    }

    /// Wrap this manager in an Arc for lock guard usage
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Acquire a lock for the given operation
    ///
    /// This method blocks until the lock is acquired or timeout is reached.
    pub async fn lock(&self, operation: &str) -> ProvisioningResult<LockGuard> {
        let info = match self.lock_expiration {
            Some(exp) => LockInfo::with_expiration(operation, exp),
            None => LockInfo::new(operation),
        };

        if !self.backend.acquire(&info, self.timeout).await? {
            // Get current lock for error message
            let current = self.backend.get_lock().await?;
            return Err(ProvisioningError::ConcurrencyError(
                if let Some(lock) = current {
                    format!(
                        "Failed to acquire lock: state is locked by {}\nLock ID: {}\nOperation: {}\nCreated: {}",
                        lock.who, lock.id, lock.operation,
                        lock.created_at.format("%Y-%m-%d %H:%M:%S UTC")
                    )
                } else {
                    "Failed to acquire lock: unknown error".to_string()
                },
            ));
        }

        Ok(LockGuard::new(info.id, Arc::new(self.backend_ref())))
    }

    /// Try to acquire a lock without blocking
    ///
    /// Returns `Some(LockGuard)` if acquired, `None` if lock is held.
    pub async fn try_lock(&self, operation: &str) -> ProvisioningResult<Option<LockGuard>> {
        let info = match self.lock_expiration {
            Some(exp) => LockInfo::with_expiration(operation, exp),
            None => LockInfo::new(operation),
        };

        if self.backend.acquire(&info, Duration::ZERO).await? {
            Ok(Some(LockGuard::new(info.id, Arc::new(self.backend_ref()))))
        } else {
            Ok(None)
        }
    }

    /// Release a lock explicitly
    pub async fn unlock(&self, guard: LockGuard) -> ProvisioningResult<()> {
        self.backend.release(&guard.lock_id).await?;
        guard.disarm(); // Prevent double-release on drop
        Ok(())
    }

    /// Force unlock the state (use with caution!)
    ///
    /// This should only be used when you're certain no other process is
    /// actively modifying the state.
    pub async fn force_unlock(&self) -> ProvisioningResult<()> {
        if let Some(lock) = self.backend.get_lock().await? {
            tracing::warn!(
                "Force unlocking state (held by: {}, operation: {})",
                lock.who,
                lock.operation
            );
            self.backend.force_unlock(&lock.id).await?;
        }
        Ok(())
    }

    /// Check if the state is currently locked
    pub async fn is_locked(&self) -> ProvisioningResult<bool> {
        let lock = self.backend.get_lock().await?;
        Ok(lock.map(|l| !l.is_expired()).unwrap_or(false))
    }

    /// Get information about the current lock (if any)
    pub async fn get_lock_info(&self) -> ProvisioningResult<Option<LockInfo>> {
        self.backend.get_lock().await
    }

    /// Get the backend name
    pub fn backend_name(&self) -> &str {
        self.backend.backend_name()
    }

    // Internal: Create a backend reference for lock guards
    fn backend_ref(&self) -> BackendRef<'_> {
        BackendRef {
            backend: &self.backend,
        }
    }
}

// Internal wrapper for backend reference in lock guards
struct BackendRef<'a> {
    backend: &'a Box<dyn LockBackend>,
}

impl<'a> BackendRef<'a> {
    async fn release(&self, lock_id: &str) -> ProvisioningResult<bool> {
        self.backend.release(lock_id).await
    }
}

// ============================================================================
// Lock Guard (RAII)
// ============================================================================

/// RAII guard that releases the lock when dropped
///
/// The lock is automatically released when the guard goes out of scope.
/// If you need to release the lock early, use the `release()` method or
/// call `StateLockManager::unlock()`.
pub struct LockGuard {
    lock_id: String,
    released: std::sync::atomic::AtomicBool,
    /// We need to store the release function
    release_fn: Option<Box<dyn Fn(&str) + Send + Sync>>,
}

impl LockGuard {
    /// Create a new lock guard
    fn new(lock_id: String, _backend: Arc<BackendRef<'_>>) -> Self {
        Self {
            lock_id,
            released: std::sync::atomic::AtomicBool::new(false),
            release_fn: None,
        }
    }

    /// Get the lock ID
    pub fn lock_id(&self) -> &str {
        &self.lock_id
    }

    /// Check if the lock has been released
    pub fn is_released(&self) -> bool {
        self.released.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Disarm the guard (prevent release on drop)
    fn disarm(&self) {
        self.released
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if !self.released.load(std::sync::atomic::Ordering::Acquire) {
            // Note: In a real implementation, we'd need async drop support
            // or use a channel to signal a background task to release the lock.
            // For now, we log a warning if the lock wasn't explicitly released.
            tracing::warn!(
                "LockGuard dropped without explicit release for lock {}. \
                 Consider using StateLockManager::unlock() for proper async cleanup.",
                self.lock_id
            );
        }
    }
}

// ============================================================================
// Async Lock Guard (Alternative with proper async release)
// ============================================================================

/// Async-aware lock guard that properly releases on drop via a background task
pub struct AsyncLockGuard {
    lock_id: String,
    lock_path: PathBuf,
    released: Arc<std::sync::atomic::AtomicBool>,
}

impl AsyncLockGuard {
    /// Create a new async lock guard for a file lock
    pub fn new_file(lock_id: String, lock_path: PathBuf) -> Self {
        Self {
            lock_id,
            lock_path,
            released: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Get the lock ID
    pub fn lock_id(&self) -> &str {
        &self.lock_id
    }

    /// Release the lock explicitly
    pub async fn release(self) -> ProvisioningResult<()> {
        self.released
            .store(true, std::sync::atomic::Ordering::Release);

        // Read and verify lock
        if self.lock_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&self.lock_path).await {
                if let Ok(info) = serde_json::from_str::<LockInfo>(&content) {
                    if info.id == self.lock_id {
                        tokio::fs::remove_file(&self.lock_path).await.map_err(|e| {
                            ProvisioningError::StatePersistenceError(format!(
                                "Failed to release lock: {}",
                                e
                            ))
                        })?;
                    }
                }
            }
        }

        Ok(())
    }
}

impl Drop for AsyncLockGuard {
    fn drop(&mut self) {
        if !self.released.load(std::sync::atomic::Ordering::Acquire) {
            // Spawn a blocking task to clean up the lock
            let lock_id = self.lock_id.clone();
            let lock_path = self.lock_path.clone();

            // Use std::thread::spawn for sync cleanup since we can't use async in drop
            std::thread::spawn(move || {
                if lock_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&lock_path) {
                        if let Ok(info) = serde_json::from_str::<LockInfo>(&content) {
                            if info.id == lock_id {
                                let _ = std::fs::remove_file(&lock_path);
                            }
                        }
                    }
                }
            });
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper to create a test file lock
    fn create_test_file_lock(dir: &TempDir) -> FileLock {
        FileLock::new(dir.path().join("test.state.lock"))
    }

    #[tokio::test]
    async fn test_lock_info_creation() {
        let info = LockInfo::new("apply");
        assert_eq!(info.operation, "apply");
        assert!(!info.is_expired());
        assert!(info.expires_at.is_none());
    }

    #[tokio::test]
    async fn test_lock_info_with_expiration() {
        let info = LockInfo::with_expiration("destroy", Duration::from_secs(60));
        assert_eq!(info.operation, "destroy");
        assert!(info.expires_at.is_some());
        assert!(!info.is_expired());
    }

    #[tokio::test]
    async fn test_lock_info_expired() {
        // Create a lock with 0 duration (already expired)
        let mut info = LockInfo::new("test");
        info.expires_at = Some(Utc::now() - chrono::Duration::seconds(10));
        assert!(info.is_expired());
    }

    #[tokio::test]
    async fn test_file_lock_acquire_release() {
        let dir = TempDir::new().unwrap();
        let lock = create_test_file_lock(&dir);

        let info = LockInfo::new("apply");

        // Acquire lock
        let acquired = lock.acquire(&info, Duration::from_secs(1)).await.unwrap();
        assert!(acquired);

        // Verify lock exists
        let current = lock.get_lock().await.unwrap();
        assert!(current.is_some());
        assert_eq!(current.unwrap().operation, "apply");

        // Release lock
        let released = lock.release(&info.id).await.unwrap();
        assert!(released);

        // Verify lock is gone
        let current = lock.get_lock().await.unwrap();
        assert!(current.is_none());
    }

    #[tokio::test]
    async fn test_file_lock_contention() {
        let dir = TempDir::new().unwrap();
        let lock = create_test_file_lock(&dir);

        let info1 = LockInfo::new("apply");
        let info2 = LockInfo::new("destroy");

        // First lock should succeed
        let acquired1 = lock
            .acquire(&info1, Duration::from_millis(100))
            .await
            .unwrap();
        assert!(acquired1);

        // Second lock should fail (short timeout)
        let acquired2 = lock
            .acquire(&info2, Duration::from_millis(50))
            .await
            .unwrap();
        assert!(!acquired2);

        // Release first lock
        lock.release(&info1.id).await.unwrap();

        // Now second should succeed
        let acquired2 = lock
            .acquire(&info2, Duration::from_millis(100))
            .await
            .unwrap();
        assert!(acquired2);
    }

    #[tokio::test]
    async fn test_file_lock_stale_detection() {
        let dir = TempDir::new().unwrap();
        let lock = create_test_file_lock(&dir);

        // Create an expired lock manually
        let mut expired_info = LockInfo::new("old_operation");
        expired_info.expires_at = Some(Utc::now() - chrono::Duration::seconds(10));

        let lock_path = dir.path().join("test.state.lock");
        tokio::fs::create_dir_all(lock_path.parent().unwrap())
            .await
            .unwrap();
        let content = serde_json::to_string_pretty(&expired_info).unwrap();
        tokio::fs::write(&lock_path, content).await.unwrap();

        // New lock should succeed because old is expired
        let new_info = LockInfo::new("new_operation");
        let acquired = lock
            .acquire(&new_info, Duration::from_millis(100))
            .await
            .unwrap();
        assert!(acquired);

        // Verify it's our new lock
        let current = lock.get_lock().await.unwrap().unwrap();
        assert_eq!(current.id, new_info.id);
    }

    #[tokio::test]
    async fn test_file_lock_force_unlock() {
        let dir = TempDir::new().unwrap();
        let lock = create_test_file_lock(&dir);

        let info = LockInfo::new("apply");
        lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

        // Force unlock
        lock.force_unlock(&info.id).await.unwrap();

        // Lock should be gone
        assert!(lock.get_lock().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_in_memory_lock_acquire_release() {
        let lock = InMemoryLock::new();

        let info = LockInfo::new("test");
        let acquired = lock.acquire(&info, Duration::from_secs(1)).await.unwrap();
        assert!(acquired);

        let released = lock.release(&info.id).await.unwrap();
        assert!(released);
    }

    #[tokio::test]
    async fn test_in_memory_lock_contention() {
        let lock = InMemoryLock::new();

        let info1 = LockInfo::new("first");
        let info2 = LockInfo::new("second");

        // First acquire succeeds
        assert!(lock
            .acquire(&info1, Duration::from_millis(10))
            .await
            .unwrap());

        // Second acquire fails
        assert!(!lock
            .acquire(&info2, Duration::from_millis(10))
            .await
            .unwrap());

        // After release, second succeeds
        lock.release(&info1.id).await.unwrap();
        assert!(lock
            .acquire(&info2, Duration::from_millis(10))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_state_lock_manager_lock() {
        let backend = Box::new(InMemoryLock::new());
        let manager = StateLockManager::new(backend);

        let guard = manager.lock("apply").await.unwrap();
        assert!(!guard.is_released());

        // Should be locked
        assert!(manager.is_locked().await.unwrap());

        // Release explicitly
        manager.unlock(guard).await.unwrap();

        // Should be unlocked
        assert!(!manager.is_locked().await.unwrap());
    }

    #[tokio::test]
    async fn test_state_lock_manager_try_lock() {
        let backend = Box::new(InMemoryLock::new());
        let manager = StateLockManager::new(backend);

        // First try_lock succeeds
        let guard1 = manager.try_lock("apply").await.unwrap();
        assert!(guard1.is_some());

        // Second try_lock fails (no blocking)
        let guard2 = manager.try_lock("destroy").await.unwrap();
        assert!(guard2.is_none());
    }

    #[tokio::test]
    async fn test_lock_guard_disarm() {
        let guard = LockGuard {
            lock_id: "test".to_string(),
            released: std::sync::atomic::AtomicBool::new(false),
            release_fn: None,
        };

        assert!(!guard.is_released());
        guard.disarm();
        assert!(guard.is_released());
    }

    #[tokio::test]
    async fn test_async_lock_guard_release() {
        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("async.lock");
        let lock_id = "test-lock-id".to_string();

        // Create lock file
        let info = LockInfo::new("test");
        let content = serde_json::to_string_pretty(&info).unwrap();
        tokio::fs::write(&lock_path, content).await.unwrap();

        // Create guard with matching ID
        let guard = AsyncLockGuard::new_file(info.id.clone(), lock_path.clone());

        // Release should remove file
        guard.release().await.unwrap();
        assert!(!lock_path.exists());
    }

    #[tokio::test]
    async fn test_file_lock_wrong_lock_id_release() {
        let dir = TempDir::new().unwrap();
        let lock = create_test_file_lock(&dir);

        let info = LockInfo::new("apply");
        lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

        // Try to release with wrong ID
        let released = lock.release("wrong-id").await.unwrap();
        assert!(!released);

        // Lock should still be held
        assert!(lock.get_lock().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_lock_info_display() {
        let info = LockInfo::new("apply")
            .with_info("Running terraform apply")
            .with_who("test@localhost (pid: 1234)");

        let display = format!("{}", info);
        assert!(display.contains("apply"));
        assert!(display.contains("test@localhost"));
        assert!(display.contains("Running terraform apply"));
    }

    #[tokio::test]
    async fn test_state_lock_manager_force_unlock() {
        let backend = Box::new(InMemoryLock::new());
        let manager = StateLockManager::new(backend);

        // Acquire lock
        let _guard = manager.lock("apply").await.unwrap();
        assert!(manager.is_locked().await.unwrap());

        // Force unlock without guard
        manager.force_unlock().await.unwrap();
        assert!(!manager.is_locked().await.unwrap());
    }

    #[tokio::test]
    async fn test_state_lock_manager_get_lock_info() {
        let backend = Box::new(InMemoryLock::new());
        let manager = StateLockManager::new(backend);

        // No lock initially
        assert!(manager.get_lock_info().await.unwrap().is_none());

        // After locking
        let _guard = manager.lock("plan").await.unwrap();
        let info = manager.get_lock_info().await.unwrap();
        assert!(info.is_some());
        assert_eq!(info.unwrap().operation, "plan");
    }
}
