//! Constructed Inventory Plugin for Rustible
//!
//! The constructed inventory plugin creates dynamic groups based on host variables
//! (hostvars) using expressions and keyed groups. This is similar to Ansible's
//! `ansible.builtin.constructed` inventory plugin.
//!
//! # Features
//!
//! - **Group Composition**: Create groups based on hostvar expressions
//! - **Keyed Groups**: Dynamically generate groups from variable values
//! - **Compose Variables**: Set host variables from expressions
//! - **Group Inheritance**: Support parent-child group relationships
//! - **Strict Mode**: Optionally fail on expression errors
//!
//! # Configuration
//!
//! ```yaml
//! plugin: constructed
//! strict: false
//! groups:
//!   # Create group based on condition
//!   webservers: "'nginx' in installed_packages or 'apache' in installed_packages"
//!   production: "environment == 'prod'"
//!   has_ssd: "disk_type is defined and disk_type == 'ssd'"
//! keyed_groups:
//!   # Create groups from variable values
//!   - key: environment
//!     prefix: env
//!     separator: "_"
//!   - key: region
//!     prefix: region
//!   - key: os_family
//!     default_value: unknown
//! compose:
//!   # Set host variables from expressions
//!   ansible_host: "primary_ip | default(ansible_host)"
//!   custom_port: "web_port | default(8080)"
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::inventory::constructed::{ConstructedPlugin, ConstructedConfig};
//! # use rustible::inventory::Inventory;
//! # let base_inventory = Inventory::new();
//!
//! // Create configuration
//! let config = ConstructedConfig::builder()
//!     .group("production", "environment == 'prod'")
//!     .keyed_group("region", "region", "_")
//!     .compose("ansible_host", "private_ip")
//!     .build()?;
//!
//! // Apply to existing inventory
//! let plugin = ConstructedPlugin::new(config)?;
//! let enhanced_inventory = plugin.process(base_inventory)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Expression Syntax
//!
//! The plugin uses a simple expression language that supports:
//! - Variable access: `variable_name`, `nested.variable`
//! - Comparisons: `==`, `!=`, `<`, `>`, `<=`, `>=`
//! - Boolean operators: `and`, `or`, `not`
//! - String operations: `in`, `startswith`, `endswith`
//! - Filters: `default(value)`, `lower`, `upper`
//! - Conditionals: `is defined`, `is not defined`

use async_trait::async_trait;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use thiserror::Error;

use super::plugins::config::{KeyedGroupConfig, PluginConfig, PluginConfigError};
use super::plugins::{DynamicInventoryPlugin, PluginOption, PluginOptionType};
use super::{Group, Inventory, InventoryError, InventoryResult};

// ============================================================================
// Configuration
// ============================================================================

/// Errors specific to the constructed plugin
#[derive(Debug, Error)]
pub enum ConstructedError {
    #[error("Expression evaluation error: {0}")]
    ExpressionError(String),

    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    #[error("Variable not found: {0}")]
    VariableNotFound(String),

    #[error("Type error: {0}")]
    TypeError(String),

    #[error("Group inheritance cycle detected: {0}")]
    CyclicInheritance(String),
}

impl From<ConstructedError> for InventoryError {
    fn from(err: ConstructedError) -> Self {
        InventoryError::DynamicInventoryFailed(err.to_string())
    }
}

/// Configuration for the constructed inventory plugin
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConstructedConfig {
    /// Groups to create based on expressions
    /// Key: group name, Value: expression that evaluates to boolean
    #[serde(default)]
    pub groups: HashMap<String, String>,

    /// Keyed groups for dynamic group creation
    #[serde(default)]
    pub keyed_groups: Vec<KeyedGroupConfig>,

    /// Variable composition expressions
    /// Key: variable name to set, Value: expression to evaluate
    #[serde(default)]
    pub compose: HashMap<String, String>,

    /// Group parents (inheritance)
    /// Key: child group, Value: parent group(s)
    #[serde(default)]
    pub group_parents: HashMap<String, Vec<String>>,

    /// Whether to fail on expression errors
    #[serde(default)]
    pub strict: bool,

    /// Whether to use Jinja2-style expressions (vs simple)
    #[serde(default)]
    pub use_jinja: bool,

    /// Cache TTL in seconds (0 = no caching)
    #[serde(default)]
    pub cache_ttl: u64,

    /// Leading separator for keyed groups
    #[serde(default = "default_leading_separator")]
    pub leading_separator: bool,

    /// Default keyed group separator
    #[serde(default = "default_separator")]
    pub default_separator: String,
}

fn default_leading_separator() -> bool {
    true
}

fn default_separator() -> String {
    "_".to_string()
}

impl ConstructedConfig {
    /// Create a new empty configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a builder for constructed config
    pub fn builder() -> ConstructedConfigBuilder {
        ConstructedConfigBuilder::default()
    }

