//! Distributed execution types and common structures
//!
//! This module defines the core types used throughout the distributed
//! execution system, including controller identification, work units,
//! and cluster state.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Unique identifier for a controller node
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ControllerId(pub String);

impl ControllerId {
    /// Create a new controller ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a random controller ID
    pub fn generate() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(format!("ctrl-{:x}", timestamp))
    }
}

impl std::fmt::Display for ControllerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a work unit
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkUnitId(pub String);

impl WorkUnitId {
    /// Create a new work unit ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a random work unit ID
    pub fn generate() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(format!("wu-{:x}", timestamp))
    }
}

impl std::fmt::Display for WorkUnitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a playbook run
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub String);

impl RunId {
    /// Create a new run ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a random run ID
    pub fn generate() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(format!("run-{:x}", timestamp))
    }
}

/// Unique identifier for a host
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HostId(pub String);

impl HostId {
    /// Create a new host ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// Controller role in the Raft cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ControllerRole {
    /// Leader node - handles work distribution
    Leader,
    /// Follower node - executes assigned work
    #[default]
    Follower,
    /// Candidate node - participating in election
    Candidate,
}

/// Health status of a controller
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ControllerHealth {
    /// Controller is healthy and responsive
    #[default]
    Healthy,
    /// Controller is degraded but functional
    Degraded,
    /// Controller is suspected to be down
    Suspected,
    /// Controller is confirmed down
    Down,
}

/// Information about a controller node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerInfo {
    /// Controller identifier
    pub id: ControllerId,
    /// Network address
    pub address: SocketAddr,
    /// Geographic region (optional)
    pub region: Option<String>,
    /// Controller capabilities
    pub capabilities: Vec<String>,
    /// Maximum host capacity
    pub capacity: u32,
    /// Current health status
    pub health: ControllerHealth,
    /// Last heartbeat timestamp (not serialized)
    #[serde(skip)]
    pub last_heartbeat: Option<Instant>,
}

impl ControllerInfo {
    /// Create new controller info
    pub fn new(id: ControllerId, address: SocketAddr) -> Self {
        Self {
            id,
            address,
            region: None,
            capabilities: Vec::new(),
            capacity: 500, // Default capacity
            health: ControllerHealth::Healthy,
            last_heartbeat: Some(Instant::now()),
        }
    }

    /// Set the region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Set the capacity
    pub fn with_capacity(mut self, capacity: u32) -> Self {
        self.capacity = capacity;
        self
    }

    /// Add a capability
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self
    }

    /// Check if controller is healthy based on heartbeat
    pub fn is_healthy(&self, timeout: Duration) -> bool {
        self.last_heartbeat
            .map(|t| t.elapsed() < timeout)
            .unwrap_or(false)
    }

    /// Update heartbeat timestamp
    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat = Some(Instant::now());
    }
}

/// Controller load metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControllerLoad {
    /// Number of active work units
    pub active_work_units: u32,
    /// Number of active host connections
    pub active_connections: u32,
    /// CPU usage percentage (0-100)
    pub cpu_usage: f32,
    /// Memory usage percentage (0-100)
    pub memory_usage: f32,
    /// Network bandwidth usage percentage (0-100)
    pub bandwidth_usage: f32,
    /// Average task latency in milliseconds
    pub avg_latency_ms: u64,
    /// Queue depth (pending work units)
    pub queue_depth: u32,
    /// Estimated capacity (hosts)
    pub capacity: u32,
}

impl ControllerLoad {
    /// Calculate composite load score (0.0 - 1.0)
    pub fn load_score(&self) -> f64 {
        if self.capacity == 0 {
            return 1.0;
        }

        let connection_load = self.active_connections as f64 / self.capacity as f64;
        let cpu_load = self.cpu_usage as f64 / 100.0;
        let memory_load = self.memory_usage as f64 / 100.0;
        let queue_load = (self.queue_depth as f64 / 100.0).min(1.0);

        // Weighted average
        (connection_load * 0.4 + cpu_load * 0.2 + memory_load * 0.2 + queue_load * 0.2).min(1.0)
    }

    /// Check if controller is overloaded
    pub fn is_overloaded(&self) -> bool {
        self.load_score() > 0.85
    }

    /// Check if controller can accept more work
    pub fn can_accept_work(&self) -> bool {
        self.load_score() < 0.75
    }
}

