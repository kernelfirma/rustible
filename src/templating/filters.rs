//! Jinja2-compatible template filters.
//!
//! Implements 50+ commonly used Jinja2 filters to maintain compatibility
//! with existing Ansible templates.

use chrono::{DateTime, TimeZone, Utc};
use regex::Regex;
use serde_json::Value;
use std::env;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur in filter operations.
#[derive(Debug, Error)]
pub enum FilterError {
    #[error("Invalid input for filter '{filter}': {message}")]
    InvalidInput { filter: String, message: String },

    #[error("Filter '{filter}' failed: {message}")]
    FilterFailed { filter: String, message: String },

    #[error("JSON error: {0}")]
    Json(String),

    #[error("Base64 error: {0}")]
    Base64(String),

    #[error("Regex error: {0}")]
    Regex(String),

    #[error("Invalid date/time: {0}")]
    InvalidDateTime(String),
}

/// Result type for filter operations.
pub type FilterResult<T> = Result<T, FilterError>;

/// Apply a Jinja2 filter to a value.
pub fn apply_filter(name: &str, value: &Value, args: &[Value]) -> FilterResult<Value> {
    match name {
        // String filters
        "upper" => filter_upper(value),
        "lower" => filter_lower(value),
        "capitalize" => filter_capitalize(value),
        "title" => filter_title(value),
        "trim" => filter_trim(value),
        "default" => filter_default(value, args),
        "replace" => filter_replace(value, args),
        "regex_replace" => filter_regex_replace(value, args),
        "split" => filter_split(value, args),
        "join" => filter_join(value, args),
        "length" => filter_length(value),
        "wordcount" => filter_wordcount(value),
        "urlencode" => filter_urlencode(value),
        "urldecode" => filter_urldecode(value),
        "basename" => filter_basename(value),
        "dirname" => filter_dirname(value),
        "realpath" => filter_realpath(value),
        "expanduser" => filter_expanduser(value),
        "bool" => filter_bool(value),
        "int" => filter_int(value),
        "float" => filter_float(value),
        "string" => filter_string(value),
        "abs" => filter_abs(value),
        "round" => filter_round(value, args),
        "to_json" => filter_to_json(value),
        "from_json" => filter_from_json(value),
        "to_yaml" => filter_to_yaml(value),
        "from_yaml" => filter_from_yaml(value),
        "to_nice_json" => filter_to_nice_json(value),
        "to_nice_yaml" => filter_to_nice_yaml(value),
        "b64encode" => filter_b64encode(value),
        "b64decode" => filter_b64decode(value),
        "md5" => filter_md5(value),
        "sha1" => filter_sha1(value),
        "sha256" => filter_sha256(value),
        "sha512" => filter_sha512(value),
        "quote" => filter_quote(value),
        "shuffle" => filter_shuffle(value),
        "sort" => filter_sort(value),
        "unique" => filter_unique(value),
        "min" => filter_min(value),
        "max" => filter_max(value),
        "sum" => filter_sum(value),
        "product" => filter_product(value),
        "mean" => filter_mean(value),
        "median" => filter_median(value),
        "first" => filter_first(value),
        "last" => filter_last(value),
        "nth" => filter_nth(value, args),
        "flatten" => filter_flatten(value),
        "items" => filter_items(value),
        "dict2items" => filter_dict2items(value),
        "items2dict" => filter_items2dict(value),
        "combine" => filter_combine(value, args),
        "dict" => filter_dict(args),
        "list" => filter_list(value),
        "range" => filter_range(value),
        "zip" => filter_zip(value, args),
        "map" => filter_map(value, args),
        "select" => filter_select(value, args),
        "reject" => filter_reject(value, args),
        "selectattr" => filter_selectattr(value, args),
        "rejectattr" => filter_rejectattr(value, args),
        "groupby" => filter_groupby(value, args),
        "strftime" => filter_strftime(value, args),
        "to_datetime" => filter_to_datetime(value),
        "timestamp" => filter_timestamp(value),
        "bool_filter" => filter_bool_filter(value),
        "mandatory" => filter_mandatory(value),
        "env" => filter_env(value),
        _ => Err(FilterError::FilterFailed {
            filter: name.to_string(),
            message: format!("Unknown filter: {}", name),
        }),
    }
}

