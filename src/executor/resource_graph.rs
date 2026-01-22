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
//! let state = rustible::provisioning::state::ProvisioningState::new();
//! let plan = graph.plan(&state, None).await?;
//! # Ok(())
//! # }
//! ```
//!
//! See the architecture document at `docs/architecture/resource-graph-model.md`
//! for the full design.

use std::collections::HashMap;
#[cfg(feature = "provisioning")]
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(feature = "provisioning")]
use serde_json::Value as JsonValue;

#[cfg(feature = "provisioning")]
use crate::provisioning::registry::{parse_resource_type, ProviderRegistry};
#[cfg(feature = "provisioning")]
use crate::provisioning::state::{ProvisioningState, ResourceId, ResourceState};
#[cfg(feature = "provisioning")]
use crate::provisioning::traits::{ChangeType, ResourceDiff};

use super::dependency::{DependencyError, DependencyGraph, DependencyKind, DependencyNode};

// ============================================================================
// State Comparison
// ============================================================================

/// Result of comparing desired state against actual (current) state.
///
/// This struct encapsulates the full comparison between what the configuration
/// declares and what exists in the provisioning state, enabling Terraform-like
/// plan output with real diffs from providers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateComparison {
    /// Resources that will be created (exist in desired but not in current state)
    pub to_create: Vec<String>,

    /// Resources that will be updated (exist in both but differ)
    pub to_update: Vec<String>,

    /// Resources that will be replaced (require destroy + create)
    pub to_replace: Vec<String>,

    /// Resources that will be deleted (exist in current state but not in desired)
    pub to_delete: Vec<String>,

    /// Resources that are unchanged
    pub unchanged: Vec<String>,

    /// Detailed plans for each resource
    pub plans: Vec<ResourcePlan>,

    /// Summary statistics
    pub summary: ComparisonSummary,
}

/// Summary statistics for a state comparison
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComparisonSummary {
    /// Total resources in desired state
    pub total_desired: usize,

    /// Total resources in current state
    pub total_current: usize,

    /// Number of resources to create
    pub create_count: usize,

    /// Number of resources to update
    pub update_count: usize,

    /// Number of resources to replace
    pub replace_count: usize,

    /// Number of resources to delete
    pub delete_count: usize,

    /// Number of unchanged resources
    pub unchanged_count: usize,
}

impl StateComparison {
    /// Creates a new empty state comparison
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if there are any changes required
    pub fn has_changes(&self) -> bool {
        !self.to_create.is_empty()
            || !self.to_update.is_empty()
            || !self.to_replace.is_empty()
            || !self.to_delete.is_empty()
    }

    /// Returns the total number of changes
    pub fn change_count(&self) -> usize {
        self.to_create.len() + self.to_update.len() + self.to_replace.len() + self.to_delete.len()
    }

    /// Formats the comparison as a human-readable plan output
    pub fn format_plan(&self) -> String {
        let mut output = String::new();

        output.push_str("\nResource Graph Plan:\n");
        output.push_str("====================\n\n");

        if !self.has_changes() {
            output.push_str("No changes. Your infrastructure matches the configuration.\n");
            return output;
        }

        for plan in &self.plans {
            if plan.action == ResourceAction::NoOp {
                continue;
            }

            let symbol = match plan.action {
                ResourceAction::Create => "+",
                ResourceAction::Update => "~",
                ResourceAction::Replace => "-/+",
                ResourceAction::Delete => "-",
                ResourceAction::NoOp => " ",
            };

            output.push_str(&format!("{} {}\n", symbol, plan.resource_id));

            if let Some(reason) = &plan.reason {
                output.push_str(&format!("    # {}\n", reason));
            }

            for (path, change) in &plan.changes {
                let old_str = change
                    .old
                    .as_ref()
                    .map(|v| format_yaml_value(v))
                    .unwrap_or_else(|| "(not set)".to_string());
                let new_str = change
                    .new
                    .as_ref()
                    .map(|v| format_yaml_value(v))
                    .unwrap_or_else(|| "(not set)".to_string());

                output.push_str(&format!("    {} = {} -> {}\n", path, old_str, new_str));
            }

            output.push('\n');
        }

        output.push_str(&format!(
            "Plan: {} to add, {} to change, {} to replace, {} to destroy.\n",
            self.summary.create_count,
            self.summary.update_count,
            self.summary.replace_count,
            self.summary.delete_count
        ));

        output
    }
}

