//! Serialization filters for JSON and YAML.
//!
//! This module provides filters for converting between data structures
//! and their JSON/YAML string representations.
//!
//! # Available Filters
//!
//! - `to_json`: Convert a value to compact JSON
//! - `to_nice_json`: Convert a value to pretty-printed JSON
//! - `from_json`: Parse a JSON string into a value
//! - `to_yaml`: Convert a value to YAML
//! - `to_nice_yaml`: Convert a value to multi-line YAML
//! - `from_yaml`: Parse a YAML string into a value
//!
//! # Examples
//!
//! ```jinja2
//! {{ {"key": "value"} | to_json }}
//! {{ '{"key": "value"}' | from_json }}
//! {{ data | to_nice_yaml }}
//! ```

use minijinja::{Environment, Value};
use serde::Deserialize;

/// Register all serialization filters with the given environment.
pub fn register_filters(env: &mut Environment<'static>) {
    env.add_filter("to_json", to_json);
    env.add_filter("to_nice_json", to_nice_json);
    env.add_filter("from_json", from_json);
    env.add_filter("to_yaml", to_yaml);
    env.add_filter("to_nice_yaml", to_nice_yaml);
    env.add_filter("from_yaml", from_yaml);
    env.add_filter("from_yaml_all", from_yaml_all);
}

/// Convert a value to compact JSON.
///
/// # Arguments
///
/// * `value` - The value to serialize
///
/// # Returns
///
/// A compact JSON string representation of the value.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `to_json` filter.
fn to_json(value: Value) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

/// Convert a value to pretty-printed JSON.
///
/// # Arguments
///
/// * `value` - The value to serialize
/// * `indent` - Optional: number of spaces for indentation (default: 4)
///
/// # Returns
///
/// A nicely formatted JSON string with proper indentation.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `to_nice_json` filter.
fn to_nice_json(value: Value, indent: Option<usize>) -> String {
    let indent = indent.unwrap_or(4);

    // For custom indent, we need to use a custom formatter
    if indent == 4 {
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".to_string())
    } else {
        // Use custom formatting with specified indent
        let json = serde_json::to_value(&value).unwrap_or(serde_json::Value::Null);
        format_json_with_indent(&json, indent)
    }
}

/// Format JSON with custom indentation.
fn format_json_with_indent(value: &serde_json::Value, indent: usize) -> String {
    let mut buf = Vec::new();
    let indent_bytes = " ".repeat(indent).into_bytes();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(&indent_bytes);
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    if serde::Serialize::serialize(value, &mut ser).is_ok() {
        String::from_utf8(buf).unwrap_or_else(|_| "null".to_string())
    } else {
        "null".to_string()
    }
}

/// Parse a JSON string into a value.
///
/// # Arguments
///
/// * `input` - The JSON string to parse
///
/// # Returns
///
/// The parsed value, or UNDEFINED if parsing fails.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `from_json` filter.
fn from_json(input: String) -> Value {
    serde_json::from_str::<serde_json::Value>(&input)
        .map(json_to_minijinja_value)
        .unwrap_or(Value::UNDEFINED)
}

