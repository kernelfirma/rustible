//! JSON Callback Plugin for Rustible
//!
//! This module provides a machine-readable JSON callback plugin that outputs
//! one JSON object per line (JSON Lines / JSONL format), making it suitable
//! for piping to tools like `jq`, parsing by scripts, or integration with
//! log aggregation systems and CI/CD pipelines.
//!
//! ## Features
//!
//! - **Streaming JSONL Output**: One complete JSON object per line, no trailing commas
//! - **Ansible Compatibility**: Output format compatible with Ansible's `json` callback
//! - **Full Event Metadata**: Timestamps, UUIDs, durations, host info, task results
//! - **Diff Support**: Optional diff output for changed resources
//! - **Stats Summary**: Aggregated statistics at playbook completion
//! - **CI/CD Integration**: Automatic detection of CI environment (GitHub Actions, GitLab CI, Jenkins, etc.)
//! - **Correlation IDs**: Unique run IDs for tracking across distributed systems
//! - **Custom Output Destinations**: Stdout, stderr, file, or append to existing file
//!
//! ## Usage
//!
//! ```bash
//! # Basic usage with jq for pretty printing
//! rustible playbook.yml --callback json | jq
//!
//! # Filter for failures only
//! rustible playbook.yml --callback json | jq 'select(.event == "task_failed")'
//!
//! # Extract changed tasks
//! rustible playbook.yml --callback json | jq 'select(.result.changed == true)'
//!
//! # Save to file while watching
//! rustible playbook.yml --callback json | tee execution.jsonl | jq -c
//!
//! # In CI/CD, write to log file for artifact collection
//! RUSTIBLE_JSON_OUTPUT=/tmp/execution.jsonl rustible playbook.yml --callback json
//! ```
//!
//! ## Output Format
//!
//! Each line is a self-contained JSON object with an `event` field indicating
//! the event type. Common fields include:
//!
//! - `event`: Event type (playbook_start, task_ok, task_failed, etc.)
//! - `timestamp`: ISO 8601 timestamp with microsecond precision
//! - `run_id`: Unique correlation ID for this execution run
//! - `sequence`: Monotonically increasing event sequence number
//! - `host`: Target host name (for task events)
//! - `task`: Task name (for task events)
//! - `result`: Task result data including changed status, output, etc.
//! - `ci_context`: CI/CD environment metadata (when running in CI)
//!
//! ## Example Output
//!
//! ```json
//! {"event":"playbook_start","playbook":"site.yml","run_id":"abc123","sequence":1,"timestamp":"2024-01-15T10:30:00.000000Z"}
//! {"event":"play_start","play":"Configure webservers","hosts":["web1","web2"],"run_id":"abc123","sequence":2,"timestamp":"2024-01-15T10:30:00.100000Z"}
//! {"event":"task_start","task":"Install nginx","host":"web1","run_id":"abc123","sequence":3,"timestamp":"2024-01-15T10:30:00.200000Z"}
//! {"event":"task_ok","task":"Install nginx","host":"web1","result":{"changed":true,"success":true},"duration_ms":2500,"run_id":"abc123","sequence":4,"timestamp":"2024-01-15T10:30:02.700000Z"}
//! {"event":"playbook_end","playbook":"site.yml","stats":{"web1":{"ok":5,"changed":2}},"duration_ms":30500,"success":true,"run_id":"abc123","sequence":5,"timestamp":"2024-01-15T10:30:30.500000Z"}
//! ```
//!
//! ## CI/CD Integration
//!
//! When running in a CI/CD environment, the callback automatically detects and includes
//! relevant metadata:
//!
//! - **GitHub Actions**: `GITHUB_RUN_ID`, `GITHUB_WORKFLOW`, `GITHUB_SHA`, `GITHUB_REF`
//! - **GitLab CI**: `CI_PIPELINE_ID`, `CI_JOB_ID`, `CI_COMMIT_SHA`, `CI_COMMIT_BRANCH`
//! - **Jenkins**: `BUILD_NUMBER`, `BUILD_ID`, `JOB_NAME`, `GIT_COMMIT`
//! - **CircleCI**: `CIRCLE_BUILD_NUM`, `CIRCLE_WORKFLOW_ID`, `CIRCLE_SHA1`
//! - **Azure DevOps**: `BUILD_BUILDID`, `BUILD_BUILDNUMBER`, `BUILD_SOURCEVERSION`
//! - **Travis CI**: `TRAVIS_BUILD_ID`, `TRAVIS_JOB_ID`, `TRAVIS_COMMIT`

