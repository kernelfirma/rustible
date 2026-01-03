//! Template engine for Rustible (Jinja2-compatible)
//!
//! This module provides a unified templating engine based on MiniJinja that handles:
//! - Template rendering (`{{ var }}` syntax)
//! - Expression evaluation (for `when`/`changed_when`/`failed_when` conditions)
//! - Common Ansible-compatible filters and tests
//!
//! # Performance
//!
//! The engine includes a fast-path check: if a string contains no template syntax
//! (`{{` or `{%`), rendering is bypassed entirely.

use crate::error::{Error, Result};
use indexmap::IndexMap;
use minijinja::value::{Value as MiniJinjaValue, ValueKind};
use minijinja::{Environment, ErrorKind};
use once_cell::sync::Lazy;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::trace;

/// Thread-safe template engine using MiniJinja
///
/// This is the unified template engine for Rustible. All template rendering
/// and condition evaluation should go through this engine to ensure consistent
/// Jinja2 semantics.
pub struct TemplateEngine {
    env: Environment<'static>,
}

impl TemplateEngine {
    /// Create a new template engine with Ansible-compatible filters and tests
    #[must_use]
    pub fn new() -> Self {
        let mut env = Environment::new();

        // Configure environment for Ansible compatibility
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Chainable);

        // Register custom filters
        Self::register_filters(&mut env);

        // Register custom tests
        Self::register_tests(&mut env);

