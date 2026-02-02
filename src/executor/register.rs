//! Enhanced result registration system for Rustible
//!
//! This module provides advanced registration capabilities including:
//! - Type-safe result accessors with strongly-typed getters
//! - Structured loop result handling with LoopResults type
//! - Failed task registration with detailed error context
//! - Async result registration for deferred access
//! - Fluent builder pattern for result construction
//!
//! # Type-Safe Result Access
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::executor::runtime::RegisteredResult;
//! # use rustible::executor::RegisteredResultExt;
//! # let result = RegisteredResult::default();
//! // Instead of raw JSON access:
//! let rc = result.data.get("rc").and_then(|v| v.as_i64());
//!
//! // Use type-safe accessors:
//! let rc = result.rc();
//! let stdout = result.stdout_str();
//! let lines = result.stdout_lines_slice();
//! # Ok(())
//! # }
//! ```
//!
//! # Loop Results
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::executor::runtime::RegisteredResult;
//! # use rustible::executor::register::LoopResultsExt;
//! # let result = RegisteredResult::default();
//! // Access loop results with type safety:
//! let loop_results = result.loop_results();
//! for item in loop_results.iter() {
//!     if item.is_changed() {
//!         println!("Item changed: {:?}", item.item());
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Failed Task Info
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! # use rustible::executor::runtime::RegisteredResult;
//! # use rustible::executor::FailedTaskInfo;
//! # let mut result = RegisteredResult::default();
//! # result.failed = true;
//! # result.msg = Some("Connection lost".to_string());
//! // Get detailed failure information:
//! if let Some(failure) = FailedTaskInfo::from_result(&result, "host1", "Run task", "command") {
//!     println!("Task failed at: {:?}", failure.timestamp);
//!     println!("Error: {}", failure.error_message);
//!     if let Some(rc) = failure.exit_code {
//!         println!("Exit code: {}", rc);
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::oneshot;

use super::runtime::RegisteredResult;

// ============================================================================
// Type-Safe Result Accessors
// ============================================================================

/// Extension trait providing type-safe accessors for RegisteredResult
pub trait RegisteredResultExt {
    // Status accessors
    /// Returns true if the task made changes
    fn is_changed(&self) -> bool;
    /// Returns true if the task failed
    fn is_failed(&self) -> bool;
    /// Returns true if the task was skipped
    fn is_skipped(&self) -> bool;
    /// Returns true if the task succeeded (not failed and not skipped)
    fn is_ok(&self) -> bool;

    // Command result accessors
    /// Returns the return code as i32, if available
    fn rc(&self) -> Option<i32>;
    /// Returns true if the return code is zero
    fn rc_is_zero(&self) -> bool;
    /// Returns the stdout as a string slice, empty string if not set
    fn stdout_str(&self) -> &str;
    /// Returns the stderr as a string slice, empty string if not set
    fn stderr_str(&self) -> &str;
    /// Returns stdout lines as a slice
    fn stdout_lines_slice(&self) -> &[String];
    /// Returns stderr lines as a slice
    fn stderr_lines_slice(&self) -> &[String];

    // Message accessors
    /// Returns the message as a string slice, empty string if not set
    fn msg_str(&self) -> &str;

    // Data accessors with type coercion
    /// Get a string value from the data map
    fn get_string(&self, key: &str) -> Option<&str>;
    /// Get an integer value from the data map
    fn get_i64(&self, key: &str) -> Option<i64>;
    /// Get a boolean value from the data map
    fn get_bool(&self, key: &str) -> Option<bool>;
    /// Get an array value from the data map
    fn get_array(&self, key: &str) -> Option<&Vec<JsonValue>>;
    /// Get an object value from the data map
    fn get_object(&self, key: &str) -> Option<&serde_json::Map<String, JsonValue>>;

    // Nested data access
    /// Get a nested value using dot notation (e.g., "stat.exists")
    fn get_nested(&self, path: &str) -> Option<&JsonValue>;
    /// Get a nested string value using dot notation
    fn get_nested_string(&self, path: &str) -> Option<&str>;
    /// Get a nested boolean value using dot notation
    fn get_nested_bool(&self, path: &str) -> Option<bool>;
    /// Get a nested integer value using dot notation
    fn get_nested_i64(&self, path: &str) -> Option<i64>;

