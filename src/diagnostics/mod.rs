//! Diagnostic tools for Rustible
//!
//! This module provides comprehensive debugging and diagnostic capabilities:
//!
//! - **Debug Mode**: Verbose connection tracing and execution logging
//! - **Variable Inspection**: Inspect variables at any execution point
//! - **Step-by-step Execution**: Execute tasks one at a time with pause points
//! - **Breakpoint Support**: Set breakpoints on tasks, hosts, or conditions
//! - **State Dump**: Automatic state capture on failure for post-mortem analysis
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::diagnostics::{DebugConfig, DebugContext, Breakpoint};
//!
//! let config = DebugConfig::builder()
//!     .with_verbosity(3)
//!     .with_trace_connections(true)
//!     .with_step_mode(true)
//!     .build();
//!
//! let mut debug_ctx = DebugContext::new(config);
//! debug_ctx.add_breakpoint(Breakpoint::on_task("Install nginx"));
//! debug_ctx.add_breakpoint(Breakpoint::on_failure());
//!
//! // Pass debug_ctx to your executor integration
//! let _ = debug_ctx;
//! # Ok(())
//! # }
//! ```

mod breakpoint;
mod config;
mod inspector;
pub mod rich_errors;
mod state_dump;
mod step_executor;
mod tracer;

pub use breakpoint::{Breakpoint, BreakpointCondition, BreakpointManager, BreakpointType};
pub use config::{DebugConfig, DebugConfigBuilder, DebugMode};
pub use inspector::{
    InspectionResult, VariableInspector, VariableScope, VariableSource, VariableWatch,
};
pub use rich_errors::{
    connection_error, invalid_module_args_error, missing_required_arg_error,
    module_not_found_error, template_syntax_error, undefined_variable_error, yaml_syntax_error,
    DiagnosticSeverity, ErrorCodeInfo, ErrorCodeRegistry, RelatedInfo, RichDiagnostic, Span,
    Suggestion,
};
pub use state_dump::{FailureContext, StateDump, StateDumpFormat, StateDumper};
pub use step_executor::{StepAction, StepExecutor, StepResult, StepState};
pub use tracer::{
    ConnectionEvent, ConnectionEventType, ConnectionTracer, TraceEntry, TraceLevel, TraceSink,
};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// The main debug context that coordinates all diagnostic features
#[derive(Debug)]
pub struct DebugContext {
    /// Debug configuration
    config: DebugConfig,
    /// Breakpoint manager for setting and checking breakpoints
    breakpoints: BreakpointManager,
    /// Connection tracer for verbose connection logging
    tracer: ConnectionTracer,
    /// Variable inspector for examining state
    inspector: VariableInspector,
    /// Step executor for step-by-step execution
    step_executor: StepExecutor,
    /// State dumper for failure capture
    state_dumper: StateDumper,
    /// Session start time
    session_start: DateTime<Utc>,
    /// Execution history
    history: Arc<RwLock<Vec<ExecutionEvent>>>,
    /// Current execution state
    state: Arc<RwLock<DebugState>>,
}

impl DebugContext {
    /// Create a new debug context with the given configuration
    pub fn new(config: DebugConfig) -> Self {
        Self {
            tracer: ConnectionTracer::new(config.trace_level),
            inspector: VariableInspector::new(),
            step_executor: StepExecutor::new(config.step_mode),
            state_dumper: StateDumper::new(config.dump_on_failure, config.dump_path.clone()),
            breakpoints: BreakpointManager::new(),
            config,
            session_start: Utc::now(),
            history: Arc::new(RwLock::new(Vec::new())),
            state: Arc::new(RwLock::new(DebugState::default())),
        }
    }

    /// Create a debug context with default settings
    pub fn default_context() -> Self {
        Self::new(DebugConfig::default())
    }

    /// Create a debug context for verbose debugging
    pub fn verbose() -> Self {
        Self::new(DebugConfig::verbose())
    }

    /// Add a breakpoint
    pub fn add_breakpoint(&mut self, breakpoint: Breakpoint) {
        self.breakpoints.add(breakpoint);
    }

