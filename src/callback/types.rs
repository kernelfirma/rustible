//! Callback event types for Rustible's plugin system
//!
//! This module defines the event and context types used by the callback system
//! to notify plugins about playbook execution lifecycle events.
//!
//! ## Event Categories
//!
//! - **Playbook Events**: Start/end of entire playbook execution
//! - **Play Events**: Start/end of individual plays
//! - **Task Events**: Task lifecycle (start, ok, failed, skipped, unreachable)
//! - **Handler Events**: Handler triggering and execution
//! - **Runner Events**: Loop iteration and retry events
//! - **Stats Events**: Final execution statistics

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// Re-export executor types for convenience
pub use crate::executor::task::{TaskDiff, TaskResult, TaskStatus};
pub use crate::executor::{ExecutionStats, HostResult};

// ============================================================================
// Core Event Enum
// ============================================================================

/// All possible callback events during playbook execution.
///
/// Events are emitted at various lifecycle points and carry context-specific
/// information about the current state of execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum CallbackEvent {
    // -------------------------------------------------------------------------
    // Playbook Lifecycle Events
    // -------------------------------------------------------------------------
    /// Emitted when a playbook starts execution.
    ///
    /// This is the first event for any playbook run.
    PlaybookStart {
        /// Information about the playbook being executed
        playbook: PlaybookInfo,
        /// Timestamp when execution started
        #[serde(skip)]
        started_at: Option<Instant>,
    },

    /// Emitted when a playbook completes execution (success or failure).
    ///
    /// This is the last event for any playbook run.
    PlaybookEnd {
        /// Information about the playbook that was executed
        playbook: PlaybookInfo,
        /// Final execution statistics per host
        stats: HashMap<String, ExecutionStats>,
        /// Whether the playbook completed successfully
        success: bool,
        /// Total duration of playbook execution
        #[serde(with = "duration_serde")]
        duration: Duration,
    },

    // -------------------------------------------------------------------------
    // Play Lifecycle Events
    // -------------------------------------------------------------------------
    /// Emitted when a play starts execution.
    PlayStart {
        /// Information about the play being executed
        play: PlayInfo,
        /// Index of this play within the playbook (0-based)
        play_index: usize,
    },

    /// Emitted when a play completes execution.
    PlayEnd {
        /// Information about the play that was executed
        play: PlayInfo,
        /// Index of this play within the playbook (0-based)
        play_index: usize,
        /// Execution statistics for this play
        stats: HashMap<String, ExecutionStats>,
        /// Whether the play completed successfully
        success: bool,
        /// Duration of play execution
        #[serde(with = "duration_serde")]
        duration: Duration,
    },

    // -------------------------------------------------------------------------
    // Task Lifecycle Events
    // -------------------------------------------------------------------------
    /// Emitted when a task starts execution on a host.
    TaskStart {
        /// Information about the task being executed
        task: TaskInfo,
        /// Whether this is a handler task
        is_handler: bool,
        /// Whether this task is conditional
        is_conditional: bool,
    },

    /// Emitted when a task completes successfully.
    TaskOk {
        /// Information about the task that was executed
        task: TaskInfo,
        /// Result information from the task
        result: ResultInfo,
        /// Whether this is a handler task
        is_handler: bool,
    },

    /// Emitted when a task fails.
    TaskFailed {
        /// Information about the task that failed
        task: TaskInfo,
        /// Result information from the failed task
        result: ResultInfo,
        /// Whether errors were ignored for this task
        ignore_errors: bool,
        /// Whether this is a handler task
        is_handler: bool,
    },

    /// Emitted when a task is skipped (condition not met).
    TaskSkipped {
        /// Information about the task that was skipped
        task: TaskInfo,
        /// Result information (contains skip reason)
        result: ResultInfo,
        /// Whether this is a handler task
        is_handler: bool,
    },

    /// Emitted when a host is unreachable for a task.
    TaskUnreachable {
        /// Information about the task that could not execute
        task: TaskInfo,
        /// Result information (contains unreachable reason)
        result: ResultInfo,
        /// Whether this is a handler task
        is_handler: bool,
    },

    // -------------------------------------------------------------------------
    // Handler Events
    // -------------------------------------------------------------------------
    /// Emitted when a handler is triggered by a task notification.
    HandlerTriggered {
        /// Name of the handler that was triggered
        handler_name: String,
        /// Name of the task that triggered the handler
        notifying_task: String,
        /// Host on which the handler was triggered
        host: String,
    },

    /// Emitted when a handler starts execution.
    HandlerStart {
        /// Information about the handler task
        task: TaskInfo,
        /// Name of the handler
        handler_name: String,
    },

    /// Emitted when a handler completes successfully.
    HandlerOk {
        /// Information about the handler task
        task: TaskInfo,
        /// Result information from the handler
        result: ResultInfo,
        /// Name of the handler
        handler_name: String,
    },

    /// Emitted when a handler fails.
    HandlerFailed {
        /// Information about the handler task
        task: TaskInfo,
        /// Result information from the failed handler
        result: ResultInfo,
        /// Name of the handler
        handler_name: String,
    },

    // -------------------------------------------------------------------------
    // Runner Events (Loop and Retry)
    // -------------------------------------------------------------------------
    /// Emitted when a task is being retried.
    RunnerRetry {
        /// Information about the task being retried
        task: TaskInfo,
        /// Current retry attempt (1-based)
        attempt: usize,
        /// Maximum retry attempts
        max_attempts: usize,
        /// Result from the failed attempt
        result: ResultInfo,
        /// Delay before next retry
        #[serde(with = "duration_serde")]
        delay: Duration,
    },

    /// Emitted when a loop item completes successfully.
    RunnerItemOk {
        /// Information about the task
        task: TaskInfo,
        /// Result information for this item
        result: ResultInfo,
        /// The loop item that was processed
        item: JsonValue,
        /// Index of this item in the loop (0-based)
        item_index: usize,
        /// Total number of items in the loop
        total_items: usize,
    },

    /// Emitted when a loop item fails.
    RunnerItemFailed {
        /// Information about the task
        task: TaskInfo,
        /// Result information for this item
        result: ResultInfo,
        /// The loop item that failed
        item: JsonValue,
        /// Index of this item in the loop (0-based)
        item_index: usize,
        /// Total number of items in the loop
        total_items: usize,
    },

    // -------------------------------------------------------------------------
    // Statistics Events
    // -------------------------------------------------------------------------
    /// Emitted at the end of execution with final statistics.
    ///
    /// This provides a comprehensive summary of the entire playbook run.
    Stats {
        /// Final statistics per host
        stats: HashMap<String, ExecutionStats>,
        /// Total execution time
        #[serde(with = "duration_serde")]
        total_duration: Duration,
        /// Custom data that can be added by callbacks
        custom_data: HashMap<String, JsonValue>,
    },

    // -------------------------------------------------------------------------
    // Informational Events
    // -------------------------------------------------------------------------
    /// Emitted for debug messages during execution.
    Debug {
        /// The debug message
        msg: String,
        /// Host context (if applicable)
        host: Option<String>,
        /// Additional data
        data: Option<JsonValue>,
    },

    /// Emitted for warning messages during execution.
    Warning {
        /// The warning message
        msg: String,
        /// Host context (if applicable)
        host: Option<String>,
    },

    /// Emitted for deprecation warnings.
    Deprecated {
        /// The deprecation message
        msg: String,
        /// Version when the feature will be removed
        removal_version: Option<String>,
        /// Suggested alternative
        alternative: Option<String>,
    },
}

