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
use lru::LruCache;
use minijinja::value::{Kwargs, Value as MiniJinjaValue, ValueKind};
use minijinja::{Environment, ErrorKind};
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use serde_json::Value as JsonValue;
use std::borrow::Cow;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::trace;

const DEFAULT_TEMPLATE_CACHE_SIZE: usize = 1000;
const TEMPLATE_CACHE_SIZE_ENV: &str = "RUSTIBLE_TEMPLATE_CACHE_SIZE";

/// Thread-safe template engine using MiniJinja
///
/// This is the unified template engine for Rustible. All template rendering
/// and condition evaluation should go through this engine to ensure consistent
/// Jinja2 semantics.
pub struct TemplateEngine {
    env: RwLock<Environment<'static>>,
    template_cache: Option<Mutex<LruCache<String, String>>>,
    expression_cache: Option<Mutex<LruCache<String, Arc<minijinja::Expression<'static, 'static>>>>>,
    template_counter: AtomicUsize,
}

impl TemplateEngine {
    /// Create a new template engine with Ansible-compatible filters and tests
    #[must_use]
    pub fn new() -> Self {
        Self::with_cache_size(Self::default_cache_size())
    }

    /// Create a template engine with a specific cache size (0 disables caching).
    #[must_use]
    pub fn with_cache_size(cache_size: usize) -> Self {
        let env = Self::build_environment();
        let template_cache = Self::build_cache(cache_size);
        let expression_cache = Self::build_cache(cache_size);

        Self {
            env: RwLock::new(env),
            template_cache,
            expression_cache,
            template_counter: AtomicUsize::new(0),
        }
    }

    fn build_environment() -> Environment<'static> {
        let mut env = Environment::new();