    /// Remove a breakpoint by ID
    pub fn remove_breakpoint(&mut self, id: &str) -> bool {
        self.breakpoints.remove(id)
    }

    /// Clear all breakpoints
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Check if we should break at the current execution point
    pub fn should_break(&self, context: &BreakpointContext) -> Option<&Breakpoint> {
        self.breakpoints.check(context)
    }

    /// Add a variable watch
    pub fn watch_variable(&mut self, name: impl Into<String>, scope: Option<VariableScope>) {
        self.inspector.add_watch(name, scope);
    }

    /// Remove a variable watch
    pub fn unwatch_variable(&mut self, name: &str) {
        self.inspector.remove_watch(name);
    }

    /// Inspect all watched variables
    pub fn inspect_watched(&self, vars: &HashMap<String, JsonValue>) -> Vec<InspectionResult> {
        self.inspector.inspect_watched(vars)
    }

    /// Inspect a specific variable
    pub fn inspect_variable(
        &self,
        name: &str,
        vars: &HashMap<String, JsonValue>,
    ) -> Option<InspectionResult> {
        self.inspector.inspect(name, vars)
    }

    /// Log a connection event
    pub fn trace_connection(&self, event: ConnectionEvent) {
        self.tracer.trace(event);
    }

    /// Get connection trace history
    pub fn get_trace(&self) -> Vec<TraceEntry> {
        self.tracer.get_entries()
    }

    /// Clear connection trace history
    pub fn clear_trace(&self) {
        self.tracer.clear();
    }

    /// Check if step mode should pause before a task
    pub fn should_step(&self) -> bool {
        self.step_executor.should_step()
    }

    /// Get the current step action
    pub fn get_step_action(&self) -> StepAction {
        self.step_executor.get_action()
    }

    /// Set the next step action
    pub fn set_step_action(&mut self, action: StepAction) {
        self.step_executor.set_action(action);
    }

    /// Record an execution event
    pub fn record_event(&self, event: ExecutionEvent) {
        let mut history = self.history.write();
        history.push(event);
    }

    /// Get execution history
    pub fn get_history(&self) -> Vec<ExecutionEvent> {
        self.history.read().clone()
    }

    /// Dump state on failure
    pub fn dump_failure_state(&self, context: FailureContext) -> Result<PathBuf, std::io::Error> {
        self.state_dumper.dump(context)
    }

    /// Get the debug configuration
    pub fn config(&self) -> &DebugConfig {
        &self.config
    }

    /// Get current debug state
    pub fn state(&self) -> DebugState {
        self.state.read().clone()
    }

    /// Update current debug state
    pub fn update_state<F>(&self, f: F)
    where
        F: FnOnce(&mut DebugState),
    {
        let mut state = self.state.write();
        f(&mut state);
    }

    /// Get session duration
    pub fn session_duration(&self) -> chrono::Duration {
        Utc::now() - self.session_start
    }

    /// Get the connection tracer
    pub fn tracer(&self) -> &ConnectionTracer {
        &self.tracer
    }

    /// Get the variable inspector
    pub fn inspector(&self) -> &VariableInspector {
        &self.inspector
    }

    /// Get the step executor
    pub fn step_executor(&self) -> &StepExecutor {
        &self.step_executor
    }

    /// Get mutable access to step executor
    pub fn step_executor_mut(&mut self) -> &mut StepExecutor {
        &mut self.step_executor
    }

    /// Check if debugging is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.mode != DebugMode::Disabled
    }

    /// Check if verbose tracing is enabled
    pub fn is_verbose(&self) -> bool {
        self.config.verbosity >= 2
    }

    /// Get verbosity level
    pub fn verbosity(&self) -> u8 {
        self.config.verbosity
    }
}

impl Default for DebugContext {
    fn default() -> Self {
        Self::default_context()
    }
}

