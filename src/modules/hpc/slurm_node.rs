//! Slurm node management module
//!
//! Manage Slurm compute node states via scontrol.
//!
//! # Parameters
//!
//! - `name` (required): Node name
//! - `state` (required): Desired state - "drain", "resume", "down", "idle", "undrain"
//! - `reason` (optional): Reason for state change (required for drain/down)
//! - `weight` (optional): Node scheduling weight
//! - `features` (optional): Node features/attributes

use std::collections::HashMap;
use std::sync::Arc;

use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
    let mut options = ExecuteOptions::new();
    if context.r#become {
        options = options.with_escalation(context.become_user.clone());
        if let Some(ref method) = context.become_method {
            options.escalate_method = Some(method.clone());
        }
        if let Some(ref password) = context.become_password {
            options.escalate_password = Some(password.clone());
        }
    }
    options
}

fn run_cmd(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<(bool, String, String)> {
    let options = get_exec_options(context);
    let result = Handle::current()
        .block_on(async { connection.execute(cmd, Some(options)).await })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;
    Ok((result.success, result.stdout, result.stderr))
}

fn run_cmd_ok(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<String> {
    let (success, stdout, stderr) = run_cmd(connection, cmd, context)?;
    if !success {
        return Err(ModuleError::ExecutionFailed(format!(
            "Command failed: {}",
            stderr.trim()
        )));
    }
    Ok(stdout)
}

/// Node state as reported by Slurm.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
    Idle,
    Allocated,
    Mixed,
    Down,
    Drained,
    Draining,
    Reserved,
    Unknown,
}

impl NodeState {
    /// Parse node state from scontrol output.
    /// Slurm state may include flags like IDLE+DRAIN, ALLOCATED+DRAIN, etc.
    pub fn from_scontrol_output(state_str: &str) -> NodeState {
        let lower = state_str.to_lowercase();

        // Check for drain states first (can be combined with other states)
        if lower.contains("drain") && !lower.contains("draining") {
            return NodeState::Drained;
        }
        if lower.contains("draining") {
            return NodeState::Draining;
        }
        if lower.contains("down") {
            return NodeState::Down;
        }
        if lower.contains("reserved") {
            return NodeState::Reserved;
        }
        if lower.contains("mixed") {
            return NodeState::Mixed;
        }
        if lower.contains("allocated") || lower.contains("alloc") {
            return NodeState::Allocated;
        }
        if lower.contains("idle") {
            return NodeState::Idle;
        }

        NodeState::Unknown
    }

    /// Check if the node is in a drained state (fully or draining).
    pub fn is_drained(&self) -> bool {
        matches!(self, NodeState::Drained | NodeState::Draining)
    }

    /// Check if the node is operational (not down, not drained).
    pub fn is_operational(&self) -> bool {
        matches!(
            self,
            NodeState::Idle | NodeState::Allocated | NodeState::Mixed | NodeState::Reserved
        )
    }
}

/// Desired node state action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeAction {
    Drain,
    Resume,
    Down,
    Idle,
    Undrain,
}

impl NodeAction {
    pub fn from_str(s: &str) -> Option<NodeAction> {
        match s.to_lowercase().as_str() {
            "drain" => Some(NodeAction::Drain),
            "resume" => Some(NodeAction::Resume),
            "down" => Some(NodeAction::Down),
            "idle" => Some(NodeAction::Idle),
            "undrain" => Some(NodeAction::Undrain),
            _ => None,
        }
    }

    /// Convert to scontrol state parameter value.
    pub fn to_scontrol_state(&self) -> &'static str {
        match self {
            NodeAction::Drain => "DRAIN",
            NodeAction::Resume => "RESUME",
            NodeAction::Down => "DOWN",
            NodeAction::Idle => "IDLE",
            NodeAction::Undrain => "UNDRAIN",
        }
    }
}

pub struct SlurmNodeModule;