    // Loop result accessors
    /// Returns true if this result contains loop results
    fn has_loop_results(&self) -> bool;
    /// Returns the number of loop iterations
    fn loop_count(&self) -> usize;
    /// Returns true if any loop iteration changed something
    fn any_loop_changed(&self) -> bool;
    /// Returns true if all loop iterations succeeded
    fn all_loop_ok(&self) -> bool;
    /// Get a specific loop result by index
    fn loop_result_at(&self, index: usize) -> Option<&RegisteredResult>;

    // Safe default access
    /// Get a value with a default fallback
    fn get_or_default<'a>(&'a self, key: &str, default: &'a JsonValue) -> &'a JsonValue;
}

impl RegisteredResultExt for RegisteredResult {
    fn is_changed(&self) -> bool {
        self.changed
    }

    fn is_failed(&self) -> bool {
        self.failed
    }

    fn is_skipped(&self) -> bool {
        self.skipped
    }

    fn is_ok(&self) -> bool {
        !self.failed && !self.skipped
    }

    fn rc(&self) -> Option<i32> {
        self.rc
    }

    fn rc_is_zero(&self) -> bool {
        self.rc == Some(0)
    }

    fn stdout_str(&self) -> &str {
        self.stdout.as_deref().unwrap_or("")
    }

    fn stderr_str(&self) -> &str {
        self.stderr.as_deref().unwrap_or("")
    }

    fn stdout_lines_slice(&self) -> &[String] {
        self.stdout_lines.as_deref().unwrap_or(&[])
    }

    fn stderr_lines_slice(&self) -> &[String] {
        self.stderr_lines.as_deref().unwrap_or(&[])
    }

    fn msg_str(&self) -> &str {
        self.msg.as_deref().unwrap_or("")
    }

    fn get_string(&self, key: &str) -> Option<&str> {
        self.data.get(key).and_then(|v| v.as_str())
    }

    fn get_i64(&self, key: &str) -> Option<i64> {
        self.data.get(key).and_then(|v| v.as_i64())
    }

    fn get_bool(&self, key: &str) -> Option<bool> {
        self.data.get(key).and_then(|v| v.as_bool())
    }

    fn get_array(&self, key: &str) -> Option<&Vec<JsonValue>> {
        self.data.get(key).and_then(|v| v.as_array())
    }

    fn get_object(&self, key: &str) -> Option<&serde_json::Map<String, JsonValue>> {
        self.data.get(key).and_then(|v| v.as_object())
    }

    fn get_nested(&self, path: &str) -> Option<&JsonValue> {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return None;
        }

        let mut current = self.data.get(parts[0])?;
        for part in &parts[1..] {
            current = current.get(part)?;
        }
        Some(current)
    }

    fn get_nested_string(&self, path: &str) -> Option<&str> {
        self.get_nested(path).and_then(|v| v.as_str())
    }

    fn get_nested_bool(&self, path: &str) -> Option<bool> {
        self.get_nested(path).and_then(|v| v.as_bool())
    }

    fn get_nested_i64(&self, path: &str) -> Option<i64> {
        self.get_nested(path).and_then(|v| v.as_i64())
    }

    fn has_loop_results(&self) -> bool {
        self.results.is_some()
    }

    fn loop_count(&self) -> usize {
        self.results.as_ref().map(|r| r.len()).unwrap_or(0)
    }

    fn any_loop_changed(&self) -> bool {
        self.results
            .as_ref()
            .map(|results| results.iter().any(|r| r.changed))
            .unwrap_or(false)
    }

    fn all_loop_ok(&self) -> bool {
        self.results
            .as_ref()
            .map(|results| results.iter().all(|r| !r.failed))
            .unwrap_or(true)
    }

    fn loop_result_at(&self, index: usize) -> Option<&RegisteredResult> {
        self.results.as_ref().and_then(|r| r.get(index))
    }

    fn get_or_default<'a>(&'a self, key: &str, default: &'a JsonValue) -> &'a JsonValue {
        self.data.get(key).unwrap_or(default)
    }
}

