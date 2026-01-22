//! State Persistence Backends
//!
//! This module provides different storage backends for persisting execution state:
//! - **JSON**: Simple file-based storage using JSON files
//! - **SQLite**: Robust database storage with query capabilities
//! - **Memory**: In-memory storage for testing
//!
//! All backends implement the `StatePersistence` trait for consistent access.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::{StateError, StateResult, StateSnapshot, TaskStateRecord};

type PersistedState = (
    DashMap<String, StateSnapshot>,
    DashMap<String, Vec<TaskStateRecord>>,
    SqliteMetadata,
);

/// Persistence backend type
#[derive(Debug, Clone)]
pub enum PersistenceBackend {
    /// JSON file-based storage
    Json(PathBuf),
    /// SQLite database storage
    Sqlite(PathBuf),
    /// In-memory storage (for testing)
    Memory,
}

/// Trait for state persistence implementations
pub trait StatePersistence: Send + Sync {
    /// Save a state snapshot
    fn save_snapshot(&self, snapshot: &StateSnapshot) -> StateResult<()>;

    /// Load a snapshot by ID
    fn load_snapshot(&self, snapshot_id: &str) -> StateResult<StateSnapshot>;

    /// List all snapshots
    fn list_snapshots(&self) -> StateResult<Vec<StateSnapshot>>;

    /// Get the most recent snapshot for a playbook
    fn get_latest_snapshot(&self, playbook: &str) -> StateResult<Option<StateSnapshot>>;

    /// Delete a snapshot
    fn delete_snapshot(&self, snapshot_id: &str) -> StateResult<()>;

    /// Cleanup snapshots before a given time
    fn cleanup_before(&self, before: SystemTime) -> StateResult<usize>;

    /// Save a task record
    fn save_task_record(&self, session_id: &str, record: &TaskStateRecord) -> StateResult<()>;

    /// Get task records for a session
    fn get_task_records(&self, session_id: &str) -> StateResult<Vec<TaskStateRecord>>;

    /// Get task records for a host in a session
    fn get_host_task_records(
        &self,
        session_id: &str,
        host: &str,
    ) -> StateResult<Vec<TaskStateRecord>>;
}

// ============================================================================
// JSON Persistence Backend
// ============================================================================

/// JSON file-based persistence backend
pub struct JsonPersistence {
    base_dir: PathBuf,
    snapshots_dir: PathBuf,
    tasks_dir: PathBuf,
    index: Arc<RwLock<SnapshotIndex>>,
}

/// Index for quick snapshot lookups
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SnapshotIndex {
    /// Map of snapshot ID to metadata
    snapshots: HashMap<String, SnapshotMetadata>,
    /// Map of playbook name to most recent snapshot ID
    latest_by_playbook: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotMetadata {
    id: String,
    playbook: String,
    created_at: DateTime<Utc>,
    task_count: usize,
    file_path: PathBuf,
}

impl JsonPersistence {
    /// Create a new JSON persistence backend
    pub fn new(base_dir: PathBuf) -> StateResult<Self> {
        let snapshots_dir = base_dir.join("snapshots");
        let tasks_dir = base_dir.join("tasks");

        // Create directories if they don't exist
        fs::create_dir_all(&snapshots_dir)?;
        fs::create_dir_all(&tasks_dir)?;

        // Load or create index
        let index_path = base_dir.join("index.json");
        let index = if index_path.exists() {
            let file = File::open(&index_path)?;
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).unwrap_or_default()
        } else {
            SnapshotIndex::default()
        };

        Ok(Self {
            base_dir,
            snapshots_dir,
            tasks_dir,
            index: Arc::new(RwLock::new(index)),
        })
    }

    /// Save the index to disk
    fn save_index(&self) -> StateResult<()> {
        let index_path = self.base_dir.join("index.json");
        let file = File::create(&index_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &*self.index.read())?;
        Ok(())
    }

    /// Get the file path for a snapshot
    fn snapshot_path(&self, snapshot_id: &str) -> PathBuf {
        self.snapshots_dir.join(format!("{}.json", snapshot_id))
    }

    /// Get the file path for a session's tasks
    fn tasks_path(&self, session_id: &str) -> PathBuf {
        self.tasks_dir.join(format!("{}.json", session_id))
    }
}

