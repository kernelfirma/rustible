//! Template engine for Rustible (Jinja2-compatible)
//!
//! This module provides a unified templating system using MiniJinja that is used
//! by both the executor and CLI for consistent Jinja2-compatible templating.

use indexmap::IndexMap;
use minijinja::{value::Value as MiniJinjaValue, Environment, Error as MiniJinjaError};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Global template engine instance for performance
static TEMPLATE_ENGINE: OnceLock<TemplateEngine> = OnceLock::new();

/// Get the global template engine instance
pub fn get_engine() -> &'static TemplateEngine {
    TEMPLATE_ENGINE.get_or_init(TemplateEngine::new)
}

/// Error type for template operations
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("Template rendering failed: {0}")]
    RenderError(String),

    #[error("Template parsing failed: {0}")]
    ParseError(String),

    #[error("Variable not found: {0}")]
    VariableNotFound(String),

    #[error("Invalid expression: {0}")]
    InvalidExpression(String),
}

impl From<MiniJinjaError> for TemplateError {
    fn from(e: MiniJinjaError) -> Self {
        TemplateError::RenderError(e.to_string())
    }
}

/// Result type for template operations
pub type TemplateResult<T> = Result<T, TemplateError>;

/// Template engine using minijinja with Ansible-compatible filters and tests
pub struct TemplateEngine {
    env: Environment<'static>,
}

impl TemplateEngine {
    /// Create a new template engine with Ansible-compatible filters and tests
    #[must_use]
    pub fn new() -> Self {
        let mut env = Environment::new();

        // Add Ansible-compatible filters
        env.add_filter("default", filter_default);
        env.add_filter("d", filter_default); // Ansible alias
        env.add_filter("lower", filter_lower);
        env.add_filter("upper", filter_upper);
        env.add_filter("capitalize", filter_capitalize);
        env.add_filter("title", filter_title);
        env.add_filter("trim", filter_trim);
        env.add_filter("replace", filter_replace);
        env.add_filter("regex_replace", filter_regex_replace);
        env.add_filter("split", filter_split);
        env.add_filter("join", filter_join);
        env.add_filter("first", filter_first);
        env.add_filter("last", filter_last);
        env.add_filter("length", filter_length);
        env.add_filter("count", filter_length); // Ansible alias
        env.add_filter("int", filter_int);
        env.add_filter("float", filter_float);
        env.add_filter("string", filter_string);
        env.add_filter("bool", filter_bool);
        env.add_filter("to_json", filter_to_json);
        env.add_filter("to_yaml", filter_to_yaml);
        env.add_filter("from_json", filter_from_json);
        env.add_filter("from_yaml", filter_from_yaml);
        env.add_filter("basename", filter_basename);
        env.add_filter("dirname", filter_dirname);
        env.add_filter("expanduser", filter_expanduser);
        env.add_filter("realpath", filter_realpath);
        env.add_filter("quote", filter_quote);
        env.add_filter("regex_search", filter_regex_search);
        env.add_filter("regex_findall", filter_regex_findall);
        env.add_filter("ternary", filter_ternary);
        env.add_filter("combine", filter_combine);
        env.add_filter("dict2items", filter_dict2items);
        env.add_filter("items2dict", filter_items2dict);
        env.add_filter("unique", filter_unique);
        env.add_filter("sort", filter_sort);
        env.add_filter("reverse", filter_reverse);
        env.add_filter("flatten", filter_flatten);
        env.add_filter("map", filter_map_attr);
        env.add_filter("select", filter_select);
        env.add_filter("selectattr", filter_selectattr);
        env.add_filter("reject", filter_reject);
        env.add_filter("rejectattr", filter_rejectattr);

        // Add Ansible-compatible tests
        env.add_test("defined", test_defined);
        env.add_test("undefined", test_undefined);
        env.add_test("none", test_none);
        env.add_test("string", test_string);
        env.add_test("number", test_number);
        env.add_test("integer", test_integer);
        env.add_test("float", test_float);
        env.add_test("mapping", test_mapping);
        env.add_test("iterable", test_iterable);
        env.add_test("sequence", test_sequence);
        env.add_test("sameas", test_sameas);
        env.add_test("empty", test_empty);
        env.add_test("truthy", test_truthy);
        env.add_test("falsy", test_falsy);
        env.add_test("even", test_even);
        env.add_test("odd", test_odd);
        env.add_test("lower", test_lower);
        env.add_test("upper", test_upper);
        env.add_test("match", test_match);
        env.add_test("search", test_search);
        env.add_test("regex", test_regex);
        env.add_test("in", test_in);
        env.add_test("contains", test_contains);
        env.add_test("startswith", test_startswith);
        env.add_test("endswith", test_endswith);
        env.add_test("file", test_file);
        env.add_test("directory", test_directory);
        env.add_test("link", test_link);
        env.add_test("exists", test_exists);
        env.add_test("abs", test_abs);
        env.add_test("subset", test_subset);
        env.add_test("superset", test_superset);
        env.add_test("version", test_version);
        env.add_test("version_compare", test_version);

        Self { env }
    }

