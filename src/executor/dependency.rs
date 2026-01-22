//! Resource Dependency Management for Rustible
//!
//! This module provides comprehensive dependency tracking and graph management
//! for task orchestration. It enables:
//!
//! - Explicit task dependencies via `depends_on`
//! - Implicit dependencies from variable registration (`register` -> usage)
//! - File/resource dependencies between tasks
//! - Circular dependency detection with detailed path reporting
//! - Topological sorting for execution ordering
//! - Visualization output in DOT and Mermaid formats
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::executor::dependency::{DependencyGraph, DependencyNode, DependencyKind};
//!
//! let mut graph = DependencyGraph::new();
//!
//! // Add nodes
//! graph.add_node("install_deps", DependencyNode::task("Install Dependencies"));
//! graph.add_node("build_app", DependencyNode::task("Build Application"));
//! graph.add_node("deploy", DependencyNode::task("Deploy"));
//!
//! // Add dependencies
//! graph.add_dependency("build_app", "install_deps", DependencyKind::Explicit)?;
//! graph.add_dependency("deploy", "build_app", DependencyKind::Explicit)?;
//!
//! // Get execution order
//! let order = graph.topological_sort()?;
//! assert_eq!(order, vec!["install_deps", "build_app", "deploy"]);
//!
//! // Visualize
//! println!("{}", graph.to_dot());
//! # Ok(())
//! # }
//! ```

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use thiserror::Error;

use super::task::Task;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during dependency resolution
#[derive(Error, Debug, Clone)]
pub enum DependencyError {
    /// A circular dependency was detected
    #[error("Circular dependency detected: {}", format_cycle(.0))]
    CircularDependency(Vec<String>),

    /// A referenced node does not exist
    #[error("Dependency target not found: '{0}'")]
    NodeNotFound(String),

    /// A self-referential dependency was attempted
    #[error("Task '{0}' cannot depend on itself")]
    SelfDependency(String),

    /// Dependency resolution failed
    #[error("Dependency resolution failed: {0}")]
    ResolutionError(String),
}

/// Format a cycle path for display
fn format_cycle(cycle: &[String]) -> String {
    if cycle.is_empty() {
        return "empty cycle".to_string();
    }
    let mut result = cycle.join(" -> ");
    if let Some(first) = cycle.first() {
        result.push_str(" -> ");
        result.push_str(first);
    }
    result
}

/// Result type for dependency operations
pub type DependencyResult<T> = Result<T, DependencyError>;

// ============================================================================
// Dependency Types
// ============================================================================

/// The kind/reason for a dependency relationship
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DependencyKind {
    /// Explicit dependency declared via `depends_on`
    Explicit,
    /// Implicit dependency from registered variable usage
    Variable,
    /// File/resource dependency (task produces file another consumes)
    Resource,
    /// Handler notification dependency
    Handler,
    /// Role dependency (meta/main.yml dependencies)
    Role,
    /// Include/import dependency
    Include,
    /// Fact dependency (task sets facts another uses)
    Fact,
}

impl fmt::Display for DependencyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DependencyKind::Explicit => write!(f, "explicit"),
            DependencyKind::Variable => write!(f, "variable"),
            DependencyKind::Resource => write!(f, "resource"),
            DependencyKind::Handler => write!(f, "handler"),
            DependencyKind::Role => write!(f, "role"),
            DependencyKind::Include => write!(f, "include"),
            DependencyKind::Fact => write!(f, "fact"),
        }
    }
}

/// An edge in the dependency graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge {
    /// Source node (the dependent)
    pub from: String,
    /// Target node (the dependency)
    pub to: String,
    /// Kind of dependency
    pub kind: DependencyKind,
    /// Optional description of the dependency
    pub description: Option<String>,
}

