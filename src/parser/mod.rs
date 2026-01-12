//! YAML parsing and templating for Rustible.
//!
//! This module provides:
//! - Playbook YAML parsing
//! - Variable file parsing
//! - Jinja2-style templating using minijinja

pub mod playbook;
pub mod schema;

pub use playbook::{Handler, Play, Playbook, Task};

use crate::utils::{get_regex, unsafe_template_access_allowed};
use indexmap::IndexMap;
use minijinja::value::ValueKind;
use minijinja::{Environment, Value};
use once_cell::sync::Lazy;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during parsing
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("template error: {0}")]
    Template(#[from] minijinja::Error),

    #[error("invalid playbook structure: {0}")]
    InvalidStructure(String),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("include error: {0}")]
    IncludeError(String),
}

/// Result type for parsing operations
pub type ParseResult<T> = Result<T, ParseError>;

/// The main parser for Rustible
#[derive(Debug)]
pub struct Parser {
    /// Jinja2-style template environment
    template_env: Environment<'static>,

    /// Base directory for relative includes
    base_dir: Option<std::path::PathBuf>,

    /// Strict mode (fail on undefined variables)
    strict: bool,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// Create a new parser
    pub fn new() -> Self {
        let mut template_env = Environment::new();

        // Configure for Ansible compatibility
        template_env.set_trim_blocks(true);
        template_env.set_lstrip_blocks(true);

        // Add built-in filters
        Self::add_builtin_filters(&mut template_env);

        // Add built-in functions
        Self::add_builtin_functions(&mut template_env);

        Self {
            template_env,
            base_dir: None,
            strict: false,
        }
    }

    /// Create a parser with a base directory
    pub fn with_base_dir<P: AsRef<Path>>(mut self, base_dir: P) -> Self {
        self.base_dir = Some(base_dir.as_ref().to_path_buf());
        self
    }