// ============================================================================
// Loop Results Wrapper
// ============================================================================

/// Wrapper type for loop results providing convenient iteration and access
#[derive(Debug, Clone)]
pub struct LoopResults<'a> {
    results: Option<&'a Vec<RegisteredResult>>,
}

impl<'a> LoopResults<'a> {
    /// Create a new LoopResults wrapper
    pub fn new(results: Option<&'a Vec<RegisteredResult>>) -> Self {
        Self { results }
    }

    /// Returns true if there are any loop results
    pub fn has_results(&self) -> bool {
        self.results.is_some()
    }

    /// Returns the number of loop iterations
    pub fn len(&self) -> usize {
        self.results.map(|r| r.len()).unwrap_or(0)
    }

    /// Returns true if there are no loop results
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate over loop results
    pub fn iter(&self) -> impl Iterator<Item = LoopResultItem<'a>> + 'a {
        let results = self.results;
        (0..self.len()).map(move |i| LoopResultItem {
            index: i,
            result: results.and_then(|r| r.get(i)),
        })
    }

    /// Get a specific loop result by index
    pub fn get(&self, index: usize) -> Option<LoopResultItem<'a>> {
        if index < self.len() {
            Some(LoopResultItem {
                index,
                result: self.results.and_then(|r| r.get(index)),
            })
        } else {
            None
        }
    }

    /// Returns true if any iteration changed something
    pub fn any_changed(&self) -> bool {
        self.results
            .map(|r| r.iter().any(|item| item.changed))
            .unwrap_or(false)
    }

    /// Returns true if all iterations succeeded
    pub fn all_ok(&self) -> bool {
        self.results
            .map(|r| r.iter().all(|item| !item.failed))
            .unwrap_or(true)
    }

    /// Returns true if any iteration failed
    pub fn any_failed(&self) -> bool {
        self.results
            .map(|r| r.iter().any(|item| item.failed))
            .unwrap_or(false)
    }

    /// Get all changed items
    pub fn changed_items(&self) -> impl Iterator<Item = LoopResultItem<'a>> + 'a {
        self.iter().filter(|item| item.is_changed())
    }

    /// Get all failed items
    pub fn failed_items(&self) -> impl Iterator<Item = LoopResultItem<'a>> + 'a {
        self.iter().filter(|item| item.is_failed())
    }

    /// Count of changed iterations
    pub fn changed_count(&self) -> usize {
        self.results
            .map(|r| r.iter().filter(|item| item.changed).count())
            .unwrap_or(0)
    }

    /// Count of failed iterations
    pub fn failed_count(&self) -> usize {
        self.results
            .map(|r| r.iter().filter(|item| item.failed).count())
            .unwrap_or(0)
    }
}

/// A single item in a loop result with index information
#[derive(Debug, Clone)]
pub struct LoopResultItem<'a> {
    /// The 0-based index of this item in the loop
    pub index: usize,
    /// The result for this iteration
    result: Option<&'a RegisteredResult>,
}

impl<'a> LoopResultItem<'a> {
    /// Get the underlying result
    pub fn result(&self) -> Option<&'a RegisteredResult> {
        self.result
    }

    /// Returns true if this iteration changed something
    pub fn is_changed(&self) -> bool {
        self.result.map(|r| r.changed).unwrap_or(false)
    }

    /// Returns true if this iteration failed
    pub fn is_failed(&self) -> bool {
        self.result.map(|r| r.failed).unwrap_or(false)
    }

    /// Returns true if this iteration was skipped
    pub fn is_skipped(&self) -> bool {
        self.result.map(|r| r.skipped).unwrap_or(false)
    }

    /// Returns true if this iteration succeeded
    pub fn is_ok(&self) -> bool {
        self.result
            .map(|r| !r.failed && !r.skipped)
            .unwrap_or(false)
    }

    /// Get the item value for this iteration (from ansible_loop.allitems[index])
    pub fn item(&self) -> Option<&JsonValue> {
        self.result.and_then(|r| r.data.get("item"))
    }

    /// Get the return code for this iteration
    pub fn rc(&self) -> Option<i32> {
        self.result.and_then(|r| r.rc)
    }

    /// Get the stdout for this iteration
    pub fn stdout(&self) -> Option<&str> {
        self.result.and_then(|r| r.stdout.as_deref())
    }

    /// Get the stderr for this iteration
    pub fn stderr(&self) -> Option<&str> {
        self.result.and_then(|r| r.stderr.as_deref())
    }

    /// Get the message for this iteration
    pub fn msg(&self) -> Option<&str> {
        self.result.and_then(|r| r.msg.as_deref())
    }
}