impl CallbackEvent {
    /// Returns the event type name as a string.
    pub fn event_type(&self) -> &'static str {
        match self {
            CallbackEvent::PlaybookStart { .. } => "playbook_start",
            CallbackEvent::PlaybookEnd { .. } => "playbook_end",
            CallbackEvent::PlayStart { .. } => "play_start",
            CallbackEvent::PlayEnd { .. } => "play_end",
            CallbackEvent::TaskStart { .. } => "task_start",
            CallbackEvent::TaskOk { .. } => "task_ok",
            CallbackEvent::TaskFailed { .. } => "task_failed",
            CallbackEvent::TaskSkipped { .. } => "task_skipped",
            CallbackEvent::TaskUnreachable { .. } => "task_unreachable",
            CallbackEvent::HandlerTriggered { .. } => "handler_triggered",
            CallbackEvent::HandlerStart { .. } => "handler_start",
            CallbackEvent::HandlerOk { .. } => "handler_ok",
            CallbackEvent::HandlerFailed { .. } => "handler_failed",
            CallbackEvent::RunnerRetry { .. } => "runner_retry",
            CallbackEvent::RunnerItemOk { .. } => "runner_item_ok",
            CallbackEvent::RunnerItemFailed { .. } => "runner_item_failed",
            CallbackEvent::Stats { .. } => "stats",
            CallbackEvent::Debug { .. } => "debug",
            CallbackEvent::Warning { .. } => "warning",
            CallbackEvent::Deprecated { .. } => "deprecated",
        }
    }

    /// Returns the host associated with this event, if any.
    pub fn host(&self) -> Option<&str> {
        match self {
            CallbackEvent::TaskStart { task, .. }
            | CallbackEvent::TaskOk { task, .. }
            | CallbackEvent::TaskFailed { task, .. }
            | CallbackEvent::TaskSkipped { task, .. }
            | CallbackEvent::TaskUnreachable { task, .. }
            | CallbackEvent::HandlerStart { task, .. }
            | CallbackEvent::HandlerOk { task, .. }
            | CallbackEvent::HandlerFailed { task, .. }
            | CallbackEvent::RunnerRetry { task, .. }
            | CallbackEvent::RunnerItemOk { task, .. }
            | CallbackEvent::RunnerItemFailed { task, .. } => Some(&task.host),
            CallbackEvent::HandlerTriggered { host, .. } => Some(host),
            CallbackEvent::Debug { host, .. } | CallbackEvent::Warning { host, .. } => {
                host.as_deref()
            }
            _ => None,
        }
    }

    /// Returns whether this is a failure event.
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            CallbackEvent::TaskFailed {
                ignore_errors: false,
                ..
            } | CallbackEvent::TaskUnreachable { .. }
                | CallbackEvent::HandlerFailed { .. }
                | CallbackEvent::RunnerItemFailed { .. }
        )
    }

    /// Returns whether this event is related to a handler.
    pub fn is_handler_event(&self) -> bool {
        matches!(
            self,
            CallbackEvent::HandlerTriggered { .. }
                | CallbackEvent::HandlerStart { .. }
                | CallbackEvent::HandlerOk { .. }
                | CallbackEvent::HandlerFailed { .. }
        ) || matches!(
            self,
            CallbackEvent::TaskStart {
                is_handler: true,
                ..
            } | CallbackEvent::TaskOk {
                is_handler: true,
                ..
            } | CallbackEvent::TaskFailed {
                is_handler: true,
                ..
            } | CallbackEvent::TaskSkipped {
                is_handler: true,
                ..
            } | CallbackEvent::TaskUnreachable {
                is_handler: true,
                ..
            }
        )
    }
}