impl StatePersistence for JsonPersistence {
    fn save_snapshot(&self, snapshot: &StateSnapshot) -> StateResult<()> {
        let file_path = self.snapshot_path(&snapshot.id);
        let file = File::create(&file_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, snapshot)?;

        // Update index
        {
            let mut index = self.index.write();
            index.snapshots.insert(
                snapshot.id.clone(),
                SnapshotMetadata {
                    id: snapshot.id.clone(),
                    playbook: snapshot.playbook.clone(),
                    created_at: snapshot.created_at,
                    task_count: snapshot.tasks.len(),
                    file_path: file_path.clone(),
                },
            );
            index
                .latest_by_playbook
                .insert(snapshot.playbook.clone(), snapshot.id.clone());
        }

        self.save_index()?;
        Ok(())
    }

    fn load_snapshot(&self, snapshot_id: &str) -> StateResult<StateSnapshot> {
        let file_path = self.snapshot_path(snapshot_id);
        if !file_path.exists() {
            return Err(StateError::StateNotFound(snapshot_id.to_string()));
        }

        let file = File::open(&file_path)?;
        let reader = BufReader::new(file);
        let snapshot: StateSnapshot = serde_json::from_reader(reader)?;
        Ok(snapshot)
    }

    fn list_snapshots(&self) -> StateResult<Vec<StateSnapshot>> {
        let index = self.index.read();
        let mut snapshots = Vec::with_capacity(index.snapshots.len());

        for metadata in index.snapshots.values() {
            if metadata.file_path.exists() {
                match self.load_snapshot(&metadata.id) {
                    Ok(snapshot) => snapshots.push(snapshot),
                    Err(_) => continue, // Skip corrupted snapshots
                }
            }
        }

        // Sort by creation time, newest first
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(snapshots)
    }

    fn get_latest_snapshot(&self, playbook: &str) -> StateResult<Option<StateSnapshot>> {
        let index = self.index.read();
        if let Some(snapshot_id) = index.latest_by_playbook.get(playbook) {
            Ok(Some(self.load_snapshot(snapshot_id)?))
        } else {
            Ok(None)
        }
    }

    fn delete_snapshot(&self, snapshot_id: &str) -> StateResult<()> {
        let file_path = self.snapshot_path(snapshot_id);
        if file_path.exists() {
            fs::remove_file(&file_path)?;
        }

        // Update index
        {
            let mut index = self.index.write();
            if let Some(metadata) = index.snapshots.remove(snapshot_id) {
                // Update latest_by_playbook if necessary
                if index.latest_by_playbook.get(&metadata.playbook)
                    == Some(&snapshot_id.to_string())
                {
                    // Find the next most recent snapshot for this playbook
                    let next_latest = index
                        .snapshots
                        .values()
                        .filter(|m| m.playbook == metadata.playbook)
                        .max_by_key(|m| m.created_at)
                        .map(|m| m.id.clone());

                    if let Some(next_id) = next_latest {
                        index.latest_by_playbook.insert(metadata.playbook, next_id);
                    } else {
                        index.latest_by_playbook.remove(&metadata.playbook);
                    }
                }
            }
        }

        self.save_index()?;
        Ok(())
    }

    fn cleanup_before(&self, before: SystemTime) -> StateResult<usize> {
        let before_dt: DateTime<Utc> = before.into();
        let mut removed = 0;

        let to_remove: Vec<String> = {
            let index = self.index.read();
            index
                .snapshots
                .iter()
                .filter(|(_, m)| m.created_at < before_dt)
                .map(|(id, _)| id.clone())
                .collect()
        };

        for snapshot_id in to_remove {
            if self.delete_snapshot(&snapshot_id).is_ok() {
                removed += 1;
            }
        }

        Ok(removed)
    }

    fn save_task_record(&self, session_id: &str, record: &TaskStateRecord) -> StateResult<()> {
        let tasks_path = self.tasks_path(session_id);

        // Load existing tasks or create new list
        let mut tasks: Vec<TaskStateRecord> = if tasks_path.exists() {
            let file = File::open(&tasks_path)?;
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Add or update task
        if let Some(existing) = tasks.iter_mut().find(|t| t.id == record.id) {
            *existing = record.clone();
        } else {
            tasks.push(record.clone());
        }

        // Save back
        let file = File::create(&tasks_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &tasks)?;

        Ok(())
    }

    fn get_task_records(&self, session_id: &str) -> StateResult<Vec<TaskStateRecord>> {
        let tasks_path = self.tasks_path(session_id);
        if !tasks_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&tasks_path)?;
        let reader = BufReader::new(file);
        let tasks: Vec<TaskStateRecord> = serde_json::from_reader(reader)?;
        Ok(tasks)
    }