/// Work unit lifecycle states
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WorkUnitState {
    /// Work unit is pending assignment
    #[default]
    Pending,
    /// Work unit has been assigned to a controller
    Assigned,
    /// Work unit is currently executing
    Running,
    /// Work unit completed successfully
    Completed,
    /// Work unit failed
    Failed { error: String },
    /// Work unit was cancelled
    Cancelled,
}

/// Task specification within a work unit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    /// Task name
    pub name: String,
    /// Module to execute
    pub module: String,
    /// Module arguments
    pub args: HashMap<String, serde_json::Value>,
    /// When conditions
    pub when: Option<String>,
    /// Register variable name
    pub register: Option<String>,
    /// Ignore errors flag
    pub ignore_errors: bool,
}

/// A unit of work for distributed execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkUnit {
    /// Unique identifier
    pub id: WorkUnitId,
    /// Parent playbook run ID
    pub run_id: RunId,
    /// Play index within playbook
    pub play_index: usize,
    /// Target hosts for this work unit
    pub hosts: Vec<HostId>,
    /// Tasks to execute
    pub tasks: Vec<TaskSpec>,
    /// Dependencies on other work units
    pub dependencies: Vec<WorkUnitId>,
    /// Priority (higher = more urgent)
    pub priority: u32,
    /// Deadline for completion (milliseconds since epoch)
    pub deadline_ms: Option<u64>,
    /// Assigned controller (None = unassigned)
    pub assigned_to: Option<ControllerId>,
    /// Current state
    pub state: WorkUnitState,
    /// Retry count
    pub retries: u32,
    /// Maximum retries allowed
    pub max_retries: u32,
    /// Creation timestamp
    pub created_at_ms: u64,
}

impl WorkUnit {
    /// Create a new work unit
    pub fn new(run_id: RunId, play_index: usize, hosts: Vec<HostId>) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            id: WorkUnitId::generate(),
            run_id,
            play_index,
            hosts,
            tasks: Vec::new(),
            dependencies: Vec::new(),
            priority: 50, // Default priority
            deadline_ms: None,
            assigned_to: None,
            state: WorkUnitState::Pending,
            retries: 0,
            max_retries: 3,
            created_at_ms: created_at,
        }
    }

    /// Add a task to the work unit
    pub fn with_task(mut self, task: TaskSpec) -> Self {
        self.tasks.push(task);
        self
    }

    /// Add a dependency
    pub fn with_dependency(mut self, dep: WorkUnitId) -> Self {
        self.dependencies.push(dep);
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Check if dependencies are satisfied
    pub fn dependencies_satisfied(&self, completed: &HashSet<WorkUnitId>) -> bool {
        self.dependencies.iter().all(|d| completed.contains(d))
    }

    /// Check if work unit can be retried
    pub fn can_retry(&self) -> bool {
        self.retries < self.max_retries
    }

    /// Increment retry count
    pub fn increment_retry(&mut self) {
        self.retries += 1;
    }
}

/// Checkpoint for resumable execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkUnitCheckpoint {
    /// Work unit ID
    pub work_unit_id: WorkUnitId,
    /// Timestamp of checkpoint (milliseconds since epoch)
    pub timestamp_ms: u64,
    /// Completed hosts
    pub completed_hosts: Vec<HostId>,
    /// Failed hosts (with errors)
    pub failed_hosts: HashMap<HostId, String>,
    /// Current task index per host
    pub host_task_index: HashMap<HostId, usize>,
    /// Pending handlers to notify
    pub pending_handlers: HashSet<String>,
}

impl WorkUnitCheckpoint {
    /// Create a new checkpoint
    pub fn new(work_unit_id: WorkUnitId) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            work_unit_id,
            timestamp_ms: timestamp,
            completed_hosts: Vec::new(),
            failed_hosts: HashMap::new(),
            host_task_index: HashMap::new(),
            pending_handlers: HashSet::new(),
        }
    }

    /// Mark a host as completed
    pub fn mark_completed(&mut self, host: HostId) {
        self.completed_hosts.push(host);
    }

    /// Mark a host as failed
    pub fn mark_failed(&mut self, host: HostId, error: String) {
        self.failed_hosts.insert(host, error);
    }

    /// Update task index for a host
    pub fn update_task_index(&mut self, host: HostId, index: usize) {
        self.host_task_index.insert(host, index);
    }
}

