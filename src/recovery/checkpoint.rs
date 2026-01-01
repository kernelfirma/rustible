//! Checkpoint and Resume Module
//!
//! Provides checkpoint/resume capability for long-running playbooks:
//!
//! - Save playbook execution state at configurable intervals
//! - Resume execution from the last successful checkpoint
//! - Track per-host and per-task progress
//! - File-based persistence with optional compression
//!
//! # Example
//!
//! ```rust,ignore
//! use rustible::recovery::checkpoint::{CheckpointStore, CheckpointConfig, PlaybookState};
//!
//! // Configure checkpointing
//! let config = CheckpointConfig {
//!     checkpoint_dir: "/var/lib/rustible/checkpoints".into(),
//!     auto_checkpoint_interval: Some(10), // Every 10 tasks
//!     compress: true,
//!     ..Default::default()
//! };
//!
//! let store = CheckpointStore::new(config);
//!
//! // Create checkpoint
//! let state = PlaybookState::new("deploy.yml");
//! let id = store.save(Checkpoint::new("deploy.yml", state))?;
//!
//! // Resume later
//! let checkpoint = store.load(&id)?;
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Error type for checkpoint operations
#[derive(Error, Debug)]
pub enum CheckpointError {
    #[error("Checkpoint not found: {0}")]
    NotFound(String),

    #[error("Invalid checkpoint data: {0}")]
    InvalidData(String),

    #[error("Checkpoint expired: {0}")]
    Expired(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Checkpoint directory not configured")]
    NoDirectory,
}

/// Unique identifier for a checkpoint
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CheckpointId(String);

impl CheckpointId {
    /// Create a new checkpoint ID
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let random: u32 = rand::random();
        Self(format!("cp-{}-{:08x}", timestamp, random))
    }

    /// Create from string
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Get the ID as a string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CheckpointId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Configuration for checkpointing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    /// Directory to store checkpoint files
    pub checkpoint_dir: PathBuf,

    /// Automatically create checkpoints every N tasks
    pub auto_checkpoint_interval: Option<usize>,

    /// Compress checkpoint data
    pub compress: bool,

    /// Maximum age of checkpoints before they're considered stale (in hours)
    pub max_age_hours: Option<u64>,

    /// Maximum number of checkpoints to keep per playbook
    pub max_checkpoints_per_playbook: Option<usize>,

    /// Include full task results in checkpoints
    pub include_results: bool,

    /// Include variable state in checkpoints
    pub include_variables: bool,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            checkpoint_dir: PathBuf::from("/tmp/rustible/checkpoints"),
            auto_checkpoint_interval: Some(20),
            compress: false,
            max_age_hours: Some(24),
            max_checkpoints_per_playbook: Some(5),
            include_results: true,
            include_variables: true,
        }
    }
}

impl CheckpointConfig {
    /// Create a production configuration
    pub fn production() -> Self {
        Self {
            checkpoint_dir: PathBuf::from("/var/lib/rustible/checkpoints"),
            auto_checkpoint_interval: Some(10),
            compress: true,
            max_age_hours: Some(72),
            max_checkpoints_per_playbook: Some(10),
            include_results: true,
            include_variables: true,
        }
    }
}

/// Progress of a single task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    /// Task name
    pub name: String,

    /// Task index in the play
    pub index: usize,

    /// Status of the task (completed, failed, skipped, pending)
    pub status: TaskCheckpointStatus,

    /// Result data (if include_results is enabled)
    pub result: Option<serde_json::Value>,

    /// Timestamp when task completed
    pub completed_at: Option<u64>,
}

/// Status of a task in the checkpoint
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskCheckpointStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

/// Progress of a single host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostProgress {
    /// Hostname
    pub host: String,

    /// Current play index
    pub current_play: usize,

    /// Current task index within the play
    pub current_task: usize,

    /// Whether the host is failed
    pub failed: bool,

    /// Whether the host is unreachable
    pub unreachable: bool,

    /// Task results for this host
    pub tasks: Vec<TaskProgress>,

    /// Host-specific variables (if include_variables is enabled)
    pub variables: Option<HashMap<String, serde_json::Value>>,
}

impl HostProgress {
    /// Create new host progress
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            current_play: 0,
            current_task: 0,
            failed: false,
            unreachable: false,
            tasks: Vec::new(),
            variables: None,
        }
    }
}