/// Extension trait for accessing loop results
pub trait LoopResultsExt {
    /// Get a LoopResults wrapper for convenient access
    fn loop_results(&self) -> LoopResults<'_>;
}

impl LoopResultsExt for RegisteredResult {
    fn loop_results(&self) -> LoopResults<'_> {
        LoopResults::new(self.results.as_ref())
    }
}

// ============================================================================
// Failed Task Information
// ============================================================================

/// Detailed information about a failed task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedTaskInfo {
    /// The host where the task failed
    pub host: String,
    /// The task name
    pub task_name: String,
    /// The module that failed
    pub module: String,
    /// The error message
    pub error_message: String,
    /// The exit code if available
    pub exit_code: Option<i32>,
    /// The stderr output if available
    pub stderr: Option<String>,
    /// The stdout output if available
    pub stdout: Option<String>,
    /// Timestamp when the failure occurred
    pub timestamp: SystemTime,
    /// Duration the task ran before failing
    pub duration: Option<Duration>,
    /// Whether ignore_errors was set
    pub ignored: bool,
    /// Exception type if this was a module exception
    pub exception_type: Option<String>,
    /// Stack trace if available
    pub stack_trace: Option<String>,
    /// Additional context data
    pub context: IndexMap<String, JsonValue>,
}

impl FailedTaskInfo {
    /// Create a new FailedTaskInfo from a RegisteredResult
    pub fn from_result(
        result: &RegisteredResult,
        host: &str,
        task_name: &str,
        module: &str,
    ) -> Option<Self> {
        if !result.failed {
            return None;
        }

        Some(Self {
            host: host.to_string(),
            task_name: task_name.to_string(),
            module: module.to_string(),
            error_message: result
                .msg
                .clone()
                .unwrap_or_else(|| "Unknown error".to_string()),
            exit_code: result.rc,
            stderr: result.stderr.clone(),
            stdout: result.stdout.clone(),
            timestamp: SystemTime::now(),
            duration: None,
            ignored: false,
            exception_type: result
                .data
                .get("exception_type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            stack_trace: result
                .data
                .get("traceback")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            context: result.data.clone(),
        })
    }

    /// Create a builder for FailedTaskInfo
    pub fn builder(host: &str, task_name: &str, module: &str) -> FailedTaskInfoBuilder {
        FailedTaskInfoBuilder::new(host, task_name, module)
    }

    /// Check if this failure was a command execution failure
    pub fn is_command_failure(&self) -> bool {
        self.exit_code.is_some() && self.exit_code != Some(0)
    }

    /// Check if this failure was a module exception
    pub fn is_exception(&self) -> bool {
        self.exception_type.is_some()
    }

    /// Get a human-readable summary of the failure
    pub fn summary(&self) -> String {
        let mut summary = format!(
            "Task '{}' failed on host '{}' (module: {})",
            self.task_name, self.host, self.module
        );

        if let Some(rc) = self.exit_code {
            summary.push_str(&format!(", exit code: {}", rc));
        }

        if let Some(ref exc_type) = self.exception_type {
            summary.push_str(&format!(", exception: {}", exc_type));
        }

        summary.push_str(&format!(": {}", self.error_message));
        summary
    }
}

/// Builder for FailedTaskInfo
#[derive(Debug, Clone)]
pub struct FailedTaskInfoBuilder {
    info: FailedTaskInfo,
}

impl FailedTaskInfoBuilder {
    /// Create a new builder
    pub fn new(host: &str, task_name: &str, module: &str) -> Self {
        Self {
            info: FailedTaskInfo {
                host: host.to_string(),
                task_name: task_name.to_string(),
                module: module.to_string(),
                error_message: String::new(),
                exit_code: None,
                stderr: None,
                stdout: None,
                timestamp: SystemTime::now(),
                duration: None,
                ignored: false,
                exception_type: None,
                stack_trace: None,
                context: IndexMap::new(),
            },
        }
    }

