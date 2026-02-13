//! Cluster topology and health graph
//!
//! This module provides a graph-based representation of the distributed
//! cluster topology, with health aggregation, querying, and rendering
//! capabilities.
//!
//! # Components
//!
//! - [`model`]: Core topology graph built on petgraph
//! - [`health`]: Health check aggregation across cluster nodes
//! - [`query`]: Query interface for filtering and traversing the topology
//! - [`renderer`]: ASCII, JSON, and table rendering of the topology graph

pub mod health;
pub mod model;
pub mod query;
pub mod renderer;

pub use health::{ClusterHealthSummary, HealthAggregator, HealthCheck, NodeHealth};
pub use model::{ClusterTopology, EdgeType, NodeRole, NodeType, TopologyEdge, TopologyNode};
pub use query::TopologyQuery;
pub use renderer::{RenderFormat, TopologyRenderer};
