//! Infrastructure Configuration
//!
//! This module handles parsing and validation of infrastructure configuration files.
//! It supports extended YAML format compatible with Ansible-style templates.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{ProvisioningError, ProvisioningResult};
use super::state::ResourceId;

// ============================================================================
// Dependency Edge Types
// ============================================================================

/// Edge type in dependency graph
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyEdge {
    /// Implicit dependency from template reference (e.g., `{{ resources.aws_vpc.main.id }}`)
    Implicit,
    /// Explicit dependency from `depends_on` declaration
    Explicit,
}

/// Reference type extracted from template strings
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReferenceType {
    /// Resource reference: `resources.TYPE.NAME.attr`
    Resource {
        resource_type: String,
        name: String,
        attribute: Option<String>,
    },
    /// Variable reference: `variables.NAME`
    Variable { name: String },
    /// Data source reference: `data.TYPE.NAME.attr`
    DataSource {
        data_type: String,
        name: String,
        attribute: Option<String>,
    },
    /// Local value reference: `locals.NAME`
    Local { name: String },
    /// Self-reference in provisioner context: `self.ATTR`
    SelfAttribute { attribute: String },
    /// Path reference: `path.module`, `path.root`, `path.cwd`
    Path { path_type: String },
    /// Terraform reference: `terraform.workspace`
    Terraform { attribute: String },
}

// ============================================================================
// Infrastructure Configuration
// ============================================================================

/// Complete infrastructure configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct InfrastructureConfig {
    /// Provider configurations
    #[serde(default)]
    pub providers: HashMap<String, Value>,

    /// Variables for templating
    #[serde(default)]
    pub variables: HashMap<String, Value>,

    /// Resource definitions grouped by type
    #[serde(default)]
    pub resources: HashMap<String, HashMap<String, Value>>,

    /// Data source queries
    #[serde(default)]
    pub data: HashMap<String, HashMap<String, Value>>,

    /// Output values
    #[serde(default)]
    pub outputs: HashMap<String, OutputConfig>,

    /// Terraform backend configuration (for state import)
    #[serde(default)]
    pub terraform: Option<TerraformConfig>,

    /// Local settings
    #[serde(default)]
    pub locals: HashMap<String, Value>,

    /// Moved blocks for resource address refactoring
    #[serde(default)]
    pub moved: Vec<super::moved::MovedBlock>,
}

/// Configuration for an output value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// The value expression
    pub value: Value,

    /// Optional description
    #[serde(default)]
    pub description: Option<String>,

    /// Whether the output is sensitive
    #[serde(default)]
    pub sensitive: bool,

    /// Dependencies (resources that must exist for this output)
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// Terraform backend configuration for state import
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerraformConfig {
    /// Backend type (local, s3, gcs, azurerm, etc.)
    pub backend: String,

    /// Backend configuration
    #[serde(flatten)]
    pub config: Value,
}


