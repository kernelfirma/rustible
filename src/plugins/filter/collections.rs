//! Collection manipulation filters for Jinja2 templates.
//!
//! This module provides filters for working with lists, dictionaries, and
//! performing set operations, compatible with Ansible's Jinja2 collection filters.
//!
//! # Available Filters
//!
//! - `combine`: Merge dictionaries together
//! - `union`: Combine lists, removing duplicates
//! - `difference`: Elements in first list but not in second
//! - `intersect`: Elements common to both lists
//! - `symmetric_difference`: Elements in either list but not both
//! - `unique`: Remove duplicates from a list
//! - `flatten`: Flatten nested lists
//! - `zip`: Combine multiple lists element-wise
//! - `zip_longest`: Like zip, but uses fillvalue for shorter lists
//! - `dict2items`: Convert dictionary to list of key-value pairs
//! - `items2dict`: Convert list of key-value pairs to dictionary
//! - `subelements`: Create combinations of items with subelements
//! - `groupby`: Group items by attribute
//! - `map_attribute`: Extract attribute from list of objects
//! - `selectattr`: Filter by attribute value
//! - `rejectattr`: Reject items by attribute value
//!
//! # Examples
//!
//! ```jinja2
//! {{ dict1 | combine(dict2) }}
//! {{ list1 | union(list2) }}
//! {{ list1 | difference(list2) }}
//! {{ data | dict2items }}
//! ```

use minijinja::value::ValueKind;
use minijinja::{Environment, Value};
use std::collections::{BTreeMap, HashSet};

trait ValueSeqExt {
    fn as_seq(&self) -> Option<Vec<Value>>;
}

impl ValueSeqExt for Value {
    fn as_seq(&self) -> Option<Vec<Value>> {
        match self.kind() {
            ValueKind::Seq | ValueKind::Iterable => self.try_iter().ok().map(|iter| iter.collect()),
            _ => None,
        }
    }
}

/// Register all collection filters with the given environment.
pub fn register_filters(env: &mut Environment<'static>) {
    env.add_filter("combine", combine);
    env.add_filter("union", union);
    env.add_filter("difference", difference);
    env.add_filter("intersect", intersect);
    env.add_filter("symmetric_difference", symmetric_difference);
    env.add_filter("unique", unique);
    env.add_filter("flatten", flatten);
    env.add_filter("zip", zip_filter);
    env.add_filter("zip_longest", zip_longest);
    env.add_filter("dict2items", dict2items);
    env.add_filter("items2dict", items2dict);
    env.add_filter("subelements", subelements);
    env.add_filter("groupby", groupby);
    env.add_filter("map_attribute", map_attribute);
    env.add_filter("selectattr", selectattr);
    env.add_filter("rejectattr", rejectattr);
    env.add_filter("product", product);
    env.add_filter("batch", batch);
    env.add_filter("slice", slice_filter);
}

/// Merge dictionaries together.
///
/// # Arguments
///
/// * `base` - The base dictionary
/// * `other` - The dictionary to merge into base
/// * `recursive` - Optional: merge nested dicts recursively (default: false)
/// * `list_merge` - Optional: how to merge lists ("replace", "keep", "append", "prepend", "append_rp", "prepend_rp")
///
/// # Returns
///
/// A new dictionary with values from `other` overlaid on `base`.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `combine` filter.
fn combine(
    base: Value,
    other: Value,
    recursive: Option<bool>,
    list_merge: Option<String>,
) -> Value {
    let recursive = recursive.unwrap_or(false);
    let list_merge = list_merge.unwrap_or_else(|| "replace".to_string());

    if let (Some(base_obj), Some(other_obj)) = (base.as_object(), other.as_object()) {
        let mut result = BTreeMap::new();

        // First, add all keys from base
        if let Some(iter) = base_obj.try_iter_pairs() {
            for (key, val) in iter {
                result.insert(key.to_string(), val);
            }
        }

        // Then, overlay/merge keys from other
        if let Some(iter) = other_obj.try_iter_pairs() {
            for (key, other_val) in iter {
                let key_str = key.to_string();
                let merged_val = if recursive {
                    if let Some(base_val) = result.get(&key_str).cloned() {
                        merge_values(base_val, other_val, &list_merge)
                    } else {
                        other_val
                    }
                } else {
                    other_val
                };
                result.insert(key_str, merged_val);
            }
        }

        Value::from_iter(result)
    } else {
        base
    }
}