    /// Check if a string contains template syntax
    #[inline]
    #[must_use]
    pub fn is_template(s: &str) -> bool {
        s.contains("{{") || s.contains("{%") || s.contains("{#")
    }

    /// Render a template string with HashMap variables (legacy compatibility)
    pub fn render(
        &self,
        template: &str,
        vars: &HashMap<String, JsonValue>,
    ) -> TemplateResult<String> {
        // Fast path: no template syntax
        if !Self::is_template(template) {
            return Ok(template.to_string());
        }

        let tmpl = self
            .env
            .template_from_str(template)
            .map_err(|e| TemplateError::ParseError(e.to_string()))?;
        let result = tmpl
            .render(vars)
            .map_err(|e| TemplateError::RenderError(e.to_string()))?;
        Ok(result)
    }

    /// Render a template string with IndexMap variables
    pub fn render_with_indexmap(
        &self,
        template: &str,
        vars: &IndexMap<String, JsonValue>,
    ) -> TemplateResult<String> {
        // Fast path: no template syntax
        if !Self::is_template(template) {
            return Ok(template.to_string());
        }

        // Convert IndexMap to HashMap for MiniJinja
        let hash_vars: HashMap<String, JsonValue> = vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        let tmpl = self
            .env
            .template_from_str(template)
            .map_err(|e| TemplateError::ParseError(e.to_string()))?;
        let result = tmpl
            .render(&hash_vars)
            .map_err(|e| TemplateError::RenderError(e.to_string()))?;
        Ok(result)
    }

    /// Render a JSON value recursively, templating all string values
    pub fn render_value(
        &self,
        value: &JsonValue,
        vars: &IndexMap<String, JsonValue>,
    ) -> TemplateResult<JsonValue> {
        match value {
            // Non-templatable primitives - fast path
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => Ok(value.clone()),

            JsonValue::String(s) => {
                // Fast path: no template syntax
                if !Self::is_template(s) {
                    return Ok(value.clone());
                }

                let rendered = self.render_with_indexmap(s, vars)?;

                // Try to parse as JSON if it looks like a value
                if let Ok(parsed) = serde_json::from_str::<JsonValue>(&rendered) {
                    if !matches!(parsed, JsonValue::Object(_)) {
                        return Ok(parsed);
                    }
                }
                Ok(JsonValue::String(rendered))
            }

            JsonValue::Array(arr) => {
                let rendered: Result<Vec<_>, _> = arr
                    .iter()
                    .map(|v| self.render_value(v, vars))
                    .collect();
                Ok(JsonValue::Array(rendered?))
            }

            JsonValue::Object(obj) => {
                let mut result = serde_json::Map::new();
                for (k, v) in obj {
                    let rendered_key = self.render_with_indexmap(k, vars)?;
                    let rendered_value = self.render_value(v, vars)?;
                    result.insert(rendered_key, rendered_value);
                }
                Ok(JsonValue::Object(result))
            }
        }
    }

