//! Execution Plan for Infrastructure Changes
//!
//! This module provides the ExecutionPlan and PlanBuilder for determining
//! what changes need to be made to reach the desired infrastructure state.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use colored::Colorize;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{ProvisioningError, ProvisioningResult};
use super::state::{ProvisioningState, ResourceId};
use super::traits::{ChangeType, ResourceDiff};

// ============================================================================
// Planned Actions
// ============================================================================

/// A single action to be taken on a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedAction {
    /// Resource being acted upon
    pub resource_id: ResourceId,

    /// Type of change
    pub change_type: ChangeType,

    /// Provider name
    pub provider: String,

    /// The diff details
    pub diff: ResourceDiff,

    /// Reason for the change
    pub reason: String,

    /// Dependencies that must complete first
    pub depends_on: Vec<ResourceId>,

    /// Whether this action can be parallelized
    pub parallelizable: bool,
}

impl PlannedAction {
    /// Create a create action
    pub fn create(
        resource_id: ResourceId,
        provider: impl Into<String>,
        diff: ResourceDiff,
    ) -> Self {
        Self {
            resource_id,
            change_type: ChangeType::Create,
            provider: provider.into(),
            diff,
            reason: "Resource does not exist".to_string(),
            depends_on: Vec::new(),
            parallelizable: true,
        }
    }

    /// Create an update action
    pub fn update(
        resource_id: ResourceId,
        provider: impl Into<String>,
        diff: ResourceDiff,
    ) -> Self {
        Self {
            resource_id,
            change_type: ChangeType::Update,
            provider: provider.into(),
            diff,
            reason: "Configuration changed".to_string(),
            depends_on: Vec::new(),
            parallelizable: true,
        }
    }

    /// Create a replace action
    pub fn replace(
        resource_id: ResourceId,
        provider: impl Into<String>,
        diff: ResourceDiff,
    ) -> Self {
        let reason = format!(
            "Force replacement due to changes in: {:?}",
            diff.replacement_fields
        );
        Self {
            resource_id,
            change_type: ChangeType::Replace,
            provider: provider.into(),
            diff,
            reason,
            depends_on: Vec::new(),
            parallelizable: false, // Replacements are usually sequential
        }
    }

    /// Create a destroy action
    pub fn destroy(resource_id: ResourceId, provider: impl Into<String>) -> Self {
        Self {
            resource_id,
            change_type: ChangeType::Destroy,
            provider: provider.into(),
            diff: ResourceDiff::destroy(),
            reason: "Resource no longer in configuration".to_string(),
            depends_on: Vec::new(),
            parallelizable: true,
        }
    }

    /// Add a dependency
    pub fn with_dependency(mut self, dep: ResourceId) -> Self {
        self.depends_on.push(dep);
        self
    }

    /// Set the reason
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }

    /// Format for display
    pub fn format_display(&self) -> String {
        let symbol = match self.change_type {
            ChangeType::Create => "+".green(),
            ChangeType::Update => "~".yellow(),
            ChangeType::Replace => "-/+".magenta(),
            ChangeType::Destroy => "-".red(),
            ChangeType::Read => "?".blue(),
            ChangeType::NoOp => " ".normal(),
        };

        format!("{} {} ({})", symbol, self.resource_id, self.provider)
    }
}

// ============================================================================
// Resource Change
// ============================================================================

/// Detailed change information for a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceChange {
    /// Resource address
    pub address: String,

    /// Previous state (if exists)
    pub before: Option<Value>,

    /// Desired state
    pub after: Option<Value>,

    /// Type of change
    pub change_type: ChangeType,

    /// Field-level changes
    pub field_changes: Vec<FieldChange>,

    /// Whether this is a sensitive resource
    pub sensitive: bool,
}

/// Change to a specific field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    /// Field path (e.g., "tags.Name")
    pub path: String,

    /// Old value
    pub old_value: Option<Value>,

    /// New value
    pub new_value: Option<Value>,

    /// Whether this forces replacement
    pub forces_replacement: bool,

    /// Whether this is a sensitive field
    pub sensitive: bool,
}

