//! Custom MiniJinja Functions and Filters for Infrastructure Templates
//!
//! This module provides Terraform-compatible template functions and filters for use
//! in infrastructure configuration templates. These functions enable resource
//! references, variable lookups, CIDR calculations, and data transformations.
//!
//! # Functions
//!
//! - `resource(type, name, attribute)`: Get resource attribute
//! - `var(name, default?)`: Get variable with optional default
//! - `data(type, name, attribute)`: Get data source attribute
//! - `local(name)`: Get local value
//! - `resource_exists(type, name)`: Check if resource exists
//! - `cidr_subnet(cidr, newbits, netnum)`: Calculate subnet CIDR
//!
//! # Filters
//!
//! - `to_json`: JSON encode a value
//! - `base64`: Base64 encode a string
//! - `join`: Join list with separator
//! - `keys`: Get keys from object
//! - `values`: Get values from object
//! - `aws_tags`: Format tags for AWS API
//!
//! # Example
//!
//! ```jinja2
//! {% if resource_exists("aws_vpc", "main") %}
//! VPC ID: {{ resource("aws_vpc", "main", "id") }}
//! {% endif %}
//!
//! Subnet CIDR: {{ cidr_subnet("10.0.0.0/16", 8, 1) }}
//! Region: {{ var("region", "us-east-1") }}
//!
//! Tags: {{ tags | to_json }}
//! Encoded: {{ "hello" | base64 }}
//! ```

use base64::Engine;
use minijinja::{Environment, Error as MiniJinjaError, ErrorKind, State, Value};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::net::Ipv4Addr;

// ============================================================================
// Template Context
// ============================================================================

/// Context for template functions containing state and configuration data.
///
/// This context is passed to template functions via MiniJinja's state mechanism,
/// providing access to resources, variables, data sources, and locals.
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    /// Resource attributes indexed by "type.name"
    pub resources: HashMap<String, HashMap<String, JsonValue>>,

    /// Variables indexed by name
    pub variables: HashMap<String, JsonValue>,

    /// Data source results indexed by "type.name"
    pub data_sources: HashMap<String, HashMap<String, JsonValue>>,

    /// Local values indexed by name
    pub locals: HashMap<String, JsonValue>,
}

impl TemplateContext {
    /// Create a new empty template context
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a builder for the template context
    pub fn builder() -> TemplateContextBuilder {
        TemplateContextBuilder::new()
    }

    /// Add a resource to the context
    pub fn add_resource(
        &mut self,
        resource_type: &str,
        name: &str,
        attributes: HashMap<String, JsonValue>,
    ) {
        let key = format!("{}.{}", resource_type, name);
        self.resources.insert(key, attributes);
    }

    /// Add a variable to the context
    pub fn add_variable(&mut self, name: &str, value: JsonValue) {
        self.variables.insert(name.to_string(), value);
    }

    /// Add a data source to the context
    pub fn add_data_source(
        &mut self,
        data_type: &str,
        name: &str,
        attributes: HashMap<String, JsonValue>,
    ) {
        let key = format!("{}.{}", data_type, name);
        self.data_sources.insert(key, attributes);
    }

    /// Add a local value to the context
    pub fn add_local(&mut self, name: &str, value: JsonValue) {
        self.locals.insert(name.to_string(), value);
    }
}

/// Builder for TemplateContext
pub struct TemplateContextBuilder {
    context: TemplateContext,
}

impl TemplateContextBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            context: TemplateContext::new(),
        }
    }

    /// Add a resource
    pub fn resource(
        mut self,
        resource_type: &str,
        name: &str,
        attributes: HashMap<String, JsonValue>,
    ) -> Self {
        self.context.add_resource(resource_type, name, attributes);
        self
    }

    /// Add a variable
    pub fn variable(mut self, name: &str, value: JsonValue) -> Self {
        self.context.add_variable(name, value);
        self
    }

    /// Add a data source
    pub fn data_source(
        mut self,
        data_type: &str,
        name: &str,
        attributes: HashMap<String, JsonValue>,
    ) -> Self {
        self.context.add_data_source(data_type, name, attributes);
        self
    }

    /// Add a local value
    pub fn local(mut self, name: &str, value: JsonValue) -> Self {
        self.context.add_local(name, value);
        self
    }

    /// Build the context
    pub fn build(self) -> TemplateContext {
        self.context
    }
}

impl Default for TemplateContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Custom Functions
// ============================================================================

/// Get a resource attribute.
///
/// # Arguments
///
/// * `resource_type` - The resource type (e.g., "aws_vpc")
/// * `name` - The resource name (e.g., "main")
/// * `attribute` - The attribute to retrieve (e.g., "id")
///
/// # Returns
///
/// The attribute value, or undefined if not found.
///
/// # Example
///
/// ```jinja2
/// {{ resource("aws_vpc", "main", "id") }}
/// ```
fn resource_fn(
    state: &State<'_, '_>,
    resource_type: String,
    name: String,
    attribute: String,
) -> Result<Value, MiniJinjaError> {
    let ctx = state.lookup("__context").ok_or_else(|| {
        MiniJinjaError::new(ErrorKind::UndefinedError, "Template context not available")
    })?;

    let key = format!("{}.{}", resource_type, name);

    // Navigate through the context structure
    if let Ok(resources) = ctx.get_attr("resources") {
        if let Ok(resource) = resources.get_attr(&key) {
            if let Ok(value) = resource.get_attr(&attribute) {
                if !value.is_undefined() {
                    return Ok(value);
                }
            }
        }
    }

    Ok(Value::UNDEFINED)
}

