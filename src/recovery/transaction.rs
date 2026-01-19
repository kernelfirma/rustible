//! Transactional Checkpoint System
//!
//! This module extends the checkpoint system with transactional semantics,
//! providing ACID-like guarantees for playbook execution:
//!
//! - **Atomicity**: All changes within a transaction are applied or none are
//! - **Consistency**: State is always valid between transactions
//! - **Isolation**: Concurrent executions are isolated via savepoints
//! - **Durability**: Committed changes are persisted immediately
//!
//! ## Features
//!
//! - Begin/commit/rollback transaction semantics
//! - Savepoints for partial rollback
//! - Write-ahead logging (WAL) for crash recovery
//! - Automatic transaction management integration with executor
//!
//! ## Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::recovery::transaction::{TaskOutcome, TransactionConfig, TransactionManager};
//!
//! let config = TransactionConfig::default();
//! let manager = TransactionManager::new(config)?;
//!
//! // Begin a transaction
//! let tx_id = manager.begin("playbook.yml").await?;
//!
//! // Create a savepoint before risky operation
//! let sp = manager.savepoint(&tx_id, "before_database_changes").await?;
//!
//! // Execute tasks...
//! manager
//!     .record_task(&tx_id, "host1", "restart_db", 0, 0, TaskOutcome::Success, None)
//!     .await?;
//!
//! // If something fails, rollback to savepoint
//! // manager.rollback_to_savepoint(&sp).await?;
//!
//! // Or commit the entire transaction
//! manager.commit(&tx_id).await?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::checkpoint::{
    Checkpoint, CheckpointConfig, CheckpointError, CheckpointId, CheckpointStore, PlaybookState,
    TaskCheckpointStatus,
};

/// Errors for transaction operations
#[derive(Error, Debug)]
pub enum TransactionError {
    #[error("Transaction not found: {0}")]
    NotFound(String),

    #[error("Transaction already committed: {0}")]
    AlreadyCommitted(String),

    #[error("Transaction already rolled back: {0}")]
    AlreadyRolledBack(String),

    #[error("Savepoint not found: {0}")]
    SavepointNotFound(String),

    #[error("Transaction timeout: {0}")]
    Timeout(String),

    #[error("Checkpoint error: {0}")]
    Checkpoint(#[from] CheckpointError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("WAL corrupted: {0}")]
    WalCorrupted(String),

    #[error("Concurrent modification detected")]
    ConcurrentModification,

    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

/// Result type for transaction operations
pub type TransactionResult<T> = Result<T, TransactionError>;

/// Unique identifier for a transaction
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(String);

impl TransactionId {
    /// Create a new transaction ID
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let random: u32 = rand::random();
        Self(format!("tx-{}-{:08x}", timestamp, random))
    }

    /// Get the ID as a string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TransactionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TransactionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a savepoint
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SavepointId {
    transaction_id: TransactionId,
    name: String,
    sequence: u64,
}

impl SavepointId {
    /// Get the transaction ID this savepoint belongs to
    pub fn transaction_id(&self) -> &TransactionId {
        &self.transaction_id
    }

    /// Get the savepoint name
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl std::fmt::Display for SavepointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.transaction_id, self.name)
    }
}

/// Context for an active transaction
#[derive(Debug, Clone)]
pub struct TransactionContext {
    pub id: TransactionId,
}

impl TransactionContext {
    pub fn new(id: TransactionId) -> Self {
        Self { id }
    }
}

/// Configuration for transaction management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionConfig {
    /// Base directory for transaction data
    pub data_dir: PathBuf,
    /// Enable write-ahead logging
    pub enable_wal: bool,
    /// Maximum transaction duration before timeout
    pub timeout: Duration,
    /// Automatically checkpoint every N task completions
    pub auto_checkpoint_interval: usize,
    /// Sync WAL to disk after each write
    pub fsync_wal: bool,
    /// Keep N completed transactions for auditing
    pub keep_completed: usize,
    /// Checkpoint configuration
    pub checkpoint_config: CheckpointConfig,
}

impl Default for TransactionConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("/tmp/rustible/transactions"),
            enable_wal: true,
            timeout: Duration::from_secs(3600), // 1 hour
            auto_checkpoint_interval: 10,
            fsync_wal: false,
            keep_completed: 10,
            checkpoint_config: CheckpointConfig::default(),
        }
    }
}