// ============================================================================
// Context Structs
// ============================================================================

/// Information about a playbook being executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookInfo {
    /// Name of the playbook
    pub name: String,
    /// Path to the playbook file (if loaded from disk)
    pub file_path: Option<PathBuf>,
    /// Number of plays in the playbook
    pub play_count: usize,
    /// Playbook-level variables (sanitized, secrets removed)
    #[serde(default)]
    pub vars: IndexMap<String, JsonValue>,
    /// Variable files referenced by the playbook
    #[serde(default)]
    pub vars_files: Vec<String>,
    /// Check mode enabled
    pub check_mode: bool,
    /// Diff mode enabled
    pub diff_mode: bool,
    /// Verbosity level
    pub verbosity: u8,
    /// Extra variables provided via command line
    #[serde(default)]
    pub extra_vars: HashMap<String, JsonValue>,
}

impl PlaybookInfo {
    /// Create a new PlaybookInfo from a playbook and config.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            file_path: None,
            play_count: 0,
            vars: IndexMap::new(),
            vars_files: Vec::new(),
            check_mode: false,
            diff_mode: false,
            verbosity: 0,
            extra_vars: HashMap::new(),
        }
    }

    /// Set the file path.
    pub fn with_file_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Set the play count.
    pub fn with_play_count(mut self, count: usize) -> Self {
        self.play_count = count;
        self
    }

    /// Set check mode.
    pub fn with_check_mode(mut self, enabled: bool) -> Self {
        self.check_mode = enabled;
        self
    }

    /// Set diff mode.
    pub fn with_diff_mode(mut self, enabled: bool) -> Self {
        self.diff_mode = enabled;
        self
    }

    /// Set verbosity level.
    pub fn with_verbosity(mut self, level: u8) -> Self {
        self.verbosity = level;
        self
    }
}