impl DependencyEdge {
    /// Create a new dependency edge
    pub fn new(from: impl Into<String>, to: impl Into<String>, kind: DependencyKind) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind,
            description: None,
        }
    }

    /// Add a description to the edge
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Type of node in the dependency graph
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    /// A task node
    Task,
    /// A handler node
    Handler,
    /// A role node
    Role,
    /// A variable/fact node
    Variable,
    /// A file/resource node
    Resource,
    /// A play node
    Play,
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeType::Task => write!(f, "task"),
            NodeType::Handler => write!(f, "handler"),
            NodeType::Role => write!(f, "role"),
            NodeType::Variable => write!(f, "variable"),
            NodeType::Resource => write!(f, "resource"),
            NodeType::Play => write!(f, "play"),
        }
    }
}

/// A node in the dependency graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyNode {
    /// Unique identifier for the node
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Type of node
    pub node_type: NodeType,
    /// Associated metadata
    pub metadata: IndexMap<String, JsonValue>,
    /// Outgoing dependencies (nodes this depends on)
    pub depends_on: HashSet<String>,
    /// Incoming dependencies (nodes that depend on this)
    pub depended_by: HashSet<String>,
}

impl DependencyNode {
    /// Create a new task node
    pub fn task(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: Self::generate_id(&name),
            name,
            node_type: NodeType::Task,
            metadata: IndexMap::new(),
            depends_on: HashSet::new(),
            depended_by: HashSet::new(),
        }
    }

    /// Create a new handler node
    pub fn handler(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: format!("handler:{}", Self::generate_id(&name)),
            name,
            node_type: NodeType::Handler,
            metadata: IndexMap::new(),
            depends_on: HashSet::new(),
            depended_by: HashSet::new(),
        }
    }

    /// Create a new role node
    pub fn role(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: format!("role:{}", Self::generate_id(&name)),
            name,
            node_type: NodeType::Role,
            metadata: IndexMap::new(),
            depends_on: HashSet::new(),
            depended_by: HashSet::new(),
        }
    }

    /// Create a new variable node
    pub fn variable(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: format!("var:{}", name),
            name,
            node_type: NodeType::Variable,
            metadata: IndexMap::new(),
            depends_on: HashSet::new(),
            depended_by: HashSet::new(),
        }
    }

    /// Create a new resource node
    pub fn resource(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            id: format!("resource:{}", path),
            name: path,
            node_type: NodeType::Resource,
            metadata: IndexMap::new(),
            depends_on: HashSet::new(),
            depended_by: HashSet::new(),
        }
    }

    /// Generate a sanitized ID from a name
    fn generate_id(name: &str) -> String {
        name.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .to_lowercase()
    }

    /// Add metadata to the node
    pub fn with_metadata(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Check if this node has any dependencies
    pub fn has_dependencies(&self) -> bool {
        !self.depends_on.is_empty()
    }

    /// Check if this node is depended on by others
    pub fn is_dependency(&self) -> bool {
        !self.depended_by.is_empty()
    }
}

// ============================================================================
// Dependency Graph
// ============================================================================