/// State of a playbook execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookState {
    /// Playbook name/path
    pub playbook_name: String,

    /// Total number of plays
    pub total_plays: usize,

    /// Total number of tasks (across all plays)
    pub total_tasks: usize,

    /// Number of completed tasks
    pub completed_tasks: usize,

    /// Current play index
    pub current_play: usize,

    /// Current task index within the current play
    pub current_task: usize,

    /// Progress per host
    pub hosts: HashMap<String, HostProgress>,

    /// Global variables (if include_variables is enabled)
    pub global_variables: Option<HashMap<String, serde_json::Value>>,

    /// Notified handlers (pending execution)
    pub notified_handlers: Vec<String>,

    /// Custom metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl PlaybookState {
    /// Create new playbook state
    pub fn new(playbook_name: impl Into<String>) -> Self {
        Self {
            playbook_name: playbook_name.into(),
            total_plays: 0,
            total_tasks: 0,
            completed_tasks: 0,
            current_play: 0,
            current_task: 0,
            hosts: HashMap::new(),
            global_variables: None,
            notified_handlers: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add a host to track
    pub fn add_host(&mut self, host: impl Into<String>) {
        let host = host.into();
        if !self.hosts.contains_key(&host) {
            self.hosts.insert(host.clone(), HostProgress::new(host));
        }
    }

    /// Update task progress for a host
    pub fn update_task(
        &mut self,
        host: &str,
        task_index: usize,
        task_name: &str,
        status: TaskCheckpointStatus,
        result: Option<serde_json::Value>,
    ) {
        if let Some(host_progress) = self.hosts.get_mut(host) {
            // Ensure task exists
            while host_progress.tasks.len() <= task_index {
                host_progress.tasks.push(TaskProgress {
                    name: String::new(),
                    index: host_progress.tasks.len(),
                    status: TaskCheckpointStatus::Pending,
                    result: None,
                    completed_at: None,
                });
            }

            // Update task
            host_progress.tasks[task_index] = TaskProgress {
                name: task_name.to_string(),
                index: task_index,
                status,
                result,
                completed_at: if status == TaskCheckpointStatus::Completed
                    || status == TaskCheckpointStatus::Failed
                    || status == TaskCheckpointStatus::Skipped
                {
                    Some(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    )
                } else {
                    None
                },
            };

            // Update current position
            if task_index >= host_progress.current_task {
                host_progress.current_task = task_index + 1;
            }
        }

        // Update completed count
        if status == TaskCheckpointStatus::Completed
            || status == TaskCheckpointStatus::Failed
            || status == TaskCheckpointStatus::Skipped
        {
            self.completed_tasks = self
                .hosts
                .values()
                .flat_map(|h| &h.tasks)
                .filter(|t| {
                    t.status != TaskCheckpointStatus::Pending
                        && t.status != TaskCheckpointStatus::InProgress
                })
                .count();
        }
    }

    /// Get the next task to execute for a host
    pub fn next_task_for_host(&self, host: &str) -> Option<(usize, usize)> {
        self.hosts
            .get(host)
            .map(|h| (h.current_play, h.current_task))
    }

    /// Check if playbook is complete
    pub fn is_complete(&self) -> bool {
        self.completed_tasks >= self.total_tasks
    }

    /// Get completion percentage
    pub fn completion_percentage(&self) -> f64 {
        if self.total_tasks == 0 {
            return 100.0;
        }
        (self.completed_tasks as f64 / self.total_tasks as f64) * 100.0
    }
}

/// A checkpoint representing execution state at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Unique checkpoint identifier
    pub id: CheckpointId,

    /// Playbook name
    pub playbook_name: String,

    /// Playbook execution state
    pub state: PlaybookState,

    /// Timestamp when checkpoint was created
    pub created_at: u64,

    /// Version of the checkpoint format
    pub version: u32,

    /// Optional description
    pub description: Option<String>,
}

impl Checkpoint {
    /// Create a new checkpoint
    pub fn new(playbook_name: impl Into<String>, state: PlaybookState) -> Self {
        Self {
            id: CheckpointId::new(),
            playbook_name: playbook_name.into(),
            state,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            version: 1,
            description: None,
        }
    }

    /// Add a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Check if checkpoint is expired
    pub fn is_expired(&self, max_age_hours: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age_hours = (now - self.created_at) / 3600;
        age_hours > max_age_hours
    }
}

/// Store for managing checkpoints
pub struct CheckpointStore {
    config: CheckpointConfig,
    checkpoints: HashMap<CheckpointId, Checkpoint>,
}

