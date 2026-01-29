//! Templating Performance Regression Tests
//!
//! Issue #293: Templating performance regression tests
//!
//! These tests use the production TemplateEngine and focus on relative
//! comparisons and coarse timing budgets to avoid flaky micro-benchmarks.

use indexmap::IndexMap;
use rustible::template::TemplateEngine;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const RENDER_ITERATIONS: usize = 5000;
const EXPRESSION_ITERATIONS: usize = 8000;
const MAX_SLOWDOWN_RATIO: f64 = 3.0;
const MAX_RENDER_BUDGET: Duration = Duration::from_secs(5);
const MAX_EXPRESSION_BUDGET: Duration = Duration::from_secs(3);

fn render_vars() -> HashMap<String, JsonValue> {
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), json!("Rustible"));
    vars.insert("count".to_string(), json!(42));
    vars.insert("enabled".to_string(), json!(true));
    vars.insert(
        "items".to_string(),
        json!([
            {"name": "alpha", "value": 1, "enabled": true},
            {"name": "beta", "value": 2, "enabled": false},
            {"name": "gamma", "value": 3, "enabled": true}
        ]),
    );
    vars
}

fn expression_vars() -> IndexMap<String, JsonValue> {
    let mut vars = IndexMap::new();
    vars.insert("enabled".to_string(), json!(true));
    vars.insert("count".to_string(), json!(42));
    vars.insert("name".to_string(), json!("Rustible"));
    vars.insert("threshold".to_string(), json!(10));
    vars
}

fn measure_render(
    engine: &TemplateEngine,
    template: &str,
    vars: &HashMap<String, JsonValue>,
    iterations: usize,
) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        engine.render(template, vars).unwrap();
    }
    start.elapsed()
}

fn measure_expression(
    engine: &TemplateEngine,
    expression: &str,
    vars: &IndexMap<String, JsonValue>,
    iterations: usize,
) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        engine.evaluate_condition(expression, vars).unwrap();
    }
    start.elapsed()
}

#[test]
fn test_cached_render_throughput_budget() {
    let engine = TemplateEngine::with_cache_size(128);
    let vars = render_vars();
    let template = "Hello {{ name }} ({{ count }}) - {{ items | length }} items";

    // Warm cache
    engine.render(template, &vars).unwrap();

    let elapsed = measure_render(&engine, template, &vars, RENDER_ITERATIONS);
    assert!(
        elapsed < MAX_RENDER_BUDGET,
        "cached render budget exceeded: {:?}",
        elapsed
    );
}

#[test]
fn test_cached_render_not_significantly_slower_than_uncached() {
    let cached_engine = TemplateEngine::with_cache_size(128);
    let uncached_engine = TemplateEngine::with_cache_size(0);
    let vars = render_vars();
    let template = r#"
        {% for item in items %}
            {% if item.enabled %}
                {{ item.name }}={{ item.value | default(0) }}
            {% endif %}
        {% endfor %}
    "#;

    // Warm cache for cached engine
    cached_engine.render(template, &vars).unwrap();

    let cached_elapsed = measure_render(&cached_engine, template, &vars, RENDER_ITERATIONS);
    let uncached_elapsed = measure_render(&uncached_engine, template, &vars, RENDER_ITERATIONS);

    let ratio = cached_elapsed.as_secs_f64() / uncached_elapsed.as_secs_f64();
    assert!(
        ratio <= MAX_SLOWDOWN_RATIO,
        "cached render slowdown too high: {:.2}x (cached {:?}, uncached {:?})",
        ratio,
        cached_elapsed,
        uncached_elapsed
    );
}

#[test]
fn test_cached_expression_throughput_budget() {
    let engine = TemplateEngine::with_cache_size(128);
    let vars = expression_vars();
    let expression = "enabled and count > threshold and name == 'Rustible'";

    // Warm cache
    engine.evaluate_condition(expression, &vars).unwrap();

    let elapsed = measure_expression(&engine, expression, &vars, EXPRESSION_ITERATIONS);
    assert!(
        elapsed < MAX_EXPRESSION_BUDGET,
        "cached expression budget exceeded: {:?}",
        elapsed
    );
}

#[test]
fn test_cached_expression_not_significantly_slower_than_uncached() {
    let cached_engine = TemplateEngine::with_cache_size(128);
    let uncached_engine = TemplateEngine::with_cache_size(0);
    let vars = expression_vars();
    let expression = "enabled and count > threshold and name == 'Rustible'";

    // Warm cache for cached engine
    cached_engine.evaluate_condition(expression, &vars).unwrap();

    let cached_elapsed = measure_expression(&cached_engine, expression, &vars, EXPRESSION_ITERATIONS);
    let uncached_elapsed = measure_expression(&uncached_engine, expression, &vars, EXPRESSION_ITERATIONS);

    let ratio = cached_elapsed.as_secs_f64() / uncached_elapsed.as_secs_f64();
    assert!(
        ratio <= MAX_SLOWDOWN_RATIO,
        "cached expression slowdown too high: {:.2}x (cached {:?}, uncached {:?})",
        ratio,
        cached_elapsed,
        uncached_elapsed
    );
}