fn merge_values(base: Value, other: Value, list_merge: &str) -> Value {
    match (base.as_object(), other.as_object()) {
        (Some(base_obj), Some(other_obj)) => {
            // Both are dicts, merge recursively
            let mut result = BTreeMap::new();
            if let Some(iter) = base_obj.try_iter_pairs() {
                for (key, val) in iter {
                    result.insert(key.to_string(), val);
                }
            }
            if let Some(iter) = other_obj.try_iter_pairs() {
                for (key, other_val) in iter {
                    let key_str = key.to_string();
                    let merged = if let Some(base_val) = result.get(&key_str).cloned() {
                        merge_values(base_val, other_val, list_merge)
                    } else {
                        other_val
                    };
                    result.insert(key_str, merged);
                }
            }
            Value::from_iter(result)
        }
        _ => {
            // Handle list merging
            if let (Some(base_seq), Some(other_seq)) = (base.as_seq(), other.as_seq()) {
                match list_merge {
                    "append" => {
                        let mut result = base_seq;
                        result.extend(other_seq);
                        Value::from(result)
                    }
                    "prepend" => {
                        let mut result = other_seq;
                        result.extend(base_seq);
                        Value::from(result)
                    }
                    "append_rp" => {
                        // Append with remove duplicates from base that exist in other
                        let other_set: HashSet<String> =
                            other_seq.iter().map(|v| v.to_string()).collect();
                        let mut result: Vec<Value> = base_seq
                            .into_iter()
                            .filter(|v| !other_set.contains(&v.to_string()))
                            .collect();
                        result.extend(other_seq);
                        Value::from(result)
                    }
                    "prepend_rp" => {
                        // Prepend with remove duplicates from base that exist in other
                        let other_set: HashSet<String> =
                            other_seq.iter().map(|v| v.to_string()).collect();
                        let mut result = other_seq;
                        result.extend(base_seq.into_iter().filter(|v| {
                            !other_set.contains(&v.to_string())
                        }));
                        Value::from(result)
                    }
                    "keep" => base,
                    _ => other, // "replace" or default
                }
            } else {
                other
            }
        }
    }
}

/// Combine lists, removing duplicates.
///
/// # Arguments
///
/// * `list1` - First list
/// * `list2` - Second list
///
/// # Returns
///
/// A new list with unique elements from both lists.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `union` filter.
fn union(list1: Value, list2: Value) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    if let Some(seq1) = list1.as_seq() {
        for item in seq1.iter() {
            let key = item.to_string();
            if seen.insert(key) {
                result.push(item.clone());
            }
        }
    }

    if let Some(seq2) = list2.as_seq() {
        for item in seq2.iter() {
            let key = item.to_string();
            if seen.insert(key) {
                result.push(item.clone());
            }
        }
    }

    result
}