impl Module for SlurmNodeModule {
    fn name(&self) -> &'static str {
        "slurm_node"
    }

    fn description(&self) -> &'static str {
        "Manage Slurm node state (drain/resume/down/idle/undrain)"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::GlobalExclusive
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let name = params.get_string_required("name")?;
        let state_str = params.get_string_required("state")?;
        let action = NodeAction::from_str(&state_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'drain', 'resume', 'down', 'idle', or 'undrain'",
                state_str
            ))
        })?;

        // Query current node state for idempotency check
        let current_state = self.get_node_state(connection, &name, context)?;

        // Check if action is needed
        if !self.action_needed(action, &current_state) {
            return Ok(ModuleOutput::ok(format!(
                "Node '{}' is already in desired state ({:?})",
                name, current_state
            ))
            .with_data("node", serde_json::json!(name))
            .with_data("state", serde_json::json!(current_state)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would set node '{}' to state {} (currently {:?})",
                name,
                action.to_scontrol_state(),
                current_state
            ))
            .with_data("node", serde_json::json!(name))
            .with_data(
                "desired_state",
                serde_json::json!(action.to_scontrol_state()),
            )
            .with_data("current_state", serde_json::json!(current_state)));
        }

        // Build and execute scontrol update command
        let mut cmd_parts = vec![
            format!("scontrol update NodeName={}", name),
            format!("State={}", action.to_scontrol_state()),
        ];

        // Add reason if provided (required for drain/down)
        if let Some(reason) = params.get_string("reason")? {
            cmd_parts.push(format!("Reason=\"{}\"", reason.replace('"', "\\\"")));
        } else if matches!(action, NodeAction::Drain | NodeAction::Down) {
            return Err(ModuleError::InvalidParameter(
                "Parameter 'reason' is required for drain/down actions".to_string(),
            ));
        }

        // Add weight if provided
        if let Some(weight) = params.get_string("weight")? {
            cmd_parts.push(format!("Weight={}", weight));
        }

        // Add features if provided
        if let Some(features) = params.get_string("features")? {
            cmd_parts.push(format!("Features={}", features));
        }

        let cmd = cmd_parts.join(" ");
        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!(
            "Node '{}' state changed to {} (was {:?})",
            name,
            action.to_scontrol_state(),
            current_state
        ))
        .with_data("node", serde_json::json!(name))
        .with_data("new_state", serde_json::json!(action.to_scontrol_state()))
        .with_data("previous_state", serde_json::json!(current_state)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["name", "state"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("reason", serde_json::json!(null));
        m.insert("weight", serde_json::json!(null));
        m.insert("features", serde_json::json!(null));
        m
    }
}