/// Get a variable value with optional default.
///
/// # Arguments
///
/// * `name` - The variable name
/// * `default` - Optional default value if variable is not set
///
/// # Returns
///
/// The variable value, the default, or undefined.
///
/// # Example
///
/// ```jinja2
/// {{ var("region", "us-east-1") }}
/// {{ var("environment") }}
/// ```
fn var_fn(
    state: &State<'_, '_>,
    name: String,
    default: Option<Value>,
) -> Result<Value, MiniJinjaError> {
    let ctx = state.lookup("__context").ok_or_else(|| {
        MiniJinjaError::new(ErrorKind::UndefinedError, "Template context not available")
    })?;

    if let Ok(variables) = ctx.get_attr("variables") {
        if let Ok(value) = variables.get_attr(&name) {
            if !value.is_undefined() {
                return Ok(value);
            }
        }
    }

    Ok(default.unwrap_or(Value::UNDEFINED))
}

/// Get a data source attribute.
///
/// # Arguments
///
/// * `data_type` - The data source type (e.g., "aws_ami")
/// * `name` - The data source name (e.g., "latest")
/// * `attribute` - The attribute to retrieve (e.g., "id")
///
/// # Returns
///
/// The attribute value, or undefined if not found.
///
/// # Example
///
/// ```jinja2
/// {{ data("aws_ami", "latest", "id") }}
/// ```
fn data_fn(
    state: &State<'_, '_>,
    data_type: String,
    name: String,
    attribute: String,
) -> Result<Value, MiniJinjaError> {
    let ctx = state.lookup("__context").ok_or_else(|| {
        MiniJinjaError::new(ErrorKind::UndefinedError, "Template context not available")
    })?;

    let key = format!("{}.{}", data_type, name);

    if let Ok(data_sources) = ctx.get_attr("data_sources") {
        if let Ok(data) = data_sources.get_attr(&key) {
            if let Ok(value) = data.get_attr(&attribute) {
                if !value.is_undefined() {
                    return Ok(value);
                }
            }
        }
    }

    Ok(Value::UNDEFINED)
}

/// Get a local value.
///
/// # Arguments
///
/// * `name` - The local value name
///
/// # Returns
///
/// The local value, or undefined if not found.
///
/// # Example
///
/// ```jinja2
/// {{ local("common_tags") }}
/// ```
fn local_fn(state: &State<'_, '_>, name: String) -> Result<Value, MiniJinjaError> {
    let ctx = state.lookup("__context").ok_or_else(|| {
        MiniJinjaError::new(ErrorKind::UndefinedError, "Template context not available")
    })?;

    if let Ok(locals) = ctx.get_attr("locals") {
        if let Ok(value) = locals.get_attr(&name) {
            if !value.is_undefined() {
                return Ok(value);
            }
        }
    }

    Ok(Value::UNDEFINED)
}

/// Check if a resource exists.
///
/// # Arguments
///
/// * `resource_type` - The resource type (e.g., "aws_vpc")
/// * `name` - The resource name (e.g., "main")
///
/// # Returns
///
/// `true` if the resource exists, `false` otherwise.
///
/// # Example
///
/// ```jinja2
/// {% if resource_exists("aws_vpc", "main") %}
///   VPC exists!
/// {% endif %}
/// ```
fn resource_exists_fn(
    state: &State<'_, '_>,
    resource_type: String,
    name: String,
) -> Result<bool, MiniJinjaError> {
    let ctx = state.lookup("__context").ok_or_else(|| {
        MiniJinjaError::new(ErrorKind::UndefinedError, "Template context not available")
    })?;

    let key = format!("{}.{}", resource_type, name);

    if let Ok(resources) = ctx.get_attr("resources") {
        if let Ok(resource) = resources.get_attr(&key) {
            return Ok(!resource.is_undefined());
        }
    }

    Ok(false)
}

/// Calculate a subnet CIDR from a base CIDR block.
///
/// This is compatible with Terraform's `cidrsubnet` function.
///
/// # Arguments
///
/// * `cidr` - The base CIDR block (e.g., "10.0.0.0/16")
/// * `newbits` - Number of bits to add to the prefix length
/// * `netnum` - The network number within the new range
///
/// # Returns
///
/// The calculated subnet CIDR string.
///
/// # Example
///
/// ```jinja2
/// {{ cidr_subnet("10.0.0.0/16", 8, 1) }}
/// // Returns "10.0.1.0/24"
///
/// {{ cidr_subnet("10.0.0.0/16", 8, 255) }}
/// // Returns "10.0.255.0/24"
/// ```
fn cidr_subnet_fn(cidr: String, newbits: i32, netnum: i32) -> Result<String, MiniJinjaError> {
    // Parse the CIDR
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return Err(MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("Invalid CIDR format: {}", cidr),
        ));
    }

    let ip: Ipv4Addr = parts[0].parse().map_err(|_| {
        MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("Invalid IP address: {}", parts[0]),
        )
    })?;

    let prefix_len: u8 = parts[1].parse().map_err(|_| {
        MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("Invalid prefix length: {}", parts[1]),
        )
    })?;

    if !(0..=32).contains(&newbits) {
        return Err(MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("Invalid newbits value: {} (must be 0-32)", newbits),
        ));
    }

    let new_prefix_len = prefix_len as i32 + newbits;
    if new_prefix_len > 32 {
        return Err(MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!(
                "New prefix length {} exceeds 32 (original: {}, newbits: {})",
                new_prefix_len, prefix_len, newbits
            ),
        ));
    }

    let max_netnum = 1i64 << newbits;
    if netnum < 0 || (netnum as i64) >= max_netnum {
        return Err(MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!(
                "Network number {} is out of range (0 to {})",
                netnum,
                max_netnum - 1
            ),
        ));
    }

    // Calculate the new IP address
    let ip_u32 = u32::from(ip);

    // Calculate the offset for this subnet
    let host_bits = 32 - new_prefix_len;
    let subnet_offset = (netnum as u32) << host_bits;

    // Apply the network mask to get the base
    let mask = if prefix_len == 0 {
        0
    } else {
        !((1u32 << (32 - prefix_len)) - 1)
    };
    let base = ip_u32 & mask;

    // Calculate the new network address
    let new_ip = base + subnet_offset;
    let new_addr = Ipv4Addr::from(new_ip);

    Ok(format!("{}/{}", new_addr, new_prefix_len))
}