impl CheckpointStore {
    /// Create a new checkpoint store
    pub fn new(config: CheckpointConfig) -> Self {
        Self {
            config,
            checkpoints: HashMap::new(),
        }
    }

    /// Initialize the checkpoint directory
    pub fn init(&self) -> Result<(), CheckpointError> {
        if !self.config.checkpoint_dir.exists() {
            fs::create_dir_all(&self.config.checkpoint_dir)?;
        }
        Ok(())
    }

    /// Save a checkpoint
    pub fn save(&mut self, checkpoint: Checkpoint) -> Result<CheckpointId, CheckpointError> {
        self.init()?;

        let id = checkpoint.id.clone();
        let filename = format!("{}.json", id.as_str());
        let path = self.config.checkpoint_dir.join(&filename);

        // Serialize checkpoint
        let data = if self.config.compress {
            // Use gzip compression
            let json = serde_json::to_vec(&checkpoint)
                .map_err(|e| CheckpointError::Serialization(e.to_string()))?;
            compress_data(&json)?
        } else {
            serde_json::to_vec_pretty(&checkpoint)
                .map_err(|e| CheckpointError::Serialization(e.to_string()))?
        };

        fs::write(&path, &data)?;

        info!(
            "Saved checkpoint {} for playbook '{}' ({} bytes)",
            id,
            checkpoint.playbook_name,
            data.len()
        );

        // Store in memory
        self.checkpoints.insert(id.clone(), checkpoint);

        // Cleanup old checkpoints
        self.cleanup_old_checkpoints()?;

        Ok(id)
    }

    /// Load a checkpoint
    pub fn load(&self, id: &CheckpointId) -> Result<Checkpoint, CheckpointError> {
        // Check in-memory cache first
        if let Some(checkpoint) = self.checkpoints.get(id) {
            return Ok(checkpoint.clone());
        }

        // Load from file
        let filename = format!("{}.json", id.as_str());
        let path = self.config.checkpoint_dir.join(&filename);

        if !path.exists() {
            return Err(CheckpointError::NotFound(id.to_string()));
        }

        let data = fs::read(&path)?;

        let checkpoint: Checkpoint = if self.config.compress || data.starts_with(&[0x1f, 0x8b]) {
            let decompressed = decompress_data(&data)?;
            serde_json::from_slice(&decompressed)
                .map_err(|e| CheckpointError::Serialization(e.to_string()))?
        } else {
            serde_json::from_slice(&data)
                .map_err(|e| CheckpointError::Serialization(e.to_string()))?
        };

        // Check if expired
        if let Some(max_age) = self.config.max_age_hours {
            if checkpoint.is_expired(max_age) {
                return Err(CheckpointError::Expired(id.to_string()));
            }
        }

        Ok(checkpoint)
    }

    /// Delete a checkpoint
    pub fn delete(&mut self, id: &CheckpointId) -> Result<(), CheckpointError> {
        let filename = format!("{}.json", id.as_str());
        let path = self.config.checkpoint_dir.join(&filename);

        if path.exists() {
            fs::remove_file(&path)?;
        }

        self.checkpoints.remove(id);

        debug!("Deleted checkpoint {}", id);
        Ok(())
    }

    /// List all checkpoints for a playbook
    pub fn list_for_playbook(&self, playbook_name: &str) -> Vec<Checkpoint> {
        let mut checkpoints: Vec<Checkpoint> = self
            .list_all()
            .into_iter()
            .filter(|c| c.playbook_name == playbook_name)
            .collect();

        // Sort by creation time (newest first)
        checkpoints.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        checkpoints
    }

    /// List all checkpoints
    pub fn list_all(&self) -> Vec<Checkpoint> {
        let mut checkpoints = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.config.checkpoint_dir) {
            for entry in entries.flatten() {
                if let Some(filename) = entry.file_name().to_str() {
                    if filename.ends_with(".json") {
                        let id_str = filename.trim_end_matches(".json");
                        let id = CheckpointId::from_string(id_str);
                        if let Ok(checkpoint) = self.load(&id) {
                            checkpoints.push(checkpoint);
                        }
                    }
                }
            }
        }

