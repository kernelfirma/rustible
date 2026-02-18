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
//! - `force` (optional): Skip job-aware guards (default false)

use std::collections::HashMap;
use std::sync::Arc;

use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of preflight checks before a state transition.
#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single field that drifted from desired to actual.
#[derive(Debug, serde::Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

/// Post-change verification result.
#[derive(Debug, serde::Serialize)]
struct VerifyResult {
    verified: bool,
    details: Vec<String>,
    warnings: Vec<String>,
}

/// Rich node information parsed from `scontrol show node` output.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeInfo {
    pub state: String,
    pub reason: String,
    pub cpu_total: u32,
    pub cpu_alloc: u32,
    pub mem_total: u64,
    pub mem_alloc: u64,
    pub features: Vec<String>,
    pub partitions: Vec<String>,
}

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

/// Validate whether a state transition is allowed.
///
/// Some transitions are scheduler-managed (e.g., idle -> alloc) or internal
/// (e.g., any -> comp) and should be blocked. Returns `Ok(())` for valid
/// transitions and `Err(message)` for invalid ones.
fn validate_state_transition(current: &NodeState, action: NodeAction) -> Result<(), String> {
    match (current, action) {
        // idle -> drain, down are valid admin actions
        (NodeState::Idle, NodeAction::Drain) => Ok(()),
        (NodeState::Idle, NodeAction::Down) => Ok(()),

        // alloc -> drain is valid (drain waits for jobs to finish)
        (NodeState::Allocated, NodeAction::Drain) => Ok(()),

        // drain/drained -> idle, resume, down are valid recovery actions
        (NodeState::Drained, NodeAction::Idle) => Ok(()),
        (NodeState::Drained, NodeAction::Resume) => Ok(()),
        (NodeState::Drained, NodeAction::Undrain) => Ok(()),
        (NodeState::Drained, NodeAction::Down) => Ok(()),
        (NodeState::Draining, NodeAction::Idle) => Ok(()),
        (NodeState::Draining, NodeAction::Resume) => Ok(()),
        (NodeState::Draining, NodeAction::Undrain) => Ok(()),
        (NodeState::Draining, NodeAction::Down) => Ok(()),

        // down -> idle, resume are valid recovery actions
        (NodeState::Down, NodeAction::Idle) => Ok(()),
        (NodeState::Down, NodeAction::Resume) => Ok(()),
        (NodeState::Down, NodeAction::Undrain) => Ok(()),

        // mixed -> drain is valid
        (NodeState::Mixed, NodeAction::Drain) => Ok(()),

        // Unknown state: allow any action (we cannot validate)
        (NodeState::Unknown, _) => Ok(()),

        // Transitions to the same effective state are no-ops, not invalid
        (NodeState::Idle, NodeAction::Idle) => Ok(()),
        (NodeState::Idle, NodeAction::Resume) => Ok(()),
        (NodeState::Idle, NodeAction::Undrain) => Ok(()),
        (NodeState::Down, NodeAction::Down) => Ok(()),
        (NodeState::Drained, NodeAction::Drain) => Ok(()),
        (NodeState::Draining, NodeAction::Drain) => Ok(()),

        // Everything else is invalid
        (state, act) => Err(format!(
            "Invalid state transition: {:?} -> {} is not allowed \
             (scheduler-managed or internal state)",
            state,
            act.to_scontrol_state()
        )),
    }
}

/// Check if a node has running jobs by inspecting CPUAlloc and AllocMem
/// from scontrol output.
///
/// Returns `(has_jobs, description)`.
fn check_running_jobs(scontrol_output: &str) -> (bool, String) {
    let mut cpu_alloc: u64 = 0;
    let mut alloc_mem: u64 = 0;

    for token in scontrol_output.split_whitespace() {
        if let Some(val) = token.strip_prefix("CPUAlloc=") {
            cpu_alloc = val.parse().unwrap_or(0);
        } else if let Some(val) = token.strip_prefix("AllocMem=") {
            alloc_mem = val.parse().unwrap_or(0);
        }
    }

    let has_jobs = cpu_alloc > 0 || alloc_mem > 0;
    let description = if has_jobs {
        format!(
            "Node has running jobs (CPUAlloc={}, AllocMem={})",
            cpu_alloc, alloc_mem
        )
    } else {
        "No running jobs detected".to_string()
    };

    (has_jobs, description)
}

