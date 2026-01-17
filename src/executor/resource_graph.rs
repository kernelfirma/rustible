//! Declarative Resource Graph Model for Rustible
//!
//! This module implements a Terraform-like declarative resource graph that coexists
//! with imperative playbooks. Resources are defined with desired state and dependencies,
//! then mapped to the existing DAG executor for deterministic ordering.
//!
//! # Overview
//!
//! The resource graph model enables:
//! - **Declarative resources** with desired state and lifecycle options
//! - **DAG-based execution** with explicit dependencies via `depends_on`
//! - **Provider extensibility** for cloud resources and custom types
//! - **Coexistence with playbooks** as a `resource_graph` task or CLI subcommand
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::executor::resource_graph::{Resource, ResourceGraph, ResourceLifecycle};
//!
//! let mut graph = ResourceGraph::new();
//!
//! graph.add_resource(Resource {
//!     id: "web_server".to_string(),
//!     resource_type: "aws_instance".to_string(),
//!     desired: serde_yaml::Value::Mapping(Default::default()),
//!     depends_on: vec!["vpc_main".to_string()],
//!     lifecycle: ResourceLifecycle::default(),
//! });
//!
//! let dag = graph.build_dag()?;
//! let plan = graph.plan()?;
//! # Ok(())
//! # }
//! ```
//!
//! See the architecture document at `docs/architecture/resource-graph-model.md`
//! for the full design.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::dependency::{DependencyError, DependencyGraph, DependencyKind, DependencyNode};

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during resource graph operations
#[derive(Error, Debug, Clone)]
pub enum ResourceGraphError {
    /// A resource with the given ID already exists
    #[error("Duplicate resource ID: '{0}'")]
    DuplicateResource(String),

    /// A dependency references a non-existent resource
    #[error("Resource '{0}' depends on unknown resource '{1}'")]
    UnknownDependency(String, String),

    /// Circular dependency detected in the resource graph
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    /// Dependency graph error
    #[error("Dependency error: {0}")]
    DependencyError(#[from] DependencyError),

    /// Provider validation failed
    #[error("Provider validation failed for resource '{0}': {1}")]
    ValidationError(String, String),
}

/// Result type for resource graph operations
pub type ResourceGraphResult<T> = Result<T, ResourceGraphError>;

// ============================================================================
// Core Types
// ============================================================================

/// Lifecycle configuration for a resource.
///
/// Controls how resources are created, updated, and destroyed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLifecycle {
    /// If true, create replacement before destroying the original.
    /// Useful for zero-downtime deployments.
    #[serde(default)]
    pub create_before_destroy: bool,

    /// List of attribute paths to ignore during update planning.
    /// Changes to these attributes won't trigger an update action.
    #[serde(default)]
    pub ignore_changes: Vec<String>,
}

/// A declarative resource definition.
///
/// Each resource represents a desired state that a provider will reconcile.
/// Resources form a DAG via the `depends_on` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// Unique, stable identifier for the resource.
    /// Used for graph edges and state tracking.
    pub id: String,

    /// Resource type (provider-specific or built-in like `playbook`).
    #[serde(rename = "type")]
    pub resource_type: String,

    /// Desired state payload.
    /// Opaque to the core engine; validated and applied by providers.
    pub desired: serde_yaml::Value,

    /// Explicit dependencies on other resources.
    /// Resources in this list must complete before this resource is processed.
    #[serde(default)]
    pub depends_on: Vec<String>,

    /// Lifecycle configuration for create/update/delete behavior.
    #[serde(default)]
    pub lifecycle: ResourceLifecycle,
}

impl Resource {
    /// Creates a new resource with minimal required fields.
    pub fn new(id: impl Into<String>, resource_type: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            resource_type: resource_type.into(),
            desired: serde_yaml::Value::Null,
            depends_on: Vec::new(),
            lifecycle: ResourceLifecycle::default(),
        }
    }

    /// Sets the desired state for the resource.
    pub fn with_desired(mut self, desired: serde_yaml::Value) -> Self {
        self.desired = desired;
        self
    }

    /// Adds a dependency on another resource.
    pub fn with_dependency(mut self, dep: impl Into<String>) -> Self {
        self.depends_on.push(dep.into());
        self
    }

    /// Sets the lifecycle configuration.
    pub fn with_lifecycle(mut self, lifecycle: ResourceLifecycle) -> Self {
        self.lifecycle = lifecycle;
        self
    }
}