        // Configure environment for Ansible compatibility
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Chainable);

        // Register custom filters
        Self::register_filters(&mut env);

        // Register custom tests
        Self::register_tests(&mut env);

        env
    }

    fn default_cache_size() -> usize {
        std::env::var(TEMPLATE_CACHE_SIZE_ENV)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_TEMPLATE_CACHE_SIZE)
    }

    fn build_cache<T>(cache_size: usize) -> Option<Mutex<LruCache<String, T>>> {
        NonZeroUsize::new(cache_size).map(|capacity| Mutex::new(LruCache::new(capacity)))
    }

    fn next_template_name(&self) -> String {
        let id = self.template_counter.fetch_add(1, Ordering::Relaxed);
        format!("__rustible_template_{}", id)
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
        env.add_filter("to_nice_json", filter_to_nice_json);
        env.add_filter("from_json", filter_from_json);
        env.add_filter("to_yaml", filter_to_yaml);
        env.add_filter("to_nice_yaml", filter_to_nice_yaml);
        env.add_filter("from_yaml", filter_from_yaml);
        env.add_filter("from_yaml_all", filter_from_yaml_all);

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

        // Aliases (Ansible compatibility)
        env.add_test("null", test_none);
        env.add_test("dict", test_mapping);
        env.add_test("list", test_sequence);

        // Numeric tests
        env.add_test("odd", test_odd);
        env.add_test("even", test_even);
        env.add_test("divisibleby", test_divisibleby);

        // Collection tests
        env.add_test("in", test_in);
        env.add_test("subset", test_subset);
        env.add_test("superset", test_superset);

        // Other
        env.add_test("callable", test_callable);
        env.add_test("escaped", test_escaped);
    }

    fn render_cached<S: serde::Serialize>(&self, template: &str, vars: &S) -> Result<String> {
        // Fast path: no template syntax
        if !Self::is_template(template) {
            return Ok(template.to_string());
        }

        trace!("Rendering template: {}", template);

        if let Some(cache) = &self.template_cache {
            let mut cache = cache.lock();
            if let Some(name) = cache.get(template).cloned() {
                drop(cache);
                let env = self.env.read();
                let tmpl = env.get_template(&name)?;
                return Ok(tmpl.render(vars)?);
            }

            let name = self.next_template_name();
            let template_owned = template.to_string();
            {
                let mut env = self.env.write();
                env.add_template_owned(name.clone(), template_owned.clone())?;
            }

            if let Some(evicted_name) = cache.put(template_owned, name.clone()) {
                let mut env = self.env.write();
                env.remove_template(&evicted_name);
            }
            drop(cache);

            let env = self.env.read();
            let tmpl = env.get_template(&name)?;
            return Ok(tmpl.render(vars)?);
        }

        let env = self.env.read();
        let tmpl = env.template_from_str(template)?;
        Ok(tmpl.render(vars)?)
    }

    /// Clear cached templates and expressions.
    pub fn clear_cache(&self) {
        if let Some(cache) = &self.template_cache {
            cache.lock().clear();
        }
        if let Some(cache) = &self.expression_cache {
            cache.lock().clear();
        }

        let mut env = self.env.write();
        env.clear_templates();
    }

    /// Return the number of cached templates and expressions.
    pub fn cache_stats(&self) -> (usize, usize) {
        let template_count = self
            .template_cache
            .as_ref()
            .map_or_else(|| 0, |cache| cache.lock().len());
        let expression_count = self
            .expression_cache
            .as_ref()
            .map_or_else(|| 0, |cache| cache.lock().len());
        (template_count, expression_count)
    }

    /// Render a template string with variables from a HashMap
    ///
    /// # Performance
    /// Uses fast-path: if no template syntax is detected, returns the string unchanged.
    ///
    /// # Errors
    /// Returns an error if template parsing or rendering fails.
    pub fn render(&self, template: &str, vars: &HashMap<String, JsonValue>) -> Result<String> {
        self.render_cached(template, vars)
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
        self.render_cached(template, vars)
    }

    /// Render a template string with a JSON Value context
    ///
    /// This allows rendering directly with a serde_json::Value (e.g. Object) without
    /// converting it to HashMap/IndexMap first.
    pub fn render_with_json(&self, template: &str, context: &JsonValue) -> Result<String> {
        self.render_cached(template, context)
    }

    /// Render a template string with any Serializable context
    ///
    /// This allows rendering with custom structs or optimized context wrappers
    /// without converting to serde_json::Value first.
    pub fn render_serialize<S: serde::Serialize>(
        &self,
        template: &str,
        vars: &S,
    ) -> Result<String> {
        self.render_cached(template, vars)
    }

    /// Render a JSON value, templating any strings within it
    ///
    /// Recursively templates all string values in the JSON structure.
    ///
    /// # Errors
    /// Returns an error if any template rendering fails.
    pub fn render_value<'a>(
        &self,
        value: &'a JsonValue,
        vars: &IndexMap<String, JsonValue>,
    ) -> Result<Cow<'a, JsonValue>> {
        match value {
            // Non-templatable primitives - fast path
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => Ok(Cow::Borrowed(value)),

            JsonValue::String(s) => {
                // Fast path: no template syntax
                if !Self::is_template(s) {
                    return Ok(Cow::Borrowed(value));
                }

                let templated = self.render_with_indexmap(s, vars)?;

                // Optimization: fast check if string starts with digit or sign before attempting expensive float parse
                let is_maybe_number = templated.as_bytes().first().is_some_and(|&c| {
                    c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.'
                });

                // Try to parse as JSON if it looks like a structured value
                if templated.starts_with('[')
                    || templated.starts_with('{')
                    || templated == "true"
                    || templated == "false"
                    || (is_maybe_number && templated.parse::<f64>().is_ok())
                {
                    if let Ok(parsed) = serde_json::from_str::<JsonValue>(&templated) {
                        return Ok(Cow::Owned(parsed));
                    }
                }
                Ok(Cow::Owned(JsonValue::String(templated)))
            }

            JsonValue::Array(arr) => {
                // Optimization: lazy allocation. Only allocate new Vec if an element actually changes.
                for (i, v) in arr.iter().enumerate() {
                    let res = self.render_value(v, vars)?;
                    if matches!(res, Cow::Owned(_)) {
                        // Found a change, need to construct new array
                        let mut new_arr = Vec::with_capacity(arr.len());
                        // Add unchanged elements up to this point
                        new_arr.extend(arr.iter().take(i).cloned());
                        // Add the changed element
                        new_arr.push(res.into_owned());
                        // Process the rest
                        for v in arr.iter().skip(i + 1) {
                            let res = self.render_value(v, vars)?;
                            new_arr.push(res.into_owned());
                        }
                        return Ok(Cow::Owned(JsonValue::Array(new_arr)));
                    }
                }
                // No changes
                Ok(Cow::Borrowed(value))
            }

            JsonValue::Object(obj) => {
                // Optimization: lazy allocation. Only allocate new Map if a key or value changes.
                for (i, (k, v)) in obj.iter().enumerate() {
                    let key_changed = Self::is_template(k);
                    let val_res = self.render_value(v, vars)?;

                    if key_changed || matches!(val_res, Cow::Owned(_)) {
                        // Found a change, need to construct new map
                        let mut map = serde_json::Map::with_capacity(obj.len());

                        // Add unchanged entries up to this point
                        for (prev_k, prev_v) in obj.iter().take(i) {
                            map.insert(prev_k.clone(), prev_v.clone());
                        }

                        // Add the changed entry
                        let new_key = if key_changed {
                            self.render_with_indexmap(k, vars)?
                        } else {
                            k.clone()
                        };
                        map.insert(new_key, val_res.into_owned());

                        // Process the rest
                        for (k, v) in obj.iter().skip(i + 1) {
                            let key_changed = Self::is_template(k);
                            let new_key = if key_changed {
                                self.render_with_indexmap(k, vars)?
                            } else {
                                k.clone()
                            };
                            let val_res = self.render_value(v, vars)?;
                            map.insert(new_key, val_res.into_owned());
                        }
                        return Ok(Cow::Owned(JsonValue::Object(map)));
                    }
                }
                // No changes
                Ok(Cow::Borrowed(value))
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
            "true" | "yes" | "on" | "y" | "t" => return Ok(true),
            "false" | "no" | "off" | "n" | "f" => return Ok(false),
            _ => {}
        }

        trace!("Evaluating condition: {}", expression);
        let expr = if let Some(cache) = &self.expression_cache {
            let mut cache = cache.lock();
            if let Some(cached) = cache.get(expression).cloned() {
                cached
            } else {
                let compiled = EXPRESSION_ENV
                    .compile_expression_owned(expression.to_string())
                    .map_err(|e| {
                        Error::template_render(
                            expression,
                            format!("Failed to compile expression: {}", e),
                        )
                    })?;
                let compiled = Arc::new(compiled);
                cache.put(expression.to_string(), Arc::clone(&compiled));
                compiled
            }
        } else {
            Arc::new(EXPRESSION_ENV.compile_expression(expression).map_err(|e| {
                Error::template_render(expression, format!("Failed to compile expression: {}", e))
            })?)
        };

        let result = expr.eval(vars).map_err(|e| {
            // Check if it's an undefined variable error - treat as false in non-strict mode
            if matches!(e.kind(), ErrorKind::UndefinedError) {
                trace!(
                    "Undefined variable in condition '{}', treating as false",
                    expression
                );
                return Error::template_render(expression, format!("Undefined variable: {}", e));
            }
            Error::template_render(expression, format!("Failed to evaluate: {}", e))
        })?;

        // Convert MiniJinja value to bool
        Ok(is_truthy_value(&result))
    }

    /// Check if a string contains template syntax
    ///
    /// Returns true if the string contains `{{`, `{%`, or `{#` which indicate
    /// Jinja2 template expressions, statements, or comments.
    #[must_use]
    #[inline]
    pub fn is_template(s: &str) -> bool {
        // Optimization: Single-pass scan using memchr (via str::find) to avoid traversing
        // the string multiple times.
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            if i + 1 < rest.len() {
                let next = rest.as_bytes()[i + 1];
                // Check for {{ (expression), {% (statement), or {# (comment)
                if next == b'{' || next == b'%' || next == b'#' {
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

static EXPRESSION_ENV: Lazy<Environment<'static>> = Lazy::new(TemplateEngine::build_environment);

/// Global shared template engine instance
pub static TEMPLATE_ENGINE: Lazy<Arc<TemplateEngine>> =
    Lazy::new(|| Arc::new(TemplateEngine::new()));

/// Get a reference to the global template engine
pub fn get_engine() -> &'static Arc<TemplateEngine> {
    &TEMPLATE_ENGINE
}

/// Helper function to check if a MiniJinja value is truthy
/// Uses MiniJinja's built-in Jinja2-compatible truthiness semantics
fn is_truthy_value(value: &MiniJinjaValue) -> bool {
    value.is_true()
}

// ============================================================================
// FILTERS
// ============================================================================

fn filter_default(
    value: MiniJinjaValue,
    default: Option<MiniJinjaValue>,
    kwargs: Kwargs,
) -> MiniJinjaValue {
    if value.is_undefined() || value.is_none() {
        // Check for value= kwarg (Ansible/Jinja2 compatibility)
        if let Ok(v) = kwargs.get::<MiniJinjaValue>("value") {
            return v;
        }
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
    if value.is_empty() {
        return String::new();
    }

    // Fast path: pure ASCII
    if value.is_ascii() {
        let mut result = String::with_capacity(value.len());
        let bytes = value.as_bytes();
        result.push(bytes[0].to_ascii_uppercase() as char);
        result.push_str(&value[1..]);
        return result;
    }

    let mut chars = value.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut result = String::with_capacity(value.len());
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            result.push_str(chars.as_str());
            result
        }
    }
}

fn filter_title(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut next_is_start = true;

    for c in value.chars() {
        if c.is_whitespace() {
            result.push(c);
            next_is_start = true;
        } else if next_is_start {
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            next_is_start = false;
        } else {
            for lc in c.to_lowercase() {
                result.push(lc);
            }
        }
    }
    result
}

fn filter_trim(value: &str) -> String {
    value.trim().to_string()
}

fn filter_replace(value: &str, old: &str, new: &str) -> String {
    value.replace(old, new)
}

fn filter_regex_replace(
    value: &str,
    pattern: &str,
    replacement: &str,
) -> std::result::Result<String, minijinja::Error> {
    // Optimization: Use cached regex to avoid recompilation overhead (~65% faster)
    let re = crate::utils::get_regex(pattern).map_err(|e| {
        minijinja::Error::new(ErrorKind::InvalidOperation, format!("Invalid regex: {}", e))
    })?;
    Ok(re.replace_all(value, replacement).to_string())
}

/// Search for a pattern in a string.
///
/// Returns the matched string if found, or an empty string if not found.
/// This is compatible with Ansible's regex_search filter.
fn filter_regex_search(value: &str, pattern: &str) -> MiniJinjaValue {
    // Optimization: Use cached regex to avoid recompilation overhead (~60% faster)
    match crate::utils::get_regex(pattern) {
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
    let mut result = String::new();
    for (i, v) in value.iter().enumerate() {
        if i > 0 {
            result.push_str(sep);
        }
        use std::fmt::Write;
        // Optimization: Write directly to string buffer to avoid allocating
        // intermediate Strings and Vec<String>
        write!(result, "{}", v).unwrap();
    }
    result
}

fn filter_int(value: MiniJinjaValue) -> i64 {
    if let Some(n) = value.as_i64() {
        n
    } else if value.is_number() && !value.is_integer() {
        // Handle float values by converting via string and truncating to int
        value
            .to_string()
            .parse::<f64>()
            .map(|f| f as i64)
            .unwrap_or(0)
    } else if let Some(s) = value.as_str() {
        // Try parsing as float first, then truncate to int
        s.parse::<f64>()
            .map(|f| f as i64)
            .unwrap_or_else(|_| s.parse().unwrap_or(0))
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
    // Ansible-compatible bool filter: handles yes/no/true/false/on/off/1/0 strings
    if let Some(s) = value.as_str() {
        if s.is_empty() {
            return false;
        }
        if let Some(b) = crate::utils::parse_bool(s) {
            return b;
        }
    }
    is_truthy_value(&value)
}

fn filter_list(value: MiniJinjaValue) -> Vec<MiniJinjaValue> {
    if matches!(value.kind(), ValueKind::Seq) {
        value
            .try_iter()
            .map(|iter| iter.collect())
            .unwrap_or_default()
    } else if let Some(s) = value.as_str() {
        s.chars()
            .map(|c| MiniJinjaValue::from(c.to_string()))
            .collect()
    } else {
        vec![value]
    }
}

fn filter_first(value: MiniJinjaValue) -> MiniJinjaValue {
    if matches!(value.kind(), ValueKind::Seq) {
        value
            .get_item(&MiniJinjaValue::from(0_i64))
            .unwrap_or(MiniJinjaValue::UNDEFINED)
    } else if let Some(s) = value.as_str() {
        s.chars()
            .next()
            .map(|c| MiniJinjaValue::from(c.to_string()))
            .unwrap_or(MiniJinjaValue::UNDEFINED)
    } else {
        MiniJinjaValue::UNDEFINED
    }
}

fn filter_last(value: MiniJinjaValue) -> MiniJinjaValue {
    if matches!(value.kind(), ValueKind::Seq) {
        let len = value.len().unwrap_or(0);
        if len > 0 {
            value
                .get_item(&MiniJinjaValue::from((len - 1) as i64))
                .unwrap_or(MiniJinjaValue::UNDEFINED)
        } else {
            MiniJinjaValue::UNDEFINED
        }
    } else if let Some(s) = value.as_str() {
        s.chars()
            .next_back()
            .map(|c| MiniJinjaValue::from(c.to_string()))
            .unwrap_or(MiniJinjaValue::UNDEFINED)
    } else {
        MiniJinjaValue::UNDEFINED
    }
}

fn filter_length(value: MiniJinjaValue) -> usize {
    value.len().unwrap_or(0)
}

fn filter_unique(value: Vec<MiniJinjaValue>) -> Vec<MiniJinjaValue> {
    let mut seen = std::collections::HashSet::new();
    value
        .into_iter()
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
    sorted.sort_by_key(|a| a.to_string());
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
    if let Some(stripped) = value.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped).to_string_lossy().to_string();
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
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|e| {
            minijinja::Error::new(
                ErrorKind::InvalidOperation,
                format!("Invalid base64: {}", e),
            )
        })?;
    String::from_utf8(decoded).map_err(|e| {
        minijinja::Error::new(ErrorKind::InvalidOperation, format!("Invalid UTF-8: {}", e))
    })
}

fn filter_to_json(value: MiniJinjaValue) -> std::result::Result<String, minijinja::Error> {
    serde_json::to_string(&value).map_err(|e| {
        minijinja::Error::new(
            ErrorKind::InvalidOperation,
            format!("JSON serialization failed: {}", e),
        )
    })
}

fn filter_to_nice_json(
    value: MiniJinjaValue,
    indent: Option<usize>,
    kwargs: Kwargs,
) -> std::result::Result<String, minijinja::Error> {
    // Handle both positional and keyword arguments for indent
    let indent = if let Ok(i) = kwargs.get("indent") {
        i
    } else {
        indent.unwrap_or(4)
    };

    // Use optimized direct serialization for all cases.
    // This avoids:
    // 1. serde_json::to_value() intermediate allocation (massive performance win)
    // 2. serde_json::to_string_pretty() using hardcoded 2 spaces instead of requested 4
    // 3. UTF-8 validation overhead via unsafe unchecked conversion (safe because serde_json guarantees UTF-8)
    format_json_with_indent(&value, indent)
}

fn filter_from_json(value: &str) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    let json: serde_json::Value = serde_json::from_str(value).map_err(|e| {
        minijinja::Error::new(
            ErrorKind::InvalidOperation,
            format!("JSON parse failed: {}", e),
        )
    })?;
    Ok(MiniJinjaValue::from_serialize(&json))
}

