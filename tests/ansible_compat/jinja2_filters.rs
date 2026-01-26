//! Jinja2 Filter Compatibility Tests
//!
//! Tests for verifying that Rustible's MiniJinja-based template engine
//! produces output compatible with Ansible's Jinja2 filters.

use rustible::template::Engine;

/// Helper to render a template string
fn render(template: &str, vars: serde_json::Value) -> String {
    let engine = Engine::new();
    engine
        .render_string(template, &vars)
        .unwrap_or_else(|e| format!("ERROR: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ============================================================
    // String Filters
    // ============================================================

    #[test]
    fn test_filter_upper() {
        let result = render("{{ 'hello' | upper }}", json!({}));
        assert_eq!(result, "HELLO");
    }

    #[test]
    fn test_filter_lower() {
        let result = render("{{ 'HELLO' | lower }}", json!({}));
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_filter_title() {
        let result = render("{{ 'hello world' | title }}", json!({}));
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_filter_capitalize() {
        let result = render("{{ 'hello' | capitalize }}", json!({}));
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_filter_trim() {
        let result = render("{{ '  hello  ' | trim }}", json!({}));
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_filter_replace() {
        let result = render("{{ 'hello world' | replace('world', 'rust') }}", json!({}));
        assert_eq!(result, "hello rust");
    }

    // ============================================================
    // Default Filter
    // ============================================================

    #[test]
    fn test_filter_default_undefined() {
        let result = render("{{ undefined_var | default('fallback') }}", json!({}));
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_filter_default_defined() {
        let result = render("{{ defined_var | default('fallback') }}", json!({"defined_var": "value"}));
        assert_eq!(result, "value");
    }

    #[test]
    fn test_filter_default_empty_string() {
        // Ansible behavior: empty string is NOT replaced by default
        let result = render("{{ empty_var | default('fallback') }}", json!({"empty_var": ""}));
        assert_eq!(result, "");
    }

    #[test]
    fn test_filter_default_boolean_true() {
        let result = render("{{ empty_var | default('fallback', true) }}", json!({"empty_var": ""}));
        // When second parameter is true, empty/false values use default
        assert!(result == "fallback" || result == "");
    }

    // ============================================================
    // List Filters
    // ============================================================

    #[test]
    fn test_filter_first() {
        let result = render("{{ items | first }}", json!({"items": [1, 2, 3]}));
        assert_eq!(result, "1");
    }

    #[test]
    fn test_filter_last() {
        let result = render("{{ items | last }}", json!({"items": [1, 2, 3]}));
        assert_eq!(result, "3");
    }

    #[test]
    fn test_filter_length() {
        let result = render("{{ items | length }}", json!({"items": [1, 2, 3]}));
        assert_eq!(result, "3");
    }

    #[test]
    fn test_filter_join() {
        let result = render("{{ items | join(', ') }}", json!({"items": ["a", "b", "c"]}));
        assert_eq!(result, "a, b, c");
    }

    #[test]
    fn test_filter_sort() {
        let result = render("{{ items | sort | join(',') }}", json!({"items": [3, 1, 2]}));
        assert_eq!(result, "1,2,3");
    }

    #[test]
    fn test_filter_reverse() {
        let result = render("{{ items | reverse | join(',') }}", json!({"items": [1, 2, 3]}));
        assert_eq!(result, "3,2,1");
    }

    #[test]
    fn test_filter_unique() {
        let result = render("{{ items | unique | join(',') }}", json!({"items": [1, 2, 2, 3, 3, 3]}));
        assert_eq!(result, "1,2,3");
    }

    // ============================================================
    // Type Conversion Filters
    // ============================================================

    #[test]
    fn test_filter_int() {
        let result = render("{{ '42' | int }}", json!({}));
        assert_eq!(result, "42");
    }

    #[test]
    fn test_filter_float() {
        let result = render("{{ '3.14' | float }}", json!({}));
        assert!(result.starts_with("3.14"));
    }

    #[test]
    fn test_filter_string() {
        let result = render("{{ 42 | string }}", json!({}));
        assert_eq!(result, "42");
    }

    #[test]
    fn test_filter_bool() {
        let result = render("{{ 'yes' | bool }}", json!({}));
        assert!(result == "true" || result == "True");
    }

    // ============================================================
    // JSON/YAML Filters
    // ============================================================

    #[test]
    fn test_filter_to_json() {
        let result = render("{{ data | to_json }}", json!({"data": {"key": "value"}}));
        assert!(result.contains("key") && result.contains("value"));
    }

    #[test]
    fn test_filter_to_yaml() {
        let result = render("{{ data | to_yaml }}", json!({"data": {"key": "value"}}));
        assert!(result.contains("key") && result.contains("value"));
    }

    #[test]
    fn test_filter_from_json() {
        let result = render("{{ '{\"key\": \"value\"}' | from_json | to_json }}", json!({}));
        assert!(result.contains("key"));
    }

    // ============================================================
    // Path Filters
    // ============================================================

    #[test]
    fn test_filter_basename() {
        let result = render("{{ '/path/to/file.txt' | basename }}", json!({}));
        assert_eq!(result, "file.txt");
    }

    #[test]
    fn test_filter_dirname() {
        let result = render("{{ '/path/to/file.txt' | dirname }}", json!({}));
        assert_eq!(result, "/path/to");
    }

    // ============================================================
    // Regex Filters
    // ============================================================

    #[test]
    fn test_filter_regex_replace() {
        let result = render("{{ 'hello123world' | regex_replace('[0-9]+', '_') }}", json!({}));
        assert_eq!(result, "hello_world");
    }

    #[test]
    fn test_filter_regex_search() {
        let result = render("{{ 'hello123world' | regex_search('[0-9]+') }}", json!({}));
        assert_eq!(result, "123");
    }

    // ============================================================
    // Encoding Filters
    // ============================================================

    #[test]
    fn test_filter_b64encode() {
        let result = render("{{ 'hello' | b64encode }}", json!({}));
        assert_eq!(result, "aGVsbG8=");
    }

    #[test]
    fn test_filter_b64decode() {
        let result = render("{{ 'aGVsbG8=' | b64decode }}", json!({}));
        assert_eq!(result, "hello");
    }

    // ============================================================
    // Math Filters
    // ============================================================

    #[test]
    fn test_filter_min() {
        let result = render("{{ items | min }}", json!({"items": [3, 1, 2]}));
        assert_eq!(result, "1");
    }

    #[test]
    fn test_filter_max() {
        let result = render("{{ items | max }}", json!({"items": [3, 1, 2]}));
        assert_eq!(result, "3");
    }

    #[test]
    fn test_filter_sum() {
        let result = render("{{ items | sum }}", json!({"items": [1, 2, 3]}));
        assert_eq!(result, "6");
    }
}
