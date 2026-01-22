//! Tree callback plugin for Rustible.
//!
//! This plugin saves execution output to a hierarchical directory structure,
//! making it easy to browse results for large playbook runs.
//!
//! # Directory Structure
//!
//! The plugin creates the following structure:
//!
//! ```text
//! tree_root/
//! |-- _playbook_summary.json
//! |-- host1/
//! |   |-- _host_summary.json
//! |   |-- task_001_install_nginx.json
//! |   |-- task_002_configure_nginx.json
//! |   +-- task_003_start_nginx.json
//! +-- host2/
//!     |-- _host_summary.json
//!     |-- task_001_install_nginx.json
//!     +-- ...
//! ```
//!
//! # Features
//!
//! - Hierarchical output: `tree_root/host/task_name.json`
//! - Task results saved as individual JSON files
//! - Summary files per host with statistics
//! - Playbook-level summary with overall results
//! - Timestamps for forensic analysis
//! - Works well with large playbook runs
//!
//! # Example Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::TreeCallback;
//!
//! let callback = TreeCallback::new("/var/log/rustible/runs/2024-01-15-deploy")?;
//! # let _ = ();
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{debug, error, warn};

use crate::error::Result;
use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Data Structures for Serialization
// ============================================================================

/// Metadata stored with each task result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMetadata {
    /// Task name from playbook
    pub task_name: String,
    /// Host this task ran on
    pub host: String,
    /// Sequence number for ordering
    pub sequence: u32,
    /// Start timestamp (ISO 8601)
    pub started_at: DateTime<Utc>,
    /// End timestamp (ISO 8601)
    pub completed_at: DateTime<Utc>,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Play name this task belongs to
    pub play_name: Option<String>,
}

/// Result data stored for each task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResultData {
    /// Metadata about the task execution
    pub metadata: TaskMetadata,
    /// Whether the task succeeded
    pub success: bool,
    /// Whether the task made changes
    pub changed: bool,
    /// Whether the task was skipped
    pub skipped: bool,
    /// Result message
    pub message: String,
    /// Additional data from the module
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Warnings from the module
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
    /// Handlers to notify
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub notify: Vec<String>,
}

/// Statistics tracked per host.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreeHostStats {
    /// Total tasks executed
    pub total: u32,
    /// Successful tasks (no changes)
    pub ok: u32,
    /// Tasks that made changes
    pub changed: u32,
    /// Failed tasks
    pub failed: u32,
    /// Skipped tasks
    pub skipped: u32,
    /// Unreachable attempts
    pub unreachable: u32,
    /// Total execution time in milliseconds
    pub total_duration_ms: u64,
}

/// Host summary saved to _host_summary.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeHostSummary {
    /// Hostname
    pub host: String,
    /// Execution statistics
    pub stats: TreeHostStats,
    /// When execution started for this host
    pub started_at: DateTime<Utc>,
    /// When execution completed for this host
    pub completed_at: DateTime<Utc>,
    /// List of task files in order
    pub task_files: Vec<String>,
    /// Facts gathered for this host (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facts: Option<serde_json::Value>,
}

/// Playbook summary saved to _playbook_summary.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreePlaybookSummary {
    /// Playbook name
    pub playbook: String,
    /// Overall success status
    pub success: bool,
    /// When playbook execution started
    pub started_at: DateTime<Utc>,
    /// When playbook execution completed
    pub completed_at: DateTime<Utc>,
    /// Total duration in milliseconds
    pub duration_ms: u64,
    /// Aggregate statistics across all hosts
    pub total_stats: TreeHostStats,
    /// Per-host statistics
    pub hosts: HashMap<String, TreeHostStats>,
    /// List of plays executed
    pub plays: Vec<String>,
}

// ============================================================================
// Internal State
// ============================================================================

/// Internal state for tracking a host during execution.
#[derive(Debug)]
struct HostState {
    /// Statistics for this host
    stats: TreeHostStats,
    /// When we started executing on this host
    started_at: DateTime<Utc>,
    /// Task files written for this host
    task_files: Vec<String>,
    /// Current task sequence number
    task_sequence: AtomicU32,
    /// Facts gathered for this host
    facts: Option<serde_json::Value>,
}

