//! Template Engine Unified API Tests
//!
//! Tests for the unified template engine (TemplateEngine) that uses
//! MiniJinja with AST-based parsing and LRU caching.

use indexmap::IndexMap;
use rustible::template::TemplateEngine;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;

/// Helper to convert JSON object to IndexMap for evaluate_condition
fn vars_from_json(v: JsonValue) -> IndexMap<String, JsonValue> {
    match v {
        JsonValue::Object(map) => map.into_iter().collect(),
        _ => IndexMap::new(),
    }
}

// ============================================================================
// Engine Initialization Tests
// ============================================================================

#[test]
fn test_engine_creation() {
    let _engine = TemplateEngine::new();
    // Engine should be created successfully
    assert!(true);
}

#[test]
fn test_engine_with_cache_size() {
    // Default cache size
    let engine = TemplateEngine::new();
    let (templates, expressions) = engine.cache_stats();
    assert_eq!(templates, 0);
    assert_eq!(expressions, 0);

    // Custom cache size
    let engine = TemplateEngine::with_cache_size(100);
    let (templates, expressions) = engine.cache_stats();
    assert_eq!(templates, 0);
    assert_eq!(expressions, 0);
}

#[test]
fn test_engine_cache_disabled() {
    // Cache size 0 disables caching
    let engine = TemplateEngine::with_cache_size(0);
    let (templates, expressions) = engine.cache_stats();
    assert_eq!(templates, 0);
    assert_eq!(expressions, 0);
}

// ============================================================================
// Template Rendering Tests
// ============================================================================

#[test]
fn test_render_simple_variable() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), json!("World"));

    let result = engine.render("Hello, {{ name }}!", &vars).unwrap();
    assert_eq!(result, "Hello, World!");
}

#[test]
fn test_render_no_template_syntax() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();

    // Fast path: no template syntax
    let result = engine.render("Plain text", &vars).unwrap();
    assert_eq!(result, "Plain text");
}

#[test]
fn test_render_multiple_variables() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("first".to_string(), json!("Hello"));
    vars.insert("second".to_string(), json!("World"));

    let result = engine.render("{{ first }}, {{ second }}!", &vars).unwrap();
    assert_eq!(result, "Hello, World!");
}

#[test]
fn test_render_nested_variable() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("user".to_string(), json!({"name": "Alice", "age": 30}));

    let result = engine.render("User: {{ user.name }}", &vars).unwrap();
    assert_eq!(result, "User: Alice");
}

#[test]
fn test_render_with_filter() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("text".to_string(), json!("hello"));

    let result = engine.render("{{ text | upper }}", &vars).unwrap();
    assert_eq!(result, "HELLO");
}

#[test]
fn test_render_with_default_filter() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();

    let result = engine
        .render("{{ undefined | default('fallback') }}", &vars)
        .unwrap();
    assert_eq!(result, "fallback");
}

// ============================================================================
// Cache Behavior Tests
// ============================================================================

#[test]
fn test_cache_populates() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("x".to_string(), json!(1));

    // Render once
    let _ = engine.render("{{ x }}", &vars).unwrap();

    // Cache should have entry
    let (templates, _) = engine.cache_stats();
    assert!(templates > 0, "Template should be cached");
}

#[test]
fn test_cache_reuse() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("x".to_string(), json!(1));

    // Render same template multiple times
    for _ in 0..10 {
        let _ = engine.render("{{ x }}", &vars).unwrap();
    }

    // Should still only have one cached template
    let (templates, _) = engine.cache_stats();
    assert_eq!(templates, 1, "Same template should be reused from cache");
}

#[test]
fn test_cache_clear() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("x".to_string(), json!(1));

    // Render to populate cache
    let _ = engine.render("{{ x }}", &vars).unwrap();
    let (templates, _) = engine.cache_stats();
    assert!(templates > 0);

    // Clear cache
    engine.clear_cache();

    // Cache should be empty
    let (templates, expressions) = engine.cache_stats();
    assert_eq!(templates, 0);
    assert_eq!(expressions, 0);
}

// ============================================================================
// Condition Evaluation Tests
// ============================================================================

#[test]
fn test_evaluate_true_condition() {
    let engine = TemplateEngine::new();
    let vars = IndexMap::new();

    let result = engine.evaluate_condition("true", &vars).unwrap();
    assert!(result);
}