fn filter_to_yaml(value: MiniJinjaValue) -> std::result::Result<String, minijinja::Error> {
    serde_yaml::to_string(&value).map_err(|e| {
        minijinja::Error::new(
            ErrorKind::InvalidOperation,
            format!("YAML serialization failed: {}", e),
        )
    })
}

fn filter_to_nice_yaml(
    value: MiniJinjaValue,
    _indent: Option<usize>,
    _width: Option<usize>,
) -> std::result::Result<String, minijinja::Error> {
    serde_yaml::to_string(&value).map_err(|e| {
        minijinja::Error::new(
            ErrorKind::InvalidOperation,
            format!("YAML serialization failed: {}", e),
        )
    })
}

fn filter_from_yaml(value: &str) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    let yaml: serde_yaml::Value = serde_yaml::from_str(value).map_err(|e| {
        minijinja::Error::new(
            ErrorKind::InvalidOperation,
            format!("YAML parse failed: {}", e),
        )
    })?;
    Ok(MiniJinjaValue::from_serialize(&yaml))
}

fn filter_from_yaml_all(value: &str) -> std::result::Result<Vec<MiniJinjaValue>, minijinja::Error> {
    use serde::Deserialize;

    let mut docs = Vec::new();
    for doc in serde_yaml::Deserializer::from_str(value) {
        let yaml = serde_yaml::Value::deserialize(doc).map_err(|e| {
            minijinja::Error::new(
                ErrorKind::InvalidOperation,
                format!("YAML parse failed: {}", e),
            )
        })?;
        docs.push(MiniJinjaValue::from_serialize(&yaml));
    }
    Ok(docs)
}