impl FieldChange {
    /// Format for display
    pub fn format_display(&self) -> String {
        let old = self
            .old_value
            .as_ref()
            .map(|v| format_value(v, self.sensitive))
            .unwrap_or_else(|| "(not set)".to_string());

        let new = self
            .new_value
            .as_ref()
            .map(|v| format_value(v, self.sensitive))
            .unwrap_or_else(|| "(not set)".to_string());

        let force_marker = if self.forces_replacement {
            " # forces replacement"
        } else {
            ""
        };

        format!("    {} = {} -> {}{}", self.path, old, new, force_marker)
    }
}

fn format_value(value: &Value, sensitive: bool) -> String {
    if sensitive {
        return "(sensitive)".to_string();
    }

    match value {
        Value::String(s) => format!("\"{}\"", s),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) => "[...]".to_string(),
        Value::Object(_) => "{...}".to_string(),
    }
}

// ============================================================================
// Execution Plan
// ============================================================================

/// Complete execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Unique plan ID
    pub plan_id: String,

    /// When the plan was created
    pub created_at: DateTime<Utc>,

    /// Actions in execution order
    pub actions: Vec<PlannedAction>,

    /// Detailed resource changes
    pub changes: HashMap<String, ResourceChange>,

    /// Resources to create
    pub to_create: Vec<ResourceId>,

    /// Resources to update
    pub to_update: Vec<ResourceId>,

    /// Resources to replace
    pub to_replace: Vec<ResourceId>,

    /// Resources to destroy
    pub to_destroy: Vec<ResourceId>,

    /// Resources unchanged
    pub unchanged: Vec<ResourceId>,

    /// Output values that will change
    pub output_changes: HashMap<String, (Option<Value>, Option<Value>)>,

    /// Warnings generated during planning
    pub warnings: Vec<String>,

    /// Whether this is a destroy-only plan
    pub is_destroy: bool,

    /// Checksum of the configuration used
    pub config_checksum: Option<String>,
}