impl Default for HostState {
    fn default() -> Self {
        Self {
            stats: TreeHostStats::default(),
            started_at: Utc::now(),
            task_files: Vec::new(),
            task_sequence: AtomicU32::new(0),
            facts: None,
        }
    }
}

/// Configuration options for the tree callback.
#[derive(Debug, Clone)]
pub struct TreeConfig {
    /// Whether to save facts in host summaries
    pub save_facts: bool,
    /// Whether to save task data (additional module output)
    pub save_task_data: bool,
    /// Maximum task name length in filenames
    pub max_task_name_len: usize,
    /// Whether to create timestamped subdirectory
    pub use_timestamp_subdir: bool,
}

impl Default for TreeConfig {
    fn default() -> Self {
        Self {
            save_facts: true,
            save_task_data: true,
            max_task_name_len: 80,
            use_timestamp_subdir: false,
        }
    }
}

// ============================================================================
// Tree Callback Implementation
// ============================================================================

/// Tree callback plugin that saves output to a directory structure.
///
/// This callback organizes execution results in a browsable tree structure,
/// with each host getting its own directory and each task saved as a JSON file.
///
/// # Design Principles
///
/// 1. **Hierarchical Organization**: Results are organized by host, then by task
/// 2. **JSON Format**: All output is machine-readable JSON
/// 3. **Sequence Numbering**: Tasks are numbered for ordering
/// 4. **Summary Files**: Both per-host and playbook-level summaries
/// 5. **Timestamps**: Full timing information for analysis
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::TreeCallback;
///
/// // Create with auto-generated run directory
/// let callback = TreeCallback::new_with_timestamp("/var/log/rustible/runs")?;
///
/// // Or specify exact path
/// let callback = TreeCallback::new("/var/log/rustible/runs/my-deploy")?;
///
/// # let _ = ();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct TreeCallback {
    /// Root directory for the tree structure
    tree_root: PathBuf,
    /// Configuration options
    config: TreeConfig,
    /// Per-host state tracking
    host_states: Arc<RwLock<HashMap<String, HostState>>>,
    /// Playbook start time
    playbook_started_at: Arc<RwLock<Option<DateTime<Utc>>>>,
    /// Current playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Current play name
    current_play: Arc<RwLock<Option<String>>>,
    /// List of plays executed
    plays: Arc<RwLock<Vec<String>>>,
    /// Overall start time for duration calculation
    start_instant: Arc<RwLock<Option<Instant>>>,
    /// Whether any failures occurred
    has_failures: Arc<RwLock<bool>>,
}

impl TreeCallback {
    /// Creates a new tree callback with the specified root directory.
    ///
    /// The directory will be created if it doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `tree_root` - Path to the root directory for output
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = TreeCallback::new("/var/log/rustible/runs/2024-01-15")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(tree_root: impl AsRef<Path>) -> Result<Self> {
        Self::with_config(tree_root, TreeConfig::default())
    }

    /// Creates a new tree callback with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `tree_root` - Path to the root directory for output
    /// * `config` - Configuration options
    pub fn with_config(tree_root: impl AsRef<Path>, config: TreeConfig) -> Result<Self> {
        let tree_root = tree_root.as_ref().to_path_buf();

        Ok(Self {
            tree_root,
            config,
            host_states: Arc::new(RwLock::new(HashMap::new())),
            playbook_started_at: Arc::new(RwLock::new(None)),
            playbook_name: Arc::new(RwLock::new(None)),
            current_play: Arc::new(RwLock::new(None)),
            plays: Arc::new(RwLock::new(Vec::new())),
            start_instant: Arc::new(RwLock::new(None)),
            has_failures: Arc::new(RwLock::new(false)),
        })
    }

