//! Template Optimization Benchmarks
//!
//! This benchmark suite measures the performance of the template engine for:
//! - Cache cold vs warm performance
//! - Variable lookup across different nesting depths
//! - Repeated render performance
//! - Complex template rendering

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde_json::json;
use std::collections::HashMap;

use rustible::template::TemplateEngine;

// ============================================================================
// Test Data Generators
// ============================================================================

fn generate_simple_vars() -> HashMap<String, serde_json::Value> {
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), json!("World"));
    vars.insert("count".to_string(), json!(42));
    vars
}

fn generate_nested_vars(depth: usize) -> HashMap<String, serde_json::Value> {
    let mut nested = json!("leaf_value");
    for i in (0..depth).rev() {
        nested = json!({ format!("level{}", i): nested });
    }

    let mut vars = HashMap::new();
    vars.insert("config".to_string(), nested);
    vars
}

fn generate_many_vars(count: usize) -> HashMap<String, serde_json::Value> {
    let mut vars = HashMap::new();
    for i in 0..count {
        vars.insert(format!("var{}", i), json!(format!("value{}", i)));
    }
    vars
}

fn generate_complex_nested() -> HashMap<String, serde_json::Value> {
    let mut vars = HashMap::new();
    vars.insert(
        "config".to_string(),
        json!({
            "database": {
                "primary": {
                    "host": "localhost",
                    "port": 5432,
                    "username": "admin"
                },
                "replica": {
                    "host": "replica.local",
                    "port": 5432
                }
            },
            "cache": {
                "redis": {
                    "host": "redis.local",
                    "port": 6379
                }
            },
            "api": {
                "endpoints": {
                    "users": "/api/v1/users",
                    "orders": "/api/v1/orders"
                }
            }
        }),
    );
    vars
}

// ============================================================================
// Cache Benchmarks
// ============================================================================

fn bench_cache_cold_vs_warm(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_cold_warm");

    let template = "Complex template: {{ a }} + {{ b }} = {{ c }}";
    let mut vars = HashMap::new();
    vars.insert("a".to_string(), json!(10));
    vars.insert("b".to_string(), json!(20));
    vars.insert("c".to_string(), json!(30));

    // Cold cache (first access)
    group.bench_function("cold_cache", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for i in 0..iters {
                let engine = TemplateEngine::new();
                let template_i = format!("{} - {}", template, i);
                let start = std::time::Instant::now();
                let _ = engine.render(&template_i, &vars);
                total += start.elapsed();
            }
            total
        })
    });

    // Warm cache
    let engine = TemplateEngine::new();
    let _ = engine.render(template, &vars);

    group.bench_function("warm_cache", |b| {
        b.iter(|| {
            let result = engine.render(black_box(template), black_box(&vars));
            black_box(result)
        })
    });

    group.finish();
}

// ============================================================================
// Variable Lookup Benchmarks
// ============================================================================

fn bench_naive_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("variable_lookup");

    let vars = generate_complex_nested();

    fn naive_lookup<'a>(
        vars: &'a HashMap<String, serde_json::Value>,
        path: &str,
    ) -> Option<&'a serde_json::Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = vars.get(parts[0])?;
        for part in &parts[1..] {
            current = current.get(part)?;
        }
        Some(current)
    }

    let path = "config.database.primary.host";

    group.bench_function("naive_lookup", |b| {
        b.iter(|| {
            let result = naive_lookup(black_box(&vars), black_box(path));
            black_box(result)
        })
    });

    // Template engine render with nested access
    let engine = TemplateEngine::new();
    let template = "{{ config.database.primary.host }}";

    group.bench_function("template_render", |b| {
        b.iter(|| {
            let result = engine.render(black_box(template), black_box(&vars));
            black_box(result)
        })
    });

    group.finish();
}

fn bench_variable_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("variable_depth");

    let engine = TemplateEngine::new();

    for depth in [3, 5, 7, 10] {
        let vars = generate_nested_vars(depth);

        // Build path to leaf
        let path: String = (0..depth)
            .map(|i| format!("level{}", i))
            .collect::<Vec<_>>()
            .join(".");
        let template = format!("{{{{ config.{} }}}}", path);

        group.throughput(Throughput::Elements(depth as u64));
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, _| {
            b.iter(|| {
                let result = engine.render(black_box(&template), black_box(&vars));
                black_box(result)
            })
        });
    }

    group.finish();
}

fn bench_many_variables(c: &mut Criterion) {
    let mut group = c.benchmark_group("many_variables");

    let engine = TemplateEngine::new();

    for count in [10, 50, 100, 500] {
        let vars = generate_many_vars(count);
        let template = "{{ var0 }}";

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                let result = engine.render(black_box(template), black_box(&vars));
                black_box(result)
            })
        });
    }

    group.finish();
}

// ============================================================================
// Render Performance Benchmarks
// ============================================================================

fn bench_simple_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_render");

    let template = "Hello {{ name }}!";
    let vars = generate_simple_vars();
    let engine = TemplateEngine::new();

    group.bench_function("simple_template", |b| {
        b.iter(|| {
            let result = engine.render(black_box(template), black_box(&vars));
            black_box(result)
        })
    });

    group.finish();
}

fn bench_full_template(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_template");

    let template = r#"
Database Configuration:
  Host: {{ config.database.primary.host }}
  Port: {{ config.database.primary.port }}

Cache Configuration:
  Host: {{ config.cache.redis.host }}
  Port: {{ config.cache.redis.port }}

API Endpoints:
  Users: {{ config.api.endpoints.users }}
  Orders: {{ config.api.endpoints.orders }}
"#;

    let vars = generate_complex_nested();
    let engine = TemplateEngine::new();

    // Warm the cache
    let _ = engine.render(template, &vars);

    group.bench_function("complex_template", |b| {
        b.iter(|| {
            let result = engine.render(black_box(template), black_box(&vars));
            black_box(result)
        })
    });

    group.finish();
}

fn bench_repeated_renders(c: &mut Criterion) {
    let mut group = c.benchmark_group("repeated_renders");

    let template = "{{ name }} - {{ value }}";
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), json!("test"));
    vars.insert("value".to_string(), json!(123));

    let engine = TemplateEngine::new();

    // Single render
    group.bench_function("single", |b| {
        b.iter(|| {
            let result = engine.render(black_box(template), black_box(&vars));
            black_box(result)
        })
    });

    // 10 repeated renders (simulates loop)
    group.bench_function("10_repeated", |b| {
        b.iter(|| {
            for _ in 0..10 {
                let result = engine.render(template, &vars);
                let _ = black_box(result);
            }
        })
    });

    // 100 repeated renders
    group.bench_function("100_repeated", |b| {
        b.iter(|| {
            for _ in 0..100 {
                let result = engine.render(template, &vars);
                let _ = black_box(result);
            }
        })
    });

    group.finish();
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(cache_benches, bench_cache_cold_vs_warm,);

criterion_group!(
    lookup_benches,
    bench_naive_lookup,
    bench_variable_depth,
    bench_many_variables,
);

criterion_group!(
    render_benches,
    bench_simple_render,
    bench_full_template,
    bench_repeated_renders,
);

criterion_main!(cache_benches, lookup_benches, render_benches,);
