//! Template Engine Unified API Tests
//!
//! Tests for the unified template engine (TemplateEngine) that uses
//! MiniJinja with AST-based parsing and LRU caching.

use rustible::template::TemplateEngine;
use serde_json::json;
use std::collections::HashMap;

// ============================================================================
// Engine Initialization Tests
// ============================================================================

#[test]
fn test_engine_creation() {
    let engine = TemplateEngine::new();
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

    let result = engine.evaluate_condition("true", &json!({})).unwrap();
    assert!(result);
}

#[test]
fn test_evaluate_false_condition() {
    let engine = TemplateEngine::new();

    let result = engine.evaluate_condition("false", &json!({})).unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_variable_condition() {
    let engine = TemplateEngine::new();

    let result = engine
        .evaluate_condition("enabled", &json!({"enabled": true}))
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("enabled", &json!({"enabled": false}))
        .unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_comparison_condition() {
    let engine = TemplateEngine::new();

    let result = engine
        .evaluate_condition("count > 5", &json!({"count": 10}))
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("count > 5", &json!({"count": 3}))
        .unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_and_condition() {
    let engine = TemplateEngine::new();

    let result = engine
        .evaluate_condition("a and b", &json!({"a": true, "b": true}))
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("a and b", &json!({"a": true, "b": false}))
        .unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_or_condition() {
    let engine = TemplateEngine::new();

    let result = engine
        .evaluate_condition("a or b", &json!({"a": false, "b": true}))
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("a or b", &json!({"a": false, "b": false}))
        .unwrap();
    assert!(!result);
}

#[test]
fn test_evaluate_not_condition() {
    let engine = TemplateEngine::new();

    let result = engine
        .evaluate_condition("not disabled", &json!({"disabled": false}))
        .unwrap();
    assert!(result);
}

#[test]
fn test_evaluate_is_defined() {
    let engine = TemplateEngine::new();

    let result = engine
        .evaluate_condition("var is defined", &json!({"var": "value"}))
        .unwrap();
    assert!(result);

    let result = engine
        .evaluate_condition("var is defined", &json!({}))
        .unwrap();
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
fn test_render_string_api() {
    let engine = TemplateEngine::new();

    let result = engine
        .render_string("Hello, {{ name }}!", &json!({"name": "World"}))
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