impl ExecutionPlan {
    /// Create an empty plan
    pub fn empty() -> Self {
        Self {
            plan_id: uuid::Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            actions: Vec::new(),
            changes: HashMap::new(),
            to_create: Vec::new(),
            to_update: Vec::new(),
            to_replace: Vec::new(),
            to_destroy: Vec::new(),
            unchanged: Vec::new(),
            output_changes: HashMap::new(),
            warnings: Vec::new(),
            is_destroy: false,
            config_checksum: None,
        }
    }

    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        !self.actions.is_empty()
    }

    /// Get total number of changes
    pub fn change_count(&self) -> usize {
        self.actions.len()
    }

    /// Get count by change type
    pub fn count_by_type(&self) -> HashMap<ChangeType, usize> {
        let mut counts = HashMap::new();
        for action in &self.actions {
            *counts.entry(action.change_type).or_insert(0) += 1;
        }
        counts
    }

    /// Get actions in execution order (respecting dependencies)
    pub fn execution_order(&self) -> ProvisioningResult<Vec<&PlannedAction>> {
        // Build dependency graph
        let mut graph: DiGraph<usize, ()> = DiGraph::new();
        let mut indices: HashMap<String, NodeIndex> = HashMap::new();

        // Add nodes
        for (i, action) in self.actions.iter().enumerate() {
            let idx = graph.add_node(i);
            indices.insert(action.resource_id.address(), idx);
        }

        // Add edges
        for (i, action) in self.actions.iter().enumerate() {
            if let Some(&to_idx) = indices.get(&action.resource_id.address()) {
                for dep in &action.depends_on {
                    if let Some(&from_idx) = indices.get(&dep.address()) {
                        graph.add_edge(from_idx, to_idx, ());
                    }
                }
            }
        }

        // Topological sort
        match toposort(&graph, None) {
            Ok(order) => Ok(order
                .into_iter()
                .filter_map(|idx| graph.node_weight(idx).map(|&i| &self.actions[i]))
                .collect()),
            Err(_) => Err(ProvisioningError::DependencyCycle(
                self.actions
                    .iter()
                    .map(|a| a.resource_id.address())
                    .collect(),
            )),
        }
    }

    /// Generate a human-readable summary
    pub fn summary(&self) -> String {
        let mut output = String::new();

        if !self.has_changes() {
            return "No changes. Your infrastructure matches the configuration.".to_string();
        }

        output.push_str(&format!(
            "Plan: {} to add, {} to change, {} to destroy.\n\n",
            self.to_create.len() + self.to_replace.len(),
            self.to_update.len(),
            self.to_destroy.len() + self.to_replace.len()
        ));

        // Group actions by type
        let mut creates: Vec<_> = self
            .actions
            .iter()
            .filter(|a| a.change_type == ChangeType::Create)
            .collect();
        let mut updates: Vec<_> = self
            .actions
            .iter()
            .filter(|a| a.change_type == ChangeType::Update)
            .collect();
        let mut replaces: Vec<_> = self
            .actions
            .iter()
            .filter(|a| a.change_type == ChangeType::Replace)
            .collect();
        let mut destroys: Vec<_> = self
            .actions
            .iter()
            .filter(|a| a.change_type == ChangeType::Destroy)
            .collect();

        // Sort each group
        creates.sort_by(|a, b| a.resource_id.address().cmp(&b.resource_id.address()));
        updates.sort_by(|a, b| a.resource_id.address().cmp(&b.resource_id.address()));
        replaces.sort_by(|a, b| a.resource_id.address().cmp(&b.resource_id.address()));
        destroys.sort_by(|a, b| a.resource_id.address().cmp(&b.resource_id.address()));

        for action in creates {
            output.push_str(&format!(
                "  {} {}\n",
                "+".green(),
                action.resource_id.address().green()
            ));
        }

        for action in updates {
            output.push_str(&format!(
                "  {} {}\n",
                "~".yellow(),
                action.resource_id.address().yellow()
            ));
        }

        for action in replaces {
            output.push_str(&format!(
                "  {} {}\n",
                "-/+".magenta(),
                action.resource_id.address().magenta()
            ));
        }

        for action in destroys {
            output.push_str(&format!(
                "  {} {}\n",
                "-".red(),
                action.resource_id.address().red()
            ));
        }

        if !self.warnings.is_empty() {
            output.push_str("\nWarnings:\n");
            for warning in &self.warnings {
                output.push_str(&format!("  - {}\n", warning.yellow()));
            }
        }

        output
    }

    /// Generate detailed output showing all field changes
    pub fn detailed_summary(&self) -> String {
        let mut output = self.summary();

        if self.has_changes() {
            output.push_str("\nDetailed Changes:\n");

            for action in &self.actions {
                output.push_str(&format!("\n{}\n", action.format_display()));
                output.push_str(&format!("  Reason: {}\n", action.reason));

                if let Some(change) = self.changes.get(&action.resource_id.address()) {
                    for field_change in &change.field_changes {
                        output.push_str(&format!("{}\n", field_change.format_display()));
                    }
                }
            }
        }

        output
    }

    /// Add a warning
    pub fn add_warning(&mut self, warning: impl Into<String>) {
        self.warnings.push(warning.into());
    }

    /// Save the plan to a file for later apply
    pub async fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> ProvisioningResult<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
        tokio::fs::write(path, json).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!("Failed to save plan: {}", e))
        })?;
        Ok(())
    }

    /// Load a plan from a file
    pub async fn load_from_file(path: impl AsRef<std::path::Path>) -> ProvisioningResult<Self> {
        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!("Failed to load plan: {}", e))
        })?;
        let plan: Self = serde_json::from_str(&content)
            .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
        Ok(plan)
    }
}