    /// Set the error message
    pub fn error_message(mut self, msg: impl Into<String>) -> Self {
        self.info.error_message = msg.into();
        self
    }

    /// Set the exit code
    pub fn exit_code(mut self, code: i32) -> Self {
        self.info.exit_code = Some(code);
        self
    }

    /// Set the stderr
    pub fn stderr(mut self, stderr: impl Into<String>) -> Self {
        self.info.stderr = Some(stderr.into());
        self
    }

    /// Set the stdout
    pub fn stdout(mut self, stdout: impl Into<String>) -> Self {
        self.info.stdout = Some(stdout.into());
        self
    }

    /// Set the duration
    pub fn duration(mut self, duration: Duration) -> Self {
        self.info.duration = Some(duration);
        self
    }

    /// Set whether the error was ignored
    pub fn ignored(mut self, ignored: bool) -> Self {
        self.info.ignored = ignored;
        self
    }

    /// Set the exception type
    pub fn exception_type(mut self, exc_type: impl Into<String>) -> Self {
        self.info.exception_type = Some(exc_type.into());
        self
    }

    /// Set the stack trace
    pub fn stack_trace(mut self, trace: impl Into<String>) -> Self {
        self.info.stack_trace = Some(trace.into());
        self
    }

    /// Add context data
    pub fn context(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.info.context.insert(key.into(), value);
        self
    }

    /// Build the FailedTaskInfo
    pub fn build(self) -> FailedTaskInfo {
        self.info
    }
}

// ============================================================================
// Async Result Registration
// ============================================================================

/// Handle for accessing an asynchronously registered result
#[derive(Debug, Default)]
pub struct AsyncResultHandle {
    receiver: Option<oneshot::Receiver<RegisteredResult>>,
    result: Option<RegisteredResult>,
}

impl AsyncResultHandle {
    /// Create a new async result handle pair (sender, handle)
    pub fn new() -> (oneshot::Sender<RegisteredResult>, Self) {
        let (sender, receiver) = oneshot::channel();
        (
            sender,
            Self {
                receiver: Some(receiver),
                result: None,
            },
        )
    }

    /// Wait for the result to be available
    pub async fn wait(&mut self) -> Option<&RegisteredResult> {
        if self.result.is_some() {
            return self.result.as_ref();
        }

        if let Some(receiver) = self.receiver.take() {
            if let Ok(result) = receiver.await {
                self.result = Some(result);
            }
        }

        self.result.as_ref()
    }

    /// Try to get the result without waiting (returns None if not ready)
    pub fn try_get(&mut self) -> Option<&RegisteredResult> {
        if self.result.is_some() {
            return self.result.as_ref();
        }

        if let Some(ref mut receiver) = self.receiver {
            if let Ok(result) = receiver.try_recv() {
                self.result = Some(result);
                self.receiver = None;
            }
        }

        self.result.as_ref()
    }

    /// Check if the result is ready
    pub fn is_ready(&self) -> bool {
        self.result.is_some()
    }
}

/// Registry for async results
#[derive(Debug, Default)]
pub struct AsyncResultRegistry {
    pending: HashMap<String, oneshot::Sender<RegisteredResult>>,
    completed: HashMap<String, RegisteredResult>,
}

impl AsyncResultRegistry {
    /// Create a new async result registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a pending async result
    pub fn register_pending(&mut self, key: &str) -> AsyncResultHandle {
        let (sender, handle) = AsyncResultHandle::new();
        self.pending.insert(key.to_string(), sender);
        handle
    }

