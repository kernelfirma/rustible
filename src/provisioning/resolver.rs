//! Template Resolver for Infrastructure Provisioning
//!
//! This module provides a comprehensive template resolution system for infrastructure
//! configurations. It handles cross-references between resources, variable substitution,
//! and dependency ordering.
//!
//! ## Key Features
//!
//! - **Resource Cross-References**: Resolve `{{ resources.aws_vpc.main.id }}` patterns
//! - **Nested Attribute Access**: Support for `{{ resources.aws_vpc.main.tags.Name }}`
//! - **Array Indexing**: Handle `{{ resources.aws_instance.web[0].public_ip }}`
//! - **Dependency Graph**: Build and topologically sort resource dependencies using petgraph
//! - **Variable Fallbacks**: Support default values for missing variables
//! - **MiniJinja Integration**: Full Jinja2-compatible template rendering
//!
//! ## Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::provisioning::resolver::{TemplateResolver, ResolverContext};
//!
//! let resolver = TemplateResolver::new();
//! let mut ctx = ResolverContext::new();
//! ctx.variables.insert("vpc_cidr".to_string(), json!("10.0.0.0/16"));
//!
//! let result = resolver.resolve_string("{{ variables.vpc_cidr }}", &ctx)?;
//! assert_eq!(result, "10.0.0.0/16");
//! # Ok(())
//! # }
//! ```

use std::collections::{HashMap, HashSet};

use minijinja::{Environment, UndefinedBehavior, Value as JinjaValue};
use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use regex::Regex;
use serde_json::Value;
use tracing::debug;

use super::config::InfrastructureConfig;
use super::error::{ProvisioningError, ProvisioningResult};
use super::state::{ProvisioningState, ResourceId};

// ============================================================================
// Resolver Context
// ============================================================================

/// Context for template resolution containing all available data
#[derive(Debug, Clone, Default)]
pub struct ResolverContext {
    /// Variables available for substitution
    pub variables: HashMap<String, Value>,

    /// Local values (computed locally)
    pub locals: HashMap<String, Value>,

    /// Resource attributes (type.name -> attributes) - flat structure for lookup
    pub resources: HashMap<String, Value>,

    /// Data source values (type.name -> attributes)
    pub data: HashMap<String, Value>,

    /// Computed outputs
    pub outputs: HashMap<String, Value>,

    /// Nested resource structure for MiniJinja (type -> name -> attributes)
    nested_resources: HashMap<String, HashMap<String, Value>>,
}

impl ResolverContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self::default()
    }

    /// Build context from configuration and state
    pub fn from_config_and_state(config: &InfrastructureConfig, state: &ProvisioningState) -> Self {
        let mut ctx = Self::new();

        // Add variables from config
        ctx.variables = config.variables.clone();

        // Add locals from config
        ctx.locals = config.locals.clone();

        // Add resource attributes from state
        for (address, resource) in &state.resources {
            // Merge config and attributes for full access
            let mut attrs = resource.attributes.clone();
            if let Value::Object(attrs_map) = &mut attrs {
                // Add cloud_id as 'id' attribute if not present
                if !attrs_map.contains_key("id") {
                    attrs_map.insert("id".to_string(), Value::String(resource.cloud_id.clone()));
                }

                // Merge in config values that may not be in attributes
                if let Value::Object(config_map) = &resource.config {
                    for (key, value) in config_map {
                        if !attrs_map.contains_key(key) {
                            attrs_map.insert(key.clone(), value.clone());
                        }
                    }
                }
            }

            // Store in flat structure
            ctx.resources.insert(address.clone(), attrs.clone());

            // Store in nested structure for MiniJinja
            if let Some(id) = ResourceId::from_address(address) {
                ctx.nested_resources
                    .entry(id.resource_type)
                    .or_default()
                    .insert(id.name, attrs);
            }
        }

        ctx
    }

    /// Set a resource's attributes
    pub fn set_resource(&mut self, resource_type: &str, name: &str, attributes: Value) {
        let address = format!("{}.{}", resource_type, name);
        self.resources.insert(address, attributes.clone());

        // Also update nested structure
        self.nested_resources
            .entry(resource_type.to_string())
            .or_default()
            .insert(name.to_string(), attributes);
    }

    /// Get a value by path (e.g., "variables.vpc_cidr" or "resources.aws_vpc.main.id")
    pub fn get_value(&self, path: &str) -> Option<Value> {
        let parts: Vec<&str> = path.splitn(2, '.').collect();
        if parts.len() < 2 {
            return None;
        }

        let (category, rest) = (parts[0], parts[1]);

        match category {
            "variables" | "var" => self.variables.get(rest).cloned(),
            "locals" | "local" => self.locals.get(rest).cloned(),
            "resources" | "resource" => self.get_resource_value(rest),
            "data" => self.get_data_value(rest),
            "outputs" | "output" => self.outputs.get(rest).cloned(),
            _ => None,
        }
    }

    /// Get a resource attribute value with support for array indexing
    fn get_resource_value(&self, path: &str) -> Option<Value> {
        // Check for array indexing pattern: type.name[index].attr
        if let Some((resource_type, name, index, attr)) = parse_array_access(path) {
            let address = format!("{}.{}", resource_type, name);
            let resource = self.resources.get(&address)?;

            // Get array element
            if let Value::Array(arr) = resource {
                let element = arr.get(index)?;
                if attr.is_empty() {
                    return Some(element.clone());
                }
                return get_nested_value(element, &attr);
            }
            return None;
        }

        // Standard path: type.name.attribute or type.name.nested.attr
        let parts: Vec<&str> = path.splitn(3, '.').collect();
        if parts.len() < 2 {
            return None;
        }

        let address = format!("{}.{}", parts[0], parts[1]);
        let attrs = self.resources.get(&address)?;

        if parts.len() == 2 {
            // Return all attributes
            return Some(attrs.clone());
        }

        // Navigate to the specific attribute
        get_nested_value(attrs, parts[2])
    }

    /// Get a data source value
    fn get_data_value(&self, path: &str) -> Option<Value> {
        let parts: Vec<&str> = path.splitn(3, '.').collect();
        if parts.len() < 2 {
            return None;
        }

        let address = format!("{}.{}", parts[0], parts[1]);
        let attrs = self.data.get(&address)?;

        if parts.len() == 2 {
            return Some(attrs.clone());
        }

        get_nested_value(attrs, parts[2])
    }

    /// Update context with new resource attributes
    pub fn update_resource(&mut self, address: &str, attributes: Value) {
        if let Some(id) = ResourceId::from_address(address) {
            self.set_resource(&id.resource_type, &id.name, attributes);
        } else {
            self.resources.insert(address.to_string(), attributes);
        }
    }

    /// Check if a resource exists in context
    pub fn has_resource(&self, address: &str) -> bool {
        self.resources.contains_key(address)
    }

    /// Convert context to a minijinja-compatible Value
    pub fn to_jinja_value(&self) -> JinjaValue {
        let mut root: HashMap<String, JinjaValue> = HashMap::new();

        // Add resources in nested structure for proper access
        let resources_map: HashMap<String, JinjaValue> = self
            .nested_resources
            .iter()
            .map(|(resource_type, names)| {
                let inner: HashMap<String, JinjaValue> = names
                    .iter()
                    .map(|(name, attrs)| (name.clone(), json_to_jinja(attrs)))
                    .collect();
                (resource_type.clone(), JinjaValue::from_serialize(&inner))
            })
            .collect();
        root.insert(
            "resources".to_string(),
            JinjaValue::from_serialize(&resources_map),
        );

        // Add variables
        let variables_map: HashMap<String, JinjaValue> = self
            .variables
            .iter()
            .map(|(k, v)| (k.clone(), json_to_jinja(v)))
            .collect();
        root.insert(
            "variables".to_string(),
            JinjaValue::from_serialize(&variables_map),
        );

        // Also add variables at root level for direct access
        for (k, v) in &self.variables {
            root.insert(k.clone(), json_to_jinja(v));
        }

        // Add locals
        let locals_map: HashMap<String, JinjaValue> = self
            .locals
            .iter()
            .map(|(k, v)| (k.clone(), json_to_jinja(v)))
            .collect();
        root.insert(
            "locals".to_string(),
            JinjaValue::from_serialize(&locals_map),
        );

        // Add data sources
        let data_map: HashMap<String, JinjaValue> = self
            .data
            .iter()
            .map(|(k, v)| (k.clone(), json_to_jinja(v)))
            .collect();
        root.insert("data".to_string(), JinjaValue::from_serialize(&data_map));

        JinjaValue::from_serialize(&root)
    }
}

