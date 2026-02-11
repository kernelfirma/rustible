//! Distributed execution support for Rustible
//!
//! This module provides distributed execution capabilities, allowing Rustible
//! to scale across multiple controller nodes for improved performance and
//! fault tolerance.
//!
//! # Architecture
//!
//! The distributed execution system uses a leader-follower architecture:
//!
//! - **Leader**: Coordinates work distribution and maintains cluster state
//! - **Followers**: Execute assigned work units and report results
//! - **Candidates**: Nodes participating in leader election
//!
//! Leader election is handled via the Raft consensus protocol (simplified
//! version focused on leader election, not full log replication).
//!
//! # Components
//!
//! - [`types`]: Core types for distributed execution
//! - [`raft`]: Raft consensus implementation for leader election
//! - [`controller`]: Controller node implementation
//! - [`cluster`]: Cluster management and discovery
//!
//! # Example
//!
//! ```rust,no_run
//! use rustible::distributed::{Controller, ClusterConfig, ControllerId};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ClusterConfig {
//!         cluster_id: "my-cluster".to_string(),
//!         controller_id: ControllerId::new("ctrl-1"),
//!         bind_address: "127.0.0.1:9000".parse()?,
//!         peers: vec!["127.0.0.1:9001".parse()?, "127.0.0.1:9002".parse()?],
//!         ..Default::default()
//!     };
//!
//!     let controller = Controller::new(config).await?;
//!     controller.start().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod cluster;
pub mod controller;
pub mod distribution;
pub mod fanout;
pub mod observability;
pub mod raft;
pub mod recovery;
pub mod state;
pub mod types;
pub mod topology;

// Re-export commonly used types
pub use cluster::{ClusterManager, ClusterState, PeerConnection};
pub use controller::{Controller, ControllerError};
pub use distribution::{
    AffinityAssigner, AssignmentStrategy, CapacityAwareAssigner, LoadBalancer, RoundRobinAssigner,
    WorkAssigner, WorkQueue,
};
pub use observability::{
    ClusterStatusResponse, ComponentHealth, ControllerStatusInfo, HealthResponse, HealthStatus,
    LiveResponse, LoadMetrics, MetricValue, MetricsResponse, ObservabilityCollector,
    PrometheusMetric, ReadyResponse, WorkUnitInfo, WorkUnitStats, WorkUnitStatusResponse,
};
pub use raft::{RaftError, RaftEvent, RaftNode, RaftState};
pub use recovery::{
    CachedTaskResult, CheckpointManager, ExecutionState, ExecutionTracker, IdempotencyKey,
    IdempotencyTracker, LeaderRecovery, PartitionDetector, PartitionState, RecoveryAction,
};
pub use state::{
    ConsistencyLevel, DistributedStateStore, FactsStore, LWWEntry, LWWMap, SyncRequest,
    SyncResponse, HLC,
};
pub use types::{
    ClusterConfig, ControllerHealth, ControllerId, ControllerInfo, ControllerLoad, ControllerRole,
    Heartbeat, HostId, RunId, TaskSpec, WorkUnit, WorkUnitCheckpoint, WorkUnitId, WorkUnitState,
};