/// A directed acyclic graph (DAG) for tracking resource dependencies
///
/// The graph supports:
/// - Adding nodes and edges
/// - Detecting circular dependencies
/// - Topological sorting for execution order
/// - Visualization in DOT and Mermaid formats
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    /// All nodes in the graph, keyed by ID
    nodes: IndexMap<String, DependencyNode>,
    /// All edges in the graph
    edges: Vec<DependencyEdge>,
    /// Cache of variable producers (variable name -> node id)
    variable_producers: HashMap<String, String>,
    /// Cache of resource producers (resource path -> node id)
    resource_producers: HashMap<String, String>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node to the graph
    ///
    /// If a node with the same ID already exists, it will be updated.
    pub fn add_node(&mut self, id: impl Into<String>, node: DependencyNode) {
        let id = id.into();
        self.nodes.insert(id, node);
    }

    /// Get a node by ID
    pub fn get_node(&self, id: &str) -> Option<&DependencyNode> {
        self.nodes.get(id)
    }

    /// Get a mutable reference to a node by ID
    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut DependencyNode> {
        self.nodes.get_mut(id)
    }

    /// Check if a node exists
    pub fn has_node(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    /// Remove a node and all its edges
    pub fn remove_node(&mut self, id: &str) -> Option<DependencyNode> {
        if let Some(node) = self.nodes.swap_remove(id) {
            // Remove edges involving this node
            self.edges.retain(|e| e.from != id && e.to != id);

            // Update other nodes' dependency sets
            for other in self.nodes.values_mut() {
                other.depends_on.remove(id);
                other.depended_by.remove(id);
            }

            Some(node)
        } else {
            None
        }
    }

    /// Add a dependency between two nodes
    ///
    /// The `from` node will depend on the `to` node.
    pub fn add_dependency(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        kind: DependencyKind,
    ) -> DependencyResult<()> {
        let from = from.into();
        let to = to.into();

        // Check for self-dependency
        if from == to {
            return Err(DependencyError::SelfDependency(from));
        }

        // Check that both nodes exist
        if !self.nodes.contains_key(&from) {
            return Err(DependencyError::NodeNotFound(from));
        }
        if !self.nodes.contains_key(&to) {
            return Err(DependencyError::NodeNotFound(to));
        }

        // Check if adding this edge would create a cycle
        if self.would_create_cycle(&from, &to) {
            let cycle = self.find_cycle_path(&from, &to);
            return Err(DependencyError::CircularDependency(cycle));
        }

        // Add the edge
        self.edges.push(DependencyEdge::new(&from, &to, kind));

        // Update node dependency sets
        if let Some(from_node) = self.nodes.get_mut(&from) {
            from_node.depends_on.insert(to.clone());
        }
        if let Some(to_node) = self.nodes.get_mut(&to) {
            to_node.depended_by.insert(from);
        }

        Ok(())
    }

    /// Add a dependency with a description
    pub fn add_dependency_with_desc(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        kind: DependencyKind,
        description: impl Into<String>,
    ) -> DependencyResult<()> {
        let from = from.into();
        let to = to.into();
        let desc = description.into();

        self.add_dependency(&from, &to, kind)?;

        // Update the edge with description
        if let Some(edge) = self.edges.last_mut() {
            edge.description = Some(desc);
        }

        Ok(())
    }

    /// Check if adding an edge would create a cycle
    fn would_create_cycle(&self, from: &str, to: &str) -> bool {
        // If 'to' can reach 'from', adding from->to would create a cycle
        self.can_reach(to, from)
    }

    /// Check if there's a path from `start` to `end`
    fn can_reach(&self, start: &str, end: &str) -> bool {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start.to_string());

        while let Some(current) = queue.pop_front() {
            if current == end {
                return true;
            }

            if visited.insert(current.clone()) {
                if let Some(node) = self.nodes.get(&current) {
                    for dep in &node.depends_on {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        false
    }

    /// Find the cycle path when adding from -> to would create a cycle
    fn find_cycle_path(&self, from: &str, to: &str) -> Vec<String> {
        // Find path from 'to' back to 'from'
        let mut path = vec![from.to_string()];
        let mut visited = HashSet::new();

        if self.find_path_dfs(to, from, &mut path, &mut visited) {
            path.insert(0, from.to_string());
        }

        path
    }

    /// DFS helper to find a path between nodes
    fn find_path_dfs(
        &self,
        current: &str,
        target: &str,
        path: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) -> bool {
        path.push(current.to_string());

        if current == target {
            return true;
        }

        if visited.insert(current.to_string()) {
            if let Some(node) = self.nodes.get(current) {
                for dep in &node.depends_on {
                    if self.find_path_dfs(dep, target, path, visited) {
                        return true;
                    }
                }
            }
        }

        path.pop();
        false
    }

    /// Register a variable producer
    pub fn register_variable_producer(
        &mut self,
        variable: impl Into<String>,
        producer: impl Into<String>,
    ) {
        self.variable_producers
            .insert(variable.into(), producer.into());
    }

    /// Get the producer of a variable
    pub fn get_variable_producer(&self, variable: &str) -> Option<&String> {
        self.variable_producers.get(variable)
    }

    /// Register a resource producer
    pub fn register_resource_producer(
        &mut self,
        resource: impl Into<String>,
        producer: impl Into<String>,
    ) {
        self.resource_producers
            .insert(resource.into(), producer.into());
    }

    /// Get the producer of a resource
    pub fn get_resource_producer(&self, resource: &str) -> Option<&String> {
        self.resource_producers.get(resource)
    }

    /// Perform topological sort to get execution order
    ///
    /// Returns nodes in an order where all dependencies come before their dependents.
    pub fn topological_sort(&self) -> DependencyResult<Vec<String>> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new();
        let mut cycle_path = Vec::new();

        // Visit each node
        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                self.topo_visit(
                    node_id,
                    &mut visited,
                    &mut temp_visited,
                    &mut result,
                    &mut cycle_path,
                )?;
            }
        }

        Ok(result)
    }

    /// DFS visitor for topological sort
    fn topo_visit(
        &self,
        node_id: &str,
        visited: &mut HashSet<String>,
        temp_visited: &mut HashSet<String>,
        result: &mut Vec<String>,
        cycle_path: &mut Vec<String>,
    ) -> DependencyResult<()> {
        // Check for cycle
        if temp_visited.contains(node_id) {
            cycle_path.push(node_id.to_string());
            return Err(DependencyError::CircularDependency(cycle_path.clone()));
        }

        // Already processed
        if visited.contains(node_id) {
            return Ok(());
        }

        temp_visited.insert(node_id.to_string());
        cycle_path.push(node_id.to_string());

        // Visit all dependencies first
        if let Some(node) = self.nodes.get(node_id) {
            for dep_id in &node.depends_on {
                self.topo_visit(dep_id, visited, temp_visited, result, cycle_path)?;
            }
        }

        temp_visited.remove(node_id);
        cycle_path.pop();
        visited.insert(node_id.to_string());
        result.push(node_id.to_string());

        Ok(())
    }

    /// Get all nodes that depend on the given node (directly or transitively)
    pub fn get_dependents(&self, node_id: &str) -> HashSet<String> {
        let mut dependents = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(node_id.to_string());

        while let Some(current) = queue.pop_front() {
            if let Some(node) = self.nodes.get(&current) {
                for dep in &node.depended_by {
                    if dependents.insert(dep.clone()) {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        dependents
    }

    /// Get all dependencies of a node (directly or transitively)
    pub fn get_all_dependencies(&self, node_id: &str) -> HashSet<String> {
        let mut dependencies = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(node_id.to_string());

        while let Some(current) = queue.pop_front() {
            if let Some(node) = self.nodes.get(&current) {
                for dep in &node.depends_on {
                    if dependencies.insert(dep.clone()) {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        dependencies
    }

    /// Get root nodes (nodes with no dependencies)
    pub fn get_roots(&self) -> Vec<&String> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.depends_on.is_empty())
            .map(|(id, _)| id)
            .collect()
    }

    /// Get leaf nodes (nodes that nothing depends on)
    pub fn get_leaves(&self) -> Vec<&String> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.depended_by.is_empty())
            .map(|(id, _)| id)
            .collect()
    }

    /// Get the number of nodes in the graph
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges in the graph
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Check if the graph is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get all nodes
    pub fn nodes(&self) -> impl Iterator<Item = (&String, &DependencyNode)> {
        self.nodes.iter()
    }

    /// Get all edges
    pub fn edges(&self) -> &[DependencyEdge] {
        &self.edges
    }

    /// Clear the graph
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.variable_producers.clear();
        self.resource_producers.clear();
    }

    // ========================================================================
    // Visualization
    // ========================================================================

    /// Generate DOT format output for Graphviz visualization
    pub fn to_dot(&self) -> String {
        let mut output = String::new();
        output.push_str("digraph DependencyGraph {\n");
        output.push_str("    rankdir=TB;\n");
        output.push_str("    node [shape=box, style=rounded];\n\n");

        // Define node styles by type
        output.push_str("    // Node styles\n");
        for (id, node) in &self.nodes {
            let shape = match node.node_type {
                NodeType::Task => "box",
                NodeType::Handler => "diamond",
                NodeType::Role => "ellipse",
                NodeType::Variable => "parallelogram",
                NodeType::Resource => "folder",
                NodeType::Play => "house",
            };
            let color = match node.node_type {
                NodeType::Task => "#4CAF50",     // Green
                NodeType::Handler => "#FF9800",  // Orange
                NodeType::Role => "#2196F3",     // Blue
                NodeType::Variable => "#9C27B0", // Purple
                NodeType::Resource => "#795548", // Brown
                NodeType::Play => "#607D8B",     // Blue Grey
            };
            output.push_str(&format!(
                "    \"{}\" [label=\"{}\\n({})\", shape={}, fillcolor=\"{}\", style=\"filled,rounded\"];\n",
                id, node.name, node.node_type, shape, color
            ));
        }

        output.push_str("\n    // Edges\n");
        for edge in &self.edges {
            let style = match edge.kind {
                DependencyKind::Explicit => "solid",
                DependencyKind::Variable => "dashed",
                DependencyKind::Resource => "dotted",
                DependencyKind::Handler => "bold",
                DependencyKind::Role => "solid",
                DependencyKind::Include => "dashed",
                DependencyKind::Fact => "dotted",
            };
            let color = match edge.kind {
                DependencyKind::Explicit => "#333333",
                DependencyKind::Variable => "#9C27B0",
                DependencyKind::Resource => "#795548",
                DependencyKind::Handler => "#FF9800",
                DependencyKind::Role => "#2196F3",
                DependencyKind::Include => "#009688",
                DependencyKind::Fact => "#E91E63",
            };
            let kind_str = edge.kind.to_string();
            let label = edge.description.as_deref().unwrap_or(&kind_str);
            output.push_str(&format!(
                "    \"{}\" -> \"{}\" [label=\"{}\", style={}, color=\"{}\"];\n",
                edge.from, edge.to, label, style, color
            ));
        }

        output.push_str("}\n");
        output
    }

    /// Generate Mermaid format output for documentation
    pub fn to_mermaid(&self) -> String {
        let mut output = String::new();
        output.push_str("graph TD\n");

        // Define nodes with shapes
        for (id, node) in &self.nodes {
            let (open, close) = match node.node_type {
                NodeType::Task => ("[", "]"),
                NodeType::Handler => ("{", "}"),
                NodeType::Role => ("([", "])"),
                NodeType::Variable => ("[/", "/]"),
                NodeType::Resource => ("[(", ")]"),
                NodeType::Play => ("{{", "}}"),
            };
            // Sanitize ID for Mermaid (replace special chars)
            let safe_id = id.replace([':', '-'], "_");
            output.push_str(&format!("    {}{}{}{};\n", safe_id, open, node.name, close));
        }

        output.push('\n');

        // Define edges
        for edge in &self.edges {
            let safe_from = edge.from.replace([':', '-'], "_");
            let safe_to = edge.to.replace([':', '-'], "_");
            let arrow = match edge.kind {
                DependencyKind::Explicit => "-->",
                DependencyKind::Variable => "-.->",
                DependencyKind::Resource => "-.-",
                DependencyKind::Handler => "==>",
                DependencyKind::Role => "-->",
                DependencyKind::Include => "-.->",
                DependencyKind::Fact => "-.-",
            };
            if let Some(desc) = &edge.description {
                output.push_str(&format!(
                    "    {} {}|{}| {};\n",
                    safe_from, arrow, desc, safe_to
                ));
            } else {
                output.push_str(&format!("    {} {} {};\n", safe_from, arrow, safe_to));
            }
        }

        output
    }

    /// Generate a simple text representation
    pub fn to_text(&self) -> String {
        let mut output = String::new();
        output.push_str("Dependency Graph:\n");
        output.push_str(&format!("  Nodes: {}\n", self.nodes.len()));
        output.push_str(&format!("  Edges: {}\n", self.edges.len()));
        output.push('\n');

        if let Ok(order) = self.topological_sort() {
            output.push_str("Execution Order:\n");
            for (i, node_id) in order.iter().enumerate() {
                if let Some(node) = self.nodes.get(node_id) {
                    output.push_str(&format!(
                        "  {}. {} ({})\n",
                        i + 1,
                        node.name,
                        node.node_type
                    ));
                    if !node.depends_on.is_empty() {
                        let deps: Vec<&str> = node.depends_on.iter().map(|s| s.as_str()).collect();
                        output.push_str(&format!("      depends on: {}\n", deps.join(", ")));
                    }
                }
            }
        }

        output
    }
}

// ============================================================================
// Dependency Analyzer
// ============================================================================

/// Analyzes tasks to extract implicit dependencies
pub struct DependencyAnalyzer {
    graph: DependencyGraph,
}

impl DependencyAnalyzer {
    /// Create a new analyzer
    pub fn new() -> Self {
        Self {
            graph: DependencyGraph::new(),
        }
    }

    /// Create an analyzer with an existing graph
    pub fn with_graph(graph: DependencyGraph) -> Self {
        Self { graph }
    }

    /// Analyze a list of tasks and build the dependency graph
    pub fn analyze_tasks(&mut self, tasks: &[Task]) -> DependencyResult<&DependencyGraph> {
        // First pass: add all tasks as nodes and track variable producers
        for (index, task) in tasks.iter().enumerate() {
            let task_id = format!("task_{}", index);
            let mut node = DependencyNode::task(&task.name);
            node.id = task_id.clone();
            node.metadata
                .insert("index".to_string(), JsonValue::Number(index.into()));
            node.metadata
                .insert("module".to_string(), JsonValue::String(task.module.clone()));

            self.graph.add_node(&task_id, node);

            // Track registered variables
            if let Some(ref register) = task.register {
                self.graph
                    .register_variable_producer(register.clone(), &task_id);
            }
        }

        // Second pass: extract dependencies
        for (index, task) in tasks.iter().enumerate() {
            let task_id = format!("task_{}", index);

            // Analyze variable usage in task arguments
            let used_vars = self.extract_variables_from_task(task);
            for var in used_vars {
                if let Some(producer_id) = self.graph.get_variable_producer(&var).cloned() {
                    if producer_id != task_id {
                        self.graph.add_dependency_with_desc(
                            &task_id,
                            &producer_id,
                            DependencyKind::Variable,
                            format!("uses variable '{}'", var),
                        )?;
                    }
                }
            }

            // Handler notifications create implicit ordering
            for handler_name in &task.notify {
                let handler_id =
                    format!("handler:{}", handler_name.to_lowercase().replace(' ', "_"));
                if !self.graph.has_node(&handler_id) {
                    self.graph
                        .add_node(&handler_id, DependencyNode::handler(handler_name));
                }
                // Handler depends on the task that notifies it
                self.graph.add_dependency_with_desc(
                    &handler_id,
                    &task_id,
                    DependencyKind::Handler,
                    format!("notified by '{}'", task.name),
                )?;
            }
        }

        Ok(&self.graph)
    }

    /// Extract variable names used in a task (from templates)
    fn extract_variables_from_task(&self, task: &Task) -> HashSet<String> {
        let mut variables = HashSet::new();

        // Extract from args
        for value in task.args.values() {
            self.extract_variables_from_value(value, &mut variables);
        }

        // Extract from when condition
        if let Some(ref when) = task.when {
            self.extract_variables_from_string(when, &mut variables);
        }

        variables
    }

    /// Extract variable names from a JSON value
    fn extract_variables_from_value(&self, value: &JsonValue, vars: &mut HashSet<String>) {
        match value {
            JsonValue::String(s) => {
                self.extract_variables_from_string(s, vars);
            }
            JsonValue::Array(arr) => {
                for item in arr {
                    self.extract_variables_from_value(item, vars);
                }
            }
            JsonValue::Object(obj) => {
                for v in obj.values() {
                    self.extract_variables_from_value(v, vars);
                }
            }
            _ => {}
        }
    }

    /// Extract variable names from a template string
    fn extract_variables_from_string(&self, s: &str, vars: &mut HashSet<String>) {
        // Match {{ variable }} patterns
        let re = regex::Regex::new(
            r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)*)\s*(?:[|}\s])",
        )
        .unwrap();
        for cap in re.captures_iter(s) {
            if let Some(m) = cap.get(1) {
                // Get the root variable name (before any dots)
                let var_name = m.as_str().split('.').next().unwrap_or(m.as_str());
                vars.insert(var_name.to_string());
            }
        }
    }

    /// Get the resulting dependency graph
    pub fn into_graph(self) -> DependencyGraph {
        self.graph
    }

    /// Get a reference to the dependency graph
    pub fn graph(&self) -> &DependencyGraph {
        &self.graph
    }
}