/// Parse array access pattern: "type.name[index].attr" -> (type, name, index, attr)
fn parse_array_access(path: &str) -> Option<(String, String, usize, String)> {
    let array_re =
        Regex::new(r"^([a-zA-Z_][a-zA-Z0-9_]*)\.([a-zA-Z_][a-zA-Z0-9_-]*)\[(\d+)\](?:\.(.+))?$")
            .ok()?;

    let caps = array_re.captures(path)?;
    let resource_type = caps.get(1)?.as_str().to_string();
    let name = caps.get(2)?.as_str().to_string();
    let index: usize = caps.get(3)?.as_str().parse().ok()?;
    let attr = caps
        .get(4)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();

    Some((resource_type, name, index, attr))
}

// ============================================================================
// Resolved Configuration
// ============================================================================

/// Configuration with all template references resolved
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// Resolved resource configurations (type -> name -> config)
    pub resources: HashMap<String, HashMap<String, Value>>,

    /// Resolution order for execution (respects dependencies)
    pub resolution_order: Vec<ResourceId>,

    /// Resolved output values
    pub outputs: HashMap<String, Value>,

    /// All resolved values by address
    pub values: HashMap<String, Value>,

    /// Unresolved references (shown as warnings during plan)
    pub unresolved: Vec<UnresolvedReference>,
}

/// An unresolved template reference
#[derive(Debug, Clone)]
pub struct UnresolvedReference {
    /// Resource that contains the reference
    pub in_resource: String,

    /// The reference path that could not be resolved
    pub reference: String,

    /// Reason for failure
    pub reason: String,
}

impl ResolvedConfig {
    /// Create a new empty resolved config
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
            resolution_order: Vec::new(),
            outputs: HashMap::new(),
            values: HashMap::new(),
            unresolved: Vec::new(),
        }
    }

    /// Check if there are any unresolved references
    pub fn has_unresolved(&self) -> bool {
        !self.unresolved.is_empty()
    }

    /// Get resolved config for a specific resource by ResourceId
    pub fn get_resource(&self, id: &ResourceId) -> Option<&Value> {
        self.resources
            .get(&id.resource_type)
            .and_then(|resources| resources.get(&id.name))
    }

    /// Get resolved config by type and name
    pub fn get_resource_by_name(&self, resource_type: &str, name: &str) -> Option<&Value> {
        self.resources.get(resource_type).and_then(|r| r.get(name))
    }
}

impl Default for ResolvedConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Resource Reference
// ============================================================================

/// A reference to a resource attribute extracted from templates
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceReference {
    /// Resource type (e.g., "aws_vpc")
    pub resource_type: String,
    /// Resource name (e.g., "main")
    pub name: String,
    /// Optional array index
    pub index: Option<usize>,
    /// Attribute path (e.g., "id" or "tags.Name")
    pub attribute: String,
}

impl ResourceReference {
    /// Create a new resource reference
    pub fn new(resource_type: &str, name: &str, attribute: &str) -> Self {
        Self {
            resource_type: resource_type.to_string(),
            name: name.to_string(),
            index: None,
            attribute: attribute.to_string(),
        }
    }

    /// Create a reference with an array index
    pub fn with_index(resource_type: &str, name: &str, index: usize, attribute: &str) -> Self {
        Self {
            resource_type: resource_type.to_string(),
            name: name.to_string(),
            index: Some(index),
            attribute: attribute.to_string(),
        }
    }

    /// Get the resource ID this reference points to
    pub fn resource_id(&self) -> ResourceId {
        ResourceId::new(&self.resource_type, &self.name)
    }

    /// Format the reference as a string
    pub fn to_reference_string(&self) -> String {
        match self.index {
            Some(idx) => format!(
                "resources.{}[{}].{}",
                self.resource_type, idx, self.attribute
            ),
            None => format!(
                "resources.{}.{}.{}",
                self.resource_type, self.name, self.attribute
            ),
        }
    }
}

// ============================================================================
// Template Resolver
// ============================================================================

/// Comprehensive template resolver for infrastructure configurations
///
/// Uses MiniJinja for Jinja2-compatible template rendering and petgraph
/// for dependency graph analysis.
pub struct TemplateResolver {
    /// MiniJinja environment for template rendering
    env: Environment<'static>,

    /// Regex pattern for template extraction
    template_pattern: Regex,

    /// Regex for {{ resources.TYPE.NAME.ATTR }} patterns
    resource_ref_regex: Regex,

    /// Regex for {{ resources.TYPE.NAME[INDEX].ATTR }} patterns
    array_ref_regex: Regex,

    /// Whether to allow partial resolution (for planning)
    allow_partial: bool,
}

impl std::fmt::Debug for TemplateResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TemplateResolver")
            .field("allow_partial", &self.allow_partial)
            .finish()
    }
}

impl Default for TemplateResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateResolver {
    /// Create a new template resolver
    pub fn new() -> Self {
        let mut env = Environment::new();

        // Use strict undefined behavior to detect missing attributes
        env.set_undefined_behavior(UndefinedBehavior::Strict);

        // Add custom filters
        env.add_filter("default", default_filter);
        env.add_filter("lower", lower_filter);
        env.add_filter("upper", upper_filter);
        env.add_filter("title", title_filter);
        env.add_filter("replace", replace_filter);
        env.add_filter("join", join_filter);
        env.add_filter("length", length_filter);

        // Match {{ ... }} patterns
        let template_pattern = Regex::new(r"\{\{\s*([^}]+?)\s*\}\}").expect("Invalid regex");

        // Regex for {{ resources.TYPE.NAME.ATTR }} patterns (with optional filters)
        let resource_ref_regex = Regex::new(
            r"\{\{\s*resources\.([a-zA-Z_][a-zA-Z0-9_]*)\.([a-zA-Z_][a-zA-Z0-9_-]*)\.([a-zA-Z_][a-zA-Z0-9_.]*)\s*(?:\|[^}]*)?\}\}"
        ).expect("Invalid regex");

        // Regex for {{ resources.TYPE.NAME[INDEX].ATTR }} patterns
        let array_ref_regex = Regex::new(
            r"\{\{\s*resources\.([a-zA-Z_][a-zA-Z0-9_]*)\.([a-zA-Z_][a-zA-Z0-9_-]*)\[(\d+)\]\.([a-zA-Z_][a-zA-Z0-9_.]*)\s*(?:\|[^}]*)?\}\}"
        ).expect("Invalid regex");

        Self {
            env,
            template_pattern,
            resource_ref_regex,
            array_ref_regex,
            allow_partial: false,
        }
    }

