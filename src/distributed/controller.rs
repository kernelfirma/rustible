//! Controller node implementation for distributed execution
//!
//! The Controller is the main entry point for a distributed Rustible node.
//! It manages:
//! - Raft consensus participation for leader election
//! - Work unit execution and reporting
//! - Peer communication
//! - Health monitoring

use super::cluster::ClusterManager;
use super::raft::{RaftNode, RaftState};
use super::types::{
    ClusterConfig, ControllerHealth, ControllerId, ControllerInfo, ControllerLoad, ControllerRole,
    TaskSpec, WorkUnit, WorkUnitId, WorkUnitState,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tokio::sync::{mpsc, RwLock};

/// Controller node for distributed execution
pub struct Controller {
    /// Controller configuration
    config: ClusterConfig,
    /// Raft consensus state
    raft_state: Arc<RaftState>,
    /// Cluster manager for peer communication
    cluster: Arc<ClusterManager>,
    /// Active work units being executed
    work_units: RwLock<HashMap<WorkUnitId, WorkUnitExecution>>,
    /// Event channel for internal events
    event_tx: mpsc::Sender<ControllerEvent>,
    /// Shutdown signal
    shutdown_tx: mpsc::Sender<()>,
}

/// Work unit execution state
struct WorkUnitExecution {
    /// The work unit being executed
    work_unit: WorkUnit,
    /// Current execution state
    state: WorkUnitState,
    /// Start time
    started_at: Instant,
    /// Task results (task index -> result)
    task_results: HashMap<usize, TaskResult>,
}

/// Task execution result
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// Whether the task succeeded
    pub success: bool,
    /// Whether the task made changes
    pub changed: bool,
    /// Task output/result data
    pub result: serde_json::Value,
    /// Error message if failed
    pub error: Option<String>,
}

/// Controller internal events
#[derive(Debug)]
enum ControllerEvent {
    /// Work unit assigned by leader
    WorkUnitAssigned(WorkUnit),
    /// Work unit completed
    WorkUnitCompleted {
        id: WorkUnitId,
        results: HashMap<usize, TaskResult>,
    },
    /// Work unit failed
    WorkUnitFailed { id: WorkUnitId, error: String },
    /// Peer message received
    PeerMessage(super::cluster::PeerMessage),
    /// Health check request
    HealthCheck,
    /// Shutdown requested
    Shutdown,
}

impl Controller {
    /// Create a new controller node
    pub async fn new(config: ClusterConfig) -> Result<Self, ControllerError> {
        let (event_tx, _event_rx) = mpsc::channel(100);
        let (shutdown_tx, _shutdown_rx) = mpsc::channel(1);

        let raft_node = RaftNode::new(config.clone());
        let raft_state = raft_node.state();

        let cluster = Arc::new(ClusterManager::new(config.clone()).await?);

        Ok(Self {
            config,
            raft_state,
            cluster,
            work_units: RwLock::new(HashMap::new()),
            event_tx,
            shutdown_tx,
        })
    }

    /// Get the controller's ID
    pub fn id(&self) -> &ControllerId {
        &self.config.controller_id
    }

    /// Get the current role (Leader, Follower, Candidate)
    pub async fn role(&self) -> ControllerRole {
        self.raft_state.role().await
    }

    /// Check if this controller is the leader
    pub async fn is_leader(&self) -> bool {
        self.raft_state.is_leader().await
    }

    /// Get the current leader's ID (if known)
    pub async fn leader_id(&self) -> Option<ControllerId> {
        self.raft_state.leader_id().await
    }

    /// Get controller info for reporting
    pub async fn info(&self) -> ControllerInfo {
        ControllerInfo {
            id: self.config.controller_id.clone(),
            address: self.config.bind_address,
            region: self.config.region.clone(),
            capabilities: vec!["execute".to_string(), "coordinate".to_string()],
            capacity: self.config.capacity,
            health: ControllerHealth::Healthy,
            last_heartbeat: Some(std::time::Instant::now()),
        }
    }

    /// Get current load metrics
    pub async fn load(&self) -> ControllerLoad {
        let work_units = self.work_units.read().await;

        let active_work_units = work_units
            .values()
            .filter(|wu| matches!(wu.state, WorkUnitState::Running))
            .count() as u32;

        ControllerLoad {
            active_work_units,
            active_connections: 0,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            bandwidth_usage: 0.0,
            avg_latency_ms: 0,
            queue_depth: 0,
            capacity: self.config.capacity,
        }
    }