// ============================================================================
// Plan Builder
// ============================================================================

/// Builder for creating execution plans
pub struct PlanBuilder {
    /// Current state
    current_state: ProvisioningState,

    /// Desired configuration
    desired_config: HashMap<ResourceId, Value>,

    /// Resource dependencies
    dependencies: HashMap<ResourceId, Vec<ResourceId>>,

    /// Force replacement fields per resource type
    force_replacement: HashMap<String, Vec<String>>,

    /// Whether this is a destroy plan
    is_destroy: bool,

    /// Target resources (empty = all)
    targets: Vec<ResourceId>,
}

impl PlanBuilder {
    /// Create a new plan builder
    pub fn new(current_state: ProvisioningState) -> Self {
        Self {
            current_state,
            desired_config: HashMap::new(),
            dependencies: HashMap::new(),
            force_replacement: HashMap::new(),
            is_destroy: false,
            targets: Vec::new(),
        }
    }

    /// Set the desired configuration for a resource
    pub fn with_resource(mut self, id: ResourceId, config: Value) -> Self {
        self.desired_config.insert(id, config);
        self
    }

    /// Add multiple resources
    pub fn with_resources(
        mut self,
        resources: impl IntoIterator<Item = (ResourceId, Value)>,
    ) -> Self {
        self.desired_config.extend(resources);
        self
    }

    /// Set resource dependencies
    pub fn with_dependencies(mut self, id: ResourceId, deps: Vec<ResourceId>) -> Self {
        self.dependencies.insert(id, deps);
        self
    }

    /// Mark as destroy plan
    pub fn destroy(mut self) -> Self {
        self.is_destroy = true;
        self
    }

    /// Target specific resources
    pub fn with_targets(mut self, targets: Vec<ResourceId>) -> Self {
        self.targets = targets;
        self
    }

    /// Build the execution plan
    pub fn build(self) -> ProvisioningResult<ExecutionPlan> {
        let mut plan = ExecutionPlan::empty();
        plan.is_destroy = self.is_destroy;

        if self.is_destroy {
            // Destroy all resources in reverse dependency order
            for (address, resource) in &self.current_state.resources {
                let id = ResourceId::from_address(address)
                    .unwrap_or_else(|| ResourceId::new(&resource.resource_type, address));

                if !self.targets.is_empty() && !self.targets.contains(&id) {
                    continue;
                }

                let action = PlannedAction::destroy(id.clone(), &resource.provider);
                plan.to_destroy.push(id);
                plan.actions.push(action);
            }
        } else {
            // Calculate creates and updates
            for (id, desired) in &self.desired_config {
                if !self.targets.is_empty() && !self.targets.contains(id) {
                    continue;
                }

                if let Some(current) = self.current_state.get_resource(id) {
                    // Resource exists - check for changes
                    let diff = compute_diff(&current.config, desired);

                    if diff.has_changes() {
                        let provider = current.provider.clone();

                        if diff.requires_replacement {
                            let action = PlannedAction::replace(id.clone(), provider, diff);
                            plan.to_replace.push(id.clone());
                            plan.actions.push(action);
                        } else {
                            let action = PlannedAction::update(id.clone(), provider, diff);
                            plan.to_update.push(id.clone());
                            plan.actions.push(action);
                        }
                    } else {
                        plan.unchanged.push(id.clone());
                    }
                } else {
                    // Resource doesn't exist - create it
                    let (provider, _) = parse_provider(&id.resource_type);
                    let diff = ResourceDiff::create(desired.clone());
                    let action = PlannedAction::create(id.clone(), provider, diff);
                    plan.to_create.push(id.clone());
                    plan.actions.push(action);
                }
            }

            // Calculate destroys (resources in state but not in config)
            for (address, resource) in &self.current_state.resources {
                let id = ResourceId::from_address(address)
                    .unwrap_or_else(|| ResourceId::new(&resource.resource_type, address));

                if !self.targets.is_empty() && !self.targets.contains(&id) {
                    continue;
                }

                if !self.desired_config.contains_key(&id) {
                    let action = PlannedAction::destroy(id.clone(), &resource.provider);
                    plan.to_destroy.push(id);
                    plan.actions.push(action);
                }
            }
        }

        // Add dependencies to actions
        for action in &mut plan.actions {
            if let Some(deps) = self.dependencies.get(&action.resource_id) {
                action.depends_on = deps.clone();
            }
        }

        Ok(plan)
    }
}