    /// Enable strict mode
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Add Ansible-compatible built-in filters
    fn add_builtin_filters(env: &mut Environment<'static>) {
        // String filters
        env.add_filter("lower", |s: String| s.to_lowercase());
        env.add_filter("upper", |s: String| s.to_uppercase());
        env.add_filter("capitalize", |s: String| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().chain(chars).collect(),
            }
        });
        env.add_filter("title", |s: String| {
            s.split_whitespace()
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(c) => c
                            .to_uppercase()
                            .chain(chars.flat_map(|c| c.to_lowercase()))
                            .collect(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        });
        env.add_filter("trim", |s: String| s.trim().to_string());
        env.add_filter("strip", |s: String| s.trim().to_string());

        // Replace filter
        env.add_filter("replace", |s: String, from: String, to: String| {
            s.replace(&from, &to)
        });

        // Split/join filters
        env.add_filter("split", |s: String, sep: Option<String>| -> Vec<String> {
            let sep = sep.unwrap_or_else(|| " ".to_string());
            s.split(&sep).map(|s| s.to_string()).collect()
        });

        // Default filter
        env.add_filter("default", |value: Value, default: Value| -> Value {
            if value.is_undefined() || value.is_none() {
                default
            } else {
                value
            }
        });
        env.add_filter("d", |value: Value, default: Value| -> Value {
            if value.is_undefined() || value.is_none() {
                default
            } else {
                value
            }
        });

        // Boolean filter
        env.add_filter("bool", |value: Value| -> bool {
            match value.as_str() {
                Some("true" | "yes" | "on" | "1") => true,
                Some("false" | "no" | "off" | "0" | "") => false,
                None => {
                    if let Ok(b) = value.clone().try_into() {
                        b
                    } else if let Ok(n) = TryInto::<i64>::try_into(value) {
                        n != 0
                    } else {
                        false
                    }
                }
                _ => true,
            }
        });

        // Int filter
        env.add_filter("int", |value: Value| -> i64 {
            if let Some(s) = value.as_str() {
                s.parse().unwrap_or(0)
            } else if let Ok(n) = value.clone().try_into() {
                n
            } else if let Ok(f) = TryInto::<f64>::try_into(value) {
                f as i64
            } else {
                0
            }
        });

        // Float filter
        env.add_filter("float", |value: Value| -> f64 {
            if let Some(s) = value.as_str() {
                s.parse().unwrap_or(0.0)
            } else if let Ok(n) = TryInto::<i64>::try_into(value.clone()) {
                n as f64
            } else if let Ok(f) = value.try_into() {
                f
            } else {
                0.0
            }
        });

        // String filter
        env.add_filter("string", |value: Value| -> String { value.to_string() });

        // Length filter
        env.add_filter("length", |value: Value| -> usize {
            if let Some(s) = value.as_str() {
                s.len()
            } else {
                value.len().unwrap_or(0)
            }
        });

        // First/last filters
        env.add_filter("first", |value: Value| -> Value {
            if matches!(value.kind(), ValueKind::Seq) {
                value
                    .get_item(&Value::from(0_i64))
                    .unwrap_or(Value::UNDEFINED)
            } else if let Some(s) = value.as_str() {
                s.chars()
                    .next()
                    .map(|c| Value::from(c.to_string()))
                    .unwrap_or(Value::UNDEFINED)
            } else {
                Value::UNDEFINED
            }
        });

        env.add_filter("last", |value: Value| -> Value {
            if matches!(value.kind(), ValueKind::Seq) {
                let len = value.len().unwrap_or(0);
                if len > 0 {
                    value
                        .get_item(&Value::from((len - 1) as i64))
                        .unwrap_or(Value::UNDEFINED)
                } else {
                    Value::UNDEFINED
                }
            } else if let Some(s) = value.as_str() {
                s.chars()
                    .last()
                    .map(|c| Value::from(c.to_string()))
                    .unwrap_or(Value::UNDEFINED)
            } else {
                Value::UNDEFINED
            }
        });

        // Unique filter
        env.add_filter("unique", |value: Value| -> Vec<Value> {
            if matches!(value.kind(), ValueKind::Seq) {
                let mut seen = std::collections::HashSet::new();
                let mut result = Vec::new();
                if let Ok(iter) = value.try_iter() {
                    for item in iter {
                        let key = item.to_string();
                        if seen.insert(key) {
                            result.push(item);
                        }
                    }
                }
                result
            } else {
                Vec::new()
            }
        });

        // Sort filter
        env.add_filter("sort", |value: Value| -> Vec<Value> {
            if matches!(value.kind(), ValueKind::Seq) {
                if let Ok(iter) = value.try_iter() {
                    let mut items: Vec<Value> = iter.collect();
                    items.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
                    items
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        // Reverse filter
        env.add_filter("reverse", |value: Value| -> Value {
            if matches!(value.kind(), ValueKind::Seq) {
                if let Ok(iter) = value.try_iter() {
                    let items: Vec<Value> = iter.collect::<Vec<_>>().into_iter().rev().collect();
                    Value::from(items)
                } else {
                    value
                }
            } else if let Some(s) = value.as_str() {
                Value::from(s.chars().rev().collect::<String>())
            } else {
                value
            }
        });

        // Flatten filter
        env.add_filter("flatten", |value: Value| -> Vec<Value> {
            fn flatten_recursive(value: &Value, result: &mut Vec<Value>) {
                if matches!(value.kind(), ValueKind::Seq) {
                    if let Ok(iter) = value.try_iter() {
                        for item in iter {
                            flatten_recursive(&item, result);
                        }
                    }
                } else {
                    result.push(value.clone());
                }
            }

            let mut result = Vec::new();
            flatten_recursive(&value, &mut result);
            result
        });

        // Map filter (select attribute)
        env.add_filter("map", |value: Value, attr: Option<String>| -> Vec<Value> {
            if matches!(value.kind(), ValueKind::Seq) {
                if let Ok(iter) = value.try_iter() {
                    if let Some(attr) = attr {
                        iter.filter_map(|item| {
                            if matches!(item.kind(), ValueKind::Map) {
                                item.get_attr(&attr).ok()
                            } else {
                                None
                            }
                        })
                        .collect()
                    } else {
                        iter.collect()
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        // Select filter
        env.add_filter(
            "select",
            |value: Value, attr: Option<String>| -> Vec<Value> {
                if matches!(value.kind(), ValueKind::Seq) {
                    if let Ok(iter) = value.try_iter() {
                        iter.filter(|item| {
                            if let Some(ref attr) = attr {
                                if matches!(item.kind(), ValueKind::Map) {
                                    item.get_attr(attr).map(|v| v.is_true()).unwrap_or(false)
                                } else {
                                    false
                                }
                            } else {
                                item.is_true()
                            }
                        })
                        .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            },
        );

        // Reject filter
        env.add_filter(
            "reject",
            |value: Value, attr: Option<String>| -> Vec<Value> {
                if matches!(value.kind(), ValueKind::Seq) {
                    if let Ok(iter) = value.try_iter() {
                        iter.filter(|item| {
                            if let Some(ref attr) = attr {
                                if matches!(item.kind(), ValueKind::Map) {
                                    !item.get_attr(attr).map(|v| v.is_true()).unwrap_or(false)
                                } else {
                                    true
                                }
                            } else {
                                !item.is_true()
                            }
                        })
                        .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            },
        );

        // JSON filters
        env.add_filter("to_json", |value: Value| -> String {
            serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
        });

        env.add_filter("to_nice_json", |value: Value| -> String {
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".to_string())
        });

        env.add_filter("from_json", |s: String| -> Value {
            serde_json::from_str(&s).unwrap_or(Value::UNDEFINED)
        });

        // YAML filters
        env.add_filter("to_yaml", |value: Value| -> String {
            serde_yaml::to_string(&value).unwrap_or_else(|_| "null".to_string())
        });

        env.add_filter("from_yaml", |s: String| -> Value {
            serde_yaml::from_str(&s).unwrap_or(Value::UNDEFINED)
        });

        // Base64 filters
        env.add_filter("b64encode", |s: String| -> String {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
        });

        env.add_filter("b64decode", |s: String| -> String {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(&s)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_default()
        });

        // Regex filters
        env.add_filter("regex_search", |s: String, pattern: String| -> bool {
            get_regex(&pattern)
                .map(|re| re.is_match(&s))
                .unwrap_or(false)
        });

        env.add_filter(
            "regex_replace",
            |s: String, pattern: String, replacement: String| -> String {
                get_regex(&pattern)
                    .map(|re| re.replace_all(&s, replacement.as_str()).to_string())
                    .unwrap_or(s)
            },
        );

        // Path filters
        env.add_filter("basename", |s: String| -> String {
            Path::new(&s)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        });

        env.add_filter("dirname", |s: String| -> String {
            Path::new(&s)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string()
        });

        env.add_filter("expanduser", |s: String| -> String {
            if !unsafe_template_access_allowed() {
                return s;
            }
            shellexpand::tilde(&s).to_string()
        });

        env.add_filter("realpath", |s: String| -> String {
            if !unsafe_template_access_allowed() {
                return s;
            }
            std::fs::canonicalize(&s)
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or(s)
        });

        // Hash filters
        env.add_filter("hash", |s: String, algorithm: Option<String>| -> String {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let algo = algorithm.unwrap_or_else(|| "sha256".to_string());
            match algo.as_str() {
                // For simplicity, use a basic hash for non-crypto purposes
                // In production, you'd use proper crypto hash functions
                _ => {
                    let mut hasher = DefaultHasher::new();
                    s.hash(&mut hasher);
                    format!("{:x}", hasher.finish())
                }
            }
        });

        // Quote filter for shell
        env.add_filter("quote", |s: String| -> String {
            format!("'{}'", s.replace('\'', "'\"'\"'"))
        });

        // Comment filter
        env.add_filter("comment", |s: String, prefix: Option<String>| -> String {
            let prefix = prefix.unwrap_or_else(|| "# ".to_string());
            s.lines()
                .map(|line| format!("{}{}", prefix, line))
                .collect::<Vec<_>>()
                .join("\n")
        });

        // Ternary filter
        env.add_filter(
            "ternary",
            |condition: bool, true_val: Value, false_val: Value| -> Value {
                if condition {
                    true_val
                } else {
                    false_val
                }
            },
        );

        // Combine filter for dicts
        env.add_filter("combine", |base: Value, other: Value| -> Value {
            let is_base_map = matches!(base.kind(), ValueKind::Map);
            let is_other_map = matches!(other.kind(), ValueKind::Map);
            if is_base_map && is_other_map {
                let mut result = std::collections::BTreeMap::new();
                // Iterate over base map entries
                if let Ok(iter) = base.try_iter() {
                    for key in iter {
                        let key_str = key.to_string();
                        if let Ok(val) = base.get_attr(&key_str) {
                            result.insert(key_str, val);
                        }
                    }
                }
                // Iterate over other map entries (overwrites base)
                if let Ok(iter) = other.try_iter() {
                    for key in iter {
                        let key_str = key.to_string();
                        if let Ok(val) = other.get_attr(&key_str) {
                            result.insert(key_str, val);
                        }
                    }
                }
                Value::from_iter(result)
            } else {
                base
            }
        });

        // Dict2items / items2dict
        env.add_filter("dict2items", |value: Value| -> Vec<Value> {
            if matches!(value.kind(), ValueKind::Map) {
                if let Ok(iter) = value.try_iter() {
                    iter.filter_map(|k| {
                        let key_str = k.to_string();
                        value.get_attr(&key_str).ok().map(|v| {
                            Value::from_iter([
                                ("key".to_string(), Value::from(key_str)),
                                ("value".to_string(), v),
                            ])
                        })
                    })
                    .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        // Type testing filters
        env.add_filter("type_debug", |value: Value| -> String {
            if value.is_undefined() {
                "undefined".to_string()
            } else if value.is_none() {
                "null".to_string()
            } else if value.as_str().is_some() {
                "string".to_string()
            } else if matches!(value.kind(), ValueKind::Seq) {
                "list".to_string()
            } else if matches!(value.kind(), ValueKind::Map) {
                "dict".to_string()
            } else if TryInto::<bool>::try_into(value.clone()).is_ok() {
                "bool".to_string()
            } else if TryInto::<i64>::try_into(value.clone()).is_ok() {
                "int".to_string()
            } else if TryInto::<f64>::try_into(value).is_ok() {
                "float".to_string()
            } else {
                "unknown".to_string()
            }
        });

        // Mandatory filter - fail if undefined
        env.add_filter(
            "mandatory",
            |value: Value, msg: Option<String>| -> Result<Value, minijinja::Error> {
                if value.is_undefined() || value.is_none() {
                    let message =
                        msg.unwrap_or_else(|| "Mandatory variable is undefined".to_string());
                    Err(minijinja::Error::new(
                        minijinja::ErrorKind::UndefinedError,
                        message,
                    ))
                } else {
                    Ok(value)
                }
            },
        );

        // Random filter - select random element or generate random number
        env.add_filter("random", |value: Value| -> Value {
            use rand::Rng;
            if matches!(value.kind(), ValueKind::Seq) {
                let len = value.len().unwrap_or(0);
                if len > 0 {
                    let idx = rand::rngs::OsRng.gen_range(0..len);
                    value
                        .get_item(&Value::from(idx))
                        .unwrap_or(Value::UNDEFINED)
                } else {
                    Value::UNDEFINED
                }
            } else if let Ok(max) = TryInto::<i64>::try_into(value) {
                let n = rand::rngs::OsRng.gen_range(0..max);
                Value::from(n)
            } else {
                Value::UNDEFINED
            }
        });

        // Shuffle filter
        env.add_filter("shuffle", |value: Value| -> Vec<Value> {
            use rand::seq::SliceRandom;
            if matches!(value.kind(), ValueKind::Seq) {
                if let Ok(iter) = value.try_iter() {
                    let mut items: Vec<Value> = iter.collect();
                    items.shuffle(&mut rand::rngs::OsRng);
                    items
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        // Regex findall filter
        env.add_filter(
            "regex_findall",
            |s: String, pattern: String| -> Vec<String> {
                get_regex(&pattern)
                    .map(|re| re.find_iter(&s).map(|m| m.as_str().to_string()).collect())
                    .unwrap_or_default()
            },
        );

        // Selectattr filter - filter items by attribute value
        env.add_filter(
            "selectattr",
            |value: Value,
             attr: String,
             test: Option<String>,
             test_val: Option<Value>|
             -> Vec<Value> {
                if matches!(value.kind(), ValueKind::Seq) {
                    if let Ok(iter) = value.try_iter() {
                        iter.filter(|item| {
                            if matches!(item.kind(), ValueKind::Map) {
                                if let Ok(attr_val) = item.get_attr(&attr) {
                                    match test.as_deref() {
                                        Some("equalto" | "==" | "eq") => {
                                            if let Some(ref tv) = test_val {
                                                attr_val.to_string() == tv.to_string()
                                            } else {
                                                false
                                            }
                                        }
                                        Some("defined") => !attr_val.is_undefined(),
                                        Some("undefined") => attr_val.is_undefined(),
                                        Some("none") => attr_val.is_none(),
                                        Some("true" | "truthy") => attr_val.is_true(),
                                        Some("false" | "falsy") => !attr_val.is_true(),
                                        None | Some(_) => attr_val.is_true(),
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        })
                        .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            },
        );

        // Rejectattr filter - reject items by attribute value
        env.add_filter(
            "rejectattr",
            |value: Value,
             attr: String,
             test: Option<String>,
             test_val: Option<Value>|
             -> Vec<Value> {
                if matches!(value.kind(), ValueKind::Seq) {
                    if let Ok(iter) = value.try_iter() {
                        iter.filter(|item| {
                            if matches!(item.kind(), ValueKind::Map) {
                                if let Ok(attr_val) = item.get_attr(&attr) {
                                    match test.as_deref() {
                                        Some("equalto" | "==" | "eq") => {
                                            if let Some(ref tv) = test_val {
                                                attr_val.to_string() != tv.to_string()
                                            } else {
                                                true
                                            }
                                        }
                                        Some("defined") => attr_val.is_undefined(),
                                        Some("undefined") => !attr_val.is_undefined(),
                                        Some("none") => !attr_val.is_none(),
                                        Some("true" | "truthy") => !attr_val.is_true(),
                                        Some("false" | "falsy") => attr_val.is_true(),
                                        None | Some(_) => !attr_val.is_true(),
                                    }
                                } else {
                                    true // No attribute = include in reject
                                }
                            } else {
                                true
                            }
                        })
                        .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            },
        );

        // Strftime filter - date formatting
        env.add_filter(
            "strftime",
            |format: String, timestamp: Option<i64>| -> String {
                use chrono::{TimeZone, Utc};
                let dt = if let Some(ts) = timestamp {
                    Utc.timestamp_opt(ts, 0).single()
                } else {
                    Some(Utc::now())
                };
                dt.map(|d| d.format(&format).to_string())
                    .unwrap_or_default()
            },
        );

        // Join filter with custom separator
        env.add_filter("join", |value: Value, sep: Option<String>| -> String {
            let separator = sep.unwrap_or_else(|| "".to_string());
            if matches!(value.kind(), ValueKind::Seq) {
                if let Ok(iter) = value.try_iter() {
                    iter.map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(&separator)
                } else {
                    value.to_string()
                }
            } else {
                value.to_string()
            }
        });

        // Set operations
        env.add_filter("difference", |value: Value, other: Value| -> Vec<Value> {
            let is_seq1 = matches!(value.kind(), ValueKind::Seq);
            let is_seq2 = matches!(other.kind(), ValueKind::Seq);
            if is_seq1 && is_seq2 {
                if let (Ok(iter1), Ok(iter2)) = (value.try_iter(), other.try_iter()) {
                    let set2: std::collections::HashSet<String> =
                        iter2.map(|v| v.to_string()).collect();
                    iter1.filter(|v| !set2.contains(&v.to_string())).collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        env.add_filter("intersect", |value: Value, other: Value| -> Vec<Value> {
            let is_seq1 = matches!(value.kind(), ValueKind::Seq);
            let is_seq2 = matches!(other.kind(), ValueKind::Seq);
            if is_seq1 && is_seq2 {
                if let (Ok(iter1), Ok(iter2)) = (value.try_iter(), other.try_iter()) {
                    let set2: std::collections::HashSet<String> =
                        iter2.map(|v| v.to_string()).collect();
                    iter1.filter(|v| set2.contains(&v.to_string())).collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        env.add_filter("union", |value: Value, other: Value| -> Vec<Value> {
            let is_seq1 = matches!(value.kind(), ValueKind::Seq);
            let is_seq2 = matches!(other.kind(), ValueKind::Seq);
            if is_seq1 && is_seq2 {
                if let (Ok(iter1), Ok(iter2)) = (value.try_iter(), other.try_iter()) {
                    let mut seen = std::collections::HashSet::new();
                    let mut result = Vec::new();
                    for item in iter1.chain(iter2) {
                        let key = item.to_string();
                        if seen.insert(key) {
                            result.push(item);
                        }
                    }
                    result
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        env.add_filter(
            "symmetric_difference",
            |value: Value, other: Value| -> Vec<Value> {
                let is_seq1 = matches!(value.kind(), ValueKind::Seq);
                let is_seq2 = matches!(other.kind(), ValueKind::Seq);
                if is_seq1 && is_seq2 {
                    // Collect iterators into Vecs since we need to iterate multiple times
                    let items1: Vec<Value> =
                        value.try_iter().map(|i| i.collect()).unwrap_or_default();
                    let items2: Vec<Value> =
                        other.try_iter().map(|i| i.collect()).unwrap_or_default();
                    let set1: std::collections::HashSet<String> =
                        items1.iter().map(|v| v.to_string()).collect();
                    let set2: std::collections::HashSet<String> =
                        items2.iter().map(|v| v.to_string()).collect();
                    let mut result = Vec::new();
                    for item in &items1 {
                        if !set2.contains(&item.to_string()) {
                            result.push(item.clone());
                        }
                    }
                    for item in &items2 {
                        if !set1.contains(&item.to_string()) {
                            result.push(item.clone());
                        }
                    }
                    result
                } else {
                    Vec::new()
                }
            },
        );

        // Zip filter - combine lists
        env.add_filter("zip", |value: Value, other: Value| -> Vec<Value> {
            let is_seq1 = matches!(value.kind(), ValueKind::Seq);
            let is_seq2 = matches!(other.kind(), ValueKind::Seq);
            if is_seq1 && is_seq2 {
                if let (Ok(iter1), Ok(iter2)) = (value.try_iter(), other.try_iter()) {
                    iter1
                        .zip(iter2)
                        .map(|(a, b)| Value::from(vec![a, b]))
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        // Min/Max filters
        env.add_filter("min", |value: Value| -> Value {
            if matches!(value.kind(), ValueKind::Seq) {
                if let Ok(iter) = value.try_iter() {
                    iter.min_by(|a, b| a.to_string().cmp(&b.to_string()))
                        .unwrap_or(Value::UNDEFINED)
                } else {
                    Value::UNDEFINED
                }
            } else {
                Value::UNDEFINED
            }
        });

        env.add_filter("max", |value: Value| -> Value {
            if matches!(value.kind(), ValueKind::Seq) {
                if let Ok(iter) = value.try_iter() {
                    iter.max_by(|a, b| a.to_string().cmp(&b.to_string()))
                        .unwrap_or(Value::UNDEFINED)
                } else {
                    Value::UNDEFINED
                }
            } else {
                Value::UNDEFINED
            }
        });

        // Subelements filter
        env.add_filter("subelements", |value: Value, key: String| -> Vec<Value> {
            if matches!(value.kind(), ValueKind::Seq) {
                let mut result = Vec::new();
                if let Ok(iter) = value.try_iter() {
                    for item in iter {
                        if matches!(item.kind(), ValueKind::Map) {
                            if let Ok(sub) = item.get_attr(&key) {
                                if matches!(sub.kind(), ValueKind::Seq) {
                                    if let Ok(sub_iter) = sub.try_iter() {
                                        for sub_item in sub_iter {
                                            result.push(Value::from(vec![item.clone(), sub_item]));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                result
            } else {
                Vec::new()
            }
        });

        // Groupby filter
        env.add_filter("groupby", |value: Value, attr: String| -> Vec<Value> {
            if matches!(value.kind(), ValueKind::Seq) {
                let mut groups: indexmap::IndexMap<String, Vec<Value>> = indexmap::IndexMap::new();
                if let Ok(iter) = value.try_iter() {
                    for item in iter {
                        let key = if matches!(item.kind(), ValueKind::Map) {
                            item.get_attr(&attr)
                                .map(|v| v.to_string())
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };
                        groups.entry(key).or_default().push(item);
                    }
                }
                groups
                    .into_iter()
                    .map(|(k, v)| {
                        Value::from_iter([
                            ("grouper".to_string(), Value::from(k)),
                            ("list".to_string(), Value::from(v)),
                        ])
                    })
                    .collect()
            } else {
                Vec::new()
            }
        });
    }

    /// Add Ansible-compatible built-in functions
    fn add_builtin_functions(env: &mut Environment<'static>) {
        // Range function
        env.add_function(
            "range",
            |start: i64, end: Option<i64>, step: Option<i64>| -> Vec<i64> {
                let (actual_start, actual_end) = match end {
                    Some(e) => (start, e),
                    None => (0, start),
                };
                let step = step.unwrap_or(1);

                if step == 0 {
                    return Vec::new();
                }

                let mut result = Vec::new();
                let mut current = actual_start;

                if step > 0 {
                    while current < actual_end {
                        result.push(current);
                        current += step;
                    }
                } else {
                    while current > actual_end {
                        result.push(current);
                        current += step;
                    }
                }

                result
            },
        );

        // Now function
        env.add_function("now", || -> String {
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
        });

        // Query/lookup function (simplified)
        env.add_function(
            "query",
            |plugin: String, _args: Option<Value>| -> Vec<Value> {
                // This would need full implementation for each lookup plugin
                match plugin.as_str() {
                    _ => Vec::new(),
                }
            },
        );

        // Lookup function
        env.add_function("lookup", |plugin: String, args: Option<Value>| -> Value {
            // Simplified lookup - would need full plugin system
            match plugin.as_str() {
                "env" => {
                    if !unsafe_template_access_allowed() {
                        return Value::UNDEFINED;
                    }
                    let var_value = args.as_ref().and_then(|a| {
                        if matches!(a.kind(), ValueKind::Seq) {
                            a.get_item(&Value::from(0)).ok()
                        } else {
                            Some(a.clone())
                        }
                    });
                    if let Some(v) = var_value {
                        if let Some(var) = v.as_str() {
                            std::env::var(var)
                                .map(Value::from)
                                .unwrap_or(Value::UNDEFINED)
                        } else {
                            Value::UNDEFINED
                        }
                    } else {
                        Value::UNDEFINED
                    }
                }
                _ => Value::UNDEFINED,
            }
        });

        // Password lookup (returns placeholder)
        env.add_function("password", |_path: String| -> String {
            // In real implementation, this would generate/retrieve passwords
            "placeholder_password".to_string()
        });

        // Omit function (for omitting parameters)
        env.add_function("omit", || -> Value { Value::from("__omit_place_holder__") });

        // Undef function
        env.add_function("undef", || -> Value { Value::UNDEFINED });
    }

    /// Parse a playbook from a file
    pub fn parse_playbook<P: AsRef<Path>>(&self, path: P) -> ParseResult<Playbook> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)?;

        let mut playbook = self.parse_playbook_str(&content)?;
        playbook.source_path = Some(path.to_path_buf());

        Ok(playbook)
    }

    /// Parse a playbook from a string
    pub fn parse_playbook_str(&self, content: &str) -> ParseResult<Playbook> {
        let docs: Vec<serde_yaml::Value> = serde_yaml::from_str(content)?;

        let mut playbook = Playbook::new();

        for doc in docs {
            if let serde_yaml::Value::Sequence(plays) = doc {
                for play_value in plays {
                    let play = self.parse_play(&play_value)?;
                    playbook.add_play(play);
                }
            } else if let serde_yaml::Value::Mapping(_) = doc {
                // Single play
                let play = self.parse_play(&doc)?;
                playbook.add_play(play);
            }
        }

        Ok(playbook)
    }

    /// Parse a single play from YAML value
    fn parse_play(&self, value: &serde_yaml::Value) -> ParseResult<Play> {
        let play: Play = serde_yaml::from_value(value.clone())?;
        Ok(play)
    }

    /// Parse a tasks file
    pub fn parse_tasks<P: AsRef<Path>>(&self, path: P) -> ParseResult<Vec<Task>> {
        let content = std::fs::read_to_string(path)?;
        self.parse_tasks_str(&content)
    }

    /// Parse tasks from a string
    pub fn parse_tasks_str(&self, content: &str) -> ParseResult<Vec<Task>> {
        let tasks: Vec<Task> = serde_yaml::from_str(content)?;
        Ok(tasks)
    }

    /// Parse a handlers file
    pub fn parse_handlers<P: AsRef<Path>>(&self, path: P) -> ParseResult<Vec<Handler>> {
        let content = std::fs::read_to_string(path)?;
        self.parse_handlers_str(&content)
    }

    /// Parse handlers from a string
    pub fn parse_handlers_str(&self, content: &str) -> ParseResult<Vec<Handler>> {
        let handlers: Vec<Handler> = serde_yaml::from_str(content)?;
        Ok(handlers)
    }

    /// Parse a variables file
    pub fn parse_vars<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> ParseResult<IndexMap<String, serde_yaml::Value>> {
        let content = std::fs::read_to_string(path)?;
        self.parse_vars_str(&content)
    }

    /// Parse variables from a string
    pub fn parse_vars_str(
        &self,
        content: &str,
    ) -> ParseResult<IndexMap<String, serde_yaml::Value>> {
        let vars: IndexMap<String, serde_yaml::Value> = serde_yaml::from_str(content)?;
        Ok(vars)
    }

    /// Render a template string with variables
    pub fn render_template(
        &self,
        template: &str,
        vars: &IndexMap<String, serde_yaml::Value>,
    ) -> ParseResult<String> {
        // Convert vars to minijinja values
        let context = yaml_to_minijinja_value(&serde_yaml::Value::Mapping(
            vars.iter()
                .map(|(k, v)| (serde_yaml::Value::String(k.clone()), v.clone()))
                .collect(),
        ));

        let result = self.template_env.render_str(template, context)?;
        Ok(result)
    }

    /// Check if a string contains template expressions
    pub fn has_template(&self, s: &str) -> bool {
        s.contains("{{") || s.contains("{%") || s.contains("{#")
    }

    /// Render all template expressions in a YAML value
    pub fn render_value(
        &self,
        value: &serde_yaml::Value,
        vars: &IndexMap<String, serde_yaml::Value>,
    ) -> ParseResult<serde_yaml::Value> {
        match value {
            serde_yaml::Value::String(s) => {
                if self.has_template(s) {
                    let rendered = self.render_template(s, vars)?;
                    // Try to parse as YAML to get proper type
                    if let Ok(parsed) = serde_yaml::from_str(&rendered) {
                        Ok(parsed)
                    } else {
                        Ok(serde_yaml::Value::String(rendered))
                    }
                } else {
                    Ok(value.clone())
                }
            }
            serde_yaml::Value::Sequence(seq) => {
                let rendered: Result<Vec<_>, _> =
                    seq.iter().map(|v| self.render_value(v, vars)).collect();
                Ok(serde_yaml::Value::Sequence(rendered?))
            }
            serde_yaml::Value::Mapping(map) => {
                let mut rendered = serde_yaml::Mapping::new();
                for (k, v) in map {
                    let rendered_key = self.render_value(k, vars)?;
                    let rendered_val = self.render_value(v, vars)?;
                    rendered.insert(rendered_key, rendered_val);
                }
                Ok(serde_yaml::Value::Mapping(rendered))
            }
            _ => Ok(value.clone()),
        }
    }

    /// Get the template environment for advanced usage
    pub fn template_env(&self) -> &Environment<'static> {
        &self.template_env
    }

    /// Get a mutable reference to the template environment
    pub fn template_env_mut(&mut self) -> &mut Environment<'static> {
        &mut self.template_env
    }
}

/// Convert YAML value to minijinja Value
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

/// Evaluate a condition expression
pub fn evaluate_condition(
    condition: &str,
    vars: &IndexMap<String, serde_yaml::Value>,
) -> ParseResult<bool> {
    let parser = Parser::new();

    // Wrap in {{ }} if not already a template
    let template = if condition.contains("{{") {
        condition.to_string()
    } else {
        format!("{{{{ {} }}}}", condition)
    };

    let result = parser.render_template(&template, vars)?;

    // Parse the result as a boolean
    Ok(matches!(
        result.trim().to_lowercase().as_str(),
        "true" | "yes" | "1"
    ))
}

/// Extract variable references from a template string
pub fn extract_variables(template: &str) -> Vec<String> {
    static VAR_PATTERN: Lazy<regex::Regex> = Lazy::new(|| {
        regex::Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)*)\s*\}\}")
            .expect("Invalid variable regex")
    });

    VAR_PATTERN
        .captures_iter(template)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_new() {
        let parser = Parser::new();
        assert!(!parser.strict);
    }

    #[test]
    fn test_render_template() {
        let parser = Parser::new();
        let mut vars = IndexMap::new();
        vars.insert(
            "name".to_string(),
            serde_yaml::Value::String("world".to_string()),
        );

        let result = parser.render_template("Hello, {{ name }}!", &vars).unwrap();
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_has_template() {
        let parser = Parser::new();
        assert!(parser.has_template("{{ var }}"));
        assert!(parser.has_template("{% if condition %}"));
        assert!(parser.has_template("{# comment #}"));
        assert!(!parser.has_template("plain string"));
    }

    #[test]
    fn test_parse_playbook_str() {
        let parser = Parser::new();
        let content = r#"
- name: Test Play
  hosts: all
  tasks:
    - name: Test Task
      debug:
        msg: "Hello"
"#;

        let playbook = parser.parse_playbook_str(content).unwrap();
        assert_eq!(playbook.play_count(), 1);
        assert_eq!(playbook.plays[0].name, "Test Play");
        assert_eq!(playbook.plays[0].tasks.len(), 1);
    }

    #[test]
    fn test_parse_vars_str() {
        let parser = Parser::new();
        let content = r#"
http_port: 80
app_name: myapp
debug: true
"#;

        let vars = parser.parse_vars_str(content).unwrap();
        assert_eq!(vars.len(), 3);
        assert_eq!(
            vars.get("http_port"),
            Some(&serde_yaml::Value::Number(80.into()))
        );
    }

    #[test]
    fn test_evaluate_condition() {
        let mut vars = IndexMap::new();
        vars.insert("enabled".to_string(), serde_yaml::Value::Bool(true));

        assert!(evaluate_condition("enabled", &vars).unwrap());
        assert!(evaluate_condition("enabled == true", &vars).unwrap());
    }

    #[test]
    fn test_extract_variables() {
        let template = "Hello {{ name }}, welcome to {{ place }}!";
        let vars = extract_variables(template);
        assert_eq!(vars, vec!["name", "place"]);
    }

    #[test]
    fn test_filters() {
        let parser = Parser::new();
        let vars = IndexMap::new();

        // Test lower filter
        let result = parser
            .render_template("{{ 'HELLO' | lower }}", &vars)
            .unwrap();
        assert_eq!(result, "hello");

        // Test upper filter
        let result = parser
            .render_template("{{ 'hello' | upper }}", &vars)
            .unwrap();
        assert_eq!(result, "HELLO");

        // Test default filter
        let result = parser
            .render_template("{{ undefined_var | default('default_value') }}", &vars)
            .unwrap();
        assert_eq!(result, "default_value");
    }
}