/// Information about a play being executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayInfo {
    /// Name of the play
    pub name: String,
    /// Host pattern for this play
    pub hosts_pattern: String,
    /// Resolved hosts for this play
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Number of hosts targeted
    pub host_count: usize,
    /// Play-level variables (sanitized)
    #[serde(default)]
    pub vars: IndexMap<String, JsonValue>,
    /// Serial execution value (if set)
    pub serial: Option<usize>,
    /// Whether fact gathering is enabled
    pub gather_facts: bool,
    /// Whether become is enabled for this play
    #[serde(rename = "become")]
    pub become_enabled: bool,
    /// Become user (if specified)
    pub become_user: Option<String>,
    /// Connection type (if specified)
    pub connection: Option<String>,
    /// Strategy for this play
    pub strategy: Option<String>,
    /// Number of tasks in this play
    pub task_count: usize,
    /// Number of handlers in this play
    pub handler_count: usize,
    /// Tags applied to this play
    #[serde(default)]
    pub tags: Vec<String>,
}

impl PlayInfo {
    /// Create a new PlayInfo.
    pub fn new(name: impl Into<String>, hosts_pattern: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            hosts_pattern: hosts_pattern.into(),
            hosts: Vec::new(),
            host_count: 0,
            vars: IndexMap::new(),
            serial: None,
            gather_facts: true,
            become_enabled: false,
            become_user: None,
            connection: None,
            strategy: None,
            task_count: 0,
            handler_count: 0,
            tags: Vec::new(),
        }
    }

    /// Set the resolved hosts.
    pub fn with_hosts(mut self, hosts: Vec<String>) -> Self {
        self.host_count = hosts.len();
        self.hosts = hosts;
        self
    }

    /// Set the task count.
    pub fn with_task_count(mut self, count: usize) -> Self {
        self.task_count = count;
        self
    }

    /// Set the handler count.
    pub fn with_handler_count(mut self, count: usize) -> Self {
        self.handler_count = count;
        self
    }

    /// Set gather_facts.
    pub fn with_gather_facts(mut self, enabled: bool) -> Self {
        self.gather_facts = enabled;
        self
    }
}

/// Information about a task being executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    /// Task name
    pub name: String,
    /// Module being executed
    pub module: String,
    /// Module arguments (sanitized, secrets removed)
    #[serde(default)]
    pub args: IndexMap<String, JsonValue>,
    /// Host on which the task is executing
    pub host: String,
    /// Unique task identifier (for correlation)
    pub task_uuid: String,
    /// Task path in the playbook (e.g., "tasks[2]")
    pub task_path: Option<String>,
    /// Action plugin being used (may differ from module)
    pub action: String,
    /// Whether the task has a when condition
    pub is_conditional: bool,
    /// Whether the task has loop items
    pub is_loop: bool,
    /// Number of loop items (if is_loop is true)
    pub loop_count: Option<usize>,
    /// Tags applied to this task
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether ignore_errors is set
    pub ignore_errors: bool,
    /// Delegate host (if delegated)
    pub delegate_to: Option<String>,
    /// Whether run_once is set
    pub run_once: bool,
}