// ============== String Filters ==============

fn filter_upper(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "upper".to_string(),
        message: "Expected string".to_string(),
    })?;
    Ok(Value::String(s.to_uppercase()))
}

fn filter_lower(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "lower".to_string(),
        message: "Expected string".to_string(),
    })?;
    Ok(Value::String(s.to_lowercase()))
}

fn filter_capitalize(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "capitalize".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let mut chars = s.chars();
    let result = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
        None => String::new(),
    };
    
    Ok(Value::String(result))
}

fn filter_title(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "title".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let result = s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    
    Ok(Value::String(result))
}

fn filter_trim(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "trim".to_string(),
        message: "Expected string".to_string(),
    })?;
    Ok(Value::String(s.trim().to_string()))
}

fn filter_default(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let default_value = args.get(0).unwrap_or(&Value::Null).clone();
    
    // Return default if value is null, undefined, or empty string
    match value {
        Value::Null => Ok(default_value),
        Value::String(s) if s.is_empty() => Ok(default_value),
        Value::Array(arr) if arr.is_empty() => Ok(default_value),
        Value::Object(obj) if obj.is_empty() => Ok(default_value),
        _ => Ok(value.clone()),
    }
}

fn filter_replace(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "replace".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let old = args.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "replace".to_string(),
            message: "Expected search string as first argument".to_string(),
        })?;
    
    let new = args.get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "replace".to_string(),
            message: "Expected replacement string as second argument".to_string(),
        })?;
    
    Ok(Value::String(s.replace(old, new)))
}

fn filter_regex_replace(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "regex_replace".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let pattern = args.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "regex_replace".to_string(),
            message: "Expected regex pattern as first argument".to_string(),
        })?;
    
    let replacement = args.get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "regex_replace".to_string(),
            message: "Expected replacement string as second argument".to_string(),
        })?;
    
    let regex = Regex::new(pattern).map_err(|e| FilterError::Regex(e.to_string()))?;
    let result = regex.replace_all(s, replacement).to_string();
    
    Ok(Value::String(result))
}

fn filter_split(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "split".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let delimiter = args.get(0)
        .and_then(|v| v.as_str())
        .unwrap_or(" ");
    
    let result: Vec<Value> = s.split(delimiter)
        .map(|part| Value::String(part.to_string()))
        .collect();
    
    Ok(Value::Array(result))
}

fn filter_join(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "join".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let separator = args.get(0)
        .and_then(|v| v.as_str())
        .unwrap_or("");
    
    let strings: Vec<&str> = arr.iter()
        .map(|v| v.as_str().unwrap_or(""))
        .collect();
    
    Ok(Value::String(strings.join(separator)))
}

fn filter_wordcount(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "wordcount".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let count = s.split_whitespace().count();
    Ok(Value::Number(count.into()))
}

fn filter_urlencode(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "urlencode".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    Ok(Value::String(urlencoding::encode(s).to_string()))
}

fn filter_urldecode(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "urldecode".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    Ok(Value::String(urlencoding::decode(s).unwrap_or_else(|_| s.to_string().into()).into_owned()))
}

fn filter_basename(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "basename".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    Ok(Value::String(
        Path::new(s).file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(s)
            .to_string()
    ))
}

fn filter_dirname(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "dirname".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    Ok(Value::String(
        Path::new(s).parent()
            .and_then(|p| p.to_str())
            .unwrap_or(".")
            .to_string()
    ))
}

fn filter_realpath(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "realpath".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    Ok(Value::String(
        Path::new(s).canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| s.to_string())
    ))
}

fn filter_expanduser(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "expanduser".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let result = if s.starts_with("~/") {
        format!("{}{}", env::var("HOME").unwrap_or_else(|_| "~".to_string()), &s[1..])
    } else {
        s.to_string()
    };
    
    Ok(Value::String(result))
}

// ============== Number Filters ==============

fn filter_length(value: &Value) -> FilterResult<Value> {
    let len = match value {
        Value::Array(arr) => arr.len(),
        Value::String(s) => s.chars().count(),
        Value::Object(obj) => obj.len(),
        _ => return Err(FilterError::InvalidInput {
            filter: "length".to_string(),
            message: "Expected array, string, or object".to_string(),
        }),
    };
    
    Ok(Value::Number(len.into()))
}