    /// Creates a new tree callback with an auto-generated timestamped subdirectory.
    ///
    /// This creates a directory like `base_path/2024-01-15T10-30-00Z` for each run.
    ///
    /// # Arguments
    ///
    /// * `base_path` - Base path where timestamped directories will be created
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// // Creates /var/log/rustible/runs/2024-01-15T10-30-00Z/
    /// let callback = TreeCallback::new_with_timestamp("/var/log/rustible/runs")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new_with_timestamp(base_path: impl AsRef<Path>) -> Result<Self> {
        let timestamp = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let tree_root = base_path.as_ref().join(timestamp);
        Self::new(tree_root)
    }

    /// Returns the root directory path for this callback.
    #[must_use]
    pub fn tree_root(&self) -> &Path {
        &self.tree_root
    }

    /// Returns whether any failures occurred during execution.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        *self.has_failures.read()
    }

    /// Gets the directory path for a specific host.
    fn host_dir(&self, host: &str) -> PathBuf {
        self.tree_root.join(sanitize_filename(host))
    }

    /// Generates a filename for a task result.
    fn task_filename(&self, sequence: u32, task_name: &str) -> String {
        let sanitized_name = sanitize_filename(task_name);
        // Truncate task name if too long (keep filenames reasonable)
        let truncated = if sanitized_name.len() > self.config.max_task_name_len {
            &sanitized_name[..self.config.max_task_name_len]
        } else {
            &sanitized_name
        };
        format!("task_{:03}_{}.json", sequence, truncated)
    }

    /// Writes a task result to the appropriate file.
    async fn write_task_result(
        &self,
        host: &str,
        task_name: &str,
        sequence: u32,
        result: TaskResultData,
    ) -> std::io::Result<String> {
        let host_dir = self.host_dir(host);
        fs::create_dir_all(&host_dir).await?;

        let filename = self.task_filename(sequence, task_name);
        let file_path = host_dir.join(&filename);

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        fs::write(&file_path, json).await?;
        debug!("Wrote task result to {}", file_path.display());

        Ok(filename)
    }

    /// Writes the host summary file.
    async fn write_host_summary(
        &self,
        host: &str,
        summary: TreeHostSummary,
    ) -> std::io::Result<()> {
        let host_dir = self.host_dir(host);
        let file_path = host_dir.join("_host_summary.json");

        let json = serde_json::to_string_pretty(&summary)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        fs::write(&file_path, json).await?;
        debug!("Wrote host summary to {}", file_path.display());

        Ok(())
    }

    /// Writes the playbook summary file.
    async fn write_playbook_summary(&self, summary: TreePlaybookSummary) -> std::io::Result<()> {
        let file_path = self.tree_root.join("_playbook_summary.json");

        let json = serde_json::to_string_pretty(&summary)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        fs::write(&file_path, json).await?;
        debug!("Wrote playbook summary to {}", file_path.display());

        Ok(())
    }
}

impl Clone for TreeCallback {
    fn clone(&self) -> Self {
        Self {
            tree_root: self.tree_root.clone(),
            config: self.config.clone(),
            host_states: Arc::clone(&self.host_states),
            playbook_started_at: Arc::clone(&self.playbook_started_at),
            playbook_name: Arc::clone(&self.playbook_name),
            current_play: Arc::clone(&self.current_play),
            plays: Arc::clone(&self.plays),
            start_instant: Arc::clone(&self.start_instant),
            has_failures: Arc::clone(&self.has_failures),
        }
    }
}

impl Default for TreeCallback {
    fn default() -> Self {
        Self::new("/tmp/rustible-tree").expect("Failed to create default TreeCallback")
    }
}

#[async_trait]
impl ExecutionCallback for TreeCallback {
    /// Called when a playbook starts - creates the root directory.
    async fn on_playbook_start(&self, name: &str) {
        // Create the tree root directory
        if let Err(e) = fs::create_dir_all(&self.tree_root).await {
            error!(
                "TreeCallback: Failed to create tree root {}: {}",
                self.tree_root.display(),
                e
            );
            return;
        }

        debug!(
            "TreeCallback: Created tree root at {}",
            self.tree_root.display()
        );

        // Initialize state
        *self.playbook_started_at.write() = Some(Utc::now());
        *self.playbook_name.write() = Some(name.to_string());
        *self.start_instant.write() = Some(Instant::now());

        // Clear state from previous runs
        self.host_states.write().clear();
        self.plays.write().clear();
        *self.has_failures.write() = false;
    }

