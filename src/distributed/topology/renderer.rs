//! Topology rendering in various output formats
//!
//! [`TopologyRenderer`] can produce ASCII art, JSON, or tabular
//! representations of a [`ClusterTopology`].

use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};

use super::model::ClusterTopology;

/// Supported rendering formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderFormat {
    /// Simple ASCII-art graph
    Ascii,
    /// Machine-readable JSON
    Json,
    /// Human-readable table
    Table,
}

/// Renders a [`ClusterTopology`] into a string.
pub struct TopologyRenderer;

impl TopologyRenderer {
    /// Render the topology in the requested format.
    pub fn render(topology: &ClusterTopology, format: RenderFormat) -> String {
        match format {
            RenderFormat::Ascii => Self::render_ascii(topology),
            RenderFormat::Json => Self::render_json(topology),
            RenderFormat::Table => Self::render_table(topology),
        }
    }

    // -- private helpers ----------------------------------------------------

    fn render_ascii(topology: &ClusterTopology) -> String {
        let graph = topology.graph();
        let mut out = String::new();

        out.push_str("Cluster Topology\n");
        out.push_str(&"=".repeat(40));
        out.push('\n');

        for idx in graph.node_indices() {
            let node = &graph[idx];
            out.push_str(&format!(
                "[{}] {} (type={}, role={})\n",
                node.id, node.name, node.node_type, node.role,
            ));

            // Outgoing edges
            for edge_ref in graph.edges(idx) {
                let target = &graph[edge_ref.target()];
                let edge = edge_ref.weight();
                let latency = edge
                    .latency_ms
                    .map(|l| format!(" ~{}ms", l))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "  └──({})──> [{}]{}\n",
                    edge.edge_type, target.id, latency,
                ));
            }
        }

        out
    }

    fn render_json(topology: &ClusterTopology) -> String {
        let graph = topology.graph();

        let nodes: Vec<serde_json::Value> = graph
            .node_weights()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "name": n.name,
                    "type": format!("{}", n.node_type),
                    "role": format!("{}", n.role),
                    "address": n.address,
                })
            })
            .collect();

        let edges: Vec<serde_json::Value> = graph
            .edge_indices()
            .filter_map(|ei| {
                let (src, tgt) = graph.edge_endpoints(ei)?;
                let edge = &graph[ei];
                Some(serde_json::json!({
                    "from": graph[src].id,
                    "to": graph[tgt].id,
                    "type": format!("{}", edge.edge_type),
                    "latency_ms": edge.latency_ms,
                }))
            })
            .collect();

        let doc = serde_json::json!({
            "nodes": nodes,
            "edges": edges,
        });

        serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
    }

    fn render_table(topology: &ClusterTopology) -> String {
        let graph = topology.graph();
        let mut out = String::new();

        out.push_str(&format!(
            "{:<15} {:<20} {:<12} {:<10} {:<20}\n",
            "ID", "NAME", "TYPE", "ROLE", "ADDRESS",
        ));
        out.push_str(&"-".repeat(77));
        out.push('\n');

        for node in graph.node_weights() {
            out.push_str(&format!(
                "{:<15} {:<20} {:<12} {:<10} {:<20}\n",
                node.id,
                node.name,
                format!("{}", node.node_type),
                format!("{}", node.role),
                node.address.as_deref().unwrap_or("-"),
            ));
        }

        out.push_str(&format!(
            "\nTotal: {} node(s), {} edge(s)\n",
            topology.node_count(),
            topology.edge_count()
        ));
        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::topology::model::{
        EdgeType, NodeRole, NodeType, TopologyEdge, TopologyNode,
    };

    fn sample_topology() -> ClusterTopology {
        let mut topo = ClusterTopology::new();
        topo.add_node(
            TopologyNode::new(
                "ctrl-1",
                "Controller 1",
                NodeType::Controller,
                NodeRole::Leader,
            )
            .with_address("10.0.0.1:9000"),
        );
        topo.add_node(TopologyNode::new(
            "worker-1",
            "Worker 1",
            NodeType::Worker,
            NodeRole::Follower,
        ));
        topo.add_edge(
            "ctrl-1",
            "worker-1",
            TopologyEdge::new(EdgeType::Control).with_latency(3),
        );
        topo
    }

    #[test]
    fn test_render_ascii() {
        let topo = sample_topology();
        let output = TopologyRenderer::render(&topo, RenderFormat::Ascii);
        assert!(output.contains("Cluster Topology"));
        assert!(output.contains("[ctrl-1]"));
        assert!(output.contains("[worker-1]"));
        assert!(output.contains("control"));
    }

    #[test]
    fn test_render_json() {
        let topo = sample_topology();
        let output = TopologyRenderer::render(&topo, RenderFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["edges"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_render_table() {
        let topo = sample_topology();
        let output = TopologyRenderer::render(&topo, RenderFormat::Table);
        assert!(output.contains("ID"));
        assert!(output.contains("ctrl-1"));
        assert!(output.contains("Total: 2 node(s), 1 edge(s)"));
    }
}