impl TaskInfo {
    /// Create a new TaskInfo.
    pub fn new(
        name: impl Into<String>,
        module: impl Into<String>,
        host: impl Into<String>,
    ) -> Self {
        let module_str = module.into();
        Self {
            name: name.into(),
            module: module_str.clone(),
            args: IndexMap::new(),
            host: host.into(),
            task_uuid: generate_uuid(),
            task_path: None,
            action: module_str,
            is_conditional: false,
            is_loop: false,
            loop_count: None,
            tags: Vec::new(),
            ignore_errors: false,
            delegate_to: None,
            run_once: false,
        }
    }

    /// Set the module arguments.
    pub fn with_args(mut self, args: IndexMap<String, JsonValue>) -> Self {
        self.args = args;
        self
    }

    /// Set the task UUID.
    pub fn with_uuid(mut self, uuid: impl Into<String>) -> Self {
        self.task_uuid = uuid.into();
        self
    }

    /// Set the task path.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.task_path = Some(path.into());
        self
    }

    /// Set whether this is a conditional task.
    pub fn with_conditional(mut self, is_conditional: bool) -> Self {
        self.is_conditional = is_conditional;
        self
    }

    /// Set loop information.
    pub fn with_loop(mut self, count: usize) -> Self {
        self.is_loop = true;
        self.loop_count = Some(count);
        self
    }

    /// Set ignore_errors.
    pub fn with_ignore_errors(mut self, ignore: bool) -> Self {
        self.ignore_errors = ignore;
        self
    }
}

/// Result information from task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultInfo {
    /// Task status
    pub status: TaskStatus,
    /// Whether something was changed
    pub changed: bool,
    /// Message from the task
    pub msg: Option<String>,
    /// Diff information (if diff_mode enabled)
    pub diff: Option<DiffInfo>,
    /// Return code (for command/shell tasks)
    pub rc: Option<i32>,
    /// Standard output (truncated for large outputs)
    pub stdout: Option<String>,
    /// Standard error (truncated for large outputs)
    pub stderr: Option<String>,
    /// Whether output was truncated
    pub output_truncated: bool,
    /// Module-specific result data
    #[serde(default)]
    pub data: IndexMap<String, JsonValue>,
    /// Execution duration for this task
    #[serde(with = "duration_serde")]
    pub duration: Duration,
    /// Start time of task execution
    #[serde(skip)]
    pub start_time: Option<Instant>,
    /// End time of task execution
    #[serde(skip)]
    pub end_time: Option<Instant>,
}

impl Default for ResultInfo {
    fn default() -> Self {
        Self {
            status: TaskStatus::Ok,
            changed: false,
            msg: None,
            diff: None,
            rc: None,
            stdout: None,
            stderr: None,
            output_truncated: false,
            data: IndexMap::new(),
            duration: Duration::ZERO,
            start_time: None,
            end_time: None,
        }
    }
}

impl ResultInfo {
    /// Create a new ResultInfo with Ok status.
    pub fn ok() -> Self {
        Self::default()
    }

    /// Create a new ResultInfo with Changed status.
    pub fn changed() -> Self {
        Self {
            status: TaskStatus::Changed,
            changed: true,
            ..Default::default()
        }
    }