#[test]
fn test_evaluate_false_condition() {
    let engine = TemplateEngine::new();
    let vars = IndexMap::new();

    let result = engine.evaluate_condition("false", &vars).unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_variable_condition() {
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"enabled": true}));
    let result = engine.evaluate_condition("enabled", &vars).unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"enabled": false}));
    let result = engine.evaluate_condition("enabled", &vars).unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_comparison_condition() {
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"count": 10}));
    let result = engine.evaluate_condition("count > 5", &vars).unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"count": 3}));
    let result = engine.evaluate_condition("count > 5", &vars).unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_and_condition() {
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"a": true, "b": true}));
    let result = engine.evaluate_condition("a and b", &vars).unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"a": true, "b": false}));
    let result = engine.evaluate_condition("a and b", &vars).unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_or_condition() {
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"a": false, "b": true}));
    let result = engine.evaluate_condition("a or b", &vars).unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"a": false, "b": false}));
    let result = engine.evaluate_condition("a or b", &vars).unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_not_condition() {
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"disabled": false}));
    let result = engine.evaluate_condition("not disabled", &vars).unwrap();
    assert!(result);
}

#[test]
fn test_evaluate_is_defined() {
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"var": "value"}));
    let result = engine.evaluate_condition("var is defined", &vars).unwrap();
    assert!(result);

    let vars = IndexMap::new();
    let result = engine.evaluate_condition("var is defined", &vars).unwrap();
    assert!(!result);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_render_syntax_error() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();

    // Unclosed template tag
    let result = engine.render("{{ unclosed", &vars);
    assert!(result.is_err());
}

#[test]
fn test_render_undefined_filter() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();

    // Undefined filter
    let result = engine.render("{{ 'text' | nonexistent_filter }}", &vars);
    assert!(result.is_err());
}

// ============================================================================
// Thread Safety Tests
// ============================================================================

#[test]
fn test_concurrent_rendering() {
    use std::sync::Arc;
    use std::thread;

    let engine = Arc::new(TemplateEngine::new());
    let mut handles = vec![];

    for i in 0..10 {
        let engine = Arc::clone(&engine);
        handles.push(thread::spawn(move || {
            let mut vars = HashMap::new();
            vars.insert("i".to_string(), json!(i));
            engine.render("Value: {{ i }}", &vars).unwrap()
        }));
    }

    for handle in handles {
        let result = handle.join().unwrap();
        assert!(result.starts_with("Value: "));
    }
}

// ============================================================================
// API Completeness Tests
// ============================================================================

#[test]
fn test_render_with_json_api() {
    let engine = TemplateEngine::new();

    let result = engine
        .render_with_json("Hello, {{ name }}!", &json!({"name": "World"}))
        .unwrap();
    assert_eq!(result, "Hello, World!");
}

#[test]
fn test_is_template_detection() {
    // Test that template detection works correctly
    assert!(TemplateEngine::is_template("{{ var }}"));
    assert!(TemplateEngine::is_template("{% if x %}y{% endif %}"));
    assert!(TemplateEngine::is_template("{# comment #}"));
    assert!(!TemplateEngine::is_template("plain text"));
    assert!(!TemplateEngine::is_template("no { braces }"));
}

// ============================================================================
// Condition Evaluation Parity Tests (when/changed_when/failed_when)
// ============================================================================

#[test]
fn test_condition_rc_check() {
    // Common pattern: check return code for changed_when/failed_when
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"rc": 0}));
    let result = engine.evaluate_condition("rc == 0", &vars).unwrap();
    assert!(result, "rc == 0 should be true when rc is 0");

    let vars = vars_from_json(json!({"rc": 1}));
    let result = engine.evaluate_condition("rc != 0", &vars).unwrap();
    assert!(result, "rc != 0 should be true when rc is 1");

    let vars = vars_from_json(json!({"rc": 1}));
    let result = engine.evaluate_condition("rc == 0", &vars).unwrap();
    assert!(!result, "rc == 0 should be false when rc is 1");
}

#[test]
fn test_condition_stdout_contains() {
    // Common pattern: check stdout content
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"stdout": "package installed"}));
    let result = engine
        .evaluate_condition("'installed' in stdout", &vars)
        .unwrap();
    assert!(result, "should find 'installed' in stdout");

    let vars = vars_from_json(json!({"stdout": "success"}));
    let result = engine
        .evaluate_condition("'error' in stdout", &vars)
        .unwrap();
    assert!(!result, "should not find 'error' in stdout");
}

#[test]
fn test_condition_result_changed() {
    // Common pattern: check if result.changed
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"result": {"changed": true}}));
    let result = engine.evaluate_condition("result.changed", &vars).unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"result": {"changed": false}}));
    let result = engine.evaluate_condition("result.changed", &vars).unwrap();
    assert!(!result);
}