    /// Start the controller
    pub async fn start(&self) -> Result<(), ControllerError> {
        tracing::info!(
            "Starting controller {} on {}",
            self.config.controller_id,
            self.config.bind_address
        );

        // Start cluster manager
        self.cluster.start().await?;

        tracing::info!("Controller {} started", self.config.controller_id);
        Ok(())
    }

    /// Stop the controller gracefully
    pub async fn stop(&self) -> Result<(), ControllerError> {
        tracing::info!("Stopping controller {}", self.config.controller_id);

        // Signal shutdown
        let _ = self.shutdown_tx.send(()).await;

        // Stop cluster manager
        self.cluster.stop().await?;

        tracing::info!("Controller {} stopped", self.config.controller_id);
        Ok(())
    }

    /// Submit a work unit for execution (leader only)
    pub async fn submit_work_unit(&self, work_unit: WorkUnit) -> Result<(), ControllerError> {
        if !self.is_leader().await {
            return Err(ControllerError::NotLeader);
        }

        // Find the best controller to assign the work unit to
        let target = self.select_controller_for_work(&work_unit).await?;

        if target == self.config.controller_id {
            // Execute locally
            self.execute_work_unit(work_unit).await?;
        } else {
            // Send to target controller
            self.cluster
                .send_work_unit(&target, work_unit)
                .await
                .map_err(|e| ControllerError::Communication(e.to_string()))?;
        }

        Ok(())
    }

    /// Execute a work unit locally
    async fn execute_work_unit(&self, work_unit: WorkUnit) -> Result<(), ControllerError> {
        let id = work_unit.id.clone();
        let tasks = work_unit.tasks.clone();

        tracing::info!(
            "Controller {} executing work unit {}",
            self.config.controller_id,
            id
        );

        // Store work unit
        {
            let mut work_units = self.work_units.write().await;
            work_units.insert(
                id.clone(),
                WorkUnitExecution {
                    work_unit,
                    state: WorkUnitState::Running,
                    started_at: Instant::now(),
                    task_results: HashMap::new(),
                },
            );
        }

        let mut work_unit_failed = None;

        for (task_index, task) in tasks.into_iter().enumerate() {
            let task_result = self.execute_task(&task).await;
            let task_success = task_result.success;
            let task_error = task_result
                .error
                .clone()
                .unwrap_or_else(|| format!("task '{}' failed", task.name));

            let mut work_units = self.work_units.write().await;
            let execution = work_units
                .get_mut(&id)
                .ok_or_else(|| ControllerError::WorkUnitNotFound(id.clone()))?;

            execution.task_results.insert(task_index, task_result);

            if !task_success && !task.ignore_errors {
                let error = format!(
                    "task {} ('{}') failed: {}",
                    task_index, task.name, task_error
                );
                execution.state = WorkUnitState::Failed {
                    error: error.clone(),
                };
                execution.work_unit.state = execution.state.clone();
                work_unit_failed = Some(error);
                break;
            }
        }

        if let Some(error) = work_unit_failed {
            tracing::warn!(
                "Controller {} failed work unit {}: {}",
                self.config.controller_id,
                id,
                error
            );
        } else {
            {
                let mut work_units = self.work_units.write().await;
                let execution = work_units
                    .get_mut(&id)
                    .ok_or_else(|| ControllerError::WorkUnitNotFound(id.clone()))?;
                execution.state = WorkUnitState::Completed;
                execution.work_unit.state = WorkUnitState::Completed;
            }

            tracing::info!(
                "Controller {} completed work unit {}",
                self.config.controller_id,
                id
            );
        }

        Ok(())
    }

    async fn execute_task(&self, task: &TaskSpec) -> TaskResult {
        match task.module.as_str() {
            "command" => Self::execute_command_task(task).await,
            "shell" => Self::execute_shell_task(task).await,
            other => Self::task_error(
                format!("unsupported task module '{}'", other),
                serde_json::json!({ "module": other }),
            ),
        }
    }

