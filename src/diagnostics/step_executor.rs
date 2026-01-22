//! Step-by-step execution control.
//!
//! This module provides fine-grained control over playbook execution,
//! allowing users to step through tasks one at a time, skip tasks,
//! or run until a specific condition is met.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Current state of step execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Not in step mode, running freely
    #[default]
    Running,
    /// Paused, waiting for user input
    Paused,
    /// Stepping through (will pause after next task)
    Stepping,
    /// Running until a breakpoint
    RunningToBreakpoint,
    /// Running until end of current play
    RunningToPlayEnd,
    /// Running until end of current host
    RunningToHostEnd,
    /// Completed execution
    Completed,
    /// Execution was aborted
    Aborted,
}

impl StepState {
    /// Check if we should pause before the next task
    pub fn should_pause(&self) -> bool {
        matches!(self, StepState::Paused | StepState::Stepping)
    }

    /// Check if execution should continue
    pub fn should_continue(&self) -> bool {
        matches!(
            self,
            StepState::Running
                | StepState::Stepping
                | StepState::RunningToBreakpoint
                | StepState::RunningToPlayEnd
                | StepState::RunningToHostEnd
        )
    }

    /// Check if execution is finished
    pub fn is_finished(&self) -> bool {
        matches!(self, StepState::Completed | StepState::Aborted)
    }
}

/// Action to take during step execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum StepAction {
    /// Continue to next task and pause
    Step,
    /// Continue to next task on same host
    StepHost,
    /// Continue running all tasks
    Continue,
    /// Run until next breakpoint
    RunToBreakpoint,
    /// Run until end of current play
    RunToPlayEnd,
    /// Run until end of current host
    RunToHostEnd,
    /// Run the current task N times (for loops)
    Repeat(usize),
    /// Skip the current task
    Skip,
    /// Skip remaining tasks for current host
    SkipHost,
    /// Skip remaining tasks in current play
    SkipPlay,
    /// Abort execution
    Abort,
    /// Retry the last failed task
    Retry,
    /// Edit the current task (interactive mode)
    Edit,
    /// Inspect current state
    Inspect,
    /// No action (waiting for input)
    #[default]
    None,
}

