//! Dependency Analysis
//!
//! This module provides analysis of task dependencies, role dependencies,
//! and execution order within playbooks.

use super::{helpers, AnalysisCategory, AnalysisFinding, AnalysisResult, Severity, SourceLocation};
use crate::playbook::{Play, Playbook, Task};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Type of dependency
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DependencyType {
    /// Variable dependency (task uses variable set by another task)
    Variable,
    /// Handler notification dependency
    Handler,
    /// File dependency (task uses file created by another task)
    File,
    /// Service dependency (task depends on service started by another task)
    Service,
    /// Role dependency
    Role,
    /// Explicit ordering (using when: previous_result)
    Explicit,
    /// Implicit ordering (task order in playbook)
    Implicit,
}

impl std::fmt::Display for DependencyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DependencyType::Variable => write!(f, "variable"),
            DependencyType::Handler => write!(f, "handler"),
            DependencyType::File => write!(f, "file"),
            DependencyType::Service => write!(f, "service"),
            DependencyType::Role => write!(f, "role"),
            DependencyType::Explicit => write!(f, "explicit"),
            DependencyType::Implicit => write!(f, "implicit"),
        }
    }
}

/// A single dependency edge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge {
    /// Source node (depends on target)
    pub from: NodeId,
    /// Target node (depended upon)
    pub to: NodeId,
    /// Type of dependency
    pub dependency_type: DependencyType,
    /// Description of the dependency
    pub description: String,
}

/// Unique identifier for a node in the dependency graph
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId {
    /// Play index
    pub play_index: usize,
    /// Task type (pre_tasks, tasks, post_tasks, handlers)
    pub task_type: String,
    /// Task index within the type
    pub task_index: usize,
}

impl NodeId {
    pub fn new(play_index: usize, task_type: impl Into<String>, task_index: usize) -> Self {
        Self {
            play_index,
            task_type: task_type.into(),
            task_index,
        }
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "play[{}].{}[{}]",
            self.play_index, self.task_type, self.task_index
        )
    }
}

/// Node in the dependency graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyNode {
    /// Node identifier
    pub id: NodeId,
    /// Task name
    pub name: String,
    /// Module name
    pub module: String,
    /// Variables this node defines
    pub defines: HashSet<String>,
    /// Variables this node uses
    pub uses: HashSet<String>,
    /// Handlers this node notifies
    pub notifies: HashSet<String>,
}