        Self { env }
    }

    /// Register Ansible-compatible filters
    fn register_filters(env: &mut Environment<'static>) {
        // String filters
        env.add_filter("default", filter_default);
        env.add_filter("d", filter_default); // Alias for default
        env.add_filter("lower", filter_lower);
        env.add_filter("upper", filter_upper);
        env.add_filter("capitalize", filter_capitalize);
        env.add_filter("title", filter_title);
        env.add_filter("trim", filter_trim);
        env.add_filter("replace", filter_replace);
        env.add_filter("regex_replace", filter_regex_replace);
        env.add_filter("regex_search", filter_regex_search);
        env.add_filter("split", filter_split);
        env.add_filter("join", filter_join);

        // Type conversion filters
        env.add_filter("int", filter_int);
        env.add_filter("float", filter_float);
        env.add_filter("string", filter_string);
        env.add_filter("bool", filter_bool);
        env.add_filter("list", filter_list);

        // Collection filters
        env.add_filter("first", filter_first);
        env.add_filter("last", filter_last);
        env.add_filter("length", filter_length);
        env.add_filter("count", filter_length); // Alias
        env.add_filter("unique", filter_unique);
        env.add_filter("sort", filter_sort);
        env.add_filter("reverse", filter_reverse);
        env.add_filter("flatten", filter_flatten);

        // Path filters
        env.add_filter("basename", filter_basename);
        env.add_filter("dirname", filter_dirname);
        env.add_filter("expanduser", filter_expanduser);
        env.add_filter("realpath", filter_realpath);

        // Encoding filters
        env.add_filter("b64encode", filter_b64encode);
        env.add_filter("b64decode", filter_b64decode);
        env.add_filter("to_json", filter_to_json);
        env.add_filter("from_json", filter_from_json);
        env.add_filter("to_yaml", filter_to_yaml);

        // Ansible-specific filters
        env.add_filter("mandatory", filter_mandatory);
        env.add_filter("ternary", filter_ternary);
        env.add_filter("combine", filter_combine);
        env.add_filter("dict2items", filter_dict2items);
        env.add_filter("items2dict", filter_items2dict);
        env.add_filter("selectattr", filter_selectattr);
        env.add_filter("rejectattr", filter_rejectattr);
        env.add_filter("map", filter_map_attr);
    }

    /// Register Ansible-compatible tests
    fn register_tests(env: &mut Environment<'static>) {
        env.add_test("defined", test_defined);
        env.add_test("undefined", test_undefined);
        env.add_test("none", test_none);
        env.add_test("truthy", test_truthy);
        env.add_test("falsy", test_falsy);
        env.add_test("boolean", test_boolean);
        env.add_test("integer", test_integer);
        env.add_test("float", test_float);
        env.add_test("number", test_number);
        env.add_test("string", test_string);
        env.add_test("mapping", test_mapping);
        env.add_test("iterable", test_iterable);
        env.add_test("sequence", test_sequence);
        env.add_test("sameas", test_sameas);
        env.add_test("contains", test_contains);
        env.add_test("match", test_match);
        env.add_test("search", test_search);
        env.add_test("startswith", test_startswith);
        env.add_test("endswith", test_endswith);
        env.add_test("file", test_file);
        env.add_test("directory", test_directory);
        env.add_test("link", test_link);
        env.add_test("exists", test_exists);
        env.add_test("abs", test_abs);
        env.add_test("success", test_success);
        env.add_test("failed", test_failed);
        env.add_test("changed", test_changed);
        env.add_test("skipped", test_skipped);
    }

    /// Render a template string with variables from a HashMap
    ///
    /// # Performance
    /// Uses fast-path: if no template syntax is detected, returns the string unchanged.
    ///
    /// # Errors
    /// Returns an error if template parsing or rendering fails.
    pub fn render(
        &self,
        template: &str,
        vars: &HashMap<String, JsonValue>,
    ) -> Result<String> {
        // Fast path: no template syntax
        if !Self::is_template(template) {
            return Ok(template.to_string());
        }

        trace!("Rendering template: {}", template);
        let tmpl = self.env.template_from_str(template)?;
        let result = tmpl.render(vars)?;
        Ok(result)
    }

    /// Render a template string with variables from an IndexMap
    ///
    /// This is the primary method used by the executor.
    ///
    /// # Performance
    /// Uses fast-path: if no template syntax is detected, returns the string unchanged.
    ///
    /// # Errors
    /// Returns an error if template parsing or rendering fails.
    pub fn render_with_indexmap(
        &self,
        template: &str,
        vars: &IndexMap<String, JsonValue>,
    ) -> Result<String> {
        // Fast path: no template syntax
        if !Self::is_template(template) {
            return Ok(template.to_string());
        }

        trace!("Rendering template: {}", template);
        // Convert IndexMap to a context that MiniJinja can use
        let context: HashMap<String, JsonValue> = vars.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let tmpl = self.env.template_from_str(template)?;
        let result = tmpl.render(&context)?;
        Ok(result)
    }

    /// Render a JSON value, templating any strings within it
    ///
    /// Recursively templates all string values in the JSON structure.
    ///
    /// # Errors
    /// Returns an error if any template rendering fails.
    pub fn render_value(
        &self,
        value: &JsonValue,
        vars: &IndexMap<String, JsonValue>,
    ) -> Result<JsonValue> {
        match value {
            // Non-templatable primitives - fast path
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => Ok(value.clone()),

            JsonValue::String(s) => {
                // Fast path: no template syntax
                if !Self::is_template(s) {
                    return Ok(value.clone());
                }

                let templated = self.render_with_indexmap(s, vars)?;

                // Try to parse as JSON if it looks like a structured value
                if templated.starts_with('[') || templated.starts_with('{')
                    || templated == "true" || templated == "false"
                    || templated.parse::<f64>().is_ok()
                {
                    if let Ok(parsed) = serde_json::from_str::<JsonValue>(&templated) {
                        return Ok(parsed);
                    }
                }
                Ok(JsonValue::String(templated))
            }

            JsonValue::Array(arr) => {
                let templated: Result<Vec<_>> = arr
                    .iter()
                    .map(|v| self.render_value(v, vars))
                    .collect();
                Ok(JsonValue::Array(templated?))
            }

            JsonValue::Object(obj) => {
                let mut result = serde_json::Map::new();
                for (k, v) in obj {
                    // Template both keys and values
                    let templated_key = self.render_with_indexmap(k, vars)?;
                    let templated_value = self.render_value(v, vars)?;
                    result.insert(templated_key, templated_value);
                }
                Ok(JsonValue::Object(result))
            }
        }
    }

    /// Evaluate a condition expression
    ///
    /// This evaluates expressions like those used in `when`, `changed_when`, `failed_when`.
    /// The expression should be a valid Jinja2 expression (without the `{{ }}` wrapper).
    ///
    /// # Examples
    /// - `"item is defined"`
    /// - `"ansible_os_family == 'Debian'"`
    /// - `"result.rc == 0"`
    /// - `"not skip_task"`
    ///
    /// # Errors
    /// Returns an error if the expression cannot be parsed or evaluated.
    pub fn evaluate_condition(
        &self,
        expression: &str,
        vars: &IndexMap<String, JsonValue>,
    ) -> Result<bool> {
        let expression = expression.trim();

        // Handle empty expression - always true
        if expression.is_empty() {
            return Ok(true);
        }

        // Handle literal booleans (common case)
        match expression.to_lowercase().as_str() {
            "true" | "yes" => return Ok(true),
            "false" | "no" => return Ok(false),
            _ => {}
        }

        trace!("Evaluating condition: {}", expression);

        // Convert vars to HashMap for MiniJinja
        let context: HashMap<String, JsonValue> = vars.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Compile and evaluate the expression
        let expr = self.env.compile_expression(expression).map_err(|e| {
            Error::template_render(expression, format!("Failed to compile expression: {}", e))
        })?;

        let result = expr.eval(&context).map_err(|e| {
            // Check if it's an undefined variable error - treat as false in non-strict mode
            if matches!(e.kind(), ErrorKind::UndefinedError) {
                trace!("Undefined variable in condition '{}', treating as false", expression);
                return Error::template_render(expression, format!("Undefined variable: {}", e));
            }
            Error::template_render(expression, format!("Failed to evaluate: {}", e))
        })?;

        // Convert MiniJinja value to bool
        Ok(is_truthy_value(&result))
    }

    /// Check if a string contains template syntax
    ///
    /// Returns true if the string contains `{{` or `{%` which indicate
    /// Jinja2 template expressions or statements.
    #[must_use]
    #[inline]
    pub fn is_template(s: &str) -> bool {
        // Optimization: Single-pass scan using memchr (via str::find) to avoid traversing
        // the string twice (once for "{{", once for "{%").
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            if i + 1 < rest.len() {
                let next = rest.as_bytes()[i + 1];
                if next == b'{' || next == b'%' {
                    return true;
                }
            }
            // Advance past the found '{'
            if i + 1 < rest.len() {
                rest = &rest[i + 1..];
            } else {
                break;
            }
        }
        false
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Global shared template engine instance
pub static TEMPLATE_ENGINE: Lazy<Arc<TemplateEngine>> = Lazy::new(|| Arc::new(TemplateEngine::new()));