fn filter_bool(value: &Value) -> FilterResult<Value> {
    let result = match value {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty() && s.to_lowercase() != "false" && s != "0",
        Value::Array(arr) => !arr.is_empty(),
        Value::Object(obj) => !obj.is_empty(),
        Value::Null => false,
    };
    
    Ok(Value::Bool(result))
}

fn filter_int(value: &Value) -> FilterResult<Value> {
    let n = match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse().ok(),
        Value::Bool(b) => Some(if *b { 1 } else { 0 }),
        _ => None,
    };
    
    Ok(Value::Number(n.unwrap_or(0).into()))
}

fn filter_float(value: &Value) -> FilterResult<Value> {
    let n = match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    };
    
    Ok(Value::Number(serde_json::Number::from_f64(n.unwrap_or(0.0)).unwrap_or_else(|| 0.into())))
}

fn filter_string(value: &Value) -> FilterResult<Value> {
    let s = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) => "[...]".to_string(),
        Value::Object(_) => "{...}".to_string(),
    };
    
    Ok(Value::String(s))
}

fn filter_abs(value: &Value) -> FilterResult<Value> {
    let n = value.as_number()
        .and_then(|n| n.as_f64())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "abs".to_string(),
            message: "Expected number".to_string(),
        })?;
    
    Ok(Value::Number(serde_json::Number::from_f64(n.abs()).unwrap_or_else(|| 0.into())))
}

fn filter_round(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let n = value.as_number()
        .and_then(|n| n.as_f64())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "round".to_string(),
            message: "Expected number".to_string(),
        })?;
    
    let precision = args.get(0)
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as usize;
    
    let multiplier = 10_f64.powi(precision as i32);
    let rounded = (n * multiplier).round() / multiplier;
    
    Ok(Value::Number(serde_json::Number::from_f64(rounded).unwrap_or_else(|| 0.into())))
}

// ============== JSON/YAML Filters ==============

fn filter_to_json(value: &Value) -> FilterResult<Value> {
    let json = serde_json::to_string(value)
        .map_err(|e| FilterError::Json(e.to_string()))?;
    Ok(Value::String(json))
}

fn filter_from_json(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "from_json".to_string(),
        message: "Expected JSON string".to_string(),
    })?;
    
    let parsed: Value = serde_json::from_str(s)
        .map_err(|e| FilterError::Json(e.to_string()))?;
    Ok(parsed)
}

fn filter_to_yaml(value: &Value) -> FilterResult<Value> {
    let yaml = serde_yaml::to_string(value)
        .map_err(|e| FilterError::FilterFailed {
            filter: "to_yaml".to_string(),
            message: e.to_string(),
        })?;
    Ok(Value::String(yaml))
}

fn filter_from_yaml(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "from_yaml".to_string(),
        message: "Expected YAML string".to_string(),
    })?;
    
    let parsed: Value = serde_yaml::from_str(s)
        .map_err(|e| FilterError::FilterFailed {
            filter: "from_yaml".to_string(),
            message: e.to_string(),
        })?;
    Ok(parsed)
}

fn filter_to_nice_json(value: &Value) -> FilterResult<Value> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| FilterError::Json(e.to_string()))?;
    Ok(Value::String(json))
}

fn filter_to_nice_yaml(value: &Value) -> FilterResult<Value> {
    filter_to_yaml(value)
}

// ============== Base64 & Hashing Filters ==============

fn filter_b64encode(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "b64encode".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    use base64::Engine;
    Ok(Value::String(base64::engine::general_purpose::STANDARD.encode(s)))
}

fn filter_b64decode(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "b64decode".to_string(),
        message: "Expected base64 string".to_string(),
    })?;
    
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD.decode(s)
        .map_err(|e| FilterError::Base64(e.to_string()))?;
    
    String::from_utf8(decoded)
        .map(|s| Value::String(s))
        .map_err(|e| FilterError::Base64(e.to_string()))
}