    /// Evaluate a condition expression (for when/changed_when/failed_when)
    ///
    /// This evaluates a Jinja2 expression and returns a boolean result.
    pub fn evaluate_condition(
        &self,
        condition: &str,
        vars: &IndexMap<String, JsonValue>,
    ) -> TemplateResult<bool> {
        let condition = condition.trim();

        // Handle empty condition
        if condition.is_empty() {
            return Ok(true);
        }

        // Wrap condition in {{ }} if not already a template
        let template = if Self::is_template(condition) {
            condition.to_string()
        } else {
            format!("{{{{ {} }}}}", condition)
        };

        let result = self.render_with_indexmap(&template, vars)?;
        let result = result.trim();

        // Evaluate the result as a boolean
        Ok(matches!(
            result.to_lowercase().as_str(),
            "true" | "yes" | "1" | "on"
        ) || (!result.is_empty()
            && result != "false"
            && result != "no"
            && result != "0"
            && result != "off"
            && result != "none"
            && result != "null"
            && result != "[]"
            && result != "{}"))
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Filter implementations
// ============================================================================

fn filter_default(value: MiniJinjaValue, default: Option<MiniJinjaValue>) -> MiniJinjaValue {
    if value.is_undefined() || value.is_none() {
        default.unwrap_or_else(|| MiniJinjaValue::from(""))
    } else {
        value
    }
}

fn filter_lower(value: String) -> String {
    value.to_lowercase()
}

fn filter_upper(value: String) -> String {
    value.to_uppercase()
}

fn filter_capitalize(value: String) -> String {
    let mut chars = value.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

fn filter_title(value: String) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn filter_trim(value: String) -> String {
    value.trim().to_string()
}

fn filter_replace(value: String, old: String, new: String) -> String {
    value.replace(&old, &new)
}

fn filter_regex_replace(value: String, pattern: String, replacement: String) -> String {
    regex::Regex::new(&pattern)
        .map(|re| re.replace_all(&value, replacement.as_str()).to_string())
        .unwrap_or(value)
}

fn filter_split(value: String, sep: Option<String>) -> Vec<String> {
    let sep = sep.unwrap_or_else(|| " ".to_string());
    value.split(&sep).map(|s| s.to_string()).collect()
}

fn filter_join(value: Vec<MiniJinjaValue>, sep: Option<String>) -> String {
    let sep = sep.unwrap_or_else(|| "".to_string());
    value
        .into_iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(&sep)
}

fn filter_first(value: MiniJinjaValue) -> MiniJinjaValue {
    if let Ok(seq) = value.try_iter() {
        seq.into_iter().next().unwrap_or(MiniJinjaValue::UNDEFINED)
    } else {
        MiniJinjaValue::UNDEFINED
    }
}

fn filter_last(value: MiniJinjaValue) -> MiniJinjaValue {
    if let Ok(seq) = value.try_iter() {
        seq.into_iter().last().unwrap_or(MiniJinjaValue::UNDEFINED)
    } else {
        MiniJinjaValue::UNDEFINED
    }
}

fn filter_length(value: MiniJinjaValue) -> usize {
    value.len().unwrap_or(0)
}

fn filter_int(value: MiniJinjaValue) -> i64 {
    if let Some(n) = value.as_i64() {
        n
    } else if let Some(s) = value.as_str() {
        // Try parsing as float first (handles "3.14" -> 3), then as integer
        s.parse::<f64>()
            .map(|f| f as i64)
            .or_else(|_| s.parse())
            .unwrap_or(0)
    } else {
        // For other numeric types, convert through string
        value.to_string().parse().unwrap_or(0)
    }
}

fn filter_float(value: MiniJinjaValue) -> f64 {
    if let Some(n) = value.as_i64() {
        n as f64
    } else if let Some(s) = value.as_str() {
        s.parse().unwrap_or(0.0)
    } else {
        // For other types, convert through string
        value.to_string().parse().unwrap_or(0.0)
    }
}

fn filter_string(value: MiniJinjaValue) -> String {
    value.to_string()
}

fn filter_bool(value: MiniJinjaValue) -> bool {
    if value.is_true() {
        true
    } else if value.is_none() || value.is_undefined() {
        false
    } else if let Some(s) = value.as_str() {
        matches!(s.to_lowercase().as_str(), "true" | "yes" | "1" | "on")
    } else {
        false
    }
}

fn filter_to_json(value: MiniJinjaValue) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

fn filter_to_yaml(value: MiniJinjaValue) -> String {
    serde_yaml::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

fn filter_from_json(value: String) -> MiniJinjaValue {
    serde_json::from_str(&value).unwrap_or(MiniJinjaValue::UNDEFINED)
}

fn filter_from_yaml(value: String) -> MiniJinjaValue {
    serde_yaml::from_str(&value).unwrap_or(MiniJinjaValue::UNDEFINED)
}

fn filter_basename(value: String) -> String {
    std::path::Path::new(&value)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn filter_dirname(value: String) -> String {
    std::path::Path::new(&value)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string()
}

fn filter_expanduser(value: String) -> String {
    shellexpand::tilde(&value).to_string()
}

fn filter_realpath(value: String) -> String {
    std::fs::canonicalize(&value)
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or(value)
}

fn filter_quote(value: String) -> String {
    shell_words::quote(&value).to_string()
}

fn filter_regex_search(value: String, pattern: String) -> MiniJinjaValue {
    regex::Regex::new(&pattern)
        .ok()
        .and_then(|re| re.find(&value))
        .map(|m| MiniJinjaValue::from(m.as_str()))
        .unwrap_or(MiniJinjaValue::UNDEFINED)
}

fn filter_regex_findall(value: String, pattern: String) -> Vec<String> {
    regex::Regex::new(&pattern)
        .ok()
        .map(|re| re.find_iter(&value).map(|m| m.as_str().to_string()).collect())
        .unwrap_or_default()
}

fn filter_ternary(value: MiniJinjaValue, true_val: MiniJinjaValue, false_val: MiniJinjaValue) -> MiniJinjaValue {
    if value.is_true() {
        true_val
    } else {
        false_val
    }
}

fn filter_combine(value: MiniJinjaValue, other: MiniJinjaValue) -> MiniJinjaValue {
    // Simple implementation - for complex merging would need more work
    if let (Ok(mut map1), Ok(map2)) = (
        serde_json::from_value::<serde_json::Map<String, JsonValue>>(
            serde_json::to_value(&value).unwrap_or(JsonValue::Null),
        ),
        serde_json::from_value::<serde_json::Map<String, JsonValue>>(
            serde_json::to_value(&other).unwrap_or(JsonValue::Null),
        ),
    ) {
        for (k, v) in map2 {
            map1.insert(k, v);
        }
        serde_json::from_value(JsonValue::Object(map1)).unwrap_or(MiniJinjaValue::UNDEFINED)
    } else {
        value
    }
}

fn filter_dict2items(value: MiniJinjaValue) -> Vec<MiniJinjaValue> {
    if let Ok(map) = serde_json::from_value::<serde_json::Map<String, JsonValue>>(
        serde_json::to_value(&value).unwrap_or(JsonValue::Null),
    ) {
        map.into_iter()
            .map(|(k, v)| {
                let mut item = serde_json::Map::new();
                item.insert("key".to_string(), JsonValue::String(k));
                item.insert("value".to_string(), v);
                serde_json::from_value(JsonValue::Object(item)).unwrap_or(MiniJinjaValue::UNDEFINED)
            })
            .collect()
    } else {
        vec![]
    }
}

fn filter_items2dict(value: Vec<MiniJinjaValue>) -> MiniJinjaValue {
    let mut result = serde_json::Map::new();
    for item in value {
        if let Ok(obj) = serde_json::from_value::<serde_json::Map<String, JsonValue>>(
            serde_json::to_value(&item).unwrap_or(JsonValue::Null),
        ) {
            if let (Some(JsonValue::String(key)), Some(val)) = (obj.get("key"), obj.get("value")) {
                result.insert(key.clone(), val.clone());
            }
        }
    }
    serde_json::from_value(JsonValue::Object(result)).unwrap_or(MiniJinjaValue::UNDEFINED)
}

fn filter_unique(value: Vec<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    let mut seen = std::collections::HashSet::new();
    value
        .into_iter()
        .filter(|v| {
            let key = v.to_string();
            seen.insert(key)
        })
        .collect()
}

fn filter_sort(value: Vec<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    let mut v = value;
    v.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
    v
}

fn filter_reverse(value: Vec<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    let mut v = value;
    v.reverse();
    v
}

fn filter_flatten(value: MiniJinjaValue) -> Vec<MiniJinjaValue> {
    fn flatten_recursive(val: MiniJinjaValue, result: &mut Vec<MiniJinjaValue>) {
        if let Ok(iter) = val.try_iter() {
            for item in iter {
                if item.try_iter().is_ok() && !item.as_str().is_some() {
                    flatten_recursive(item, result);
                } else {
                    result.push(item);
                }
            }
        } else {
            result.push(val);
        }
    }
    let mut result = Vec::new();
    flatten_recursive(value, &mut result);
    result
}

fn filter_map_attr(value: Vec<MiniJinjaValue>, attr: String) -> Vec<MiniJinjaValue> {
    value
        .into_iter()
        .filter_map(|v| v.get_attr(&attr).ok())
        .collect()
}

fn filter_select(value: Vec<MiniJinjaValue>, test: String) -> Vec<MiniJinjaValue> {
    value
        .into_iter()
        .filter(|v| match test.as_str() {
            "defined" => !v.is_undefined(),
            "truthy" => v.is_true(),
            _ => true,
        })
        .collect()
}

fn filter_selectattr(value: Vec<MiniJinjaValue>, attr: String, test: Option<String>) -> Vec<MiniJinjaValue> {
    value
        .into_iter()
        .filter(|v| {
            if let Ok(attr_val) = v.get_attr(&attr) {
                match test.as_deref().unwrap_or("truthy") {
                    "defined" => !attr_val.is_undefined(),
                    "truthy" => attr_val.is_true(),
                    _ => true,
                }
            } else {
                false
            }
        })
        .collect()
}

fn filter_reject(value: Vec<MiniJinjaValue>, test: String) -> Vec<MiniJinjaValue> {
    value
        .into_iter()
        .filter(|v| match test.as_str() {
            "defined" => v.is_undefined(),
            "truthy" => !v.is_true(),
            _ => false,
        })
        .collect()
}

fn filter_rejectattr(value: Vec<MiniJinjaValue>, attr: String, test: Option<String>) -> Vec<MiniJinjaValue> {
    value
        .into_iter()
        .filter(|v| {
            if let Ok(attr_val) = v.get_attr(&attr) {
                match test.as_deref().unwrap_or("truthy") {
                    "defined" => attr_val.is_undefined(),
                    "truthy" => !attr_val.is_true(),
                    _ => false,
                }
            } else {
                true
            }
        })
        .collect()
}

// ============================================================================
// Test implementations
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

fn test_string(value: &MiniJinjaValue) -> bool {
    value.as_str().is_some()
}

fn test_number(value: &MiniJinjaValue) -> bool {
    // Check if it's an integer or can be parsed as a number
    value.as_i64().is_some() || value.to_string().parse::<f64>().is_ok()
}

fn test_integer(value: &MiniJinjaValue) -> bool {
    value.as_i64().is_some()
}

fn test_float(value: &MiniJinjaValue) -> bool {
    // Check if it's a number but not an integer
    if value.as_i64().is_some() {
        return false;
    }
    value.to_string().parse::<f64>().is_ok()
}

fn test_mapping(value: &MiniJinjaValue) -> bool {
    value.get_attr("__iter__").is_ok() && value.as_str().is_none() && !value.try_iter().is_ok()
}

fn test_iterable(value: &MiniJinjaValue) -> bool {
    value.try_iter().is_ok()
}

fn test_sequence(value: &MiniJinjaValue) -> bool {
    value.try_iter().is_ok() && value.as_str().is_none()
}

fn test_sameas(value: &MiniJinjaValue, other: &MiniJinjaValue) -> bool {
    value == other
}

fn test_empty(value: &MiniJinjaValue) -> bool {
    if value.is_none() || value.is_undefined() {
        return true;
    }
    if let Some(s) = value.as_str() {
        return s.is_empty();
    }
    value.len().map(|l| l == 0).unwrap_or(false)
}

fn test_truthy(value: &MiniJinjaValue) -> bool {
    value.is_true()
}

fn test_falsy(value: &MiniJinjaValue) -> bool {
    !value.is_true()
}

fn test_even(value: &MiniJinjaValue) -> bool {
    value.as_i64().map(|n| n % 2 == 0).unwrap_or(false)
}

fn test_odd(value: &MiniJinjaValue) -> bool {
    value.as_i64().map(|n| n % 2 != 0).unwrap_or(false)
}

fn test_lower(value: &MiniJinjaValue) -> bool {
    value
        .as_str()
        .map(|s| s.chars().all(|c| !c.is_alphabetic() || c.is_lowercase()))
        .unwrap_or(false)
}

fn test_upper(value: &MiniJinjaValue) -> bool {
    value
        .as_str()
        .map(|s| s.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()))
        .unwrap_or(false)
}

fn test_match(value: &MiniJinjaValue, pattern: &str) -> bool {
    value
        .as_str()
        .and_then(|s| regex::Regex::new(pattern).ok().map(|re| re.is_match(s)))
        .unwrap_or(false)
}

fn test_search(value: &MiniJinjaValue, pattern: &str) -> bool {
    test_match(value, pattern)
}

fn test_regex(value: &MiniJinjaValue, pattern: &str) -> bool {
    test_match(value, pattern)
}

fn test_in(value: &MiniJinjaValue, container: &MiniJinjaValue) -> bool {
    if let Some(s) = container.as_str() {
        value.as_str().map(|v| s.contains(v)).unwrap_or(false)
    } else if let Ok(iter) = container.try_iter() {
        iter.into_iter().any(|item| &item == value)
    } else {
        false
    }
}

fn test_contains(value: &MiniJinjaValue, item: &MiniJinjaValue) -> bool {
    test_in(item, value)
}

fn test_startswith(value: &MiniJinjaValue, prefix: &str) -> bool {
    value.as_str().map(|s| s.starts_with(prefix)).unwrap_or(false)
}

fn test_endswith(value: &MiniJinjaValue, suffix: &str) -> bool {
    value.as_str().map(|s| s.ends_with(suffix)).unwrap_or(false)
}

fn test_file(value: &MiniJinjaValue) -> bool {
    value
        .as_str()
        .map(|s| std::path::Path::new(s).is_file())
        .unwrap_or(false)
}

fn test_directory(value: &MiniJinjaValue) -> bool {
    value
        .as_str()
        .map(|s| std::path::Path::new(s).is_dir())
        .unwrap_or(false)
}

fn test_link(value: &MiniJinjaValue) -> bool {
    value
        .as_str()
        .map(|s| std::path::Path::new(s).is_symlink())
        .unwrap_or(false)
}

fn test_exists(value: &MiniJinjaValue) -> bool {
    value
        .as_str()
        .map(|s| std::path::Path::new(s).exists())
        .unwrap_or(false)
}

fn test_abs(value: &MiniJinjaValue) -> bool {
    value
        .as_str()
        .map(|s| std::path::Path::new(s).is_absolute())
        .unwrap_or(false)
}

fn test_subset(value: &MiniJinjaValue, superset: &MiniJinjaValue) -> bool {
    if let (Ok(v_iter), Ok(s_iter)) = (value.try_iter(), superset.try_iter()) {
        let superset_set: std::collections::HashSet<_> = s_iter.into_iter().map(|v| v.to_string()).collect();
        v_iter.into_iter().all(|v| superset_set.contains(&v.to_string()))
    } else {
        false
    }
}

fn test_superset(value: &MiniJinjaValue, subset: &MiniJinjaValue) -> bool {
    test_subset(subset, value)
}

fn test_version(value: &MiniJinjaValue, version: &str, op: Option<&str>) -> bool {
    let v1 = value.as_str().unwrap_or("");
    let op = op.unwrap_or("==");

    let cmp = compare_versions(v1, version);

    match op {
        "==" | "eq" => cmp == std::cmp::Ordering::Equal,
        "!=" | "ne" => cmp != std::cmp::Ordering::Equal,
        "<" | "lt" => cmp == std::cmp::Ordering::Less,
        "<=" | "le" => cmp != std::cmp::Ordering::Greater,
        ">" | "gt" => cmp == std::cmp::Ordering::Greater,
        ">=" | "ge" => cmp != std::cmp::Ordering::Less,
        _ => false,
    }
}

/// Compare version strings (e.g., "1.2.3" vs "1.3.0")
fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    let parse_parts = |v: &str| -> Vec<i64> {
        v.split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<i64>().ok())
            .collect()
    };

    let p1 = parse_parts(v1);
    let p2 = parse_parts(v2);

    for i in 0..std::cmp::max(p1.len(), p2.len()) {
        let n1 = p1.get(i).copied().unwrap_or(0);
        let n2 = p2.get(i).copied().unwrap_or(0);
        match n1.cmp(&n2) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

// ============================================================================
// YAML support - for CLI and other YAML-based code
// ============================================================================

impl TemplateEngine {
    /// Render a template string with YAML variables
    pub fn render_with_yaml(
        &self,
        template: &str,
        vars: &IndexMap<String, serde_yaml::Value>,
    ) -> TemplateResult<String> {
        // Fast path: no template syntax
        if !Self::is_template(template) {
            return Ok(template.to_string());
        }

        // Convert YAML values to JSON for MiniJinja compatibility
        let json_vars: HashMap<String, JsonValue> = vars
            .iter()
            .map(|(k, v)| (k.clone(), yaml_to_json(v)))
            .collect();

        let tmpl = self
            .env
            .template_from_str(template)
            .map_err(|e| TemplateError::ParseError(e.to_string()))?;
        let result = tmpl
            .render(&json_vars)
            .map_err(|e| TemplateError::RenderError(e.to_string()))?;
        Ok(result)
    }
}

/// Convert a YAML value to a JSON value
fn yaml_to_json(value: &serde_yaml::Value) -> JsonValue {
    match value {
        serde_yaml::Value::Null => JsonValue::Null,
        serde_yaml::Value::Bool(b) => JsonValue::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                JsonValue::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(JsonValue::Number)
                    .unwrap_or(JsonValue::Null)
            } else {
                JsonValue::Null
            }
        }
        serde_yaml::Value::String(s) => JsonValue::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            JsonValue::Array(seq.iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, JsonValue> = map
                .iter()
                .filter_map(|(k, v)| {
                    k.as_str().map(|key| (key.to_string(), yaml_to_json(v)))
                })
                .collect();
            JsonValue::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

// ============================================================================
// Legacy compatibility - kept for existing code that uses crate::error::Result
// ============================================================================

impl TemplateEngine {
    /// Render a template string (legacy compatibility)
    ///
    /// # Errors
    ///
    /// Returns an error if template parsing or rendering fails.
    pub fn render_legacy(
        &self,
        template: &str,
        vars: &HashMap<String, serde_json::Value>,
    ) -> crate::error::Result<String> {
        self.render(template, vars).map_err(|e| crate::error::Error::TemplateRender {
            template: template.to_string(),
            message: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_template() {
        assert!(TemplateEngine::is_template("{{ foo }}"));
        assert!(TemplateEngine::is_template("{% if x %}y{% endif %}"));
        assert!(TemplateEngine::is_template("{# comment #}"));
        assert!(!TemplateEngine::is_template("plain text"));
        assert!(!TemplateEngine::is_template("no templates here"));
    }

    #[test]
    fn test_simple_render() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("name".to_string(), JsonValue::String("World".to_string()));

        let result = engine.render_with_indexmap("Hello, {{ name }}!", &vars).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_fast_path_no_template() {
        let engine = TemplateEngine::new();
        let vars = IndexMap::new();

        let result = engine.render_with_indexmap("plain text", &vars).unwrap();
        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_default_filter() {
        let engine = TemplateEngine::new();
        let vars = IndexMap::new();

        let result = engine
            .render_with_indexmap("{{ undefined_var | default('fallback') }}", &vars)
            .unwrap();
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_condition_evaluation() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("enabled".to_string(), JsonValue::Bool(true));
        vars.insert("count".to_string(), JsonValue::Number(5.into()));

        assert!(engine.evaluate_condition("enabled", &vars).unwrap());
        assert!(engine.evaluate_condition("count > 3", &vars).unwrap());
        assert!(!engine.evaluate_condition("count < 3", &vars).unwrap());
    }

    #[test]
    fn test_render_value() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("name".to_string(), JsonValue::String("test".to_string()));

        let value = serde_json::json!({
            "greeting": "Hello, {{ name }}",
            "static": "no template",
            "number": 42
        });

        let result = engine.render_value(&value, &vars).unwrap();
        assert_eq!(result["greeting"], "Hello, test");
        assert_eq!(result["static"], "no template");
        assert_eq!(result["number"], 42);
    }
}