    async fn execute_command_task(task: &TaskSpec) -> TaskResult {
        let parsed =
            if let Some(program) = task.args.get("program").and_then(|value| value.as_str()) {
                let args = match task.args.get("argv") {
                    Some(value) => {
                        let Some(argv) = value.as_array() else {
                            return Self::task_error(
                                "command task 'argv' must be an array".to_string(),
                                serde_json::json!({ "program": program }),
                            );
                        };

                        let mut parsed_args = Vec::with_capacity(argv.len());
                        for value in argv {
                            let Some(arg) = value.as_str() else {
                                return Self::task_error(
                                    "command task 'argv' entries must be strings".to_string(),
                                    serde_json::json!({ "program": program }),
                                );
                            };
                            parsed_args.push(arg.to_string());
                        }
                        parsed_args
                    }
                    None => Vec::new(),
                };

                Ok((program.to_string(), args))
            } else if let Some(cmd) = task
                .args
                .get("cmd")
                .or_else(|| task.args.get("command"))
                .and_then(|value| value.as_str())
            {
                match shell_words::split(cmd) {
                    Ok(parts) if !parts.is_empty() => Ok((parts[0].clone(), parts[1..].to_vec())),
                    Ok(_) => Err("command task 'cmd' cannot be empty".to_string()),
                    Err(error) => Err(format!("failed to parse command task 'cmd': {}", error)),
                }
            } else {
                Err("command task requires either 'program' or 'cmd' argument".to_string())
            };

        let (program, args) = match parsed {
            Ok(value) => value,
            Err(error) => {
                return Self::task_error(error, serde_json::json!({ "module": "command" }))
            }
        };

        let mut command = Command::new(&program);
        command.args(&args);

        let metadata = serde_json::json!({
            "module": "command",
            "program": program,
            "args": args,
        });
        Self::run_process(command, metadata).await
    }

    async fn execute_shell_task(task: &TaskSpec) -> TaskResult {
        let Some(command) = task
            .args
            .get("cmd")
            .or_else(|| task.args.get("command"))
            .and_then(|value| value.as_str())
        else {
            return Self::task_error(
                "shell task requires 'cmd' argument".to_string(),
                serde_json::json!({ "module": "shell" }),
            );
        };

        #[cfg(target_os = "windows")]
        let process = {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(command);
            cmd
        };

        #[cfg(not(target_os = "windows"))]
        let process = {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(command);
            cmd
        };

        let metadata = serde_json::json!({
            "module": "shell",
            "command": command,
        });
        Self::run_process(process, metadata).await
    }

    async fn run_process(mut command: Command, metadata: serde_json::Value) -> TaskResult {
        match command.output().await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code();
                let success = output.status.success();
                let error = if success {
                    None
                } else if !stderr.trim().is_empty() {
                    Some(stderr.trim().to_string())
                } else if let Some(code) = exit_code {
                    Some(format!("process exited with status {}", code))
                } else {
                    Some("process terminated by signal".to_string())
                };

                TaskResult {
                    success,
                    changed: false,
                    result: serde_json::json!({
                        "execution": metadata,
                        "stdout": stdout,
                        "stderr": stderr,
                        "exit_code": exit_code,
                    }),
                    error,
                }
            }
            Err(error) => Self::task_error(
                format!("failed to start process: {}", error),
                serde_json::json!({ "execution": metadata }),
            ),
        }
    }

    fn task_error(error: String, result: serde_json::Value) -> TaskResult {
        TaskResult {
            success: false,
            changed: false,
            result,
            error: Some(error),
        }
    }

    /// Select the best controller to execute a work unit
    async fn select_controller_for_work(
        &self,
        _work_unit: &WorkUnit,
    ) -> Result<ControllerId, ControllerError> {
        // Simple strategy: find the controller with the lowest load
        // For now, just use self since we don't have load info from peers
        let peers = self.cluster.connected_peers().await;

        let mut best_controller = self.config.controller_id.clone();
        let mut best_capacity = self.config.capacity;

        // Use capacity as a proxy for available work (higher capacity = can handle more)
        for (peer_id, peer_info) in peers {
            if peer_info.capacity > best_capacity && peer_info.health == ControllerHealth::Healthy {
                best_controller = peer_id;
                best_capacity = peer_info.capacity;
            }
        }

        Ok(best_controller)
    }

    /// Get work unit status
    pub async fn get_work_unit_status(&self, id: &WorkUnitId) -> Option<WorkUnitState> {
        self.work_units
            .read()
            .await
            .get(id)
            .map(|wu| wu.state.clone())
    }

    /// Get all active work units
    pub async fn active_work_units(&self) -> Vec<WorkUnitId> {
        self.work_units
            .read()
            .await
            .iter()
            .filter(|(_, wu)| matches!(wu.state, WorkUnitState::Running))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get cluster health status
    pub async fn cluster_health(&self) -> ClusterHealthReport {
        let peers = self.cluster.connected_peers().await;
        let peer_count = peers.len();
        let healthy_peers = peers
            .values()
            .filter(|p| matches!(p.health, ControllerHealth::Healthy))
            .count();

        ClusterHealthReport {
            controller_id: self.config.controller_id.clone(),
            role: self.role().await,
            leader_id: self.leader_id().await,
            peer_count,
            healthy_peers,
            total_work_units: self.work_units.read().await.len(),
            cluster_healthy: healthy_peers >= self.raft_state.quorum_size() - 1,
        }
    }
}

