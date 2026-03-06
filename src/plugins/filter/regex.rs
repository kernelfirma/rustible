//! Regular expression filters for Jinja2 templates.
//!
//! This module provides regex-based string manipulation filters that are
//! compatible with Ansible's Jinja2 regex filters.
//!
//! # Available Filters
//!
//! - `regex_search`: Search for a pattern in a string (returns bool or match)
//! - `regex_replace`: Replace matches with a replacement string
//! - `regex_findall`: Find all matches of a pattern
//! - `regex_escape`: Escape special regex characters
//! - `regex_split`: Split string by regex pattern
//!
//! # Examples
//!
//! ```jinja2
//! {{ 'hello123world' | regex_search('[0-9]+') }}
//! {{ 'hello world' | regex_replace('world', 'universe') }}
//! {{ 'a1b2c3' | regex_findall('[0-9]+') }}
//! {{ 'a.b*c' | regex_escape }}
//! ```

use minijinja::{Environment, Value};

/// Register all regex filters with the given environment.
pub fn register_filters(env: &mut Environment<'static>) {
    env.add_filter("regex_search", regex_search);
    env.add_filter("regex_replace", regex_replace);
    env.add_filter("regex_findall", regex_findall);
    env.add_filter("regex_escape", regex_escape);
    env.add_filter("regex_split", regex_split);
    env.add_filter("regex_match", regex_match);
}

/// Search for a pattern in a string.
///
/// # Arguments
///
/// * `input` - The string to search in
/// * `pattern` - The regex pattern to search for
/// * `ignorecase` - Optional: case-insensitive matching (default: false)
/// * `multiline` - Optional: multiline mode (default: false)
///
/// # Returns
///
/// If the pattern contains groups, returns the first captured group.
/// Otherwise, returns a boolean indicating if the pattern was found.
///
/// # Ansible Compatibility
///
/// This filter is compatible with Ansible's `regex_search` filter.
/// When groups are present in the pattern, the first group match is returned.
fn regex_search(
    input: String,
    pattern: String,
    ignorecase: Option<bool>,
    multiline: Option<bool>,
) -> Value {
    let pattern = build_pattern(
        &pattern,
        ignorecase.unwrap_or(false),
        multiline.unwrap_or(false),
    );

    match crate::utils::get_regex(&pattern) {
        Ok(re) => {
            if let Some(caps) = re.captures(&input) {
                // If there are capture groups, return the first one
                if caps.len() > 1 {
                    if let Some(m) = caps.get(1) {
                        return Value::from(m.as_str().to_string());
                    }
                }
                // Otherwise return the full match
                if let Some(m) = caps.get(0) {
                    return Value::from(m.as_str().to_string());
                }
            }
            Value::from("")
        }
        Err(_) => Value::from(""),
    }
}

/// Search for a pattern and return whether it matches (boolean).
///
/// This is a simpler version that always returns a boolean.
fn regex_match(input: String, pattern: String, ignorecase: Option<bool>) -> bool {
    let pattern = build_pattern(&pattern, ignorecase.unwrap_or(false), false);

    match crate::utils::get_regex(&pattern) {
        Ok(re) => re.is_match(&input),
        Err(_) => false,
    }
}

/// Replace all occurrences of a pattern with a replacement string.
///
/// # Arguments
///
/// * `input` - The string to perform replacements in
/// * `pattern` - The regex pattern to match
/// * `replacement` - The replacement string (supports backreferences like $1, $2)
/// * `ignorecase` - Optional: case-insensitive matching (default: false)
/// * `count` - Optional: maximum number of replacements (default: all)
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `regex_replace` filter. Supports backreferences
/// using `$1`, `$2`, etc. or `\1`, `\2`, etc.
fn regex_replace(
    input: String,
    pattern: String,
    replacement: String,
    ignorecase: Option<bool>,
    count: Option<usize>,
) -> String {
    let pattern = build_pattern(&pattern, ignorecase.unwrap_or(false), false);

    // Convert Ansible-style backreferences (\1, \2) to Rust regex style ($1, $2)
    let replacement = replacement
        .replace("\\1", "$1")
        .replace("\\2", "$2")
        .replace("\\3", "$3")
        .replace("\\4", "$4")
        .replace("\\5", "$5")
        .replace("\\6", "$6")
        .replace("\\7", "$7")
        .replace("\\8", "$8")
        .replace("\\9", "$9");

    match crate::utils::get_regex(&pattern) {
        Ok(re) => match count {
            Some(n) if n > 0 => {
                let mut result = input.clone();
                for _ in 0..n {
                    if let Some(mat) = re.find(&result) {
                        let before = &result[..mat.start()];
                        let after = &result[mat.end()..];
                        let replaced = re.replace(mat.as_str(), replacement.as_str());
                        result = format!("{}{}{}", before, replaced, after);
                    } else {
                        break;
                    }
                }
                result
            }
            _ => re.replace_all(&input, replacement.as_str()).to_string(),
        },
        Err(_) => input,
    }
}