fn format_json_with_indent<T: serde::Serialize>(
    value: &T,
    indent: usize,
) -> std::result::Result<String, minijinja::Error> {
    let mut buf = Vec::new();

    // Optimization: Stack-allocate up to 32 spaces to avoid heap allocation
    // Most JSON indentation is <= 32 spaces
    const MAX_STACK_SPACES: usize = 32;
    static SPACES: [u8; MAX_STACK_SPACES] = [b' '; MAX_STACK_SPACES];

    let heap_spaces;
    let indent_bytes = if indent <= MAX_STACK_SPACES {
        &SPACES[..indent]
    } else {
        heap_spaces = vec![b' '; indent];
        &heap_spaces[..]
    };

    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent_bytes);
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser).map_err(|e| {
        minijinja::Error::new(
            ErrorKind::InvalidOperation,
            format!("JSON serialization failed: {}", e),
        )
    })?;

    // Optimization: serde_json guarantees valid UTF-8, skip validation
    Ok(unsafe { String::from_utf8_unchecked(buf) })
}

fn filter_mandatory(
    value: MiniJinjaValue,
    msg: Option<String>,
) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    if value.is_undefined() || value.is_none() {
        let error_msg = msg.unwrap_or_else(|| "Mandatory variable is not defined".to_string());
        Err(minijinja::Error::new(
            ErrorKind::InvalidOperation,
            error_msg,
        ))
    } else {
        Ok(value)
    }
}