    /// Called when a playbook ends - writes final summaries.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let now = Utc::now();

        // Collect all data under the lock, then release before async operations
        let (host_summaries, total_stats, host_stats_map, started_at, duration_ms, plays) = {
            let host_states = self.host_states.read();
            let started_at = *self.playbook_started_at.read();
            let start_instant = *self.start_instant.read();
            let plays = self.plays.read().clone();

            // Collect host summaries
            let mut summaries = Vec::new();
            for (host, state) in host_states.iter() {
                let facts = if self.config.save_facts {
                    state.facts.clone()
                } else {
                    None
                };

                let summary = TreeHostSummary {
                    host: host.clone(),
                    stats: state.stats.clone(),
                    started_at: state.started_at,
                    completed_at: now,
                    task_files: state.task_files.clone(),
                    facts,
                };
                summaries.push((host.clone(), summary));
            }

            // Calculate aggregate statistics
            let mut total_stats = TreeHostStats::default();
            let mut host_stats_map = HashMap::new();

            for (host, state) in host_states.iter() {
                total_stats.total += state.stats.total;
                total_stats.ok += state.stats.ok;
                total_stats.changed += state.stats.changed;
                total_stats.failed += state.stats.failed;
                total_stats.skipped += state.stats.skipped;
                total_stats.unreachable += state.stats.unreachable;
                total_stats.total_duration_ms += state.stats.total_duration_ms;
                host_stats_map.insert(host.clone(), state.stats.clone());
            }

            // Calculate playbook duration
            let duration_ms = start_instant
                .map(|start| start.elapsed().as_millis() as u64)
                .unwrap_or(0);

            (
                summaries,
                total_stats,
                host_stats_map,
                started_at,
                duration_ms,
                plays,
            )
        };
        // Lock released here

        // Write host summaries (async operations now safe)
        for (host, summary) in host_summaries {
            if let Err(e) = self.write_host_summary(&host, summary).await {
                error!(
                    "TreeCallback: Failed to write host summary for {}: {}",
                    host, e
                );
            }
        }

        // Write playbook summary
        let summary = TreePlaybookSummary {
            playbook: name.to_string(),
            success,
            started_at: started_at.unwrap_or_else(Utc::now),
            completed_at: now,
            duration_ms,
            total_stats,
            hosts: host_stats_map,
            plays,
        };

        if let Err(e) = self.write_playbook_summary(summary).await {
            error!("TreeCallback: Failed to write playbook summary: {}", e);
        }