/// Find all matches of a pattern in a string.
///
/// # Arguments
///
/// * `input` - The string to search in
/// * `pattern` - The regex pattern to find
/// * `ignorecase` - Optional: case-insensitive matching (default: false)
/// * `multiline` - Optional: multiline mode (default: false)
///
/// # Returns
///
/// A list of all matches. If the pattern contains groups, returns the
/// captured groups; otherwise returns the full matches.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `regex_findall` filter.
fn regex_findall(
    input: String,
    pattern: String,
    ignorecase: Option<bool>,
    multiline: Option<bool>,
) -> Vec<Value> {
    let pattern = build_pattern(
        &pattern,
        ignorecase.unwrap_or(false),
        multiline.unwrap_or(false),
    );

    match crate::utils::get_regex(&pattern) {
        Ok(re) => {
            // Check if pattern has capture groups
            let has_groups = re.captures_len() > 1;

            if has_groups {
                re.captures_iter(&input)
                    .filter_map(|caps| {
                        // Return array of captured groups (excluding full match)
                        let groups: Vec<Value> = caps
                            .iter()
                            .skip(1) // Skip the full match
                            .filter_map(|m| m.map(|mat| Value::from(mat.as_str().to_string())))
                            .collect();
                        if groups.len() == 1 {
                            Some(groups.into_iter().next().unwrap())
                        } else if !groups.is_empty() {
                            Some(Value::from(groups))
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                re.find_iter(&input)
                    .map(|m| Value::from(m.as_str().to_string()))
                    .collect()
            }
        }
        Err(_) => Vec::new(),
    }
}

/// Escape special regex characters in a string.
///
/// # Arguments
///
/// * `input` - The string to escape
///
/// # Returns
///
/// The input string with all regex special characters escaped.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `regex_escape` filter.
fn regex_escape(input: String) -> String {
    regex::escape(&input)
}

/// Split a string by a regex pattern.
///
/// # Arguments
///
/// * `input` - The string to split
/// * `pattern` - The regex pattern to split on
/// * `maxsplit` - Optional: maximum number of splits (default: unlimited)
///
/// # Returns
///
/// A list of substrings.
fn regex_split(input: String, pattern: String, maxsplit: Option<usize>) -> Vec<String> {
    match crate::utils::get_regex(&pattern) {
        Ok(re) => {
            let splits: Vec<&str> = match maxsplit {
                Some(n) if n > 0 => re.splitn(&input, n + 1).collect(),
                _ => re.split(&input).collect(),
            };
            splits.into_iter().map(|s| s.to_string()).collect()
        }
        Err(_) => vec![input],
    }
}

/// Build a regex pattern with optional flags.
fn build_pattern(pattern: &str, ignorecase: bool, multiline: bool) -> String {
    let mut flags = String::new();

    if ignorecase {
        flags.push_str("(?i)");
    }
    if multiline {
        flags.push_str("(?m)");
    }

    if flags.is_empty() {
        pattern.to_string()
    } else {
        format!("{}{}", flags, pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regex_search_basic() {
        let result = regex_search(
            "hello123world".to_string(),
            "[0-9]+".to_string(),
            None,
            None,
        );
        assert_eq!(result.to_string(), "123");
    }

    #[test]
    fn test_regex_search_with_groups() {
        let result = regex_search(
            "hello123world".to_string(),
            "hello([0-9]+)world".to_string(),
            None,
            None,
        );
        assert_eq!(result.to_string(), "123");
    }

    #[test]
    fn test_regex_search_no_match() {
        let result = regex_search("hello world".to_string(), "[0-9]+".to_string(), None, None);
        assert_eq!(result.to_string(), "");
    }

    #[test]
    fn test_regex_search_ignorecase() {
        let result = regex_search(
            "HELLO world".to_string(),
            "hello".to_string(),
            Some(true),
            None,
        );
        assert_eq!(result.to_string(), "HELLO");
    }

    #[test]
    fn test_regex_match_basic() {
        assert!(regex_match(
            "hello123".to_string(),
            "[0-9]+".to_string(),
            None
        ));
        assert!(!regex_match(
            "hello".to_string(),
            "[0-9]+".to_string(),
            None
        ));
    }

    #[test]
    fn test_regex_replace_basic() {
        let result = regex_replace(
            "hello world".to_string(),
            "world".to_string(),
            "universe".to_string(),
            None,
            None,
        );
        assert_eq!(result, "hello universe");
    }

    #[test]
    fn test_regex_replace_with_groups() {
        let result = regex_replace(
            "hello 123 world".to_string(),
            "(\\d+)".to_string(),
            "[$1]".to_string(),
            None,
            None,
        );
        assert_eq!(result, "hello [123] world");
    }

    #[test]
    fn test_regex_replace_multiple() {
        let result = regex_replace(
            "a1b2c3".to_string(),
            "[0-9]".to_string(),
            "X".to_string(),
            None,
            None,
        );
        assert_eq!(result, "aXbXcX");
    }

    #[test]
    fn test_regex_replace_with_count() {
        let result = regex_replace(
            "a1b2c3".to_string(),
            "[0-9]".to_string(),
            "X".to_string(),
            None,
            Some(2),
        );
        assert_eq!(result, "aXbXc3");
    }

    #[test]
    fn test_regex_findall_basic() {
        let result = regex_findall("a1b2c3".to_string(), "[0-9]+".to_string(), None, None);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].to_string(), "1");
        assert_eq!(result[1].to_string(), "2");
        assert_eq!(result[2].to_string(), "3");
    }

    #[test]
    fn test_regex_findall_with_groups() {
        let result = regex_findall(
            "a1b2c3".to_string(),
            "([a-z])([0-9])".to_string(),
            None,
            None,
        );
        assert_eq!(result.len(), 3);
        // Each match should be an array of [letter, number]
    }

    #[test]
    fn test_regex_escape() {
        let result = regex_escape("a.b*c?d+e[f]".to_string());
        assert_eq!(result, "a\\.b\\*c\\?d\\+e\\[f\\]");
    }

    #[test]
    fn test_regex_split_basic() {
        let result = regex_split("a1b2c3d".to_string(), "[0-9]".to_string(), None);
        assert_eq!(result, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_regex_split_with_maxsplit() {
        let result = regex_split("a1b2c3d".to_string(), "[0-9]".to_string(), Some(2));
        assert_eq!(result, vec!["a", "b", "c3d"]);
    }
}