fn filter_md5(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "md5".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let hash = md5::compute(s.as_bytes());
    Ok(Value::String(format!("{:x}", hash)))
}

fn filter_sha1(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "sha1".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    use sha1::{Sha1, Digest};
    let mut hasher = Sha1::new();
    hasher.update(s.as_bytes());
    let hash = hasher.finalize();
    Ok(Value::String(format!("{:x}", hash)))
}

fn filter_sha256(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "sha256".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let hash = hasher.finalize();
    Ok(Value::String(format!("{:x}", hash)))
}

fn filter_sha512(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "sha512".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    use sha2::{Sha512, Digest};
    let mut hasher = Sha512::new();
    hasher.update(s.as_bytes());
    let hash = hasher.finalize();
    Ok(Value::String(format!("{:x}", hash)))
}

// ============== List Filters ==============

fn filter_quote(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "quote".to_string(),
        message: "Expected string".to_string(),
    })?;
    
    let quoted = s.chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\'' => vec!['\\', '\''],
            '\\' => vec!['\\', '\\'],
            _ => vec![c],
        })
        .collect::<String>();
    
    Ok(Value::String(format!("\"{}\"", quoted)))
}

fn filter_shuffle(value: &Value) -> FilterResult<Value> {
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "shuffle".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let mut result = arr.clone();
    let mut rng = thread_rng();
    result.shuffle(&mut rng);
    
    Ok(Value::Array(result))
}

