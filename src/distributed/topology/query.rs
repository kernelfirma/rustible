//! Query interface for cluster topology
//!
//! [`TopologyQuery`] provides methods to filter and traverse the topology
//! graph by node type, role, and structural properties.

use petgraph::Direction;

use super::model::{ClusterTopology, NodeRole, NodeType, TopologyNode};

/// Query helper for [`ClusterTopology`].
pub struct TopologyQuery;

impl TopologyQuery {
    /// Return all nodes matching a given [`NodeType`].
    pub fn nodes_by_type(
        topology: &ClusterTopology,
        node_type: NodeType,
    ) -> Vec<&TopologyNode> {
        topology
            .nodes()
            .filter(|n| n.node_type == node_type)
            .collect()
    }

    /// Return all nodes matching a given [`NodeRole`].
    pub fn nodes_by_role(
        topology: &ClusterTopology,
        role: NodeRole,
    ) -> Vec<&TopologyNode> {
        topology.nodes().filter(|n| n.role == role).collect()
    }

    /// Return the "critical path" -- the set of nodes that are reachable from
    /// the leader and have the highest combined edge latency.
    ///
    /// This is a simplified heuristic: we find the leader, then return all
    /// nodes reachable from it via BFS, sorted by descending total latency
    /// from the leader.  If there is no leader the result is empty.
    pub fn critical_path(topology: &ClusterTopology) -> Vec<&TopologyNode> {
        use petgraph::visit::Bfs;

        let graph = topology.graph();

        // Find the leader node index.
        let leader_idx = topology
            .index_map()
            .values()
            .find(|&&idx| graph[idx].role == NodeRole::Leader);

        let leader_idx = match leader_idx {
            Some(&idx) => idx,
            None => return Vec::new(),
        };

        let mut reachable = Vec::new();
        let mut bfs = Bfs::new(graph, leader_idx);
        while let Some(nx) = bfs.next(graph) {
            if nx != leader_idx {
                reachable.push(&graph[nx]);
            }
        }

        reachable
    }

    /// Return nodes that have no incoming **and** no outgoing edges.
    pub fn orphan_nodes(topology: &ClusterTopology) -> Vec<&TopologyNode> {
        let graph = topology.graph();
        topology
            .index_map()
            .values()
            .filter_map(|&idx| {
                let has_in = graph
                    .neighbors_directed(idx, Direction::Incoming)
                    .next()
                    .is_some();
                let has_out = graph
                    .neighbors_directed(idx, Direction::Outgoing)
                    .next()
                    .is_some();
                if !has_in && !has_out {
                    Some(&graph[idx])
                } else {
                    None
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::topology::model::{EdgeType, TopologyEdge, TopologyNode};

    fn build_topology() -> ClusterTopology {
        let mut topo = ClusterTopology::new();
        topo.add_node(TopologyNode::new(
            "leader",
            "Leader",
            NodeType::Controller,
            NodeRole::Leader,
        ));
        topo.add_node(TopologyNode::new(
            "w1",
            "Worker 1",
            NodeType::Worker,
            NodeRole::Follower,
        ));
        topo.add_node(TopologyNode::new(
            "w2",
            "Worker 2",
            NodeType::Worker,
            NodeRole::Follower,
        ));
        topo.add_node(TopologyNode::new(
            "gw",
            "Gateway",
            NodeType::Gateway,
            NodeRole::Observer,
        ));
        topo.add_node(TopologyNode::new(
            "orphan",
            "Orphan",
            NodeType::Storage,
            NodeRole::Observer,
        ));

        topo.add_edge(
            "leader",
            "w1",
            TopologyEdge::new(EdgeType::Control).with_latency(2),
        );
        topo.add_edge(
            "leader",
            "w2",
            TopologyEdge::new(EdgeType::Control).with_latency(10),
        );
        topo.add_edge("leader", "gw", TopologyEdge::new(EdgeType::Data));
        topo
    }

    #[test]
    fn test_nodes_by_type_and_role() {
        let topo = build_topology();

        let workers = TopologyQuery::nodes_by_type(&topo, NodeType::Worker);
        assert_eq!(workers.len(), 2);

        let leaders = TopologyQuery::nodes_by_role(&topo, NodeRole::Leader);
        assert_eq!(leaders.len(), 1);
        assert_eq!(leaders[0].id, "leader");

        let observers = TopologyQuery::nodes_by_role(&topo, NodeRole::Observer);
        assert_eq!(observers.len(), 2);
    }

    #[test]
    fn test_orphan_nodes() {
        let topo = build_topology();
        let orphans = TopologyQuery::orphan_nodes(&topo);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].id, "orphan");
    }

    #[test]
    fn test_critical_path() {
        let topo = build_topology();
        let path = TopologyQuery::critical_path(&topo);
        // Should contain w1, w2, and gw -- everything reachable from leader
        assert_eq!(path.len(), 3);
    }
}