/// Formats a YAML value for display in plan output
fn format_yaml_value(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::Null => "null".to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::String(s) => format!("\"{}\"", s),
        serde_yaml::Value::Sequence(_) => "[...]".to_string(),
        serde_yaml::Value::Mapping(_) => "{...}".to_string(),
        serde_yaml::Value::Tagged(t) => format_yaml_value(&t.value),
    }
}

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

    /// Planning failed
    #[error("Plan error: {0}")]
    PlanError(String),
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
    /// Resource will be replaced (destroy + create).
    Replace,
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
            ResourceAction::Replace => write!(f, "replace"),
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
            dag.add_node(&resource.id, DependencyNode::task(resource.label()));
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
    /// Compares desired state against the current provisioning state and uses
    /// provider-specific diff logic when a provider registry is supplied.
    #[cfg(feature = "provisioning")]
    pub async fn plan(
        &self,
        state: &ProvisioningState,
        providers: Option<&ProviderRegistry>,
    ) -> ResourceGraphResult<Vec<ResourcePlan>> {
        let order = self.execution_order()?;
        let mut plans = Vec::new();
        let mut desired_addresses = HashSet::new();

        for id in order {
            let resource = self.resources.get(&id).cloned().ok_or_else(|| {
                ResourceGraphError::PlanError(format!("Resource '{}' missing", id))
            })?;
            let address = format!("{}.{}", resource.resource_type, resource.id);
            desired_addresses.insert(address);

            let state_id = ResourceId::new(&resource.resource_type, &resource.id);
            let current_state = state.get_resource(&state_id).cloned();
            let current_json = current_state.as_ref().map(current_state_value);

            let provider_diff = if let Some(providers) = providers {
                match parse_resource_type(&resource.resource_type) {
                    Ok((provider_name, _)) => {
                        let desired_json = yaml_to_json(&resource.desired)?;
                        Some(
                            provider_plan(
                                providers,
                                &provider_name,
                                &resource,
                                &desired_json,
                                current_json.as_ref(),
                            )
                            .await?,
                        )
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

            let plan = if let Some(mut diff) = provider_diff {
                apply_ignore_changes(&mut diff, &resource.lifecycle.ignore_changes);
                let changes = diff_to_changes(&diff, current_json.as_ref())?;
                let action = action_from_diff(&diff);
                let reason = reason_for_action(action, Some(&diff), &resource.lifecycle);
                ResourcePlan {
                    resource_id: resource.id,
                    action,
                    reason: Some(reason),
                    changes,
                }
            } else {
                let current_yaml = match current_json.as_ref() {
                    Some(value) => Some(json_to_yaml(value)?),
                    None => None,
                };
                let changes = match current_yaml.as_ref() {
                    Some(current) => compute_yaml_changes(
                        &resource.desired,
                        current,
                        &resource.lifecycle.ignore_changes,
                    )?,
                    None => HashMap::new(),
                };
                let action = if current_yaml.is_none() {
                    ResourceAction::Create
                } else if changes.is_empty() {
                    ResourceAction::NoOp
                } else {
                    ResourceAction::Update
                };
                let reason = reason_for_action(action, None, &resource.lifecycle);
                ResourcePlan {
                    resource_id: resource.id,
                    action,
                    reason: Some(reason),
                    changes,
                }
            };

            plans.push(plan);
        }

        for (address, _) in &state.resources {
            if desired_addresses.contains(address) {
                continue;
            }

            plans.push(ResourcePlan {
                resource_id: address.clone(),
                action: ResourceAction::Delete,
                reason: Some("Resource no longer in configuration".to_string()),
                changes: HashMap::new(),
            });
        }

        Ok(plans)
    }

    /// Compares desired state against current state and returns a structured comparison.
    ///
    /// This method provides a comprehensive view of what changes will be made,
    /// including summary statistics and formatted output suitable for Terraform-like
    /// plan displays.
    #[cfg(feature = "provisioning")]
    pub async fn compare(
        &self,
        state: &ProvisioningState,
        providers: Option<&ProviderRegistry>,
    ) -> ResourceGraphResult<StateComparison> {
        let plans = self.plan(state, providers).await?;

        let mut comparison = StateComparison::new();
        comparison.summary.total_desired = self.len();
        comparison.summary.total_current = state.resources.len();

        for plan in &plans {
            match plan.action {
                ResourceAction::Create => {
                    comparison.to_create.push(plan.resource_id.clone());
                    comparison.summary.create_count += 1;
                }
                ResourceAction::Update => {
                    comparison.to_update.push(plan.resource_id.clone());
                    comparison.summary.update_count += 1;
                }
                ResourceAction::Replace => {
                    comparison.to_replace.push(plan.resource_id.clone());
                    comparison.summary.replace_count += 1;
                }
                ResourceAction::Delete => {
                    comparison.to_delete.push(plan.resource_id.clone());
                    comparison.summary.delete_count += 1;
                }
                ResourceAction::NoOp => {
                    comparison.unchanged.push(plan.resource_id.clone());
                    comparison.summary.unchanged_count += 1;
                }
            }
        }

        comparison.plans = plans;
        Ok(comparison)
    }

    /// Compares desired state against current state (non-provisioning version).
    ///
    /// Returns a comparison where all resources are marked as unchanged since
    /// actual state comparison requires the provisioning feature.
    #[cfg(not(feature = "provisioning"))]
    pub fn compare(&self) -> ResourceGraphResult<StateComparison> {
        let plans = self.plan()?;

        let mut comparison = StateComparison::new();
        comparison.summary.total_desired = self.len();

        for plan in &plans {
            comparison.unchanged.push(plan.resource_id.clone());
            comparison.summary.unchanged_count += 1;
        }

        comparison.plans = plans;
        Ok(comparison)
    }

    /// Generates an execution plan for all resources.
    ///
    /// This is a placeholder that returns NoOp for all resources.
    /// Actual implementation requires the provisioning feature.
    #[cfg(not(feature = "provisioning"))]
    pub fn plan(&self) -> ResourceGraphResult<Vec<ResourcePlan>> {
        let order = self.execution_order()?;

        let plans = order
            .into_iter()
            .map(|id| ResourcePlan {
                resource_id: id,
                action: ResourceAction::NoOp,
                reason: Some("State comparison requires provisioning feature".to_string()),
                changes: HashMap::new(),
            })
            .collect();

        Ok(plans)
    }
}

// ============================================================================
// Planning Helpers
// ============================================================================

#[cfg(feature = "provisioning")]
fn current_state_value(state: &ResourceState) -> JsonValue {
    if state.attributes.is_null() {
        state.config.clone()
    } else {
        state.attributes.clone()
    }
}

#[cfg(feature = "provisioning")]
async fn provider_plan(
    providers: &ProviderRegistry,
    provider_name: &str,
    resource: &Resource,
    desired: &JsonValue,
    current: Option<&JsonValue>,
) -> ResourceGraphResult<ResourceDiff> {
    let provider_lock = providers
        .get_provider(provider_name)
        .map_err(|err| ResourceGraphError::PlanError(err.to_string()))?;

    let (resource_impl, context) = {
        let provider = provider_lock.read();
        let resource_impl = provider
            .resource(&resource.resource_type)
            .map_err(|err| ResourceGraphError::PlanError(err.to_string()))?;
        let context = provider
            .context()
            .map_err(|err| ResourceGraphError::PlanError(err.to_string()))?;
        (resource_impl, context)
    };

    resource_impl
        .validate(desired)
        .map_err(|err| ResourceGraphError::ValidationError(resource.id.clone(), err.to_string()))?;

    resource_impl
        .plan(desired, current, &context)
        .await
        .map_err(|err| ResourceGraphError::PlanError(err.to_string()))
}

#[cfg(feature = "provisioning")]
fn action_from_diff(diff: &ResourceDiff) -> ResourceAction {
    if diff.requires_replacement || diff.change_type == ChangeType::Replace {
        return ResourceAction::Replace;
    }

    match diff.change_type {
        ChangeType::Create => ResourceAction::Create,
        ChangeType::Update => ResourceAction::Update,
        ChangeType::Destroy => ResourceAction::Delete,
        ChangeType::NoOp | ChangeType::Read => ResourceAction::NoOp,
        ChangeType::Replace => ResourceAction::Replace,
    }
}

#[cfg(feature = "provisioning")]
fn reason_for_action(
    action: ResourceAction,
    diff: Option<&ResourceDiff>,
    lifecycle: &ResourceLifecycle,
) -> String {
    match action {
        ResourceAction::Create => "Resource not found in state".to_string(),
        ResourceAction::Update => "Desired state differs from current state".to_string(),
        ResourceAction::Replace => {
            let mut reason = match diff {
                Some(diff) if !diff.replacement_fields.is_empty() => format!(
                    "Change requires replacement due to {:?}",
                    diff.replacement_fields
                ),
                _ => "Change requires replacement".to_string(),
            };
            if lifecycle.create_before_destroy {
                reason.push_str(" (create_before_destroy)");
            }
            reason
        }
        ResourceAction::Delete => "Resource no longer in configuration".to_string(),
        ResourceAction::NoOp => "State matches desired".to_string(),
    }
}

#[cfg(feature = "provisioning")]
fn apply_ignore_changes(diff: &mut ResourceDiff, ignore_changes: &[String]) {
    if ignore_changes.is_empty() {
        return;
    }

    let should_ignore = |path: &str| should_ignore_path(path, ignore_changes);
    diff.additions.retain(|path, _| !should_ignore(path));
    diff.modifications.retain(|path, _| !should_ignore(path));
    diff.deletions.retain(|path| !should_ignore(path));
    diff.replacement_fields.retain(|path| !should_ignore(path));

    if matches!(diff.change_type, ChangeType::Create | ChangeType::Destroy) {
        return;
    }

    let has_changes =
        !diff.additions.is_empty() || !diff.modifications.is_empty() || !diff.deletions.is_empty();
    diff.requires_replacement = !diff.replacement_fields.is_empty();

    diff.change_type = if diff.requires_replacement {
        ChangeType::Replace
    } else if has_changes {
        ChangeType::Update
    } else {
        ChangeType::NoOp
    };
}

#[cfg(feature = "provisioning")]
fn diff_to_changes(
    diff: &ResourceDiff,
    current: Option<&JsonValue>,
) -> ResourceGraphResult<HashMap<String, AttributeChange>> {
    let mut changes = HashMap::new();

    for (path, value) in &diff.additions {
        changes.insert(
            path.clone(),
            AttributeChange {
                path: path.clone(),
                old: None,
                new: Some(json_to_yaml(value)?),
            },
        );
    }

    for (path, (old, new)) in &diff.modifications {
        changes.insert(
            path.clone(),
            AttributeChange {
                path: path.clone(),
                old: Some(json_to_yaml(old)?),
                new: Some(json_to_yaml(new)?),
            },
        );
    }

    for path in &diff.deletions {
        let old_value = current.and_then(|value| value.get(path)).cloned();
        let old_yaml = match old_value {
            Some(value) => Some(json_to_yaml(&value)?),
            None => None,
        };
        changes.insert(
            path.clone(),
            AttributeChange {
                path: path.clone(),
                old: old_yaml,
                new: None,
            },
        );
    }

    Ok(changes)
}

#[cfg(feature = "provisioning")]
fn compute_yaml_changes(
    desired: &serde_yaml::Value,
    current: &serde_yaml::Value,
    ignore_changes: &[String],
) -> ResourceGraphResult<HashMap<String, AttributeChange>> {
    let mut changes = HashMap::new();
    collect_changes("", desired, current, ignore_changes, &mut changes)?;
    Ok(changes)
}

#[cfg(feature = "provisioning")]
fn collect_changes(
    path: &str,
    desired: &serde_yaml::Value,
    current: &serde_yaml::Value,
    ignore_changes: &[String],
    changes: &mut HashMap<String, AttributeChange>,
) -> ResourceGraphResult<()> {
    if !path.is_empty() && should_ignore_path(path, ignore_changes) {
        return Ok(());
    }

    match (desired, current) {
        (serde_yaml::Value::Mapping(desired_map), serde_yaml::Value::Mapping(current_map)) => {
            let mut keys = HashSet::new();
            for key in desired_map.keys() {
                keys.insert(map_key_to_string(key)?);
            }
            for key in current_map.keys() {
                keys.insert(map_key_to_string(key)?);
            }

            for key in keys {
                let key_value = serde_yaml::Value::String(key.clone());
                let desired_val = desired_map.get(&key_value);
                let current_val = current_map.get(&key_value);
                let next_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };

                match (desired_val, current_val) {
                    (Some(desired_val), Some(current_val)) => {
                        if desired_val == current_val {
                            continue;
                        }
                        collect_changes(
                            &next_path,
                            desired_val,
                            current_val,
                            ignore_changes,
                            changes,
                        )?;
                    }
                    (Some(desired_val), None) => {
                        if should_ignore_path(&next_path, ignore_changes) {
                            continue;
                        }
                        changes.insert(
                            next_path.clone(),
                            AttributeChange {
                                path: next_path,
                                old: None,
                                new: Some(desired_val.clone()),
                            },
                        );
                    }
                    (None, Some(current_val)) => {
                        if should_ignore_path(&next_path, ignore_changes) {
                            continue;
                        }
                        changes.insert(
                            next_path.clone(),
                            AttributeChange {
                                path: next_path,
                                old: Some(current_val.clone()),
                                new: None,
                            },
                        );
                    }
                    (None, None) => {}
                }
            }
        }
        (serde_yaml::Value::Sequence(desired_seq), serde_yaml::Value::Sequence(current_seq)) => {
            let max_len = desired_seq.len().max(current_seq.len());
            for index in 0..max_len {
                let next_path = if path.is_empty() {
                    index.to_string()
                } else {
                    format!("{}.{}", path, index)
                };
                let desired_val = desired_seq.get(index);
                let current_val = current_seq.get(index);

                match (desired_val, current_val) {
                    (Some(desired_val), Some(current_val)) => {
                        if desired_val == current_val {
                            continue;
                        }
                        collect_changes(
                            &next_path,
                            desired_val,
                            current_val,
                            ignore_changes,
                            changes,
                        )?;
                    }
                    (Some(desired_val), None) => {
                        if should_ignore_path(&next_path, ignore_changes) {
                            continue;
                        }
                        changes.insert(
                            next_path.clone(),
                            AttributeChange {
                                path: next_path,
                                old: None,
                                new: Some(desired_val.clone()),
                            },
                        );
                    }
                    (None, Some(current_val)) => {
                        if should_ignore_path(&next_path, ignore_changes) {
                            continue;
                        }
                        changes.insert(
                            next_path.clone(),
                            AttributeChange {
                                path: next_path,
                                old: Some(current_val.clone()),
                                new: None,
                            },
                        );
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {
            if desired != current {
                let change_path = if path.is_empty() { "root" } else { path };
                if should_ignore_path(change_path, ignore_changes) {
                    return Ok(());
                }
                changes.insert(
                    change_path.to_string(),
                    AttributeChange {
                        path: change_path.to_string(),
                        old: Some(current.clone()),
                        new: Some(desired.clone()),
                    },
                );
            }
        }
    }

    Ok(())
}

#[cfg(feature = "provisioning")]
fn should_ignore_path(path: &str, ignore_changes: &[String]) -> bool {
    ignore_changes.iter().any(|ignore| {
        if path == ignore {
            return true;
        }
        path.starts_with(&format!("{}.", ignore))
    })
}

#[cfg(feature = "provisioning")]
fn map_key_to_string(key: &serde_yaml::Value) -> ResourceGraphResult<String> {
    match key {
        serde_yaml::Value::String(value) => Ok(value.clone()),
        _ => Err(ResourceGraphError::PlanError(
            "Only string keys are supported in resource graphs".to_string(),
        )),
    }
}

#[cfg(feature = "provisioning")]
fn yaml_to_json(value: &serde_yaml::Value) -> ResourceGraphResult<JsonValue> {
    Ok(match value {
        serde_yaml::Value::Null => JsonValue::Null,
        serde_yaml::Value::Bool(value) => JsonValue::Bool(*value),
        serde_yaml::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                JsonValue::Number(value.into())
            } else if let Some(value) = number.as_u64() {
                JsonValue::Number(value.into())
            } else if let Some(value) = number.as_f64() {
                let number = serde_json::Number::from_f64(value).ok_or_else(|| {
                    ResourceGraphError::PlanError(format!(
                        "Unsupported floating point value: {}",
                        value
                    ))
                })?;
                JsonValue::Number(number)
            } else {
                return Err(ResourceGraphError::PlanError(
                    "Unsupported numeric value".to_string(),
                ));
            }
        }
        serde_yaml::Value::String(value) => JsonValue::String(value.clone()),
        serde_yaml::Value::Sequence(values) => JsonValue::Array(
            values
                .iter()
                .map(yaml_to_json)
                .collect::<ResourceGraphResult<Vec<_>>>()?,
        ),
        serde_yaml::Value::Mapping(map) => {
            let mut object = serde_json::Map::new();
            for (key, value) in map {
                let key = map_key_to_string(key)?;
                object.insert(key, yaml_to_json(value)?);
            }
            JsonValue::Object(object)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(&tagged.value)?,
    })
}

#[cfg(feature = "provisioning")]
fn json_to_yaml(value: &JsonValue) -> ResourceGraphResult<serde_yaml::Value> {
    serde_yaml::to_value(value).map_err(|err| ResourceGraphError::PlanError(err.to_string()))
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
    #[cfg(feature = "provisioning")]
    use crate::provisioning::state::{ProvisioningState, ResourceId, ResourceState};
    #[cfg(feature = "provisioning")]
    use serde_json::json;

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
        graph.add_resource(Resource::new("test", "type1")).unwrap();

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

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_plan_generation() {
        let mut graph = ResourceGraph::new();
        graph.add_resource(Resource::new("test", "type")).unwrap();

        let state = ProvisioningState::new();
        let plans = graph.plan(&state, None).await.unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].resource_id, "test");
        assert_eq!(plans[0].action, ResourceAction::Create);
    }

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_plan_detects_update() {
        let mut graph = ResourceGraph::new();
        let desired = serde_yaml::to_value(json!({ "size": "small" })).unwrap();
        graph
            .add_resource(Resource::new("app", "type").with_desired(desired))
            .unwrap();

        let mut state = ProvisioningState::new();
        let id = ResourceId::new("type", "app");
        state.add_resource(ResourceState::new(
            id,
            "cloud-1",
            "test",
            json!({ "size": "large" }),
            json!({ "size": "large" }),
        ));

        let plans = graph.plan(&state, None).await.unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, ResourceAction::Update);
        assert!(plans[0].changes.contains_key("size"));
    }

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_plan_ignores_changes() {
        let mut graph = ResourceGraph::new();
        let desired = serde_yaml::to_value(json!({ "size": "small" })).unwrap();
        let lifecycle = ResourceLifecycle {
            ignore_changes: vec!["size".to_string()],
            ..ResourceLifecycle::default()
        };
        graph
            .add_resource(
                Resource::new("app", "type")
                    .with_desired(desired)
                    .with_lifecycle(lifecycle),
            )
            .unwrap();

        let mut state = ProvisioningState::new();
        let id = ResourceId::new("type", "app");
        state.add_resource(ResourceState::new(
            id,
            "cloud-1",
            "test",
            json!({ "size": "large" }),
            json!({ "size": "large" }),
        ));

        let plans = graph.plan(&state, None).await.unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, ResourceAction::NoOp);
        assert!(plans[0].changes.is_empty());
    }

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_plan_deletes_orphans() {
        let graph = ResourceGraph::new();
        let mut state = ProvisioningState::new();
        let id = ResourceId::new("type", "orphan");
        state.add_resource(ResourceState::new(
            id,
            "cloud-1",
            "test",
            json!({}),
            json!({ "size": "large" }),
        ));

        let plans = graph.plan(&state, None).await.unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].action, ResourceAction::Delete);
        assert_eq!(plans[0].resource_id, "type.orphan");
    }

    #[cfg(not(feature = "provisioning"))]
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
        assert_eq!(ResourceAction::Replace.to_string(), "replace");
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
        let output = ResourceOutput::new("web_server").with_value(
            "public_ip",
            serde_yaml::Value::String("1.2.3.4".to_string()),
        );

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

    #[test]
    fn test_state_comparison_new() {
        let comparison = StateComparison::new();
        assert!(!comparison.has_changes());
        assert_eq!(comparison.change_count(), 0);
        assert!(comparison.to_create.is_empty());
        assert!(comparison.to_update.is_empty());
        assert!(comparison.to_replace.is_empty());
        assert!(comparison.to_delete.is_empty());
    }

    #[test]
    fn test_state_comparison_has_changes() {
        let mut comparison = StateComparison::new();
        assert!(!comparison.has_changes());

        comparison.to_create.push("resource1".to_string());
        assert!(comparison.has_changes());
        assert_eq!(comparison.change_count(), 1);
    }

    #[test]
    fn test_state_comparison_format_plan_no_changes() {
        let comparison = StateComparison::new();
        let output = comparison.format_plan();
        assert!(output.contains("No changes"));
    }

    #[test]
    fn test_state_comparison_format_plan_with_changes() {
        let mut comparison = StateComparison::new();
        comparison.to_create.push("web_server".to_string());
        comparison.summary.create_count = 1;
        comparison.plans.push(ResourcePlan {
            resource_id: "web_server".to_string(),
            action: ResourceAction::Create,
            reason: Some("Resource not found in state".to_string()),
            changes: HashMap::new(),
        });

        let output = comparison.format_plan();
        assert!(output.contains("+ web_server"));
        assert!(output.contains("1 to add"));
    }

    #[test]
    fn test_comparison_summary_default() {
        let summary = ComparisonSummary::default();
        assert_eq!(summary.total_desired, 0);
        assert_eq!(summary.total_current, 0);
        assert_eq!(summary.create_count, 0);
        assert_eq!(summary.update_count, 0);
        assert_eq!(summary.replace_count, 0);
        assert_eq!(summary.delete_count, 0);
        assert_eq!(summary.unchanged_count, 0);
    }

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_compare_empty_graph_empty_state() {
        let graph = ResourceGraph::new();
        let state = ProvisioningState::new();

        let comparison = graph.compare(&state, None).await.unwrap();
        assert!(!comparison.has_changes());
        assert_eq!(comparison.summary.total_desired, 0);
        assert_eq!(comparison.summary.total_current, 0);
    }

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_compare_detects_creates() {
        let mut graph = ResourceGraph::new();
        graph
            .add_resource(Resource::new("web", "aws_instance"))
            .unwrap();
        graph.add_resource(Resource::new("db", "aws_rds")).unwrap();

        let state = ProvisioningState::new();
        let comparison = graph.compare(&state, None).await.unwrap();

        assert!(comparison.has_changes());
        assert_eq!(comparison.summary.create_count, 2);
        assert_eq!(comparison.to_create.len(), 2);
        assert!(comparison.to_create.contains(&"web".to_string()));
        assert!(comparison.to_create.contains(&"db".to_string()));
    }

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_compare_detects_deletes() {
        let graph = ResourceGraph::new();
        let mut state = ProvisioningState::new();

        let id = ResourceId::new("aws_instance", "orphan");
        state.add_resource(ResourceState::new(
            id,
            "i-12345",
            "aws",
            json!({}),
            json!({}),
        ));

        let comparison = graph.compare(&state, None).await.unwrap();
        assert!(comparison.has_changes());
        assert_eq!(comparison.summary.delete_count, 1);
        assert_eq!(comparison.to_delete.len(), 1);
    }

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_compare_detects_updates() {
        let mut graph = ResourceGraph::new();
        let desired = serde_yaml::to_value(json!({ "instance_type": "t3.large" })).unwrap();
        graph
            .add_resource(Resource::new("web", "aws_instance").with_desired(desired))
            .unwrap();

        let mut state = ProvisioningState::new();
        let id = ResourceId::new("aws_instance", "web");
        state.add_resource(ResourceState::new(
            id,
            "i-12345",
            "aws",
            json!({ "instance_type": "t3.small" }),
            json!({ "instance_type": "t3.small" }),
        ));

        let comparison = graph.compare(&state, None).await.unwrap();
        assert!(comparison.has_changes());
        assert_eq!(comparison.summary.update_count, 1);
        assert_eq!(comparison.to_update.len(), 1);
        assert!(comparison.to_update.contains(&"web".to_string()));
    }

    #[cfg(feature = "provisioning")]
    #[tokio::test]
    async fn test_compare_unchanged_resources() {
        let mut graph = ResourceGraph::new();
        let desired = serde_yaml::to_value(json!({ "instance_type": "t3.small" })).unwrap();
        graph
            .add_resource(Resource::new("web", "aws_instance").with_desired(desired))
            .unwrap();

        let mut state = ProvisioningState::new();
        let id = ResourceId::new("aws_instance", "web");
        state.add_resource(ResourceState::new(
            id,
            "i-12345",
            "aws",
            json!({ "instance_type": "t3.small" }),
            json!({ "instance_type": "t3.small" }),
        ));

        let comparison = graph.compare(&state, None).await.unwrap();
        assert!(!comparison.has_changes());
        assert_eq!(comparison.summary.unchanged_count, 1);
        assert_eq!(comparison.unchanged.len(), 1);
    }

    #[test]
    fn test_format_yaml_value() {
        assert_eq!(format_yaml_value(&serde_yaml::Value::Null), "null");
        assert_eq!(format_yaml_value(&serde_yaml::Value::Bool(true)), "true");
        assert_eq!(
            format_yaml_value(&serde_yaml::Value::String("test".to_string())),
            "\"test\""
        );
    }
}