/// Helper function to check if a MiniJinja value is truthy
/// Uses MiniJinja's built-in Jinja2-compatible truthiness semantics
fn is_truthy_value(value: &MiniJinjaValue) -> bool {
    value.is_true()
}

// ============================================================================
// FILTERS
// ============================================================================

fn filter_default(value: MiniJinjaValue, default: Option<MiniJinjaValue>) -> MiniJinjaValue {
    if value.is_undefined() || value.is_none() {
        default.unwrap_or(MiniJinjaValue::from(""))
    } else {
        value
    }
}

fn filter_lower(value: &str) -> String {
    value.to_lowercase()
}

fn filter_upper(value: &str) -> String {
    value.to_uppercase()
}

fn filter_capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

fn filter_title(value: &str) -> String {
    value.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().chain(chars.map(|c| c.to_lowercase().next().unwrap_or(c))).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn filter_trim(value: &str) -> String {
    value.trim().to_string()
}

fn filter_replace(value: &str, old: &str, new: &str) -> String {
    value.replace(old, new)
}

fn filter_regex_replace(value: &str, pattern: &str, replacement: &str) -> std::result::Result<String, minijinja::Error> {
    let re = regex::Regex::new(pattern).map_err(|e| {
        minijinja::Error::new(ErrorKind::InvalidOperation, format!("Invalid regex: {}", e))
    })?;
    Ok(re.replace_all(value, replacement).to_string())
}

/// Search for a pattern in a string.
///
/// Returns the matched string if found, or an empty string if not found.
/// This is compatible with Ansible's regex_search filter.
fn filter_regex_search(value: &str, pattern: &str) -> MiniJinjaValue {
    match regex::Regex::new(pattern) {
        Ok(re) => {
            if let Some(caps) = re.captures(value) {
                // If there are capture groups, return the first one
                if caps.len() > 1 {
                    if let Some(m) = caps.get(1) {
                        return MiniJinjaValue::from(m.as_str().to_string());
                    }
                }
                // Otherwise return the full match
                if let Some(m) = caps.get(0) {
                    return MiniJinjaValue::from(m.as_str().to_string());
                }
            }
            MiniJinjaValue::from("")
        }
        Err(_) => MiniJinjaValue::from(""),
    }
}

fn filter_split(value: &str, sep: Option<&str>) -> Vec<String> {
    let sep = sep.unwrap_or(" ");
    value.split(sep).map(|s| s.to_string()).collect()
}

fn filter_join(value: Vec<MiniJinjaValue>, sep: Option<&str>) -> String {
    let sep = sep.unwrap_or("");
    value.iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(sep)
}

fn filter_int(value: MiniJinjaValue) -> i64 {
    if let Some(n) = value.as_i64() {
        n
    } else if let Some(s) = value.as_str() {
        // Try parsing as float first, then truncate to int
        s.parse::<f64>().map(|f| f as i64).unwrap_or_else(|_| s.parse().unwrap_or(0))
    } else {
        0
    }
}

fn filter_float(value: MiniJinjaValue) -> f64 {
    if let Some(n) = value.as_i64() {
        n as f64
    } else if let Some(s) = value.as_str() {
        s.parse().unwrap_or(0.0)
    } else {
        0.0
    }
}

fn filter_string(value: MiniJinjaValue) -> String {
    value.to_string()
}

fn filter_bool(value: MiniJinjaValue) -> bool {
    is_truthy_value(&value)
}

fn filter_list(value: MiniJinjaValue) -> Vec<MiniJinjaValue> {
    if matches!(value.kind(), ValueKind::Seq) {
        value.try_iter()
            .map(|iter| iter.collect())
            .unwrap_or_default()
    } else if let Some(s) = value.as_str() {
        s.chars().map(|c| MiniJinjaValue::from(c.to_string())).collect()
    } else {
        vec![value]
    }
}

fn filter_first(value: MiniJinjaValue) -> MiniJinjaValue {
    if matches!(value.kind(), ValueKind::Seq) {
        value.get_item(&MiniJinjaValue::from(0_i64)).unwrap_or(MiniJinjaValue::UNDEFINED)
    } else if let Some(s) = value.as_str() {
        s.chars().next().map(|c| MiniJinjaValue::from(c.to_string())).unwrap_or(MiniJinjaValue::UNDEFINED)
    } else {
        MiniJinjaValue::UNDEFINED
    }
}

fn filter_last(value: MiniJinjaValue) -> MiniJinjaValue {
    if matches!(value.kind(), ValueKind::Seq) {
        let len = value.len().unwrap_or(0);
        if len > 0 {
            value.get_item(&MiniJinjaValue::from((len - 1) as i64)).unwrap_or(MiniJinjaValue::UNDEFINED)
        } else {
            MiniJinjaValue::UNDEFINED
        }
    } else if let Some(s) = value.as_str() {
        s.chars().last().map(|c| MiniJinjaValue::from(c.to_string())).unwrap_or(MiniJinjaValue::UNDEFINED)
    } else {
        MiniJinjaValue::UNDEFINED
    }
}

fn filter_length(value: MiniJinjaValue) -> usize {
    value.len().unwrap_or(0)
}

fn filter_unique(value: Vec<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    let mut seen = std::collections::HashSet::new();
    value.into_iter()
        .filter(|v| {
            let key = v.to_string();
            if seen.contains(&key) {
                false
            } else {
                seen.insert(key);
                true
            }
        })
        .collect()
}

fn filter_sort(value: Vec<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    let mut sorted = value;
    sorted.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
    sorted
}

fn filter_reverse(value: Vec<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    let mut reversed = value;
    reversed.reverse();
    reversed
}

fn filter_flatten(value: Vec<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    let mut result = Vec::new();
    for item in value {
        if matches!(item.kind(), ValueKind::Seq) {
            if let Ok(iter) = item.try_iter() {
                result.extend(iter);
            }
        } else {
            result.push(item);
        }
    }
    result
}

fn filter_basename(value: &str) -> String {
    std::path::Path::new(value)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn filter_dirname(value: &str) -> String {
    std::path::Path::new(value)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string()
}

fn filter_expanduser(value: &str) -> String {
    if value.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&value[2..]).to_string_lossy().to_string();
        }
    }
    value.to_string()
}