/// Current debug execution state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugState {
    /// Current playbook being executed
    pub current_playbook: Option<String>,
    /// Current play being executed
    pub current_play: Option<String>,
    /// Current task being executed
    pub current_task: Option<String>,
    /// Current host being targeted
    pub current_host: Option<String>,
    /// Whether we're paused at a breakpoint
    pub paused: bool,
    /// Reason for pause (if paused)
    pub pause_reason: Option<String>,
    /// Number of tasks executed
    pub tasks_executed: usize,
    /// Number of tasks failed
    pub tasks_failed: usize,
    /// Number of tasks skipped
    pub tasks_skipped: usize,
    /// Current loop index (if in a loop)
    pub loop_index: Option<usize>,
    /// Current loop item (if in a loop)
    pub loop_item: Option<JsonValue>,
}

impl DebugState {
    /// Create a new debug state
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the current playbook
    pub fn set_playbook(&mut self, name: impl Into<String>) {
        self.current_playbook = Some(name.into());
        self.current_play = None;
        self.current_task = None;
    }

    /// Set the current play
    pub fn set_play(&mut self, name: impl Into<String>) {
        self.current_play = Some(name.into());
        self.current_task = None;
    }

    /// Set the current task
    pub fn set_task(&mut self, name: impl Into<String>) {
        self.current_task = Some(name.into());
    }

    /// Set the current host
    pub fn set_host(&mut self, host: impl Into<String>) {
        self.current_host = Some(host.into());
    }

    /// Mark as paused
    pub fn pause(&mut self, reason: impl Into<String>) {
        self.paused = true;
        self.pause_reason = Some(reason.into());
    }

    /// Resume execution
    pub fn resume(&mut self) {
        self.paused = false;
        self.pause_reason = None;
    }

    /// Record a task execution
    pub fn record_task(&mut self, failed: bool, skipped: bool) {
        self.tasks_executed += 1;
        if failed {
            self.tasks_failed += 1;
        }
        if skipped {
            self.tasks_skipped += 1;
        }
    }

    /// Set loop context
    pub fn set_loop(&mut self, index: usize, item: JsonValue) {
        self.loop_index = Some(index);
        self.loop_item = Some(item);
    }

    /// Clear loop context
    pub fn clear_loop(&mut self) {
        self.loop_index = None;
        self.loop_item = None;
    }
}

/// An execution event for history tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEvent {
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// Type of event
    pub event_type: ExecutionEventType,
    /// Host involved (if any)
    pub host: Option<String>,
    /// Task involved (if any)
    pub task: Option<String>,
    /// Module involved (if any)
    pub module: Option<String>,
    /// Additional details
    pub details: Option<String>,
    /// Result data (if applicable)
    pub result: Option<JsonValue>,
}

impl ExecutionEvent {
    /// Create a new execution event
    pub fn new(event_type: ExecutionEventType) -> Self {
        Self {
            timestamp: Utc::now(),
            event_type,
            host: None,
            task: None,
            module: None,
            details: None,
            result: None,
        }
    }

    /// Set the host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Set the task
    pub fn with_task(mut self, task: impl Into<String>) -> Self {
        self.task = Some(task.into());
        self
    }

    /// Set the module
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module = Some(module.into());
        self
    }

    /// Set details
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Set result
    pub fn with_result(mut self, result: JsonValue) -> Self {
        self.result = Some(result);
        self
    }
}

/// Type of execution event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionEventType {
    /// Playbook started
    PlaybookStart,
    /// Playbook completed
    PlaybookEnd,
    /// Play started
    PlayStart,
    /// Play completed
    PlayEnd,
    /// Task started
    TaskStart,
    /// Task completed successfully
    TaskOk,
    /// Task made changes
    TaskChanged,
    /// Task failed
    TaskFailed,
    /// Task skipped
    TaskSkipped,
    /// Host unreachable
    HostUnreachable,
    /// Handler triggered
    HandlerNotified,
    /// Handler executed
    HandlerExecuted,
    /// Variable set
    VariableSet,
    /// Fact gathered
    FactGathered,
    /// Breakpoint hit
    BreakpointHit,
    /// User paused execution
    UserPause,
    /// User resumed execution
    UserResume,
    /// Connection established
    ConnectionEstablished,
    /// Connection closed
    ConnectionClosed,
    /// Connection error
    ConnectionError,
}