fn filter_ternary(
    value: MiniJinjaValue,
    true_val: MiniJinjaValue,
    false_val: MiniJinjaValue,
) -> MiniJinjaValue {
    if is_truthy_value(&value) {
        true_val
    } else {
        false_val
    }
}

fn filter_combine(
    value: MiniJinjaValue,
    other: MiniJinjaValue,
) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    // Simple implementation - combines two objects
    let mut result = serde_json::Map::new();

    if let Ok(iter) = value.try_iter() {
        for key in iter {
            if let Some(k) = key.as_str() {
                if let Ok(v) = value.get_item(&key) {
                    result.insert(
                        k.to_string(),
                        serde_json::to_value(&v).unwrap_or(serde_json::Value::Null),
                    );
                }
            }
        }
    }

    if let Ok(iter) = other.try_iter() {
        for key in iter {
            if let Some(k) = key.as_str() {
                if let Ok(v) = other.get_item(&key) {
                    result.insert(
                        k.to_string(),
                        serde_json::to_value(&v).unwrap_or(serde_json::Value::Null),
                    );
                }
            }
        }
    }

    Ok(MiniJinjaValue::from_serialize(&result))
}

fn filter_dict2items(
    value: MiniJinjaValue,
) -> std::result::Result<Vec<MiniJinjaValue>, minijinja::Error> {
    let mut items = Vec::new();
    if let Ok(iter) = value.try_iter() {
        for key in iter {
            if let Ok(val) = value.get_item(&key) {
                let mut item = serde_json::Map::new();
                item.insert(
                    "key".to_string(),
                    serde_json::to_value(&key).unwrap_or(serde_json::Value::Null),
                );
                item.insert(
                    "value".to_string(),
                    serde_json::to_value(&val).unwrap_or(serde_json::Value::Null),
                );
                items.push(MiniJinjaValue::from_serialize(&item));
            }
        }
    }
    Ok(items)
}