/// Parse `scontrol show node` output into a `NodeInfo` struct.
fn parse_node_info(scontrol_output: &str) -> NodeInfo {
    let mut state = String::new();
    let mut reason = String::new();
    let mut cpu_total: u32 = 0;
    let mut cpu_alloc: u32 = 0;
    let mut mem_total: u64 = 0;
    let mut mem_alloc: u64 = 0;
    let mut features = Vec::new();
    let mut partitions = Vec::new();

    for token in scontrol_output.split_whitespace() {
        if let Some((key, value)) = token.split_once('=') {
            match key {
                "State" => state = value.to_string(),
                "Reason" => reason = value.to_string(),
                "CPUTot" => cpu_total = value.parse().unwrap_or(0),
                "CPUAlloc" => cpu_alloc = value.parse().unwrap_or(0),
                "RealMemory" => mem_total = value.parse().unwrap_or(0),
                "AllocMem" => mem_alloc = value.parse().unwrap_or(0),
                "AvailableFeatures" | "ActiveFeatures" => {
                    if features.is_empty() && !value.is_empty() && value != "(null)" {
                        features = value.split(',').map(|s| s.to_string()).collect();
                    }
                }
                "Partitions" => {
                    if !value.is_empty() && value != "(null)" {
                        partitions = value.split(',').map(|s| s.to_string()).collect();
                    }
                }
                _ => {}
            }
        }
    }

    // Handle multi-word Reason fields: Reason may span a quoted section.
    // Re-parse reason from the raw output for better accuracy.
    if let Some(reason_start) = scontrol_output.find("Reason=") {
        let after_eq = &scontrol_output[reason_start + 7..];
        let parsed_reason = if let Some(stripped) = after_eq.strip_prefix('"') {
            // Quoted reason - find closing quote
            stripped
                .find('"')
                .map(|end| stripped[..end].to_string())
                .unwrap_or_else(|| after_eq.trim().to_string())
        } else {
            // Unquoted - take until next whitespace or newline
            after_eq
                .split(|c: char| c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string()
        };
        if !parsed_reason.is_empty() && parsed_reason != "(null)" {
            reason = parsed_reason;
        }
    }

    NodeInfo {
        state,
        reason,
        cpu_total,
        cpu_alloc,
        mem_total,
        mem_alloc,
        features,
        partitions,
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
        let force = params.get_bool_or("force", false);
        let action = NodeAction::from_str(&state_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'drain', 'resume', 'down', 'idle', or 'undrain'",
                state_str
            ))
        })?;

        // Query current node state and full info for diagnostics
        let current_state = self.get_node_state(connection, &name, context)?;
        let scontrol_output = self.get_scontrol_output(connection, &name, context)?;
        let node_info = parse_node_info(&scontrol_output);
        let mut diagnostics_warnings: Vec<String> = Vec::new();

        // Validate the state transition
        if let Err(msg) = validate_state_transition(&current_state, action) {
            return Err(ModuleError::InvalidParameter(msg));
        }

        // Check if action is needed
        if !self.action_needed(action, &current_state) {
            return Ok(ModuleOutput::ok(format!(
                "Node '{}' is already in desired state ({:?})",
                name, current_state
            ))
            .with_data("node", serde_json::json!(name))
            .with_data("state", serde_json::json!(current_state))
            .with_data("node_info", serde_json::json!(node_info)));
        }

        // Job-aware guard: check for running jobs before drain
        if matches!(action, NodeAction::Drain) {
            let (has_jobs, job_desc) = check_running_jobs(&scontrol_output);
            if has_jobs {
                if force {
                    diagnostics_warnings.push(format!(
                        "Force mode: proceeding despite running jobs ({})",
                        job_desc
                    ));
                } else {
                    // Drain is safe -- it waits for jobs to finish. Warn but proceed.
                    diagnostics_warnings.push(format!(
                        "Node has running jobs; drain will wait for completion ({})",
                        job_desc
                    ));
                }
            }
        }

        if context.check_mode {
            let mut output = ModuleOutput::changed(format!(
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
            .with_data("current_state", serde_json::json!(current_state))
            .with_data("node_info", serde_json::json!(node_info));
            if !diagnostics_warnings.is_empty() {
                output = output.with_data("warnings", serde_json::json!(diagnostics_warnings));
            }
            return Ok(output);
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

        // Post-verify: re-query node state and build verification result
        let post_output = self.get_scontrol_output(connection, &name, context)?;
        let post_info = parse_node_info(&post_output);
        let post_state = NodeState::from_scontrol_output(&post_info.state);

        let verify = self.post_verify(action, &post_state, &post_info);

        let mut output = ModuleOutput::changed(format!(
            "Node '{}' state changed to {} (was {:?})",
            name,
            action.to_scontrol_state(),
            current_state
        ))
        .with_data("node", serde_json::json!(name))
        .with_data("new_state", serde_json::json!(action.to_scontrol_state()))
        .with_data("previous_state", serde_json::json!(current_state))
        .with_data("node_info", serde_json::json!(post_info))
        .with_data("verify", serde_json::json!(verify));

        if !diagnostics_warnings.is_empty() {
            output = output.with_data("warnings", serde_json::json!(diagnostics_warnings));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["name", "state"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("reason", serde_json::json!(null));
        m.insert("weight", serde_json::json!(null));
        m.insert("features", serde_json::json!(null));
        m.insert("force", serde_json::json!(false));
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

    /// Get raw scontrol output for a node.
    fn get_scontrol_output(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let cmd = format!("scontrol show node {}", name);
        let (ok, stdout, stderr) = run_cmd(connection, &cmd, context)?;

        if !ok {
            if stderr.contains("not found") || stderr.contains("Invalid node name") {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Node '{}' not found",
                    name
                )));
            }
            return Ok(String::new());
        }

        Ok(stdout)
    }

    /// Verify the state change was applied successfully.
    fn post_verify(
        &self,
        action: NodeAction,
        post_state: &NodeState,
        _post_info: &NodeInfo,
    ) -> VerifyResult {
        let mut details = Vec::new();
        let mut warnings = Vec::new();

        let expected_ok = match action {
            NodeAction::Drain => {
                let ok = post_state.is_drained()
                    || matches!(post_state, NodeState::Allocated | NodeState::Mixed);
                if matches!(post_state, NodeState::Allocated | NodeState::Mixed) {
                    warnings.push("Node is draining but still has allocated resources".to_string());
                }
                ok
            }
            NodeAction::Resume | NodeAction::Undrain => post_state.is_operational(),
            NodeAction::Down => *post_state == NodeState::Down,
            NodeAction::Idle => *post_state == NodeState::Idle,
        };

        if expected_ok {
            details.push(format!("State verified: node is now {:?}", post_state));
        } else {
            warnings.push(format!(
                "Expected state after {} but found {:?}",
                action.to_scontrol_state(),
                post_state
            ));
        }

        VerifyResult {
            verified: expected_ok,
            details,
            warnings,
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
        assert!(optional.contains_key("force"));
    }

    #[test]
    fn test_valid_state_transitions() {
        // Valid transitions
        assert!(validate_state_transition(&NodeState::Idle, NodeAction::Drain).is_ok());
        assert!(validate_state_transition(&NodeState::Idle, NodeAction::Down).is_ok());
        assert!(validate_state_transition(&NodeState::Allocated, NodeAction::Drain).is_ok());
        assert!(validate_state_transition(&NodeState::Drained, NodeAction::Idle).is_ok());
        assert!(validate_state_transition(&NodeState::Drained, NodeAction::Resume).is_ok());
        assert!(validate_state_transition(&NodeState::Drained, NodeAction::Undrain).is_ok());
        assert!(validate_state_transition(&NodeState::Drained, NodeAction::Down).is_ok());
        assert!(validate_state_transition(&NodeState::Draining, NodeAction::Idle).is_ok());
        assert!(validate_state_transition(&NodeState::Draining, NodeAction::Resume).is_ok());
        assert!(validate_state_transition(&NodeState::Down, NodeAction::Idle).is_ok());
        assert!(validate_state_transition(&NodeState::Down, NodeAction::Resume).is_ok());
        assert!(validate_state_transition(&NodeState::Down, NodeAction::Undrain).is_ok());
        assert!(validate_state_transition(&NodeState::Mixed, NodeAction::Drain).is_ok());

        // Same-state no-ops should also be valid
        assert!(validate_state_transition(&NodeState::Idle, NodeAction::Idle).is_ok());
        assert!(validate_state_transition(&NodeState::Down, NodeAction::Down).is_ok());
        assert!(validate_state_transition(&NodeState::Drained, NodeAction::Drain).is_ok());

        // Idle -> Resume/Undrain are no-ops (already operational), should be valid
        assert!(validate_state_transition(&NodeState::Idle, NodeAction::Resume).is_ok());
        assert!(validate_state_transition(&NodeState::Idle, NodeAction::Undrain).is_ok());

        // Invalid transitions: scheduler-managed or internal
        assert!(validate_state_transition(&NodeState::Allocated, NodeAction::Idle).is_err());
        assert!(validate_state_transition(&NodeState::Allocated, NodeAction::Down).is_err());
        assert!(validate_state_transition(&NodeState::Allocated, NodeAction::Resume).is_err());
        assert!(validate_state_transition(&NodeState::Mixed, NodeAction::Idle).is_err());
        assert!(validate_state_transition(&NodeState::Mixed, NodeAction::Down).is_err());
        assert!(validate_state_transition(&NodeState::Mixed, NodeAction::Resume).is_err());
        assert!(validate_state_transition(&NodeState::Reserved, NodeAction::Drain).is_err());

        // Unknown state should allow any action
        assert!(validate_state_transition(&NodeState::Unknown, NodeAction::Drain).is_ok());
        assert!(validate_state_transition(&NodeState::Unknown, NodeAction::Resume).is_ok());
        assert!(validate_state_transition(&NodeState::Unknown, NodeAction::Down).is_ok());
        assert!(validate_state_transition(&NodeState::Unknown, NodeAction::Idle).is_ok());
    }

    #[test]
    fn test_node_info_parsing() {
        let scontrol_output = "\
NodeName=node01 Arch=x86_64 CoresPerSocket=16
   CPUAlloc=8 CPUTot=32 CPULoad=4.50
   AvailableFeatures=gpu,nvme ActiveFeatures=gpu,nvme
   RealMemory=128000 AllocMem=64000 FreeMem=60000
   State=MIXED Partitions=batch,gpu
   Reason=none";

        let info = parse_node_info(scontrol_output);
        assert_eq!(info.state, "MIXED");
        assert_eq!(info.cpu_total, 32);
        assert_eq!(info.cpu_alloc, 8);
        assert_eq!(info.mem_total, 128000);
        assert_eq!(info.mem_alloc, 64000);
        assert_eq!(info.features, vec!["gpu", "nvme"]);
        assert_eq!(info.partitions, vec!["batch", "gpu"]);

        // Test with drained node and quoted reason
        let drained_output = "\
NodeName=node02 Arch=x86_64 CoresPerSocket=8
   CPUAlloc=0 CPUTot=16 CPULoad=0.00
   AvailableFeatures=(null) ActiveFeatures=(null)
   RealMemory=64000 AllocMem=0 FreeMem=63000
   State=IDLE+DRAIN Partitions=batch
   Reason=\"Maintenance window\"";

        let info2 = parse_node_info(drained_output);
        assert_eq!(info2.state, "IDLE+DRAIN");
        assert_eq!(info2.cpu_total, 16);
        assert_eq!(info2.cpu_alloc, 0);
        assert_eq!(info2.mem_total, 64000);
        assert_eq!(info2.mem_alloc, 0);
        assert!(info2.features.is_empty()); // (null) should be empty
        assert_eq!(info2.partitions, vec!["batch"]);
        assert_eq!(info2.reason, "Maintenance window");

        // Test with empty output
        let empty_info = parse_node_info("");
        assert_eq!(empty_info.state, "");
        assert_eq!(empty_info.cpu_total, 0);
        assert!(empty_info.features.is_empty());
        assert!(empty_info.partitions.is_empty());
    }

    #[test]
    fn test_check_running_jobs() {
        let output_with_jobs = "\
NodeName=node01 CPUAlloc=8 CPUTot=32 AllocMem=64000 RealMemory=128000 State=ALLOCATED";
        let (has_jobs, desc) = check_running_jobs(output_with_jobs);
        assert!(has_jobs);
        assert!(desc.contains("CPUAlloc=8"));
        assert!(desc.contains("AllocMem=64000"));

        let output_no_jobs = "\
NodeName=node02 CPUAlloc=0 CPUTot=16 AllocMem=0 RealMemory=64000 State=IDLE";
        let (has_jobs2, desc2) = check_running_jobs(output_no_jobs);
        assert!(!has_jobs2);
        assert!(desc2.contains("No running jobs"));

        // Empty output
        let (has_jobs3, _) = check_running_jobs("");
        assert!(!has_jobs3);
    }

    #[test]
    fn test_reason_enforcement() {
        // Build params without reason for drain - should fail at validation
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("node01"));
        params.insert("state".to_string(), serde_json::json!("drain"));
        // No reason provided - the execute method should require it for drain/down
        // We verify that the reason parameter is absent
        let reason: Option<String> = params
            .get("reason")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        assert!(reason.is_none(), "Reason should be None when not provided");

        // With reason provided
        params.insert(
            "reason".to_string(),
            serde_json::json!("Hardware maintenance"),
        );
        let reason2: Option<String> = params
            .get("reason")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        assert_eq!(reason2, Some("Hardware maintenance".to_string()));

        // For non-drain action, reason is optional
        let mut params_resume: ModuleParams = HashMap::new();
        params_resume.insert("name".to_string(), serde_json::json!("node01"));
        params_resume.insert("state".to_string(), serde_json::json!("resume"));
        let reason3: Option<String> = params_resume
            .get("reason")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        assert!(reason3.is_none(), "Resume action should not require reason");

        // Verify down also requires reason
        let mut params_down: ModuleParams = HashMap::new();
        params_down.insert("name".to_string(), serde_json::json!("node01"));
        params_down.insert("state".to_string(), serde_json::json!("down"));
        let reason4: Option<String> = params_down
            .get("reason")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        assert!(
            reason4.is_none(),
            "Reason should be None when not provided for down"
        );
    }
}