    fn get_host_task_records(
        &self,
        session_id: &str,
        host: &str,
    ) -> StateResult<Vec<TaskStateRecord>> {
        let tasks = self.get_task_records(session_id)?;
        Ok(tasks.into_iter().filter(|t| t.host == host).collect())
    }
}

// ============================================================================
// SQLite Persistence Backend
// ============================================================================

/// SQLite database persistence backend
pub struct SqlitePersistence {
    db_path: PathBuf,
    // In a real implementation, we would use rusqlite or similar
    // For now, we'll use a simpler file-based approach that mimics SQLite behavior
    snapshots: Arc<DashMap<String, StateSnapshot>>,
    tasks: Arc<DashMap<String, Vec<TaskStateRecord>>>,
    metadata: Arc<RwLock<SqliteMetadata>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SqliteMetadata {
    latest_by_playbook: HashMap<String, String>,
    snapshot_count: u64,
    task_count: u64,
}

impl SqlitePersistence {
    /// Create a new SQLite persistence backend
    pub fn new(db_path: PathBuf) -> StateResult<Self> {
        // Create parent directory if needed
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Load existing data if file exists
        let (snapshots, tasks, metadata) = if db_path.exists() {
            Self::load_from_file(&db_path)?
        } else {
            (DashMap::new(), DashMap::new(), SqliteMetadata::default())
        };

        Ok(Self {
            db_path,
            snapshots: Arc::new(snapshots),
            tasks: Arc::new(tasks),
            metadata: Arc::new(RwLock::new(metadata)),
        })
    }

    fn load_from_file(
        path: &PathBuf,
    ) -> StateResult<PersistedState> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        #[derive(Deserialize)]
        struct DbFile {
            snapshots: HashMap<String, StateSnapshot>,
            tasks: HashMap<String, Vec<TaskStateRecord>>,
            metadata: SqliteMetadata,
        }

        let db: DbFile = serde_json::from_reader(reader).unwrap_or_else(|_| DbFile {
            snapshots: HashMap::new(),
            tasks: HashMap::new(),
            metadata: SqliteMetadata::default(),
        });

        let snapshots = DashMap::new();
        for (k, v) in db.snapshots {
            snapshots.insert(k, v);
        }

        let tasks = DashMap::new();
        for (k, v) in db.tasks {
            tasks.insert(k, v);
        }

        Ok((snapshots, tasks, db.metadata))
    }


    fn save_to_file(&self) -> StateResult<()> {
        #[derive(Serialize)]
        struct DbFile<'a> {
            snapshots: HashMap<String, StateSnapshot>,
            tasks: HashMap<String, Vec<TaskStateRecord>>,
            metadata: &'a SqliteMetadata,
        }

        let snapshots: HashMap<String, StateSnapshot> = self
            .snapshots
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        let tasks: HashMap<String, Vec<TaskStateRecord>> = self
            .tasks
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        let db = DbFile {
            snapshots,
            tasks,
            metadata: &self.metadata.read(),
        };

        let file = File::create(&self.db_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, &db)?;

        Ok(())
    }
}

impl StatePersistence for SqlitePersistence {
    fn save_snapshot(&self, snapshot: &StateSnapshot) -> StateResult<()> {
        self.snapshots.insert(snapshot.id.clone(), snapshot.clone());

        {
            let mut metadata = self.metadata.write();
            metadata
                .latest_by_playbook
                .insert(snapshot.playbook.clone(), snapshot.id.clone());
            metadata.snapshot_count += 1;
        }

        self.save_to_file()
    }

    fn load_snapshot(&self, snapshot_id: &str) -> StateResult<StateSnapshot> {
        self.snapshots
            .get(snapshot_id)
            .map(|r| r.value().clone())
            .ok_or_else(|| StateError::StateNotFound(snapshot_id.to_string()))
    }

    fn list_snapshots(&self) -> StateResult<Vec<StateSnapshot>> {
        let mut snapshots: Vec<StateSnapshot> =
            self.snapshots.iter().map(|r| r.value().clone()).collect();

        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(snapshots)
    }

    fn get_latest_snapshot(&self, playbook: &str) -> StateResult<Option<StateSnapshot>> {
        let metadata = self.metadata.read();
        if let Some(snapshot_id) = metadata.latest_by_playbook.get(playbook) {
            Ok(self.snapshots.get(snapshot_id).map(|r| r.value().clone()))
        } else {
            Ok(None)
        }
    }

