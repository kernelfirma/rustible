//! Core topology graph model
//!
//! Provides the [`ClusterTopology`] struct which wraps a petgraph directed
//! graph to represent cluster nodes and the edges (links) between them.

use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Type of a node in the topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    /// A controller that coordinates work distribution
    Controller,
    /// A worker that executes assigned tasks
    Worker,
    /// An API gateway or ingress point
    Gateway,
    /// A storage or state-persistence node
    Storage,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Controller => write!(f, "controller"),
            Self::Worker => write!(f, "worker"),
            Self::Gateway => write!(f, "gateway"),
            Self::Storage => write!(f, "storage"),
        }
    }
}

/// Role a node plays in the consensus protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    /// Current cluster leader
    Leader,
    /// Active follower replicating state
    Follower,
    /// Node participating in an election
    Candidate,
    /// Read-only observer (non-voting)
    Observer,
}

impl std::fmt::Display for NodeRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Leader => write!(f, "leader"),
            Self::Follower => write!(f, "follower"),
            Self::Candidate => write!(f, "candidate"),
            Self::Observer => write!(f, "observer"),
        }
    }
}

/// A node in the cluster topology graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyNode {
    /// Unique identifier for this node
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Type of node
    pub node_type: NodeType,
    /// Consensus role
    pub role: NodeRole,
    /// Network address (host:port) if known
    pub address: Option<String>,
}

impl TopologyNode {
    /// Create a new topology node.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        node_type: NodeType,
        role: NodeRole,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            node_type,
            role,
            address: None,
        }
    }

    /// Set the network address.
    pub fn with_address(mut self, address: impl Into<String>) -> Self {
        self.address = Some(address.into());
        self
    }
}

/// Type of edge connecting two topology nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeType {
    /// Control-plane communication (elections, coordination)
    Control,
    /// Data-plane communication (task payloads, results)
    Data,
    /// Heartbeat / health-check link
    Heartbeat,
}

impl std::fmt::Display for EdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Control => write!(f, "control"),
            Self::Data => write!(f, "data"),
            Self::Heartbeat => write!(f, "heartbeat"),
        }
    }
}

/// An edge (link) between two topology nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyEdge {
    /// Kind of communication this edge represents
    pub edge_type: EdgeType,
    /// Measured one-way latency in milliseconds, if known
    pub latency_ms: Option<u64>,
}

impl TopologyEdge {
    /// Create a new topology edge.
    pub fn new(edge_type: EdgeType) -> Self {
        Self {
            edge_type,
            latency_ms: None,
        }
    }

    /// Set the latency measurement.
    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = Some(latency_ms);
        self
    }
}

/// Graph-based representation of a cluster topology.
///
/// Wraps a [`petgraph::DiGraph`] with convenience methods for adding and
/// querying nodes and edges by their logical identifiers.
pub struct ClusterTopology {
    graph: DiGraph<TopologyNode, TopologyEdge>,
    /// Maps node id strings to their graph indices for fast lookup.
    index_map: HashMap<String, NodeIndex>,
}

impl ClusterTopology {
    /// Create an empty topology.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            index_map: HashMap::new(),
        }
    }

    /// Add a node to the topology. Returns the petgraph [`NodeIndex`].
    ///
    /// If a node with the same `id` already exists, it is replaced and the
    /// same index is reused.
    pub fn add_node(&mut self, node: TopologyNode) -> NodeIndex {
        if let Some(&idx) = self.index_map.get(&node.id) {
            self.graph[idx] = node;
            idx
        } else {
            let id = node.id.clone();
            let idx = self.graph.add_node(node);
            self.index_map.insert(id, idx);
            idx
        }
    }

    /// Add a directed edge between two nodes identified by their string ids.
    ///
    /// Returns `true` if the edge was added, `false` if either node id was not
    /// found in the topology.
    pub fn add_edge(&mut self, from_id: &str, to_id: &str, edge: TopologyEdge) -> bool {
        let from = self.index_map.get(from_id).copied();
        let to = self.index_map.get(to_id).copied();
        match (from, to) {
            (Some(a), Some(b)) => {
                self.graph.add_edge(a, b, edge);
                true
            }
            _ => false,
        }
    }

    /// Number of nodes in the topology.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of edges in the topology.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Look up a node by its string id.
    pub fn get_node(&self, id: &str) -> Option<&TopologyNode> {
        self.index_map.get(id).map(|&idx| &self.graph[idx])
    }

    /// Return a reference to the underlying petgraph.
    pub fn graph(&self) -> &DiGraph<TopologyNode, TopologyEdge> {
        &self.graph
    }

    /// Return the index map (node-id -> NodeIndex).
    pub fn index_map(&self) -> &HashMap<String, NodeIndex> {
        &self.index_map
    }

    /// Iterate over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &TopologyNode> {
        self.graph.node_weights()
    }
}

impl Default for ClusterTopology {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_retrieve_nodes() {
        let mut topo = ClusterTopology::new();

        let n1 = TopologyNode::new(
            "ctrl-1",
            "Controller 1",
            NodeType::Controller,
            NodeRole::Leader,
        )
        .with_address("10.0.0.1:9000");
        let n2 = TopologyNode::new("worker-1", "Worker 1", NodeType::Worker, NodeRole::Follower);

        topo.add_node(n1);
        topo.add_node(n2);

        assert_eq!(topo.node_count(), 2);
        assert_eq!(topo.edge_count(), 0);

        let retrieved = topo.get_node("ctrl-1").unwrap();
        assert_eq!(retrieved.name, "Controller 1");
        assert_eq!(retrieved.node_type, NodeType::Controller);
        assert_eq!(retrieved.role, NodeRole::Leader);
        assert_eq!(retrieved.address.as_deref(), Some("10.0.0.1:9000"));

        assert!(topo.get_node("nonexistent").is_none());
    }

    #[test]
    fn test_add_edges() {
        let mut topo = ClusterTopology::new();

        topo.add_node(TopologyNode::new(
            "a",
            "A",
            NodeType::Controller,
            NodeRole::Leader,
        ));
        topo.add_node(TopologyNode::new(
            "b",
            "B",
            NodeType::Worker,
            NodeRole::Follower,
        ));
        topo.add_node(TopologyNode::new(
            "c",
            "C",
            NodeType::Worker,
            NodeRole::Follower,
        ));

        assert!(topo.add_edge("a", "b", TopologyEdge::new(EdgeType::Control)));
        assert!(topo.add_edge("a", "c", TopologyEdge::new(EdgeType::Data).with_latency(5)));
        // Non-existent target should fail gracefully.
        assert!(!topo.add_edge("a", "z", TopologyEdge::new(EdgeType::Heartbeat)));

        assert_eq!(topo.edge_count(), 2);
    }

    #[test]
    fn test_replace_existing_node() {
        let mut topo = ClusterTopology::new();

        topo.add_node(TopologyNode::new(
            "x",
            "X-old",
            NodeType::Worker,
            NodeRole::Follower,
        ));
        assert_eq!(topo.get_node("x").unwrap().name, "X-old");

        topo.add_node(TopologyNode::new(
            "x",
            "X-new",
            NodeType::Worker,
            NodeRole::Leader,
        ));
        assert_eq!(topo.node_count(), 1);
        assert_eq!(topo.get_node("x").unwrap().name, "X-new");
        assert_eq!(topo.get_node("x").unwrap().role, NodeRole::Leader);
    }
}