fn filter_sort(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "sort".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let mut result = arr.clone();
    result.sort_by(|a, b| {
        match (a.as_str(), b.as_str()) {
            (Some(sa), Some(sb)) => sa.cmp(sb),
            (Some(sa), None) => {
                if let Some(nb) = b.as_i64() {
                    sa.parse::<i64>().map(|ia| ia.cmp(&nb)).unwrap_or(std::cmp::Ordering::Greater)
                } else {
                    std::cmp::Ordering::Greater
                }
            }
            (None, Some(sb)) => {
                if let Some(na) = a.as_i64() {
                    sb.parse::<i64>().map(|ib| na.cmp(&ib)).unwrap_or(std::cmp::Ordering::Less)
                } else {
                    std::cmp::Ordering::Less
                }
            }
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    
    Ok(Value::Array(result))
}

fn filter_unique(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "unique".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let mut seen = std::collections::HashSet::new();
    let result: Vec<Value> = arr.iter()
        .filter(|v| seen.insert(v.to_string()))
        .cloned()
        .collect();
    
    Ok(Value::Array(result))
}

fn filter_min(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "min".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    if arr.is_empty() {
        return Ok(Value::Null);
    }
    
    let min = arr.iter().min_by(|a, b| {
        match (a.as_f64(), b.as_f64()) {
            (Some(fa), Some(fb)) => fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    
    Ok(min.cloned().unwrap_or(Value::Null))
}

fn filter_max(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "max".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    if arr.is_empty() {
        return Ok(Value::Null);
    }
    
    let max = arr.iter().max_by(|a, b| {
        match (a.as_f64(), b.as_f64()) {
            (Some(fa), Some(fb)) => fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    
    Ok(max.cloned().unwrap_or(Value::Null))
}

fn filter_sum(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "sum".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let sum: f64 = arr.iter()
        .filter_map(|v| v.as_f64())
        .sum();
    
    Ok(Value::Number(serde_json::Number::from_f64(sum).unwrap_or_else(|| 0.into())))
}

fn filter_product(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "product".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let product: f64 = arr.iter()
        .filter_map(|v| v.as_f64())
        .product();
    
    Ok(Value::Number(serde_json::Number::from_f64(product).unwrap_or_else(|| 0.into())))
}

fn filter_mean(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "mean".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    if arr.is_empty() {
        return Ok(Value::Null);
    }
    
    let sum: f64 = arr.iter()
        .filter_map(|v| v.as_f64())
        .sum();
    
    let mean = sum / arr.len() as f64;
    Ok(Value::Number(serde_json::Number::from_f64(mean).unwrap_or_else(|| 0.into())))
}

fn filter_median(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "median".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    if arr.is_empty() {
        return Ok(Value::Null);
    }
    
    let mut nums: Vec<f64> = arr.iter()
        .filter_map(|v| v.as_f64())
        .collect();
    
    nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    let median = if nums.len() % 2 == 0 {
        (nums[nums.len() / 2 - 1] + nums[nums.len() / 2]) / 2.0
    } else {
        nums[nums.len() / 2]
    };
    
    Ok(Value::Number(serde_json::Number::from_f64(median).unwrap_or_else(|| 0.into())))
}

fn filter_first(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "first".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    Ok(arr.first().cloned().unwrap_or(Value::Null))
}

fn filter_last(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "last".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    Ok(arr.last().cloned().unwrap_or(Value::Null))
}

fn filter_nth(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "nth".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let index = args.get(0)
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as usize;
    
    Ok(arr.get(index).cloned().unwrap_or(Value::Null))
}

fn filter_flatten(value: &Value) -> FilterResult<Value> {
    let mut result = Vec::new();
    
    fn flatten(arr: &[Value], result: &mut Vec<Value>) {
        for item in arr {
            if let Value::Array(inner) = item {
                flatten(inner, result);
            } else {
                result.push(item.clone());
            }
        }
    }
    
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "flatten".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    flatten(arr, &mut result);
    Ok(Value::Array(result))
}

// ============== Dict Filters ==============

fn filter_items(value: &Value) -> FilterResult<Value> {
    let obj = value.as_object().ok_or_else(|| FilterError::InvalidInput {
        filter: "items".to_string(),
        message: "Expected object".to_string(),
    })?;
    
    let result: Vec<Value> = obj.iter()
        .map(|(k, v)| {
            Value::Array(vec![Value::String(k.clone()), v.clone()])
        })
        .collect();
    
    Ok(Value::Array(result))
}

fn filter_dict2items(value: &Value) -> FilterResult<Value> {
    filter_items(value)
}

fn filter_items2dict(value: &Value) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "items2dict".to_string(),
        message: "Expected array of [key, value] pairs".to_string(),
    })?;
    
    let mut result = serde_json::Map::new();
    
    for item in arr {
        if let Value::Array(pair) = item {
            if pair.len() >= 2 {
                if let Some(key) = pair[0].as_str() {
                    result.insert(key.to_string(), pair[1].clone());
                }
            }
        }
    }
    
    Ok(Value::Object(result))
}

fn filter_combine(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let mut result = value.as_object()
        .cloned()
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "combine".to_string(),
            message: "Expected object".to_string(),
        })?;
    
    for arg in args {
        if let Value::Object(other) = arg {
            for (k, v) in other {
                result.insert(k.clone(), v.clone());
            }
        }
    }
    
    Ok(Value::Object(result))
}

fn filter_dict(args: &[Value]) -> FilterResult<Value> {
    let mut result = serde_json::Map::new();
    
    for arg in args {
        if let Value::Array(pair) = arg {
            if pair.len() >= 2 {
                if let Some(key) = pair[0].as_str() {
                    result.insert(key.to_string(), pair[1].clone());
                }
            }
        }
    }
    
    Ok(Value::Object(result))
}

// ============== Miscellaneous Filters ==============

fn filter_list(value: &Value) -> FilterResult<Value> {
    match value {
        Value::Array(arr) => Ok(value.clone()),
        Value::String(s) => {
            let result: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
            Ok(Value::Array(result))
        }
        Value::Object(obj) => {
            let result: Vec<Value> = obj.keys().map(|k| Value::String(k.clone())).collect();
            Ok(Value::Array(result))
        }
        _ => Ok(Value::Array(vec![value.clone()])),
    }
}

fn filter_range(value: &Value) -> FilterResult<Value> {
    let n = value.as_i64().ok_or_else(|| FilterError::InvalidInput {
        filter: "range".to_string(),
        message: "Expected integer".to_string(),
    })?;
    
    let result: Vec<Value> = (0..n).map(|i| Value::Number(i.into())).collect();
    Ok(Value::Array(result))
}

fn filter_zip(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let arr1 = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "zip".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let mut iterators: Vec<std::slice::Iter<Value>> = vec![arr1.iter()];
    for arg in args {
        if let Value::Array(arr) = arg {
            iterators.push(arr.iter());
        }
    }
    
    let mut result = Vec::new();
    loop {
        let mut row = Vec::new();
        let mut all_empty = true;
        
        for iter in &mut iterators {
            if let Some(item) = iter.next() {
                row.push(item.clone());
                all_empty = false;
            }
        }
        
        if all_empty {
            break;
        }
        
        if row.len() == iterators.len() {
            result.push(Value::Array(row));
        }
    }
    
    Ok(Value::Array(result))
}

fn filter_map(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "map".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let attr = args.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "map".to_string(),
            message: "Expected attribute name as first argument".to_string(),
        })?;
    
    let result: Vec<Value> = arr.iter()
        .filter_map(|v| {
            if let Value::Object(obj) = v {
                obj.get(attr).cloned()
            } else {
                None
            }
        })
        .collect();
    
    Ok(Value::Array(result))
}

fn filter_select(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "select".to_string(),
        message: "Expected array".to_string(),
    })?;
    
    let test = args.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "select".to_string(),
            message: "Expected test name as first argument".to_string(),
        })?;
    
    let result: Vec<Value> = arr.iter()
        .filter(|v| {
            match test {
                "defined" => !matches!(v, Value::Null),
                "undefined" => matches!(v, Value::Null),
                "truthy" => filter_bool(v).unwrap_or(Value::Bool(false)).as_bool().unwrap_or(false),
                "falsy" => !filter_bool(v).unwrap_or(Value::Bool(true)).as_bool().unwrap_or(true),
                _ => true,
            }
        })
        .cloned()
        .collect();
    
    Ok(Value::Array(result))
}