    /// Create a new ResultInfo with Failed status.
    pub fn failed(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Failed,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    /// Create a new ResultInfo with Skipped status.
    pub fn skipped(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Skipped,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    /// Create a new ResultInfo with Unreachable status.
    pub fn unreachable(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Unreachable,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    /// Create from a TaskResult.
    pub fn from_task_result(result: &TaskResult, duration: Duration) -> Self {
        Self {
            status: result.status,
            changed: result.changed,
            msg: result.msg.clone(),
            diff: result.diff.as_ref().map(DiffInfo::from_task_diff),
            rc: None,
            stdout: None,
            stderr: None,
            output_truncated: false,
            data: result
                .result
                .as_ref()
                .and_then(|v| v.as_object())
                .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default(),
            duration,
            start_time: None,
            end_time: None,
        }
    }

    /// Set the message.
    pub fn with_msg(mut self, msg: impl Into<String>) -> Self {
        self.msg = Some(msg.into());
        self
    }

    /// Set the diff information.
    pub fn with_diff(mut self, diff: DiffInfo) -> Self {
        self.diff = Some(diff);
        self
    }

    /// Set command output.
    pub fn with_output(mut self, rc: i32, stdout: String, stderr: String) -> Self {
        self.rc = Some(rc);
        self.stdout = Some(truncate_output(&stdout, MAX_OUTPUT_LENGTH));
        self.stderr = Some(truncate_output(&stderr, MAX_OUTPUT_LENGTH));
        self.output_truncated =
            stdout.len() > MAX_OUTPUT_LENGTH || stderr.len() > MAX_OUTPUT_LENGTH;
        self
    }

    /// Set the duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

/// Maximum length for stdout/stderr in events (to prevent memory issues).
const MAX_OUTPUT_LENGTH: usize = 10_000;

/// Truncate output to a maximum length.
fn truncate_output(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}... (truncated, {} bytes total)", &s[..max_len], s.len())
    }
}

/// Diff information showing before/after state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffInfo {
    /// Content before the change
    pub before: Option<String>,
    /// Content after the change
    pub after: Option<String>,
    /// Header/label for the before content
    pub before_header: Option<String>,
    /// Header/label for the after content
    pub after_header: Option<String>,
    /// Prepared unified diff (if available)
    pub prepared: Option<String>,
}

impl DiffInfo {
    /// Create a new DiffInfo.
    pub fn new() -> Self {
        Self {
            before: None,
            after: None,
            before_header: None,
            after_header: None,
            prepared: None,
        }
    }

    /// Create from a TaskDiff.
    pub fn from_task_diff(diff: &TaskDiff) -> Self {
        Self {
            before: diff.before.clone(),
            after: diff.after.clone(),
            before_header: diff.before_header.clone(),
            after_header: diff.after_header.clone(),
            prepared: None,
        }
    }

    /// Set before content.
    pub fn with_before(mut self, content: impl Into<String>) -> Self {
        self.before = Some(content.into());
        self
    }

    /// Set after content.
    pub fn with_after(mut self, content: impl Into<String>) -> Self {
        self.after = Some(content.into());
        self
    }
}

impl Default for DiffInfo {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Timing Context
// ============================================================================

/// Timing information for performance tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingInfo {
    /// Start timestamp (milliseconds since epoch)
    pub start_ms: u64,
    /// End timestamp (milliseconds since epoch)
    pub end_ms: Option<u64>,
    /// Duration in milliseconds
    pub duration_ms: Option<u64>,
}

impl TimingInfo {
    /// Create a new TimingInfo starting now.
    pub fn start_now() -> Self {
        Self {
            start_ms: current_time_ms(),
            end_ms: None,
            duration_ms: None,
        }
    }

    /// Mark the timing as complete.
    pub fn complete(&mut self) {
        let now = current_time_ms();
        self.end_ms = Some(now);
        self.duration_ms = Some(now.saturating_sub(self.start_ms));
    }
}

/// Get current time in milliseconds since Unix epoch.
fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Generate a UUID v4 string.
fn generate_uuid() -> String {
    // Simple UUID generation without external dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let rand_part: u64 = now.as_nanos() as u64 ^ (now.as_secs() << 32);
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (rand_part >> 32) as u32,
        ((rand_part >> 16) & 0xFFFF) as u16,
        (rand_part & 0x0FFF) as u16,
        (0x8000 | (rand_part & 0x3FFF)) as u16,
        rand_part & 0xFFFF_FFFF_FFFF
    )
}