    /// Create resolver that allows partial resolution (for planning)
    pub fn with_partial_resolution(mut self) -> Self {
        self.allow_partial = true;
        self
    }

    /// Resolve all templates in the configuration
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Dependency cycle is detected
    /// - Required references cannot be resolved (unless partial resolution is enabled)
    pub fn resolve_config(
        &self,
        config: &InfrastructureConfig,
        state: &ProvisioningState,
    ) -> ProvisioningResult<ResolvedConfig> {
        // Build dependency graph using petgraph
        let (graph, node_map) = self.build_dependency_graph(config)?;

        // Get topological order
        let order = toposort(&graph, None).map_err(|_| {
            ProvisioningError::DependencyCycle(self.find_cycle_members(&graph, &node_map))
        })?;

        // Build resolution order from topological sort
        let resolution_order: Vec<ResourceId> = order
            .iter()
            .filter_map(|idx| graph.node_weight(*idx).cloned())
            .collect();

        // Create context from state
        let mut ctx = ResolverContext::from_config_and_state(config, state);

        // Resolve resources in order
        let mut resolved = ResolvedConfig {
            resources: HashMap::new(),
            outputs: HashMap::new(),
            resolution_order: resolution_order.clone(),
            values: HashMap::new(),
            unresolved: Vec::new(),
        };

        for id in &resolution_order {
            let original_config = config
                .resources
                .get(&id.resource_type)
                .and_then(|r| r.get(&id.name))
                .cloned()
                .unwrap_or(Value::Object(Default::default()));

            let (resolved_config, unresolved) =
                self.resolve_value_internal(&original_config, &ctx, &id.address());

            // Merge resolved config with existing state attributes (preserve computed values like 'id')
            let merged_config = if let Some(existing) = ctx.resources.get(&id.address()) {
                merge_json_values(existing, &resolved_config)
            } else {
                resolved_config.clone()
            };

            // Store resolved config
            resolved
                .resources
                .entry(id.resource_type.clone())
                .or_default()
                .insert(id.name.clone(), merged_config.clone());

            // Store in values map
            resolved.values.insert(id.address(), merged_config.clone());

            // Track unresolved references
            resolved.unresolved.extend(unresolved);

            // Update context with merged values (preserves state attributes for subsequent resources)
            ctx.set_resource(&id.resource_type, &id.name, merged_config);
        }

        // Resolve locals (they may depend on variables but not resources)
        for (name, value) in &config.locals {
            let (resolved_value, _) =
                self.resolve_value_internal(value, &ctx, &format!("local.{}", name));
            ctx.locals.insert(name.clone(), resolved_value);
        }

        // Resolve outputs
        for (name, output_config) in &config.outputs {
            let (resolved_value, unresolved) = self.resolve_value_internal(
                &output_config.value,
                &ctx,
                &format!("output.{}", name),
            );
            resolved.outputs.insert(name.clone(), resolved_value);
            resolved.unresolved.extend(unresolved);
        }

        Ok(resolved)
    }

    /// Resolve a single value with the given context
    pub fn resolve_value(&self, value: &Value, ctx: &ResolverContext) -> ProvisioningResult<Value> {
        let (resolved, unresolved) = self.resolve_value_internal(value, ctx, "single");

        if !unresolved.is_empty() && !self.allow_partial {
            let refs: Vec<_> = unresolved.iter().map(|u| u.reference.clone()).collect();
            return Err(ProvisioningError::UnresolvedReference {
                reference: refs.join(", "),
            });
        }

        Ok(resolved)
    }

    /// Internal value resolution with unresolved tracking
    fn resolve_value_internal(
        &self,
        value: &Value,
        ctx: &ResolverContext,
        context_name: &str,
    ) -> (Value, Vec<UnresolvedReference>) {
        let mut unresolved = Vec::new();

        let resolved = match value {
            Value::String(s) => {
                let (resolved_str, refs) = self.resolve_string_internal(s, ctx, context_name);
                unresolved.extend(refs);
                resolved_str
            }
            Value::Array(arr) => {
                let resolved_arr: Vec<Value> = arr
                    .iter()
                    .map(|v| {
                        let (resolved, refs) = self.resolve_value_internal(v, ctx, context_name);
                        unresolved.extend(refs);
                        resolved
                    })
                    .collect();
                Value::Array(resolved_arr)
            }
            Value::Object(map) => {
                let resolved_map: serde_json::Map<String, Value> = map
                    .iter()
                    .map(|(k, v)| {
                        let (resolved, refs) = self.resolve_value_internal(v, ctx, context_name);
                        unresolved.extend(refs);
                        (k.clone(), resolved)
                    })
                    .collect();
                Value::Object(resolved_map)
            }
            // Primitives pass through unchanged
            other => other.clone(),
        };

        (resolved, unresolved)
    }

    /// Resolve a template string
    pub fn resolve_string(
        &self,
        template: &str,
        ctx: &ResolverContext,
    ) -> ProvisioningResult<String> {
        let (resolved, unresolved) = self.resolve_string_internal(template, ctx, "string");

        if !unresolved.is_empty() && !self.allow_partial {
            let refs: Vec<_> = unresolved.iter().map(|u| u.reference.clone()).collect();
            return Err(ProvisioningError::UnresolvedReference {
                reference: refs.join(", "),
            });
        }

        match resolved {
            Value::String(s) => Ok(s),
            other => Ok(serde_json::to_string(&other).unwrap_or_else(|_| "(complex)".to_string())),
        }
    }

    /// Internal string resolution with unresolved tracking
    fn resolve_string_internal(
        &self,
        s: &str,
        ctx: &ResolverContext,
        context_name: &str,
    ) -> (Value, Vec<UnresolvedReference>) {
        let mut unresolved = Vec::new();

        // If not a template, return as-is
        if !is_template(s) {
            return (Value::String(s.to_string()), unresolved);
        }

        // Find all template references
        let captures: Vec<_> = self.template_pattern.captures_iter(s).collect();

        if captures.is_empty() {
            return (Value::String(s.to_string()), unresolved);
        }

        // Check if the entire string is a single template (return raw value)
        if captures.len() == 1 {
            let full_match = captures[0].get(0).unwrap().as_str();
            if s.trim() == full_match {
                let path = captures[0].get(1).unwrap().as_str().trim();

                // Try direct context lookup first
                if let Some(value) = ctx.get_value(path) {
                    return (value, unresolved);
                }

                // Try MiniJinja rendering for complex expressions (filters, etc.)
                match self.render_with_jinja(s, ctx) {
                    Ok(result) => {
                        // Try to parse as JSON for type preservation
                        if let Ok(json_val) = serde_json::from_str::<Value>(&result) {
                            return (json_val, unresolved);
                        }
                        return (Value::String(result), unresolved);
                    }
                    Err(e) => {
                        if self.allow_partial {
                            debug!(
                                "Unresolved reference '{}' in {} (partial resolution allowed): {}",
                                path, context_name, e
                            );
                            unresolved.push(UnresolvedReference {
                                in_resource: context_name.to_string(),
                                reference: path.to_string(),
                                reason: "Reference not yet available".to_string(),
                            });
                            return (Value::String(format!("(unknown: {})", path)), unresolved);
                        }
                        unresolved.push(UnresolvedReference {
                            in_resource: context_name.to_string(),
                            reference: path.to_string(),
                            reason: format!("Resolution failed: {}", e),
                        });
                        return (Value::String(s.to_string()), unresolved);
                    }
                }
            }
        }

        // Multiple templates or embedded templates - try MiniJinja first
        match self.render_with_jinja(s, ctx) {
            Ok(result) => (Value::String(result), unresolved),
            Err(_) => {
                // Fall back to manual replacement
                let mut result = s.to_string();
                for cap in &captures {
                    let full_match = cap.get(0).unwrap().as_str();
                    let path = cap.get(1).unwrap().as_str().trim();

                    if let Some(value) = ctx.get_value(path) {
                        let replacement = match value {
                            Value::String(s) => s,
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            Value::Null => "null".to_string(),
                            _ => serde_json::to_string(&value)
                                .unwrap_or_else(|_| "(complex)".to_string()),
                        };
                        result = result.replace(full_match, &replacement);
                    } else if self.allow_partial {
                        unresolved.push(UnresolvedReference {
                            in_resource: context_name.to_string(),
                            reference: path.to_string(),
                            reason: "Reference not yet available".to_string(),
                        });
                        result = result.replace(full_match, &format!("(unknown: {})", path));
                    } else {
                        unresolved.push(UnresolvedReference {
                            in_resource: context_name.to_string(),
                            reference: path.to_string(),
                            reason: "Reference not found".to_string(),
                        });
                    }
                }
                (Value::String(result), unresolved)
            }
        }
    }