fn filter_realpath(value: &str) -> String {
    std::fs::canonicalize(value)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| value.to_string())
}

fn filter_b64encode(value: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
}

fn filter_b64decode(value: &str) -> std::result::Result<String, minijinja::Error> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD.decode(value).map_err(|e| {
        minijinja::Error::new(ErrorKind::InvalidOperation, format!("Invalid base64: {}", e))
    })?;
    String::from_utf8(decoded).map_err(|e| {
        minijinja::Error::new(ErrorKind::InvalidOperation, format!("Invalid UTF-8: {}", e))
    })
}

fn filter_to_json(value: MiniJinjaValue) -> std::result::Result<String, minijinja::Error> {
    serde_json::to_string(&value).map_err(|e| {
        minijinja::Error::new(ErrorKind::InvalidOperation, format!("JSON serialization failed: {}", e))
    })
}

fn filter_from_json(value: &str) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    let json: serde_json::Value = serde_json::from_str(value).map_err(|e| {
        minijinja::Error::new(ErrorKind::InvalidOperation, format!("JSON parse failed: {}", e))
    })?;
    Ok(MiniJinjaValue::from_serialize(&json))
}

fn filter_to_yaml(value: MiniJinjaValue) -> std::result::Result<String, minijinja::Error> {
    serde_yaml::to_string(&value).map_err(|e| {
        minijinja::Error::new(ErrorKind::InvalidOperation, format!("YAML serialization failed: {}", e))
    })
}