/// Cluster health report
#[derive(Debug, Clone)]
pub struct ClusterHealthReport {
    /// This controller's ID
    pub controller_id: ControllerId,
    /// Current role
    pub role: ControllerRole,
    /// Current leader (if known)
    pub leader_id: Option<ControllerId>,
    /// Number of known peers
    pub peer_count: usize,
    /// Number of healthy peers
    pub healthy_peers: usize,
    /// Total work units across cluster
    pub total_work_units: usize,
    /// Whether the cluster is healthy (has quorum)
    pub cluster_healthy: bool,
}

/// Controller errors
#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    #[error("Not the leader")]
    NotLeader,
    #[error("No leader available")]
    NoLeader,
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Work unit not found: {0}")]
    WorkUnitNotFound(WorkUnitId),
    #[error("Cluster error: {0}")]
    Cluster(#[from] super::cluster::ClusterError),
    #[error("Raft error: {0}")]
    Raft(#[from] super::raft::RaftError),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::types::{RunId, TaskSpec};
    use serde_json::json;
    use std::collections::HashMap;

    fn test_config() -> ClusterConfig {
        ClusterConfig {
            cluster_id: "test".to_string(),
            controller_id: ControllerId::new("test-ctrl"),
            bind_address: "127.0.0.1:9000".parse().unwrap(),
            peers: vec![],
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
            region: None,
            capacity: 500,
        }
    }

    #[tokio::test]
    async fn test_controller_creation() {
        let config = test_config();
        let controller = Controller::new(config.clone()).await.unwrap();

        assert_eq!(controller.id(), &config.controller_id);
        assert_eq!(controller.role().await, ControllerRole::Follower);
    }

    #[tokio::test]
    async fn test_controller_info() {
        let config = test_config();
        let controller = Controller::new(config.clone()).await.unwrap();

        let info = controller.info().await;
        assert_eq!(info.id, config.controller_id);
        assert_eq!(info.health, ControllerHealth::Healthy);
        assert_eq!(info.capacity, config.capacity);
    }

    fn test_task(name: &str, module: &str, args: HashMap<String, serde_json::Value>) -> TaskSpec {
        TaskSpec {
            name: name.to_string(),
            module: module.to_string(),
            args,
            when: None,
            register: None,
            ignore_errors: false,
        }
    }

    #[tokio::test]
    async fn test_execute_work_unit_not_completed_when_task_cannot_execute() {
        let controller = Controller::new(test_config()).await.unwrap();

        let task = test_task("missing command args", "command", HashMap::new());
        let work_unit = WorkUnit::new(RunId::generate(), 0, vec![]).with_task(task);
        let work_unit_id = work_unit.id.clone();

        controller.execute_work_unit(work_unit).await.unwrap();

        let state = controller
            .get_work_unit_status(&work_unit_id)
            .await
            .unwrap();
        assert!(matches!(state, WorkUnitState::Failed { .. }));
        assert!(!matches!(state, WorkUnitState::Completed));

        let work_units = controller.work_units.read().await;
        let execution = work_units.get(&work_unit_id).unwrap();
        let task_result = execution.task_results.get(&0).unwrap();
        assert!(!task_result.success);
        assert!(task_result.error.is_some());
    }

    #[tokio::test]
    async fn test_execute_work_unit_marks_failed_on_task_failure() {
        let controller = Controller::new(test_config()).await.unwrap();

        let mut args = HashMap::new();
        args.insert("program".to_string(), json!("rustc"));
        args.insert(
            "argv".to_string(),
            json!(["--definitely-invalid-flag-for-controller-test"]),
        );

        let task = test_task("failing rustc command", "command", args);
        let work_unit = WorkUnit::new(RunId::generate(), 0, vec![]).with_task(task);
        let work_unit_id = work_unit.id.clone();

        controller.execute_work_unit(work_unit).await.unwrap();

        let state = controller
            .get_work_unit_status(&work_unit_id)
            .await
            .unwrap();
        match state {
            WorkUnitState::Failed { error } => assert!(error.contains("task 0")),
            other => panic!("expected failed state, got {:?}", other),
        }

        let work_units = controller.work_units.read().await;
        let execution = work_units.get(&work_unit_id).unwrap();
        let task_result = execution.task_results.get(&0).unwrap();
        assert!(!task_result.success);
        assert!(task_result.error.is_some());
    }
}