    /// Complete a pending result
    pub fn complete(&mut self, key: &str, result: RegisteredResult) -> bool {
        if let Some(sender) = self.pending.remove(key) {
            // Store in completed first
            self.completed.insert(key.to_string(), result.clone());
            // Try to send (may fail if receiver dropped)
            let _ = sender.send(result);
            true
        } else {
            // Not pending, just store as completed
            self.completed.insert(key.to_string(), result);
            false
        }
    }

    /// Get a completed result
    pub fn get_completed(&self, key: &str) -> Option<&RegisteredResult> {
        self.completed.get(key)
    }

    /// Check if a result is pending
    pub fn is_pending(&self, key: &str) -> bool {
        self.pending.contains_key(key)
    }

    /// Get count of pending results
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get count of completed results
    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }
}

// ============================================================================
// Result Builder
// ============================================================================

/// Builder for creating RegisteredResult with a fluent API
#[derive(Debug, Clone, Default)]
pub struct RegisteredResultBuilder {
    result: RegisteredResult,
}

impl RegisteredResultBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a builder starting with an ok result
    pub fn ok() -> Self {
        Self {
            result: RegisteredResult::ok(false),
        }
    }

    /// Create a builder starting with a changed result
    pub fn changed() -> Self {
        Self {
            result: RegisteredResult::ok(true),
        }
    }

    /// Create a builder starting with a failed result
    pub fn failed(msg: impl Into<String>) -> Self {
        Self {
            result: RegisteredResult::failed(msg),
        }
    }

    /// Create a builder starting with a skipped result
    pub fn skipped(msg: impl Into<String>) -> Self {
        Self {
            result: RegisteredResult::skipped(msg),
        }
    }

    /// Set the changed flag
    pub fn set_changed(mut self, changed: bool) -> Self {
        self.result.changed = changed;
        self
    }

    /// Set the failed flag
    pub fn set_failed(mut self, failed: bool) -> Self {
        self.result.failed = failed;
        self
    }

    /// Set the skipped flag
    pub fn set_skipped(mut self, skipped: bool) -> Self {
        self.result.skipped = skipped;
        self
    }

    /// Set the return code
    pub fn rc(mut self, rc: i32) -> Self {
        self.result.rc = Some(rc);
        self
    }

    /// Set stdout
    pub fn stdout(mut self, stdout: impl Into<String>) -> Self {
        let stdout_str: String = stdout.into();
        self.result.stdout_lines = Some(stdout_str.lines().map(String::from).collect());
        self.result.stdout = Some(stdout_str);
        self
    }

    /// Set stderr
    pub fn stderr(mut self, stderr: impl Into<String>) -> Self {
        let stderr_str: String = stderr.into();
        self.result.stderr_lines = Some(stderr_str.lines().map(String::from).collect());
        self.result.stderr = Some(stderr_str);
        self
    }

    /// Set the message
    pub fn msg(mut self, msg: impl Into<String>) -> Self {
        self.result.msg = Some(msg.into());
        self
    }

    /// Add a data field
    pub fn data(mut self, key: impl Into<String>, value: impl Into<JsonValue>) -> Self {
        self.result.data.insert(key.into(), value.into());
        self
    }

    /// Add multiple data fields
    pub fn data_from(mut self, data: IndexMap<String, JsonValue>) -> Self {
        for (k, v) in data {
            self.result.data.insert(k, v);
        }
        self
    }

    /// Set loop results
    pub fn results(mut self, results: Vec<RegisteredResult>) -> Self {
        self.result.results = Some(results);
        self
    }

    /// Add a single loop result
    pub fn add_result(mut self, result: RegisteredResult) -> Self {
        if self.result.results.is_none() {
            self.result.results = Some(Vec::new());
        }
        if let Some(ref mut results) = self.result.results {
            results.push(result);
        }
        self
    }

    /// Set command execution metadata
    pub fn command_result(
        mut self,
        rc: i32,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
    ) -> Self {
        let stdout_str: String = stdout.into();
        let stderr_str: String = stderr.into();

        self.result.rc = Some(rc);
        self.result.stdout_lines = Some(stdout_str.lines().map(String::from).collect());
        self.result.stdout = Some(stdout_str);
        self.result.stderr_lines = Some(stderr_str.lines().map(String::from).collect());
        self.result.stderr = Some(stderr_str);
        self.result.changed = true;
        self
    }

    /// Set stat result data
    pub fn stat_result(mut self, exists: bool, is_dir: bool, mode: &str) -> Self {
        self.result.data.insert(
            "stat".to_string(),
            serde_json::json!({
                "exists": exists,
                "isdir": is_dir,
                "isreg": !is_dir && exists,
                "mode": mode
            }),
        );
        self
    }

    /// Build the result
    pub fn build(self) -> RegisteredResult {
        self.result
    }
}