fn filter_mandatory(value: MiniJinjaValue, msg: Option<String>) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    if value.is_undefined() || value.is_none() {
        let error_msg = msg.unwrap_or_else(|| "Mandatory variable is not defined".to_string());
        Err(minijinja::Error::new(ErrorKind::InvalidOperation, error_msg))
    } else {
        Ok(value)
    }
}

fn filter_ternary(value: MiniJinjaValue, true_val: MiniJinjaValue, false_val: MiniJinjaValue) -> MiniJinjaValue {
    if is_truthy_value(&value) {
        true_val
    } else {
        false_val
    }
}

fn filter_combine(value: MiniJinjaValue, other: MiniJinjaValue) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    // Simple implementation - combines two objects
    let mut result = serde_json::Map::new();

    if let Ok(iter) = value.try_iter() {
        for key in iter {
            if let Some(k) = key.as_str() {
                if let Ok(v) = value.get_item(&key) {
                    result.insert(k.to_string(), serde_json::to_value(&v).unwrap_or(serde_json::Value::Null));
                }
            }
        }
    }

    if let Ok(iter) = other.try_iter() {
        for key in iter {
            if let Some(k) = key.as_str() {
                if let Ok(v) = other.get_item(&key) {
                    result.insert(k.to_string(), serde_json::to_value(&v).unwrap_or(serde_json::Value::Null));
                }
            }
        }
    }

    Ok(MiniJinjaValue::from_serialize(&result))
}

fn filter_dict2items(value: MiniJinjaValue) -> std::result::Result<Vec<MiniJinjaValue>, minijinja::Error> {
    let mut items = Vec::new();
    if let Ok(iter) = value.try_iter() {
        for key in iter {
            if let Ok(val) = value.get_item(&key) {
                let mut item = serde_json::Map::new();
                item.insert("key".to_string(), serde_json::to_value(&key).unwrap_or(serde_json::Value::Null));
                item.insert("value".to_string(), serde_json::to_value(&val).unwrap_or(serde_json::Value::Null));
                items.push(MiniJinjaValue::from_serialize(&item));
            }
        }
    }
    Ok(items)
}