/// Check if a data source exists.
///
/// # Arguments
///
/// * `data_type` - The data source type (e.g., "aws_ami")
/// * `name` - The data source name (e.g., "latest")
///
/// # Returns
///
/// `true` if the data source exists, `false` otherwise.
fn data_exists_fn(
    state: &State<'_, '_>,
    data_type: String,
    name: String,
) -> Result<bool, MiniJinjaError> {
    let ctx = state.lookup("__context").ok_or_else(|| {
        MiniJinjaError::new(ErrorKind::UndefinedError, "Template context not available")
    })?;

    let key = format!("{}.{}", data_type, name);

    if let Ok(data_sources) = ctx.get_attr("data_sources") {
        if let Ok(data) = data_sources.get_attr(&key) {
            return Ok(!data.is_undefined());
        }
    }

    Ok(false)
}

/// Get the CIDR host address at a given index.
///
/// # Arguments
///
/// * `cidr` - The CIDR block (e.g., "10.0.0.0/24")
/// * `hostnum` - The host number within the CIDR range
///
/// # Returns
///
/// The IP address at the given host index.
fn cidr_host_fn(cidr: String, hostnum: i32) -> Result<String, MiniJinjaError> {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return Err(MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("Invalid CIDR format: {}", cidr),
        ));
    }

    let ip: Ipv4Addr = parts[0].parse().map_err(|_| {
        MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("Invalid IP address: {}", parts[0]),
        )
    })?;

    let prefix_len: u8 = parts[1].parse().map_err(|_| {
        MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("Invalid prefix length: {}", parts[1]),
        )
    })?;

    let ip_u32 = u32::from(ip);

    // Calculate the network mask
    let mask = if prefix_len == 0 {
        0
    } else {
        !((1u32 << (32 - prefix_len)) - 1)
    };
    let base = ip_u32 & mask;

    // Calculate max hosts
    let host_bits = 32 - prefix_len;
    let max_hosts = if host_bits >= 32 {
        u32::MAX
    } else {
        (1u32 << host_bits) - 1
    };

    if hostnum < 0 || (hostnum as u32) > max_hosts {
        return Err(MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("Host number {} is out of range for CIDR {}", hostnum, cidr),
        ));
    }

    let new_ip = base + hostnum as u32;
    Ok(Ipv4Addr::from(new_ip).to_string())
}

// ============================================================================
// Custom Filters
// ============================================================================

/// JSON encode a value.
///
/// # Arguments
///
/// * `value` - The value to encode
///
/// # Returns
///
/// The JSON-encoded string.
///
/// # Example
///
/// ```jinja2
/// {{ {"key": "value"} | to_json }}
/// // Returns '{"key":"value"}'
/// ```
fn to_json_filter(value: Value) -> Result<String, MiniJinjaError> {
    // Convert MiniJinja Value to serde_json::Value
    let json_value: JsonValue = serde_json::from_str(&value.to_string()).unwrap_or_else(|_| {
        // If parsing fails, treat it as a string
        JsonValue::String(value.to_string())
    });

    serde_json::to_string(&json_value).map_err(|e| {
        MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("JSON encoding failed: {}", e),
        )
    })
}

/// JSON encode a value with pretty formatting.
///
/// # Arguments
///
/// * `value` - The value to encode
///
/// # Returns
///
/// The JSON-encoded string with indentation.
fn to_json_pretty_filter(value: Value) -> Result<String, MiniJinjaError> {
    let json_value: JsonValue = serde_json::from_str(&value.to_string())
        .unwrap_or_else(|_| JsonValue::String(value.to_string()));

    serde_json::to_string_pretty(&json_value).map_err(|e| {
        MiniJinjaError::new(
            ErrorKind::InvalidOperation,
            format!("JSON encoding failed: {}", e),
        )
    })
}

/// Base64 encode a string.
///
/// # Arguments
///
/// * `value` - The string to encode
///
/// # Returns
///
/// The Base64-encoded string.
///
/// # Example
///
/// ```jinja2
/// {{ "hello world" | base64 }}
/// // Returns "aGVsbG8gd29ybGQ="
/// ```
fn base64_encode_filter(value: String) -> String {
    base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
}

/// Base64 decode a string.
///
/// # Arguments
///
/// * `value` - The Base64-encoded string to decode
///
/// # Returns
///
/// The decoded string, or empty string if decoding fails.
fn base64_decode_filter(value: String) -> String {
    base64::engine::general_purpose::STANDARD
        .decode(&value)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}