        debug!(
            "TreeCallback: Playbook '{}' completed, wrote summaries to {}",
            name,
            self.tree_root.display()
        );
    }

    /// Called when a play starts - records the play name.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Update current play
        *self.current_play.write() = Some(name.to_string());

        // Add to plays list
        {
            let mut plays = self.plays.write();
            if !plays.contains(&name.to_string()) {
                plays.push(name.to_string());
            }
        }

        // Initialize host states for all hosts in this play
        let mut host_states = self.host_states.write();
        for host in hosts {
            host_states.entry(host.clone()).or_default();
        }

        debug!(
            "TreeCallback: Play '{}' started with {} hosts",
            name,
            hosts.len()
        );
    }

    /// Called when a play ends.
    async fn on_play_end(&self, name: &str, _success: bool) {
        debug!("TreeCallback: Play '{}' ended", name);
    }

    /// Called when a task starts - currently no action needed.
    async fn on_task_start(&self, _name: &str, _host: &str) {
        // Task start is recorded when the task completes with full timing info
    }

    /// Called when a task completes - writes the task result to a file.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let now = Utc::now();
        let current_play = self.current_play.read().clone();

        // Get or create host state and update it
        let sequence = {
            let mut host_states = self.host_states.write();
            let host_state = host_states.entry(result.host.clone()).or_default();

            // Increment sequence and get the current value
            let seq = host_state.task_sequence.fetch_add(1, Ordering::SeqCst);

            // Update statistics
            host_state.stats.total += 1;
            if result.result.skipped {
                host_state.stats.skipped += 1;
            } else if !result.result.success {
                host_state.stats.failed += 1;
            } else if result.result.changed {
                host_state.stats.changed += 1;
            } else {
                host_state.stats.ok += 1;
            }
            host_state.stats.total_duration_ms += result.duration.as_millis() as u64;

            seq
        };

        // Mark failures
        if !result.result.success {
            *self.has_failures.write() = true;
        }

        // Calculate task started_at from completion time and duration
        let started_at = now - chrono::Duration::from_std(result.duration).unwrap_or_default();

        // Build task result data
        let task_data = TaskResultData {
            metadata: TaskMetadata {
                task_name: result.task_name.clone(),
                host: result.host.clone(),
                sequence,
                started_at,
                completed_at: now,
                duration_ms: result.duration.as_millis() as u64,
                play_name: current_play,
            },
            success: result.result.success,
            changed: result.result.changed,
            skipped: result.result.skipped,
            message: result.result.message.clone(),
            data: if self.config.save_task_data {
                result.result.data.clone()
            } else {
                None
            },
            warnings: result.result.warnings.clone(),
            notify: result.notify.clone(),
        };

        // Write the task result file
        match self
            .write_task_result(&result.host, &result.task_name, sequence, task_data)
            .await
        {
            Ok(filename) => {
                // Record the filename in host state
                let mut host_states = self.host_states.write();
                if let Some(host_state) = host_states.get_mut(&result.host) {
                    host_state.task_files.push(filename);
                }
            }
            Err(e) => {
                error!(
                    "TreeCallback: Failed to write task result for {} on {}: {}",
                    result.task_name, result.host, e
                );
            }
        }
    }

    /// Called when a handler is triggered - logged in task results.
    async fn on_handler_triggered(&self, name: &str) {
        debug!("TreeCallback: Handler '{}' triggered", name);
    }

    /// Called when facts are gathered - stores facts for the host summary.
    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        if !self.config.save_facts {
            return;
        }

        let mut host_states = self.host_states.write();
        if let Some(host_state) = host_states.get_mut(host) {
            // Convert facts to JSON value for storage
            match serde_json::to_value(facts) {
                Ok(facts_json) => {
                    host_state.facts = Some(facts_json);
                    debug!("TreeCallback: Stored facts for host '{}'", host);
                }
                Err(e) => {
                    warn!(
                        "TreeCallback: Failed to serialize facts for {}: {}",
                        host, e
                    );
                }
            }
        }
    }
}

// ============================================================================
// Unreachable Callback Trait
// ============================================================================

/// Trait extension for handling unreachable hosts.
#[async_trait]
pub trait TreeUnreachableCallback: ExecutionCallback {
    /// Called when a host becomes unreachable.
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str);
}

