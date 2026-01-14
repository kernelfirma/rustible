use std::collections::{HashMap, HashSet};

use super::{ExecutorError, ExecutorResult};

/// Dependency graph for task ordering using topological sort.
///
/// Used internally to resolve task dependencies and detect circular
/// dependencies that would prevent execution.
///
/// # Example
///
/// ```rust
/// use rustible::executor::DependencyGraph;
///
/// let mut graph = DependencyGraph::new();
/// graph.add_dependency("install_app", "install_deps");
/// graph.add_dependency("configure_app", "install_app");
///
/// let order = graph.topological_sort().expect("no cycles");
/// // order: ["install_deps", "install_app", "configure_app"]
/// ```
pub struct DependencyGraph {
    nodes: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    /// Creates a new empty dependency graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
        }
    }

    /// Adds a dependency relationship.
    ///
    /// Declares that `task` depends on `dependency`, meaning `dependency`
    /// must complete before `task` can begin.
    ///
    /// # Arguments
    ///
    /// * `task` - The task that has a dependency
    /// * `dependency` - The task that must complete first
    pub fn add_dependency(&mut self, task: &str, dependency: &str) {
        self.nodes
            .entry(task.to_string())
            .or_default()
            .push(dependency.to_string());
        // Also ensure the dependency exists as a node (with no dependencies of its own)
        self.nodes.entry(dependency.to_string()).or_default();
    }

    /// Returns tasks in topologically sorted order.
    ///
    /// The returned order ensures that all dependencies appear before
    /// their dependents.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutorError::DependencyCycle`] if a circular dependency
    /// is detected in the graph.
    pub fn topological_sort(&self) -> ExecutorResult<Vec<String>> {
        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new();
        let mut result = Vec::new();

        for node in self.nodes.keys() {
            if !visited.contains(node) {
                self.visit(node, &mut visited, &mut temp_visited, &mut result)?;
            }
        }

        // Don't reverse - the order is already correct (dependencies come before dependents)
        Ok(result)
    }

    fn visit(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        temp_visited: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) -> ExecutorResult<()> {
        if temp_visited.contains(node) {
            return Err(ExecutorError::DependencyCycle(node.to_string()));
        }

        if !visited.contains(node) {
            temp_visited.insert(node.to_string());

            if let Some(deps) = self.nodes.get(node) {
                for dep in deps {
                    self.visit(dep, visited, temp_visited, result)?;
                }
            }

            temp_visited.remove(node);
            visited.insert(node.to_string());
            result.push(node.to_string());
        }

        Ok(())
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::DependencyGraph;
    use crate::executor::ExecutorError;

    #[test]
    fn test_dependency_graph_no_cycle() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("task3", "task2");
        graph.add_dependency("task2", "task1");

        let order = graph.topological_sort().unwrap();
        // The order should respect dependencies: task1 before task2 before task3
        assert_eq!(order.len(), 3);
        let t1_pos = order.iter().position(|x| *x == "task1").unwrap();
        let t2_pos = order.iter().position(|x| *x == "task2").unwrap();
        let t3_pos = order.iter().position(|x| *x == "task3").unwrap();
        assert!(t1_pos < t2_pos, "task1 should come before task2");
        assert!(t2_pos < t3_pos, "task2 should come before task3");
    }

    #[test]
    fn test_dependency_graph_cycle_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("task1", "task2");
        graph.add_dependency("task2", "task3");
        graph.add_dependency("task3", "task1");

        let result = graph.topological_sort();
        assert!(matches!(result, Err(ExecutorError::DependencyCycle(_))));
    }
}