fn filter_items2dict(value: Vec<MiniJinjaValue>) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    let mut result = serde_json::Map::new();
    for item in value {
        if let (Ok(key), Ok(val)) = (item.get_item(&MiniJinjaValue::from("key")), item.get_item(&MiniJinjaValue::from("value"))) {
            if let Some(k) = key.as_str() {
                result.insert(k.to_string(), serde_json::to_value(&val).unwrap_or(serde_json::Value::Null));
            }
        }
    }
    Ok(MiniJinjaValue::from_serialize(&result))
}

fn filter_selectattr(value: Vec<MiniJinjaValue>, attr: &str, test: Option<&str>, test_value: Option<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    value.into_iter()
        .filter(|item| {
            if let Ok(attr_val) = item.get_item(&MiniJinjaValue::from(attr)) {
                match test.unwrap_or("truthy") {
                    "truthy" => is_truthy_value(&attr_val),
                    "equalto" | "eq" | "==" => test_value.as_ref().map(|v| attr_val.to_string() == v.to_string()).unwrap_or(false),
                    "defined" => !attr_val.is_undefined(),
                    _ => is_truthy_value(&attr_val),
                }
            } else {
                false
            }
        })
        .collect()
}

fn filter_rejectattr(value: Vec<MiniJinjaValue>, attr: &str, test: Option<&str>, test_value: Option<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    value.into_iter()
        .filter(|item| {
            if let Ok(attr_val) = item.get_item(&MiniJinjaValue::from(attr)) {
                match test.unwrap_or("truthy") {
                    "truthy" => !is_truthy_value(&attr_val),
                    "equalto" | "eq" | "==" => test_value.as_ref().map(|v| attr_val.to_string() != v.to_string()).unwrap_or(true),
                    "defined" => attr_val.is_undefined(),
                    _ => !is_truthy_value(&attr_val),
                }
            } else {
                true
            }
        })
        .collect()
}

fn filter_map_attr(value: Vec<MiniJinjaValue>, attr: Option<&str>) -> Vec<MiniJinjaValue> {
    if let Some(attr) = attr {
        value.into_iter()
            .filter_map(|item| item.get_item(&MiniJinjaValue::from(attr)).ok())
            .collect()
    } else {
        value
    }
}

// ============================================================================
// TESTS
// ============================================================================

fn test_defined(value: &MiniJinjaValue) -> bool {
    !value.is_undefined()
}

fn test_undefined(value: &MiniJinjaValue) -> bool {
    value.is_undefined()
}

fn test_none(value: &MiniJinjaValue) -> bool {
    value.is_none()
}

fn test_truthy(value: &MiniJinjaValue) -> bool {
    is_truthy_value(value)
}

fn test_falsy(value: &MiniJinjaValue) -> bool {
    !is_truthy_value(value)
}

fn test_boolean(value: &MiniJinjaValue) -> bool {
    matches!(value.kind(), ValueKind::Bool)
}

fn test_integer(value: &MiniJinjaValue) -> bool {
    value.is_integer()
}

fn test_float(value: &MiniJinjaValue) -> bool {
    value.is_number() && !value.is_integer()
}

fn test_number(value: &MiniJinjaValue) -> bool {
    value.is_number()
}

fn test_string(value: &MiniJinjaValue) -> bool {
    value.as_str().is_some()
}

fn test_mapping(value: &MiniJinjaValue) -> bool {
    matches!(value.kind(), ValueKind::Map)
}

fn test_iterable(value: &MiniJinjaValue) -> bool {
    // Iterable includes sequences, maps, and strings
    matches!(value.kind(), ValueKind::Seq | ValueKind::Map | ValueKind::Iterable)
        || value.as_str().is_some()
}

fn test_sequence(value: &MiniJinjaValue) -> bool {
    matches!(value.kind(), ValueKind::Seq)
}

fn test_sameas(value: &MiniJinjaValue, other: &MiniJinjaValue) -> bool {
    value.to_string() == other.to_string()
}

fn test_contains(haystack: &MiniJinjaValue, needle: &MiniJinjaValue) -> bool {
    if let Some(s) = haystack.as_str() {
        if let Some(n) = needle.as_str() {
            return s.contains(n);
        }
    }
    if matches!(haystack.kind(), ValueKind::Seq) {
        if let Ok(iter) = haystack.try_iter() {
            for item in iter {
                if item.to_string() == needle.to_string() {
                    return true;
                }
            }
        }
    }
    false
}