impl TransactionConfig {
    /// Create a production configuration
    pub fn production() -> Self {
        Self {
            data_dir: PathBuf::from("/var/lib/rustible/transactions"),
            enable_wal: true,
            timeout: Duration::from_secs(7200), // 2 hours
            auto_checkpoint_interval: 5,
            fsync_wal: true,
            keep_completed: 100,
            checkpoint_config: CheckpointConfig::production(),
        }
    }
}

/// State of a transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionState {
    /// Transaction is active
    Active,
    /// Transaction is being committed
    Committing,
    /// Transaction has been committed
    Committed,
    /// Transaction is being rolled back
    RollingBack,
    /// Transaction has been rolled back
    RolledBack,
    /// Transaction timed out
    TimedOut,
}

/// Outcome of a task within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskOutcome {
    Success,
    Changed,
    Skipped,
    Failed { message: String },
    Unreachable { message: String },
}

impl TaskOutcome {
    /// Convert to checkpoint status
    pub fn to_checkpoint_status(&self) -> TaskCheckpointStatus {
        match self {
            TaskOutcome::Success | TaskOutcome::Changed => TaskCheckpointStatus::Completed,
            TaskOutcome::Skipped => TaskCheckpointStatus::Skipped,
            TaskOutcome::Failed { .. } | TaskOutcome::Unreachable { .. } => {
                TaskCheckpointStatus::Failed
            }
        }
    }
}

/// A recorded change within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingChange {
    /// Unique sequence number within transaction
    pub sequence: u64,
    /// Host this change applies to
    pub host: String,
    /// Task name
    pub task_name: String,
    /// Task index
    pub task_index: usize,
    /// Play index
    pub play_index: usize,
    /// Outcome of the task
    pub outcome: TaskOutcome,
    /// Timestamp
    pub timestamp: u64,
    /// Optional result data
    pub result: Option<serde_json::Value>,
}

/// A savepoint within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Savepoint {
    /// Savepoint identifier
    pub id: SavepointId,
    /// Sequence number at time of savepoint
    pub sequence: u64,
    /// Snapshot of state at savepoint
    pub state_snapshot: PlaybookState,
    /// Timestamp
    pub created_at: u64,
}

/// A transaction representing an atomic unit of playbook execution
#[derive(Debug)]
pub struct Transaction {
    /// Transaction ID
    pub id: TransactionId,
    /// Playbook being executed
    pub playbook_name: String,
    /// Current status
    pub status: TransactionState,
    /// Started at timestamp
    pub started_at: u64,
    /// Current playbook state
    pub state: PlaybookState,
    /// Pending changes (not yet committed)
    pub changes: Vec<PendingChange>,
    /// Savepoints
    pub savepoints: Vec<Savepoint>,
    /// Next sequence number
    pub next_sequence: u64,
    /// Tasks since last checkpoint
    pub tasks_since_checkpoint: usize,
    /// Last checkpoint ID
    pub last_checkpoint: Option<CheckpointId>,
}

impl Transaction {
    /// Create a new transaction
    pub fn new(playbook_name: impl Into<String>) -> Self {
        let name = playbook_name.into();
        Self {
            id: TransactionId::new(),
            playbook_name: name.clone(),
            status: TransactionState::Active,
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            state: PlaybookState::new(name),
            changes: Vec::new(),
            savepoints: Vec::new(),
            next_sequence: 0,
            tasks_since_checkpoint: 0,
            last_checkpoint: None,
        }
    }

    /// Check if transaction is active
    pub fn is_active(&self) -> bool {
        self.status == TransactionState::Active
    }