impl SlurmNodeModule {
    /// Query the current state of a node using scontrol.
    fn get_node_state(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<NodeState> {
        let cmd = format!("scontrol show node {}", name);
        let (ok, stdout, stderr) = run_cmd(connection, &cmd, context)?;

        if !ok {
            // If node doesn't exist or query fails, return Unknown
            return if stderr.contains("not found") || stderr.contains("Invalid node name") {
                Err(ModuleError::ExecutionFailed(format!(
                    "Node '{}' not found",
                    name
                )))
            } else {
                Ok(NodeState::Unknown)
            };
        }

        // Parse state from scontrol output
        // Format: "State=IDLE+DRAIN" or "State=ALLOCATED" etc.
        for line in stdout.lines() {
            if let Some(state_value) = line
                .strip_prefix("   State=")
                .or_else(|| line.strip_prefix("State="))
            {
                // Extract just the state part (before any space or other field)
                let state_str = state_value.split_whitespace().next().unwrap_or("");
                return Ok(NodeState::from_scontrol_output(state_str));
            }
        }

        Ok(NodeState::Unknown)
    }

    /// Determine if an action is needed based on current state.
    fn action_needed(&self, action: NodeAction, current: &NodeState) -> bool {
        match action {
            NodeAction::Drain => !current.is_drained(),
            NodeAction::Undrain | NodeAction::Resume => {
                current.is_drained() || !current.is_operational()
            }
            NodeAction::Down => *current != NodeState::Down,
            NodeAction::Idle => *current != NodeState::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_action_from_str() {
        assert_eq!(NodeAction::from_str("drain"), Some(NodeAction::Drain));
        assert_eq!(NodeAction::from_str("RESUME"), Some(NodeAction::Resume));
        assert_eq!(NodeAction::from_str("Down"), Some(NodeAction::Down));
        assert_eq!(NodeAction::from_str("idle"), Some(NodeAction::Idle));
        assert_eq!(NodeAction::from_str("undrain"), Some(NodeAction::Undrain));
        assert_eq!(NodeAction::from_str("invalid"), None);
        assert_eq!(NodeAction::from_str(""), None);
    }

    #[test]
    fn test_node_action_to_scontrol_state() {
        assert_eq!(NodeAction::Drain.to_scontrol_state(), "DRAIN");
        assert_eq!(NodeAction::Resume.to_scontrol_state(), "RESUME");
        assert_eq!(NodeAction::Down.to_scontrol_state(), "DOWN");
        assert_eq!(NodeAction::Idle.to_scontrol_state(), "IDLE");
        assert_eq!(NodeAction::Undrain.to_scontrol_state(), "UNDRAIN");
    }

    #[test]
    fn test_node_state_from_scontrol_output() {
        assert_eq!(NodeState::from_scontrol_output("IDLE"), NodeState::Idle);
        assert_eq!(
            NodeState::from_scontrol_output("ALLOCATED"),
            NodeState::Allocated
        );
        assert_eq!(NodeState::from_scontrol_output("MIXED"), NodeState::Mixed);
        assert_eq!(NodeState::from_scontrol_output("DOWN"), NodeState::Down);
        assert_eq!(
            NodeState::from_scontrol_output("DRAINED"),
            NodeState::Drained
        );
        assert_eq!(
            NodeState::from_scontrol_output("DRAINING"),
            NodeState::Draining
        );
        assert_eq!(
            NodeState::from_scontrol_output("IDLE+DRAIN"),
            NodeState::Drained
        );
        assert_eq!(
            NodeState::from_scontrol_output("ALLOCATED+DRAIN"),
            NodeState::Drained
        );
        assert_eq!(NodeState::from_scontrol_output("DOWN*"), NodeState::Down);
        assert_eq!(
            NodeState::from_scontrol_output("RESERVED"),
            NodeState::Reserved
        );
        assert_eq!(
            NodeState::from_scontrol_output("UNKNOWN_STATE"),
            NodeState::Unknown
        );
    }

    #[test]
    fn test_node_state_is_drained() {
        assert!(NodeState::Drained.is_drained());
        assert!(NodeState::Draining.is_drained());
        assert!(!NodeState::Idle.is_drained());
        assert!(!NodeState::Allocated.is_drained());
        assert!(!NodeState::Down.is_drained());
    }

    #[test]
    fn test_node_state_is_operational() {
        assert!(NodeState::Idle.is_operational());
        assert!(NodeState::Allocated.is_operational());
        assert!(NodeState::Mixed.is_operational());
        assert!(NodeState::Reserved.is_operational());
        assert!(!NodeState::Drained.is_operational());
        assert!(!NodeState::Draining.is_operational());
        assert!(!NodeState::Down.is_operational());
        assert!(!NodeState::Unknown.is_operational());
    }

    #[test]
    fn test_action_needed_drain() {
        let module = SlurmNodeModule;

        // Should drain if not already drained
        assert!(module.action_needed(NodeAction::Drain, &NodeState::Idle));
        assert!(module.action_needed(NodeAction::Drain, &NodeState::Allocated));

        // Should not drain if already drained
        assert!(!module.action_needed(NodeAction::Drain, &NodeState::Drained));
        assert!(!module.action_needed(NodeAction::Drain, &NodeState::Draining));
    }

    #[test]
    fn test_action_needed_resume() {
        let module = SlurmNodeModule;

        // Should resume if drained
        assert!(module.action_needed(NodeAction::Resume, &NodeState::Drained));
        assert!(module.action_needed(NodeAction::Resume, &NodeState::Draining));

        // Should resume if down
        assert!(module.action_needed(NodeAction::Resume, &NodeState::Down));

        // Should not resume if operational
        assert!(!module.action_needed(NodeAction::Resume, &NodeState::Idle));
    }

    #[test]
    fn test_action_needed_down() {
        let module = SlurmNodeModule;

        // Should set down if not already down
        assert!(module.action_needed(NodeAction::Down, &NodeState::Idle));
        assert!(module.action_needed(NodeAction::Down, &NodeState::Drained));

        // Should not set down if already down
        assert!(!module.action_needed(NodeAction::Down, &NodeState::Down));
    }

    #[test]
    fn test_action_needed_idle() {
        let module = SlurmNodeModule;

        // Should set idle if not idle
        assert!(module.action_needed(NodeAction::Idle, &NodeState::Allocated));
        assert!(module.action_needed(NodeAction::Idle, &NodeState::Down));

        // Should not set idle if already idle
        assert!(!module.action_needed(NodeAction::Idle, &NodeState::Idle));
    }

    #[test]
    fn test_action_needed_undrain() {
        let module = SlurmNodeModule;

        // Same logic as resume
        assert!(module.action_needed(NodeAction::Undrain, &NodeState::Drained));
        assert!(!module.action_needed(NodeAction::Undrain, &NodeState::Idle));
    }

    #[test]
    fn test_module_name_and_description() {
        let module = SlurmNodeModule;
        assert_eq!(module.name(), "slurm_node");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_module_required_params() {
        let module = SlurmNodeModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
        assert!(required.contains(&"state"));
    }

    #[test]
    fn test_module_optional_params() {
        let module = SlurmNodeModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("reason"));
        assert!(optional.contains_key("weight"));
        assert!(optional.contains_key("features"));
    }
}