impl StepAction {
    /// Parse an action from a string command
    pub fn from_command(cmd: &str) -> Option<Self> {
        let cmd = cmd.trim().to_lowercase();
        match cmd.as_str() {
            "s" | "step" | "n" | "next" => Some(StepAction::Step),
            "sh" | "step-host" => Some(StepAction::StepHost),
            "c" | "continue" | "run" => Some(StepAction::Continue),
            "b" | "breakpoint" | "run-to-breakpoint" => Some(StepAction::RunToBreakpoint),
            "p" | "play" | "run-to-play-end" => Some(StepAction::RunToPlayEnd),
            "h" | "host" | "run-to-host-end" => Some(StepAction::RunToHostEnd),
            "skip" | "sk" => Some(StepAction::Skip),
            "skip-host" | "skh" => Some(StepAction::SkipHost),
            "skip-play" | "skp" => Some(StepAction::SkipPlay),
            "q" | "quit" | "abort" | "exit" => Some(StepAction::Abort),
            "r" | "retry" => Some(StepAction::Retry),
            "e" | "edit" => Some(StepAction::Edit),
            "i" | "inspect" | "vars" => Some(StepAction::Inspect),
            _ => {
                // Check for repeat command: repeat N or rN
                if let Some(repeat_count) = cmd.strip_prefix("repeat ") {
                    repeat_count.parse().ok().map(StepAction::Repeat)
                } else if let Some(repeat_count) = cmd.strip_prefix('r') {
                    if repeat_count.is_empty() {
                        None
                    } else {
                        repeat_count.parse().ok().map(StepAction::Repeat)
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Get help text for available commands
    pub fn help() -> &'static str {
        r"Step execution commands:
  s, step, n, next    - Execute current task and pause
  sh, step-host       - Step to next task on same host
  c, continue, run    - Continue running all tasks
  b, breakpoint       - Run until next breakpoint
  p, play             - Run until end of current play
  h, host             - Run until end of current host
  skip, sk            - Skip current task
  skip-host, skh      - Skip remaining tasks for current host
  skip-play, skp      - Skip remaining tasks in current play
  repeat N, rN        - Repeat current task N times
  r, retry            - Retry the last failed task
  e, edit             - Edit current task (interactive)
  i, inspect, vars    - Inspect current variables
  q, quit, abort      - Abort execution"
    }
}

/// Result of a step operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Whether the step was successful
    pub success: bool,
    /// Current state after the step
    pub state: StepState,
    /// Task that was executed (if any)
    pub task_name: Option<String>,
    /// Host that was targeted (if any)
    pub host: Option<String>,
    /// Whether the task was skipped
    pub skipped: bool,
    /// Whether the task made changes
    pub changed: bool,
    /// Error message (if any)
    pub error: Option<String>,
    /// Suggested next action
    pub suggested_action: Option<StepAction>,
}

impl StepResult {
    /// Create a successful step result
    pub fn success(state: StepState) -> Self {
        Self {
            success: true,
            state,
            task_name: None,
            host: None,
            skipped: false,
            changed: false,
            error: None,
            suggested_action: None,
        }
    }

    /// Create a failed step result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            state: StepState::Paused,
            task_name: None,
            host: None,
            skipped: false,
            changed: false,
            error: Some(error.into()),
            suggested_action: Some(StepAction::Retry),
        }
    }

    /// Set the task name
    pub fn with_task(mut self, name: impl Into<String>) -> Self {
        self.task_name = Some(name.into());
        self
    }

    /// Set the host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Set skipped flag
    pub fn with_skipped(mut self, skipped: bool) -> Self {
        self.skipped = skipped;
        self
    }

    /// Set changed flag
    pub fn with_changed(mut self, changed: bool) -> Self {
        self.changed = changed;
        self
    }
}

/// Step executor for controlling task-by-task execution
#[derive(Debug)]
pub struct StepExecutor {
    /// Whether step mode is enabled
    enabled: bool,
    /// Current state
    state: Arc<RwLock<StepState>>,
    /// Current action
    action: Arc<RwLock<StepAction>>,
    /// Hosts to skip
    skipped_hosts: Arc<RwLock<HashSet<String>>>,
    /// Current play (for play-level operations)
    current_play: Arc<RwLock<Option<String>>>,
    /// Current task index
    task_index: Arc<RwLock<usize>>,
    /// Total tasks in current play
    total_tasks: Arc<RwLock<usize>>,
    /// Tasks executed this session
    tasks_executed: Arc<RwLock<usize>>,
    /// History of step results
    history: Arc<RwLock<Vec<StepResult>>>,
    /// Maximum history size
    max_history: usize,
}

impl StepExecutor {
    /// Create a new step executor
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            state: Arc::new(RwLock::new(if enabled {
                StepState::Paused
            } else {
                StepState::Running
            })),
            action: Arc::new(RwLock::new(StepAction::None)),
            skipped_hosts: Arc::new(RwLock::new(HashSet::new())),
            current_play: Arc::new(RwLock::new(None)),
            task_index: Arc::new(RwLock::new(0)),
            total_tasks: Arc::new(RwLock::new(0)),
            tasks_executed: Arc::new(RwLock::new(0)),
            history: Arc::new(RwLock::new(Vec::new())),
            max_history: 100,
        }
    }

    /// Check if step mode is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable step mode
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            *self.state.write() = StepState::Paused;
        } else {
            *self.state.write() = StepState::Running;
        }
    }

    /// Check if we should step (pause) before next task
    pub fn should_step(&self) -> bool {
        self.enabled && self.state.read().should_pause()
    }

    /// Get the current state
    pub fn state(&self) -> StepState {
        *self.state.read()
    }

    /// Set the current state
    pub fn set_state(&self, state: StepState) {
        *self.state.write() = state;
    }

    /// Get the current action
    pub fn get_action(&self) -> StepAction {
        self.action.read().clone()
    }

    /// Set the next action
    pub fn set_action(&self, action: StepAction) {
        let mut state = self.state.write();
        let mut act = self.action.write();

        match &action {
            StepAction::Step | StepAction::StepHost => {
                *state = StepState::Stepping;
            }
            StepAction::Continue => {
                *state = StepState::Running;
            }
            StepAction::RunToBreakpoint => {
                *state = StepState::RunningToBreakpoint;
            }
            StepAction::RunToPlayEnd => {
                *state = StepState::RunningToPlayEnd;
            }
            StepAction::RunToHostEnd => {
                *state = StepState::RunningToHostEnd;
            }
            StepAction::Abort => {
                *state = StepState::Aborted;
            }
            _ => {}
        }

        *act = action;
    }

    /// Check if a host should be skipped
    pub fn is_host_skipped(&self, host: &str) -> bool {
        self.skipped_hosts.read().contains(host)
    }

    /// Mark a host to be skipped
    pub fn skip_host(&self, host: impl Into<String>) {
        self.skipped_hosts.write().insert(host.into());
    }

    /// Clear skipped hosts
    pub fn clear_skipped_hosts(&self) {
        self.skipped_hosts.write().clear();
    }

    /// Set the current play
    pub fn set_play(&self, name: impl Into<String>, total_tasks: usize) {
        *self.current_play.write() = Some(name.into());
        *self.total_tasks.write() = total_tasks;
        *self.task_index.write() = 0;
    }

    /// Get current play name
    pub fn current_play(&self) -> Option<String> {
        self.current_play.read().clone()
    }

    /// Advance to next task
    pub fn advance_task(&self) -> usize {
        let mut idx = self.task_index.write();
        *idx += 1;
        *self.tasks_executed.write() += 1;

        // After stepping, go back to paused
        let state = *self.state.read();
        if state == StepState::Stepping {
            *self.state.write() = StepState::Paused;
        }

        *idx
    }

    /// Get current task index
    pub fn task_index(&self) -> usize {
        *self.task_index.read()
    }

    /// Get total tasks
    pub fn total_tasks(&self) -> usize {
        *self.total_tasks.read()
    }

    /// Get tasks executed count
    pub fn tasks_executed(&self) -> usize {
        *self.tasks_executed.read()
    }

    /// Get progress as a percentage
    pub fn progress(&self) -> f64 {
        let total = *self.total_tasks.read();
        if total == 0 {
            0.0
        } else {
            (*self.task_index.read() as f64 / total as f64) * 100.0
        }
    }

    /// Check if at end of play
    pub fn is_play_complete(&self) -> bool {
        *self.task_index.read() >= *self.total_tasks.read()
    }

    /// Record a step result
    pub fn record_result(&self, result: StepResult) {
        let mut history = self.history.write();
        history.push(result);
        if history.len() > self.max_history {
            history.remove(0);
        }
    }

    /// Get step history
    pub fn history(&self) -> Vec<StepResult> {
        self.history.read().clone()
    }

    /// Get the last step result
    pub fn last_result(&self) -> Option<StepResult> {
        self.history.read().last().cloned()
    }

    /// Clear history
    pub fn clear_history(&self) {
        self.history.write().clear();
    }

    /// Reset for a new playbook
    pub fn reset(&self) {
        *self.state.write() = if self.enabled {
            StepState::Paused
        } else {
            StepState::Running
        };
        *self.action.write() = StepAction::None;
        self.skipped_hosts.write().clear();
        *self.current_play.write() = None;
        *self.task_index.write() = 0;
        *self.total_tasks.write() = 0;
        *self.tasks_executed.write() = 0;
    }

    /// Mark execution as complete
    pub fn complete(&self) {
        *self.state.write() = StepState::Completed;
    }

    /// Pause execution
    pub fn pause(&self) {
        *self.state.write() = StepState::Paused;
    }

    /// Resume execution
    pub fn resume(&self) {
        *self.state.write() = StepState::Running;
    }

    /// Check if we should continue based on current state and play context
    pub fn should_continue_play(&self, play_name: &str) -> bool {
        let state = *self.state.read();
        let current = self.current_play.read();

        match state {
            StepState::RunningToPlayEnd => current.as_deref() == Some(play_name),
            StepState::Running | StepState::RunningToBreakpoint => true,
            _ => false,
        }
    }

    /// Check if we should continue based on current host
    pub fn should_continue_host(&self, host: &str) -> bool {
        if self.is_host_skipped(host) {
            return false;
        }

        let state = *self.state.read();
        matches!(
            state,
            StepState::Running
                | StepState::RunningToBreakpoint
                | StepState::RunningToPlayEnd
                | StepState::RunningToHostEnd
        )
    }
}