/// Actions that can be taken on a resource during plan execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceAction {
    /// Resource will be created (does not exist in current state).
    Create,
    /// Resource will be updated (exists but desired state differs).
    Update,
    /// Resource will be deleted (exists in state but not in desired config).
    Delete,
    /// No operation needed (current state matches desired state).
    NoOp,
}

impl std::fmt::Display for ResourceAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceAction::Create => write!(f, "create"),
            ResourceAction::Update => write!(f, "update"),
            ResourceAction::Delete => write!(f, "delete"),
            ResourceAction::NoOp => write!(f, "no-op"),
        }
    }
}

/// A planned action for a single resource.
///
/// Generated during the `plan` phase and executed during `apply`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourcePlan {
    /// The resource ID this plan applies to.
    pub resource_id: String,

    /// The action to take.
    pub action: ResourceAction,

    /// Human-readable reason for the action.
    pub reason: Option<String>,

    /// Attributes that will change (for Update actions).
    #[serde(default)]
    pub changes: HashMap<String, AttributeChange>,
}

/// Represents a change to a single attribute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeChange {
    /// The attribute path (e.g., "tags.Name").
    pub path: String,
    /// The current value (if any).
    pub old: Option<serde_yaml::Value>,
    /// The new desired value (if any).
    pub new: Option<serde_yaml::Value>,
}

/// Output values produced by a resource after apply.
///
/// These outputs can be referenced by downstream resources or playbooks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceOutput {
    /// The resource ID that produced these outputs.
    pub resource_id: String,

    /// Output key-value pairs.
    /// Keys are output names, values are the output data.
    pub values: HashMap<String, serde_yaml::Value>,
}

impl ResourceOutput {
    /// Creates a new empty output for a resource.
    pub fn new(resource_id: impl Into<String>) -> Self {
        Self {
            resource_id: resource_id.into(),
            values: HashMap::new(),
        }
    }

    /// Adds an output value.
    pub fn with_value(mut self, key: impl Into<String>, value: serde_yaml::Value) -> Self {
        self.values.insert(key.into(), value);
        self
    }
}

// ============================================================================
// Graph Node Trait
// ============================================================================

/// Trait for types that can be nodes in the resource DAG.
///
/// This enables integration with the existing dependency graph infrastructure.
pub trait GraphNode {
    /// Returns the unique identifier for this node.
    fn node_id(&self) -> &str;

    /// Returns the IDs of nodes this node depends on.
    fn dependencies(&self) -> &[String];

    /// Returns a human-readable label for visualization.
    fn label(&self) -> String;
}

impl GraphNode for Resource {
    fn node_id(&self) -> &str {
        &self.id
    }

    fn dependencies(&self) -> &[String] {
        &self.depends_on
    }

    fn label(&self) -> String {
        format!("{}[{}]", self.id, self.resource_type)
    }
}

// ============================================================================
// Resource Graph
// ============================================================================

/// A collection of resources with dependency relationships.
///
/// The ResourceGraph holds resources and provides methods to:
/// - Build a DAG for execution ordering
/// - Generate execution plans
/// - Validate dependencies
#[derive(Debug, Clone, Default)]
pub struct ResourceGraph {
    /// Resources indexed by their ID.
    resources: HashMap<String, Resource>,

    /// Insertion order for deterministic iteration.
    order: Vec<String>,
}

impl ResourceGraph {
    /// Creates a new empty resource graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a resource to the graph.
    ///
    /// # Errors
    ///
    /// Returns an error if a resource with the same ID already exists.
    pub fn add_resource(&mut self, resource: Resource) -> ResourceGraphResult<()> {
        if self.resources.contains_key(&resource.id) {
            return Err(ResourceGraphError::DuplicateResource(resource.id.clone()));
        }

        self.order.push(resource.id.clone());
        self.resources.insert(resource.id.clone(), resource);
        Ok(())
    }

