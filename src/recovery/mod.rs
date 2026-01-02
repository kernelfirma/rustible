//! Error Recovery Module for Rustible
//!
//! This module provides comprehensive error recovery mechanisms for playbook execution:
//!
//! - **Retry Policies**: Configurable retry strategies with exponential backoff
//! - **Checkpoints**: Save/resume capability for long-running playbooks
//! - **Rollback**: Partial rollback on failure with state tracking
//! - **Transactions**: Transaction-like semantics for critical operations
//! - **Graceful Degradation**: Patterns for handling partial failures
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Recovery Manager                             │
//! │    (Coordinates retry, checkpoint, rollback, and degradation)        │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!          ┌─────────────────────────┼─────────────────────────────────┐
//!          ▼                         ▼                                 ▼
//! ┌─────────────────┐   ┌─────────────────────┐   ┌─────────────────────┐
//! │  Retry Policy   │   │  Checkpoint Store   │   │  Rollback Manager   │
//! │  (Exponential,  │   │  (Save/Resume/      │   │  (State Tracking,   │
//! │   Linear, etc.) │   │   File-based)       │   │   Undo Actions)     │
//! └─────────────────┘   └─────────────────────┘   └─────────────────────┘
//!          │                         │                                 │
//!          └─────────────────────────┼─────────────────────────────────┘
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Transaction Context                          │
//! │          (ACID-like semantics for critical operations)               │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use rustible::recovery::{RecoveryManager, RetryPolicy, CheckpointConfig};
//!
//! // Configure recovery with retry and checkpointing
//! let recovery = RecoveryManager::builder()
//!     .retry_policy(RetryPolicy::exponential_backoff(3, Duration::from_secs(1)))
//!     .checkpoint_dir("/var/lib/rustible/checkpoints")
//!     .enable_rollback(true)
//!     .build();
//!
//! // Execute with recovery support
//! let result = recovery.execute_with_recovery(|| {
//!     // Your operation here
//! }).await?;
//! ```

pub mod checkpoint;
pub mod degradation;
pub mod retry;
pub mod rollback;
pub mod transaction;

pub use checkpoint::{
    Checkpoint, CheckpointConfig, CheckpointError, CheckpointId, CheckpointStore, PlaybookState,
    TaskProgress,
};
pub use degradation::{
    CircuitBreaker, CircuitBreakerConfig, CircuitState, DegradationLevel, DegradationPolicy,
    FallbackAction, GracefulDegradation,
};
pub use retry::{
    BackoffStrategy, RetryAction, RetryConfig, RetryContext, RetryError, RetryPolicy, RetryResult,
    RetryableError,
};
pub use rollback::{
    RollbackAction, RollbackContext, RollbackError, RollbackManager, RollbackPlan, RollbackState,
    StateChange, StateSnapshot, UndoOperation,
};
pub use transaction::{
    TaskOutcome, Transaction, TransactionConfig, TransactionContext, TransactionError,
    TransactionId, TransactionManager, TransactionState,
};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Error type for recovery operations
#[derive(Error, Debug)]
pub enum RecoveryError {
    #[error("Retry failed after {attempts} attempts: {message}")]
    RetryExhausted { attempts: u32, message: String },

    #[error("Checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),