fn filter_reject(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let selected = filter_select(value, args)?;
    
    if let Value::Array(selected_arr) = selected {
        if let Value::Array(original_arr) = value {
            let selected_set: std::collections::HashSet<&Value> = selected_arr.iter().collect();
            let result: Vec<Value> = original_arr.iter()
                .filter(|v| !selected_set.contains(v))
                .cloned()
                .collect();
            return Ok(Value::Array(result));
        }
    }
    
    Ok(value.clone())
}

fn filter_selectattr(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "selectattr".to_string(),
        message: "Expected array of objects".to_string(),
    })?;
    
    let attr = args.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "selectattr".to_string(),
            message: "Expected attribute name as first argument".to_string(),
        })?;
    
    let result: Vec<Value> = arr.iter()
        .filter(|v| {
            if let Value::Object(obj) = v {
                obj.contains_key(attr)
            } else {
                false
            }
        })
        .cloned()
        .collect();
    
    Ok(Value::Array(result))
}

fn filter_rejectattr(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let selected = filter_selectattr(value, args)?;
    
    if let Value::Array(selected_arr) = selected {
        if let Value::Array(original_arr) = value {
            let selected_set: std::collections::HashSet<&Value> = selected_arr.iter().collect();
            let result: Vec<Value> = original_arr.iter()
                .filter(|v| !selected_set.contains(v))
                .cloned()
                .collect();
            return Ok(Value::Array(result));
        }
    }
    
    Ok(value.clone())
}

fn filter_groupby(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let arr = value.as_array().ok_or_else(|| FilterError::InvalidInput {
        filter: "groupby".to_string(),
        message: "Expected array of objects".to_string(),
    })?;
    
    let attr = args.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| FilterError::InvalidInput {
            filter: "groupby".to_string(),
            message: "Expected attribute name as first argument".to_string(),
        })?;
    
    let mut groups: std::collections::HashMap<String, Vec<Value>> = std::collections::HashMap::new();
    
    for item in arr {
        if let Value::Object(obj) = item {
            if let Some(key_value) = obj.get(attr) {
                let key = key_value.to_string();
                groups.entry(key).or_insert_with(Vec::new).push(item.clone());
            }
        }
    }
    
    let result: Vec<Value> = groups.into_iter()
        .map(|(key, items)| {
            Value::Array(vec![Value::String(key), Value::Array(items)])
        })
        .collect();
    
    Ok(Value::Array(result))
}

// ============== Date/Time Filters ==============