#[test]
fn test_condition_result_failed() {
    // Common pattern: check if result.failed
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"result": {"failed": true}}));
    let result = engine.evaluate_condition("result.failed", &vars).unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"result": {"failed": false}}));
    let result = engine
        .evaluate_condition("not result.failed", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_ansible_os_family() {
    // Common when condition: check OS family
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"ansible_os_family": "Debian"}));
    let result = engine
        .evaluate_condition("ansible_os_family == 'Debian'", &vars)
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("ansible_os_family == 'RedHat'", &vars)
        .unwrap();
    assert!(!result);
}

#[test]
fn test_condition_ansible_distribution() {
    // Common when condition: check distribution
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"ansible_distribution": "Ubuntu"}));
    let result = engine
        .evaluate_condition("ansible_distribution == 'Ubuntu'", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_ansible_version() {
    // Common when condition: check version
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"ansible_distribution_major_version": "22"}));
    let result = engine
        .evaluate_condition("ansible_distribution_major_version | int >= 20", &vars)
        .unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"ansible_distribution_major_version": "18"}));
    let result = engine
        .evaluate_condition("ansible_distribution_major_version | int < 20", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_item_in_loop() {
    // Common when condition in loops
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"item": {"name": "nginx", "state": "present"}}));
    let result = engine
        .evaluate_condition("item.state == 'present'", &vars)
        .unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"item": {"name": "nginx"}}));
    let result = engine
        .evaluate_condition("item.enabled | default(true)", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_inventory_hostname() {
    // Common when condition: check inventory hostname
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({
        "inventory_hostname": "web1",
        "groups": {"webservers": ["web1", "web2"]}
    }));
    let result = engine
        .evaluate_condition("inventory_hostname in groups['webservers']", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_complex_and_or() {
    // Complex conditions with and/or
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({
        "ansible_os_family": "Debian",
        "ansible_distribution": "Ubuntu"
    }));
    let result = engine
        .evaluate_condition(
            "(ansible_os_family == 'Debian' and ansible_distribution == 'Ubuntu') or ansible_os_family == 'RedHat'",
            &vars,
        )
        .unwrap();
    assert!(result);

    let vars = vars_from_json(json!({
        "enabled": true,
        "skip_task": false,
        "force": false
    }));
    let result = engine
        .evaluate_condition("(enabled and not skip_task) or force", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_regex_match() {
    // Regex matching in conditions
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"ansible_hostname": "web01"}));
    let result = engine
        .evaluate_condition("ansible_hostname is match('^web.*')", &vars)
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("ansible_hostname is match('^db.*')", &vars)
        .unwrap();
    assert!(!result);
}

#[test]
fn test_condition_stat_result() {
    // Common pattern: check stat result
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"stat_result": {"stat": {"exists": true}}}));
    let result = engine
        .evaluate_condition("stat_result.stat.exists", &vars)
        .unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"stat_result": {"stat": {"exists": false}}}));
    let result = engine
        .evaluate_condition("not stat_result.stat.exists", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_string_tests() {
    // String tests in conditions
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"myvar": "prefix_value"}));
    let result = engine
        .evaluate_condition("myvar is startswith('prefix')", &vars)
        .unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"myvar": "file.txt"}));
    let result = engine
        .evaluate_condition("myvar is endswith('.txt')", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_list_operations() {
    // List operations in conditions
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"packages": ["nginx", "apache"]}));
    let result = engine
        .evaluate_condition("packages | length > 0", &vars)
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("'nginx' in packages", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_ternary_expression() {
    // Ternary-style conditions
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"enabled": true}));
    let result = engine
        .evaluate_condition("enabled | ternary(true, false)", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_empty_string_falsy() {
    // Empty string should be falsy
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"myvar": ""}));
    let result = engine.evaluate_condition("myvar", &vars).unwrap();
    assert!(!result, "empty string should be falsy");

    let vars = vars_from_json(json!({"myvar": "value"}));
    let result = engine.evaluate_condition("myvar", &vars).unwrap();
    assert!(result, "non-empty string should be truthy");
}

#[test]
fn test_condition_null_handling() {
    // Null handling in conditions
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"myvar": null}));
    let result = engine.evaluate_condition("myvar is none", &vars).unwrap();
    assert!(result);

    let vars = vars_from_json(json!({"myvar": "value"}));
    let result = engine
        .evaluate_condition("myvar is not none", &vars)
        .unwrap();
    assert!(result);
}

#[test]
fn test_condition_numeric_comparisons() {
    // Numeric comparisons
    let engine = TemplateEngine::new();

    let vars = vars_from_json(json!({"count": 10}));
    let result = engine
        .evaluate_condition("count > 5 and count < 15", &vars)
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("count >= 10 and count <= 10", &vars)
        .unwrap();
    assert!(result);
}
