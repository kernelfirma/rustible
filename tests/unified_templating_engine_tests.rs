//! Unified Templating Engine Tests
//!
//! Issue #292: Unified templating engine in production
//!
//! These tests exercise the production TemplateEngine for both rendering
//! and condition evaluation.

use indexmap::IndexMap;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;

use rustible::template::TemplateEngine;

fn render(engine: &TemplateEngine, template: &str, vars: &HashMap<String, JsonValue>) -> String {
    engine
        .render(template, vars)
        .expect("render should succeed")
}

#[test]
fn test_render_fast_path_no_template() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();

    let rendered = render(&engine, "plain text", &vars);
    assert_eq!(rendered, "plain text");
}

#[test]
fn test_render_simple_variable() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), json!("Rustible"));

    let rendered = render(&engine, "Hello {{ name }}", &vars);
    assert_eq!(rendered, "Hello Rustible");
}

#[test]
fn test_render_value_recursively() {
    let engine = TemplateEngine::new();
    let mut vars = IndexMap::new();
    vars.insert("user".to_string(), json!("alex"));
    vars.insert("role".to_string(), json!("admin"));

    let input = json!({
        "greeting": "hi {{ user }}",
        "nested": { "role": "{{ role }}" },
        "items": ["{{ user }}", "static"]
    });

    let rendered = engine
        .render_value(&input, &vars)
        .expect("render_value should succeed");

    assert_eq!(
        *rendered,
        json!({
            "greeting": "hi alex",
            "nested": { "role": "admin" },
            "items": ["alex", "static"]
        })
    );
}

#[test]
fn test_condition_evaluation_literals_and_comparisons() {
    let engine = TemplateEngine::new();
    let mut vars = IndexMap::new();
    vars.insert("env".to_string(), json!("prod"));
    vars.insert("enabled".to_string(), json!(true));

    assert!(engine.evaluate_condition("true", &vars).unwrap());
    assert!(!engine.evaluate_condition("false", &vars).unwrap());
    assert!(engine.evaluate_condition("env == 'prod'", &vars).unwrap());
    assert!(engine.evaluate_condition("env != 'dev'", &vars).unwrap());
    assert!(engine.evaluate_condition("enabled", &vars).unwrap());
}

#[test]
fn test_condition_defined_undefined_and_logic() {
    let engine = TemplateEngine::new();
    let mut vars = IndexMap::new();
    vars.insert("present".to_string(), json!("yes"));
    vars.insert("flag".to_string(), json!(true));

    assert!(engine
        .evaluate_condition("present is defined", &vars)
        .unwrap());
    assert!(engine
        .evaluate_condition("missing is undefined", &vars)
        .unwrap());
    assert!(engine
        .evaluate_condition("flag and present is defined", &vars)
        .unwrap());
    assert!(!engine
        .evaluate_condition("flag and missing is defined", &vars)
        .unwrap());
}

#[test]
fn test_template_and_expression_cache_stats() {
    let engine = TemplateEngine::with_cache_size(10);
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), json!("cache"));

    let rendered = render(&engine, "Hello {{ name }}", &vars);
    assert_eq!(rendered, "Hello cache");

    let (_, expr_count_before) = engine.cache_stats();
    let mut expr_vars = IndexMap::new();
    expr_vars.insert("name".to_string(), json!("cache"));
    assert!(engine
        .evaluate_condition("name == 'cache'", &expr_vars)
        .unwrap());
    let (template_count, expr_count) = engine.cache_stats();

    assert!(template_count >= 1);
    assert!(expr_count >= expr_count_before);
}