/// Join a list with a separator.
///
/// # Arguments
///
/// * `value` - The list to join
/// * `separator` - The separator string (defaults to ",")
///
/// # Returns
///
/// The joined string.
///
/// # Example
///
/// ```jinja2
/// {{ ["a", "b", "c"] | join(", ") }}
/// // Returns "a, b, c"
/// ```
fn join_filter(value: Value, separator: Option<String>) -> Result<String, MiniJinjaError> {
    let sep = separator.unwrap_or_else(|| ",".to_string());

    if let Ok(iter) = value.try_iter() {
        let parts: Vec<String> = iter.map(|item| item.to_string()).collect();
        Ok(parts.join(&sep))
    } else {
        // Single value
        Ok(value.to_string())
    }
}

/// Get keys from an object.
///
/// # Arguments
///
/// * `value` - The object to extract keys from
///
/// # Returns
///
/// A list of keys.
///
/// # Example
///
/// ```jinja2
/// {{ {"a": 1, "b": 2} | keys }}
/// // Returns ["a", "b"]
/// ```
fn keys_filter(value: Value) -> Result<Value, MiniJinjaError> {
    if value.is_undefined() || value.is_none() {
        return Ok(Value::from(Vec::<String>::new()));
    }

    // Try to iterate over the value as a mapping
    let mut keys = Vec::new();
    if let Ok(iter) = value.try_iter() {
        for key in iter {
            keys.push(key);
        }
    }

    Ok(Value::from_iter(keys))
}

/// Get values from an object.
///
/// # Arguments
///
/// * `value` - The object to extract values from
///
/// # Returns
///
/// A list of values.
///
/// # Example
///
/// ```jinja2
/// {{ {"a": 1, "b": 2} | values }}
/// // Returns [1, 2]
/// ```
fn values_filter(value: Value) -> Result<Value, MiniJinjaError> {
    if value.is_undefined() || value.is_none() {
        return Ok(Value::from(Vec::<Value>::new()));
    }

    let mut values = Vec::new();

    // Try to iterate over the value as a mapping and get values
    if let Ok(iter) = value.try_iter() {
        for key in iter {
            if let Ok(v) = value.get_attr(&key.to_string()) {
                values.push(v);
            }
        }
    }

    Ok(Value::from_iter(values))
}

/// Format tags for AWS API.
///
/// Converts a simple key-value map to AWS's tag format.
///
/// # Arguments
///
/// * `tags` - A map of tag keys to values
///
/// # Returns
///
/// A list of objects with "Key" and "Value" fields.
///
/// # Example
///
/// ```jinja2
/// {{ {"Name": "my-resource", "Environment": "prod"} | aws_tags }}
/// // Returns [{"Key": "Name", "Value": "my-resource"}, {"Key": "Environment", "Value": "prod"}]
/// ```
fn aws_tags_filter(value: Value) -> Result<Value, MiniJinjaError> {
    if value.is_undefined() || value.is_none() {
        return Ok(Value::from(Vec::<Value>::new()));
    }

    let mut tags = Vec::new();

    // Iterate over the map
    if let Ok(iter) = value.try_iter() {
        for key in iter {
            if let Ok(v) = value.get_attr(&key.to_string()) {
                let tag = Value::from_iter([("Key".to_string(), key), ("Value".to_string(), v)]);
                tags.push(tag);
            }
        }
    }

    Ok(Value::from_iter(tags))
}

/// Format tags for GCP API (labels format).
///
/// Ensures tag keys are lowercase and valid for GCP.
fn gcp_labels_filter(value: Value) -> Result<Value, MiniJinjaError> {
    if value.is_undefined() || value.is_none() {
        return Ok(Value::from(HashMap::<String, String>::new()));
    }

    let mut labels: HashMap<String, String> = HashMap::new();

    if let Ok(iter) = value.try_iter() {
        for key in iter {
            if let Ok(v) = value.get_attr(&key.to_string()) {
                // GCP labels must be lowercase
                let key_lower = key.to_string().to_lowercase().replace(' ', "_");
                labels.insert(key_lower, v.to_string());
            }
        }
    }

    Ok(Value::from_iter(labels))
}

/// Convert a value to HCL format (HashiCorp Configuration Language).
fn to_hcl_filter(value: Value) -> Result<String, MiniJinjaError> {
    fn value_to_hcl(v: &Value, indent: usize) -> String {
        use minijinja::value::ValueKind;

        let indent_str = "  ".repeat(indent);

        // Check kind first to properly distinguish types
        match v.kind() {
            ValueKind::Undefined => return "null".to_string(),
            ValueKind::None => return "null".to_string(),
            ValueKind::Bool => {
                return if v.is_true() { "true" } else { "false" }.to_string();
            }
            ValueKind::Number => {
                // MiniJinja doesn't expose as_f64, so use string representation
                return v.to_string();
            }
            ValueKind::String => {
                if let Some(s) = v.as_str() {
                    return format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
                }
            }
            ValueKind::Seq => {
                let parts: Vec<String> = v
                    .try_iter()
                    .map(|iter| iter.map(|item| value_to_hcl(&item, indent + 1)).collect())
                    .unwrap_or_default();
                if parts.is_empty() {
                    return "[]".to_string();
                }
                return format!(
                    "[\n{}{}\n{}]",
                    indent_str.clone() + "  ",
                    parts.join(&format!(",\n{}  ", indent_str)),
                    indent_str
                );
            }
            ValueKind::Map => {
                if let Ok(iter) = v.try_iter() {
                    let keys: Vec<_> = iter.collect();
                    if keys.is_empty() {
                        return "{}".to_string();
                    }

                    let mut parts = Vec::new();
                    for key in keys {
                        if let Ok(val) = v.get_attr(&key.to_string()) {
                            parts.push(format!(
                                "{}  {} = {}",
                                indent_str,
                                key,
                                value_to_hcl(&val, indent + 1)
                            ));
                        }
                    }

                    return format!("{{\n{}\n{}}}", parts.join("\n"), indent_str);
                }
            }
            _ => {}
        }

        // Fallback to string representation
        format!("\"{}\"", v)
    }

    Ok(value_to_hcl(&value, 0))
}