    /// Check if transaction has timed out
    pub fn is_timed_out(&self, timeout: Duration) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let elapsed = now - self.started_at;
        elapsed > timeout.as_secs()
    }

    /// Record a task completion
    pub fn record_task(
        &mut self,
        host: &str,
        task_name: &str,
        task_index: usize,
        play_index: usize,
        outcome: TaskOutcome,
        result: Option<serde_json::Value>,
    ) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence += 1;

        let change = PendingChange {
            sequence,
            host: host.to_string(),
            task_name: task_name.to_string(),
            task_index,
            play_index,
            outcome: outcome.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            result: result.clone(),
        };

        self.changes.push(change);

        // Update state
        self.state.add_host(host);
        self.state.update_task(
            host,
            task_index,
            task_name,
            outcome.to_checkpoint_status(),
            result,
        );

        self.tasks_since_checkpoint += 1;

        sequence
    }

    /// Create a savepoint
    pub fn create_savepoint(&mut self, name: &str) -> SavepointId {
        let id = SavepointId {
            transaction_id: self.id.clone(),
            name: name.to_string(),
            sequence: self.next_sequence,
        };

        let savepoint = Savepoint {
            id: id.clone(),
            sequence: self.next_sequence,
            state_snapshot: self.state.clone(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        self.savepoints.push(savepoint);
        id
    }

    /// Rollback to a savepoint
    pub fn rollback_to_savepoint(&mut self, savepoint_id: &SavepointId) -> TransactionResult<()> {
        let savepoint = self
            .savepoints
            .iter()
            .find(|s| &s.id == savepoint_id)
            .ok_or_else(|| TransactionError::SavepointNotFound(savepoint_id.to_string()))?
            .clone();

        // Remove changes after savepoint
        self.changes.retain(|c| c.sequence < savepoint.sequence);

        // Restore state
        self.state = savepoint.state_snapshot;

        // Remove savepoints after this one
        self.savepoints.retain(|s| s.sequence <= savepoint.sequence);

        debug!(
            "Rolled back transaction {} to savepoint {}",
            self.id, savepoint_id
        );

        Ok(())
    }

    /// Get all savepoint IDs
    pub fn savepoint_ids(&self) -> Vec<SavepointId> {
        self.savepoints.iter().map(|s| s.id.clone()).collect()
    }
}

/// Write-ahead log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalEntry {
    /// Transaction started
    Begin {
        tx_id: TransactionId,
        playbook: String,
        timestamp: u64,
    },
    /// Task recorded
    Task {
        tx_id: TransactionId,
        sequence: u64,
        host: String,
        task_name: String,
        task_index: usize,
        play_index: usize,
        outcome: TaskOutcome,
    },
    /// Savepoint created
    Savepoint {
        tx_id: TransactionId,
        name: String,
        sequence: u64,
    },
    /// Transaction committed
    Commit {
        tx_id: TransactionId,
        timestamp: u64,
    },
    /// Transaction rolled back
    Rollback {
        tx_id: TransactionId,
        timestamp: u64,
    },
    /// Rolled back to savepoint
    RollbackToSavepoint {
        tx_id: TransactionId,
        savepoint: String,
        timestamp: u64,
    },
}

/// Transaction manager coordinating all transactions
pub struct TransactionManager {
    config: TransactionConfig,
    /// Active transactions
    transactions: Arc<RwLock<HashMap<TransactionId, Transaction>>>,
    /// Checkpoint store
    checkpoint_store: Mutex<CheckpointStore>,
    /// WAL file handle
    wal: Option<Mutex<File>>,
}

impl TransactionManager {
    /// Create a new transaction manager
    pub fn new(config: TransactionConfig) -> TransactionResult<Self> {
        // Create directories
        fs::create_dir_all(&config.data_dir)?;
        fs::create_dir_all(&config.checkpoint_config.checkpoint_dir)?;

        // Open WAL if enabled
        let wal = if config.enable_wal {
            let wal_path = config.data_dir.join("transaction.wal");
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&wal_path)?;
            Some(Mutex::new(file))
        } else {
            None
        };

        let checkpoint_store = CheckpointStore::new(config.checkpoint_config.clone());