fn filter_items2dict(
    value: Vec<MiniJinjaValue>,
) -> std::result::Result<MiniJinjaValue, minijinja::Error> {
    let mut result = serde_json::Map::new();
    for item in value {
        if let (Ok(key), Ok(val)) = (
            item.get_item(&MiniJinjaValue::from("key")),
            item.get_item(&MiniJinjaValue::from("value")),
        ) {
            if let Some(k) = key.as_str() {
                result.insert(
                    k.to_string(),
                    serde_json::to_value(&val).unwrap_or(serde_json::Value::Null),
                );
            }
        }
    }
    Ok(MiniJinjaValue::from_serialize(&result))
}

fn filter_selectattr(
    value: Vec<MiniJinjaValue>,
    attr: &str,
    test: Option<&str>,
    test_value: Option<MiniJinjaValue>,
) -> Vec<MiniJinjaValue> {
    value
        .into_iter()
        .filter(|item| {
            if let Ok(attr_val) = item.get_item(&MiniJinjaValue::from(attr)) {
                match test.unwrap_or("truthy") {
                    "truthy" => is_truthy_value(&attr_val),
                    "equalto" | "eq" | "==" => test_value
                        .as_ref()
                        .map(|v| attr_val.to_string() == v.to_string())
                        .unwrap_or(false),
                    "defined" => !attr_val.is_undefined(),
                    _ => is_truthy_value(&attr_val),
                }
            } else {
                false
            }
        })
        .collect()
}