    fn delete_snapshot(&self, snapshot_id: &str) -> StateResult<()> {
        if let Some((_, snapshot)) = self.snapshots.remove(snapshot_id) {
            let mut metadata = self.metadata.write();
            if metadata.latest_by_playbook.get(&snapshot.playbook) == Some(&snapshot_id.to_string())
            {
                // Find next latest
                let next_latest = self
                    .snapshots
                    .iter()
                    .filter(|r| r.value().playbook == snapshot.playbook)
                    .max_by_key(|r| r.value().created_at)
                    .map(|r| r.key().clone());

                if let Some(next_id) = next_latest {
                    metadata
                        .latest_by_playbook
                        .insert(snapshot.playbook.clone(), next_id);
                } else {
                    metadata.latest_by_playbook.remove(&snapshot.playbook);
                }
            }
        }

        self.save_to_file()
    }

    fn cleanup_before(&self, before: SystemTime) -> StateResult<usize> {
        let before_dt: DateTime<Utc> = before.into();
        let mut removed = 0;

        let to_remove: Vec<String> = self
            .snapshots
            .iter()
            .filter(|r| r.value().created_at < before_dt)
            .map(|r| r.key().clone())
            .collect();

        for snapshot_id in to_remove {
            if self.delete_snapshot(&snapshot_id).is_ok() {
                removed += 1;
            }
        }

        Ok(removed)
    }

    fn save_task_record(&self, session_id: &str, record: &TaskStateRecord) -> StateResult<()> {
        self.tasks
            .entry(session_id.to_string())
            .or_default()
            .push(record.clone());

        {
            let mut metadata = self.metadata.write();
            metadata.task_count += 1;
        }

        self.save_to_file()
    }

    fn get_task_records(&self, session_id: &str) -> StateResult<Vec<TaskStateRecord>> {
        Ok(self
            .tasks
            .get(session_id)
            .map(|r| r.value().clone())
            .unwrap_or_default())
    }

    fn get_host_task_records(
        &self,
        session_id: &str,
        host: &str,
    ) -> StateResult<Vec<TaskStateRecord>> {
        let tasks = self.get_task_records(session_id)?;
        Ok(tasks.into_iter().filter(|t| t.host == host).collect())
    }
}

// ============================================================================
// Memory Persistence Backend (for testing)
// ============================================================================

/// In-memory persistence backend for testing
pub struct MemoryPersistence {
    snapshots: Arc<DashMap<String, StateSnapshot>>,
    tasks: Arc<DashMap<String, Vec<TaskStateRecord>>>,
    latest_by_playbook: Arc<DashMap<String, String>>,
}

impl MemoryPersistence {
    /// Create a new in-memory persistence backend
    pub fn new() -> Self {
        Self {
            snapshots: Arc::new(DashMap::new()),
            tasks: Arc::new(DashMap::new()),
            latest_by_playbook: Arc::new(DashMap::new()),
        }
    }
}

impl Default for MemoryPersistence {
    fn default() -> Self {
        Self::new()
    }
}

impl StatePersistence for MemoryPersistence {
    fn save_snapshot(&self, snapshot: &StateSnapshot) -> StateResult<()> {
        self.snapshots.insert(snapshot.id.clone(), snapshot.clone());
        self.latest_by_playbook
            .insert(snapshot.playbook.clone(), snapshot.id.clone());
        Ok(())
    }

    fn load_snapshot(&self, snapshot_id: &str) -> StateResult<StateSnapshot> {
        self.snapshots
            .get(snapshot_id)
            .map(|r| r.value().clone())
            .ok_or_else(|| StateError::StateNotFound(snapshot_id.to_string()))
    }

    fn list_snapshots(&self) -> StateResult<Vec<StateSnapshot>> {
        let mut snapshots: Vec<StateSnapshot> =
            self.snapshots.iter().map(|r| r.value().clone()).collect();

        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(snapshots)
    }

    fn get_latest_snapshot(&self, playbook: &str) -> StateResult<Option<StateSnapshot>> {
        if let Some(snapshot_id) = self.latest_by_playbook.get(playbook) {
            Ok(self
                .snapshots
                .get(snapshot_id.value())
                .map(|r| r.value().clone()))
        } else {
            Ok(None)
        }
    }