// Helper function to compute diff between two values
fn compute_diff(current: &Value, desired: &Value) -> ResourceDiff {
    let mut diff = ResourceDiff::no_change();

    match (current, desired) {
        (Value::Object(curr_map), Value::Object(des_map)) => {
            // Check for additions and modifications
            for (key, des_val) in des_map {
                match curr_map.get(key) {
                    Some(curr_val) if curr_val != des_val => {
                        diff.modifications
                            .insert(key.clone(), (curr_val.clone(), des_val.clone()));
                        diff.change_type = ChangeType::Update;
                    }
                    None => {
                        diff.additions.insert(key.clone(), des_val.clone());
                        diff.change_type = ChangeType::Update;
                    }
                    _ => {}
                }
            }

            // Check for deletions
            for key in curr_map.keys() {
                if !des_map.contains_key(key) {
                    diff.deletions.push(key.clone());
                    diff.change_type = ChangeType::Update;
                }
            }
        }
        _ if current != desired => {
            diff.change_type = ChangeType::Update;
        }
        _ => {}
    }

    diff
}

// Parse provider from resource type
fn parse_provider(resource_type: &str) -> (String, String) {
    if let Some(idx) = resource_type.find('_') {
        let (provider, rest) = resource_type.split_at(idx);
        let type_part = rest.strip_prefix('_').unwrap_or(rest);
        (provider.to_string(), type_part.to_string())
    } else {
        (resource_type.to_string(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_plan() {
        let plan = ExecutionPlan::empty();
        assert!(!plan.has_changes());
        assert_eq!(plan.change_count(), 0);
    }

    #[test]
    fn test_plan_builder_create() {
        let state = ProvisioningState::new();
        let id = ResourceId::new("aws_vpc", "main");
        let config = serde_json::json!({"cidr_block": "10.0.0.0/16"});

        let plan = PlanBuilder::new(state)
            .with_resource(id.clone(), config)
            .build()
            .unwrap();

        assert!(plan.has_changes());
        assert_eq!(plan.to_create.len(), 1);
        assert!(plan.to_create.contains(&id));
    }

    #[test]
    fn test_plan_builder_destroy() {
        let mut state = ProvisioningState::new();
        let id = ResourceId::new("aws_vpc", "main");

        state.add_resource(super::super::state::ResourceState::new(
            id.clone(),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({}),
        ));

        let plan = PlanBuilder::new(state).destroy().build().unwrap();

        assert!(plan.has_changes());
        assert_eq!(plan.to_destroy.len(), 1);
    }

    #[test]
    fn test_compute_diff_no_change() {
        let current = serde_json::json!({"key": "value"});
        let desired = serde_json::json!({"key": "value"});

        let diff = compute_diff(&current, &desired);
        assert!(!diff.has_changes());
    }

    #[test]
    fn test_compute_diff_addition() {
        let current = serde_json::json!({"key": "value"});
        let desired = serde_json::json!({"key": "value", "new_key": "new_value"});

        let diff = compute_diff(&current, &desired);
        assert!(diff.has_changes());
        assert!(diff.additions.contains_key("new_key"));
    }

    #[test]
    fn test_compute_diff_modification() {
        let current = serde_json::json!({"key": "old_value"});
        let desired = serde_json::json!({"key": "new_value"});

        let diff = compute_diff(&current, &desired);
        assert!(diff.has_changes());
        assert!(diff.modifications.contains_key("key"));
    }
}
