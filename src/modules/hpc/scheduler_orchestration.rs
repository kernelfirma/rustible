//! Scheduler orchestration for maintenance windows
//!
//! Implements the drain-operate-resume pattern for Slurm nodes:
//! 1. Drain node (stop accepting new jobs, wait for running jobs)
//! 2. Perform maintenance operation
//! 3. Resume node (mark as available)

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Node drain state during orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrainState {
    /// Node is active and accepting jobs.
    Active,
    /// Node is draining (no new jobs, waiting for current).
    Draining,
    /// Node is fully drained (no jobs running).
    Drained,
    /// Node is under maintenance.
    Maintenance,
    /// Node has been resumed.
    Resumed,
    /// Drain/resume failed.
    Failed(String),
}

/// Configuration for scheduler orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationConfig {
    /// Maximum time to wait for drain to complete.
    #[serde(with = "humantime_serde")]
    pub drain_timeout: Duration,
    /// Reason string passed to `scontrol drain`.
    pub drain_reason: String,
    /// Whether to force-drain (cancel running jobs).
    pub force_drain: bool,
    /// Poll interval for checking drain status.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            drain_timeout: Duration::from_secs(3600),
            drain_reason: "maintenance".to_string(),
            force_drain: false,
            poll_interval: Duration::from_secs(10),
        }
    }
}

/// Scheduler orchestration module.
pub struct SchedulerOrchestrationModule {
    config: OrchestrationConfig,
}

/// Result of an orchestrated operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationResult {
    /// Node name.
    pub node: String,
    /// Final state.
    pub state: DrainState,
    /// Whether the full cycle completed successfully.
    pub success: bool,
    /// Any messages or warnings.
    pub messages: Vec<String>,
}

impl SchedulerOrchestrationModule {
    /// Create a new orchestration module.
    pub fn new(config: OrchestrationConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(OrchestrationConfig::default())
    }

    /// Get the current configuration.
    pub fn config(&self) -> &OrchestrationConfig {
        &self.config
    }

    /// Generate the drain command for a node.
    pub fn drain_command(&self, node: &str) -> String {
        format!(
            "scontrol update nodename={} state=drain reason=\"{}\"",
            node, self.config.drain_reason
        )
    }

    /// Generate the resume command for a node.
    pub fn resume_command(&self, node: &str) -> String {
        format!("scontrol update nodename={} state=resume", node)
    }

    /// Generate the status check command for a node.
    pub fn status_command(&self, node: &str) -> String {
        format!("scontrol show node {} --oneliner", node)
    }

    /// Parse node state from scontrol output.
    pub fn parse_node_state(output: &str) -> DrainState {
        // Check for fully drained states first (more specific patterns)
        if output.contains("State=DRAINED") || output.contains("State=IDLE+DRAIN") {
            DrainState::Drained
        } else if output.contains("State=DRAINING") {
            DrainState::Draining
        } else if output.contains("State=IDLE")
            || output.contains("State=MIXED")
            || output.contains("State=ALLOCATED")
        {
            DrainState::Active
        } else if output.contains("State=DOWN") {
            DrainState::Maintenance
        } else {
            DrainState::Failed(format!("Unknown state in output: {}", output))
        }
    }

    /// Plan a drain-operate-resume cycle (returns commands to execute).
    pub fn plan_maintenance(&self, nodes: &[String]) -> Vec<MaintenanceStep> {
        let mut steps = Vec::new();
        for node in nodes {
            steps.push(MaintenanceStep {
                node: node.clone(),
                action: MaintenanceAction::Drain,
                command: self.drain_command(node),
            });
            steps.push(MaintenanceStep {
                node: node.clone(),
                action: MaintenanceAction::WaitDrained,
                command: self.status_command(node),
            });
            steps.push(MaintenanceStep {
                node: node.clone(),
                action: MaintenanceAction::Operate,
                command: String::new(), // user-supplied
            });
            steps.push(MaintenanceStep {
                node: node.clone(),
                action: MaintenanceAction::Resume,
                command: self.resume_command(node),
            });
        }
        steps
    }

    /// Build an orchestration result for a completed cycle.
    pub fn build_result(
        &self,
        node: &str,
        state: DrainState,
        messages: Vec<String>,
    ) -> OrchestrationResult {
        let success = matches!(state, DrainState::Resumed | DrainState::Active);
        OrchestrationResult {
            node: node.to_string(),
            state,
            success,
            messages,
        }
    }
}

/// A single step in a maintenance plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceStep {
    /// Target node.
    pub node: String,
    /// Action to perform.
    pub action: MaintenanceAction,
    /// Command to execute (empty for user-supplied operations).
    pub command: String,
}