fn test_match(value: &MiniJinjaValue, pattern: &str) -> bool {
    if let Some(s) = value.as_str() {
        regex::Regex::new(pattern)
            .map(|re| re.is_match(s))
            .unwrap_or(false)
    } else {
        false
    }
}

fn test_search(value: &MiniJinjaValue, pattern: &str) -> bool {
    if let Some(s) = value.as_str() {
        regex::Regex::new(pattern)
            .map(|re| re.find(s).is_some())
            .unwrap_or(false)
    } else {
        false
    }
}

fn test_startswith(value: &MiniJinjaValue, prefix: &str) -> bool {
    value.as_str().map(|s| s.starts_with(prefix)).unwrap_or(false)
}

fn test_endswith(value: &MiniJinjaValue, suffix: &str) -> bool {
    value.as_str().map(|s| s.ends_with(suffix)).unwrap_or(false)
}

fn test_file(value: &MiniJinjaValue) -> bool {
    value.as_str().map(|s| std::path::Path::new(s).is_file()).unwrap_or(false)
}

fn test_directory(value: &MiniJinjaValue) -> bool {
    value.as_str().map(|s| std::path::Path::new(s).is_dir()).unwrap_or(false)
}

fn test_link(value: &MiniJinjaValue) -> bool {
    value.as_str().map(|s| std::path::Path::new(s).is_symlink()).unwrap_or(false)
}

fn test_exists(value: &MiniJinjaValue) -> bool {
    value.as_str().map(|s| std::path::Path::new(s).exists()).unwrap_or(false)
}

fn test_abs(value: &MiniJinjaValue) -> bool {
    value.as_str().map(|s| std::path::Path::new(s).is_absolute()).unwrap_or(false)
}

fn test_success(value: &MiniJinjaValue) -> bool {
    // Check for rc == 0 or failed == false
    if let Ok(rc) = value.get_item(&MiniJinjaValue::from("rc")) {
        if let Some(n) = rc.as_i64() {
            return n == 0;
        }
    }
    if let Ok(failed) = value.get_item(&MiniJinjaValue::from("failed")) {
        if matches!(failed.kind(), ValueKind::Bool) {
            return !failed.is_true();
        }
    }
    true
}

fn test_failed(value: &MiniJinjaValue) -> bool {
    // Check for rc != 0 or failed == true
    if let Ok(failed) = value.get_item(&MiniJinjaValue::from("failed")) {
        if matches!(failed.kind(), ValueKind::Bool) {
            return failed.is_true();
        }
    }
    if let Ok(rc) = value.get_item(&MiniJinjaValue::from("rc")) {
        if let Some(n) = rc.as_i64() {
            return n != 0;
        }
    }
    false
}

fn test_changed(value: &MiniJinjaValue) -> bool {
    if let Ok(changed) = value.get_item(&MiniJinjaValue::from("changed")) {
        if matches!(changed.kind(), ValueKind::Bool) {
            return changed.is_true();
        }
    }
    false
}