impl Default for StepExecutor {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_state_should_pause() {
        assert!(StepState::Paused.should_pause());
        assert!(StepState::Stepping.should_pause());
        assert!(!StepState::Running.should_pause());
        assert!(!StepState::RunningToBreakpoint.should_pause());
    }

    #[test]
    fn test_step_action_from_command() {
        assert_eq!(StepAction::from_command("step"), Some(StepAction::Step));
        assert_eq!(StepAction::from_command("c"), Some(StepAction::Continue));
        assert_eq!(StepAction::from_command("skip"), Some(StepAction::Skip));
        assert_eq!(
            StepAction::from_command("repeat 5"),
            Some(StepAction::Repeat(5))
        );
        assert_eq!(StepAction::from_command("r3"), Some(StepAction::Repeat(3)));
        assert_eq!(StepAction::from_command("unknown"), None);
    }

    #[test]
    fn test_step_executor_creation() {
        let executor = StepExecutor::new(true);
        assert!(executor.is_enabled());
        assert_eq!(executor.state(), StepState::Paused);

        let executor = StepExecutor::new(false);
        assert!(!executor.is_enabled());
        assert_eq!(executor.state(), StepState::Running);
    }

    #[test]
    fn test_step_executor_actions() {
        let executor = StepExecutor::new(true);

        executor.set_action(StepAction::Step);
        assert_eq!(executor.state(), StepState::Stepping);

        executor.set_action(StepAction::Continue);
        assert_eq!(executor.state(), StepState::Running);

        executor.set_action(StepAction::Abort);
        assert_eq!(executor.state(), StepState::Aborted);
    }

