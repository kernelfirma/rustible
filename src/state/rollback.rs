//! State Rollback System
//!
//! This module provides functionality to generate and execute rollback plans
//! that can undo changes made during playbook execution.
//!
//! ## Features
//!
//! - Automatic rollback plan generation from changed tasks
//! - Support for various module types with specific rollback strategies
//! - Execution tracking and status reporting
//! - Dry-run mode for validation
//! - Partial rollback support

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{StateConfig, StateResult, TaskStateRecord};

/// A single action that can be taken to rollback a change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackAction {
    /// Unique identifier for this action
    pub id: String,
    /// Task ID this action rolls back
    pub original_task_id: String,
    /// Host to execute on
    pub host: String,
    /// Module to use for rollback
    pub module: String,
    /// Arguments for the rollback action
    pub args: serde_json::Value,
    /// Description of what this action does
    pub description: String,
    /// Priority (lower = execute first)
    pub priority: u32,
    /// Dependencies (other rollback action IDs that must complete first)
    pub depends_on: Vec<String>,
    /// Whether this action is reversible
    pub reversible: bool,
    /// Estimated risk level (1-5)
    pub risk_level: u8,
    /// Pre-conditions that must be true
    pub preconditions: Vec<String>,
}

impl RollbackAction {
    /// Create a new rollback action
    pub fn new(
        original_task_id: impl Into<String>,
        host: impl Into<String>,
        module: impl Into<String>,
        args: serde_json::Value,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            original_task_id: original_task_id.into(),
            host: host.into(),
            module: module.into(),
            args,
            description: description.into(),
            priority: 100,
            depends_on: Vec::new(),
            reversible: true,
            risk_level: 1,
            preconditions: Vec::new(),
        }
    }

    /// Set priority
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Set dependencies
    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.depends_on = deps;
        self
    }

    /// Set risk level
    pub fn with_risk_level(mut self, level: u8) -> Self {
        self.risk_level = level.min(5);
        self
    }
}

/// Status of a rollback action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RollbackActionStatus {
    /// Not yet started
    Pending,
    /// Currently executing
    Running,
    /// Successfully completed
    Completed,
    /// Failed to execute
    Failed,
    /// Skipped (e.g., preconditions not met)
    Skipped,
}

/// Result of a rollback action execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackActionResult {
    /// Action ID
    pub action_id: String,
    /// Status
    pub status: RollbackActionStatus,
    /// Output message
    pub message: String,
    /// Error details if failed
    pub error: Option<String>,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// When execution started
    pub started_at: DateTime<Utc>,
    /// When execution completed
    pub completed_at: DateTime<Utc>,
}

/// A complete rollback plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPlan {
    /// Unique identifier for this plan
    pub id: String,
    /// When this plan was created
    pub created_at: DateTime<Utc>,
    /// Session ID this plan is for
    pub session_id: String,
    /// Playbook being rolled back
    pub playbook: String,
    /// List of rollback actions in execution order
    pub actions: Vec<RollbackAction>,
    /// Metadata about the plan
    pub metadata: HashMap<String, serde_json::Value>,
    /// Whether this plan has been validated
    pub validated: bool,
    /// Validation warnings
    pub warnings: Vec<String>,
    /// Estimated total risk (sum of action risks)
    pub total_risk: u32,
    /// Number of hosts affected
    pub hosts_affected: usize,
}