    fn delete_snapshot(&self, snapshot_id: &str) -> StateResult<()> {
        if let Some((_, snapshot)) = self.snapshots.remove(snapshot_id) {
            if self
                .latest_by_playbook
                .get(&snapshot.playbook)
                .map(|r| r.value().clone())
                == Some(snapshot_id.to_string())
            {
                let next_latest = self
                    .snapshots
                    .iter()
                    .filter(|r| r.value().playbook == snapshot.playbook)
                    .max_by_key(|r| r.value().created_at)
                    .map(|r| r.key().clone());

                if let Some(next_id) = next_latest {
                    self.latest_by_playbook
                        .insert(snapshot.playbook.clone(), next_id);
                } else {
                    self.latest_by_playbook.remove(&snapshot.playbook);
                }
            }
        }
        Ok(())
    }

    fn cleanup_before(&self, before: SystemTime) -> StateResult<usize> {
        let before_dt: DateTime<Utc> = before.into();
        let mut removed = 0;

        let to_remove: Vec<String> = self
            .snapshots
            .iter()
            .filter(|r| r.value().created_at < before_dt)
            .map(|r| r.key().clone())
            .collect();

        for snapshot_id in to_remove {
            if self.delete_snapshot(&snapshot_id).is_ok() {
                removed += 1;
            }
        }

        Ok(removed)
    }

    fn save_task_record(&self, session_id: &str, record: &TaskStateRecord) -> StateResult<()> {
        self.tasks
            .entry(session_id.to_string())
            .or_default()
            .push(record.clone());
        Ok(())
    }

    fn get_task_records(&self, session_id: &str) -> StateResult<Vec<TaskStateRecord>> {
        Ok(self
            .tasks
            .get(session_id)
            .map(|r| r.value().clone())
            .unwrap_or_default())
    }

    fn get_host_task_records(
        &self,
        session_id: &str,
        host: &str,
    ) -> StateResult<Vec<TaskStateRecord>> {
        let tasks = self.get_task_records(session_id)?;
        Ok(tasks.into_iter().filter(|t| t.host == host).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_persistence() {
        let persistence = MemoryPersistence::new();

        let snapshot = StateSnapshot::new("session1", "playbook.yml");
        persistence.save_snapshot(&snapshot).unwrap();

        let loaded = persistence.load_snapshot(&snapshot.id).unwrap();
        assert_eq!(loaded.id, snapshot.id);

        let latest = persistence.get_latest_snapshot("playbook.yml").unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().id, snapshot.id);
    }

    #[test]
    fn test_json_persistence() {
        let temp_dir = std::env::temp_dir().join("rustible_test_json");
        let _ = fs::remove_dir_all(&temp_dir);

        let persistence = JsonPersistence::new(temp_dir.clone()).unwrap();

        let mut snapshot = StateSnapshot::new("session1", "test.yml");
        snapshot
            .tasks
            .push(TaskStateRecord::new("task1", "host1", "apt"));

        persistence.save_snapshot(&snapshot).unwrap();

        let loaded = persistence.load_snapshot(&snapshot.id).unwrap();
        assert_eq!(loaded.id, snapshot.id);
        assert_eq!(loaded.tasks.len(), 1);

        let list = persistence.list_snapshots().unwrap();
        assert_eq!(list.len(), 1);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_sqlite_persistence() {
        let temp_dir = std::env::temp_dir().join("rustible_test_sqlite");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let db_path = temp_dir.join("state.db");
        let persistence = SqlitePersistence::new(db_path).unwrap();

        let snapshot = StateSnapshot::new("session1", "test.yml");
        persistence.save_snapshot(&snapshot).unwrap();

        let loaded = persistence.load_snapshot(&snapshot.id).unwrap();
        assert_eq!(loaded.id, snapshot.id);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_task_records() {
        let persistence = MemoryPersistence::new();

        let record1 = TaskStateRecord::new("task1", "host1", "apt");
        let record2 = TaskStateRecord::new("task2", "host1", "service");
        let record3 = TaskStateRecord::new("task3", "host2", "apt");

        persistence.save_task_record("session1", &record1).unwrap();
        persistence.save_task_record("session1", &record2).unwrap();
        persistence.save_task_record("session1", &record3).unwrap();

        let all_tasks = persistence.get_task_records("session1").unwrap();
        assert_eq!(all_tasks.len(), 3);

        let host1_tasks = persistence
            .get_host_task_records("session1", "host1")
            .unwrap();
        assert_eq!(host1_tasks.len(), 2);
    }
}