    /// Load configuration from a PluginConfig
    pub fn from_plugin_config(config: &PluginConfig) -> Result<Self, PluginConfigError> {
        let mut result = Self::new();

        // Copy keyed groups
        result.keyed_groups = config.keyed_groups.clone();

        // Copy strict mode
        result.strict = config.strict;

        // Copy cache TTL
        result.cache_ttl = config.cache_ttl;

        // Parse groups from extra options
        if let Some(groups) = config.extra.get("groups") {
            if let Some(groups_map) = groups.as_mapping() {
                for (key, value) in groups_map {
                    if let (Some(k), Some(v)) = (key.as_str(), value.as_str()) {
                        result.groups.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }

        // Parse compose from config
        if let Some(ref ansible_host) = config.compose.ansible_host {
            result
                .compose
                .insert("ansible_host".to_string(), ansible_host.clone());
        }
        if let Some(ref ansible_port) = config.compose.ansible_port {
            result
                .compose
                .insert("ansible_port".to_string(), ansible_port.clone());
        }
        if let Some(ref ansible_user) = config.compose.ansible_user {
            result
                .compose
                .insert("ansible_user".to_string(), ansible_user.clone());
        }
        for (key, value) in &config.compose.extra_vars {
            result.compose.insert(key.clone(), value.clone());
        }

        // Parse group parents
        if let Some(parents) = config.extra.get("group_parents") {
            if let Some(parents_map) = parents.as_mapping() {
                for (key, value) in parents_map {
                    if let Some(k) = key.as_str() {
                        let parent_list: Vec<String> = match value {
                            serde_yaml::Value::String(s) => vec![s.clone()],
                            serde_yaml::Value::Sequence(seq) => seq
                                .iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect(),
                            _ => Vec::new(),
                        };
                        result.group_parents.insert(k.to_string(), parent_list);
                    }
                }
            }
        }

        Ok(result)
    }
}

/// Builder for ConstructedConfig
#[derive(Debug, Default)]
pub struct ConstructedConfigBuilder {
    config: ConstructedConfig,
}

impl ConstructedConfigBuilder {
    /// Add a conditional group
    pub fn group(mut self, name: impl Into<String>, expression: impl Into<String>) -> Self {
        self.config.groups.insert(name.into(), expression.into());
        self
    }

    /// Add a keyed group
    pub fn keyed_group(
        mut self,
        key: impl Into<String>,
        prefix: impl Into<String>,
        separator: impl Into<String>,
    ) -> Self {
        self.config.keyed_groups.push(KeyedGroupConfig {
            key: key.into(),
            prefix: prefix.into(),
            separator: separator.into(),
            parent_group: None,
            default_value: None,
            trailing_separator: false,
        });
        self
    }

    /// Add a keyed group with parent
    pub fn keyed_group_with_parent(
        mut self,
        key: impl Into<String>,
        prefix: impl Into<String>,
        separator: impl Into<String>,
        parent: impl Into<String>,
    ) -> Self {
        self.config.keyed_groups.push(KeyedGroupConfig {
            key: key.into(),
            prefix: prefix.into(),
            separator: separator.into(),
            parent_group: Some(parent.into()),
            default_value: None,
            trailing_separator: false,
        });
        self
    }

    /// Add a keyed group with default value
    pub fn keyed_group_with_default(
        mut self,
        key: impl Into<String>,
        prefix: impl Into<String>,
        default: impl Into<String>,
    ) -> Self {
        self.config.keyed_groups.push(KeyedGroupConfig {
            key: key.into(),
            prefix: prefix.into(),
            separator: default_separator(),
            parent_group: None,
            default_value: Some(default.into()),
            trailing_separator: false,
        });
        self
    }

    /// Add a compose expression
    pub fn compose(mut self, variable: impl Into<String>, expression: impl Into<String>) -> Self {
        self.config
            .compose
            .insert(variable.into(), expression.into());
        self
    }

    /// Set group parent relationship
    pub fn parent(mut self, child: impl Into<String>, parent: impl Into<String>) -> Self {
        let child_name = child.into();
        self.config
            .group_parents
            .entry(child_name)
            .or_default()
            .push(parent.into());
        self
    }

    /// Enable strict mode
    pub fn strict(mut self, strict: bool) -> Self {
        self.config.strict = strict;
        self
    }

    /// Set cache TTL
    pub fn cache_ttl(mut self, ttl: u64) -> Self {
        self.config.cache_ttl = ttl;
        self
    }

    /// Build the configuration
    pub fn build(self) -> Result<ConstructedConfig, PluginConfigError> {
        // Validate configuration
        self.validate()?;
        Ok(self.config)
    }

    fn validate(&self) -> Result<(), PluginConfigError> {
        // Check for cyclic inheritance
        let mut visited = HashSet::new();
        for child in self.config.group_parents.keys() {
            if self.has_cycle(child, &mut visited, &mut HashSet::new()) {
                return Err(PluginConfigError::Invalid(format!(
                    "Cyclic group inheritance detected involving '{}'",
                    child
                )));
            }
        }
        Ok(())
    }

    fn has_cycle(
        &self,
        group: &str,
        visited: &mut HashSet<String>,
        path: &mut HashSet<String>,
    ) -> bool {
        if path.contains(group) {
            return true;
        }
        if visited.contains(group) {
            return false;
        }

        visited.insert(group.to_string());
        path.insert(group.to_string());

        if let Some(parents) = self.config.group_parents.get(group) {
            for parent in parents {
                if self.has_cycle(parent, visited, path) {
                    return true;
                }
            }
        }

        path.remove(group);
        false
    }
}

// ============================================================================
// Expression Evaluator
// ============================================================================

/// Simple expression evaluator for host variables
#[derive(Debug)]
pub struct ExpressionEvaluator {
    strict: bool,
}

impl ExpressionEvaluator {
    /// Create a new expression evaluator
    pub fn new(strict: bool) -> Self {
        Self { strict }
    }

    /// Evaluate an expression to a boolean result
    pub fn evaluate_bool(
        &self,
        expr: &str,
        hostvars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<bool, ConstructedError> {
        let expr = expr.trim();

        // Handle empty expression
        if expr.is_empty() {
            return Ok(false);
        }

        // Handle boolean literals
        if expr == "true" || expr == "True" {
            return Ok(true);
        }
        if expr == "false" || expr == "False" {
            return Ok(false);
        }

        // Handle 'is defined' checks
        if let Some(var_name) = Self::parse_is_defined(expr) {
            return Ok(hostvars.contains_key(var_name));
        }

        // Handle 'is not defined' checks
        if let Some(var_name) = Self::parse_is_not_defined(expr) {
            return Ok(!hostvars.contains_key(var_name));
        }

        // Handle 'or' operator first (lowest precedence, so parse first)
        if let Some((left, right)) = Self::parse_or_operator(expr) {
            let left_result = self.evaluate_bool(left, hostvars)?;
            let right_result = self.evaluate_bool(right, hostvars)?;
            return Ok(left_result || right_result);
        }

        // Handle 'and' operator (higher precedence than 'or')
        if let Some((left, right)) = Self::parse_and_operator(expr) {
            let left_result = self.evaluate_bool(left, hostvars)?;
            let right_result = self.evaluate_bool(right, hostvars)?;
            return Ok(left_result && right_result);
        }

        // Handle 'not' operator
        if let Some(inner) = Self::parse_not_operator(expr) {
            let inner_result = self.evaluate_bool(inner, hostvars)?;
            return Ok(!inner_result);
        }

        // Handle 'in' operator
        if let Some((item, collection)) = Self::parse_in_operator(expr) {
            return self.evaluate_in_operator(item, collection, hostvars);
        }

        // Handle comparison operators
        if let Some((left, op, right)) = Self::parse_comparison(expr) {
            return self.evaluate_comparison(left, op, right, hostvars);
        }

        // Try to evaluate as a truthy value
        match self.evaluate_value(expr, hostvars) {
            Ok(value) => Ok(Self::is_truthy(&value)),
            Err(e) if self.strict => Err(e),
            Err(_) => Ok(false),
        }
    }

    /// Evaluate an expression to a value
    pub fn evaluate_value(
        &self,
        expr: &str,
        hostvars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<serde_yaml::Value, ConstructedError> {
        let expr = expr.trim();

        // Handle quoted strings
        if (expr.starts_with('"') && expr.ends_with('"'))
            || (expr.starts_with('\'') && expr.ends_with('\''))
        {
            return Ok(serde_yaml::Value::String(
                expr[1..expr.len() - 1].to_string(),
            ));
        }

        // Handle numeric literals
        if let Ok(n) = expr.parse::<i64>() {
            return Ok(serde_yaml::Value::Number(n.into()));
        }
        if let Ok(n) = expr.parse::<f64>() {
            return Ok(serde_yaml::Value::Number(serde_yaml::Number::from(
                n as i64,
            )));
        }

        // Handle boolean literals
        if expr == "true" || expr == "True" {
            return Ok(serde_yaml::Value::Bool(true));
        }
        if expr == "false" || expr == "False" {
            return Ok(serde_yaml::Value::Bool(false));
        }

        // Handle null
        if expr == "null" || expr == "None" || expr == "none" {
            return Ok(serde_yaml::Value::Null);
        }

        // Handle default filter: "var | default(value)"
        if let Some((var, default)) = Self::parse_default_filter(expr) {
            match self.resolve_variable(var, hostvars) {
                Ok(v) if v != serde_yaml::Value::Null => return Ok(v),
                _ => return self.evaluate_value(default, hostvars),
            }
        }

        // Handle other filters
        if let Some((value_expr, filter)) = Self::parse_filter(expr) {
            let value = self.evaluate_value(value_expr, hostvars)?;
            return self.apply_filter(&value, filter);
        }

        // Resolve variable reference
        self.resolve_variable(expr, hostvars)
    }

    /// Resolve a variable reference (including nested paths)
    fn resolve_variable(
        &self,
        var_path: &str,
        hostvars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<serde_yaml::Value, ConstructedError> {
        let parts: Vec<&str> = var_path.split('.').collect();
        let root = parts[0];

        let mut current = hostvars
            .get(root)
            .cloned()
            .ok_or_else(|| ConstructedError::VariableNotFound(var_path.to_string()))?;

        for part in parts.iter().skip(1) {
            current = match current {
                serde_yaml::Value::Mapping(ref map) => map
                    .get(serde_yaml::Value::String(part.to_string()))
                    .cloned()
                    .unwrap_or(serde_yaml::Value::Null),
                _ => serde_yaml::Value::Null,
            };
        }

        Ok(current)
    }

    /// Apply a filter to a value
    fn apply_filter(
        &self,
        value: &serde_yaml::Value,
        filter: &str,
    ) -> Result<serde_yaml::Value, ConstructedError> {
        let filter = filter.trim();

        match filter {
            "lower" => {
                if let serde_yaml::Value::String(s) = value {
                    Ok(serde_yaml::Value::String(s.to_lowercase()))
                } else {
                    Ok(value.clone())
                }
            }
            "upper" => {
                if let serde_yaml::Value::String(s) = value {
                    Ok(serde_yaml::Value::String(s.to_uppercase()))
                } else {
                    Ok(value.clone())
                }
            }
            "trim" => {
                if let serde_yaml::Value::String(s) = value {
                    Ok(serde_yaml::Value::String(s.trim().to_string()))
                } else {
                    Ok(value.clone())
                }
            }
            "string" => Ok(serde_yaml::Value::String(Self::value_to_string(value))),
            "int" => {
                let s = Self::value_to_string(value);
                let n: i64 = s.parse().unwrap_or(0);
                Ok(serde_yaml::Value::Number(n.into()))
            }
            "bool" => Ok(serde_yaml::Value::Bool(Self::is_truthy(value))),
            "length" | "len" => {
                let len = match value {
                    serde_yaml::Value::String(s) => s.len(),
                    serde_yaml::Value::Sequence(seq) => seq.len(),
                    serde_yaml::Value::Mapping(map) => map.len(),
                    _ => 0,
                };
                Ok(serde_yaml::Value::Number((len as i64).into()))
            }
            _ => {
                if self.strict {
                    Err(ConstructedError::ExpressionError(format!(
                        "Unknown filter: {}",
                        filter
                    )))
                } else {
                    Ok(value.clone())
                }
            }
        }
    }

    // Parser helper functions
    fn parse_is_defined(expr: &str) -> Option<&str> {
        // Pattern: "var is defined"
        static RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(\w+(?:\.\w+)*)\s+is\s+defined$").expect("Invalid regex"));
        RE.captures(expr).map(|c| c.get(1).unwrap().as_str())
    }

    fn parse_is_not_defined(expr: &str) -> Option<&str> {
        // Pattern: "var is not defined" or "var is undefined"
        static RE_NOT_DEFINED: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^(\w+(?:\.\w+)*)\s+is\s+not\s+defined$").expect("Invalid regex")
        });
        if let Some(caps) = RE_NOT_DEFINED.captures(expr) {
            return Some(caps.get(1).unwrap().as_str());
        }

        static RE_UNDEFINED: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(\w+(?:\.\w+)*)\s+is\s+undefined$").expect("Invalid regex"));
        RE_UNDEFINED
            .captures(expr)
            .map(|c| c.get(1).unwrap().as_str())
    }

    fn parse_in_operator(expr: &str) -> Option<(&str, &str)> {
        // Pattern: "'item' in collection" or "item in collection"
        static RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(.+?)\s+in\s+(.+)$").expect("Invalid regex"));
        RE.captures(expr).map(|c| {
            (
                c.get(1).unwrap().as_str().trim(),
                c.get(2).unwrap().as_str().trim(),
            )
        })
    }

    fn parse_comparison(expr: &str) -> Option<(&str, &str, &str)> {
        // Pattern: "left op right" where op is ==, !=, <, >, <=, >=
        static RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(.+?)\s*(==|!=|<=|>=|<|>)\s*(.+)$").expect("Invalid regex"));
        RE.captures(expr).map(|c| {
            (
                c.get(1).unwrap().as_str().trim(),
                c.get(2).unwrap().as_str(),
                c.get(3).unwrap().as_str().trim(),
            )
        })
    }

    fn parse_and_operator(expr: &str) -> Option<(&str, &str)> {
        // Split on ' and ' (word boundary)
        Self::split_logical_operator(expr, " and ")
    }

    fn parse_or_operator(expr: &str) -> Option<(&str, &str)> {
        // Split on ' or ' (word boundary)
        Self::split_logical_operator(expr, " or ")
    }

    fn split_logical_operator<'a>(expr: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
        // Find the operator, avoiding splitting inside quotes or parentheses
        let mut depth: usize = 0;
        let mut in_quote = false;
        let mut quote_char = ' ';

        let op_bytes = op.as_bytes();
        let expr_bytes = expr.as_bytes();

        for i in 0..expr.len().saturating_sub(op.len()) {
            let ch = expr_bytes[i] as char;

            if in_quote {
                if ch == quote_char {
                    in_quote = false;
                }
                continue;
            }

            match ch {
                '"' | '\'' => {
                    in_quote = true;
                    quote_char = ch;
                }
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                _ if depth == 0 && &expr_bytes[i..i + op.len()] == op_bytes => {
                    return Some((expr[..i].trim(), expr[i + op.len()..].trim()));
                }
                _ => {}
            }
        }
        None
    }

    fn parse_not_operator(expr: &str) -> Option<&str> {
        let expr = expr.trim();
        if let Some(stripped) = expr.strip_prefix("not ") {
            Some(stripped.trim())
        } else if let Some(stripped) = expr.strip_prefix('!') {
            Some(stripped.trim())
        } else {
            None
        }
    }

    fn parse_default_filter(expr: &str) -> Option<(&str, &str)> {
        // Pattern: "var | default(value)" or "var | d(value)"
        static RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^(.+?)\s*\|\s*(?:default|d)\s*\(\s*(.+?)\s*\)$").expect("Invalid regex")
        });
        RE.captures(expr).map(|c| {
            (
                c.get(1).unwrap().as_str().trim(),
                c.get(2).unwrap().as_str().trim(),
            )
        })
    }

    fn parse_filter(expr: &str) -> Option<(&str, &str)> {
        // Pattern: "value | filter"
        static RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(.+?)\s*\|\s*(\w+)$").expect("Invalid regex"));
        RE.captures(expr).map(|c| {
            (
                c.get(1).unwrap().as_str().trim(),
                c.get(2).unwrap().as_str().trim(),
            )
        })
    }

    fn evaluate_in_operator(
        &self,
        item: &str,
        collection: &str,
        hostvars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<bool, ConstructedError> {
        let item_value = self.evaluate_value(item, hostvars)?;
        let collection_value = self.evaluate_value(collection, hostvars)?;

        match collection_value {
            serde_yaml::Value::Sequence(seq) => Ok(seq.contains(&item_value)),
            serde_yaml::Value::String(s) => {
                let item_str = Self::value_to_string(&item_value);
                Ok(s.contains(&item_str))
            }
            serde_yaml::Value::Mapping(map) => {
                let item_str = Self::value_to_string(&item_value);
                Ok(map.contains_key(serde_yaml::Value::String(item_str)))
            }
            _ => Ok(false),
        }
    }

    fn evaluate_comparison(
        &self,
        left: &str,
        op: &str,
        right: &str,
        hostvars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<bool, ConstructedError> {
        let left_value = self.evaluate_value(left, hostvars)?;
        let right_value = self.evaluate_value(right, hostvars)?;

        match op {
            "==" => Ok(left_value == right_value),
            "!=" => Ok(left_value != right_value),
            "<" | ">" | "<=" | ">=" => {
                // Numeric comparison
                let left_num = Self::value_to_f64(&left_value);
                let right_num = Self::value_to_f64(&right_value);

                if let (Some(l), Some(r)) = (left_num, right_num) {
                    match op {
                        "<" => Ok(l < r),
                        ">" => Ok(l > r),
                        "<=" => Ok(l <= r),
                        ">=" => Ok(l >= r),
                        _ => unreachable!(),
                    }
                } else {
                    // String comparison fallback
                    let left_str = Self::value_to_string(&left_value);
                    let right_str = Self::value_to_string(&right_value);
                    match op {
                        "<" => Ok(left_str < right_str),
                        ">" => Ok(left_str > right_str),
                        "<=" => Ok(left_str <= right_str),
                        ">=" => Ok(left_str >= right_str),
                        _ => unreachable!(),
                    }
                }
            }
            _ => Err(ConstructedError::ExpressionError(format!(
                "Unknown operator: {}",
                op
            ))),
        }
    }

    fn is_truthy(value: &serde_yaml::Value) -> bool {
        match value {
            serde_yaml::Value::Null => false,
            serde_yaml::Value::Bool(b) => *b,
            serde_yaml::Value::Number(n) => n.as_i64().map(|i| i != 0).unwrap_or(true),
            serde_yaml::Value::String(s) => !s.is_empty(),
            serde_yaml::Value::Sequence(seq) => !seq.is_empty(),
            serde_yaml::Value::Mapping(map) => !map.is_empty(),
            serde_yaml::Value::Tagged(_) => true,
        }
    }

    fn value_to_string(value: &serde_yaml::Value) -> String {
        match value {
            serde_yaml::Value::Null => String::new(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::String(s) => s.clone(),
            serde_yaml::Value::Sequence(_) | serde_yaml::Value::Mapping(_) => {
                serde_yaml::to_string(value).unwrap_or_default()
            }
            serde_yaml::Value::Tagged(t) => Self::value_to_string(&t.value),
        }
    }

    fn value_to_f64(value: &serde_yaml::Value) -> Option<f64> {
        match value {
            serde_yaml::Value::Number(n) => n.as_f64().or_else(|| n.as_i64().map(|i| i as f64)),
            serde_yaml::Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}

// ============================================================================
// Constructed Inventory Plugin
// ============================================================================

/// The constructed inventory plugin
#[derive(Debug)]
pub struct ConstructedPlugin {
    config: ConstructedConfig,
    evaluator: ExpressionEvaluator,
}

impl ConstructedPlugin {
    /// Create a new constructed plugin with configuration
    pub fn new(config: ConstructedConfig) -> Result<Self, PluginConfigError> {
        let evaluator = ExpressionEvaluator::new(config.strict);
        Ok(Self { config, evaluator })
    }

    /// Create from a PluginConfig
    pub fn from_plugin_config(config: &PluginConfig) -> Result<Self, PluginConfigError> {
        let constructed_config = ConstructedConfig::from_plugin_config(config)?;
        Self::new(constructed_config)
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self, PluginConfigError> {
        Self::new(ConstructedConfig::new())
    }

    /// Process an existing inventory and apply constructed rules
    pub fn process(&self, mut inventory: Inventory) -> InventoryResult<Inventory> {
        // Collect host names first to avoid borrow conflicts
        let host_names: Vec<String> = inventory.hosts().map(|h| h.name.clone()).collect();

        // Process each host
        for host_name in &host_names {
            let hostvars = {
                let host = inventory.get_host(host_name).unwrap();
                inventory.get_host_vars(host)
            };

            // Apply compose expressions
            let compose_results = self.evaluate_compose(&hostvars)?;

            // Apply keyed groups
            let keyed_groups = self.evaluate_keyed_groups(&hostvars)?;

            // Apply conditional groups
            let conditional_groups = self.evaluate_conditional_groups(&hostvars)?;

            // Now apply changes
            if let Some(host) = inventory.get_host_mut(host_name) {
                // Apply composed variables
                for (key, value) in compose_results {
                    match key.as_str() {
                        "ansible_host" => {
                            if let serde_yaml::Value::String(s) = &value {
                                host.ansible_host = Some(s.clone());
                            }
                        }
                        "ansible_port" => {
                            if let serde_yaml::Value::Number(n) = &value {
                                if let Some(port) = n.as_u64() {
                                    host.connection.ssh.port = port as u16;
                                }
                            }
                        }
                        "ansible_user" => {
                            if let serde_yaml::Value::String(s) = &value {
                                host.connection.ssh.user = Some(s.clone());
                            }
                        }
                        _ => {
                            host.set_var(&key, value);
                        }
                    }
                }

                // Add host to keyed groups
                for group_name in &keyed_groups {
                    host.add_to_group(group_name.clone());
                }

                // Add host to conditional groups
                for group_name in &conditional_groups {
                    host.add_to_group(group_name.clone());
                }
            }

            // Ensure groups exist in inventory
            for group_name in keyed_groups.iter().chain(conditional_groups.iter()) {
                if inventory.get_group(group_name).is_none() {
                    let group = Group::new(group_name);
                    inventory.add_group(group)?;
                }
                if let Some(group) = inventory.get_group_mut(group_name) {
                    group.add_host(host_name.clone());
                }
            }
        }

        // Apply group inheritance
        self.apply_group_inheritance(&mut inventory)?;

        Ok(inventory)
    }

    /// Evaluate compose expressions for a host
    fn evaluate_compose(
        &self,
        hostvars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<Vec<(String, serde_yaml::Value)>, ConstructedError> {
        let mut results = Vec::new();

        for (var_name, expr) in &self.config.compose {
            match self.evaluator.evaluate_value(expr, hostvars) {
                Ok(value) => {
                    results.push((var_name.clone(), value));
                }
                Err(e) if self.config.strict => {
                    return Err(e);
                }
                Err(_) => {
                    // Skip this compose on error in non-strict mode
                }
            }
        }

        Ok(results)
    }

    /// Evaluate keyed groups for a host
    fn evaluate_keyed_groups(
        &self,
        hostvars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<Vec<String>, ConstructedError> {
        let mut groups = Vec::new();

        for keyed_group in &self.config.keyed_groups {
            let value = match self.evaluator.evaluate_value(&keyed_group.key, hostvars) {
                Ok(v) if v != serde_yaml::Value::Null => Some(v),
                Ok(_) => keyed_group
                    .default_value
                    .as_ref()
                    .map(|d| serde_yaml::Value::String(d.clone())),
                Err(_) if !self.config.strict => keyed_group
                    .default_value
                    .as_ref()
                    .map(|d| serde_yaml::Value::String(d.clone())),
                Err(e) => return Err(e),
            };

            if let Some(value) = value {
                let value_str = ExpressionEvaluator::value_to_string(&value);
                if !value_str.is_empty() {
                    let group_name = keyed_group.generate_group_name(&value_str);
                    if !group_name.is_empty() {
                        groups.push(group_name);
                    }
                }
            }
        }

        Ok(groups)
    }

    /// Evaluate conditional groups for a host
    fn evaluate_conditional_groups(
        &self,
        hostvars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<Vec<String>, ConstructedError> {
        let mut groups = Vec::new();

        for (group_name, expr) in &self.config.groups {
            match self.evaluator.evaluate_bool(expr, hostvars) {
                Ok(true) => {
                    groups.push(group_name.clone());
                }
                Ok(false) => {
                    // Host doesn't match this group
                }
                Err(e) if self.config.strict => {
                    return Err(e);
                }
                Err(_) => {
                    // Skip this group on error in non-strict mode
                }
            }
        }

        Ok(groups)
    }

    /// Apply group inheritance/parents
    fn apply_group_inheritance(&self, inventory: &mut Inventory) -> InventoryResult<()> {
        for (child_name, parent_names) in &self.config.group_parents {
            // Ensure child group exists
            if inventory.get_group(child_name).is_none() {
                let group = Group::new(child_name);
                inventory.add_group(group)?;
            }

            for parent_name in parent_names {
                // Ensure parent group exists
                if inventory.get_group(parent_name).is_none() {
                    let group = Group::new(parent_name);
                    inventory.add_group(group)?;
                }

                // Set up parent-child relationship
                if let Some(parent) = inventory.get_group_mut(parent_name) {
                    parent.add_child(child_name.clone());
                }

                if let Some(child) = inventory.get_group_mut(child_name) {
                    child.add_parent(parent_name.clone());
                }
            }
        }

        // Also set up parent relationships for keyed groups with parent_group
        for keyed_group in &self.config.keyed_groups {
            if let Some(ref parent_name) = keyed_group.parent_group {
                // Ensure parent group exists
                if inventory.get_group(parent_name).is_none() {
                    let group = Group::new(parent_name);
                    inventory.add_group(group)?;
                }

                // Find all groups created by this keyed group pattern
                let prefix = &keyed_group.prefix;
                let separator = &keyed_group.separator;
                let pattern = if prefix.is_empty() {
                    String::new()
                } else {
                    format!("{}{}", prefix, separator)
                };

                let matching_groups: Vec<String> = inventory
                    .groups()
                    .filter(|g| {
                        if pattern.is_empty() {
                            // Match all groups that could be keyed groups
                            true
                        } else {
                            g.name.starts_with(&pattern)
                        }
                    })
                    .map(|g| g.name.clone())
                    .collect();

                for group_name in matching_groups {
                    if let Some(parent) = inventory.get_group_mut(parent_name) {
                        parent.add_child(group_name.clone());
                    }
                    if let Some(child) = inventory.get_group_mut(&group_name) {
                        child.add_parent(parent_name.clone());
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl DynamicInventoryPlugin for ConstructedPlugin {
    fn name(&self) -> &str {
        "constructed"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Constructed inventory plugin for dynamic group creation from host variables"
    }

    fn verify(&self) -> InventoryResult<()> {
        // Validate expressions if strict mode is enabled
        if self.config.strict {
            for (name, expr) in &self.config.groups {
                if expr.is_empty() {
                    return Err(InventoryError::DynamicInventoryFailed(format!(
                        "Empty expression for group '{}'",
                        name
                    )));
                }
            }
        }
        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        // The constructed plugin doesn't parse inventory itself,
        // it processes an existing inventory. Return empty inventory
        // as this method is required by the trait.
        Ok(Inventory::new())
    }

    fn options_documentation(&self) -> Vec<PluginOption> {
        vec![
            PluginOption {
                name: "groups".to_string(),
                description: "Groups to create based on boolean expressions".to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::Dict,
                env_var: None,
            },
            PluginOption {
                name: "keyed_groups".to_string(),
                description: "Dynamic groups based on variable values".to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::List,
                env_var: None,
            },
            PluginOption {
                name: "compose".to_string(),
                description: "Variables to set from expressions".to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::Dict,
                env_var: None,
            },
            PluginOption {
                name: "group_parents".to_string(),
                description: "Parent-child group relationships".to_string(),
                required: false,
                default: None,
                option_type: PluginOptionType::Dict,
                env_var: None,
            },
            PluginOption::optional_bool("strict", "Fail on expression evaluation errors", false),
            PluginOption {
                name: "cache_ttl".to_string(),
                description: "Cache TTL in seconds (0 = no caching)".to_string(),
                required: false,
                default: Some("0".to_string()),
                option_type: PluginOptionType::Int,
                env_var: None,
            },
        ]
    }
}

impl fmt::Display for ConstructedPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ConstructedPlugin(groups={}, keyed_groups={}, compose={})",
            self.config.groups.len(),
            self.config.keyed_groups.len(),
            self.config.compose.len()
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Host;

    fn create_test_hostvars() -> IndexMap<String, serde_yaml::Value> {
        let mut vars = IndexMap::new();
        vars.insert(
            "environment".to_string(),
            serde_yaml::Value::String("production".to_string()),
        );
        vars.insert(
            "region".to_string(),
            serde_yaml::Value::String("us-east-1".to_string()),
        );
        vars.insert(
            "os_family".to_string(),
            serde_yaml::Value::String("RedHat".to_string()),
        );
        vars.insert(
            "web_port".to_string(),
            serde_yaml::Value::Number(8080.into()),
        );
        vars.insert("enabled".to_string(), serde_yaml::Value::Bool(true));

        let mut packages = serde_yaml::Sequence::new();
        packages.push(serde_yaml::Value::String("nginx".to_string()));
        packages.push(serde_yaml::Value::String("redis".to_string()));
        vars.insert(
            "installed_packages".to_string(),
            serde_yaml::Value::Sequence(packages),
        );

        vars
    }

    #[test]
    fn test_expression_evaluator_simple_comparison() {
        let evaluator = ExpressionEvaluator::new(false);
        let hostvars = create_test_hostvars();

        assert!(evaluator
            .evaluate_bool("environment == 'production'", &hostvars)
            .unwrap());
        assert!(!evaluator
            .evaluate_bool("environment == 'staging'", &hostvars)
            .unwrap());
        assert!(evaluator
            .evaluate_bool("web_port == 8080", &hostvars)
            .unwrap());
        assert!(evaluator.evaluate_bool("web_port > 80", &hostvars).unwrap());
    }

    #[test]
    fn test_expression_evaluator_is_defined() {
        let evaluator = ExpressionEvaluator::new(false);
        let hostvars = create_test_hostvars();

        assert!(evaluator
            .evaluate_bool("environment is defined", &hostvars)
            .unwrap());
        assert!(!evaluator
            .evaluate_bool("nonexistent is defined", &hostvars)
            .unwrap());
        assert!(evaluator
            .evaluate_bool("nonexistent is not defined", &hostvars)
            .unwrap());
    }

    #[test]
    fn test_expression_evaluator_in_operator() {
        let evaluator = ExpressionEvaluator::new(false);
        let hostvars = create_test_hostvars();

        assert!(evaluator
            .evaluate_bool("'nginx' in installed_packages", &hostvars)
            .unwrap());
        assert!(!evaluator
            .evaluate_bool("'apache' in installed_packages", &hostvars)
            .unwrap());
    }

    #[test]
    fn test_expression_evaluator_boolean_operators() {
        let evaluator = ExpressionEvaluator::new(false);
        let hostvars = create_test_hostvars();

        assert!(evaluator
            .evaluate_bool(
                "environment == 'production' and region == 'us-east-1'",
                &hostvars
            )
            .unwrap());
        assert!(evaluator
            .evaluate_bool(
                "environment == 'staging' or region == 'us-east-1'",
                &hostvars
            )
            .unwrap());
        assert!(evaluator
            .evaluate_bool("not environment == 'staging'", &hostvars)
            .unwrap());
    }

    #[test]
    fn test_expression_evaluator_default_filter() {
        let evaluator = ExpressionEvaluator::new(false);
        let hostvars = create_test_hostvars();

        let result = evaluator
            .evaluate_value("web_port | default(80)", &hostvars)
            .unwrap();
        assert_eq!(result, serde_yaml::Value::Number(8080.into()));

        let result = evaluator
            .evaluate_value("nonexistent | default(9999)", &hostvars)
            .unwrap();
        assert_eq!(result, serde_yaml::Value::Number(9999.into()));
    }

    #[test]
    fn test_expression_evaluator_string_filters() {
        let evaluator = ExpressionEvaluator::new(false);
        let hostvars = create_test_hostvars();

        let result = evaluator
            .evaluate_value("region | upper", &hostvars)
            .unwrap();
        assert_eq!(result, serde_yaml::Value::String("US-EAST-1".to_string()));

        let result = evaluator
            .evaluate_value("os_family | lower", &hostvars)
            .unwrap();
        assert_eq!(result, serde_yaml::Value::String("redhat".to_string()));
    }

    #[test]
    fn test_constructed_config_builder() {
        let config = ConstructedConfig::builder()
            .group("production", "environment == 'prod'")
            .keyed_group("region", "region", "_")
            .compose("ansible_host", "private_ip")
            .parent("webservers", "production")
            .strict(true)
            .build()
            .unwrap();

        assert_eq!(config.groups.len(), 1);
        assert_eq!(config.keyed_groups.len(), 1);
        assert_eq!(config.compose.len(), 1);
        assert_eq!(config.group_parents.len(), 1);
        assert!(config.strict);
    }

    #[test]
    fn test_constructed_plugin_keyed_groups() {
        let config = ConstructedConfig::builder()
            .keyed_group("environment", "env", "_")
            .keyed_group("region", "region", "_")
            .build()
            .unwrap();

        let plugin = ConstructedPlugin::new(config).unwrap();
        let hostvars = create_test_hostvars();

        let groups = plugin.evaluate_keyed_groups(&hostvars).unwrap();
        assert!(groups.contains(&"env_production".to_string()));
        assert!(groups.contains(&"region_us_east_1".to_string()));
    }

    #[test]
    fn test_constructed_plugin_conditional_groups() {
        let config = ConstructedConfig::builder()
            .group("production_servers", "environment == 'production'")
            .group("has_nginx", "'nginx' in installed_packages")
            .group("high_ports", "web_port > 1024")
            .build()
            .unwrap();

        let plugin = ConstructedPlugin::new(config).unwrap();
        let hostvars = create_test_hostvars();

        let groups = plugin.evaluate_conditional_groups(&hostvars).unwrap();
        assert!(groups.contains(&"production_servers".to_string()));
        assert!(groups.contains(&"has_nginx".to_string()));
        assert!(groups.contains(&"high_ports".to_string()));
    }

    #[test]
    fn test_constructed_plugin_compose() {
        let config = ConstructedConfig::builder()
            .compose("custom_port", "web_port")
            .compose("default_port", "missing_var | default(80)")
            .build()
            .unwrap();

        let plugin = ConstructedPlugin::new(config).unwrap();
        let hostvars = create_test_hostvars();

        let results = plugin.evaluate_compose(&hostvars).unwrap();
        assert!(results
            .iter()
            .any(|(k, v)| k == "custom_port" && v == &serde_yaml::Value::Number(8080.into())));
        assert!(results
            .iter()
            .any(|(k, v)| k == "default_port" && v == &serde_yaml::Value::Number(80.into())));
    }

    #[test]
    fn test_constructed_plugin_process_inventory() {
        let config = ConstructedConfig::builder()
            .group("enabled_hosts", "enabled == true")
            .keyed_group("environment", "env", "_")
            .build()
            .unwrap();

        let plugin = ConstructedPlugin::new(config).unwrap();

        // Create test inventory
        let mut inventory = Inventory::new();
        let mut host = Host::new("test-host");
        host.set_var("enabled", serde_yaml::Value::Bool(true));
        host.set_var(
            "environment",
            serde_yaml::Value::String("production".to_string()),
        );
        inventory.add_host(host).unwrap();

        // Process inventory
        let processed = plugin.process(inventory).unwrap();

        // Verify groups were created
        assert!(processed.get_group("enabled_hosts").is_some());
        assert!(processed.get_group("env_production").is_some());

        // Verify host is in groups
        let host = processed.get_host("test-host").unwrap();
        assert!(host.in_group("enabled_hosts"));
        assert!(host.in_group("env_production"));
    }

    #[test]
    fn test_cyclic_inheritance_detection() {
        let result = ConstructedConfig::builder()
            .parent("a", "b")
            .parent("b", "c")
            .parent("c", "a") // Creates cycle
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_info() {
        let plugin = ConstructedPlugin::with_defaults().unwrap();
        assert_eq!(plugin.name(), "constructed");
        assert_eq!(plugin.version(), "1.0.0");
        assert!(!plugin.options_documentation().is_empty());
    }
}