fn filter_strftime(value: &Value, args: &[Value]) -> FilterResult<Value> {
    let format = args.get(0)
        .and_then(|v| v.as_str())
        .unwrap_or("%Y-%m-%d %H:%M:%S");
    
    let dt = match value {
        Value::String(s) => {
            // Try parsing ISO 8601
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }
        Value::Number(n) => {
            // Unix timestamp
            n.as_i64().map(|ts| Utc.timestamp_opt(ts, 0).single()).flatten()
        }
        _ => None,
    };
    
    let dt = dt.ok_or_else(|| FilterError::InvalidDateTime("Invalid datetime format".to_string()))?;
    
    Ok(Value::String(dt.format(format).to_string()))
}

fn filter_to_datetime(value: &Value) -> FilterResult<Value> {
    let s = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "to_datetime".to_string(),
        message: "Expected datetime string".to_string(),
    })?;
    
    let dt = DateTime::parse_from_rfc3339(s)
        .map_err(|_| FilterError::InvalidDateTime("Invalid datetime format".to_string()))?;
    
    Ok(Value::String(dt.to_rfc3339()))
}

fn filter_timestamp(value: &Value) -> FilterResult<Value> {
    match value {
        Value::String(s) => {
            let dt = DateTime::parse_from_rfc3339(s)
                .map_err(|_| FilterError::InvalidDateTime("Invalid datetime format".to_string()))?;
            Ok(Value::Number(dt.timestamp().into()))
        }
        Value::Number(n) => Ok(value.clone()),
        _ => Err(FilterError::InvalidInput {
            filter: "timestamp".to_string(),
            message: "Expected datetime string or number".to_string(),
        }),
    }
}

// ============== Other Filters ==============

fn filter_bool_filter(value: &Value) -> FilterResult<Value> {
    filter_bool(value)
}

fn filter_mandatory(value: &Value) -> FilterResult<Value> {
    match value {
        Value::Null => Err(FilterError::InvalidInput {
            filter: "mandatory".to_string(),
            message: "Mandatory value is null or undefined".to_string(),
        }),
        Value::String(s) if s.is_empty() => Err(FilterError::InvalidInput {
            filter: "mandatory".to_string(),
            message: "Mandatory value is empty".to_string(),
        }),
        _ => Ok(value.clone()),
    }
}

fn filter_env(value: &Value) -> FilterResult<Value> {
    let key = value.as_str().ok_or_else(|| FilterError::InvalidInput {
        filter: "env".to_string(),
        message: "Expected environment variable name".to_string(),
    })?;
    
    Ok(Value::String(env::var(key).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upper_filter() {
        let value = Value::String("hello".to_string());
        let result = apply_filter("upper", &value, &[]).unwrap();
        assert_eq!(result, Value::String("HELLO".to_string()));
    }

    #[test]
    fn test_lower_filter() {
        let value = Value::String("HELLO".to_string());
        let result = apply_filter("lower", &value, &[]).unwrap();
        assert_eq!(result, Value::String("hello".to_string()));
    }

    #[test]
    fn test_split_filter() {
        let value = Value::String("a,b,c".to_string());
        let result = apply_filter("split", &value, &[Value::String(",".to_string())]).unwrap();
        assert_eq!(result, Value::Array(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            Value::String("c".to_string()),
        ]));
    }

    #[test]
    fn test_join_filter() {
        let value = Value::Array(vec![
            Value::String("a".to_string()),
            Value::String("b".to_string()),
            Value::String("c".to_string()),
        ]);
        let result = apply_filter("join", &value, &[Value::String(",".to_string())]).unwrap();
        assert_eq!(result, Value::String("a,b,c".to_string()));
    }

    #[test]
    fn test_to_json_filter() {
        let mut map = serde_json::Map::new();
        map.insert("key".to_string(), Value::String("value".to_string()));
        let value = Value::Object(map);
        
        let result = apply_filter("to_json", &value, &[]).unwrap();
        assert_eq!(result, Value::String(r#"{"key":"value"}"#.to_string()));
    }

    #[test]
    fn test_b64encode_filter() {
        let value = Value::String("hello".to_string());
        let result = apply_filter("b64encode", &value, &[]).unwrap();
        assert_eq!(result, Value::String("aGVsbG8=".to_string()));
    }
}