    #[test]
    fn test_step_executor_task_tracking() {
        let executor = StepExecutor::new(true);
        executor.set_play("Test Play", 5);

        assert_eq!(executor.task_index(), 0);
        assert_eq!(executor.total_tasks(), 5);
        assert_eq!(executor.progress(), 0.0);

        executor.advance_task();
        assert_eq!(executor.task_index(), 1);
        assert_eq!(executor.progress(), 20.0);

        executor.advance_task();
        executor.advance_task();
        executor.advance_task();
        executor.advance_task();
        assert!(executor.is_play_complete());
    }

    #[test]
    fn test_step_executor_host_skipping() {
        let executor = StepExecutor::new(true);

        assert!(!executor.is_host_skipped("host1"));

        executor.skip_host("host1");
        assert!(executor.is_host_skipped("host1"));
        assert!(!executor.is_host_skipped("host2"));

        executor.clear_skipped_hosts();
        assert!(!executor.is_host_skipped("host1"));
    }

    #[test]
    fn test_step_result() {
        let result = StepResult::success(StepState::Paused)
            .with_task("Install nginx")
            .with_host("web1")
            .with_changed(true);

        assert!(result.success);
        assert_eq!(result.task_name, Some("Install nginx".to_string()));
        assert!(result.changed);

        let result = StepResult::failure("Connection refused");
        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(result.suggested_action, Some(StepAction::Retry));
    }

    #[test]
    fn test_step_executor_stepping_pauses_after() {
        let executor = StepExecutor::new(true);
        executor.set_action(StepAction::Step);
        assert_eq!(executor.state(), StepState::Stepping);

        executor.advance_task();
        assert_eq!(executor.state(), StepState::Paused);
    }
}