/// Actions in a maintenance cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaintenanceAction {
    /// Drain the node.
    Drain,
    /// Wait until drained.
    WaitDrained,
    /// Perform maintenance.
    Operate,
    /// Resume the node.
    Resume,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = OrchestrationConfig::default();
        assert_eq!(config.drain_timeout, Duration::from_secs(3600));
        assert_eq!(config.drain_reason, "maintenance");
        assert!(!config.force_drain);
        assert_eq!(config.poll_interval, Duration::from_secs(10));
    }

    #[test]
    fn test_drain_command() {
        let module = SchedulerOrchestrationModule::with_defaults();
        let cmd = module.drain_command("node01");
        assert!(cmd.contains("nodename=node01"));
        assert!(cmd.contains("state=drain"));
        assert!(cmd.contains("reason=\"maintenance\""));
    }

    #[test]
    fn test_drain_command_custom_reason() {
        let module = SchedulerOrchestrationModule::new(OrchestrationConfig {
            drain_reason: "firmware update".to_string(),
            ..Default::default()
        });
        let cmd = module.drain_command("gpu-node01");
        assert!(cmd.contains("nodename=gpu-node01"));
        assert!(cmd.contains("reason=\"firmware update\""));
    }

    #[test]
    fn test_resume_command() {
        let module = SchedulerOrchestrationModule::with_defaults();
        let cmd = module.resume_command("node01");
        assert!(cmd.contains("nodename=node01"));
        assert!(cmd.contains("state=resume"));
    }

    #[test]
    fn test_status_command() {
        let module = SchedulerOrchestrationModule::with_defaults();
        let cmd = module.status_command("node01");
        assert!(cmd.contains("scontrol show node node01"));
        assert!(cmd.contains("--oneliner"));
    }

    #[test]
    fn test_parse_active_idle() {
        assert_eq!(
            SchedulerOrchestrationModule::parse_node_state("State=IDLE CPUs=32"),
            DrainState::Active
        );
    }

    #[test]
    fn test_parse_active_mixed() {
        assert_eq!(
            SchedulerOrchestrationModule::parse_node_state("State=MIXED CPUs=32"),
            DrainState::Active
        );
    }

    #[test]
    fn test_parse_active_allocated() {
        assert_eq!(
            SchedulerOrchestrationModule::parse_node_state("State=ALLOCATED CPUs=32"),
            DrainState::Active
        );
    }

    #[test]
    fn test_parse_draining() {
        assert_eq!(
            SchedulerOrchestrationModule::parse_node_state("State=DRAINING Reason=maint"),
            DrainState::Draining
        );
    }

    #[test]
    fn test_parse_drained() {
        assert_eq!(
            SchedulerOrchestrationModule::parse_node_state("State=IDLE+DRAIN Reason=maint"),
            DrainState::Drained
        );
    }

    #[test]
    fn test_parse_drained_explicit() {
        assert_eq!(
            SchedulerOrchestrationModule::parse_node_state("State=DRAINED Reason=maint"),
            DrainState::Drained
        );
    }

    #[test]
    fn test_parse_down() {
        assert_eq!(
            SchedulerOrchestrationModule::parse_node_state("State=DOWN Reason=maint"),
            DrainState::Maintenance
        );
    }

    #[test]
    fn test_parse_unknown() {
        let state = SchedulerOrchestrationModule::parse_node_state("State=RESERVED");
        assert!(matches!(state, DrainState::Failed(_)));
    }

    #[test]
    fn test_plan_maintenance_single_node() {
        let module = SchedulerOrchestrationModule::with_defaults();
        let steps = module.plan_maintenance(&["node01".into()]);
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].action, MaintenanceAction::Drain);
        assert_eq!(steps[1].action, MaintenanceAction::WaitDrained);
        assert_eq!(steps[2].action, MaintenanceAction::Operate);
        assert!(steps[2].command.is_empty());
        assert_eq!(steps[3].action, MaintenanceAction::Resume);
    }

    #[test]
    fn test_plan_maintenance_multiple_nodes() {
        let module = SchedulerOrchestrationModule::with_defaults();
        let steps = module.plan_maintenance(&["node01".into(), "node02".into()]);
        assert_eq!(steps.len(), 8); // 4 steps per node
        assert_eq!(steps[0].node, "node01");
        assert_eq!(steps[0].action, MaintenanceAction::Drain);
        assert_eq!(steps[3].node, "node01");
        assert_eq!(steps[3].action, MaintenanceAction::Resume);
        assert_eq!(steps[4].node, "node02");
        assert_eq!(steps[4].action, MaintenanceAction::Drain);
    }

    #[test]
    fn test_build_result_success() {
        let module = SchedulerOrchestrationModule::with_defaults();
        let result = module.build_result("node01", DrainState::Resumed, vec![]);
        assert!(result.success);
        assert_eq!(result.node, "node01");
    }

    #[test]
    fn test_build_result_failure() {
        let module = SchedulerOrchestrationModule::with_defaults();
        let result = module.build_result(
            "node01",
            DrainState::Failed("timeout".into()),
            vec!["drain timed out".into()],
        );
        assert!(!result.success);
        assert_eq!(result.messages.len(), 1);
    }
}