fn test_skipped(value: &MiniJinjaValue) -> bool {
    if let Ok(skipped) = value.get_item(&MiniJinjaValue::from("skipped")) {
        if matches!(skipped.kind(), ValueKind::Bool) {
            return skipped.is_true();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_template() {
        assert!(TemplateEngine::is_template("{{ foo }}"));
        assert!(TemplateEngine::is_template("{% if true %}"));
        assert!(!TemplateEngine::is_template("hello world"));
        assert!(!TemplateEngine::is_template("{ not template }"));
    }

    #[test]
    fn test_render_no_template() {
        let engine = TemplateEngine::new();
        let vars = HashMap::new();
        let result = engine.render("hello world", &vars).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_render_simple_variable() {
        let engine = TemplateEngine::new();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), JsonValue::String("Alice".to_string()));
        let result = engine.render("Hello, {{ name }}!", &vars).unwrap();
        assert_eq!(result, "Hello, Alice!");
    }

    #[test]
    fn test_render_with_filter() {
        let engine = TemplateEngine::new();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), JsonValue::String("alice".to_string()));
        let result = engine.render("Hello, {{ name | upper }}!", &vars).unwrap();
        assert_eq!(result, "Hello, ALICE!");
    }

    #[test]
    fn test_render_with_default_filter() {
        let engine = TemplateEngine::new();
        let vars = HashMap::new();
        let result = engine.render("Hello, {{ name | default('World') }}!", &vars).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_evaluate_condition_true() {
        let engine = TemplateEngine::new();
        let vars = IndexMap::new();
        assert!(engine.evaluate_condition("true", &vars).unwrap());
        assert!(engine.evaluate_condition("True", &vars).unwrap());
        assert!(engine.evaluate_condition("yes", &vars).unwrap());
    }

    #[test]
    fn test_evaluate_condition_false() {
        let engine = TemplateEngine::new();
        let vars = IndexMap::new();
        assert!(!engine.evaluate_condition("false", &vars).unwrap());
        assert!(!engine.evaluate_condition("False", &vars).unwrap());
        assert!(!engine.evaluate_condition("no", &vars).unwrap());
    }

    #[test]
    fn test_evaluate_condition_variable() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("enabled".to_string(), JsonValue::Bool(true));
        vars.insert("disabled".to_string(), JsonValue::Bool(false));

        assert!(engine.evaluate_condition("enabled", &vars).unwrap());
        assert!(!engine.evaluate_condition("disabled", &vars).unwrap());
    }

    #[test]
    fn test_evaluate_condition_is_defined() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("existing".to_string(), JsonValue::String("value".to_string()));

        assert!(engine.evaluate_condition("existing is defined", &vars).unwrap());
        assert!(engine.evaluate_condition("nonexistent is undefined", &vars).unwrap());
    }

    #[test]
    fn test_evaluate_condition_comparison() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("os".to_string(), JsonValue::String("Debian".to_string()));
        vars.insert("version".to_string(), JsonValue::Number(serde_json::Number::from(10)));

        assert!(engine.evaluate_condition("os == 'Debian'", &vars).unwrap());
        assert!(!engine.evaluate_condition("os == 'RedHat'", &vars).unwrap());
        assert!(engine.evaluate_condition("version >= 10", &vars).unwrap());
        assert!(engine.evaluate_condition("version > 5", &vars).unwrap());
    }

    #[test]
    fn test_evaluate_condition_and_or() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("a".to_string(), JsonValue::Bool(true));
        vars.insert("b".to_string(), JsonValue::Bool(false));

        assert!(engine.evaluate_condition("a and not b", &vars).unwrap());
        assert!(engine.evaluate_condition("a or b", &vars).unwrap());
        assert!(!engine.evaluate_condition("a and b", &vars).unwrap());
    }

    #[test]
    fn test_render_value_string() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("name".to_string(), JsonValue::String("test".to_string()));

        let value = JsonValue::String("Hello {{ name }}".to_string());
        let result = engine.render_value(&value, &vars).unwrap();
        assert_eq!(result, JsonValue::String("Hello test".to_string()));
    }

    #[test]
    fn test_render_value_nested() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("host".to_string(), JsonValue::String("localhost".to_string()));

        let value = serde_json::json!({
            "server": "{{ host }}",
            "port": 8080
        });
        let result = engine.render_value(&value, &vars).unwrap();
        assert_eq!(result["server"], "localhost");
        assert_eq!(result["port"], 8080);
    }

    #[test]
    fn test_filter_basename() {
        assert_eq!(filter_basename("/path/to/file.txt"), "file.txt");
        assert_eq!(filter_basename("file.txt"), "file.txt");
    }

    #[test]
    fn test_filter_dirname() {
        assert_eq!(filter_dirname("/path/to/file.txt"), "/path/to");
        assert_eq!(filter_dirname("file.txt"), "");
    }

    #[test]
    fn test_filter_b64encode() {
        assert_eq!(filter_b64encode("hello"), "aGVsbG8=");
    }

    #[test]
    fn test_filter_b64decode() {
        assert_eq!(filter_b64decode("aGVsbG8=").unwrap(), "hello");
    }
}