impl InfrastructureConfig {
    /// Create a new empty configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from a file
    pub async fn from_file(path: impl AsRef<Path>) -> ProvisioningResult<Self> {
        let path = path.as_ref();

        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            ProvisioningError::ConfigError(format!("Failed to read config file: {}", e))
        })?;

        Self::from_str(&content)
    }

    /// Parse configuration from a string
    pub fn from_str(content: &str) -> ProvisioningResult<Self> {
        let config: Self = serde_yaml::from_str(content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> ProvisioningResult<()> {
        // Validate provider configurations
        for (name, config) in &self.providers {
            self.validate_provider(name, config)?;
        }

        // Validate resource configurations
        for (resource_type, resources) in &self.resources {
            for (name, config) in resources {
                self.validate_resource(resource_type, name, config)?;
            }
        }

        // Check for dependency cycles
        self.check_dependency_cycles()?;

        Ok(())
    }

    /// Validate a provider configuration
    fn validate_provider(&self, name: &str, _config: &Value) -> ProvisioningResult<()> {
        // Validate provider name
        let valid_providers = [
            "aws",
            "azure",
            "azurerm",
            "google",
            "gcp",
            "kubernetes",
            "local",
        ];

        if !valid_providers.iter().any(|p| name.starts_with(p)) {
            // Allow custom providers but warn
            tracing::warn!("Unknown provider: {}. This may be a custom provider.", name);
        }

        Ok(())
    }

    /// Validate a resource configuration
    fn validate_resource(
        &self,
        resource_type: &str,
        name: &str,
        config: &Value,
    ) -> ProvisioningResult<()> {
        // Validate resource name
        if name.is_empty() {
            return Err(ProvisioningError::ValidationError(format!(
                "Resource name cannot be empty for type {}",
                resource_type
            )));
        }

        // Validate name format (alphanumeric with underscores)
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ProvisioningError::ValidationError(format!(
                "Invalid resource name '{}': must contain only alphanumeric characters, underscores, and hyphens",
                name
            )));
        }

        // Validate config is an object
        if !config.is_object() {
            return Err(ProvisioningError::ValidationError(format!(
                "Resource {}.{} configuration must be an object",
                resource_type, name
            )));
        }

        Ok(())
    }

    /// Check for dependency cycles
    fn check_dependency_cycles(&self) -> ProvisioningResult<()> {
        // Build dependency graph
        let deps = self.extract_dependencies();

        // Use DFS to detect cycles
        let mut visited = HashMap::new();
        let mut rec_stack = HashMap::new();

        for address in deps.keys() {
            if self.has_cycle(address, &deps, &mut visited, &mut rec_stack) {
                return Err(ProvisioningError::DependencyCycle(vec![address.clone()]));
            }
        }

        Ok(())
    }

    /// DFS helper for cycle detection
    fn has_cycle(
        &self,
        node: &str,
        deps: &HashMap<String, Vec<String>>,
        visited: &mut HashMap<String, bool>,
        rec_stack: &mut HashMap<String, bool>,
    ) -> bool {
        if rec_stack.get(node).copied().unwrap_or(false) {
            return true;
        }

        if visited.get(node).copied().unwrap_or(false) {
            return false;
        }

        visited.insert(node.to_string(), true);
        rec_stack.insert(node.to_string(), true);

        if let Some(neighbors) = deps.get(node) {
            for neighbor in neighbors {
                if self.has_cycle(neighbor, deps, visited, rec_stack) {
                    return true;
                }
            }
        }

        rec_stack.insert(node.to_string(), false);
        false
    }

    /// Extract all resource addresses
    pub fn resource_addresses(&self) -> Vec<ResourceId> {
        let mut addresses = Vec::new();

        for (resource_type, resources) in &self.resources {
            for name in resources.keys() {
                addresses.push(ResourceId::new(resource_type, name));
            }
        }

        addresses
    }

    /// Extract dependencies from configurations
    pub fn extract_dependencies(&self) -> HashMap<String, Vec<String>> {
        let mut deps: HashMap<String, Vec<String>> = HashMap::new();

        for (resource_type, resources) in &self.resources {
            for (name, config) in resources {
                let address = format!("{}.{}", resource_type, name);
                let mut resource_deps = self.extract_deps_from_value(config);

                // Also check explicit depends_on
                if let Some(explicit) = config.get("depends_on") {
                    if let Some(arr) = explicit.as_array() {
                        for dep in arr {
                            if let Some(s) = dep.as_str() {
                                if !resource_deps.contains(&s.to_string()) {
                                    resource_deps.push(s.to_string());
                                }
                            }
                        }
                    }
                }

                deps.insert(address, resource_deps);
            }
        }

        deps
    }

    /// Extract dependencies from a value by looking for template references
    fn extract_deps_from_value(&self, value: &Value) -> Vec<String> {
        let mut deps = Vec::new();

        match value {
            Value::String(s) => {
                // Look for {{ resources.TYPE.NAME.* }} patterns
                deps.extend(self.extract_refs_from_string(s));
            }
            Value::Array(arr) => {
                for item in arr {
                    deps.extend(self.extract_deps_from_value(item));
                }
            }
            Value::Object(map) => {
                for v in map.values() {
                    deps.extend(self.extract_deps_from_value(v));
                }
            }
            _ => {}
        }

        deps
    }

    /// Extract resource references from a template string (returns resource addresses only)
    fn extract_refs_from_string(&self, s: &str) -> Vec<String> {
        self.extract_all_references(s)
            .into_iter()
            .filter_map(|r| match r {
                ReferenceType::Resource {
                    resource_type,
                    name,
                    ..
                } => Some(format!("{}.{}", resource_type, name)),
                _ => None,
            })
            .collect()
    }

    /// Extract all reference types from a template string
    ///
    /// Supports:
    /// - `{{ resources.TYPE.NAME.attr }}` - Resource references
    /// - `{{ variables.NAME }}` - Variable references
    /// - `{{ data.TYPE.NAME.attr }}` - Data source references
    /// - `{{ locals.NAME }}` - Local value references
    pub fn extract_all_references(&self, s: &str) -> Vec<ReferenceType> {
        let mut refs = HashSet::new();
        let mut remaining = s;

        while let Some(start_idx) = remaining.find("{{") {
            let after_start = &remaining[start_idx + 2..];

            if let Some(end_idx) = after_start.find("}}") {
                let ref_content = after_start[..end_idx].trim();

                // Try to parse different reference types
                if let Some(reference) = self.parse_reference(ref_content) {
                    refs.insert(reference);
                }

                remaining = &after_start[end_idx + 2..];
            } else {
                break;
            }
        }

        refs.into_iter().collect()
    }

    /// Parse a reference expression into a ReferenceType
    fn parse_reference(&self, expr: &str) -> Option<ReferenceType> {
        self.parse_reference_type(expr)
    }

    /// Parse a reference expression into a ReferenceType (public API)
    ///
    /// Supports:
    /// - `resources.TYPE.NAME.attr`
    /// - `variables.NAME`
    /// - `data.TYPE.NAME.attr`
    /// - `locals.NAME`
    /// - `self.attr`
    /// - `path.module`, `path.root`, `path.cwd`
    /// - `terraform.workspace`
    pub fn parse_reference_type(&self, expr: &str) -> Option<ReferenceType> {
        let parts: Vec<&str> = expr.split('.').collect();

        if parts.is_empty() {
            return None;
        }

        match parts[0] {
            "resources" if parts.len() >= 3 => {
                let resource_type = parts[1].to_string();
                let name = parts[2].to_string();
                let attribute = if parts.len() > 3 {
                    Some(parts[3..].join("."))
                } else {
                    None
                };
                Some(ReferenceType::Resource {
                    resource_type,
                    name,
                    attribute,
                })
            }
            "variables" if parts.len() >= 2 => Some(ReferenceType::Variable {
                name: parts[1..].join("."),
            }),
            "data" if parts.len() >= 3 => {
                let data_type = parts[1].to_string();
                let name = parts[2].to_string();
                let attribute = if parts.len() > 3 {
                    Some(parts[3..].join("."))
                } else {
                    None
                };
                Some(ReferenceType::DataSource {
                    data_type,
                    name,
                    attribute,
                })
            }
            "locals" if parts.len() >= 2 => Some(ReferenceType::Local {
                name: parts[1..].join("."),
            }),
            "self" if parts.len() >= 2 => Some(ReferenceType::SelfAttribute {
                attribute: parts[1..].join("."),
            }),
            "path" if parts.len() >= 2 => {
                let path_type = parts[1].to_string();
                if ["module", "root", "cwd"].contains(&path_type.as_str()) {
                    Some(ReferenceType::Path { path_type })
                } else {
                    None
                }
            }
            "terraform" if parts.len() >= 2 => {
                let attribute = parts[1].to_string();
                if attribute == "workspace" {
                    Some(ReferenceType::Terraform { attribute })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Build complete dependency graph with petgraph
    ///
    /// Returns a directed graph where:
    /// - Nodes are resource IDs
    /// - Edges represent dependencies (from dependent -> dependency)
    /// - Edge weights indicate whether the dependency is implicit or explicit
    pub fn dependency_graph(&self) -> ProvisioningResult<DiGraph<ResourceId, DependencyEdge>> {
        let mut graph = DiGraph::new();
        let mut node_indices: HashMap<String, NodeIndex> = HashMap::new();

        // First pass: add all resources as nodes
        for (resource_type, resources) in &self.resources {
            for name in resources.keys() {
                let id = ResourceId::new(resource_type, name);
                let idx = graph.add_node(id.clone());
                node_indices.insert(id.address(), idx);
            }
        }

        // Second pass: add edges for dependencies
        for (resource_type, resources) in &self.resources {
            for (name, config) in resources {
                let address = format!("{}.{}", resource_type, name);
                let source_idx = node_indices[&address];

                // Extract implicit dependencies from template references
                let implicit_deps = self.extract_deps_from_value(config);
                for dep_address in &implicit_deps {
                    if let Some(&target_idx) = node_indices.get(dep_address) {
                        // Edge goes from source (dependent) to target (dependency)
                        graph.add_edge(source_idx, target_idx, DependencyEdge::Implicit);
                    }
                }

                // Extract explicit dependencies from depends_on
                if let Some(explicit) = config.get("depends_on") {
                    if let Some(arr) = explicit.as_array() {
                        for dep in arr {
                            if let Some(dep_address) = dep.as_str() {
                                if let Some(&target_idx) = node_indices.get(dep_address) {
                                    // Only add if not already added as implicit
                                    if !implicit_deps.contains(&dep_address.to_string()) {
                                        graph.add_edge(
                                            source_idx,
                                            target_idx,
                                            DependencyEdge::Explicit,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(graph)
    }

    /// Get topological order for resource resolution
    ///
    /// Returns resources sorted so that dependencies come before dependents.
    /// This is the order in which resources should be created.
    pub fn resolution_order(&self) -> ProvisioningResult<Vec<ResourceId>> {
        let graph = self.dependency_graph()?;

        // Perform topological sort
        match toposort(&graph, None) {
            Ok(sorted_indices) => {
                // Reverse to get dependencies first (toposort returns dependents first)
                let mut result: Vec<ResourceId> = sorted_indices
                    .into_iter()
                    .map(|idx| graph[idx].clone())
                    .collect();
                result.reverse();
                Ok(result)
            }
            Err(cycle) => {
                // Extract the node that's part of a cycle
                let cycle_node = graph[cycle.node_id()].clone();
                Err(ProvisioningError::DependencyCycle(vec![
                    cycle_node.address()
                ]))
            }
        }
    }

    /// Validate all template references exist
    ///
    /// Checks that all references in template strings point to defined:
    /// - Resources (in `resources` section)
    /// - Variables (in `variables` section)
    /// - Data sources (in `data` section)
    /// - Locals (in `locals` section)
    pub fn validate_references(&self) -> ProvisioningResult<()> {
        let mut missing_refs = Vec::new();

        // Collect all defined identifiers
        let defined_resources: HashSet<String> = self
            .resources
            .iter()
            .flat_map(|(t, r)| {
                let t = t.clone();
                r.keys().map(move |n| format!("{}.{}", t, n))
            })
            .collect();

        let defined_variables: HashSet<&String> = self.variables.keys().collect();

        let defined_data_sources: HashSet<String> = self
            .data
            .iter()
            .flat_map(|(t, d)| {
                let t = t.clone();
                d.keys().map(move |n| format!("{}.{}", t, n))
            })
            .collect();

        let defined_locals: HashSet<&String> = self.locals.keys().collect();

        // Check all resource configurations
        for (resource_type, resources) in &self.resources {
            for (name, config) in resources {
                let refs = self.extract_all_refs_from_value(config);
                for reference in refs {
                    match &reference {
                        ReferenceType::Resource {
                            resource_type: rt,
                            name: n,
                            ..
                        } => {
                            let addr = format!("{}.{}", rt, n);
                            if !defined_resources.contains(&addr) {
                                missing_refs.push(format!(
                                    "Resource {}.{} references undefined resource: {}",
                                    resource_type, name, addr
                                ));
                            }
                        }
                        ReferenceType::Variable { name: var_name } => {
                            // Handle nested variable paths (take first part)
                            let root_name = var_name.split('.').next().unwrap_or(var_name);
                            if !defined_variables.contains(&root_name.to_string()) {
                                missing_refs.push(format!(
                                    "Resource {}.{} references undefined variable: {}",
                                    resource_type, name, var_name
                                ));
                            }
                        }
                        ReferenceType::DataSource {
                            data_type: dt,
                            name: n,
                            ..
                        } => {
                            let addr = format!("{}.{}", dt, n);
                            if !defined_data_sources.contains(&addr) {
                                missing_refs.push(format!(
                                    "Resource {}.{} references undefined data source: {}",
                                    resource_type, name, addr
                                ));
                            }
                        }
                        ReferenceType::Local { name: local_name } => {
                            // Handle nested local paths (take first part)
                            let root_name = local_name.split('.').next().unwrap_or(local_name);
                            if !defined_locals.contains(&root_name.to_string()) {
                                missing_refs.push(format!(
                                    "Resource {}.{} references undefined local: {}",
                                    resource_type, name, local_name
                                ));
                            }
                        }
                        // self.*, path.*, terraform.* are resolved at runtime, no validation needed
                        ReferenceType::SelfAttribute { .. }
                        | ReferenceType::Path { .. }
                        | ReferenceType::Terraform { .. } => {}
                    }
                }
            }
        }

        // Check output configurations
        for (output_name, output_config) in &self.outputs {
            let refs = self.extract_all_refs_from_value(&output_config.value);
            for reference in refs {
                match &reference {
                    ReferenceType::Resource {
                        resource_type: rt,
                        name: n,
                        ..
                    } => {
                        let addr = format!("{}.{}", rt, n);
                        if !defined_resources.contains(&addr) {
                            missing_refs.push(format!(
                                "Output {} references undefined resource: {}",
                                output_name, addr
                            ));
                        }
                    }
                    ReferenceType::Variable { name: var_name } => {
                        let root_name = var_name.split('.').next().unwrap_or(var_name);
                        if !defined_variables.contains(&root_name.to_string()) {
                            missing_refs.push(format!(
                                "Output {} references undefined variable: {}",
                                output_name, var_name
                            ));
                        }
                    }
                    ReferenceType::DataSource {
                        data_type: dt,
                        name: n,
                        ..
                    } => {
                        let addr = format!("{}.{}", dt, n);
                        if !defined_data_sources.contains(&addr) {
                            missing_refs.push(format!(
                                "Output {} references undefined data source: {}",
                                output_name, addr
                            ));
                        }
                    }
                    ReferenceType::Local { name: local_name } => {
                        let root_name = local_name.split('.').next().unwrap_or(local_name);
                        if !defined_locals.contains(&root_name.to_string()) {
                            missing_refs.push(format!(
                                "Output {} references undefined local: {}",
                                output_name, local_name
                            ));
                        }
                    }
                    // self.*, path.*, terraform.* are resolved at runtime, no validation needed
                    ReferenceType::SelfAttribute { .. }
                    | ReferenceType::Path { .. }
                    | ReferenceType::Terraform { .. } => {}
                }
            }
        }

        if missing_refs.is_empty() {
            Ok(())
        } else {
            Err(ProvisioningError::ValidationError(missing_refs.join("\n")))
        }
    }

    /// Extract all reference types from a value (recursive helper)
    fn extract_all_refs_from_value(&self, value: &Value) -> Vec<ReferenceType> {
        let mut refs = Vec::new();

        match value {
            Value::String(s) => {
                refs.extend(self.extract_all_references(s));
            }
            Value::Array(arr) => {
                for item in arr {
                    refs.extend(self.extract_all_refs_from_value(item));
                }
            }
            Value::Object(map) => {
                for v in map.values() {
                    refs.extend(self.extract_all_refs_from_value(v));
                }
            }
            _ => {}
        }

        refs
    }

    /// Get data source dependencies for a resource
    pub fn get_data_dependencies(&self, resource_address: &str) -> Vec<String> {
        if let Some(id) = ResourceId::from_address(resource_address) {
            if let Some(resources) = self.resources.get(&id.resource_type) {
                if let Some(config) = resources.get(&id.name) {
                    return self
                        .extract_all_refs_from_value(config)
                        .into_iter()
                        .filter_map(|r| match r {
                            ReferenceType::DataSource {
                                data_type, name, ..
                            } => Some(format!("data.{}.{}", data_type, name)),
                            _ => None,
                        })
                        .collect();
                }
            }
        }
        Vec::new()
    }

    /// Get a resource configuration by address
    pub fn get_resource(&self, address: &str) -> Option<&Value> {
        if let Some(id) = ResourceId::from_address(address) {
            self.resources
                .get(&id.resource_type)
                .and_then(|r| r.get(&id.name))
        } else {
            None
        }
    }

    /// Get a variable value
    pub fn get_variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    /// Merge another configuration into this one
    pub fn merge(&mut self, other: InfrastructureConfig) {
        // Merge providers (other takes precedence)
        for (name, config) in other.providers {
            self.providers.insert(name, config);
        }

        // Merge variables
        for (name, value) in other.variables {
            self.variables.insert(name, value);
        }

        // Merge resources
        for (resource_type, resources) in other.resources {
            let entry = self.resources.entry(resource_type).or_default();
            for (name, config) in resources {
                entry.insert(name, config);
            }
        }

        // Merge outputs
        for (name, config) in other.outputs {
            self.outputs.insert(name, config);
        }

        // Merge locals
        for (name, value) in other.locals {
            self.locals.insert(name, value);
        }

        // Merge moved blocks
        self.moved.extend(other.moved);

        // Take terraform config from other if this doesn't have one
        if self.terraform.is_none() {
            self.terraform = other.terraform;
        }
    }

    /// Count total resources
    pub fn resource_count(&self) -> usize {
        self.resources.values().map(|r| r.len()).sum()
    }
}

// ============================================================================
// Configuration Builder
// ============================================================================

/// Builder for creating infrastructure configurations programmatically
pub struct InfrastructureConfigBuilder {
    config: InfrastructureConfig,
}

impl InfrastructureConfigBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            config: InfrastructureConfig::new(),
        }
    }

    /// Add a provider
    pub fn provider(mut self, name: impl Into<String>, config: Value) -> Self {
        self.config.providers.insert(name.into(), config);
        self
    }

    /// Add a variable
    pub fn variable(mut self, name: impl Into<String>, value: Value) -> Self {
        self.config.variables.insert(name.into(), value);
        self
    }

    /// Add a resource
    pub fn resource(
        mut self,
        resource_type: impl Into<String>,
        name: impl Into<String>,
        config: Value,
    ) -> Self {
        let resource_type = resource_type.into();
        let entry = self.config.resources.entry(resource_type).or_default();
        entry.insert(name.into(), config);
        self
    }

    /// Add an output
    pub fn output(mut self, name: impl Into<String>, output: OutputConfig) -> Self {
        self.config.outputs.insert(name.into(), output);
        self
    }

    /// Build the configuration
    pub fn build(self) -> ProvisioningResult<InfrastructureConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}

impl Default for InfrastructureConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_config() {
        let config = InfrastructureConfig::new();
        assert!(config.providers.is_empty());
        assert!(config.resources.is_empty());
        assert_eq!(config.resource_count(), 0);
    }

    #[test]
    fn test_parse_yaml() {
        let yaml = r#"
providers:
  aws:
    region: us-east-1

variables:
  vpc_cidr: "10.0.0.0/16"

resources:
  aws_vpc:
    main:
      cidr_block: "{{ variables.vpc_cidr }}"
      enable_dns_hostnames: true

outputs:
  vpc_id:
    value: "{{ resources.aws_vpc.main.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        assert!(config.providers.contains_key("aws"));
        assert!(config.variables.contains_key("vpc_cidr"));
        assert_eq!(config.resource_count(), 1);
        assert!(config.outputs.contains_key("vpc_id"));
    }

    #[test]
    fn test_extract_refs() {
        let config = InfrastructureConfig::new();
        let refs = config.extract_refs_from_string("{{ resources.aws_vpc.main.id }}");
        assert_eq!(refs, vec!["aws_vpc.main"]);
    }

    #[test]
    fn test_extract_multiple_refs() {
        let config = InfrastructureConfig::new();
        let refs = config.extract_refs_from_string(
            "VPC: {{ resources.aws_vpc.main.id }}, Subnet: {{ resources.aws_subnet.public.id }}",
        );
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"aws_vpc.main".to_string()));
        assert!(refs.contains(&"aws_subnet.public".to_string()));
    }

    #[test]
    fn test_builder() {
        let config = InfrastructureConfigBuilder::new()
            .provider("aws", serde_json::json!({"region": "us-east-1"}))
            .variable("environment", serde_json::json!("production"))
            .resource(
                "aws_vpc",
                "main",
                serde_json::json!({"cidr_block": "10.0.0.0/16"}),
            )
            .build()
            .unwrap();

        assert!(config.providers.contains_key("aws"));
        assert!(config.variables.contains_key("environment"));
        assert_eq!(config.resource_count(), 1);
    }

    #[test]
    fn test_resource_addresses() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      cidr_block: "10.0.1.0/24"
    private:
      cidr_block: "10.0.2.0/24"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let addresses = config.resource_addresses();

        assert_eq!(addresses.len(), 3);
    }

    #[test]
    fn test_invalid_resource_name() {
        let yaml = r#"
resources:
  aws_vpc:
    "invalid name!":
      cidr_block: "10.0.0.0/16"
"#;

        let result = InfrastructureConfig::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_configs() {
        let mut config1 = InfrastructureConfigBuilder::new()
            .provider("aws", serde_json::json!({"region": "us-east-1"}))
            .variable("env", serde_json::json!("dev"))
            .build()
            .unwrap();

        let config2 = InfrastructureConfigBuilder::new()
            .provider("aws", serde_json::json!({"region": "us-west-2"}))
            .variable("name", serde_json::json!("production"))
            .resource("aws_vpc", "main", serde_json::json!({}))
            .build()
            .unwrap();

        config1.merge(config2);

        // Provider from config2 overwrites
        assert_eq!(
            config1.providers.get("aws").unwrap().get("region").unwrap(),
            "us-west-2"
        );

        // Both variables present
        assert!(config1.variables.contains_key("env"));
        assert!(config1.variables.contains_key("name"));

        // Resource from config2 added
        assert_eq!(config1.resource_count(), 1);
    }

    #[test]
    fn test_dependency_extraction() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
      cidr_block: "10.0.1.0/24"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let deps = config.extract_dependencies();

        assert!(deps
            .get("aws_subnet.public")
            .unwrap()
            .contains(&"aws_vpc.main".to_string()));
    }

    // ========================================================================
    // New Tests for Template Resolution Integration
    // ========================================================================

    #[test]
    fn test_extract_variable_references() {
        let config = InfrastructureConfig::new();
        let refs = config.extract_all_references("cidr: {{ variables.vpc_cidr }}");

        assert_eq!(refs.len(), 1);
        assert!(refs
            .iter()
            .any(|r| matches!(r, ReferenceType::Variable { name } if name == "vpc_cidr")));
    }

    #[test]
    fn test_extract_data_source_references() {
        let config = InfrastructureConfig::new();
        let refs = config.extract_all_references("ami: {{ data.aws_ami.latest.id }}");

        assert_eq!(refs.len(), 1);
        assert!(refs.iter().any(|r| matches!(
            r,
            ReferenceType::DataSource { data_type, name, attribute }
            if data_type == "aws_ami" && name == "latest" && attribute == &Some("id".to_string())
        )));
    }

    #[test]
    fn test_extract_local_references() {
        let config = InfrastructureConfig::new();
        let refs = config.extract_all_references("tags: {{ locals.common_tags }}");

        assert_eq!(refs.len(), 1);
        assert!(refs
            .iter()
            .any(|r| matches!(r, ReferenceType::Local { name } if name == "common_tags")));
    }

    #[test]
    fn test_extract_nested_attribute_references() {
        let config = InfrastructureConfig::new();
        let refs = config.extract_all_references("{{ resources.aws_vpc.main.cidr_block.primary }}");

        assert_eq!(refs.len(), 1);
        assert!(refs.iter().any(|r| matches!(
            r,
            ReferenceType::Resource { resource_type, name, attribute }
            if resource_type == "aws_vpc" && name == "main" && attribute == &Some("cidr_block.primary".to_string())
        )));
    }

    #[test]
    fn test_extract_mixed_references() {
        let config = InfrastructureConfig::new();
        let template = r#"
            VPC: {{ resources.aws_vpc.main.id }}
            CIDR: {{ variables.vpc_cidr }}
            AMI: {{ data.aws_ami.latest.image_id }}
            Tags: {{ locals.common_tags }}
        "#;

        let refs = config.extract_all_references(template);

        assert_eq!(refs.len(), 4);
        assert!(refs
            .iter()
            .any(|r| matches!(r, ReferenceType::Resource { .. })));
        assert!(refs
            .iter()
            .any(|r| matches!(r, ReferenceType::Variable { .. })));
        assert!(refs
            .iter()
            .any(|r| matches!(r, ReferenceType::DataSource { .. })));
        assert!(refs
            .iter()
            .any(|r| matches!(r, ReferenceType::Local { .. })));
    }

    #[test]
    fn test_resolution_order_simple_chain() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
  aws_instance:
    web:
      subnet_id: "{{ resources.aws_subnet.public.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let order = config.resolution_order().unwrap();

        // VPC must come before Subnet, Subnet must come before Instance
        let vpc_pos = order
            .iter()
            .position(|r| r.address() == "aws_vpc.main")
            .unwrap();
        let subnet_pos = order
            .iter()
            .position(|r| r.address() == "aws_subnet.public")
            .unwrap();
        let instance_pos = order
            .iter()
            .position(|r| r.address() == "aws_instance.web")
            .unwrap();

        assert!(vpc_pos < subnet_pos, "VPC must be resolved before Subnet");
        assert!(
            subnet_pos < instance_pos,
            "Subnet must be resolved before Instance"
        );
    }

    #[test]
    fn test_resolution_order_complex_dependencies() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_internet_gateway:
    main:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
  aws_route_table:
    main:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
      gateway_id: "{{ resources.aws_internet_gateway.main.id }}"
  aws_route_table_association:
    public:
      subnet_id: "{{ resources.aws_subnet.public.id }}"
      route_table_id: "{{ resources.aws_route_table.main.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let order = config.resolution_order().unwrap();

        // VPC must be first (no dependencies)
        let vpc_pos = order
            .iter()
            .position(|r| r.address() == "aws_vpc.main")
            .unwrap();
        let igw_pos = order
            .iter()
            .position(|r| r.address() == "aws_internet_gateway.main")
            .unwrap();
        let subnet_pos = order
            .iter()
            .position(|r| r.address() == "aws_subnet.public")
            .unwrap();
        let rt_pos = order
            .iter()
            .position(|r| r.address() == "aws_route_table.main")
            .unwrap();
        let assoc_pos = order
            .iter()
            .position(|r| r.address() == "aws_route_table_association.public")
            .unwrap();

        assert!(vpc_pos < igw_pos);
        assert!(vpc_pos < subnet_pos);
        assert!(vpc_pos < rt_pos);
        assert!(igw_pos < rt_pos);
        assert!(subnet_pos < assoc_pos);
        assert!(rt_pos < assoc_pos);
    }

    #[test]
    fn test_cycle_detection() {
        let yaml = r#"
resources:
  aws_resource:
    a:
      depends_on: ["aws_resource.b"]
    b:
      depends_on: ["aws_resource.c"]
    c:
      depends_on: ["aws_resource.a"]
"#;

        let config = InfrastructureConfig::from_str(yaml);
        assert!(config.is_err());

        if let Err(e) = config {
            assert!(matches!(e, ProvisioningError::DependencyCycle(_)));
        }
    }

    #[test]
    fn test_cycle_detection_via_implicit_deps() {
        let yaml = r#"
resources:
  aws_resource:
    a:
      ref: "{{ resources.aws_resource.b.id }}"
    b:
      ref: "{{ resources.aws_resource.a.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml);
        assert!(config.is_err());

        if let Err(e) = config {
            assert!(matches!(e, ProvisioningError::DependencyCycle(_)));
        }
    }

    #[test]
    fn test_validate_references_success() {
        let yaml = r#"
variables:
  vpc_cidr: "10.0.0.0/16"

locals:
  common_tags:
    Environment: production

data:
  aws_ami:
    latest: {}

resources:
  aws_vpc:
    main:
      cidr_block: "{{ variables.vpc_cidr }}"
      tags: "{{ locals.common_tags }}"
  aws_instance:
    web:
      ami: "{{ data.aws_ami.latest.id }}"
      vpc_id: "{{ resources.aws_vpc.main.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        assert!(config.validate_references().is_ok());
    }

    #[test]
    fn test_validate_references_missing_resource() {
        let yaml = r#"
resources:
  aws_instance:
    web:
      vpc_id: "{{ resources.aws_vpc.nonexistent.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let result = config.validate_references();

        assert!(result.is_err());
        if let Err(ProvisioningError::ValidationError(msg)) = result {
            assert!(msg.contains("undefined resource"));
            assert!(msg.contains("aws_vpc.nonexistent"));
        }
    }

    #[test]
    fn test_validate_references_missing_variable() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "{{ variables.undefined_var }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let result = config.validate_references();

        assert!(result.is_err());
        if let Err(ProvisioningError::ValidationError(msg)) = result {
            assert!(msg.contains("undefined variable"));
            assert!(msg.contains("undefined_var"));
        }
    }

    #[test]
    fn test_validate_references_missing_data_source() {
        let yaml = r#"
resources:
  aws_instance:
    web:
      ami: "{{ data.aws_ami.nonexistent.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let result = config.validate_references();

        assert!(result.is_err());
        if let Err(ProvisioningError::ValidationError(msg)) = result {
            assert!(msg.contains("undefined data source"));
            assert!(msg.contains("aws_ami.nonexistent"));
        }
    }

    #[test]
    fn test_validate_references_missing_local() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      tags: "{{ locals.undefined_local }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let result = config.validate_references();

        assert!(result.is_err());
        if let Err(ProvisioningError::ValidationError(msg)) = result {
            assert!(msg.contains("undefined local"));
            assert!(msg.contains("undefined_local"));
        }
    }

    #[test]
    fn test_dependency_graph_edge_types() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
  aws_instance:
    web:
      depends_on: ["aws_subnet.public"]
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let graph = config.dependency_graph().unwrap();

        // Check that we have the expected number of nodes and edges
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);

        // Verify edge types
        let mut has_implicit = false;
        let mut has_explicit = false;

        for edge in graph.edge_references() {
            match edge.weight() {
                DependencyEdge::Implicit => has_implicit = true,
                DependencyEdge::Explicit => has_explicit = true,
            }
        }

        assert!(
            has_implicit,
            "Should have implicit dependency from subnet to vpc"
        );
        assert!(
            has_explicit,
            "Should have explicit dependency from instance to subnet"
        );
    }

    #[test]
    fn test_mixed_implicit_explicit_dependencies() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
      depends_on: ["aws_vpc.main"]
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let graph = config.dependency_graph().unwrap();

        // Should only have one edge (implicit takes precedence when same dep)
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn test_get_data_dependencies() {
        let yaml = r#"
data:
  aws_ami:
    latest: {}
    backup: {}

resources:
  aws_instance:
    web:
      ami: "{{ data.aws_ami.latest.id }}"
      backup_ami: "{{ data.aws_ami.backup.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let data_deps = config.get_data_dependencies("aws_instance.web");

        assert_eq!(data_deps.len(), 2);
        assert!(data_deps.contains(&"data.aws_ami.latest".to_string()));
        assert!(data_deps.contains(&"data.aws_ami.backup".to_string()));
    }

    #[test]
    fn test_resolution_order_no_dependencies() {
        let yaml = r#"
resources:
  aws_vpc:
    vpc1:
      cidr_block: "10.0.0.0/16"
    vpc2:
      cidr_block: "10.1.0.0/16"
    vpc3:
      cidr_block: "10.2.0.0/16"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let order = config.resolution_order().unwrap();

        // All VPCs should be present (order doesn't matter since no deps)
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn test_nested_variable_path() {
        let config = InfrastructureConfig::new();
        let refs = config.extract_all_references("{{ variables.database.connection.host }}");

        assert_eq!(refs.len(), 1);
        assert!(refs.iter().any(|r| matches!(
            r,
            ReferenceType::Variable { name } if name == "database.connection.host"
        )));
    }

    #[test]
    fn test_validate_nested_variable_reference() {
        let yaml = r#"
variables:
  database:
    host: "localhost"

resources:
  aws_instance:
    db:
      host: "{{ variables.database.host }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        // This should pass because 'database' is defined (even if 'database.host' isn't explicitly)
        assert!(config.validate_references().is_ok());
    }
}