/// Dependency graph for a playbook
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGraph {
    /// All nodes in the graph
    pub nodes: HashMap<NodeId, DependencyNode>,
    /// All edges in the graph
    pub edges: Vec<DependencyEdge>,
    /// Adjacency list (node -> nodes it depends on)
    pub dependencies: HashMap<NodeId, Vec<NodeId>>,
    /// Reverse adjacency list (node -> nodes that depend on it)
    pub dependents: HashMap<NodeId, Vec<NodeId>>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            dependencies: HashMap::new(),
            dependents: HashMap::new(),
        }
    }

    /// Add a node to the graph
    pub fn add_node(&mut self, node: DependencyNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Add an edge to the graph
    pub fn add_edge(&mut self, edge: DependencyEdge) {
        self.dependencies
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to.clone());
        self.dependents
            .entry(edge.to.clone())
            .or_default()
            .push(edge.from.clone());
        self.edges.push(edge);
    }

    /// Get all nodes that a given node depends on
    pub fn get_dependencies(&self, node_id: &NodeId) -> Vec<&DependencyNode> {
        self.dependencies
            .get(node_id)
            .map(|deps| deps.iter().filter_map(|id| self.nodes.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get all nodes that depend on a given node
    pub fn get_dependents(&self, node_id: &NodeId) -> Vec<&DependencyNode> {
        self.dependents
            .get(node_id)
            .map(|deps| deps.iter().filter_map(|id| self.nodes.get(id)).collect())
            .unwrap_or_default()
    }

    /// Check for circular dependencies
    pub fn find_cycles(&self) -> Vec<Vec<NodeId>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                self.dfs_cycle(
                    node_id,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        cycles
    }

    fn dfs_cycle(
        &self,
        node_id: &NodeId,
        visited: &mut HashSet<NodeId>,
        rec_stack: &mut HashSet<NodeId>,
        path: &mut Vec<NodeId>,
        cycles: &mut Vec<Vec<NodeId>>,
    ) {
        visited.insert(node_id.clone());
        rec_stack.insert(node_id.clone());
        path.push(node_id.clone());

        if let Some(deps) = self.dependencies.get(node_id) {
            for dep in deps {
                if !visited.contains(dep) {
                    self.dfs_cycle(dep, visited, rec_stack, path, cycles);
                } else if rec_stack.contains(dep) {
                    // Found a cycle
                    let cycle_start = path.iter().position(|n| n == dep).unwrap();
                    cycles.push(path[cycle_start..].to_vec());
                }
            }
        }

        path.pop();
        rec_stack.remove(node_id);
    }

    /// Get topological ordering of nodes (if no cycles)
    pub fn topological_order(&self) -> Option<Vec<NodeId>> {
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();

        // Initialize in-degrees
        for node_id in self.nodes.keys() {
            in_degree.insert(node_id.clone(), 0);
        }

        // Calculate in-degrees
        for deps in self.dependencies.values() {
            for dep in deps {
                *in_degree.entry(dep.clone()).or_insert(0) += 1;
            }
        }

        // Start with nodes that have no dependencies
        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(id, _)| id.clone())
            .collect();

        let mut result = Vec::new();

        while let Some(node_id) = queue.pop_front() {
            result.push(node_id.clone());

            if let Some(deps) = self.dependencies.get(&node_id) {
                for dep in deps {
                    if let Some(degree) = in_degree.get_mut(dep) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dep.clone());
                        }
                    }
                }
            }
        }

        if result.len() == self.nodes.len() {
            Some(result)
        } else {
            None // Cycle exists
        }
    }

    /// Get nodes with no dependencies (entry points)
    pub fn get_entry_points(&self) -> Vec<&DependencyNode> {
        self.nodes
            .iter()
            .filter(|(id, _)| {
                self.dependencies
                    .get(*id)
                    .map(|deps| deps.is_empty())
                    .unwrap_or(true)
            })
            .map(|(_, node)| node)
            .collect()
    }

    /// Get nodes with no dependents (exit points)
    pub fn get_exit_points(&self) -> Vec<&DependencyNode> {
        self.nodes
            .iter()
            .filter(|(id, _)| {
                self.dependents
                    .get(*id)
                    .map(|deps| deps.is_empty())
                    .unwrap_or(true)
            })
            .map(|(_, node)| node)
            .collect()
    }

    /// Calculate the critical path (longest dependency chain)
    pub fn critical_path(&self) -> Vec<NodeId> {
        let mut longest: HashMap<NodeId, (usize, Vec<NodeId>)> = HashMap::new();

        // Process nodes in topological order
        if let Some(order) = self.topological_order() {
            for node_id in order {
                let mut max_len = 0;
                let mut max_path = vec![node_id.clone()];

                if let Some(deps) = self.dependencies.get(&node_id) {
                    for dep in deps {
                        if let Some((len, path)) = longest.get(dep) {
                            if len + 1 > max_len {
                                max_len = len + 1;
                                max_path = path.clone();
                                max_path.push(node_id.clone());
                            }
                        }
                    }
                }

                longest.insert(node_id, (max_len, max_path));
            }
        }

        longest
            .into_values()
            .max_by_key(|(len, _)| *len)
            .map(|(_, path)| path)
            .unwrap_or_default()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Dependency analyzer
pub struct DependencyAnalyzer {
    /// Track variable dependencies
    track_variables: bool,
    /// Track handler dependencies
    track_handlers: bool,
}

impl DependencyAnalyzer {
    pub fn new() -> Self {
        Self {
            track_variables: true,
            track_handlers: true,
        }
    }

    /// Analyze a playbook and build its dependency graph
    pub fn analyze(
        &self,
        playbook: &Playbook,
    ) -> AnalysisResult<(DependencyGraph, Vec<AnalysisFinding>)> {
        let mut graph = DependencyGraph::new();
        let mut findings = Vec::new();
        let source_file = playbook
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        // Build nodes for all tasks
        for (play_idx, play) in playbook.plays.iter().enumerate() {
            self.add_tasks_as_nodes(&mut graph, play_idx, "pre_tasks", &play.pre_tasks);
            self.add_tasks_as_nodes(&mut graph, play_idx, "tasks", &play.tasks);
            self.add_tasks_as_nodes(&mut graph, play_idx, "post_tasks", &play.post_tasks);
        }

        // Build edges based on dependencies
        if self.track_variables {
            self.add_variable_dependencies(&mut graph);
        }

        if self.track_handlers {
            for (play_idx, play) in playbook.plays.iter().enumerate() {
                self.add_handler_dependencies(&mut graph, play_idx, play);
            }
        }

        // Check for issues
        // Circular dependencies
        let cycles = graph.find_cycles();
        for cycle in &cycles {
            let cycle_str = cycle
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(" -> ");

            let first_node = cycle.first().unwrap();
            let location = SourceLocation::new()
                .with_play(
                    first_node.play_index,
                    format!("play[{}]", first_node.play_index),
                )
                .with_task(
                    first_node.task_index,
                    format!("task[{}]", first_node.task_index),
                );
            let location = if let Some(f) = &source_file {
                location.with_file(f.clone())
            } else {
                location
            };

            findings.push(
                AnalysisFinding::new(
                    "DEP001",
                    AnalysisCategory::Dependency,
                    Severity::Error,
                    "Circular dependency detected",
                )
                .with_description(format!("Cycle: {}", cycle_str))
                .with_location(location)
                .with_suggestion(
                    "Break the circular dependency by restructuring tasks or using explicit ordering."
                ),
            );
        }

        // Missing dependencies (variables used before defined)
        findings.extend(self.check_missing_dependencies(&graph, &source_file)?);

        Ok((graph, findings))
    }

    /// Add tasks as nodes to the graph
    fn add_tasks_as_nodes(
        &self,
        graph: &mut DependencyGraph,
        play_idx: usize,
        task_type: &str,
        tasks: &[Task],
    ) {
        for (task_idx, task) in tasks.iter().enumerate() {
            let node_id = NodeId::new(play_idx, task_type, task_idx);

            let mut defines = HashSet::new();
            let mut uses = HashSet::new();
            let mut notifies = HashSet::new();

            // Register variable
            if let Some(reg) = &task.register {
                defines.insert(reg.clone());
            }

            // set_fact module
            if task.module.name == "set_fact" || task.module.name == "ansible.builtin.set_fact" {
                if let Some(obj) = task.module.args.as_object() {
                    for key in obj.keys() {
                        if key != "cacheable" {
                            defines.insert(key.clone());
                        }
                    }
                }
            }

            // Variables used in module args
            uses.extend(helpers::extract_value_variables(&task.module.args));

            // Variables used in when conditions
            if let Some(when) = &task.when {
                for condition in when.conditions() {
                    uses.extend(helpers::extract_when_variables(condition));
                }
            }

            // Variables used in loop
            if let Some(loop_expr) = &task.loop_ {
                uses.extend(helpers::extract_value_variables(loop_expr));
            }

            // Handlers notified
            notifies.extend(task.notify.iter().cloned());

            let node = DependencyNode {
                id: node_id.clone(),
                name: task.name.clone(),
                module: task.module.name.clone(),
                defines,
                uses,
                notifies,
            };

            graph.add_node(node);
        }
    }

    /// Add variable-based dependencies
    fn add_variable_dependencies(&self, graph: &mut DependencyGraph) {
        // Build a map of variable -> defining node
        let mut var_definitions: HashMap<String, NodeId> = HashMap::new();

        for (node_id, node) in &graph.nodes {
            for var in &node.defines {
                var_definitions.insert(var.clone(), node_id.clone());
            }
        }

        // Add edges for variable usage
        let edges: Vec<_> = graph
            .nodes
            .iter()
            .flat_map(|(node_id, node)| {
                node.uses.iter().filter_map(|var| {
                    var_definitions.get(var).map(|def_node_id| DependencyEdge {
                        from: node_id.clone(),
                        to: def_node_id.clone(),
                        dependency_type: DependencyType::Variable,
                        description: format!("Uses variable '{}'", var),
                    })
                })
            })
            .collect();

        for edge in edges {
            graph.add_edge(edge);
        }
    }

    /// Add handler notification dependencies
    fn add_handler_dependencies(&self, graph: &mut DependencyGraph, play_idx: usize, play: &Play) {
        // Build handler name -> task mapping
        let handler_names: HashSet<_> = play
            .handlers
            .iter()
            .flat_map(|h| {
                let mut names = vec![h.name.clone()];
                names.extend(h.listen.clone());
                names
            })
            .collect();

        // Add implicit dependency: tasks that notify handlers depend on those handlers being defined
        for (node_id, node) in &graph.nodes {
            if node_id.play_index != play_idx {
                continue;
            }

            for notify in &node.notifies {
                if handler_names.contains(notify) {
                    // This is informational - handlers run after tasks complete
                    // No actual edge needed, but we could track the relationship
                }
            }
        }
    }

    /// Check for variables used before they are defined
    fn check_missing_dependencies(
        &self,
        graph: &DependencyGraph,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        // Collect all defined variables
        let all_definitions: HashSet<_> = graph
            .nodes
            .values()
            .flat_map(|n| n.defines.iter().cloned())
            .collect();

        // Built-in variables that are always available
        let builtins: HashSet<&str> = [
            "item",
            "ansible_loop",
            "ansible_index_var",
            "ansible_facts",
            "inventory_hostname",
            "groups",
            "hostvars",
            "play_hosts",
            "ansible_play_hosts",
            "ansible_check_mode",
            "ansible_diff_mode",
            "omit",
        ]
        .into_iter()
        .collect();

        for (node_id, node) in &graph.nodes {
            for var in &node.uses {
                // Skip if it's defined somewhere or is a builtin
                if all_definitions.contains(var) || builtins.contains(var.as_str()) {
                    continue;
                }

                // Skip ansible_* variables (facts)
                if var.starts_with("ansible_") {
                    continue;
                }

                let location = SourceLocation::new()
                    .with_play(node_id.play_index, format!("play[{}]", node_id.play_index))
                    .with_task(node_id.task_index, &node.name);
                let location = if let Some(f) = source_file {
                    location.with_file(f.clone())
                } else {
                    location
                };

                findings.push(
                    AnalysisFinding::new(
                        "DEP002",
                        AnalysisCategory::Dependency,
                        Severity::Info,
                        format!("Variable '{}' may not be defined before use", var),
                    )
                    .with_description(
                        "This variable is used but not defined by any task in the playbook. \
                         It might be defined in inventory, extra vars, or role defaults.",
                    )
                    .with_location(location),
                );
            }
        }

        Ok(findings)
    }
}

impl Default for DependencyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_display() {
        let node_id = NodeId::new(0, "tasks", 5);
        assert_eq!(format!("{}", node_id), "play[0].tasks[5]");
    }

    #[test]
    fn test_dependency_graph_cycles() {
        let mut graph = DependencyGraph::new();

        // Create a simple cycle: A -> B -> C -> A
        graph.add_node(DependencyNode {
            id: NodeId::new(0, "tasks", 0),
            name: "A".to_string(),
            module: "debug".to_string(),
            defines: HashSet::new(),
            uses: HashSet::new(),
            notifies: HashSet::new(),
        });
        graph.add_node(DependencyNode {
            id: NodeId::new(0, "tasks", 1),
            name: "B".to_string(),
            module: "debug".to_string(),
            defines: HashSet::new(),
            uses: HashSet::new(),
            notifies: HashSet::new(),
        });
        graph.add_node(DependencyNode {
            id: NodeId::new(0, "tasks", 2),
            name: "C".to_string(),
            module: "debug".to_string(),
            defines: HashSet::new(),
            uses: HashSet::new(),
            notifies: HashSet::new(),
        });

        graph.add_edge(DependencyEdge {
            from: NodeId::new(0, "tasks", 0),
            to: NodeId::new(0, "tasks", 1),
            dependency_type: DependencyType::Variable,
            description: "test".to_string(),
        });
        graph.add_edge(DependencyEdge {
            from: NodeId::new(0, "tasks", 1),
            to: NodeId::new(0, "tasks", 2),
            dependency_type: DependencyType::Variable,
            description: "test".to_string(),
        });
        graph.add_edge(DependencyEdge {
            from: NodeId::new(0, "tasks", 2),
            to: NodeId::new(0, "tasks", 0),
            dependency_type: DependencyType::Variable,
            description: "test".to_string(),
        });

        let cycles = graph.find_cycles();
        assert!(!cycles.is_empty());
    }

    #[test]
    fn test_dependency_graph_no_cycles() {
        let mut graph = DependencyGraph::new();

        // Create a DAG: A -> B -> C
        graph.add_node(DependencyNode {
            id: NodeId::new(0, "tasks", 0),
            name: "A".to_string(),
            module: "debug".to_string(),
            defines: HashSet::new(),
            uses: HashSet::new(),
            notifies: HashSet::new(),
        });
        graph.add_node(DependencyNode {
            id: NodeId::new(0, "tasks", 1),
            name: "B".to_string(),
            module: "debug".to_string(),
            defines: HashSet::new(),
            uses: HashSet::new(),
            notifies: HashSet::new(),
        });
        graph.add_node(DependencyNode {
            id: NodeId::new(0, "tasks", 2),
            name: "C".to_string(),
            module: "debug".to_string(),
            defines: HashSet::new(),
            uses: HashSet::new(),
            notifies: HashSet::new(),
        });

        graph.add_edge(DependencyEdge {
            from: NodeId::new(0, "tasks", 1),
            to: NodeId::new(0, "tasks", 0),
            dependency_type: DependencyType::Variable,
            description: "test".to_string(),
        });
        graph.add_edge(DependencyEdge {
            from: NodeId::new(0, "tasks", 2),
            to: NodeId::new(0, "tasks", 1),
            dependency_type: DependencyType::Variable,
            description: "test".to_string(),
        });

        let order = graph.topological_order();
        assert!(order.is_some());
    }
}