    /// Returns a reference to a resource by ID.
    pub fn get(&self, id: &str) -> Option<&Resource> {
        self.resources.get(id)
    }

    /// Returns the number of resources in the graph.
    pub fn len(&self) -> usize {
        self.resources.len()
    }

    /// Returns true if the graph has no resources.
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// Returns an iterator over resources in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &Resource> {
        self.order.iter().filter_map(|id| self.resources.get(id))
    }

    /// Validates all dependencies in the graph.
    ///
    /// Ensures that all `depends_on` references point to existing resources.
    pub fn validate_dependencies(&self) -> ResourceGraphResult<()> {
        for resource in self.iter() {
            for dep in &resource.depends_on {
                if !self.resources.contains_key(dep) {
                    return Err(ResourceGraphError::UnknownDependency(
                        resource.id.clone(),
                        dep.clone(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Builds a DependencyGraph from the resources.
    ///
    /// This maps resources to the existing DAG infrastructure for execution ordering.
    ///
    /// # Errors
    ///
    /// Returns an error if dependencies are invalid or circular.
    pub fn build_dag(&self) -> ResourceGraphResult<DependencyGraph> {
        self.validate_dependencies()?;

        let mut dag = DependencyGraph::new();

        // Add all resources as nodes
        for resource in self.iter() {
            dag.add_node(&resource.id, DependencyNode::task(&resource.label()));
        }

        // Add dependency edges
        for resource in self.iter() {
            for dep in &resource.depends_on {
                dag.add_dependency(&resource.id, dep, DependencyKind::Explicit)?;
            }
        }

        Ok(dag)
    }

    /// Generates a topologically sorted execution order.
    ///
    /// Resources are ordered such that dependencies are processed first.
    pub fn execution_order(&self) -> ResourceGraphResult<Vec<String>> {
        let dag = self.build_dag()?;
        Ok(dag.topological_sort()?)
    }

    /// Generates an execution plan for all resources.
    ///
    /// This is a placeholder that returns NoOp for all resources.
    /// Actual implementation will compare desired state against current state.
    pub fn plan(&self) -> ResourceGraphResult<Vec<ResourcePlan>> {
        let order = self.execution_order()?;

        // TODO: Implement actual state comparison via providers
        let plans = order
            .into_iter()
            .map(|id| ResourcePlan {
                resource_id: id,
                action: ResourceAction::NoOp,
                reason: Some("State comparison not yet implemented".to_string()),
                changes: HashMap::new(),
            })
            .collect();

        Ok(plans)
    }
}

// ============================================================================
// Serialization Support
// ============================================================================

/// A resource graph file format for YAML serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceGraphFile {
    /// List of resources in the graph.
    pub resources: Vec<Resource>,
}

impl TryFrom<ResourceGraphFile> for ResourceGraph {
    type Error = ResourceGraphError;

    fn try_from(file: ResourceGraphFile) -> Result<Self, Self::Error> {
        let mut graph = ResourceGraph::new();
        for resource in file.resources {
            graph.add_resource(resource)?;
        }
        Ok(graph)
    }
}

impl From<ResourceGraph> for ResourceGraphFile {
    fn from(graph: ResourceGraph) -> Self {
        Self {
            resources: graph.iter().cloned().collect(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_creation() {
        let resource = Resource::new("web_server", "aws_instance")
            .with_dependency("vpc")
            .with_lifecycle(ResourceLifecycle {
                create_before_destroy: true,
                ignore_changes: vec!["tags.Generated".to_string()],
            });

        assert_eq!(resource.id, "web_server");
        assert_eq!(resource.resource_type, "aws_instance");
        assert_eq!(resource.depends_on, vec!["vpc"]);
        assert!(resource.lifecycle.create_before_destroy);
    }

    #[test]
    fn test_empty_graph() {
        let graph = ResourceGraph::new();
        assert!(graph.is_empty());
        assert_eq!(graph.len(), 0);
    }

    #[test]
    fn test_add_resource() {
        let mut graph = ResourceGraph::new();
        let resource = Resource::new("test", "test_type");

        graph.add_resource(resource).unwrap();
        assert_eq!(graph.len(), 1);
        assert!(graph.get("test").is_some());
    }

    #[test]
    fn test_duplicate_resource_error() {
        let mut graph = ResourceGraph::new();
        graph
            .add_resource(Resource::new("test", "type1"))
            .unwrap();

        let result = graph.add_resource(Resource::new("test", "type2"));
        assert!(matches!(
            result,
            Err(ResourceGraphError::DuplicateResource(_))
        ));
    }

    #[test]
    fn test_unknown_dependency_error() {
        let mut graph = ResourceGraph::new();
        let resource = Resource::new("web", "instance").with_dependency("nonexistent");
        graph.add_resource(resource).unwrap();

        let result = graph.validate_dependencies();
        assert!(matches!(
            result,
            Err(ResourceGraphError::UnknownDependency(_, _))
        ));
    }

    #[test]
    fn test_build_dag() {
        let mut graph = ResourceGraph::new();
        graph.add_resource(Resource::new("vpc", "aws_vpc")).unwrap();
        graph
            .add_resource(Resource::new("subnet", "aws_subnet").with_dependency("vpc"))
            .unwrap();
        graph
            .add_resource(Resource::new("instance", "aws_instance").with_dependency("subnet"))
            .unwrap();

        let dag = graph.build_dag().unwrap();
        let order = dag.topological_sort().unwrap();

        // vpc must come before subnet, subnet before instance
        let vpc_pos = order.iter().position(|x| x == "vpc").unwrap();
        let subnet_pos = order.iter().position(|x| x == "subnet").unwrap();
        let instance_pos = order.iter().position(|x| x == "instance").unwrap();

        assert!(vpc_pos < subnet_pos);
        assert!(subnet_pos < instance_pos);
    }

    #[test]
    fn test_execution_order() {
        let mut graph = ResourceGraph::new();
        graph.add_resource(Resource::new("a", "t")).unwrap();
        graph
            .add_resource(Resource::new("b", "t").with_dependency("a"))
            .unwrap();
        graph
            .add_resource(Resource::new("c", "t").with_dependency("b"))
            .unwrap();

        let order = graph.execution_order().unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_plan_generation() {
        let mut graph = ResourceGraph::new();
        graph.add_resource(Resource::new("test", "type")).unwrap();

        let plans = graph.plan().unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].resource_id, "test");
        assert_eq!(plans[0].action, ResourceAction::NoOp);
    }

    #[test]
    fn test_resource_action_display() {
        assert_eq!(ResourceAction::Create.to_string(), "create");
        assert_eq!(ResourceAction::Update.to_string(), "update");
        assert_eq!(ResourceAction::Delete.to_string(), "delete");
        assert_eq!(ResourceAction::NoOp.to_string(), "no-op");
    }

    #[test]
    fn test_graph_node_trait() {
        let resource = Resource::new("test", "aws_instance").with_dependency("vpc");

        assert_eq!(resource.node_id(), "test");
        assert_eq!(resource.dependencies(), &["vpc"]);
        assert_eq!(resource.label(), "test[aws_instance]");
    }

    #[test]
    fn test_resource_output() {
        let output = ResourceOutput::new("web_server")
            .with_value("public_ip", serde_yaml::Value::String("1.2.3.4".to_string()));

        assert_eq!(output.resource_id, "web_server");
        assert!(output.values.contains_key("public_ip"));
    }

    #[test]
    fn test_yaml_serialization() {
        let resource = Resource::new("test", "aws_instance");
        let yaml = serde_yaml::to_string(&resource).unwrap();
        assert!(yaml.contains("id: test"));
        assert!(yaml.contains("type: aws_instance"));
    }

    #[test]
    fn test_resource_graph_file_conversion() {
        let file = ResourceGraphFile {
            resources: vec![
                Resource::new("a", "t"),
                Resource::new("b", "t").with_dependency("a"),
            ],
        };

        let graph: ResourceGraph = file.try_into().unwrap();
        assert_eq!(graph.len(), 2);

        let back: ResourceGraphFile = graph.into();
        assert_eq!(back.resources.len(), 2);
    }
}