/// Get elements in first list but not in second.
///
/// # Arguments
///
/// * `list1` - First list
/// * `list2` - Second list
///
/// # Returns
///
/// Elements that are in `list1` but not in `list2`.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `difference` filter.
fn difference(list1: Value, list2: Value) -> Vec<Value> {
    let set2: HashSet<String> = list2
        .as_seq()
        .map(|s| s.iter().map(|v| v.to_string()).collect())
        .unwrap_or_default();

    list1
        .as_seq()
        .map(|s| {
            s.iter()
                .filter(|v| !set2.contains(&v.to_string()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Get elements common to both lists.
///
/// # Arguments
///
/// * `list1` - First list
/// * `list2` - Second list
///
/// # Returns
///
/// Elements that exist in both lists.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `intersect` filter.
fn intersect(list1: Value, list2: Value) -> Vec<Value> {
    let set2: HashSet<String> = list2
        .as_seq()
        .map(|s| s.iter().map(|v| v.to_string()).collect())
        .unwrap_or_default();

    list1
        .as_seq()
        .map(|s| {
            s.iter()
                .filter(|v| set2.contains(&v.to_string()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Get elements in either list but not in both.
///
/// # Arguments
///
/// * `list1` - First list
/// * `list2` - Second list
///
/// # Returns
///
/// Elements unique to each list (XOR).
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `symmetric_difference` filter.
fn symmetric_difference(list1: Value, list2: Value) -> Vec<Value> {
    let set1: HashSet<String> = list1
        .as_seq()
        .map(|s| s.iter().map(|v| v.to_string()).collect())
        .unwrap_or_default();

    let set2: HashSet<String> = list2
        .as_seq()
        .map(|s| s.iter().map(|v| v.to_string()).collect())
        .unwrap_or_default();

    let mut result = Vec::new();

    if let Some(seq1) = list1.as_seq() {
        for item in seq1.iter() {
            if !set2.contains(&item.to_string()) {
                result.push(item.clone());
            }
        }
    }

    if let Some(seq2) = list2.as_seq() {
        for item in seq2.iter() {
            if !set1.contains(&item.to_string()) {
                result.push(item.clone());
            }
        }
    }

    result
}

/// Remove duplicates from a list.
///
/// # Arguments
///
/// * `list` - The list to deduplicate
/// * `case_sensitive` - Optional: case-sensitive comparison (default: true)
///
/// # Returns
///
/// A list with duplicate values removed, preserving order.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `unique` filter.
fn unique(list: Value, case_sensitive: Option<bool>) -> Vec<Value> {
    let case_sensitive = case_sensitive.unwrap_or(true);
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    if let Some(seq) = list.as_seq() {
        for item in seq.iter() {
            let key = if case_sensitive {
                item.to_string()
            } else {
                item.to_string().to_lowercase()
            };
            if seen.insert(key) {
                result.push(item.clone());
            }
        }
    }

    result
}

/// Flatten nested lists.
///
/// # Arguments
///
/// * `list` - The nested list to flatten
/// * `levels` - Optional: number of levels to flatten (default: all)
///
/// # Returns
///
/// A flattened list.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `flatten` filter.
fn flatten(list: Value, levels: Option<i64>) -> Vec<Value> {
    fn flatten_recursive(value: &Value, depth: i64, max_depth: Option<i64>, result: &mut Vec<Value>) {
        if let Some(max) = max_depth {
            if depth > max {
                result.push(value.clone());
                return;
            }
        }

        if let Some(seq) = value.as_seq() {
            for item in seq.iter() {
                flatten_recursive(item, depth + 1, max_depth, result);
            }
        } else {
            result.push(value.clone());
        }
    }

    let mut result = Vec::new();
    flatten_recursive(&list, 0, levels, &mut result);
    result
}

/// Combine lists element-wise.
///
/// # Arguments
///
/// * `list1` - First list
/// * `list2` - Second list
///
/// # Returns
///
/// A list of pairs, stopping at the shorter list.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `zip` filter.
fn zip_filter(list1: Value, list2: Value) -> Vec<Value> {
    let seq1 = list1.as_seq();
    let seq2 = list2.as_seq();

    match (seq1, seq2) {
        (Some(s1), Some(s2)) => s1
            .iter()
            .zip(s2.iter())
            .map(|(a, b)| Value::from(vec![a.clone(), b.clone()]))
            .collect(),
        _ => Vec::new(),
    }
}

/// Combine lists element-wise, filling shorter lists.
///
/// # Arguments
///
/// * `list1` - First list
/// * `list2` - Second list
/// * `fillvalue` - Optional: value to use for missing elements (default: none)
///
/// # Returns
///
/// A list of pairs, continuing to the longer list.
fn zip_longest(list1: Value, list2: Value, fillvalue: Option<Value>) -> Vec<Value> {
    let fillvalue = fillvalue.unwrap_or(Value::from(()));

    let seq1 = list1.as_seq().unwrap_or_default();
    let seq2 = list2.as_seq().unwrap_or_default();

    let max_len = seq1.len().max(seq2.len());
    let mut result = Vec::new();

    for i in 0..max_len {
        let a = seq1.get(i).cloned().unwrap_or_else(|| fillvalue.clone());
        let b = seq2.get(i).cloned().unwrap_or_else(|| fillvalue.clone());
        result.push(Value::from(vec![a, b]));
    }

    result
}

/// Convert dictionary to list of key-value pairs.
///
/// # Arguments
///
/// * `dict` - The dictionary to convert
/// * `key_name` - Optional: name for the key field (default: "key")
/// * `value_name` - Optional: name for the value field (default: "value")
///
/// # Returns
///
/// A list of objects with key and value fields.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `dict2items` filter.
fn dict2items(dict: Value, key_name: Option<String>, value_name: Option<String>) -> Vec<Value> {
    let key_name = key_name.unwrap_or_else(|| "key".to_string());
    let value_name = value_name.unwrap_or_else(|| "value".to_string());

    if let Some(obj) = dict.as_object() {
        if let Some(iter) = obj.try_iter_pairs() {
            return iter
                .map(|(k, v)| {
                    Value::from_iter([
                        (key_name.clone(), k),
                        (value_name.clone(), v),
                    ])
                })
                .collect();
        }
    }
    Vec::new()
}

/// Convert list of key-value pairs to dictionary.
///
/// # Arguments
///
/// * `list` - The list of key-value objects
/// * `key_name` - Optional: name of the key field (default: "key")
/// * `value_name` - Optional: name of the value field (default: "value")
///
/// # Returns
///
/// A dictionary constructed from the key-value pairs.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `items2dict` filter.
fn items2dict(list: Value, key_name: Option<String>, value_name: Option<String>) -> Value {
    let key_name = key_name.unwrap_or_else(|| "key".to_string());
    let value_name = value_name.unwrap_or_else(|| "value".to_string());

    if let Some(seq) = list.as_seq() {
        let mut result = BTreeMap::new();
        for item in seq.iter() {
            if let Some(obj) = item.as_object() {
                if let (Some(k), Some(v)) = (
                    obj.get_value(&Value::from(key_name.clone())),
                    obj.get_value(&Value::from(value_name.clone())),
                ) {
                    result.insert(k.to_string(), v);
                }
            }
        }
        Value::from_iter(result)
    } else {
        Value::from(BTreeMap::<String, Value>::new())
    }
}

/// Create combinations of items with subelements.
///
/// # Arguments
///
/// * `list` - List of objects
/// * `key` - Key of the subelement list
///
/// # Returns
///
/// A list of [item, subelement] pairs.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `subelements` filter.
fn subelements(list: Value, key: String) -> Vec<Value> {
    let mut result = Vec::new();

    if let Some(seq) = list.as_seq() {
        for item in seq.iter() {
            if let Some(obj) = item.as_object() {
                if let Some(sub) = obj.get_value(&Value::from(key.clone())) {
                    if let Some(sub_seq) = sub.as_seq() {
                        for sub_item in sub_seq.iter() {
                            result.push(Value::from(vec![item.clone(), sub_item.clone()]));
                        }
                    }
                }
            }
        }
    }

    result
}

/// Group items by attribute.
///
/// # Arguments
///
/// * `list` - List of objects
/// * `attr` - Attribute to group by
///
/// # Returns
///
/// A list of objects with `grouper` and `list` fields.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `groupby` filter.
fn groupby(list: Value, attr: String) -> Vec<Value> {
    if let Some(seq) = list.as_seq() {
        let mut groups: indexmap::IndexMap<String, Vec<Value>> = indexmap::IndexMap::new();

        for item in seq.iter() {
            let key = if let Some(obj) = item.as_object() {
                obj.get_value(&Value::from(attr.clone()))
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            groups.entry(key).or_default().push(item.clone());
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
}

/// Extract attribute from list of objects.
///
/// # Arguments
///
/// * `list` - List of objects
/// * `attr` - Attribute to extract
/// * `default` - Optional default value if attribute is missing
///
/// # Returns
///
/// A list of attribute values.
fn map_attribute(list: Value, attr: String, default: Option<Value>) -> Vec<Value> {
    let default = default.unwrap_or(Value::UNDEFINED);

    if let Some(seq) = list.as_seq() {
        seq.iter()
            .map(|item| {
                if let Some(obj) = item.as_object() {
                    obj.get_value(&Value::from(attr.clone()))
                        .unwrap_or_else(|| default.clone())
                } else {
                    default.clone()
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Filter items by attribute value.
///
/// # Arguments
///
/// * `list` - List of objects
/// * `attr` - Attribute to test
/// * `test` - Optional test to apply (default: truthy)
/// * `value` - Optional value to compare against
///
/// # Returns
///
/// Items where the attribute passes the test.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `selectattr` filter.
fn selectattr(list: Value, attr: String, test: Option<String>, value: Option<Value>) -> Vec<Value> {
    if let Some(seq) = list.as_seq() {
        seq.iter()
            .filter(|item| {
                if let Some(obj) = item.as_object() {
                    if let Some(attr_val) = obj.get_value(&Value::from(attr.clone())) {
                        apply_test(&attr_val, test.as_deref(), value.as_ref())
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    }
}

/// Reject items by attribute value.
///
/// # Arguments
///
/// * `list` - List of objects
/// * `attr` - Attribute to test
/// * `test` - Optional test to apply (default: truthy)
/// * `value` - Optional value to compare against
///
/// # Returns
///
/// Items where the attribute fails the test.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `rejectattr` filter.
fn rejectattr(list: Value, attr: String, test: Option<String>, value: Option<Value>) -> Vec<Value> {
    if let Some(seq) = list.as_seq() {
        seq.iter()
            .filter(|item| {
                if let Some(obj) = item.as_object() {
                    if let Some(attr_val) = obj.get_value(&Value::from(attr.clone())) {
                        !apply_test(&attr_val, test.as_deref(), value.as_ref())
                    } else {
                        true
                    }
                } else {
                    true
                }
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    }
}

fn apply_test(val: &Value, test: Option<&str>, compare: Option<&Value>) -> bool {
    match test {
        Some("equalto" | "==" | "eq") => {
            compare.map(|c| val.to_string() == c.to_string()).unwrap_or(false)
        }
        Some("ne" | "!=") => {
            compare.map(|c| val.to_string() != c.to_string()).unwrap_or(true)
        }
        Some("defined") => !val.is_undefined(),
        Some("undefined") => val.is_undefined(),
        Some("none" | "null") => val.is_none(),
        Some("true" | "truthy") => val.is_true(),
        Some("false" | "falsy") => !val.is_true(),
        Some("in") => {
            if let Some(list) = compare.and_then(|c| c.as_seq()) {
                list.iter().any(|item| item.to_string() == val.to_string())
            } else {
                false
            }
        }
        Some("contains") => {
            if let Some(seq) = val.as_seq() {
                compare.map(|c| seq.iter().any(|item| item.to_string() == c.to_string())).unwrap_or(false)
            } else if let Some(s) = val.as_str() {
                compare.and_then(|c| c.as_str()).map(|substr| s.contains(substr)).unwrap_or(false)
            } else {
                false
            }
        }
        None | Some(_) => val.is_true(),
    }
}

/// Compute Cartesian product of lists.
///
/// # Arguments
///
/// * `list1` - First list
/// * `list2` - Second list
///
/// # Returns
///
/// All combinations of elements from both lists.
fn product(list1: Value, list2: Value) -> Vec<Value> {
    let seq1 = list1.as_seq().unwrap_or_default();
    let seq2 = list2.as_seq().unwrap_or_default();

    let mut result = Vec::new();
    for a in &seq1 {
        for b in &seq2 {
            result.push(Value::from(vec![a.clone(), b.clone()]));
        }
    }
    result
}

/// Split list into fixed-size batches.
///
/// # Arguments
///
/// * `list` - The list to batch
/// * `size` - Batch size
/// * `fill` - Optional value to fill incomplete batches
///
/// # Returns
///
/// A list of batches.
fn batch(list: Value, size: i64, fill: Option<Value>) -> Vec<Value> {
    let size = size.max(1) as usize;

    if let Some(seq) = list.as_seq() {
        let items = seq;
        let mut result = Vec::new();

        for chunk in items.chunks(size) {
            let mut batch_items: Vec<Value> = chunk.to_vec();
            if let Some(ref fill_val) = fill {
                while batch_items.len() < size {
                    batch_items.push(fill_val.clone());
                }
            }
            result.push(Value::from(batch_items));
        }

        result
    } else {
        Vec::new()
    }
}

/// Extract a slice of a list.
///
/// # Arguments
///
/// * `list` - The list to slice
/// * `start` - Start index
/// * `end` - Optional end index
/// * `step` - Optional step
///
/// # Returns
///
/// The sliced portion of the list.
fn slice_filter(list: Value, start: i64, end: Option<i64>, step: Option<i64>) -> Vec<Value> {
    if let Some(seq) = list.as_seq() {
        let items = seq;
        let len = items.len() as i64;

        // Handle negative indices
        let start = if start < 0 { (len + start).max(0) } else { start.min(len) } as usize;
        let end = end
            .map(|e| if e < 0 { (len + e).max(0) } else { e.min(len) } as usize)
            .unwrap_or(len as usize);
        let step = step.unwrap_or(1).max(1) as usize;

        items[start..end].iter().step_by(step).cloned().collect()
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combine_basic() {
        let base = Value::from_iter([
            ("a".to_string(), Value::from(1)),
            ("b".to_string(), Value::from(2)),
        ]);
        let other = Value::from_iter([
            ("b".to_string(), Value::from(3)),
            ("c".to_string(), Value::from(4)),
        ]);

        let result = combine(base, other, None, None);
        assert!(!result.is_undefined());
    }

    #[test]
    fn test_union() {
        let list1 = Value::from(vec![Value::from(1), Value::from(2), Value::from(3)]);
        let list2 = Value::from(vec![Value::from(2), Value::from(3), Value::from(4)]);

        let result = union(list1, list2);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_difference() {
        let list1 = Value::from(vec![Value::from(1), Value::from(2), Value::from(3)]);
        let list2 = Value::from(vec![Value::from(2), Value::from(3), Value::from(4)]);

        let result = difference(list1, list2);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].to_string(), "1");
    }

    #[test]
    fn test_intersect() {
        let list1 = Value::from(vec![Value::from(1), Value::from(2), Value::from(3)]);
        let list2 = Value::from(vec![Value::from(2), Value::from(3), Value::from(4)]);

        let result = intersect(list1, list2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_symmetric_difference() {
        let list1 = Value::from(vec![Value::from(1), Value::from(2), Value::from(3)]);
        let list2 = Value::from(vec![Value::from(2), Value::from(3), Value::from(4)]);

        let result = symmetric_difference(list1, list2);
        assert_eq!(result.len(), 2); // 1 and 4
    }

    #[test]
    fn test_unique() {
        let list = Value::from(vec![
            Value::from(1),
            Value::from(2),
            Value::from(2),
            Value::from(3),
            Value::from(1),
        ]);

        let result = unique(list, None);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_flatten() {
        let nested = Value::from(vec![
            Value::from(1),
            Value::from(vec![Value::from(2), Value::from(3)]),
            Value::from(vec![
                Value::from(4),
                Value::from(vec![Value::from(5)]),
            ]),
        ]);

        let result = flatten(nested, None);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_zip() {
        let list1 = Value::from(vec![Value::from("a"), Value::from("b"), Value::from("c")]);
        let list2 = Value::from(vec![Value::from(1), Value::from(2), Value::from(3)]);

        let result = zip_filter(list1, list2);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_dict2items() {
        let dict = Value::from_iter([
            ("a".to_string(), Value::from(1)),
            ("b".to_string(), Value::from(2)),
        ]);

        let result = dict2items(dict, None, None);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_items2dict() {
        let items = Value::from(vec![
            Value::from_iter([
                ("key".to_string(), Value::from("a")),
                ("value".to_string(), Value::from(1)),
            ]),
            Value::from_iter([
                ("key".to_string(), Value::from("b")),
                ("value".to_string(), Value::from(2)),
            ]),
        ]);

        let result = items2dict(items, None, None);
        assert!(!result.is_undefined());
    }

    #[test]
    fn test_groupby() {
        let items = Value::from(vec![
            Value::from_iter([
                ("name".to_string(), Value::from("alice")),
                ("group".to_string(), Value::from("A")),
            ]),
            Value::from_iter([
                ("name".to_string(), Value::from("bob")),
                ("group".to_string(), Value::from("B")),
            ]),
            Value::from_iter([
                ("name".to_string(), Value::from("charlie")),
                ("group".to_string(), Value::from("A")),
            ]),
        ]);

        let result = groupby(items, "group".to_string());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_batch() {
        let list = Value::from(vec![
            Value::from(1),
            Value::from(2),
            Value::from(3),
            Value::from(4),
            Value::from(5),
        ]);

        let result = batch(list, 2, None);
        assert_eq!(result.len(), 3); // [1,2], [3,4], [5]
    }

    #[test]
    fn test_slice() {
        let list = Value::from(vec![
            Value::from(0),
            Value::from(1),
            Value::from(2),
            Value::from(3),
            Value::from(4),
        ]);

        let result = slice_filter(list, 1, Some(4), None);
        assert_eq!(result.len(), 3); // [1, 2, 3]
    }

    #[test]
    fn test_product() {
        let list1 = Value::from(vec![Value::from("a"), Value::from("b")]);
        let list2 = Value::from(vec![Value::from(1), Value::from(2)]);

        let result = product(list1, list2);
        assert_eq!(result.len(), 4); // (a,1), (a,2), (b,1), (b,2)
    }
}