    #[error("Rollback error: {0}")]
    Rollback(#[from] RollbackError),

    #[error("Transaction error: {0}")]
    Transaction(#[from] TransactionError),

    #[error("Degradation error: {0}")]
    Degradation(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("Recovery not possible: {0}")]
    Unrecoverable(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for recovery operations
pub type RecoveryResult<T> = Result<T, RecoveryError>;

/// Configuration for the recovery manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Enable automatic retry for transient failures
    pub enable_retry: bool,

    /// Default retry policy
    pub retry_policy: RetryPolicy,

    /// Enable checkpointing for long playbooks
    pub enable_checkpoints: bool,

    /// Checkpoint configuration
    pub checkpoint_config: CheckpointConfig,

    /// Enable automatic rollback on failure
    pub enable_rollback: bool,

    /// Enable transaction semantics for critical operations
    pub enable_transactions: bool,

    /// Transaction configuration
    pub transaction_config: TransactionConfig,

    /// Enable graceful degradation
    pub enable_degradation: bool,

    /// Degradation policy
    pub degradation_policy: DegradationPolicy,

    /// Circuit breaker configuration
    pub circuit_breaker_config: CircuitBreakerConfig,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            enable_retry: true,
            retry_policy: RetryPolicy::default(),
            enable_checkpoints: false,
            checkpoint_config: CheckpointConfig::default(),
            enable_rollback: true,
            enable_transactions: false,
            transaction_config: TransactionConfig::default(),
            enable_degradation: true,
            degradation_policy: DegradationPolicy::default(),
            circuit_breaker_config: CircuitBreakerConfig::default(),
        }
    }
}

impl RecoveryConfig {
    /// Create a minimal recovery configuration (retry only)
    pub fn minimal() -> Self {
        Self {
            enable_retry: true,
            retry_policy: RetryPolicy::simple(3),
            enable_checkpoints: false,
            checkpoint_config: CheckpointConfig::default(),
            enable_rollback: false,
            enable_transactions: false,
            transaction_config: TransactionConfig::default(),
            enable_degradation: false,
            degradation_policy: DegradationPolicy::default(),
            circuit_breaker_config: CircuitBreakerConfig::default(),
        }
    }

    /// Create a production recovery configuration with all features
    pub fn production() -> Self {
        Self {
            enable_retry: true,
            retry_policy: RetryPolicy::exponential_backoff(5, Duration::from_secs(1)),
            enable_checkpoints: true,
            checkpoint_config: CheckpointConfig::production(),
            enable_rollback: true,
            enable_transactions: true,
            transaction_config: TransactionConfig::production(),
            enable_degradation: true,
            degradation_policy: DegradationPolicy::default(),
            circuit_breaker_config: CircuitBreakerConfig::default(),
        }
    }
}

/// Main recovery manager that coordinates all recovery mechanisms
pub struct RecoveryManager {
    config: RecoveryConfig,
    checkpoint_store: Option<Arc<RwLock<CheckpointStore>>>,
    rollback_manager: Option<Arc<RwLock<RollbackManager>>>,
    transaction_manager: Option<Arc<RwLock<TransactionManager>>>,
    circuit_breakers: Arc<RwLock<HashMap<String, CircuitBreaker>>>,
    degradation: Option<Arc<GracefulDegradation>>,
}

impl RecoveryManager {
    /// Create a new recovery manager with the given configuration
    pub fn new(config: RecoveryConfig) -> Self {
        let checkpoint_store = if config.enable_checkpoints {
            Some(Arc::new(RwLock::new(CheckpointStore::new(
                config.checkpoint_config.clone(),
            ))))
        } else {
            None
        };

        let rollback_manager = if config.enable_rollback {
            Some(Arc::new(RwLock::new(RollbackManager::new())))
        } else {
            None
        };

        let transaction_manager = if config.enable_transactions {
            Some(Arc::new(RwLock::new(
                TransactionManager::new(config.transaction_config.clone())
                    .expect("Failed to initialize TransactionManager"),
            )))
        } else {
            None
        };

        let degradation = if config.enable_degradation {
            Some(Arc::new(GracefulDegradation::new(
                config.degradation_policy.clone(),
            )))
        } else {
            None
        };

        Self {
            config,
            checkpoint_store,
            rollback_manager,
            transaction_manager,
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            degradation,
        }
    }

    /// Create a builder for configuring the recovery manager
    pub fn builder() -> RecoveryManagerBuilder {
        RecoveryManagerBuilder::new()
    }