// ============================================================================
// Enhanced RuntimeContext Extension for Registration
// ============================================================================

/// Extension trait for enhanced result registration on RuntimeContext
pub trait RuntimeContextRegisterExt {
    /// Register a result with failure tracking
    fn register_with_failure_tracking(
        &mut self,
        host: &str,
        name: String,
        result: RegisteredResult,
        task_name: &str,
        module: &str,
    ) -> Option<FailedTaskInfo>;

    /// Get all failed task info for a host
    fn get_failed_tasks(&self, host: &str) -> Vec<&FailedTaskInfo>;

    /// Clear failed task info for a host
    fn clear_failed_tasks(&mut self, host: &str);
}

/// Storage for failed task information per host
#[derive(Debug, Default)]
pub struct FailedTaskStore {
    failures: HashMap<String, Vec<FailedTaskInfo>>,
}

impl FailedTaskStore {
    /// Create a new failed task store
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a failure
    pub fn add_failure(&mut self, host: &str, info: FailedTaskInfo) {
        self.failures
            .entry(host.to_string())
            .or_default()
            .push(info);
    }

    /// Get failures for a host
    pub fn get_failures(&self, host: &str) -> &[FailedTaskInfo] {
        self.failures.get(host).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Clear failures for a host
    pub fn clear_failures(&mut self, host: &str) {
        self.failures.remove(host);
    }

    /// Get all failures across all hosts
    pub fn all_failures(&self) -> impl Iterator<Item = &FailedTaskInfo> {
        self.failures.values().flatten()
    }

    /// Get total failure count
    pub fn failure_count(&self) -> usize {
        self.failures.values().map(|v| v.len()).sum()
    }

    /// Check if any failures exist
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registered_result_ext_basic() {
        let result = RegisteredResult {
            changed: true,
            failed: false,
            skipped: false,
            rc: Some(0),
            stdout: Some("hello\nworld".to_string()),
            stdout_lines: Some(vec!["hello".to_string(), "world".to_string()]),
            stderr: None,
            stderr_lines: None,
            msg: Some("Test message".to_string()),
            results: None,
            data: IndexMap::new(),
        };

        assert!(result.is_changed());
        assert!(result.is_ok());
        assert!(!result.is_failed());
        assert!(!result.is_skipped());
        assert_eq!(result.rc(), Some(0));
        assert!(result.rc_is_zero());
        assert_eq!(result.stdout_str(), "hello\nworld");
        assert_eq!(result.stdout_lines_slice().len(), 2);
        assert_eq!(result.msg_str(), "Test message");
    }

    #[test]
    fn test_registered_result_ext_nested_access() {
        let mut result = RegisteredResult::ok(false);
        result.data.insert(
            "stat".to_string(),
            serde_json::json!({
                "exists": true,
                "mode": "0644",
                "size": 1234
            }),
        );

        assert_eq!(result.get_nested_bool("stat.exists"), Some(true));
        assert_eq!(result.get_nested_string("stat.mode"), Some("0644"));
        assert_eq!(result.get_nested_i64("stat.size"), Some(1234));
        assert_eq!(result.get_nested("stat.nonexistent"), None);
    }

    #[test]
    fn test_loop_results() {
        let mut result = RegisteredResult::ok(true);
        result.results = Some(vec![
            RegisteredResult {
                changed: true,
                failed: false,
                ..Default::default()
            },
            RegisteredResult {
                changed: false,
                failed: false,
                ..Default::default()
            },
            RegisteredResult {
                changed: true,
                failed: true,
                ..Default::default()
            },
        ]);

        let loop_results = result.loop_results();
        assert!(loop_results.has_results());
        assert_eq!(loop_results.len(), 3);
        assert!(loop_results.any_changed());
        assert!(!loop_results.all_ok());
        assert!(loop_results.any_failed());
        assert_eq!(loop_results.changed_count(), 2);
        assert_eq!(loop_results.failed_count(), 1);

        let item = loop_results.get(1).unwrap();
        assert_eq!(item.index, 1);
        assert!(!item.is_changed());
        assert!(!item.is_failed());
    }

    #[test]
    fn test_failed_task_info() {
        let mut result = RegisteredResult::failed("Command not found");
        result.rc = Some(127);
        result.stderr = Some("bash: foo: command not found".to_string());

        let info = FailedTaskInfo::from_result(&result, "localhost", "Run foo", "command");
        assert!(info.is_some());

        let info = info.unwrap();
        assert_eq!(info.host, "localhost");
        assert_eq!(info.task_name, "Run foo");
        assert_eq!(info.module, "command");
        assert_eq!(info.error_message, "Command not found");
        assert_eq!(info.exit_code, Some(127));
        assert!(info.is_command_failure());
    }

    #[test]
    fn test_failed_task_info_builder() {
        let info = FailedTaskInfo::builder("host1", "Install package", "apt")
            .error_message("Package not found")
            .exit_code(100)
            .stderr("E: Unable to locate package foo")
            .context("package_name", serde_json::json!("foo"))
            .build();

        assert_eq!(info.host, "host1");
        assert_eq!(info.error_message, "Package not found");
        assert_eq!(info.exit_code, Some(100));
        assert!(info.context.contains_key("package_name"));
    }

    #[test]
    fn test_registered_result_builder() {
        let result = RegisteredResultBuilder::ok()
            .set_changed(true)
            .rc(0)
            .stdout("line1\nline2")
            .msg("Command executed successfully")
            .data("cmd", serde_json::json!("echo hello"))
            .build();

        assert!(result.changed);
        assert!(!result.failed);
        assert_eq!(result.rc, Some(0));
        assert_eq!(result.stdout, Some("line1\nline2".to_string()));
        assert_eq!(result.stdout_lines.as_ref().unwrap().len(), 2);
        assert_eq!(
            result.msg,
            Some("Command executed successfully".to_string())
        );
        assert!(result.data.contains_key("cmd"));
    }

    #[test]
    fn test_registered_result_builder_command_result() {
        let result = RegisteredResultBuilder::new()
            .command_result(0, "output here", "warning here")
            .msg("Done")
            .build();

        assert!(result.changed);
        assert_eq!(result.rc, Some(0));
        assert_eq!(result.stdout, Some("output here".to_string()));
        assert_eq!(result.stderr, Some("warning here".to_string()));
    }

    #[test]
    fn test_async_result_registry() {
        let mut registry = AsyncResultRegistry::new();

        let _handle = registry.register_pending("task1");
        assert!(registry.is_pending("task1"));
        assert_eq!(registry.pending_count(), 1);

        let result = RegisteredResult::ok(true);
        assert!(registry.complete("task1", result));
        assert!(!registry.is_pending("task1"));
        assert!(registry.get_completed("task1").is_some());
        assert_eq!(registry.completed_count(), 1);
    }

    #[test]
    fn test_failed_task_store() {
        let mut store = FailedTaskStore::new();

        let info1 = FailedTaskInfo::builder("host1", "Task 1", "command")
            .error_message("Error 1")
            .build();

        let info2 = FailedTaskInfo::builder("host1", "Task 2", "shell")
            .error_message("Error 2")
            .build();

        let info3 = FailedTaskInfo::builder("host2", "Task 1", "apt")
            .error_message("Error 3")
            .build();

        store.add_failure("host1", info1);
        store.add_failure("host1", info2);
        store.add_failure("host2", info3);

        assert!(store.has_failures());
        assert_eq!(store.failure_count(), 3);
        assert_eq!(store.get_failures("host1").len(), 2);
        assert_eq!(store.get_failures("host2").len(), 1);

        store.clear_failures("host1");
        assert_eq!(store.failure_count(), 1);
    }
}