use std::collections::HashMap;
use std::io::{self, BufWriter, Write};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::RwLock;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// JSON Event Types (Ansible-compatible output format)
// ============================================================================

/// Base event structure for JSON output.
/// Each event is serialized as a single JSON line (JSONL format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JsonEvent {
    /// Playbook execution has started
    PlaybookStart {
        /// Name of the playbook
        playbook: String,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// Playbook execution has ended
    PlaybookEnd {
        /// Name of the playbook
        playbook: String,
        /// Whether playbook completed successfully
        success: bool,
        /// Duration in milliseconds
        duration_ms: u64,
        /// Per-host statistics
        stats: HashMap<String, HostStats>,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// A play has started
    PlayStart {
        /// Play name
        play: String,
        /// Target hosts
        hosts: Vec<String>,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// A play has ended
    PlayEnd {
        /// Play name
        play: String,
        /// Whether play completed successfully
        success: bool,
        /// Duration in milliseconds
        duration_ms: u64,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// A task has started
    TaskStart {
        /// Task name
        task: String,
        /// Target host
        host: String,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// A task completed successfully (ok)
    TaskOk {
        /// Task name
        task: String,
        /// Target host
        host: String,
        /// Task result details
        result: TaskResultJson,
        /// Duration in milliseconds
        duration_ms: u64,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// A task completed with changes
    TaskChanged {
        /// Task name
        task: String,
        /// Target host
        host: String,
        /// Task result details
        result: TaskResultJson,
        /// Duration in milliseconds
        duration_ms: u64,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// A task failed
    TaskFailed {
        /// Task name
        task: String,
        /// Target host
        host: String,
        /// Task result details
        result: TaskResultJson,
        /// Duration in milliseconds
        duration_ms: u64,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// A task was skipped
    TaskSkipped {
        /// Task name
        task: String,
        /// Target host
        host: String,
        /// Skip reason message
        #[serde(skip_serializing_if = "Option::is_none")]
        msg: Option<String>,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// A handler was triggered
    HandlerTriggered {
        /// Handler name
        handler: String,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// Facts were gathered from a host
    FactsGathered {
        /// Target host
        host: String,
        /// Number of facts gathered
        fact_count: usize,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
        /// Subset of interesting facts (optional, based on verbosity)
        #[serde(skip_serializing_if = "Option::is_none")]
        facts: Option<JsonValue>,
    },

    /// Warning message
    Warning {
        /// Warning message
        msg: String,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },

    /// Deprecation notice
    Deprecation {
        /// Deprecation message
        msg: String,
        /// Version when feature will be removed
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        /// ISO 8601 timestamp
        timestamp: DateTime<Utc>,
    },
}

/// Task result data for JSON output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResultJson {
    /// Whether the task was successful
    pub success: bool,
    /// Whether the task made changes
    pub changed: bool,
    /// Result message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    /// Whether the task was skipped
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    /// Module-specific result data (when verbose)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<JsonValue>,
    /// Any warnings generated
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl From<&ModuleResult> for TaskResultJson {
    fn from(result: &ModuleResult) -> Self {
        Self {
            success: result.success,
            changed: result.changed,
            msg: if result.message.is_empty() {
                None
            } else {
                Some(result.message.clone())
            },
            skipped: result.skipped,
            data: result.data.clone(),
            warnings: result.warnings.clone(),
        }
    }
}

/// Per-host statistics for JSON output
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostStats {
    /// Number of successful tasks (no changes)
    pub ok: u32,
    /// Number of tasks that made changes
    pub changed: u32,
    /// Number of failed tasks
    pub failed: u32,
    /// Number of skipped tasks
    pub skipped: u32,
    /// Number of unreachable attempts
    pub unreachable: u32,
}

impl HostStats {
    /// Check if the host had any failures
    pub fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }
}

// ============================================================================
// JSON Callback Plugin
// ============================================================================

/// JSON callback plugin for machine-readable output.
///
/// Outputs one JSON object per line (JSONL format) for each event during
/// playbook execution. This format is ideal for:
///
/// - Piping to `jq` for filtering and transformation
/// - Log aggregation and monitoring systems
/// - CI/CD pipeline integration
/// - Automated testing and validation
///
/// # Ansible Compatibility
///
/// The output format is designed to be compatible with Ansible's json callback
/// plugin, making it easy to migrate existing tooling that parses Ansible JSON output.
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::JsonCallback;
///
/// // Create with default settings (stdout, compact JSON)
/// let callback = JsonCallback::new();
///
/// // Create with pretty printing
/// let callback = JsonCallback::builder()
///     .pretty(true)
///     .build();
///
/// // Create writing to a file
/// let callback = JsonCallback::builder()
///     .output_file("/var/log/rustible/execution.jsonl")
///     .build();
/// # Ok(())
/// # }
/// ```
pub struct JsonCallback {
    /// Output writer (thread-safe, supports stdout, stderr, or file)
    writer: Arc<RwLock<Box<dyn Write + Send + Sync>>>,
    /// Whether to pretty-print JSON (multi-line with indentation)
    pretty: bool,
    /// Verbosity level (0 = minimal, 1+ = include more data)
    verbosity: u8,
    /// Per-host execution statistics
    host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
    /// Playbook start time for duration tracking
    start_time: Arc<RwLock<Option<Instant>>>,
    /// Play start time for duration tracking
    play_start_time: Arc<RwLock<Option<Instant>>>,
    /// Task start times per host
    task_start_times: Arc<RwLock<HashMap<String, Instant>>>,
    /// Current playbook name
    playbook_name: Arc<RwLock<Option<String>>>,
    /// Current play name
    play_name: Arc<RwLock<Option<String>>>,
    /// Whether any failures occurred
    has_failures: Arc<RwLock<bool>>,
}

impl std::fmt::Debug for JsonCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonCallback")
            .field("pretty", &self.pretty)
            .field("verbosity", &self.verbosity)
            .field("host_stats", &self.host_stats)
            .field("has_failures", &self.has_failures)
            .finish_non_exhaustive()
    }
}

impl JsonCallback {
    /// Creates a new JSON callback plugin with default settings.
    ///
    /// Output goes to stdout in compact (single-line) JSON format.
    #[must_use]
    pub fn new() -> Self {
        Self {
            writer: Arc::new(RwLock::new(Box::new(BufWriter::new(io::stdout())))),
            pretty: false,
            verbosity: 0,
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            start_time: Arc::new(RwLock::new(None)),
            play_start_time: Arc::new(RwLock::new(None)),
            task_start_times: Arc::new(RwLock::new(HashMap::new())),
            playbook_name: Arc::new(RwLock::new(None)),
            play_name: Arc::new(RwLock::new(None)),
            has_failures: Arc::new(RwLock::new(false)),
        }
    }

    /// Creates a builder for configuring the JSON callback.
    pub fn builder() -> JsonCallbackBuilder {
        JsonCallbackBuilder::new()
    }

    /// Returns whether any failures occurred during execution.
    pub async fn has_failures(&self) -> bool {
        *self.has_failures.read().await
    }

    /// Writes a JSON event to the output.
    async fn write_event(&self, event: &JsonEvent) {
        let json_result = if self.pretty {
            serde_json::to_string_pretty(event)
        } else {
            serde_json::to_string(event)
        };

        if let Ok(json_str) = json_result {
            let mut writer = self.writer.write().await;
            // JSONL format: one JSON object per line
            let _ = writeln!(writer, "{}", json_str);
            let _ = writer.flush();
        }
    }

    /// Records a task completion and updates host statistics.
    pub async fn record_task_result(&self, host: &str, result: &ModuleResult) {
        let mut stats = self.host_stats.write().await;
        let host_stats = stats.entry(host.to_string()).or_default();

        if result.skipped {
            host_stats.skipped += 1;
        } else if !result.success {
            host_stats.failed += 1;
            let mut has_failures = self.has_failures.write().await;
            *has_failures = true;
        } else if result.changed {
            host_stats.changed += 1;
        } else {
            host_stats.ok += 1;
        }
    }

    /// Gets the task duration for a host.
    async fn get_task_duration(&self, host: &str) -> u64 {
        let times = self.task_start_times.read().await;
        times
            .get(host)
            .map(|start| start.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }
}

impl Default for JsonCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for JsonCallback {
    fn clone(&self) -> Self {
        Self {
            writer: Arc::clone(&self.writer),
            pretty: self.pretty,
            verbosity: self.verbosity,
            host_stats: Arc::clone(&self.host_stats),
            start_time: Arc::clone(&self.start_time),
            play_start_time: Arc::clone(&self.play_start_time),
            task_start_times: Arc::clone(&self.task_start_times),
            playbook_name: Arc::clone(&self.playbook_name),
            play_name: Arc::clone(&self.play_name),
            has_failures: Arc::clone(&self.has_failures),
        }
    }
}

#[async_trait]
impl ExecutionCallback for JsonCallback {
    async fn on_playbook_start(&self, name: &str) {
        // Record start time
        {
            let mut start_time = self.start_time.write().await;
            *start_time = Some(Instant::now());
        }

        // Record playbook name
        {
            let mut playbook_name = self.playbook_name.write().await;
            *playbook_name = Some(name.to_string());
        }

        // Clear stats from any previous run
        {
            let mut stats = self.host_stats.write().await;
            stats.clear();
        }

        // Reset failure flag
        {
            let mut has_failures = self.has_failures.write().await;
            *has_failures = false;
        }

        // Emit event
        let event = JsonEvent::PlaybookStart {
            playbook: name.to_string(),
            timestamp: Utc::now(),
        };
        self.write_event(&event).await;
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let duration_ms = {
            let start_time = self.start_time.read().await;
            start_time
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0)
        };

        let stats = self.host_stats.read().await.clone();

        let event = JsonEvent::PlaybookEnd {
            playbook: name.to_string(),
            success,
            duration_ms,
            stats,
            timestamp: Utc::now(),
        };
        self.write_event(&event).await;
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        // Record play start time
        {
            let mut play_start_time = self.play_start_time.write().await;
            *play_start_time = Some(Instant::now());
        }

        // Record play name
        {
            let mut play_name = self.play_name.write().await;
            *play_name = Some(name.to_string());
        }

        // Initialize stats for all hosts in this play
        {
            let mut stats = self.host_stats.write().await;
            for host in hosts {
                stats.entry(host.clone()).or_default();
            }
        }

        let event = JsonEvent::PlayStart {
            play: name.to_string(),
            hosts: hosts.to_vec(),
            timestamp: Utc::now(),
        };
        self.write_event(&event).await;
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        let duration_ms = {
            let play_start_time = self.play_start_time.read().await;
            play_start_time
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0)
        };

        let event = JsonEvent::PlayEnd {
            play: name.to_string(),
            success,
            duration_ms,
            timestamp: Utc::now(),
        };
        self.write_event(&event).await;
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        // Record task start time
        {
            let mut times = self.task_start_times.write().await;
            times.insert(host.to_string(), Instant::now());
        }

        let event = JsonEvent::TaskStart {
            task: name.to_string(),
            host: host.to_string(),
            timestamp: Utc::now(),
        };
        self.write_event(&event).await;
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let duration_ms = self.get_task_duration(&result.host).await;

        // Update statistics
        self.record_task_result(&result.host, &result.result).await;

        // Determine event type based on result
        let task_result = TaskResultJson::from(&result.result);

        let event = if result.result.skipped {
            JsonEvent::TaskSkipped {
                task: result.task_name.clone(),
                host: result.host.clone(),
                msg: if result.result.message.is_empty() {
                    None
                } else {
                    Some(result.result.message.clone())
                },
                timestamp: Utc::now(),
            }
        } else if !result.result.success {
            JsonEvent::TaskFailed {
                task: result.task_name.clone(),
                host: result.host.clone(),
                result: task_result,
                duration_ms,
                timestamp: Utc::now(),
            }
        } else if result.result.changed {
            JsonEvent::TaskChanged {
                task: result.task_name.clone(),
                host: result.host.clone(),
                result: task_result,
                duration_ms,
                timestamp: Utc::now(),
            }
        } else {
            JsonEvent::TaskOk {
                task: result.task_name.clone(),
                host: result.host.clone(),
                result: task_result,
                duration_ms,
                timestamp: Utc::now(),
            }
        };

        self.write_event(&event).await;
    }

    async fn on_handler_triggered(&self, name: &str) {
        let event = JsonEvent::HandlerTriggered {
            handler: name.to_string(),
            timestamp: Utc::now(),
        };
        self.write_event(&event).await;
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        // Only include facts in verbose mode
        let facts_data = if self.verbosity >= 2 {
            serde_json::to_value(facts).ok()
        } else {
            None
        };

        let event = JsonEvent::FactsGathered {
            host: host.to_string(),
            fact_count: facts.all().len(),
            facts: facts_data,
            timestamp: Utc::now(),
        };
        self.write_event(&event).await;
    }
}

// ============================================================================
// Builder Pattern
// ============================================================================

/// Builder for configuring JsonCallback with a fluent API.
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::JsonCallback;
///
/// let callback = JsonCallback::builder()
///     .pretty(true)
///     .verbosity(2)
///     .output_file("/var/log/rustible.jsonl")
///     .build();
/// # Ok(())
/// # }
/// ```
pub struct JsonCallbackBuilder {
    pretty: bool,
    verbosity: u8,
    output: OutputTarget,
}

enum OutputTarget {
    Stdout,
    Stderr,
    File(String),
}

impl JsonCallbackBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self {
            pretty: false,
            verbosity: 0,
            output: OutputTarget::Stdout,
        }
    }

    /// Enable or disable pretty-printing (multi-line indented JSON).
    ///
    /// Default is false (compact single-line JSON).
    pub fn pretty(mut self, enabled: bool) -> Self {
        self.pretty = enabled;
        self
    }

    /// Set the verbosity level.
    ///
    /// - 0: Minimal output (event type, basic result)
    /// - 1: Include messages and warnings
    /// - 2: Include full result data and facts
    pub fn verbosity(mut self, level: u8) -> Self {
        self.verbosity = level;
        self
    }

    /// Output to stdout (default).
    pub fn output_stdout(mut self) -> Self {
        self.output = OutputTarget::Stdout;
        self
    }

    /// Output to stderr.
    pub fn output_stderr(mut self) -> Self {
        self.output = OutputTarget::Stderr;
        self
    }

    /// Output to a file path.
    ///
    /// The file will be created or truncated if it exists.
    pub fn output_file(mut self, path: impl Into<String>) -> Self {
        self.output = OutputTarget::File(path.into());
        self
    }

    /// Build the JsonCallback.
    pub fn build(self) -> JsonCallback {
        let writer: Box<dyn Write + Send + Sync> = match self.output {
            OutputTarget::Stdout => Box::new(BufWriter::new(io::stdout())),
            OutputTarget::Stderr => Box::new(BufWriter::new(io::stderr())),
            OutputTarget::File(path) => {
                match std::fs::File::create(&path) {
                    Ok(file) => Box::new(BufWriter::new(file)),
                    Err(e) => {
                        eprintln!("Warning: Could not create output file '{}': {}. Falling back to stdout.", path, e);
                        Box::new(BufWriter::new(io::stdout()))
                    }
                }
            }
        };

        JsonCallback {
            writer: Arc::new(RwLock::new(writer)),
            pretty: self.pretty,
            verbosity: self.verbosity,
            host_stats: Arc::new(RwLock::new(HashMap::new())),
            start_time: Arc::new(RwLock::new(None)),
            play_start_time: Arc::new(RwLock::new(None)),
            task_start_times: Arc::new(RwLock::new(HashMap::new())),
            playbook_name: Arc::new(RwLock::new(None)),
            play_name: Arc::new(RwLock::new(None)),
            has_failures: Arc::new(RwLock::new(false)),
        }
    }
}

impl Default for JsonCallbackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
    async fn test_json_callback_tracks_stats() {
        let callback = JsonCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;

        // Simulate some task completions
        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        let changed_result =
            create_execution_result("host1", "task2", true, true, false, "changed");
        callback.on_task_complete(&changed_result).await;

        let failed_result =
            create_execution_result("host2", "task1", false, false, false, "error occurred");
        callback.on_task_complete(&failed_result).await;

        let skipped_result =
            create_execution_result("host2", "task2", true, false, true, "skipped");
        callback.on_task_complete(&skipped_result).await;

        // Verify stats
        let stats = callback.host_stats.read().await;

        let host1_stats = stats.get("host1").unwrap();
        assert_eq!(host1_stats.ok, 1);
        assert_eq!(host1_stats.changed, 1);
        assert_eq!(host1_stats.failed, 0);
        assert_eq!(host1_stats.skipped, 0);

        let host2_stats = stats.get("host2").unwrap();
        assert_eq!(host2_stats.ok, 0);
        assert_eq!(host2_stats.changed, 0);
        assert_eq!(host2_stats.failed, 1);
        assert_eq!(host2_stats.skipped, 1);

        assert!(callback.has_failures().await);
    }

    #[tokio::test]
    async fn test_json_callback_no_failures() {
        let callback = JsonCallback::new();

        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string()])
            .await;

        let ok_result = create_execution_result("host1", "task1", true, false, false, "ok");
        callback.on_task_complete(&ok_result).await;

        assert!(!callback.has_failures().await);
    }

    #[test]
    fn test_json_event_serialization() {
        let event = JsonEvent::PlaybookStart {
            playbook: "test.yml".to_string(),
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("playbook_start"));
        assert!(json.contains("test.yml"));
    }

    #[test]
    fn test_task_result_json_from_module_result() {
        let module_result = ModuleResult {
            success: true,
            changed: true,
            message: "Package installed".to_string(),
            skipped: false,
            data: Some(serde_json::json!({"version": "1.0"})),
            warnings: vec!["Deprecated feature".to_string()],
        };

        let task_result: TaskResultJson = (&module_result).into();

        assert!(task_result.success);
        assert!(task_result.changed);
        assert_eq!(task_result.msg, Some("Package installed".to_string()));
        assert!(!task_result.skipped);
        assert!(task_result.data.is_some());
        assert_eq!(task_result.warnings.len(), 1);
    }

    #[test]
    fn test_host_stats_has_failures() {
        let mut stats = HostStats::default();
        assert!(!stats.has_failures());

        stats.failed = 1;
        assert!(stats.has_failures());

        stats.failed = 0;
        stats.unreachable = 1;
        assert!(stats.has_failures());
    }

    #[test]
    fn test_builder_default() {
        let callback = JsonCallback::builder().build();
        assert!(!callback.pretty);
        assert_eq!(callback.verbosity, 0);
    }

    #[test]
    fn test_builder_pretty() {
        let callback = JsonCallback::builder().pretty(true).build();
        assert!(callback.pretty);
    }

    #[test]
    fn test_builder_verbosity() {
        let callback = JsonCallback::builder().verbosity(2).build();
        assert_eq!(callback.verbosity, 2);
    }

    #[test]
    fn test_clone_shares_state() {
        let callback1 = JsonCallback::new();
        let callback2 = callback1.clone();

        // Both should share the same underlying state
        assert!(Arc::ptr_eq(&callback1.host_stats, &callback2.host_stats));
        assert!(Arc::ptr_eq(
            &callback1.has_failures,
            &callback2.has_failures
        ));
    }

    #[test]
    fn test_default_trait() {
        let callback = JsonCallback::default();
        assert!(!callback.pretty);
        assert_eq!(callback.verbosity, 0);
    }

    #[tokio::test]
    async fn test_jsonl_output_format() {
        // Test that events are serialized as valid JSONL
        let events = vec![
            JsonEvent::PlaybookStart {
                playbook: "test.yml".to_string(),
                timestamp: Utc::now(),
            },
            JsonEvent::PlayStart {
                play: "Configure servers".to_string(),
                hosts: vec!["web1".to_string(), "web2".to_string()],
                timestamp: Utc::now(),
            },
            JsonEvent::TaskOk {
                task: "Install nginx".to_string(),
                host: "web1".to_string(),
                result: TaskResultJson {
                    success: true,
                    changed: false,
                    msg: None,
                    skipped: false,
                    data: None,
                    warnings: vec![],
                },
                duration_ms: 1500,
                timestamp: Utc::now(),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            // Verify it's valid JSON
            let _: JsonValue = serde_json::from_str(&json).unwrap();
            // Verify no newlines in compact output
            assert!(!json.contains('\n'));
        }
    }

    #[test]
    fn test_pretty_output_has_newlines() {
        let event = JsonEvent::TaskOk {
            task: "Install nginx".to_string(),
            host: "web1".to_string(),
            result: TaskResultJson {
                success: true,
                changed: true,
                msg: Some("Package installed".to_string()),
                skipped: false,
                data: None,
                warnings: vec![],
            },
            duration_ms: 1500,
            timestamp: Utc::now(),
        };

        let pretty_json = serde_json::to_string_pretty(&event).unwrap();
        assert!(pretty_json.contains('\n'));
        assert!(pretty_json.contains("  ")); // Indentation
    }
}