    /// Render a template using MiniJinja
    fn render_with_jinja(&self, template: &str, ctx: &ResolverContext) -> Result<String, String> {
        let jinja_ctx = ctx.to_jinja_value();

        let tmpl = self
            .env
            .template_from_str(template)
            .map_err(|e| format!("Template parse error: {}", e))?;

        tmpl.render(&jinja_ctx)
            .map_err(|e| format!("Template render error: {}", e))
    }

    /// Extract all resource dependencies from a value
    pub fn extract_dependencies(&self, value: &Value) -> Vec<ResourceId> {
        let mut deps = HashSet::new();
        self.extract_deps_recursive(value, &mut deps);
        deps.into_iter().collect()
    }

    /// Recursively extract dependencies
    fn extract_deps_recursive(&self, value: &Value, deps: &mut HashSet<ResourceId>) {
        match value {
            Value::String(s) => {
                // Extract standard resource references
                for cap in self.resource_ref_regex.captures_iter(s) {
                    let resource_type = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
                    let name = cap.get(2).map(|m| m.as_str()).unwrap_or_default();
                    deps.insert(ResourceId::new(resource_type, name));
                }

                // Extract array references
                for cap in self.array_ref_regex.captures_iter(s) {
                    let resource_type = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
                    let name = cap.get(2).map(|m| m.as_str()).unwrap_or_default();
                    deps.insert(ResourceId::new(resource_type, name));
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    self.extract_deps_recursive(item, deps);
                }
            }
            Value::Object(map) => {
                for v in map.values() {
                    self.extract_deps_recursive(v, deps);
                }
            }
            _ => {}
        }
    }

    /// Extract all resource references from a value (with full details)
    pub fn extract_references(&self, value: &Value) -> Vec<ResourceReference> {
        let mut refs = Vec::new();
        self.extract_refs_recursive(value, &mut refs);
        refs
    }

    /// Recursively extract references with full details
    fn extract_refs_recursive(&self, value: &Value, refs: &mut Vec<ResourceReference>) {
        match value {
            Value::String(s) => {
                // Extract standard resource references
                for cap in self.resource_ref_regex.captures_iter(s) {
                    let resource_type = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
                    let name = cap.get(2).map(|m| m.as_str()).unwrap_or_default();
                    let attr = cap.get(3).map(|m| m.as_str()).unwrap_or_default();
                    refs.push(ResourceReference::new(resource_type, name, attr));
                }

                // Extract array references
                for cap in self.array_ref_regex.captures_iter(s) {
                    let resource_type = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
                    let name = cap.get(2).map(|m| m.as_str()).unwrap_or_default();
                    let index: usize = cap
                        .get(3)
                        .and_then(|m| m.as_str().parse().ok())
                        .unwrap_or(0);
                    let attr = cap.get(4).map(|m| m.as_str()).unwrap_or_default();
                    refs.push(ResourceReference::with_index(
                        resource_type,
                        name,
                        index,
                        attr,
                    ));
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    self.extract_refs_recursive(item, refs);
                }
            }
            Value::Object(map) => {
                for v in map.values() {
                    self.extract_refs_recursive(v, refs);
                }
            }
            _ => {}
        }
    }

    /// Build a dependency graph from the configuration using petgraph
    pub fn build_dependency_graph(
        &self,
        config: &InfrastructureConfig,
    ) -> ProvisioningResult<(DiGraph<ResourceId, ()>, HashMap<ResourceId, NodeIndex>)> {
        let mut graph = DiGraph::new();
        let mut node_map: HashMap<ResourceId, NodeIndex> = HashMap::new();

        // First pass: add all resources as nodes
        for (resource_type, resources) in &config.resources {
            for name in resources.keys() {
                let id = ResourceId::new(resource_type, name);
                let idx = graph.add_node(id.clone());
                node_map.insert(id, idx);
            }
        }

        // Second pass: add edges for dependencies
        for (resource_type, resources) in &config.resources {
            for (name, resource_config) in resources {
                let id = ResourceId::new(resource_type, name);
                let deps = self.extract_dependencies(resource_config);

                // Also check explicit depends_on
                let explicit_deps = resource_config
                    .get("depends_on")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .filter_map(ResourceId::from_address)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                let all_deps: HashSet<_> = deps.into_iter().chain(explicit_deps).collect();

                for dep in all_deps {
                    // Add edge from dependency to this resource
                    if let (Some(&from_idx), Some(&to_idx)) =
                        (node_map.get(&dep), node_map.get(&id))
                    {
                        graph.add_edge(from_idx, to_idx, ());
                    } else if node_map.get(&dep).is_none() && !dep.resource_type.is_empty() {
                        // Dependency references a resource that doesn't exist in config
                        return Err(ProvisioningError::MissingDependency {
                            resource: id.address(),
                            dependency: dep.address(),
                        });
                    }
                }
            }
        }

        // Check for cycles using tarjan's algorithm
        let sccs = tarjan_scc(&graph);
        for scc in &sccs {
            if scc.len() > 1 {
                let cycle_members: Vec<String> = scc
                    .iter()
                    .filter_map(|idx| graph.node_weight(*idx).map(|id| id.address()))
                    .collect();
                return Err(ProvisioningError::DependencyCycle(cycle_members));
            }
        }

        Ok((graph, node_map))
    }

    /// Find members of a cycle in the graph
    fn find_cycle_members(
        &self,
        graph: &DiGraph<ResourceId, ()>,
        _node_map: &HashMap<ResourceId, NodeIndex>,
    ) -> Vec<String> {
        let sccs = tarjan_scc(graph);
        for scc in sccs {
            if scc.len() > 1 {
                return scc
                    .iter()
                    .filter_map(|idx| graph.node_weight(*idx).map(|id| id.address()))
                    .collect();
            }
        }
        vec!["Circular dependency detected".to_string()]
    }

    /// Get the resolution order for resources
    pub fn get_resolution_order(
        &self,
        config: &InfrastructureConfig,
    ) -> ProvisioningResult<Vec<ResourceId>> {
        let (graph, node_map) = self.build_dependency_graph(config)?;

        let order = toposort(&graph, None).map_err(|_| {
            ProvisioningError::DependencyCycle(self.find_cycle_members(&graph, &node_map))
        })?;

        Ok(order
            .iter()
            .filter_map(|idx| graph.node_weight(*idx).cloned())
            .collect())
    }