/// Flatten a nested list.
fn flatten_filter(value: Value) -> Result<Value, MiniJinjaError> {
    fn flatten_recursive(v: &Value, result: &mut Vec<Value>) {
        if let Ok(iter) = v.try_iter() {
            for item in iter {
                if item.try_iter().is_ok() {
                    flatten_recursive(&item, result);
                } else {
                    result.push(item);
                }
            }
        } else {
            result.push(v.clone());
        }
    }

    let mut result = Vec::new();
    flatten_recursive(&value, &mut result);
    Ok(Value::from_iter(result))
}

/// Create a map from two lists (keys and values).
fn zipmap_filter(keys: Value, values: Value) -> Result<Value, MiniJinjaError> {
    let keys_seq: Vec<Value> = keys
        .try_iter()
        .map(|iter| iter.collect())
        .unwrap_or_default();
    let values_seq: Vec<Value> = values
        .try_iter()
        .map(|iter| iter.collect())
        .unwrap_or_default();

    let mut result: HashMap<String, Value> = HashMap::new();

    for (k, v) in keys_seq.iter().zip(values_seq.iter()) {
        result.insert(k.to_string(), v.clone());
    }

    Ok(Value::from_iter(result))
}

// ============================================================================
// Registration
// ============================================================================

/// Register all infrastructure functions with a MiniJinja environment.
///
/// # Arguments
///
/// * `env` - The MiniJinja environment to register functions with
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use minijinja::Environment;
/// use rustible::provisioning::template_functions::register_infrastructure_functions;
///
/// let mut env = Environment::new();
/// register_infrastructure_functions(&mut env);
/// # Ok(())
/// # }
/// ```
pub fn register_infrastructure_functions(env: &mut Environment<'static>) {
    // Register functions
    env.add_function("resource", resource_fn);
    env.add_function("var", var_fn);
    env.add_function("data", data_fn);
    env.add_function("local", local_fn);
    env.add_function("resource_exists", resource_exists_fn);
    env.add_function("data_exists", data_exists_fn);
    env.add_function("cidr_subnet", cidr_subnet_fn);
    env.add_function("cidrsubnet", cidr_subnet_fn); // Terraform-compatible alias
    env.add_function("cidr_host", cidr_host_fn);
    env.add_function("cidrhost", cidr_host_fn); // Terraform-compatible alias

    // Register filters
    env.add_filter("to_json", to_json_filter);
    env.add_filter("tojson", to_json_filter); // Alias
    env.add_filter("to_json_pretty", to_json_pretty_filter);
    env.add_filter("base64", base64_encode_filter);
    env.add_filter("b64encode", base64_encode_filter); // Ansible-compatible alias
    env.add_filter("base64_decode", base64_decode_filter);
    env.add_filter("b64decode", base64_decode_filter); // Ansible-compatible alias
    env.add_filter("join", join_filter);
    env.add_filter("keys", keys_filter);
    env.add_filter("values", values_filter);
    env.add_filter("aws_tags", aws_tags_filter);
    env.add_filter("gcp_labels", gcp_labels_filter);
    env.add_filter("to_hcl", to_hcl_filter);
    env.add_filter("flatten", flatten_filter);
    env.add_filter("zipmap", zipmap_filter);
}

/// Create a MiniJinja Value from a TemplateContext for use in templates.
///
/// # Arguments
///
/// * `ctx` - The template context
///
/// # Returns
///
/// A MiniJinja Value that can be added to the template variables.
pub fn context_to_value(ctx: &TemplateContext) -> Value {
    let mut context_map: HashMap<String, Value> = HashMap::new();

    // Convert resources
    let mut resources_map: HashMap<String, Value> = HashMap::new();
    for (key, attrs) in &ctx.resources {
        let attrs_value: HashMap<String, Value> = attrs
            .iter()
            .map(|(k, v)| (k.clone(), json_to_minijinja(v)))
            .collect();
        resources_map.insert(key.clone(), Value::from_iter(attrs_value));
    }
    context_map.insert("resources".to_string(), Value::from_iter(resources_map));

    // Convert variables
    let variables_map: HashMap<String, Value> = ctx
        .variables
        .iter()
        .map(|(k, v)| (k.clone(), json_to_minijinja(v)))
        .collect();
    context_map.insert("variables".to_string(), Value::from_iter(variables_map));

    // Convert data sources
    let mut data_sources_map: HashMap<String, Value> = HashMap::new();
    for (key, attrs) in &ctx.data_sources {
        let attrs_value: HashMap<String, Value> = attrs
            .iter()
            .map(|(k, v)| (k.clone(), json_to_minijinja(v)))
            .collect();
        data_sources_map.insert(key.clone(), Value::from_iter(attrs_value));
    }
    context_map.insert(
        "data_sources".to_string(),
        Value::from_iter(data_sources_map),
    );

    // Convert locals
    let locals_map: HashMap<String, Value> = ctx
        .locals
        .iter()
        .map(|(k, v)| (k.clone(), json_to_minijinja(v)))
        .collect();
    context_map.insert("locals".to_string(), Value::from_iter(locals_map));

    Value::from_iter(context_map)
}