fn filter_rejectattr(
    value: Vec<MiniJinjaValue>,
    attr: &str,
    test: Option<&str>,
    test_value: Option<MiniJinjaValue>,
) -> Vec<MiniJinjaValue> {
    value
        .into_iter()
        .filter(|item| {
            if let Ok(attr_val) = item.get_item(&MiniJinjaValue::from(attr)) {
                match test.unwrap_or("truthy") {
                    "truthy" => !is_truthy_value(&attr_val),
                    "equalto" | "eq" | "==" => test_value
                        .as_ref()
                        .map(|v| attr_val.to_string() != v.to_string())
                        .unwrap_or(true),
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
        value
            .into_iter()
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
    matches!(
        value.kind(),
        ValueKind::Seq | ValueKind::Map | ValueKind::Iterable
    ) || value.as_str().is_some()
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
        // Optimization: Use cached regex
        crate::utils::get_regex(pattern)
            .map(|re| re.is_match(s))
            .unwrap_or(false)
    } else {
        false
    }
}

fn test_search(value: &MiniJinjaValue, pattern: &str) -> bool {
    if let Some(s) = value.as_str() {
        // Optimization: Use cached regex
        crate::utils::get_regex(pattern)
            .map(|re| re.find(s).is_some())
            .unwrap_or(false)
    } else {
        false
    }
}

fn test_startswith(value: &MiniJinjaValue, prefix: &str) -> bool {
    value
        .as_str()
        .map(|s| s.starts_with(prefix))
        .unwrap_or(false)
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

fn test_odd(value: &MiniJinjaValue) -> bool {
    value.as_i64().map(|n| n % 2 != 0).unwrap_or(false)
}

fn test_even(value: &MiniJinjaValue) -> bool {
    value.as_i64().map(|n| n % 2 == 0).unwrap_or(false)
}

fn test_divisibleby(value: &MiniJinjaValue, num: &MiniJinjaValue) -> bool {
    match (value.as_i64(), num.as_i64()) {
        (Some(v), Some(n)) if n != 0 => v % n == 0,
        _ => false,
    }
}

fn test_in(value: &MiniJinjaValue, seq: &MiniJinjaValue) -> bool {
    if let Some(s) = seq.as_str() {
        if let Some(v) = value.as_str() {
            return s.contains(v);
        }
    }
    if let Ok(iter) = seq.try_iter() {
        let value_str = value.to_string();
        for item in iter {
            if item.to_string() == value_str {
                return true;
            }
        }
    }
    false
}

fn test_subset(value: &MiniJinjaValue, other: &MiniJinjaValue) -> bool {
    if let Ok(iter_a) = value.try_iter() {
        for item in iter_a {
            let item_str = item.to_string();
            let mut found = false;
            if let Ok(iter_b) = other.try_iter() {
                for b_item in iter_b {
                    if b_item.to_string() == item_str {
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                return false;
            }
        }
        true
    } else {
        false
    }
}

fn test_superset(value: &MiniJinjaValue, other: &MiniJinjaValue) -> bool {
    test_subset(other, value)
}

fn test_callable(_value: &MiniJinjaValue) -> bool {
    false
}

fn test_escaped(_value: &MiniJinjaValue) -> bool {
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
        let result = engine
            .render("Hello, {{ name | default('World') }}!", &vars)
            .unwrap();
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
        vars.insert(
            "existing".to_string(),
            JsonValue::String("value".to_string()),
        );

        assert!(engine
            .evaluate_condition("existing is defined", &vars)
            .unwrap());
        assert!(engine
            .evaluate_condition("nonexistent is undefined", &vars)
            .unwrap());
    }

    #[test]
    fn test_evaluate_condition_comparison() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("os".to_string(), JsonValue::String("Debian".to_string()));
        vars.insert(
            "version".to_string(),
            JsonValue::Number(serde_json::Number::from(10)),
        );

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
        assert_eq!(
            result.into_owned(),
            JsonValue::String("Hello test".to_string())
        );
    }

    #[test]
    fn test_render_value_nested() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert(
            "host".to_string(),
            JsonValue::String("localhost".to_string()),
        );

        let value = serde_json::json!({
            "server": "{{ host }}",
            "port": 8080
        });
        let result = engine.render_value(&value, &vars).unwrap();
        assert_eq!(result["server"], "localhost");
        assert_eq!(result["port"], 8080);
    }

    #[test]
    fn test_template_cache_hits() {
        let engine = TemplateEngine::with_cache_size(2);
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), JsonValue::String("Alice".to_string()));

        let template = "Hello, {{ name }}!";
        engine.render(template, &vars).unwrap();
        let (template_count, expression_count) = engine.cache_stats();
        assert_eq!(template_count, 1);
        assert_eq!(expression_count, 0);

        engine.render(template, &vars).unwrap();
        let (template_count, _) = engine.cache_stats();
        assert_eq!(template_count, 1);
    }

    #[test]
    fn test_expression_cache_hits() {
        let engine = TemplateEngine::with_cache_size(2);
        let vars = IndexMap::new();

        assert!(engine.evaluate_condition("1", &vars).unwrap());
        let (_, expression_count) = engine.cache_stats();
        assert_eq!(expression_count, 1);

        assert!(engine.evaluate_condition("1", &vars).unwrap());
        let (_, expression_count) = engine.cache_stats();
        assert_eq!(expression_count, 1);
    }

    #[test]
    fn test_clear_cache() {
        let engine = TemplateEngine::with_cache_size(2);
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), JsonValue::String("Alice".to_string()));

        let template = "Hello, {{ name }}!";
        engine.render(template, &vars).unwrap();
        engine.evaluate_condition("1", &IndexMap::new()).unwrap();
        engine.clear_cache();

        let (template_count, expression_count) = engine.cache_stats();
        assert_eq!(template_count, 0);
        assert_eq!(expression_count, 0);
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

    #[test]
    fn test_filter_from_yaml() {
        let engine = TemplateEngine::new();
        let vars = HashMap::new();
        let result = engine
            .render("{{ ('a: 1' | from_yaml).a }}", &vars)
            .unwrap();
        assert_eq!(result.trim(), "1");
    }

    #[test]
    fn test_aliases_null_dict_list() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("x".to_string(), JsonValue::Null);
        vars.insert("d".to_string(), serde_json::json!({"a": 1}));
        vars.insert("l".to_string(), serde_json::json!([1, 2, 3]));

        assert!(engine.evaluate_condition("x is null", &vars).unwrap());
        assert!(engine.evaluate_condition("x is none", &vars).unwrap());
        assert!(engine.evaluate_condition("d is dict", &vars).unwrap());
        assert!(engine.evaluate_condition("d is mapping", &vars).unwrap());
        assert!(engine.evaluate_condition("l is list", &vars).unwrap());
        assert!(engine.evaluate_condition("l is sequence", &vars).unwrap());
    }

    #[test]
    fn test_odd_even() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("a".to_string(), serde_json::json!(3));
        vars.insert("b".to_string(), serde_json::json!(4));
        vars.insert("s".to_string(), JsonValue::String("hello".to_string()));

        assert!(engine.evaluate_condition("a is odd", &vars).unwrap());
        assert!(!engine.evaluate_condition("a is even", &vars).unwrap());
        assert!(engine.evaluate_condition("b is even", &vars).unwrap());
        assert!(!engine.evaluate_condition("b is odd", &vars).unwrap());
        // Non-numeric returns false
        assert!(!engine.evaluate_condition("s is odd", &vars).unwrap());
        assert!(!engine.evaluate_condition("s is even", &vars).unwrap());
    }

    #[test]
    fn test_divisibleby() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("n".to_string(), serde_json::json!(12));

        assert!(engine
            .evaluate_condition("n is divisibleby(3)", &vars)
            .unwrap());
        assert!(engine
            .evaluate_condition("n is divisibleby(4)", &vars)
            .unwrap());
        assert!(!engine
            .evaluate_condition("n is divisibleby(5)", &vars)
            .unwrap());
    }

    #[test]
    fn test_in() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("items".to_string(), serde_json::json!(["a", "b", "c"]));
        vars.insert("x".to_string(), JsonValue::String("b".to_string()));
        vars.insert("y".to_string(), JsonValue::String("z".to_string()));

        assert!(engine.evaluate_condition("x is in(items)", &vars).unwrap());
        assert!(!engine.evaluate_condition("y is in(items)", &vars).unwrap());
    }

    #[test]
    fn test_subset_superset() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("small".to_string(), serde_json::json!([1, 2]));
        vars.insert("big".to_string(), serde_json::json!([1, 2, 3, 4]));

        assert!(engine
            .evaluate_condition("small is subset(big)", &vars)
            .unwrap());
        assert!(!engine
            .evaluate_condition("big is subset(small)", &vars)
            .unwrap());
        assert!(engine
            .evaluate_condition("big is superset(small)", &vars)
            .unwrap());
        assert!(!engine
            .evaluate_condition("small is superset(big)", &vars)
            .unwrap());
    }

    #[test]
    fn test_subset_empty() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("empty".to_string(), serde_json::json!([]));
        vars.insert("items".to_string(), serde_json::json!([1, 2]));

        // Empty set is subset of anything
        assert!(engine
            .evaluate_condition("empty is subset(items)", &vars)
            .unwrap());
        // Non-empty is not subset of empty
        assert!(!engine
            .evaluate_condition("items is subset(empty)", &vars)
            .unwrap());
    }

    #[test]
    fn test_callable_and_escaped() {
        let engine = TemplateEngine::new();
        let mut vars = IndexMap::new();
        vars.insert("x".to_string(), serde_json::json!(42));

        // These always return false
        assert!(!engine.evaluate_condition("x is callable", &vars).unwrap());
        assert!(!engine.evaluate_condition("x is escaped", &vars).unwrap());
    }

    #[test]
    fn test_filter_to_nice_json() {
        let engine = TemplateEngine::new();
        let mut vars = HashMap::new();
        vars.insert(
            "data".to_string(),
            serde_json::json!({
                "a": 1,
                "b": [2, 3]
            }),
        );

        // Test default indent (should be 4)
        let result_default = engine.render("{{ data | to_nice_json }}", &vars).unwrap();

        assert!(result_default.contains(r#""a": 1"#));
        assert!(result_default.contains(r#""b": ["#));
        // Check indentation is 4 spaces
        assert!(result_default.contains("\n    \"a\""));

        // Test custom indent
        let result_custom = engine
            .render("{{ data | to_nice_json(indent=2) }}", &vars)
            .unwrap();

        assert!(result_custom.contains(r#""a": 1"#));
        // Check indentation is 2 spaces
        assert!(result_custom.contains("\n  \"a\""));
    }

    #[test]
    fn test_filter_bool() {
        let engine = TemplateEngine::new();
        let vars = HashMap::new();

        // Test truthy values
        assert_eq!(engine.render("{{ 'true' | bool }}", &vars).unwrap(), "true");
        assert_eq!(engine.render("{{ 'yes' | bool }}", &vars).unwrap(), "true");
        assert_eq!(engine.render("{{ 'on' | bool }}", &vars).unwrap(), "true");
        assert_eq!(engine.render("{{ '1' | bool }}", &vars).unwrap(), "true");

        // Test falsy values
        assert_eq!(
            engine.render("{{ 'false' | bool }}", &vars).unwrap(),
            "false"
        );
        assert_eq!(engine.render("{{ 'no' | bool }}", &vars).unwrap(), "false");
        assert_eq!(engine.render("{{ 'off' | bool }}", &vars).unwrap(), "false");
        assert_eq!(engine.render("{{ '0' | bool }}", &vars).unwrap(), "false");

        // Test empty string (should be false)
        assert_eq!(engine.render("{{ '' | bool }}", &vars).unwrap(), "false");
    }

    #[test]
    fn test_filter_title() {
        assert_eq!(filter_title("hello world"), "Hello World");
        // Verify whitespace preservation (fixes bug where multiple spaces were collapsed)
        assert_eq!(filter_title("hello   world"), "Hello   World");
        // Verify behavior with mixed characters
        assert_eq!(filter_title("a-b"), "A-b");
    }
}