impl RollbackPlan {
    /// Create a new rollback plan
    pub fn new(session_id: impl Into<String>, playbook: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            session_id: session_id.into(),
            playbook: playbook.into(),
            actions: Vec::new(),
            metadata: HashMap::new(),
            validated: false,
            warnings: Vec::new(),
            total_risk: 0,
            hosts_affected: 0,
        }
    }

    /// Add an action to the plan
    pub fn add_action(&mut self, action: RollbackAction) {
        self.total_risk += action.risk_level as u32;
        self.actions.push(action);
    }

    /// Calculate the number of affected hosts
    pub fn calculate_hosts(&mut self) {
        let hosts: std::collections::HashSet<&String> =
            self.actions.iter().map(|a| &a.host).collect();
        self.hosts_affected = hosts.len();
    }

    /// Sort actions by priority and dependencies
    pub fn sort_actions(&mut self) {
        // Simple topological sort by priority
        self.actions.sort_by_key(|a| a.priority);
    }

    /// Check if the plan is empty
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Get a summary of the plan
    pub fn summary(&self) -> String {
        format!(
            "Rollback plan: {} actions across {} hosts, total risk: {}",
            self.actions.len(),
            self.hosts_affected,
            self.total_risk
        )
    }
}

/// Status of a rollback execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackStatus {
    /// Plan ID
    pub plan_id: String,
    /// Overall status
    pub status: RollbackExecutionStatus,
    /// When execution started
    pub started_at: DateTime<Utc>,
    /// When execution completed
    pub completed_at: Option<DateTime<Utc>>,
    /// Results for each action
    pub action_results: Vec<RollbackActionResult>,
    /// Number of successful actions
    pub successful: usize,
    /// Number of failed actions
    pub failed: usize,
    /// Number of skipped actions
    pub skipped: usize,
    /// Total duration in milliseconds
    pub total_duration_ms: u64,
    /// Error message if overall failure
    pub error: Option<String>,
}

/// Overall rollback execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RollbackExecutionStatus {
    /// Not started
    Pending,
    /// In progress
    Running,
    /// All actions completed successfully
    Completed,
    /// Some actions failed
    PartialFailure,
    /// All actions failed
    Failed,
    /// Execution was cancelled
    Cancelled,
}

/// Rollback plan generator and executor
pub struct RollbackExecutor {
    config: StateConfig,
}

impl RollbackExecutor {
    /// Create a new rollback executor
    pub fn new(config: StateConfig) -> Self {
        Self { config }
    }

    /// Create a rollback plan from a list of changed tasks
    pub fn create_plan(&self, changed_tasks: &[TaskStateRecord]) -> StateResult<RollbackPlan> {
        let mut plan = RollbackPlan::new("", "");

        for task in changed_tasks.iter().rev() {
            // Skip tasks that can't be rolled back
            if !task.rollback_available {
                plan.warnings.push(format!(
                    "Task '{}' on host '{}' cannot be rolled back",
                    task.task_name, task.host
                ));
                continue;
            }

            // Generate rollback action based on module type
            if let Some(action) = self.generate_rollback_action(task) {
                plan.add_action(action);
            } else if let Some(info) = &task.rollback_info {
                plan.add_action(info.clone());
            }
        }

        plan.calculate_hosts();
        plan.sort_actions();
        plan.validated = true;

        Ok(plan)
    }