/// Convert a serde_json::Value to a MiniJinja Value.
fn json_to_minijinja(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::from(()),
        JsonValue::Bool(b) => Value::from(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(f) = n.as_f64() {
                Value::from(f)
            } else {
                Value::from(n.to_string())
            }
        }
        JsonValue::String(s) => Value::from(s.clone()),
        JsonValue::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(json_to_minijinja).collect();
            Value::from_iter(items)
        }
        JsonValue::Object(obj) => {
            let map: HashMap<String, Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_minijinja(v)))
                .collect();
            Value::from_iter(map)
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test environment with context
    fn create_test_env() -> Environment<'static> {
        let mut env = Environment::new();
        register_infrastructure_functions(&mut env);
        env
    }

    fn create_test_context() -> TemplateContext {
        let mut ctx = TemplateContext::new();

        // Add a VPC resource
        let mut vpc_attrs = HashMap::new();
        vpc_attrs.insert("id".to_string(), JsonValue::String("vpc-12345".to_string()));
        vpc_attrs.insert(
            "cidr_block".to_string(),
            JsonValue::String("10.0.0.0/16".to_string()),
        );
        vpc_attrs.insert(
            "arn".to_string(),
            JsonValue::String("arn:aws:ec2:us-east-1:123456789:vpc/vpc-12345".to_string()),
        );
        ctx.add_resource("aws_vpc", "main", vpc_attrs);

        // Add a subnet resource
        let mut subnet_attrs = HashMap::new();
        subnet_attrs.insert(
            "id".to_string(),
            JsonValue::String("subnet-67890".to_string()),
        );
        subnet_attrs.insert(
            "cidr_block".to_string(),
            JsonValue::String("10.0.1.0/24".to_string()),
        );
        ctx.add_resource("aws_subnet", "public", subnet_attrs);

        // Add variables
        ctx.add_variable("region", JsonValue::String("us-east-1".to_string()));
        ctx.add_variable("environment", JsonValue::String("production".to_string()));
        ctx.add_variable("count", JsonValue::Number(3.into()));

        // Add a data source
        let mut ami_attrs = HashMap::new();
        ami_attrs.insert(
            "id".to_string(),
            JsonValue::String("ami-abc123".to_string()),
        );
        ami_attrs.insert(
            "name".to_string(),
            JsonValue::String("amazon-linux-2".to_string()),
        );
        ctx.add_data_source("aws_ami", "latest", ami_attrs);

        // Add locals
        ctx.add_local(
            "common_tags",
            JsonValue::Object(serde_json::Map::from_iter([
                (
                    "Environment".to_string(),
                    JsonValue::String("production".to_string()),
                ),
                (
                    "ManagedBy".to_string(),
                    JsonValue::String("rustible".to_string()),
                ),
            ])),
        );

        ctx
    }

    // ==================== CIDR Function Tests ====================

    #[test]
    fn test_cidr_subnet_basic() {
        let result = cidr_subnet_fn("10.0.0.0/16".to_string(), 8, 0).unwrap();
        assert_eq!(result, "10.0.0.0/24");
    }

    #[test]
    fn test_cidr_subnet_with_netnum() {
        let result = cidr_subnet_fn("10.0.0.0/16".to_string(), 8, 1).unwrap();
        assert_eq!(result, "10.0.1.0/24");
    }

    #[test]
    fn test_cidr_subnet_netnum_255() {
        let result = cidr_subnet_fn("10.0.0.0/16".to_string(), 8, 255).unwrap();
        assert_eq!(result, "10.0.255.0/24");
    }

    #[test]
    fn test_cidr_subnet_smaller_prefix() {
        let result = cidr_subnet_fn("10.0.0.0/8".to_string(), 8, 1).unwrap();
        assert_eq!(result, "10.1.0.0/16");
    }

    #[test]
    fn test_cidr_subnet_larger_newbits() {
        let result = cidr_subnet_fn("10.0.0.0/16".to_string(), 4, 5).unwrap();
        assert_eq!(result, "10.0.80.0/20");
    }

    #[test]
    fn test_cidr_subnet_invalid_cidr() {
        let result = cidr_subnet_fn("invalid".to_string(), 8, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_cidr_subnet_overflow() {
        // Trying to add 20 bits to /16 would exceed /32
        let result = cidr_subnet_fn("10.0.0.0/16".to_string(), 20, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_cidr_subnet_netnum_out_of_range() {
        // With 8 newbits, max netnum is 255
        let result = cidr_subnet_fn("10.0.0.0/16".to_string(), 8, 256);
        assert!(result.is_err());
    }

    #[test]
    fn test_cidr_host_basic() {
        let result = cidr_host_fn("10.0.0.0/24".to_string(), 1).unwrap();
        assert_eq!(result, "10.0.0.1");
    }

    #[test]
    fn test_cidr_host_last() {
        let result = cidr_host_fn("10.0.0.0/24".to_string(), 254).unwrap();
        assert_eq!(result, "10.0.0.254");
    }

    #[test]
    fn test_cidr_host_out_of_range() {
        let result = cidr_host_fn("10.0.0.0/24".to_string(), 300);
        assert!(result.is_err());
    }

    // ==================== Base64 Filter Tests ====================

    #[test]
    fn test_base64_encode_basic() {
        let result = base64_encode_filter("hello world".to_string());
        assert_eq!(result, "aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn test_base64_encode_empty() {
        let result = base64_encode_filter("".to_string());
        assert_eq!(result, "");
    }

    #[test]
    fn test_base64_encode_special_chars() {
        let result = base64_encode_filter("hello\nworld".to_string());
        assert_eq!(result, "aGVsbG8Kd29ybGQ=");
    }

    #[test]
    fn test_base64_decode_basic() {
        let result = base64_decode_filter("aGVsbG8gd29ybGQ=".to_string());
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_base64_decode_invalid() {
        let result = base64_decode_filter("not valid!!!".to_string());
        assert_eq!(result, "");
    }

    #[test]
    fn test_base64_roundtrip() {
        let original = "The quick brown fox!";
        let encoded = base64_encode_filter(original.to_string());
        let decoded = base64_decode_filter(encoded);
        assert_eq!(decoded, original);
    }

    // ==================== JSON Filter Tests ====================

    #[test]
    fn test_to_json_string() {
        let value = Value::from("hello");
        let result = to_json_filter(value).unwrap();
        assert!(result.contains("hello"));
    }

    #[test]
    fn test_to_json_number() {
        let value = Value::from(42);
        let result = to_json_filter(value).unwrap();
        assert!(result.contains("42"));
    }

    #[test]
    fn test_to_json_object() {
        let value = Value::from_iter([("key".to_string(), Value::from("value"))]);
        let result = to_json_filter(value).unwrap();
        // Result should be valid JSON
        assert!(result.contains("key") || result.contains("value"));
    }

    // ==================== Join Filter Tests ====================

    #[test]
    fn test_join_basic() {
        let value = Value::from_iter(vec!["a", "b", "c"]);
        let result = join_filter(value, Some(", ".to_string())).unwrap();
        assert_eq!(result, "a, b, c");
    }

    #[test]
    fn test_join_default_separator() {
        let value = Value::from_iter(vec!["a", "b", "c"]);
        let result = join_filter(value, None).unwrap();
        assert_eq!(result, "a,b,c");
    }

    #[test]
    fn test_join_single_element() {
        let value = Value::from_iter(vec!["only"]);
        let result = join_filter(value, Some("-".to_string())).unwrap();
        assert_eq!(result, "only");
    }

    #[test]
    fn test_join_empty() {
        let value = Value::from_iter(Vec::<String>::new());
        let result = join_filter(value, Some("-".to_string())).unwrap();
        assert_eq!(result, "");
    }

    // ==================== Keys/Values Filter Tests ====================

    #[test]
    fn test_keys_filter_basic() {
        let value = Value::from_iter([
            ("a".to_string(), Value::from(1)),
            ("b".to_string(), Value::from(2)),
        ]);
        let result = keys_filter(value).unwrap();
        let len = result.len().unwrap();
        assert_eq!(len, 2);
    }

    #[test]
    fn test_keys_filter_empty() {
        let value = Value::from_iter(HashMap::<String, Value>::new());
        let result = keys_filter(value).unwrap();
        let len = result.len().unwrap_or(0);
        assert_eq!(len, 0);
    }

    #[test]
    fn test_values_filter_basic() {
        let value = Value::from_iter([
            ("a".to_string(), Value::from(1)),
            ("b".to_string(), Value::from(2)),
        ]);
        let result = values_filter(value).unwrap();
        let len = result.len().unwrap();
        assert_eq!(len, 2);
    }

    // ==================== AWS Tags Filter Tests ====================

    #[test]
    fn test_aws_tags_basic() {
        let value = Value::from_iter([
            ("Name".to_string(), Value::from("my-resource")),
            ("Environment".to_string(), Value::from("prod")),
        ]);
        let result = aws_tags_filter(value).unwrap();
        let len = result.len().unwrap();
        assert_eq!(len, 2);
    }

    #[test]
    fn test_aws_tags_empty() {
        let value = Value::from_iter(HashMap::<String, Value>::new());
        let result = aws_tags_filter(value).unwrap();
        let len = result.len().unwrap_or(0);
        assert_eq!(len, 0);
    }

    #[test]
    fn test_aws_tags_structure() {
        let value = Value::from_iter([("Name".to_string(), Value::from("test"))]);
        let result = aws_tags_filter(value).unwrap();
        // Should have Key and Value fields in each tag
        if let Ok(first_tag) = result.get_item(&Value::from(0)) {
            assert!(first_tag.get_attr("Key").is_ok());
            assert!(first_tag.get_attr("Value").is_ok());
        }
    }

    // ==================== GCP Labels Filter Tests ====================

    #[test]
    fn test_gcp_labels_lowercase() {
        let value = Value::from_iter([
            ("Name".to_string(), Value::from("my-resource")),
            ("Environment".to_string(), Value::from("prod")),
        ]);
        let result = gcp_labels_filter(value).unwrap();
        // Keys should be lowercase
        assert!(result.get_attr("name").is_ok());
        assert!(result.get_attr("environment").is_ok());
    }

    // ==================== Flatten Filter Tests ====================

    #[test]
    fn test_flatten_nested() {
        let inner = Value::from_iter(vec![Value::from(3), Value::from(4)]);
        let outer = Value::from_iter(vec![Value::from(1), Value::from(2), inner]);
        let result = flatten_filter(outer).unwrap();
        let len = result.len().unwrap();
        assert_eq!(len, 4);
    }

    #[test]
    fn test_flatten_already_flat() {
        let value = Value::from_iter(vec![Value::from(1), Value::from(2), Value::from(3)]);
        let result = flatten_filter(value).unwrap();
        let len = result.len().unwrap();
        assert_eq!(len, 3);
    }

    // ==================== Zipmap Filter Tests ====================

    #[test]
    fn test_zipmap_basic() {
        let keys = Value::from_iter(vec!["a", "b", "c"]);
        let values = Value::from_iter(vec![1, 2, 3]);
        let result = zipmap_filter(keys, values).unwrap();

        assert_eq!(result.get_attr("a").unwrap().to_string(), "1");
        assert_eq!(result.get_attr("b").unwrap().to_string(), "2");
        assert_eq!(result.get_attr("c").unwrap().to_string(), "3");
    }

    #[test]
    fn test_zipmap_uneven_lengths() {
        let keys = Value::from_iter(vec!["a", "b"]);
        let values = Value::from_iter(vec![1, 2, 3, 4]); // More values than keys
        let result = zipmap_filter(keys, values).unwrap();

        // Should only have as many pairs as the shorter list
        assert!(result.get_attr("a").is_ok());
        assert!(result.get_attr("b").is_ok());
    }

    // ==================== HCL Filter Tests ====================

    #[test]
    fn test_to_hcl_string() {
        let value = Value::from("hello");
        let result = to_hcl_filter(value).unwrap();
        assert_eq!(result, "\"hello\"");
    }

    #[test]
    fn test_to_hcl_number() {
        let value = Value::from(42);
        let result = to_hcl_filter(value).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_to_hcl_bool() {
        let value = Value::from(true);
        let result = to_hcl_filter(value).unwrap();
        assert_eq!(result, "true");
    }

    // ==================== Template Context Tests ====================

    #[test]
    fn test_context_builder() {
        let ctx = TemplateContext::builder()
            .variable("region", JsonValue::String("us-east-1".to_string()))
            .local("tags", JsonValue::Object(Default::default()))
            .build();

        assert!(ctx.variables.contains_key("region"));
        assert!(ctx.locals.contains_key("tags"));
    }

    #[test]
    fn test_context_to_value() {
        let ctx = create_test_context();
        let value = context_to_value(&ctx);

        // Should have all sections
        assert!(value.get_attr("resources").is_ok());
        assert!(value.get_attr("variables").is_ok());
        assert!(value.get_attr("data_sources").is_ok());
        assert!(value.get_attr("locals").is_ok());
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_full_template_rendering() {
        let env = create_test_env();
        let ctx = create_test_context();

        // Create template variables including context
        let mut vars: HashMap<String, Value> = HashMap::new();
        vars.insert("__context".to_string(), context_to_value(&ctx));

        // Render a simple template using context directly
        let template = "Region: {{ __context.variables.region }}";
        let tmpl = env.template_from_str(template).unwrap();
        let result = tmpl.render(&vars).unwrap();

        assert!(result.contains("us-east-1"));
    }

    #[test]
    fn test_cidr_in_template() {
        let env = create_test_env();

        let template = "{{ cidr_subnet('10.0.0.0/16', 8, 5) }}";
        let tmpl = env.template_from_str(template).unwrap();
        let result = tmpl.render(HashMap::<String, Value>::new()).unwrap();

        assert_eq!(result, "10.0.5.0/24");
    }

    #[test]
    fn test_base64_in_template() {
        let env = create_test_env();

        let template = "{{ 'hello world' | base64 }}";
        let tmpl = env.template_from_str(template).unwrap();
        let result = tmpl.render(HashMap::<String, Value>::new()).unwrap();

        assert_eq!(result, "aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn test_join_in_template() {
        let env = create_test_env();

        let mut vars: HashMap<String, Value> = HashMap::new();
        vars.insert("items".to_string(), Value::from_iter(vec!["a", "b", "c"]));

        let template = "{{ items | join(', ') }}";
        let tmpl = env.template_from_str(template).unwrap();
        let result = tmpl.render(&vars).unwrap();

        assert_eq!(result, "a, b, c");
    }

    #[test]
    fn test_multiple_cidrs() {
        let env = create_test_env();

        let template = r#"{% for i in range(3) %}{{ cidr_subnet('10.0.0.0/16', 8, i) }}
{% endfor %}"#;
        let tmpl = env.template_from_str(template).unwrap();
        let result = tmpl.render(HashMap::<String, Value>::new()).unwrap();

        assert!(result.contains("10.0.0.0/24"));
        assert!(result.contains("10.0.1.0/24"));
        assert!(result.contains("10.0.2.0/24"));
    }

    // ==================== JSON Conversion Tests ====================

    #[test]
    fn test_json_to_minijinja_null() {
        let result = json_to_minijinja(&JsonValue::Null);
        assert!(result.is_none());
    }

    #[test]
    fn test_json_to_minijinja_bool() {
        let result = json_to_minijinja(&JsonValue::Bool(true));
        assert!(result.is_true());
    }

    #[test]
    fn test_json_to_minijinja_number() {
        let result = json_to_minijinja(&JsonValue::Number(42.into()));
        assert_eq!(result.as_i64(), Some(42));
    }

    #[test]
    fn test_json_to_minijinja_string() {
        let result = json_to_minijinja(&JsonValue::String("test".to_string()));
        assert_eq!(result.as_str(), Some("test"));
    }

    #[test]
    fn test_json_to_minijinja_array() {
        let arr = JsonValue::Array(vec![JsonValue::from(1), JsonValue::from(2)]);
        let result = json_to_minijinja(&arr);
        assert_eq!(result.len().unwrap(), 2);
    }

    #[test]
    fn test_json_to_minijinja_object() {
        let obj = JsonValue::Object(serde_json::Map::from_iter([(
            "key".to_string(),
            JsonValue::String("value".to_string()),
        )]));
        let result = json_to_minijinja(&obj);
        assert!(result.get_attr("key").is_ok());
    }
}