    /// Execute an operation with retry support
    pub async fn with_retry<F, T, E>(
        &self,
        operation_name: &str,
        mut operation: F,
    ) -> RecoveryResult<T>
    where
        F: FnMut() -> Result<T, E>,
        E: std::error::Error + RetryableError,
    {
        if !self.config.enable_retry {
            return operation().map_err(|e| RecoveryError::Unrecoverable(e.to_string()));
        }

        let policy = &self.config.retry_policy;
        let mut context = RetryContext::new(operation_name);

        loop {
            match operation() {
                Ok(result) => {
                    if context.attempt > 0 {
                        info!(
                            "Operation '{}' succeeded after {} retries",
                            operation_name, context.attempt
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    let action = policy.should_retry(&context, &e);
                    context.record_attempt(&e);

                    match action {
                        RetryAction::Retry { delay } => {
                            warn!(
                                "Operation '{}' failed (attempt {}), retrying in {:?}: {}",
                                operation_name, context.attempt, delay, e
                            );
                            tokio::time::sleep(delay).await;
                        }
                        RetryAction::Stop { reason } => {
                            error!(
                                "Operation '{}' failed after {} attempts: {} ({})",
                                operation_name, context.attempt, e, reason
                            );
                            return Err(RecoveryError::RetryExhausted {
                                attempts: context.attempt,
                                message: e.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Execute an async operation with retry support
    pub async fn with_retry_async<F, Fut, T, E>(
        &self,
        operation_name: &str,
        mut operation: F,
    ) -> RecoveryResult<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::error::Error + RetryableError,
    {
        if !self.config.enable_retry {
            return operation()
                .await
                .map_err(|e| RecoveryError::Unrecoverable(e.to_string()));
        }

        let policy = &self.config.retry_policy;
        let mut context = RetryContext::new(operation_name);

        loop {
            match operation().await {
                Ok(result) => {
                    if context.attempt > 0 {
                        info!(
                            "Operation '{}' succeeded after {} retries",
                            operation_name, context.attempt
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    let action = policy.should_retry(&context, &e);
                    context.record_attempt(&e);

                    match action {
                        RetryAction::Retry { delay } => {
                            warn!(
                                "Operation '{}' failed (attempt {}), retrying in {:?}: {}",
                                operation_name, context.attempt, delay, e
                            );
                            tokio::time::sleep(delay).await;
                        }
                        RetryAction::Stop { reason } => {
                            error!(
                                "Operation '{}' failed after {} attempts: {} ({})",
                                operation_name, context.attempt, e, reason
                            );
                            return Err(RecoveryError::RetryExhausted {
                                attempts: context.attempt,
                                message: e.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Create a checkpoint for the current playbook state
    pub async fn create_checkpoint(
        &self,
        playbook_name: &str,
        state: PlaybookState,
    ) -> RecoveryResult<CheckpointId> {
        let store = self.checkpoint_store.as_ref().ok_or_else(|| {
            RecoveryError::Unrecoverable("Checkpointing is not enabled".to_string())
        })?;

        let mut store = store.write().await;
        let checkpoint = Checkpoint::new(playbook_name, state);
        let id = store.save(checkpoint)?;

        info!("Created checkpoint {} for playbook '{}'", id, playbook_name);
        Ok(id)
    }

    /// Resume execution from a checkpoint
    pub async fn resume_from_checkpoint(
        &self,
        checkpoint_id: &CheckpointId,
    ) -> RecoveryResult<PlaybookState> {
        let store = self.checkpoint_store.as_ref().ok_or_else(|| {
            RecoveryError::Unrecoverable("Checkpointing is not enabled".to_string())
        })?;

        let store = store.read().await;
        let checkpoint = store.load(checkpoint_id)?;

        info!(
            "Resuming from checkpoint {} for playbook '{}'",
            checkpoint_id, checkpoint.playbook_name
        );
        Ok(checkpoint.state)
    }

    /// List available checkpoints for a playbook
    pub async fn list_checkpoints(&self, playbook_name: &str) -> RecoveryResult<Vec<Checkpoint>> {
        let store = self.checkpoint_store.as_ref().ok_or_else(|| {
            RecoveryError::Unrecoverable("Checkpointing is not enabled".to_string())
        })?;

        let store = store.read().await;
        Ok(store.list_for_playbook(playbook_name))
    }

    /// Begin tracking state changes for potential rollback
    pub async fn begin_rollback_tracking(&self) -> RecoveryResult<RollbackContext> {
        let manager = self
            .rollback_manager
            .as_ref()
            .ok_or_else(|| RecoveryError::Unrecoverable("Rollback is not enabled".to_string()))?;

        let mut manager = manager.write().await;
        let context = manager.begin_context();

        debug!("Started rollback tracking context: {}", context.id);
        Ok(context)
    }

    /// Record a state change that can be rolled back
    pub async fn record_state_change(
        &self,
        context_id: &str,
        change: StateChange,
    ) -> RecoveryResult<()> {
        let manager = self
            .rollback_manager
            .as_ref()
            .ok_or_else(|| RecoveryError::Unrecoverable("Rollback is not enabled".to_string()))?;

        let mut manager = manager.write().await;
        manager.record_change(context_id, change)?;

        Ok(())
    }

    /// Execute a rollback for a context
    pub async fn rollback(&self, context_id: &str) -> RecoveryResult<()> {
        let manager = self
            .rollback_manager
            .as_ref()
            .ok_or_else(|| RecoveryError::Unrecoverable("Rollback is not enabled".to_string()))?;

        let mut manager = manager.write().await;
        let plan = manager.create_rollback_plan(context_id)?;

        info!(
            "Executing rollback plan with {} actions for context {}",
            plan.actions.len(),
            context_id
        );

        for action in plan.actions.iter().rev() {
            debug!("Executing rollback action: {:?}", action);
            manager.execute_rollback_action(action).await?;
        }

        manager.complete_rollback(context_id)?;
        info!("Rollback completed for context {}", context_id);

        Ok(())
    }

    /// Begin a transaction for critical operations
    pub async fn begin_transaction(&self, name: &str) -> RecoveryResult<TransactionId> {
        let manager = self.transaction_manager.as_ref().ok_or_else(|| {
            RecoveryError::Unrecoverable("Transactions are not enabled".to_string())
        })?;

        let manager = manager.write().await;
        let id = manager.begin(name).await?;

        info!("Started transaction '{}' with id {}", name, id);
        Ok(id)
    }

    /// Commit a transaction
    pub async fn commit_transaction(&self, transaction_id: &TransactionId) -> RecoveryResult<()> {
        let manager = self.transaction_manager.as_ref().ok_or_else(|| {
            RecoveryError::Unrecoverable("Transactions are not enabled".to_string())
        })?;

        let manager = manager.write().await;
        manager.commit(transaction_id).await?;

        info!("Committed transaction {}", transaction_id);
        Ok(())
    }

    /// Rollback a transaction
    pub async fn rollback_transaction(&self, transaction_id: &TransactionId) -> RecoveryResult<()> {
        let manager = self.transaction_manager.as_ref().ok_or_else(|| {
            RecoveryError::Unrecoverable("Transactions are not enabled".to_string())
        })?;

        let manager = manager.write().await;
        manager.rollback(transaction_id).await?;

        info!("Rolled back transaction {}", transaction_id);
        Ok(())
    }

    /// Record a task execution within a transaction
    pub async fn record_task(
        &self,
        transaction_id: TransactionId,
        task_name: String,
        host: String,
        status: TaskOutcome,
        changed: bool,
    ) -> RecoveryResult<()> {
        let manager = self.transaction_manager.as_ref().ok_or_else(|| {
            RecoveryError::Unrecoverable("Transactions are not enabled".to_string())
        })?;

        let manager = manager.read().await;
        manager
            .record_task(&transaction_id, &host, &task_name, 0, 0, status, None)
            .await
            .map(|_| ())
            .map_err(RecoveryError::Transaction)
    }

    /// Execute an operation within a transaction
    pub async fn with_transaction<F, Fut, T, E>(
        &self,
        name: &str,
        operation: F,
    ) -> RecoveryResult<T>
    where
        F: FnOnce(TransactionContext) -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::error::Error,
    {
        let tx_id = self.begin_transaction(name).await?;

        let context = TransactionContext::new(tx_id.clone());

        match operation(context).await {
            Ok(result) => {
                self.commit_transaction(&tx_id).await?;
                Ok(result)
            }
            Err(e) => {
                error!("Transaction '{}' failed: {}", name, e);
                self.rollback_transaction(&tx_id).await?;
                Err(RecoveryError::Transaction(
                    TransactionError::OperationFailed(e.to_string()),
                ))
            }
        }
    }

    /// Get or create a circuit breaker for a service
    pub async fn circuit_breaker(&self, service_name: &str) -> CircuitBreaker {
        let mut breakers = self.circuit_breakers.write().await;

        if let Some(breaker) = breakers.get(service_name) {
            return breaker.clone();
        }

        let breaker = CircuitBreaker::new(service_name, self.config.circuit_breaker_config.clone());
        breakers.insert(service_name.to_string(), breaker.clone());
        breaker
    }

    /// Check if an operation should proceed based on degradation level
    pub async fn check_degradation(&self, operation_criticality: u8) -> DegradationLevel {
        if let Some(degradation) = &self.degradation {
            degradation.current_level(operation_criticality).await
        } else {
            DegradationLevel::Normal
        }
    }

    /// Report a failure for degradation tracking
    pub async fn report_failure(&self, service_name: &str) {
        if let Some(degradation) = &self.degradation {
            degradation.report_failure(service_name).await;
        }
    }

    /// Report a success for degradation tracking
    pub async fn report_success(&self, service_name: &str) {
        if let Some(degradation) = &self.degradation {
            degradation.report_success(service_name).await;
        }
    }

    /// Get the current recovery configuration
    pub fn config(&self) -> &RecoveryConfig {
        &self.config
    }
}

/// Builder for RecoveryManager
pub struct RecoveryManagerBuilder {
    config: RecoveryConfig,
}

impl RecoveryManagerBuilder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: RecoveryConfig::default(),
        }
    }

    /// Set the retry policy
    pub fn retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.config.enable_retry = true;
        self.config.retry_policy = policy;
        self
    }

    /// Disable retry
    pub fn no_retry(mut self) -> Self {
        self.config.enable_retry = false;
        self
    }

    /// Enable checkpointing with the given directory
    pub fn checkpoint_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.enable_checkpoints = true;
        self.config.checkpoint_config.checkpoint_dir = dir.into();
        self
    }

    /// Set checkpoint configuration
    pub fn checkpoint_config(mut self, config: CheckpointConfig) -> Self {
        self.config.enable_checkpoints = true;
        self.config.checkpoint_config = config;
        self
    }

    /// Enable rollback
    pub fn enable_rollback(mut self, enable: bool) -> Self {
        self.config.enable_rollback = enable;
        self
    }

    /// Enable transactions
    pub fn enable_transactions(mut self, enable: bool) -> Self {
        self.config.enable_transactions = enable;
        self
    }

    /// Set transaction configuration
    pub fn transaction_config(mut self, config: TransactionConfig) -> Self {
        self.config.enable_transactions = true;
        self.config.transaction_config = config;
        self
    }

    /// Enable graceful degradation
    pub fn enable_degradation(mut self, enable: bool) -> Self {
        self.config.enable_degradation = enable;
        self
    }

    /// Set degradation policy
    pub fn degradation_policy(mut self, policy: DegradationPolicy) -> Self {
        self.config.enable_degradation = true;
        self.config.degradation_policy = policy;
        self
    }

    /// Set circuit breaker configuration
    pub fn circuit_breaker_config(mut self, config: CircuitBreakerConfig) -> Self {
        self.config.circuit_breaker_config = config;
        self
    }

    /// Build the recovery manager
    pub fn build(self) -> RecoveryManager {
        RecoveryManager::new(self.config)
    }
}

impl Default for RecoveryManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_config_default() {
        let config = RecoveryConfig::default();
        assert!(config.enable_retry);
        assert!(!config.enable_checkpoints);
        assert!(config.enable_rollback);
    }

    #[test]
    fn test_recovery_config_minimal() {
        let config = RecoveryConfig::minimal();
        assert!(config.enable_retry);
        assert!(!config.enable_checkpoints);
        assert!(!config.enable_rollback);
    }

    #[test]
    fn test_recovery_config_production() {
        let config = RecoveryConfig::production();
        assert!(config.enable_retry);
        assert!(config.enable_checkpoints);
        assert!(config.enable_rollback);
        assert!(config.enable_transactions);
        assert!(config.enable_degradation);
    }

    #[test]
    fn test_recovery_manager_builder() {
        let manager = RecoveryManager::builder()
            .retry_policy(RetryPolicy::simple(5))
            .enable_rollback(true)
            .build();

        assert!(manager.config.enable_retry);
        assert!(manager.config.enable_rollback);
    }

    #[tokio::test]
    async fn test_recovery_manager_new() {
        let config = RecoveryConfig::default();
        let manager = RecoveryManager::new(config);

        assert!(manager.checkpoint_store.is_none());
        assert!(manager.rollback_manager.is_some());
    }
}