        Ok(Self {
            config,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            checkpoint_store: Mutex::new(checkpoint_store),
            wal,
        })
    }

    /// Begin a new transaction
    pub async fn begin(&self, playbook_name: &str) -> TransactionResult<TransactionId> {
        let tx = Transaction::new(playbook_name);
        let tx_id = tx.id.clone();

        // Write to WAL
        self.write_wal(WalEntry::Begin {
            tx_id: tx_id.clone(),
            playbook: playbook_name.to_string(),
            timestamp: tx.started_at,
        })
        .await?;

        // Store transaction
        self.transactions.write().insert(tx_id.clone(), tx);

        info!(
            "Started transaction {} for playbook '{}'",
            tx_id, playbook_name
        );
        Ok(tx_id)
    }

    /// Record a task completion
    #[allow(clippy::too_many_arguments)]
    pub async fn record_task(
        &self,
        tx_id: &TransactionId,
        host: &str,
        task_name: &str,
        task_index: usize,
        play_index: usize,
        outcome: TaskOutcome,
        result: Option<serde_json::Value>,
    ) -> TransactionResult<u64> {
        let (sequence, should_checkpoint) = {
            let mut transactions = self.transactions.write();
            let tx = transactions
                .get_mut(tx_id)
                .ok_or_else(|| TransactionError::NotFound(tx_id.to_string()))?;

            if !tx.is_active() {
                return Err(TransactionError::AlreadyCommitted(tx_id.to_string()));
            }

            if tx.is_timed_out(self.config.timeout) {
                tx.status = TransactionState::TimedOut;
                return Err(TransactionError::Timeout(tx_id.to_string()));
            }

            let seq = tx.record_task(
                host,
                task_name,
                task_index,
                play_index,
                outcome.clone(),
                result,
            );
            let should_cp = tx.tasks_since_checkpoint >= self.config.auto_checkpoint_interval;

            (seq, should_cp)
        };

        // Write to WAL
        self.write_wal(WalEntry::Task {
            tx_id: tx_id.clone(),
            sequence,
            host: host.to_string(),
            task_name: task_name.to_string(),
            task_index,
            play_index,
            outcome,
        })
        .await?;

        // Auto-checkpoint if needed
        if should_checkpoint {
            self.checkpoint(tx_id).await?;
        }

        Ok(sequence)
    }

    /// Create a savepoint
    pub async fn savepoint(
        &self,
        tx_id: &TransactionId,
        name: &str,
    ) -> TransactionResult<SavepointId> {
        let savepoint_id = {
            let mut transactions = self.transactions.write();
            let tx = transactions
                .get_mut(tx_id)
                .ok_or_else(|| TransactionError::NotFound(tx_id.to_string()))?;

            if !tx.is_active() {
                return Err(TransactionError::AlreadyCommitted(tx_id.to_string()));
            }

            tx.create_savepoint(name)
        };

        // Write to WAL
        self.write_wal(WalEntry::Savepoint {
            tx_id: tx_id.clone(),
            name: name.to_string(),
            sequence: savepoint_id.sequence,
        })
        .await?;

        debug!("Created savepoint '{}' in transaction {}", name, tx_id);
        Ok(savepoint_id)
    }

    /// Rollback to a savepoint
    pub async fn rollback_to_savepoint(&self, savepoint_id: &SavepointId) -> TransactionResult<()> {
        let tx_id = savepoint_id.transaction_id();

        {
            let mut transactions = self.transactions.write();
            let tx = transactions
                .get_mut(tx_id)
                .ok_or_else(|| TransactionError::NotFound(tx_id.to_string()))?;

            if !tx.is_active() {
                return Err(TransactionError::AlreadyCommitted(tx_id.to_string()));
            }

            tx.rollback_to_savepoint(savepoint_id)?;
        }

        // Write to WAL
        self.write_wal(WalEntry::RollbackToSavepoint {
            tx_id: tx_id.clone(),
            savepoint: savepoint_id.name().to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
        .await?;

        info!(
            "Rolled back transaction {} to savepoint '{}'",
            tx_id,
            savepoint_id.name()
        );
        Ok(())
    }

    /// Commit a transaction
    pub async fn commit(&self, tx_id: &TransactionId) -> TransactionResult<CheckpointId> {
        let (checkpoint, playbook_name) = {
            let mut transactions = self.transactions.write();
            let tx = transactions
                .get_mut(tx_id)
                .ok_or_else(|| TransactionError::NotFound(tx_id.to_string()))?;

            if tx.status == TransactionState::Committed {
                return Err(TransactionError::AlreadyCommitted(tx_id.to_string()));
            }

            if tx.status == TransactionState::RolledBack {
                return Err(TransactionError::AlreadyRolledBack(tx_id.to_string()));
            }

            tx.status = TransactionState::Committing;

            // Create final checkpoint
            let checkpoint = Checkpoint::new(&tx.playbook_name, tx.state.clone())
                .with_description(format!("Final checkpoint for transaction {}", tx_id));

            (checkpoint, tx.playbook_name.clone())
        };

        // Persist checkpoint
        let checkpoint_id = {
            let mut store = self.checkpoint_store.lock().await;
            store.save(checkpoint)?
        };

        // Update transaction status
        {
            let mut transactions = self.transactions.write();
            if let Some(tx) = transactions.get_mut(tx_id) {
                tx.status = TransactionState::Committed;
                tx.last_checkpoint = Some(checkpoint_id.clone());
            }
        }

        // Write to WAL
        self.write_wal(WalEntry::Commit {
            tx_id: tx_id.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
        .await?;

        info!(
            "Committed transaction {} with checkpoint {}",
            tx_id, checkpoint_id
        );
        Ok(checkpoint_id)
    }

    /// Rollback a transaction
    pub async fn rollback(&self, tx_id: &TransactionId) -> TransactionResult<()> {
        {
            let mut transactions = self.transactions.write();
            let tx = transactions
                .get_mut(tx_id)
                .ok_or_else(|| TransactionError::NotFound(tx_id.to_string()))?;

            if tx.status == TransactionState::Committed {
                return Err(TransactionError::AlreadyCommitted(tx_id.to_string()));
            }

            tx.status = TransactionState::RolledBack;
            tx.changes.clear();
            tx.savepoints.clear();
        }

        // Write to WAL
        self.write_wal(WalEntry::Rollback {
            tx_id: tx_id.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
        .await?;

        info!("Rolled back transaction {}", tx_id);
        Ok(())
    }

    /// Create a checkpoint without committing
    pub async fn checkpoint(&self, tx_id: &TransactionId) -> TransactionResult<CheckpointId> {
        let checkpoint = {
            let transactions = self.transactions.read();
            let tx = transactions
                .get(tx_id)
                .ok_or_else(|| TransactionError::NotFound(tx_id.to_string()))?;

            if !tx.is_active() {
                return Err(TransactionError::AlreadyCommitted(tx_id.to_string()));
            }

            Checkpoint::new(&tx.playbook_name, tx.state.clone())
                .with_description(format!("Checkpoint for transaction {}", tx_id))
        };

        let checkpoint_id = {
            let mut store = self.checkpoint_store.lock().await;
            store.save(checkpoint)?
        };

        // Update transaction
        {
            let mut transactions = self.transactions.write();
            if let Some(tx) = transactions.get_mut(tx_id) {
                tx.last_checkpoint = Some(checkpoint_id.clone());
                tx.tasks_since_checkpoint = 0;
            }
        }

        debug!(
            "Created checkpoint {} for transaction {}",
            checkpoint_id, tx_id
        );
        Ok(checkpoint_id)
    }

    /// Get transaction status
    pub fn status(&self, tx_id: &TransactionId) -> Option<TransactionState> {
        self.transactions.read().get(tx_id).map(|tx| tx.status)
    }

    /// Get transaction state
    pub fn get_state(&self, tx_id: &TransactionId) -> Option<PlaybookState> {
        self.transactions
            .read()
            .get(tx_id)
            .map(|tx| tx.state.clone())
    }

    /// List active transactions
    pub fn active_transactions(&self) -> Vec<TransactionId> {
        self.transactions
            .read()
            .iter()
            .filter(|(_, tx)| tx.is_active())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Recover incomplete transactions from WAL
    pub async fn recover(&self) -> TransactionResult<Vec<TransactionId>> {
        if !self.config.enable_wal {
            return Ok(Vec::new());
        }

        let wal_path = self.config.data_dir.join("transaction.wal");
        if !wal_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&wal_path)?;
        let reader = BufReader::new(file);
        let mut recovered = HashMap::new();

        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }

            let entry: WalEntry = serde_json::from_str(&line)
                .map_err(|e| TransactionError::WalCorrupted(e.to_string()))?;

            match entry {
                WalEntry::Begin {
                    tx_id, playbook, ..
                } => {
                    let tx = Transaction::new(playbook);
                    recovered.insert(tx_id, tx);
                }
                WalEntry::Task {
                    tx_id,
                    host,
                    task_name,
                    task_index,
                    play_index,
                    outcome,
                    ..
                } => {
                    if let Some(tx) = recovered.get_mut(&tx_id) {
                        tx.record_task(&host, &task_name, task_index, play_index, outcome, None);
                    }
                }
                WalEntry::Savepoint { tx_id, name, .. } => {
                    if let Some(tx) = recovered.get_mut(&tx_id) {
                        tx.create_savepoint(&name);
                    }
                }
                WalEntry::Commit { tx_id, .. } => {
                    recovered.remove(&tx_id);
                }
                WalEntry::Rollback { tx_id, .. } => {
                    recovered.remove(&tx_id);
                }
                WalEntry::RollbackToSavepoint {
                    tx_id, savepoint, ..
                } => {
                    if let Some(tx) = recovered.get_mut(&tx_id) {
                        // Find and rollback to savepoint
                        if let Some(sp) = tx.savepoints.iter().find(|s| s.id.name == savepoint) {
                            let _ = tx.rollback_to_savepoint(&sp.id.clone());
                        }
                    }
                }
            }
        }

        // Store recovered transactions
        let recovered_ids: Vec<_> = recovered.keys().cloned().collect();
        {
            let mut transactions = self.transactions.write();
            for (id, tx) in recovered {
                transactions.insert(id, tx);
            }
        }

        if !recovered_ids.is_empty() {
            warn!(
                "Recovered {} incomplete transactions from WAL",
                recovered_ids.len()
            );
        }

        Ok(recovered_ids)
    }

    /// Write entry to WAL
    async fn write_wal(&self, entry: WalEntry) -> TransactionResult<()> {
        if let Some(ref wal) = self.wal {
            let mut file = wal.lock().await;
            let json = serde_json::to_string(&entry)
                .map_err(|e| TransactionError::Serialization(e.to_string()))?;
            writeln!(file, "{}", json)?;
            if self.config.fsync_wal {
                file.sync_data()?;
            }
        }
        Ok(())
    }

    /// Cleanup completed transactions
    pub async fn cleanup(&self) -> TransactionResult<usize> {
        let mut to_remove = Vec::new();

        {
            let transactions = self.transactions.read();
            for (id, tx) in transactions.iter() {
                if tx.status == TransactionState::Committed
                    || tx.status == TransactionState::RolledBack
                {
                    to_remove.push(id.clone());
                }
            }
        }

        // Keep only the most recent completed transactions
        if to_remove.len() > self.config.keep_completed {
            let excess = to_remove.len() - self.config.keep_completed;
            let mut transactions = self.transactions.write();
            for id in to_remove.iter().take(excess) {
                transactions.remove(id);
            }
            return Ok(excess);
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    fn test_config(dir: &Path) -> TransactionConfig {
        TransactionConfig {
            data_dir: dir.to_path_buf(),
            enable_wal: true,
            fsync_wal: false,
            checkpoint_config: CheckpointConfig {
                checkpoint_dir: dir.join("checkpoints"),
                compress: false,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_transaction_lifecycle() {
        let dir = tempdir().unwrap();
        let config = test_config(dir.path());
        let manager = TransactionManager::new(config).unwrap();

        // Begin transaction
        let tx_id = manager.begin("test.yml").await.unwrap();
        assert!(manager.status(&tx_id).unwrap() == TransactionState::Active);

        // Record a task
        manager
            .record_task(&tx_id, "host1", "task1", 0, 0, TaskOutcome::Success, None)
            .await
            .unwrap();

        // Commit
        let checkpoint_id = manager.commit(&tx_id).await.unwrap();
        assert!(manager.status(&tx_id).unwrap() == TransactionState::Committed);
        assert!(!checkpoint_id.as_str().is_empty());
    }

    #[tokio::test]
    async fn test_savepoint_rollback() {
        let dir = tempdir().unwrap();
        let config = test_config(dir.path());
        let manager = TransactionManager::new(config).unwrap();

        let tx_id = manager.begin("test.yml").await.unwrap();

        // Record task 1
        manager
            .record_task(&tx_id, "host1", "task1", 0, 0, TaskOutcome::Success, None)
            .await
            .unwrap();

        // Create savepoint
        let sp = manager.savepoint(&tx_id, "before_risky").await.unwrap();

        // Record task 2
        manager
            .record_task(
                &tx_id,
                "host1",
                "task2",
                1,
                0,
                TaskOutcome::Failed {
                    message: "oops".into(),
                },
                None,
            )
            .await
            .unwrap();

        // Rollback to savepoint
        manager.rollback_to_savepoint(&sp).await.unwrap();

        // State should be back to after task1
        let state = manager.get_state(&tx_id).unwrap();
        assert_eq!(state.completed_tasks, 1);
    }

    #[tokio::test]
    async fn test_transaction_rollback() {
        let dir = tempdir().unwrap();
        let config = test_config(dir.path());
        let manager = TransactionManager::new(config).unwrap();

        let tx_id = manager.begin("test.yml").await.unwrap();

        manager
            .record_task(&tx_id, "host1", "task1", 0, 0, TaskOutcome::Success, None)
            .await
            .unwrap();

        manager.rollback(&tx_id).await.unwrap();
        assert!(manager.status(&tx_id).unwrap() == TransactionState::RolledBack);
    }

    #[test]
    fn test_transaction_id() {
        let id1 = TransactionId::new();
        let id2 = TransactionId::new();

        assert_ne!(id1, id2);
        assert!(id1.as_str().starts_with("tx-"));
    }
}