    /// Validate that all references in a config can be resolved
    pub fn validate_references(&self, config: &InfrastructureConfig) -> ProvisioningResult<()> {
        let all_resources: HashSet<_> = config
            .resource_addresses()
            .into_iter()
            .map(|id| id.address())
            .collect();

        for (resource_type, resources) in &config.resources {
            for (name, resource_config) in resources {
                let refs = self.extract_references(resource_config);

                for reference in refs {
                    let ref_address = reference.resource_id().address();
                    if !all_resources.contains(&ref_address) {
                        return Err(ProvisioningError::UnresolvedReference {
                            reference: format!(
                                "{}.{} references non-existent resource {}",
                                resource_type, name, ref_address
                            ),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a value contains any template references
    pub fn has_templates(value: &Value) -> bool {
        match value {
            Value::String(s) => is_template(s),
            Value::Array(arr) => arr.iter().any(Self::has_templates),
            Value::Object(map) => map.values().any(Self::has_templates),
            _ => false,
        }
    }

    /// Resolve a single value with the given context (legacy API)
    pub fn resolve_single(
        &self,
        value: &Value,
        ctx: &ResolverContext,
    ) -> ProvisioningResult<Value> {
        self.resolve_value(value, ctx)
    }

    /// Extract all template references from a value (returns paths)
    pub fn extract_template_refs(&self, value: &Value) -> Vec<String> {
        let mut refs = Vec::new();
        self.extract_template_refs_recursive(value, &mut refs);
        refs
    }

    fn extract_template_refs_recursive(&self, value: &Value, refs: &mut Vec<String>) {
        match value {
            Value::String(s) => {
                for cap in self.template_pattern.captures_iter(s) {
                    if let Some(path) = cap.get(1) {
                        refs.push(path.as_str().trim().to_string());
                    }
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    self.extract_template_refs_recursive(item, refs);
                }
            }
            Value::Object(map) => {
                for v in map.values() {
                    self.extract_template_refs_recursive(v, refs);
                }
            }
            _ => {}
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if a string contains template syntax
fn is_template(s: &str) -> bool {
    s.contains("{{") && s.contains("}}")
}

/// Get a nested value from a JSON value using dot notation
fn get_nested_value(value: &Value, path: &str) -> Option<Value> {
    let mut current = value.clone();

    for part in path.split('.') {
        // Check for array index notation in part
        if let Some(idx_start) = part.find('[') {
            let key = &part[..idx_start];
            if let Some(idx_end) = part.find(']') {
                let idx: usize = part[idx_start + 1..idx_end].parse().ok()?;

                if !key.is_empty() {
                    current = current.get(key)?.clone();
                }
                current = current.get(idx)?.clone();
                continue;
            }
        }

        match &current {
            Value::Object(map) => {
                current = map.get(part)?.clone();
            }
            Value::Array(arr) => {
                let idx: usize = part.parse().ok()?;
                current = arr.get(idx)?.clone();
            }
            _ => return None,
        }
    }

    Some(current)
}

/// Merge two JSON values, with `overlay` values taking precedence
/// This is used to merge resolved config with existing state attributes
fn merge_json_values(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut result = base_map.clone();
            for (key, value) in overlay_map {
                if let Some(base_value) = result.get(key) {
                    // Recursively merge nested objects
                    result.insert(key.clone(), merge_json_values(base_value, value));
                } else {
                    result.insert(key.clone(), value.clone());
                }
            }
            Value::Object(result)
        }
        // For non-objects, overlay takes precedence
        (_, overlay) => overlay.clone(),
    }
}

/// Convert a serde_json::Value to a minijinja::Value
fn json_to_jinja(value: &Value) -> JinjaValue {
    match value {
        Value::Null => JinjaValue::UNDEFINED,
        Value::Bool(b) => JinjaValue::from(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JinjaValue::from(i)
            } else if let Some(f) = n.as_f64() {
                JinjaValue::from(f)
            } else {
                JinjaValue::from(n.to_string())
            }
        }
        Value::String(s) => JinjaValue::from(s.as_str()),
        Value::Array(arr) => {
            let items: Vec<JinjaValue> = arr.iter().map(json_to_jinja).collect();
            JinjaValue::from(items)
        }
        Value::Object(map) => {
            let items: HashMap<String, JinjaValue> = map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_jinja(v)))
                .collect();
            JinjaValue::from_serialize(&items)
        }
    }
}

// ============================================================================
// Custom Filters for MiniJinja
// ============================================================================

/// Default filter for providing fallback values
fn default_filter(value: JinjaValue, default: JinjaValue) -> JinjaValue {
    if value.is_undefined() || value.is_none() {
        default
    } else {
        value
    }
}

/// Lowercase filter
fn lower_filter(value: &str) -> String {
    value.to_lowercase()
}

/// Uppercase filter
fn upper_filter(value: &str) -> String {
    value.to_uppercase()
}

/// Title case filter
fn title_filter(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Replace filter
fn replace_filter(value: &str, from: &str, to: &str) -> String {
    value.replace(from, to)
}

/// Join filter for arrays
fn join_filter(value: Vec<JinjaValue>, separator: &str) -> String {
    value
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(separator)
}

/// Length filter
fn length_filter(value: JinjaValue) -> usize {
    value.len().unwrap_or(0)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> ResolverContext {
        let mut ctx = ResolverContext::new();

        ctx.variables.insert(
            "vpc_cidr".to_string(),
            Value::String("10.0.0.0/16".to_string()),
        );
        ctx.variables.insert(
            "environment".to_string(),
            Value::String("production".to_string()),
        );
        ctx.variables
            .insert("port".to_string(), serde_json::json!(8080));
        ctx.variables
            .insert("enabled".to_string(), serde_json::json!(true));

        ctx.locals.insert(
            "computed_name".to_string(),
            Value::String("my-resource".to_string()),
        );

        ctx.set_resource(
            "aws_vpc",
            "main",
            serde_json::json!({
                "id": "vpc-12345",
                "cidr_block": "10.0.0.0/16",
                "tags": {
                    "Name": "production-vpc",
                    "Environment": "prod"
                }
            }),
        );

        ctx.set_resource(
            "aws_subnet",
            "public",
            serde_json::json!({
                "id": "subnet-67890",
                "vpc_id": "vpc-12345",
                "cidr_block": "10.0.1.0/24"
            }),
        );

        ctx.set_resource(
            "aws_instance",
            "web",
            serde_json::json!([
                {"id": "i-111", "public_ip": "1.2.3.4"},
                {"id": "i-222", "public_ip": "5.6.7.8"}
            ]),
        );

        ctx
    }

    // ========================================================================
    // Test 1: Simple Variable Resolution
    // ========================================================================

    #[test]
    fn test_simple_variable_resolution() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!("{{ variables.vpc_cidr }}");
        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        assert_eq!(resolved, serde_json::json!("10.0.0.0/16"));
    }

    // ========================================================================
    // Test 2: Simple Resource Reference
    // ========================================================================

    #[test]
    fn test_simple_resource_reference() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!("{{ resources.aws_vpc.main.id }}");
        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        assert_eq!(resolved, serde_json::json!("vpc-12345"));
    }

    // ========================================================================
    // Test 3: Nested Attribute Access
    // ========================================================================

    #[test]
    fn test_nested_attribute_access() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!("{{ resources.aws_vpc.main.tags.Name }}");
        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        assert_eq!(resolved, serde_json::json!("production-vpc"));
    }

    // ========================================================================
    // Test 4: Array Indexing
    // ========================================================================

    #[test]
    fn test_array_indexing_extraction() {
        let resolver = TemplateResolver::new();
        let value = serde_json::json!("{{ resources.aws_instance.web[0].public_ip }}");

        let refs = resolver.extract_references(&value);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].resource_type, "aws_instance");
        assert_eq!(refs[0].name, "web");
        assert_eq!(refs[0].index, Some(0));
        assert_eq!(refs[0].attribute, "public_ip");
    }

    #[test]
    fn test_array_indexing_second_element() {
        let resolver = TemplateResolver::new();
        let value = serde_json::json!("{{ resources.aws_instance.web[1].id }}");

        let refs = resolver.extract_references(&value);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].index, Some(1));
    }

    // ========================================================================
    // Test 5: Multiple References in String
    // ========================================================================

    #[test]
    fn test_multiple_references_in_string() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let template =
            "VPC: {{ resources.aws_vpc.main.id }}, Subnet: {{ resources.aws_subnet.public.id }}";
        let result = resolver.resolve_string(template, &ctx).unwrap();

        assert_eq!(result, "VPC: vpc-12345, Subnet: subnet-67890");
    }

    // ========================================================================
    // Test 6: Dependency Extraction
    // ========================================================================

    #[test]
    fn test_extract_single_dependency() {
        let resolver = TemplateResolver::new();
        let value = serde_json::json!("{{ resources.aws_vpc.main.id }}");

        let deps = resolver.extract_dependencies(&value);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].resource_type, "aws_vpc");
        assert_eq!(deps[0].name, "main");
    }

    #[test]
    fn test_extract_multiple_dependencies() {
        let resolver = TemplateResolver::new();
        let value = serde_json::json!({
            "vpc_id": "{{ resources.aws_vpc.main.id }}",
            "subnet_id": "{{ resources.aws_subnet.public.id }}"
        });

        let deps = resolver.extract_dependencies(&value);
        assert_eq!(deps.len(), 2);
    }

    #[test]
    fn test_extract_nested_dependencies() {
        let resolver = TemplateResolver::new();
        let value = serde_json::json!({
            "config": {
                "network": {
                    "vpc_id": "{{ resources.aws_vpc.main.id }}"
                }
            }
        });

        let deps = resolver.extract_dependencies(&value);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].address(), "aws_vpc.main");
    }

    // ========================================================================
    // Test 7: Circular Dependency Detection
    // ========================================================================

    #[test]
    fn test_circular_dependency_detection() {
        let resolver = TemplateResolver::new();

        let mut config = InfrastructureConfig::new();

        let mut sg_a_config = serde_json::Map::new();
        sg_a_config.insert(
            "ingress".to_string(),
            serde_json::json!("{{ resources.aws_security_group.b.id }}"),
        );

        let mut sg_b_config = serde_json::Map::new();
        sg_b_config.insert(
            "ingress".to_string(),
            serde_json::json!("{{ resources.aws_security_group.a.id }}"),
        );

        let mut sg_resources = HashMap::new();
        sg_resources.insert("a".to_string(), Value::Object(sg_a_config));
        sg_resources.insert("b".to_string(), Value::Object(sg_b_config));

        config
            .resources
            .insert("aws_security_group".to_string(), sg_resources);

        let result = resolver.build_dependency_graph(&config);

        assert!(result.is_err());
        match result {
            Err(ProvisioningError::DependencyCycle(members)) => {
                assert!(!members.is_empty());
                assert!(members.iter().any(|m| m.contains("aws_security_group")));
            }
            _ => panic!("Expected DependencyCycle error"),
        }
    }

    // ========================================================================
    // Test 8: Missing Reference Handling
    // ========================================================================

    #[test]
    fn test_missing_dependency_detection() {
        let resolver = TemplateResolver::new();
        let yaml = r#"
resources:
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.nonexistent.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let result = resolver.build_dependency_graph(&config);

        assert!(result.is_err());
        match result {
            Err(ProvisioningError::MissingDependency {
                resource,
                dependency,
            }) => {
                assert_eq!(resource, "aws_subnet.public");
                assert_eq!(dependency, "aws_vpc.nonexistent");
            }
            _ => panic!("Expected MissingDependency error"),
        }
    }

    #[test]
    fn test_unresolved_reference_strict() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!("{{ resources.aws_instance.nonexistent.id }}");
        let result = resolver.resolve_value(&value, &ctx);

        assert!(result.is_err());
    }

    #[test]
    fn test_unresolved_reference_partial() {
        let resolver = TemplateResolver::new().with_partial_resolution();
        let ctx = create_test_context();

        let value = serde_json::json!("{{ resources.aws_instance.nonexistent.id }}");
        let (resolved, unresolved) = resolver.resolve_value_internal(&value, &ctx, "test");

        assert!(!unresolved.is_empty());
        assert!(resolved.as_str().unwrap().contains("unknown"));
    }

    // ========================================================================
    // Test 9: Variable Fallback to Default
    // ========================================================================

    #[test]
    fn test_default_filter() {
        let resolver = TemplateResolver::new();
        let ctx = ResolverContext::new();

        let result = resolver
            .resolve_string("{{ undefined_var | default('fallback') }}", &ctx)
            .unwrap();
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_default_filter_with_existing_value() {
        let resolver = TemplateResolver::new();
        let mut ctx = ResolverContext::new();
        ctx.variables
            .insert("existing".to_string(), serde_json::json!("real_value"));

        let result = resolver
            .resolve_string("{{ variables.existing | default('fallback') }}", &ctx)
            .unwrap();
        assert_eq!(result, "real_value");
    }

    // ========================================================================
    // Test 10: Resolution Order
    // ========================================================================

    #[test]
    fn test_resolution_order() {
        let resolver = TemplateResolver::new();
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
      cidr_block: "10.0.1.0/24"
  aws_instance:
    web:
      subnet_id: "{{ resources.aws_subnet.public.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let order = resolver.get_resolution_order(&config).unwrap();

        let vpc_pos = order.iter().position(|id| id.address() == "aws_vpc.main");
        let subnet_pos = order
            .iter()
            .position(|id| id.address() == "aws_subnet.public");
        let instance_pos = order
            .iter()
            .position(|id| id.address() == "aws_instance.web");

        assert!(vpc_pos.is_some());
        assert!(subnet_pos.is_some());
        assert!(instance_pos.is_some());
        assert!(vpc_pos < subnet_pos);
        assert!(subnet_pos < instance_pos);
    }

    // ========================================================================
    // Test 11: Object Value Resolution
    // ========================================================================

    #[test]
    fn test_resolve_object_with_templates() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!({
            "vpc_id": "{{ resources.aws_vpc.main.id }}",
            "cidr_block": "{{ variables.vpc_cidr }}",
            "environment": "{{ variables.environment }}"
        });

        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        let obj = resolved.as_object().unwrap();
        assert_eq!(obj.get("vpc_id").unwrap(), "vpc-12345");
        assert_eq!(obj.get("cidr_block").unwrap(), "10.0.0.0/16");
        assert_eq!(obj.get("environment").unwrap(), "production");
    }

    // ========================================================================
    // Test 12: Array Value Resolution
    // ========================================================================

    #[test]
    fn test_resolve_array_with_templates() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!([
            "{{ resources.aws_vpc.main.id }}",
            "{{ resources.aws_subnet.public.id }}"
        ]);

        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        let arr = resolved.as_array().unwrap();
        assert_eq!(arr[0], "vpc-12345");
        assert_eq!(arr[1], "subnet-67890");
    }

    // ========================================================================
    // Test 13: Preserve Non-String Values
    // ========================================================================

    #[test]
    fn test_preserve_non_string_values() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!({
            "count": 5,
            "enabled": true,
            "name": "{{ variables.environment }}"
        });

        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        let obj = resolved.as_object().unwrap();
        assert_eq!(obj.get("count").unwrap(), &serde_json::json!(5));
        assert_eq!(obj.get("enabled").unwrap(), &serde_json::json!(true));
        assert_eq!(obj.get("name").unwrap(), "production");
    }

    #[test]
    fn test_resolve_number_as_value() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!("{{ variables.port }}");
        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        assert_eq!(resolved, serde_json::json!(8080));
    }

    #[test]
    fn test_resolve_boolean_as_value() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!("{{ variables.enabled }}");
        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        assert_eq!(resolved, serde_json::json!(true));
    }

    // ========================================================================
    // Test 14: Filter Tests
    // ========================================================================

    #[test]
    fn test_lower_filter() {
        let resolver = TemplateResolver::new();
        let mut ctx = ResolverContext::new();
        ctx.variables
            .insert("name".to_string(), serde_json::json!("PRODUCTION"));

        let result = resolver
            .resolve_string("{{ variables.name | lower }}", &ctx)
            .unwrap();
        assert_eq!(result, "production");
    }

    #[test]
    fn test_upper_filter() {
        let resolver = TemplateResolver::new();
        let mut ctx = ResolverContext::new();
        ctx.variables
            .insert("name".to_string(), serde_json::json!("production"));

        let result = resolver
            .resolve_string("{{ variables.name | upper }}", &ctx)
            .unwrap();
        assert_eq!(result, "PRODUCTION");
    }

    #[test]
    fn test_title_filter() {
        let resolver = TemplateResolver::new();
        let mut ctx = ResolverContext::new();
        ctx.variables
            .insert("name".to_string(), serde_json::json!("hello world"));

        let result = resolver
            .resolve_string("{{ variables.name | title }}", &ctx)
            .unwrap();
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_replace_filter() {
        let resolver = TemplateResolver::new();
        let mut ctx = ResolverContext::new();
        ctx.variables
            .insert("text".to_string(), serde_json::json!("hello-world"));

        let result = resolver
            .resolve_string("{{ variables.text | replace('-', '_') }}", &ctx)
            .unwrap();
        assert_eq!(result, "hello_world");
    }

    // ========================================================================
    // Test 15: Full Config Resolution
    // ========================================================================

    #[test]
    fn test_resolve_full_config() {
        let resolver = TemplateResolver::new();

        let yaml = r#"
variables:
  vpc_cidr: "10.0.0.0/16"
  environment: "production"

resources:
  aws_vpc:
    main:
      cidr_block: "{{ variables.vpc_cidr }}"
      tags:
        Environment: "{{ variables.environment }}"

outputs:
  vpc_id:
    value: "{{ resources.aws_vpc.main.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();

        let mut state = ProvisioningState::new();
        let vpc_state = super::super::state::ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({"cidr_block": "10.0.0.0/16"}),
            serde_json::json!({"id": "vpc-123", "cidr_block": "10.0.0.0/16"}),
        );
        state.add_resource(vpc_state);

        let resolved = resolver.resolve_config(&config, &state).unwrap();

        // Check resolved resources
        let vpc_config = resolved.get_resource_by_name("aws_vpc", "main").unwrap();
        assert_eq!(vpc_config["cidr_block"], "10.0.0.0/16");
        assert_eq!(vpc_config["tags"]["Environment"], "production");

        // Check resolved outputs
        assert_eq!(resolved.outputs["vpc_id"], "vpc-123");
    }

    // ========================================================================
    // Test 16: Empty Config
    // ========================================================================

    #[test]
    fn test_empty_config() {
        let resolver = TemplateResolver::new();
        let config = InfrastructureConfig::new();
        let state = ProvisioningState::new();

        let resolved = resolver.resolve_config(&config, &state).unwrap();
        assert!(resolved.resources.is_empty());
        assert!(resolved.outputs.is_empty());
        assert!(resolved.resolution_order.is_empty());
    }

    // ========================================================================
    // Test 17: No Templates in Config
    // ========================================================================

    #[test]
    fn test_no_templates_in_config() {
        let resolver = TemplateResolver::new();

        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
      enable_dns_hostnames: true
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let state = ProvisioningState::new();

        let resolved = resolver.resolve_config(&config, &state).unwrap();
        let vpc_config = resolved.get_resource_by_name("aws_vpc", "main").unwrap();

        assert_eq!(vpc_config["cidr_block"], "10.0.0.0/16");
        assert_eq!(vpc_config["enable_dns_hostnames"], true);
    }

    // ========================================================================
    // Test 18: Resource with Hyphen in Name
    // ========================================================================

    #[test]
    fn test_resource_with_hyphen_in_name() {
        let resolver = TemplateResolver::new();

        let yaml = r#"
resources:
  aws_vpc:
    my-vpc:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    my-subnet:
      vpc_id: "{{ resources.aws_vpc.my-vpc.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let deps =
            resolver.extract_dependencies(config.get_resource("aws_subnet.my-subnet").unwrap());

        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "my-vpc");
    }

    // ========================================================================
    // Test 19: Validate References
    // ========================================================================

    #[test]
    fn test_validate_references_success() {
        let resolver = TemplateResolver::new();

        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let result = resolver.validate_references(&config);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_references_failure() {
        let resolver = TemplateResolver::new();

        let yaml = r#"
resources:
  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.nonexistent.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let result = resolver.validate_references(&config);

        assert!(result.is_err());
    }

    // ========================================================================
    // Test 20+: Helper Functions and Edge Cases
    // ========================================================================

    #[test]
    fn test_is_template() {
        assert!(is_template("{{ var }}"));
        assert!(is_template("prefix {{ var }} suffix"));
        assert!(!is_template("plain string"));
        assert!(!is_template("{ not a template }"));
        assert!(!is_template("{{ incomplete"));
        assert!(!is_template("incomplete }}"));
    }

    #[test]
    fn test_get_nested_value() {
        let value = serde_json::json!({
            "a": {
                "b": {
                    "c": "deep value"
                }
            },
            "array": [1, 2, 3]
        });

        assert_eq!(
            get_nested_value(&value, "a.b.c"),
            Some(serde_json::json!("deep value"))
        );
        assert_eq!(
            get_nested_value(&value, "array"),
            Some(serde_json::json!([1, 2, 3]))
        );
        assert_eq!(get_nested_value(&value, "nonexistent"), None);
    }

    #[test]
    fn test_get_nested_value_with_array_notation() {
        let value = serde_json::json!({
            "items": [
                {"name": "first"},
                {"name": "second"}
            ]
        });

        assert_eq!(
            get_nested_value(&value, "items.0.name"),
            Some(serde_json::json!("first"))
        );
        assert_eq!(
            get_nested_value(&value, "items.1.name"),
            Some(serde_json::json!("second"))
        );
    }

    #[test]
    fn test_json_to_jinja_conversion() {
        let json_val = serde_json::json!({
            "string": "hello",
            "number": 42,
            "boolean": true,
            "null_val": null,
            "array": [1, 2, 3]
        });

        let jinja_val = json_to_jinja(&json_val);
        assert!(!jinja_val.is_undefined());
    }

    #[test]
    fn test_resource_reference_formatting() {
        let simple_ref = ResourceReference::new("aws_vpc", "main", "id");
        assert_eq!(
            simple_ref.to_reference_string(),
            "resources.aws_vpc.main.id"
        );

        let indexed_ref = ResourceReference::with_index("aws_instance", "web", 0, "public_ip");
        assert_eq!(
            indexed_ref.to_reference_string(),
            "resources.aws_instance[0].public_ip"
        );
    }

    #[test]
    fn test_resolver_context_to_jinja() {
        let mut ctx = ResolverContext::new();
        ctx.variables
            .insert("name".to_string(), serde_json::json!("test"));
        ctx.set_resource("aws_vpc", "main", serde_json::json!({"id": "vpc-123"}));

        let jinja_ctx = ctx.to_jinja_value();
        assert!(!jinja_ctx.is_undefined());
    }

    #[test]
    fn test_has_templates() {
        assert!(TemplateResolver::has_templates(&serde_json::json!(
            "{{ foo }}"
        )));
        assert!(TemplateResolver::has_templates(
            &serde_json::json!({"key": "{{ bar }}"})
        ));
        assert!(TemplateResolver::has_templates(&serde_json::json!([
            "{{ baz }}"
        ])));
        assert!(!TemplateResolver::has_templates(&serde_json::json!(
            "plain"
        )));
        assert!(!TemplateResolver::has_templates(&serde_json::json!(123)));
    }

    #[test]
    fn test_context_from_config_and_state() {
        let config = InfrastructureConfig::from_str(
            r#"
variables:
  env: production
locals:
  name: my-app
"#,
        )
        .unwrap();

        let mut state = ProvisioningState::new();
        state.add_resource(super::super::state::ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-12345",
            "aws",
            serde_json::json!({"cidr_block": "10.0.0.0/16"}),
            serde_json::json!({"arn": "arn:aws:ec2:vpc/vpc-12345"}),
        ));

        let ctx = ResolverContext::from_config_and_state(&config, &state);

        assert_eq!(
            ctx.variables.get("env"),
            Some(&serde_json::json!("production"))
        );
        assert_eq!(ctx.locals.get("name"), Some(&serde_json::json!("my-app")));
        assert!(ctx.resources.contains_key("aws_vpc.main"));

        let vpc_attrs = ctx.resources.get("aws_vpc.main").unwrap();
        assert_eq!(vpc_attrs.get("id"), Some(&serde_json::json!("vpc-12345")));
    }

    #[test]
    fn test_local_reference() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!("{{ locals.computed_name }}");
        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        assert_eq!(resolved, serde_json::json!("my-resource"));
    }

    #[test]
    fn test_mixed_static_and_template() {
        let resolver = TemplateResolver::new();
        let ctx = create_test_context();

        let value = serde_json::json!({
            "static_field": "constant",
            "dynamic_field": "prefix-{{ variables.environment }}-suffix"
        });

        let resolved = resolver.resolve_value(&value, &ctx).unwrap();

        assert_eq!(resolved["static_field"], "constant");
        assert_eq!(resolved["dynamic_field"], "prefix-production-suffix");
    }

    #[test]
    fn test_parse_array_access() {
        let result = parse_array_access("aws_instance.web[0].public_ip");
        assert!(result.is_some());
        let (rtype, name, idx, attr) = result.unwrap();
        assert_eq!(rtype, "aws_instance");
        assert_eq!(name, "web");
        assert_eq!(idx, 0);
        assert_eq!(attr, "public_ip");

        let result2 = parse_array_access("aws_instance.web[1]");
        assert!(result2.is_some());
        let (_, _, idx2, attr2) = result2.unwrap();
        assert_eq!(idx2, 1);
        assert_eq!(attr2, "");

        assert!(parse_array_access("aws_vpc.main.id").is_none());
    }

    #[test]
    fn test_passthrough_non_template_values() {
        let ctx = ResolverContext::new();
        let resolver = TemplateResolver::new();

        let test_cases = vec![
            (serde_json::json!(42), serde_json::json!(42)),
            (serde_json::json!(true), serde_json::json!(true)),
            (serde_json::json!(null), serde_json::json!(null)),
            (serde_json::json!(3.14), serde_json::json!(3.14)),
            (
                serde_json::json!("plain string"),
                serde_json::json!("plain string"),
            ),
            (
                serde_json::json!(["a", "b", "c"]),
                serde_json::json!(["a", "b", "c"]),
            ),
            (
                serde_json::json!({"key": "value", "num": 123}),
                serde_json::json!({"key": "value", "num": 123}),
            ),
        ];

        for (input, expected) in test_cases {
            let (resolved, unresolved) = resolver.resolve_value_internal(&input, &ctx, "test");
            assert!(
                unresolved.is_empty(),
                "Non-template value should have no unresolved refs"
            );
            assert_eq!(resolved, expected, "Value should pass through unchanged");
        }
    }

    #[test]
    fn test_extract_template_refs() {
        let resolver = TemplateResolver::new();

        let value = serde_json::json!({
            "vpc_id": "{{ resources.aws_vpc.main.id }}",
            "subnets": ["{{ resources.aws_subnet.public.id }}"],
            "env": "{{ variables.environment }}"
        });

        let refs = resolver.extract_template_refs(&value);

        assert_eq!(refs.len(), 3);
        assert!(refs.contains(&"resources.aws_vpc.main.id".to_string()));
        assert!(refs.contains(&"resources.aws_subnet.public.id".to_string()));
        assert!(refs.contains(&"variables.environment".to_string()));
    }

    #[test]
    fn test_complex_dependency_chain() {
        let yaml = r#"
resources:
  aws_vpc:
    main:
      cidr_block: "10.0.0.0/16"

  aws_internet_gateway:
    main:
      vpc_id: "{{ resources.aws_vpc.main.id }}"

  aws_route_table:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
      gateway_id: "{{ resources.aws_internet_gateway.main.id }}"

  aws_subnet:
    public:
      vpc_id: "{{ resources.aws_vpc.main.id }}"
      route_table_id: "{{ resources.aws_route_table.public.id }}"

  aws_instance:
    web:
      subnet_id: "{{ resources.aws_subnet.public.id }}"
"#;

        let config = InfrastructureConfig::from_str(yaml).unwrap();
        let resolver = TemplateResolver::new();

        let order = resolver.get_resolution_order(&config).unwrap();

        let indices: HashMap<String, usize> = order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.address(), i))
            .collect();

        let vpc_idx = indices.get("aws_vpc.main").expect("VPC should be in order");

        if let Some(igw_idx) = indices.get("aws_internet_gateway.main") {
            assert!(vpc_idx < igw_idx, "VPC should come before IGW");
        }

        if let Some(rt_idx) = indices.get("aws_route_table.public") {
            assert!(vpc_idx < rt_idx, "VPC should come before Route Table");
            if let Some(igw_idx) = indices.get("aws_internet_gateway.main") {
                assert!(igw_idx < rt_idx, "IGW should come before Route Table");
            }
        }

        if let Some(subnet_idx) = indices.get("aws_subnet.public") {
            assert!(vpc_idx < subnet_idx, "VPC should come before Subnet");
            if let Some(rt_idx) = indices.get("aws_route_table.public") {
                assert!(rt_idx < subnet_idx, "Route Table should come before Subnet");
            }
        }

        if let (Some(subnet_idx), Some(instance_idx)) = (
            indices.get("aws_subnet.public"),
            indices.get("aws_instance.web"),
        ) {
            assert!(
                subnet_idx < instance_idx,
                "Subnet should come before Instance"
            );
        }
    }
}