// ============================================================================
// Event Metadata
// ============================================================================

/// Metadata attached to every callback event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    /// Unique event identifier
    pub event_id: String,
    /// Timestamp when the event was created
    pub timestamp: u64,
    /// Correlation ID for tracking related events
    pub correlation_id: Option<String>,
    /// Playbook run UUID
    pub playbook_uuid: String,
    /// Sequence number within the playbook run
    pub sequence: u64,
}

impl EventMetadata {
    /// Create new metadata for an event.
    pub fn new(playbook_uuid: impl Into<String>, sequence: u64) -> Self {
        Self {
            event_id: generate_uuid(),
            timestamp: current_time_ms(),
            correlation_id: None,
            playbook_uuid: playbook_uuid.into(),
            sequence,
        }
    }

    /// Set the correlation ID.
    pub fn with_correlation(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }
}

/// An event with its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackEventWithMeta {
    /// The event itself
    pub event: CallbackEvent,
    /// Event metadata
    pub metadata: EventMetadata,
}

impl CallbackEventWithMeta {
    /// Create a new event with metadata.
    pub fn new(event: CallbackEvent, metadata: EventMetadata) -> Self {
        Self { event, metadata }
    }
}

// ============================================================================
// Serde Helpers
// ============================================================================

/// Serde module for Duration serialization.
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    #[derive(Serialize, Deserialize)]
    struct DurationHelper {
        secs: u64,
        nanos: u32,
    }

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let helper = DurationHelper {
            secs: duration.as_secs(),
            nanos: duration.subsec_nanos(),
        };
        helper.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let helper = DurationHelper::deserialize(deserializer)?;
        Ok(Duration::new(helper.secs, helper.nanos))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_callback_event_type() {
        let event = CallbackEvent::PlaybookStart {
            playbook: PlaybookInfo::new("test"),
            started_at: None,
        };
        assert_eq!(event.event_type(), "playbook_start");
    }

    #[test]
    fn test_callback_event_host() {
        let task = TaskInfo::new("Test task", "debug", "server1");
        let event = CallbackEvent::TaskOk {
            task,
            result: ResultInfo::ok(),
            is_handler: false,
        };
        assert_eq!(event.host(), Some("server1"));
    }

    #[test]
    fn test_callback_event_is_failure() {
        let task = TaskInfo::new("Test task", "debug", "server1");
        let event = CallbackEvent::TaskFailed {
            task: task.clone(),
            result: ResultInfo::failed("test error"),
            ignore_errors: false,
            is_handler: false,
        };
        assert!(event.is_failure());

        let event_ignored = CallbackEvent::TaskFailed {
            task,
            result: ResultInfo::failed("test error"),
            ignore_errors: true,
            is_handler: false,
        };
        assert!(!event_ignored.is_failure());
    }

    #[test]
    fn test_playbook_info_builder() {
        let info = PlaybookInfo::new("my-playbook")
            .with_file_path("/path/to/playbook.yml")
            .with_play_count(3)
            .with_check_mode(true);

        assert_eq!(info.name, "my-playbook");
        assert_eq!(info.file_path, Some(PathBuf::from("/path/to/playbook.yml")));
        assert_eq!(info.play_count, 3);
        assert!(info.check_mode);
    }

    #[test]
    fn test_task_info_builder() {
        let info = TaskInfo::new("Install nginx", "package", "webserver1")
            .with_conditional(true)
            .with_loop(5)
            .with_ignore_errors(true);

        assert_eq!(info.name, "Install nginx");
        assert_eq!(info.module, "package");
        assert_eq!(info.host, "webserver1");
        assert!(info.is_conditional);
        assert!(info.is_loop);
        assert_eq!(info.loop_count, Some(5));
        assert!(info.ignore_errors);
    }

    #[test]
    fn test_result_info_from_task_result() {
        let task_result = TaskResult {
            status: TaskStatus::Changed,
            changed: true,
            msg: Some("Package installed".to_string()),
            result: None,
            diff: None,
        };

        let result_info = ResultInfo::from_task_result(&task_result, Duration::from_millis(1500));

        assert_eq!(result_info.status, TaskStatus::Changed);
        assert!(result_info.changed);
        assert_eq!(result_info.msg, Some("Package installed".to_string()));
        assert_eq!(result_info.duration, Duration::from_millis(1500));
    }

    #[test]
    fn test_truncate_output() {
        let short = "short string";
        assert_eq!(truncate_output(short, 100), short);

        let long = "x".repeat(200);
        let truncated = truncate_output(&long, 100);
        assert!(truncated.contains("truncated"));
        assert!(truncated.contains("200 bytes"));
    }

    #[test]
    fn test_event_serialization() {
        let event = CallbackEvent::TaskOk {
            task: TaskInfo::new("Test", "debug", "localhost"),
            result: ResultInfo::ok().with_msg("Success"),
            is_handler: false,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("task_ok"));

        let deserialized: CallbackEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.event_type(), "task_ok");
    }

    #[test]
    fn test_timing_info() {
        let mut timing = TimingInfo::start_now();
        assert!(timing.start_ms > 0);
        assert!(timing.end_ms.is_none());

        timing.complete();
        assert!(timing.end_ms.is_some());
        assert!(timing.duration_ms.is_some());
    }

    #[test]
    fn test_event_metadata() {
        let meta = EventMetadata::new("playbook-123", 1);
        assert!(!meta.event_id.is_empty());
        assert_eq!(meta.playbook_uuid, "playbook-123");
        assert_eq!(meta.sequence, 1);
    }

    #[test]
    fn test_generate_uuid() {
        let uuid1 = generate_uuid();
        let uuid2 = generate_uuid();
        // UUIDs should be non-empty and different (with high probability)
        assert!(!uuid1.is_empty());
        assert!(!uuid2.is_empty());
        // Format check: xxxxxxxx-xxxx-4xxx-xxxx-xxxxxxxxxxxx
        assert!(uuid1.contains("-4"));
    }

    #[test]
    fn test_diff_info() {
        let diff = DiffInfo::new()
            .with_before("old content")
            .with_after("new content");

        assert_eq!(diff.before, Some("old content".to_string()));
        assert_eq!(diff.after, Some("new content".to_string()));
    }

    #[test]
    fn test_result_info_with_output() {
        let result = ResultInfo::ok().with_output(0, "stdout content".to_string(), "".to_string());

        assert_eq!(result.rc, Some(0));
        assert_eq!(result.stdout, Some("stdout content".to_string()));
        assert!(!result.output_truncated);
    }

    #[test]
    fn test_play_info_builder() {
        let info = PlayInfo::new("Configure web servers", "webservers")
            .with_hosts(vec!["web1".to_string(), "web2".to_string()])
            .with_task_count(5)
            .with_handler_count(2)
            .with_gather_facts(false);

        assert_eq!(info.name, "Configure web servers");
        assert_eq!(info.hosts_pattern, "webservers");
        assert_eq!(info.host_count, 2);
        assert_eq!(info.task_count, 5);
        assert_eq!(info.handler_count, 2);
        assert!(!info.gather_facts);
    }

    #[test]
    fn test_callback_event_with_meta() {
        let event = CallbackEvent::Debug {
            msg: "Test message".to_string(),
            host: None,
            data: None,
        };
        let meta = EventMetadata::new("run-123", 42);
        let event_with_meta = CallbackEventWithMeta::new(event, meta);

        assert_eq!(event_with_meta.metadata.sequence, 42);
        assert_eq!(event_with_meta.event.event_type(), "debug");
    }
}