impl Default for DependencyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_graph_basic() {
        let mut graph = DependencyGraph::new();

        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph.add_node("c", DependencyNode::task("Task C"));

        assert!(graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .is_ok());
        assert!(graph
            .add_dependency("c", "b", DependencyKind::Explicit)
            .is_ok());

        let order = graph.topological_sort().unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut graph = DependencyGraph::new();

        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph.add_node("c", DependencyNode::task("Task C"));

        assert!(graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .is_ok());
        assert!(graph
            .add_dependency("c", "b", DependencyKind::Explicit)
            .is_ok());

        // This should fail - creates a->b->c->a cycle
        let result = graph.add_dependency("a", "c", DependencyKind::Explicit);
        assert!(matches!(
            result,
            Err(DependencyError::CircularDependency(_))
        ));
    }

    #[test]
    fn test_self_dependency_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_node("a", DependencyNode::task("Task A"));

        let result = graph.add_dependency("a", "a", DependencyKind::Explicit);
        assert!(matches!(result, Err(DependencyError::SelfDependency(_))));
    }

    #[test]
    fn test_node_not_found() {
        let mut graph = DependencyGraph::new();
        graph.add_node("a", DependencyNode::task("Task A"));

        let result = graph.add_dependency("a", "nonexistent", DependencyKind::Explicit);
        assert!(matches!(result, Err(DependencyError::NodeNotFound(_))));
    }

    #[test]
    fn test_complex_graph() {
        let mut graph = DependencyGraph::new();

        // Create a diamond dependency:
        //     a
        //    / \
        //   b   c
        //    \ /
        //     d

        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph.add_node("c", DependencyNode::task("Task C"));
        graph.add_node("d", DependencyNode::task("Task D"));

        assert!(graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .is_ok());
        assert!(graph
            .add_dependency("c", "a", DependencyKind::Explicit)
            .is_ok());
        assert!(graph
            .add_dependency("d", "b", DependencyKind::Explicit)
            .is_ok());
        assert!(graph
            .add_dependency("d", "c", DependencyKind::Explicit)
            .is_ok());

        let order = graph.topological_sort().unwrap();

        // a must come first, d must come last
        assert_eq!(order.first(), Some(&"a".to_string()));
        assert_eq!(order.last(), Some(&"d".to_string()));

        // b and c must come after a but before d
        let a_pos = order.iter().position(|x| x == "a").unwrap();
        let b_pos = order.iter().position(|x| x == "b").unwrap();
        let c_pos = order.iter().position(|x| x == "c").unwrap();
        let d_pos = order.iter().position(|x| x == "d").unwrap();

        assert!(a_pos < b_pos);
        assert!(a_pos < c_pos);
        assert!(b_pos < d_pos);
        assert!(c_pos < d_pos);
    }

    #[test]
    fn test_get_dependents() {
        let mut graph = DependencyGraph::new();

        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph.add_node("c", DependencyNode::task("Task C"));

        graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .unwrap();
        graph
            .add_dependency("c", "b", DependencyKind::Explicit)
            .unwrap();

        let dependents_of_a = graph.get_dependents("a");
        assert!(dependents_of_a.contains("b"));
        assert!(dependents_of_a.contains("c"));

        let dependents_of_c = graph.get_dependents("c");
        assert!(dependents_of_c.is_empty());
    }

    #[test]
    fn test_get_all_dependencies() {
        let mut graph = DependencyGraph::new();

        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph.add_node("c", DependencyNode::task("Task C"));

        graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .unwrap();
        graph
            .add_dependency("c", "b", DependencyKind::Explicit)
            .unwrap();

        let deps_of_c = graph.get_all_dependencies("c");
        assert!(deps_of_c.contains("a"));
        assert!(deps_of_c.contains("b"));

        let deps_of_a = graph.get_all_dependencies("a");
        assert!(deps_of_a.is_empty());
    }

    #[test]
    fn test_roots_and_leaves() {
        let mut graph = DependencyGraph::new();

        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph.add_node("c", DependencyNode::task("Task C"));

        graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .unwrap();
        graph
            .add_dependency("c", "b", DependencyKind::Explicit)
            .unwrap();

        let roots: Vec<_> = graph.get_roots().into_iter().cloned().collect();
        assert_eq!(roots, vec!["a"]);

        let leaves: Vec<_> = graph.get_leaves().into_iter().cloned().collect();
        assert_eq!(leaves, vec!["c"]);
    }

    #[test]
    fn test_visualization_dot() {
        let mut graph = DependencyGraph::new();
        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .unwrap();

        let dot = graph.to_dot();
        assert!(dot.contains("digraph"));
        assert!(dot.contains("Task A"));
        assert!(dot.contains("Task B"));
        assert!(dot.contains("->"));
    }

    #[test]
    fn test_visualization_mermaid() {
        let mut graph = DependencyGraph::new();
        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .unwrap();

        let mermaid = graph.to_mermaid();
        assert!(mermaid.contains("graph TD"));
        assert!(mermaid.contains("Task A"));
        assert!(mermaid.contains("Task B"));
        assert!(mermaid.contains("-->"));
    }

    #[test]
    fn test_variable_producers() {
        let mut graph = DependencyGraph::new();
        graph.add_node("task1", DependencyNode::task("Task 1"));
        graph.register_variable_producer("my_var", "task1");

        assert_eq!(
            graph.get_variable_producer("my_var"),
            Some(&"task1".to_string())
        );
        assert_eq!(graph.get_variable_producer("other_var"), None);
    }

    #[test]
    fn test_remove_node() {
        let mut graph = DependencyGraph::new();
        graph.add_node("a", DependencyNode::task("Task A"));
        graph.add_node("b", DependencyNode::task("Task B"));
        graph
            .add_dependency("b", "a", DependencyKind::Explicit)
            .unwrap();

        let removed = graph.remove_node("a");
        assert!(removed.is_some());
        assert!(!graph.has_node("a"));
        assert!(graph.edges().is_empty());

        // b should no longer have any dependencies
        let b = graph.get_node("b").unwrap();
        assert!(b.depends_on.is_empty());
    }
}