        checkpoints
    }

    /// Get the latest checkpoint for a playbook
    pub fn latest_for_playbook(&self, playbook_name: &str) -> Option<Checkpoint> {
        self.list_for_playbook(playbook_name).into_iter().next()
    }

    /// Cleanup old checkpoints
    fn cleanup_old_checkpoints(&self) -> Result<(), CheckpointError> {
        if self.config.max_checkpoints_per_playbook.is_none() && self.config.max_age_hours.is_none()
        {
            return Ok(());
        }

        // Group checkpoints by playbook
        let all = self.list_all();
        let mut by_playbook: HashMap<String, Vec<Checkpoint>> = HashMap::new();
        for cp in all {
            by_playbook
                .entry(cp.playbook_name.clone())
                .or_default()
                .push(cp);
        }

        for (_, mut checkpoints) in by_playbook {
            // Sort by age (oldest first)
            checkpoints.sort_by(|a, b| a.created_at.cmp(&b.created_at));

            let mut to_delete = Vec::new();

            // Mark expired checkpoints
            if let Some(max_age) = self.config.max_age_hours {
                for cp in &checkpoints {
                    if cp.is_expired(max_age) {
                        to_delete.push(cp.id.clone());
                    }
                }
            }

            // Mark excess checkpoints
            if let Some(max_count) = self.config.max_checkpoints_per_playbook {
                if checkpoints.len() > max_count {
                    let excess = checkpoints.len() - max_count;
                    for cp in checkpoints.iter().take(excess) {
                        if !to_delete.contains(&cp.id) {
                            to_delete.push(cp.id.clone());
                        }
                    }
                }
            }

            // Delete marked checkpoints
            for id in to_delete {
                let filename = format!("{}.json", id.as_str());
                let path = self.config.checkpoint_dir.join(&filename);
                if path.exists() {
                    fs::remove_file(&path)?;
                    debug!("Cleaned up old checkpoint {}", id);
                }
            }
        }

        Ok(())
    }
}

/// Compress data using flate2 (gzip)
fn compress_data(data: &[u8]) -> Result<Vec<u8>, CheckpointError> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish().map_err(CheckpointError::Io)
}

/// Decompress gzip data
fn decompress_data(data: &[u8]) -> Result<Vec<u8>, CheckpointError> {
    use flate2::read::GzDecoder;
    use std::io::Read;

    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_checkpoint_id() {
        let id1 = CheckpointId::new();
        let id2 = CheckpointId::new();

        assert_ne!(id1, id2);
        assert!(id1.as_str().starts_with("cp-"));
    }

    #[test]
    fn test_playbook_state() {
        let mut state = PlaybookState::new("test.yml");
        state.total_tasks = 10;
        state.add_host("host1");
        state.add_host("host2");

        state.update_task("host1", 0, "task1", TaskCheckpointStatus::Completed, None);

        assert_eq!(state.hosts.len(), 2);
        assert_eq!(state.hosts.get("host1").unwrap().tasks.len(), 1);
    }

    #[test]
    fn test_checkpoint_store() {
        let dir = tempdir().unwrap();
        let config = CheckpointConfig {
            checkpoint_dir: dir.path().to_path_buf(),
            compress: false,
            ..Default::default()
        };

        let mut store = CheckpointStore::new(config);

        let mut state = PlaybookState::new("test.yml");
        state.add_host("host1");

        let checkpoint = Checkpoint::new("test.yml", state);
        let id = store.save(checkpoint.clone()).unwrap();

        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.playbook_name, "test.yml");
        assert_eq!(loaded.state.hosts.len(), 1);
    }

    #[test]
    fn test_checkpoint_compression() {
        let dir = tempdir().unwrap();
        let config = CheckpointConfig {
            checkpoint_dir: dir.path().to_path_buf(),
            compress: true,
            ..Default::default()
        };

        let mut store = CheckpointStore::new(config);

        let state = PlaybookState::new("test.yml");
        let checkpoint = Checkpoint::new("test.yml", state);
        let id = store.save(checkpoint).unwrap();

        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.playbook_name, "test.yml");
    }

    #[test]
    fn test_checkpoint_expiry() {
        let checkpoint = Checkpoint {
            id: CheckpointId::new(),
            playbook_name: "test.yml".to_string(),
            state: PlaybookState::new("test.yml"),
            created_at: 0, // Very old
            version: 1,
            description: None,
        };

        assert!(checkpoint.is_expired(24));
    }

    #[test]
    fn test_completion_percentage() {
        let mut state = PlaybookState::new("test.yml");
        state.total_tasks = 10;
        state.completed_tasks = 5;

        assert!((state.completion_percentage() - 50.0).abs() < f64::EPSILON);
    }
}
