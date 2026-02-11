//! Immutable audit storage with hash-chain integrity
//!
//! This module provides an append-only audit store that combines hash-chain
//! verification with persistent storage. The `AuditStorage` trait abstracts
//! the storage backend, and `FileAuditStorage` provides a JSON-lines file
//! implementation.

use super::hashchain::{HashChainEntry, HashChainState};
use async_trait::async_trait;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors specific to immutable audit storage operations.
#[derive(Error, Debug)]
pub enum ImmutableAuditError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("chain verification failed")]
    VerificationFailed,
}

/// Result type alias for immutable audit operations.
pub type ImmutableAuditResult<T> = std::result::Result<T, ImmutableAuditError>;

/// Trait for append-only audit storage backends.
#[async_trait]
pub trait AuditStorage: Send + Sync {
    /// Append a hash-chain entry to the storage.
    async fn append(&self, entry: &HashChainEntry) -> ImmutableAuditResult<()>;

    /// Read all entries from the storage.
    async fn read_all(&self) -> ImmutableAuditResult<Vec<HashChainEntry>>;

    /// Verify the integrity of the stored chain.
    async fn verify(&self) -> ImmutableAuditResult<bool> {
        let entries = self.read_all().await?;
        Ok(HashChainState::verify_chain(&entries))
    }
}

/// File-based append-only audit storage using JSON lines format.
///
/// Each `HashChainEntry` is serialized as a single JSON line appended to the file.
/// The file is opened in append mode to prevent accidental overwrites.
#[derive(Debug, Clone)]
pub struct FileAuditStorage {
    path: PathBuf,
}

impl FileAuditStorage {
    /// Create a new file-based audit storage at the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Get the path to the backing file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl AuditStorage for FileAuditStorage {
    async fn append(&self, entry: &HashChainEntry) -> ImmutableAuditResult<()> {
        use std::fs::OpenOptions;
        use std::io::Write;

        let mut line = serde_json::to_string(entry)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        file.write_all(line.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    async fn read_all(&self) -> ImmutableAuditResult<Vec<HashChainEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.path)?;
        let mut entries = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: HashChainEntry = serde_json::from_str(trimmed)?;
            entries.push(entry);
        }
        Ok(entries)
    }
}

/// High-level immutable audit store combining hash-chain state with storage.
///
/// This struct maintains an in-memory `HashChainState` and writes every new
/// event to the underlying `AuditStorage` backend.
pub struct ImmutableAuditStore {
    chain: HashChainState,
    storage: Box<dyn AuditStorage>,
}

impl ImmutableAuditStore {
    /// Create a new store with the given storage backend.
    pub fn new(storage: Box<dyn AuditStorage>) -> Self {
        Self {
            chain: HashChainState::new(),
            storage,
        }
    }

    /// Create a store and replay existing entries to rebuild chain state.
    pub async fn open(storage: Box<dyn AuditStorage>) -> ImmutableAuditResult<Self> {
        let entries = storage.read_all().await?;
        let chain = if entries.is_empty() {
            HashChainState::new()
        } else {
            let last = entries.last().unwrap();
            HashChainState::resume(last.sequence + 1, last.chain_hash.clone())
        };
        Ok(Self { chain, storage })
    }

    /// Record a new audit event (as raw bytes) into the chain and persist it.
    pub async fn record(&mut self, event_data: &[u8]) -> ImmutableAuditResult<HashChainEntry> {
        let entry = self.chain.append(event_data);
        self.storage.append(&entry).await?;
        Ok(entry)
    }

    /// Verify the entire stored chain.
    pub async fn verify(&self) -> ImmutableAuditResult<bool> {
        self.storage.verify().await
    }

    /// Get the current sequence number.
    pub fn next_sequence(&self) -> u64 {
        self.chain.next_sequence()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_file_storage_round_trip() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = FileAuditStorage::new(tmp.path());

        let mut chain = HashChainState::new();
        let e0 = chain.append(b"hello");
        let e1 = chain.append(b"world");

        storage.append(&e0).await.unwrap();
        storage.append(&e1).await.unwrap();

        let entries = storage.read_all().await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], e0);
        assert_eq!(entries[1], e1);

        assert!(storage.verify().await.unwrap());
    }

    #[tokio::test]
    async fn test_immutable_store_record_and_verify() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Box::new(FileAuditStorage::new(tmp.path()));
        let mut store = ImmutableAuditStore::new(storage);

        let e0 = store.record(b"event-a").await.unwrap();
        assert_eq!(e0.sequence, 0);

        let e1 = store.record(b"event-b").await.unwrap();
        assert_eq!(e1.sequence, 1);

        assert!(store.verify().await.unwrap());
    }

    #[tokio::test]
    async fn test_immutable_store_open_resumes() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        // Write two entries
        {
            let storage = Box::new(FileAuditStorage::new(&path));
            let mut store = ImmutableAuditStore::new(storage);
            store.record(b"first").await.unwrap();
            store.record(b"second").await.unwrap();
        }

        // Re-open and continue
        let storage = Box::new(FileAuditStorage::new(&path));
        let mut store = ImmutableAuditStore::open(storage).await.unwrap();
        assert_eq!(store.next_sequence(), 2);

        let e2 = store.record(b"third").await.unwrap();
        assert_eq!(e2.sequence, 2);

        assert!(store.verify().await.unwrap());
    }
}