#[async_trait]
impl TreeUnreachableCallback for TreeCallback {
    async fn on_host_unreachable(&self, host: &str, task_name: &str, error: &str) {
        let now = Utc::now();
        let current_play = self.current_play.read().clone();

        // Get or create host state and update unreachable count
        let sequence = {
            let mut host_states = self.host_states.write();
            let host_state = host_states.entry(host.to_string()).or_default();
            let seq = host_state.task_sequence.fetch_add(1, Ordering::SeqCst);
            host_state.stats.total += 1;
            host_state.stats.unreachable += 1;
            seq
        };

        // Mark as failure
        *self.has_failures.write() = true;

        // Build unreachable task result
        let task_data = TaskResultData {
            metadata: TaskMetadata {
                task_name: task_name.to_string(),
                host: host.to_string(),
                sequence,
                started_at: now,
                completed_at: now,
                duration_ms: 0,
                play_name: current_play,
            },
            success: false,
            changed: false,
            skipped: false,
            message: format!("UNREACHABLE: {}", error),
            data: None,
            warnings: Vec::new(),
            notify: Vec::new(),
        };

        // Write the task result file
        match self
            .write_task_result(host, task_name, sequence, task_data)
            .await
        {
            Ok(filename) => {
                let mut host_states = self.host_states.write();
                if let Some(host_state) = host_states.get_mut(host) {
                    host_state.task_files.push(filename);
                }
            }
            Err(e) => {
                error!(
                    "TreeCallback: Failed to write unreachable result for {} on {}: {}",
                    task_name, host, e
                );
            }
        }

        warn!(
            "TreeCallback: Host '{}' unreachable during '{}': {}",
            host, task_name, error
        );
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Sanitizes a string for use as a filename.
///
/// Replaces or removes characters that are problematic in filenames.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ' ' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;
    use std::time::Duration;
    use tempfile::TempDir;

    fn create_execution_result(
        host: &str,
        task_name: &str,
        success: bool,
        changed: bool,
        skipped: bool,
        message: &str,
    ) -> ExecutionResult {
        ExecutionResult {
            host: host.to_string(),
            task_name: task_name.to_string(),
            result: ModuleResult {
                success,
                changed,
                message: message.to_string(),
                skipped,
                data: None,
                warnings: Vec::new(),
            },
            duration: Duration::from_millis(100),
            notify: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_tree_callback_creates_structure() {
        let temp_dir = TempDir::new().unwrap();
        let callback = TreeCallback::new(temp_dir.path().join("tree")).unwrap();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        // Execute some tasks
        let result1 = create_execution_result("host1", "Install nginx", true, true, false, "ok");
        callback.on_task_complete(&result1).await;

        let result2 = create_execution_result("host1", "Start nginx", true, false, false, "ok");
        callback.on_task_complete(&result2).await;

        let result3 =
            create_execution_result("host2", "Install nginx", false, false, false, "failed");
        callback.on_task_complete(&result3).await;

        callback.on_play_end("test-play", true).await;
        callback.on_playbook_end("test-playbook", false).await;

        // Verify directory structure
        let tree_root = temp_dir.path().join("tree");
        assert!(tree_root.exists());
        assert!(tree_root.join("host1").exists());
        assert!(tree_root.join("host2").exists());

        // Verify task files
        assert!(tree_root.join("host1/task_000_Install_nginx.json").exists());
        assert!(tree_root.join("host1/task_001_Start_nginx.json").exists());
        assert!(tree_root.join("host2/task_000_Install_nginx.json").exists());

        // Verify summary files
        assert!(tree_root.join("host1/_host_summary.json").exists());
        assert!(tree_root.join("host2/_host_summary.json").exists());
        assert!(tree_root.join("_playbook_summary.json").exists());

        // Verify failures tracked
        assert!(callback.has_failures());
    }

    #[tokio::test]
    async fn test_tree_callback_task_content() {
        let temp_dir = TempDir::new().unwrap();
        let callback = TreeCallback::new(temp_dir.path().join("tree")).unwrap();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let result = create_execution_result(
            "host1",
            "Install nginx",
            true,
            true,
            false,
            "Package installed",
        );
        callback.on_task_complete(&result).await;

        callback.on_playbook_end("test-playbook", true).await;

        // Read and verify task file content
        let task_file = temp_dir
            .path()
            .join("tree/host1/task_000_Install_nginx.json");
        let content = fs::read_to_string(&task_file).await.unwrap();
        let task_data: TaskResultData = serde_json::from_str(&content).unwrap();

        assert_eq!(task_data.metadata.task_name, "Install nginx");
        assert_eq!(task_data.metadata.host, "host1");
        assert_eq!(task_data.metadata.sequence, 0);
        assert!(task_data.success);
        assert!(task_data.changed);
        assert!(!task_data.skipped);
        assert_eq!(task_data.message, "Package installed");
    }

    #[tokio::test]
    async fn test_tree_callback_host_summary() {
        let temp_dir = TempDir::new().unwrap();
        let callback = TreeCallback::new(temp_dir.path().join("tree")).unwrap();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        // Various task results
        callback
            .on_task_complete(&create_execution_result(
                "host1", "task1", true, false, false, "ok",
            ))
            .await;
        callback
            .on_task_complete(&create_execution_result(
                "host1", "task2", true, true, false, "changed",
            ))
            .await;
        callback
            .on_task_complete(&create_execution_result(
                "host1", "task3", false, false, false, "failed",
            ))
            .await;
        callback
            .on_task_complete(&create_execution_result(
                "host1", "task4", true, false, true, "skipped",
            ))
            .await;

        callback.on_playbook_end("test-playbook", false).await;

        // Read and verify host summary
        let summary_file = temp_dir.path().join("tree/host1/_host_summary.json");
        let content = fs::read_to_string(&summary_file).await.unwrap();
        let summary: TreeHostSummary = serde_json::from_str(&content).unwrap();

        assert_eq!(summary.host, "host1");
        assert_eq!(summary.stats.total, 4);
        assert_eq!(summary.stats.ok, 1);
        assert_eq!(summary.stats.changed, 1);
        assert_eq!(summary.stats.failed, 1);
        assert_eq!(summary.stats.skipped, 1);
        assert_eq!(summary.task_files.len(), 4);
    }

    #[tokio::test]
    async fn test_tree_callback_playbook_summary() {
        let temp_dir = TempDir::new().unwrap();
        let callback = TreeCallback::new(temp_dir.path().join("tree")).unwrap();

        callback.on_playbook_start("deploy-app").await;
        callback
            .on_play_start("install", &["web1".to_string(), "web2".to_string()])
            .await;

        callback
            .on_task_complete(&create_execution_result(
                "web1", "install", true, true, false, "ok",
            ))
            .await;
        callback
            .on_task_complete(&create_execution_result(
                "web2", "install", true, true, false, "ok",
            ))
            .await;

        callback.on_play_end("install", true).await;
        callback.on_playbook_end("deploy-app", true).await;

        // Read and verify playbook summary
        let summary_file = temp_dir.path().join("tree/_playbook_summary.json");
        let content = fs::read_to_string(&summary_file).await.unwrap();
        let summary: TreePlaybookSummary = serde_json::from_str(&content).unwrap();

        assert_eq!(summary.playbook, "deploy-app");
        assert!(summary.success);
        assert_eq!(summary.total_stats.total, 2);
        assert_eq!(summary.total_stats.changed, 2);
        assert_eq!(summary.hosts.len(), 2);
        assert!(summary.plays.contains(&"install".to_string()));
    }

    #[tokio::test]
    async fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("simple"), "simple");
        assert_eq!(sanitize_filename("with spaces"), "with_spaces");
        assert_eq!(sanitize_filename("path/to/file"), "path_to_file");
        assert_eq!(sanitize_filename("file:name"), "file_name");
        // Note: trim_matches('_') removes trailing underscores
        assert_eq!(sanitize_filename("file*name?"), "file_name");
        // Leading/trailing spaces become _, then get trimmed
        assert_eq!(sanitize_filename("  trimmed  "), "trimmed");
    }

    #[tokio::test]
    async fn test_tree_callback_unreachable() {
        let temp_dir = TempDir::new().unwrap();
        let callback = TreeCallback::new(temp_dir.path().join("tree")).unwrap();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        callback
            .on_host_unreachable("host1", "gather_facts", "Connection refused")
            .await;

        callback.on_playbook_end("test-playbook", false).await;

        // Verify unreachable was recorded
        assert!(callback.has_failures());

        let summary_file = temp_dir.path().join("tree/host1/_host_summary.json");
        let content = fs::read_to_string(&summary_file).await.unwrap();
        let summary: TreeHostSummary = serde_json::from_str(&content).unwrap();

        assert_eq!(summary.stats.unreachable, 1);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = TreeCallback::new("/tmp/test").unwrap();
        let callback2 = callback1.clone();

        assert!(Arc::ptr_eq(&callback1.host_states, &callback2.host_states));
        assert!(Arc::ptr_eq(
            &callback1.has_failures,
            &callback2.has_failures
        ));
    }

    #[test]
    fn test_task_filename_generation() {
        let callback = TreeCallback::new("/tmp/test").unwrap();

        assert_eq!(
            callback.task_filename(0, "Install nginx"),
            "task_000_Install_nginx.json"
        );
        assert_eq!(
            callback.task_filename(1, "Configure firewall"),
            "task_001_Configure_firewall.json"
        );
        assert_eq!(
            callback.task_filename(99, "Deploy app"),
            "task_099_Deploy_app.json"
        );

        // Long task names should be truncated
        let long_name = "a".repeat(100);
        let filename = callback.task_filename(0, &long_name);
        assert!(filename.len() < 100);
    }
}