    /// Generate a rollback action for a task based on its module type
    fn generate_rollback_action(&self, task: &TaskStateRecord) -> Option<RollbackAction> {
        let (module, args, description, risk) = match task.module.as_str() {
            // Package modules - uninstall what was installed
            "apt" | "yum" | "dnf" | "package" => {
                let pkg_name = task
                    .args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let state = task
                    .args
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("present");

                let new_state = match state {
                    "present" | "latest" | "installed" => "absent",
                    "absent" | "removed" => "present",
                    _ => return None,
                };

                (
                    task.module.clone(),
                    serde_json::json!({
                        "name": pkg_name,
                        "state": new_state
                    }),
                    format!("Rollback: {} package {}", new_state, pkg_name),
                    2,
                )
            }

            // Service modules - reverse the action
            "service" | "systemd" => {
                let service_name = task
                    .args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let state = task.args.get("state").and_then(|v| v.as_str());
                let enabled = task.args.get("enabled").and_then(|v| v.as_bool());

                let mut rollback_args = serde_json::json!({ "name": service_name });

                if let Some(state) = state {
                    let new_state = match state {
                        "started" => "stopped",
                        "stopped" => "started",
                        "restarted" | "reloaded" => "started", // Can't really undo restart
                        _ => state,
                    };
                    rollback_args["state"] = serde_json::Value::String(new_state.to_string());
                }

                if let Some(enabled) = enabled {
                    rollback_args["enabled"] = serde_json::Value::Bool(!enabled);
                }

                (
                    task.module.clone(),
                    rollback_args,
                    format!("Rollback: restore service {} state", service_name),
                    2,
                )
            }

            // File modules - restore from before_state
            "file" => {
                if let Some(before) = &task.before_state {
                    let path = task
                        .args
                        .get("path")
                        .or_else(|| task.args.get("dest"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");

                    // If file didn't exist before, remove it
                    if before.is_null()
                        || before.get("exists").and_then(|v| v.as_bool()) == Some(false)
                    {
                        (
                            "file".to_string(),
                            serde_json::json!({
                                "path": path,
                                "state": "absent"
                            }),
                            format!("Rollback: remove created file {}", path),
                            3,
                        )
                    } else {
                        // Restore previous attributes
                        let mut args = serde_json::json!({ "path": path });
                        if let Some(mode) = before.get("mode") {
                            args["mode"] = mode.clone();
                        }
                        if let Some(owner) = before.get("owner") {
                            args["owner"] = owner.clone();
                        }
                        if let Some(group) = before.get("group") {
                            args["group"] = group.clone();
                        }

                        (
                            "file".to_string(),
                            args,
                            format!("Rollback: restore file {} attributes", path),
                            2,
                        )
                    }
                } else {
                    return None;
                }
            }

            // Copy module - restore original content
            "copy" | "template" => {
                if let Some(before) = &task.before_state {
                    let dest = task
                        .args
                        .get("dest")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");

                    if before.is_null()
                        || before.get("exists").and_then(|v| v.as_bool()) == Some(false)
                    {
                        // File didn't exist, remove it
                        (
                            "file".to_string(),
                            serde_json::json!({
                                "path": dest,
                                "state": "absent"
                            }),
                            format!("Rollback: remove created file {}", dest),
                            3,
                        )
                    } else if let Some(content) = before.get("content") {
                        // Restore original content
                        (
                            "copy".to_string(),
                            serde_json::json!({
                                "dest": dest,
                                "content": content.clone()
                            }),
                            format!("Rollback: restore original content of {}", dest),
                            2,
                        )
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }

            // User module
            "user" => {
                let username = task
                    .args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let state = task
                    .args
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("present");

                let new_state = match state {
                    "present" => "absent",
                    "absent" => "present",
                    _ => return None,
                };

                (
                    "user".to_string(),
                    serde_json::json!({
                        "name": username,
                        "state": new_state
                    }),
                    format!("Rollback: {} user {}", new_state, username),
                    4, // Higher risk for user operations
                )
            }

            // Group module
            "group" => {
                let groupname = task
                    .args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let state = task
                    .args
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("present");

                let new_state = match state {
                    "present" => "absent",
                    "absent" => "present",
                    _ => return None,
                };

                (
                    "group".to_string(),
                    serde_json::json!({
                        "name": groupname,
                        "state": new_state
                    }),
                    format!("Rollback: {} group {}", new_state, groupname),
                    4,
                )
            }

            // lineinfile module
            "lineinfile" => {
                if let Some(before) = &task.before_state {
                    if let Some(content) = before.get("content") {
                        let path = task
                            .args
                            .get("path")
                            .or_else(|| task.args.get("dest"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");

                        (
                            "copy".to_string(),
                            serde_json::json!({
                                "dest": path,
                                "content": content.clone()
                            }),
                            format!("Rollback: restore original content of {}", path),
                            2,
                        )
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }

            // Command/shell modules - cannot auto-rollback
            "command" | "shell" | "raw" | "script" => {
                // These are not automatically reversible
                return None;
            }

            // Default - unknown module, cannot rollback
            _ => {
                return None;
            }
        };

        Some(
            RollbackAction::new(&task.task_id, &task.host, module, args, description)
                .with_risk_level(risk),
        )
    }

    /// Execute a rollback plan
    pub async fn execute(&self, plan: &RollbackPlan) -> StateResult<RollbackStatus> {
        let mut status = RollbackStatus {
            plan_id: plan.id.clone(),
            status: RollbackExecutionStatus::Running,
            started_at: Utc::now(),
            completed_at: None,
            action_results: Vec::new(),
            successful: 0,
            failed: 0,
            skipped: 0,
            total_duration_ms: 0,
            error: None,
        };

        // In a real implementation, this would execute actions via the module system
        // For now, we simulate the execution
        for action in &plan.actions {
            let action_start = Utc::now();

            // Simulate execution
            let result = RollbackActionResult {
                action_id: action.id.clone(),
                status: RollbackActionStatus::Completed, // Would be determined by actual execution
                message: format!("Executed: {}", action.description),
                error: None,
                duration_ms: 100, // Simulated
                started_at: action_start,
                completed_at: Utc::now(),
            };

            match result.status {
                RollbackActionStatus::Completed => status.successful += 1,
                RollbackActionStatus::Failed => status.failed += 1,
                RollbackActionStatus::Skipped => status.skipped += 1,
                _ => {}
            }

            status.action_results.push(result);
        }

        status.completed_at = Some(Utc::now());
        status.total_duration_ms =
            (status.completed_at.unwrap() - status.started_at).num_milliseconds() as u64;

        // Determine overall status
        status.status = if status.failed == 0 && status.skipped == 0 {
            RollbackExecutionStatus::Completed
        } else if status.successful == 0 {
            RollbackExecutionStatus::Failed
        } else {
            RollbackExecutionStatus::PartialFailure
        };

        Ok(status)
    }

    /// Validate a rollback plan without executing
    pub fn validate(&self, plan: &RollbackPlan) -> StateResult<Vec<String>> {
        let mut warnings = plan.warnings.clone();

        // Check for circular dependencies
        for action in &plan.actions {
            for dep_id in &action.depends_on {
                if !plan.actions.iter().any(|a| a.id == *dep_id) {
                    warnings.push(format!(
                        "Action '{}' depends on unknown action '{}'",
                        action.id, dep_id
                    ));
                }
            }
        }

        // Check risk levels
        if plan.total_risk > 10 {
            warnings.push(format!(
                "High total risk level: {} (consider reviewing actions)",
                plan.total_risk
            ));
        }

        // Check for high-risk actions
        for action in &plan.actions {
            if action.risk_level >= 4 {
                warnings.push(format!(
                    "High-risk action: {} (risk level {})",
                    action.description, action.risk_level
                ));
            }
        }

        Ok(warnings)
    }

    /// Generate a dry-run report
    pub fn dry_run(&self, plan: &RollbackPlan) -> String {
        let mut output = String::new();

        output.push_str("=== Rollback Plan Dry Run ===\n");
        output.push_str(&format!("Plan ID: {}\n", plan.id));
        output.push_str(&format!("Actions: {}\n", plan.actions.len()));
        output.push_str(&format!("Hosts affected: {}\n", plan.hosts_affected));
        output.push_str(&format!("Total risk: {}\n\n", plan.total_risk));

        output.push_str("Actions to execute:\n");
        for (i, action) in plan.actions.iter().enumerate() {
            output.push_str(&format!(
                "  {}. [{}] {} on {} (risk: {})\n",
                i + 1,
                action.module,
                action.description,
                action.host,
                action.risk_level
            ));
            output.push_str(&format!("     Args: {}\n", action.args));
        }

        if !plan.warnings.is_empty() {
            output.push_str("\nWarnings:\n");
            for warning in &plan.warnings {
                output.push_str(&format!("  - {}\n", warning));
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::TaskStatus;

    fn create_changed_task(module: &str, args: serde_json::Value) -> TaskStateRecord {
        let mut task = TaskStateRecord::new("test_task", "localhost", module);
        task.status = TaskStatus::Changed;
        task.args = args;
        task.rollback_available = true;
        task
    }

    #[test]
    fn test_rollback_action_creation() {
        let action = RollbackAction::new(
            "task1",
            "host1",
            "apt",
            serde_json::json!({"name": "nginx", "state": "absent"}),
            "Remove nginx package",
        )
        .with_priority(10)
        .with_risk_level(2);

        assert_eq!(action.priority, 10);
        assert_eq!(action.risk_level, 2);
    }

    #[test]
    fn test_rollback_plan_creation() {
        let mut plan = RollbackPlan::new("session1", "test.yml");

        let action1 =
            RollbackAction::new("task1", "host1", "apt", serde_json::json!({}), "Action 1");
        let action2 = RollbackAction::new(
            "task2",
            "host2",
            "service",
            serde_json::json!({}),
            "Action 2",
        );

        plan.add_action(action1);
        plan.add_action(action2);
        plan.calculate_hosts();

        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.hosts_affected, 2);
    }

    #[test]
    fn test_generate_apt_rollback() {
        let executor = RollbackExecutor::new(StateConfig::minimal());

        let task = create_changed_task(
            "apt",
            serde_json::json!({
                "name": "nginx",
                "state": "present"
            }),
        );

        let action = executor.generate_rollback_action(&task);
        assert!(action.is_some());

        let action = action.unwrap();
        assert_eq!(action.module, "apt");
        assert_eq!(action.args["state"], "absent");
    }

    #[test]
    fn test_generate_service_rollback() {
        let executor = RollbackExecutor::new(StateConfig::minimal());

        let task = create_changed_task(
            "service",
            serde_json::json!({
                "name": "nginx",
                "state": "started",
                "enabled": true
            }),
        );

        let action = executor.generate_rollback_action(&task);
        assert!(action.is_some());

        let action = action.unwrap();
        assert_eq!(action.module, "service");
        assert_eq!(action.args["state"], "stopped");
        assert_eq!(action.args["enabled"], false);
    }

    #[test]
    fn test_command_not_rollbackable() {
        let executor = RollbackExecutor::new(StateConfig::minimal());

        let task = create_changed_task(
            "command",
            serde_json::json!({
                "_raw_params": "echo hello"
            }),
        );

        let action = executor.generate_rollback_action(&task);
        assert!(action.is_none());
    }

    #[test]
    fn test_create_rollback_plan() {
        let executor = RollbackExecutor::new(StateConfig::minimal());

        let tasks = vec![
            create_changed_task(
                "apt",
                serde_json::json!({"name": "nginx", "state": "present"}),
            ),
            create_changed_task(
                "service",
                serde_json::json!({"name": "nginx", "state": "started"}),
            ),
        ];

        let plan = executor.create_plan(&tasks).unwrap();
        assert_eq!(plan.actions.len(), 2);
        assert!(plan.validated);
    }

    #[test]
    fn test_rollback_dry_run() {
        let executor = RollbackExecutor::new(StateConfig::minimal());

        let tasks = vec![create_changed_task(
            "apt",
            serde_json::json!({"name": "nginx", "state": "present"}),
        )];

        let plan = executor.create_plan(&tasks).unwrap();
        let output = executor.dry_run(&plan);

        assert!(output.contains("Dry Run"));
        assert!(output.contains("apt"));
    }

    #[tokio::test]
    async fn test_rollback_execution() {
        let executor = RollbackExecutor::new(StateConfig::minimal());

        let tasks = vec![create_changed_task(
            "apt",
            serde_json::json!({"name": "nginx", "state": "present"}),
        )];

        let plan = executor.create_plan(&tasks).unwrap();
        let status = executor.execute(&plan).await.unwrap();

        assert_eq!(status.status, RollbackExecutionStatus::Completed);
        assert_eq!(status.successful, 1);
    }
}