/// Heartbeat message between controllers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    /// Source controller ID
    pub controller_id: ControllerId,
    /// Timestamp (milliseconds since epoch)
    pub timestamp_ms: u64,
    /// Current load metrics
    pub load: ControllerLoad,
    /// Active work unit IDs
    pub active_work_units: Vec<WorkUnitId>,
    /// Current Raft term
    pub term: u64,
    /// Leader ID (if known)
    pub leader_id: Option<ControllerId>,
}

impl Heartbeat {
    /// Create a new heartbeat
    pub fn new(controller_id: ControllerId, term: u64) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            controller_id,
            timestamp_ms: timestamp,
            load: ControllerLoad::default(),
            active_work_units: Vec::new(),
            term,
            leader_id: None,
        }
    }

    /// Set load metrics
    pub fn with_load(mut self, load: ControllerLoad) -> Self {
        self.load = load;
        self
    }

    /// Set active work units
    pub fn with_work_units(mut self, units: Vec<WorkUnitId>) -> Self {
        self.active_work_units = units;
        self
    }

    /// Set leader ID
    pub fn with_leader(mut self, leader: ControllerId) -> Self {
        self.leader_id = Some(leader);
        self
    }
}

/// Cluster configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Unique cluster identifier
    pub cluster_id: String,
    /// This controller's ID
    pub controller_id: ControllerId,
    /// This controller's bind address
    pub bind_address: SocketAddr,
    /// Peer controller addresses
    pub peers: Vec<SocketAddr>,
    /// Raft election timeout minimum (milliseconds)
    pub election_timeout_min_ms: u64,
    /// Raft election timeout maximum (milliseconds)
    pub election_timeout_max_ms: u64,
    /// Heartbeat interval (milliseconds)
    pub heartbeat_interval_ms: u64,
    /// Controller region
    pub region: Option<String>,
    /// Maximum host capacity
    pub capacity: u32,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            cluster_id: "rustible-default".to_string(),
            controller_id: ControllerId::generate(),
            bind_address: "0.0.0.0:9000".parse().unwrap(),
            peers: Vec::new(),
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
            region: None,
            capacity: 500,
        }
    }
}

impl ClusterConfig {
    /// Create new cluster configuration
    pub fn new(cluster_id: impl Into<String>, bind_address: SocketAddr) -> Self {
        Self {
            cluster_id: cluster_id.into(),
            controller_id: ControllerId::generate(),
            bind_address,
            ..Default::default()
        }
    }

    /// Add a peer address
    pub fn with_peer(mut self, peer: SocketAddr) -> Self {
        self.peers.push(peer);
        self
    }

    /// Set the region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Get random election timeout
    pub fn random_election_timeout(&self) -> Duration {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let timeout_ms = rng.gen_range(self.election_timeout_min_ms..=self.election_timeout_max_ms);
        Duration::from_millis(timeout_ms)
    }

    /// Get heartbeat interval
    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_millis(self.heartbeat_interval_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_id_generation() {
        let id1 = ControllerId::generate();
        let id2 = ControllerId::generate();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_controller_load_score() {
        let load = ControllerLoad {
            active_connections: 250,
            cpu_usage: 50.0,
            memory_usage: 60.0,
            queue_depth: 10,
            capacity: 500,
            ..Default::default()
        };

        let score = load.load_score();
        assert!(score > 0.0 && score < 1.0);
    }

    #[test]
    fn test_work_unit_dependencies() {
        let wu = WorkUnit::new(RunId::generate(), 0, vec![])
            .with_dependency(WorkUnitId::new("dep-1"))
            .with_dependency(WorkUnitId::new("dep-2"));

        let mut completed = HashSet::new();
        assert!(!wu.dependencies_satisfied(&completed));

        completed.insert(WorkUnitId::new("dep-1"));
        assert!(!wu.dependencies_satisfied(&completed));

        completed.insert(WorkUnitId::new("dep-2"));
        assert!(wu.dependencies_satisfied(&completed));
    }

    #[test]
    fn test_controller_info_health() {
        let mut info =
            ControllerInfo::new(ControllerId::new("test"), "127.0.0.1:9000".parse().unwrap());

        assert!(info.is_healthy(Duration::from_secs(5)));

        // Simulate old heartbeat by setting to None
        info.last_heartbeat = None;
        assert!(!info.is_healthy(Duration::from_secs(5)));
    }
}