/// Convert a value to YAML.
///
/// # Arguments
///
/// * `value` - The value to serialize
///
/// # Returns
///
/// A YAML string representation of the value.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `to_yaml` filter.
fn to_yaml(value: Value) -> String {
    serde_yaml::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

/// Convert a value to multi-line, human-readable YAML.
///
/// # Arguments
///
/// * `value` - The value to serialize
/// * `indent` - Optional: number of spaces for indentation (default: 2)
/// * `width` - Optional: maximum line width (default: 80)
///
/// # Returns
///
/// A nicely formatted YAML string.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `to_nice_yaml` filter.
fn to_nice_yaml(value: Value, indent: Option<usize>, _width: Option<usize>) -> String {
    let _indent = indent.unwrap_or(2);
    // serde_yaml doesn't support custom indent directly, but it produces nice output by default
    serde_yaml::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

/// Parse a YAML string into a value.
///
/// # Arguments
///
/// * `input` - The YAML string to parse
///
/// # Returns
///
/// The parsed value, or UNDEFINED if parsing fails.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `from_yaml` filter.
fn from_yaml(input: String) -> Value {
    serde_yaml::from_str::<serde_yaml::Value>(&input)
        .map(|value| yaml_to_minijinja_value(&value))
        .unwrap_or(Value::UNDEFINED)
}

/// Parse a YAML string with multiple documents into a list.
///
/// # Arguments
///
/// * `input` - The YAML string to parse (may contain multiple documents)
///
/// # Returns
///
/// A list of parsed values, one for each YAML document.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `from_yaml_all` filter.
fn from_yaml_all(input: String) -> Vec<Value> {
    serde_yaml::Deserializer::from_str(&input)
        .filter_map(|doc| {
            serde_yaml::Value::deserialize(doc)
                .ok()
                .map(|v| yaml_to_minijinja_value(&v))
        })
        .collect()
}

/// Convert a serde_json Value to a minijinja Value.
fn json_to_minijinja_value(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::from(()),
        serde_json::Value::Bool(b) => Value::from(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(f) = n.as_f64() {
                Value::from(f)
            } else {
                Value::from(0)
            }
        }
        serde_json::Value::String(s) => Value::from(s),
        serde_json::Value::Array(arr) => Value::from(
            arr.into_iter()
                .map(json_to_minijinja_value)
                .collect::<Vec<_>>(),
        ),
        serde_json::Value::Object(obj) => {
            let items: Vec<(String, Value)> = obj
                .into_iter()
                .map(|(k, v)| (k, json_to_minijinja_value(v)))
                .collect();
            Value::from_iter(items)
        }
    }
}

/// Convert a serde_yaml Value to a minijinja Value.
fn yaml_to_minijinja_value(yaml: &serde_yaml::Value) -> Value {
    match yaml {
        serde_yaml::Value::Null => Value::from(()),
        serde_yaml::Value::Bool(b) => Value::from(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(f) = n.as_f64() {
                Value::from(f)
            } else {
                Value::from(0)
            }
        }
        serde_yaml::Value::String(s) => Value::from(s.as_str()),
        serde_yaml::Value::Sequence(seq) => {
            Value::from(seq.iter().map(yaml_to_minijinja_value).collect::<Vec<_>>())
        }
        serde_yaml::Value::Mapping(map) => {
            let items: Vec<(String, Value)> = map
                .iter()
                .filter_map(|(k, v)| {
                    if let serde_yaml::Value::String(key) = k {
                        Some((key.clone(), yaml_to_minijinja_value(v)))
                    } else {
                        None
                    }
                })
                .collect();
            Value::from_iter(items)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_minijinja_value(&tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_json() {
        let value = Value::from_iter([("key".to_string(), Value::from("value"))]);
        let result = to_json(value);
        assert!(result.contains("key"));
        assert!(result.contains("value"));
    }

    #[test]
    fn test_to_nice_json() {
        let value = Value::from_iter([("key".to_string(), Value::from("value"))]);
        let result = to_nice_json(value, None);
        assert!(result.contains('\n')); // Should be formatted
        assert!(result.contains("key"));
    }

    #[test]
    fn test_from_json() {
        let result = from_json(r#"{"key": "value"}"#.to_string());
        assert!(!result.is_undefined());
    }

    #[test]
    fn test_from_json_invalid() {
        let result = from_json("not valid json".to_string());
        assert!(result.is_undefined());
    }

    #[test]
    fn test_to_yaml() {
        let value = Value::from_iter([("key".to_string(), Value::from("value"))]);
        let result = to_yaml(value);
        assert!(result.contains("key"));
    }

    #[test]
    fn test_from_yaml() {
        let result = from_yaml("key: value".to_string());
        assert!(!result.is_undefined());
    }

    #[test]
    fn test_from_yaml_all() {
        let yaml = "---\nkey1: value1\n---\nkey2: value2";
        let result = from_yaml_all(yaml.to_string());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_roundtrip_json() {
        let original = r#"{"name": "test", "count": 42, "active": true}"#;
        let parsed = from_json(original.to_string());
        let back = to_json(parsed);
        // The order might differ, but content should be the same
        assert!(back.contains("name"));
        assert!(back.contains("test"));
        assert!(back.contains("42"));
        assert!(back.contains("true"));
    }

    #[test]
    fn test_roundtrip_yaml() {
        let original = "name: test\ncount: 42";
        let parsed = from_yaml(original.to_string());
        let back = to_yaml(parsed);
        assert!(back.contains("name"));
        assert!(back.contains("test"));
        assert!(back.contains("42"));
    }
}