/// Context for breakpoint evaluation
#[derive(Debug, Clone)]
pub struct BreakpointContext {
    /// Current playbook name
    pub playbook: Option<String>,
    /// Current play name
    pub play: Option<String>,
    /// Current task name
    pub task: Option<String>,
    /// Current host
    pub host: Option<String>,
    /// Current module
    pub module: Option<String>,
    /// Whether the last task failed
    pub failed: bool,
    /// Whether the last task changed something
    pub changed: bool,
    /// Current task number
    pub task_number: usize,
    /// Total number of tasks
    pub total_tasks: usize,
    /// Current variables
    pub variables: HashMap<String, JsonValue>,
}

impl BreakpointContext {
    /// Create a new breakpoint context
    pub fn new() -> Self {
        Self {
            playbook: None,
            play: None,
            task: None,
            host: None,
            module: None,
            failed: false,
            changed: false,
            task_number: 0,
            total_tasks: 0,
            variables: HashMap::new(),
        }
    }

    /// Set the playbook
    pub fn with_playbook(mut self, playbook: impl Into<String>) -> Self {
        self.playbook = Some(playbook.into());
        self
    }

    /// Set the play
    pub fn with_play(mut self, play: impl Into<String>) -> Self {
        self.play = Some(play.into());
        self
    }

    /// Set the task
    pub fn with_task(mut self, task: impl Into<String>) -> Self {
        self.task = Some(task.into());
        self
    }

    /// Set the host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Set the module
    pub fn with_module(mut self, module: impl Into<String>) -> Self {
        self.module = Some(module.into());
        self
    }

    /// Set the failed flag
    pub fn with_failed(mut self, failed: bool) -> Self {
        self.failed = failed;
        self
    }

    /// Set the changed flag
    pub fn with_changed(mut self, changed: bool) -> Self {
        self.changed = changed;
        self
    }

    /// Set task progress
    pub fn with_progress(mut self, current: usize, total: usize) -> Self {
        self.task_number = current;
        self.total_tasks = total;
        self
    }

    /// Set variables
    pub fn with_variables(mut self, vars: HashMap<String, JsonValue>) -> Self {
        self.variables = vars;
        self
    }
}

impl Default for BreakpointContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_context_creation() {
        let config = DebugConfig::default();
        let ctx = DebugContext::new(config);
        assert!(!ctx.is_enabled());
    }

    #[test]
    fn test_debug_context_verbose() {
        let ctx = DebugContext::verbose();
        assert!(ctx.is_enabled());
        assert!(ctx.is_verbose());
    }

    #[test]
    fn test_debug_state() {
        let mut state = DebugState::new();
        state.set_playbook("test.yml");
        state.set_play("Install software");
        state.set_task("Install nginx");
        state.set_host("web1");

        assert_eq!(state.current_playbook, Some("test.yml".to_string()));
        assert_eq!(state.current_play, Some("Install software".to_string()));
        assert_eq!(state.current_task, Some("Install nginx".to_string()));
        assert_eq!(state.current_host, Some("web1".to_string()));
    }

    #[test]
    fn test_execution_event() {
        let event = ExecutionEvent::new(ExecutionEventType::TaskStart)
            .with_host("web1")
            .with_task("Install nginx")
            .with_module("apt");

        assert_eq!(event.event_type, ExecutionEventType::TaskStart);
        assert_eq!(event.host, Some("web1".to_string()));
        assert_eq!(event.task, Some("Install nginx".to_string()));
        assert_eq!(event.module, Some("apt".to_string()));
    }

    #[test]
    fn test_breakpoint_context() {
        let ctx = BreakpointContext::new()
            .with_playbook("test.yml")
            .with_play("Install")
            .with_task("apt install")
            .with_host("web1")
            .with_failed(true);

        assert_eq!(ctx.playbook, Some("test.yml".to_string()));
        assert!(ctx.failed);
    }
}
