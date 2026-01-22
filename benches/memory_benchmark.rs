//! Memory Usage Benchmark Suite for Rustible
//!
//! This benchmark suite focuses on memory efficiency testing for:
//!
//! 1. PLAYBOOK PARSING:
//!    - Memory usage per task
//!    - Large playbook parsing efficiency
//!    - Task cloning overhead
//!
//! 2. VARIABLE STORAGE:
//!    - Variables memory scaling
//!    - Variable scope memory overhead
//!
//! 3. INVENTORY:
//!    - Inventory memory scaling
//!    - Host variable memory
//!
//! 4. TASK RESULTS:
//!    - Stats accumulation memory growth
//!    - Result storage efficiency
//!
//! Run with: cargo bench --bench memory_benchmark

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::hint::black_box as bb;

// Import Rustible types - use prelude for common types
use rustible::inventory::{Group, Host, Inventory};
use rustible::playbook::{Play, Playbook, Task};
use rustible::vars::Variables;

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Generate a task with typical content
fn generate_task(name: &str, complexity: usize) -> Task {
    let mut args = serde_json::Map::new();
    for i in 0..complexity {
        args.insert(
            format!("arg_{}", i),
            serde_json::json!(format!("value_{}", i)),
        );
    }

    Task::new(name, "command", serde_json::Value::Object(args))
}

/// Generate playbook YAML with specified tasks
fn generate_playbook_yaml(num_tasks: usize, vars_per_task: usize) -> String {
    let mut yaml = String::from(
        r#"
- name: Memory Test Play
  hosts: all
  gather_facts: false
  tasks:
"#,
    );

    for i in 0..num_tasks {
        yaml.push_str(&format!(
            r#"    - name: Task {}
      command: echo "test {}"
"#,
            i, i
        ));
        if vars_per_task > 0 {
            yaml.push_str("      vars:\n");
            for v in 0..vars_per_task {
                yaml.push_str(&format!("        var_{}: value_{}\n", v, v));
            }
        }
    }

    yaml
}

/// Generate inventory with specified hosts
fn generate_inventory(num_hosts: usize, vars_per_host: usize) -> String {
    let mut yaml = String::from("all:\n  hosts:\n");

    for h in 0..num_hosts {
        yaml.push_str(&format!("    host{:05}:\n", h));
        yaml.push_str(&format!(
            "      ansible_host: 10.{}.{}.{}\n",
            (h / 65536) % 256,
            (h / 256) % 256,
            h % 256
        ));
        for v in 0..vars_per_host {
            yaml.push_str(&format!("      var_{}: value_{}\n", v, v));
        }
    }

    yaml
}

// ============================================================================
// PLAYBOOK PARSING MEMORY BENCHMARKS
// ============================================================================

fn bench_task_creation_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_task_creation");

    // Benchmark task creation with varying complexity
    for complexity in [1, 5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::new("args", complexity),
            complexity,
            |b, &complexity| {
                b.iter(|| {
                    let task = generate_task(bb("test_task"), complexity);
                    black_box(task)
                })
            },
        );
    }

    group.finish();
}

fn bench_task_cloning_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_task_cloning");

    // Create tasks with different sizes
    let small_task = generate_task("small", 2);
    let medium_task = generate_task("medium", 10);
    let large_task = generate_task("large", 50);

    group.bench_function("clone_small", |b| b.iter(|| black_box(small_task.clone())));

    group.bench_function("clone_medium", |b| {
        b.iter(|| black_box(medium_task.clone()))
    });

    group.bench_function("clone_large", |b| b.iter(|| black_box(large_task.clone())));

    // Benchmark batch cloning (common in role expansion)
    let tasks: Vec<Task> = (0..100)
        .map(|i| generate_task(&format!("task_{}", i), 5))
        .collect();

    group.bench_function("clone_batch_100", |b| {
        b.iter(|| {
            let cloned: Vec<Task> = tasks.iter().cloned().collect();
            black_box(cloned)
        })
    });

    group.finish();
}

fn bench_playbook_parsing_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_playbook_parsing");
    group.sample_size(20);

    // Test with different playbook sizes
    for num_tasks in [10, 50, 100, 500].iter() {
        let yaml = generate_playbook_yaml(*num_tasks, 0);
        group.throughput(Throughput::Elements(*num_tasks as u64));

        group.bench_with_input(
            BenchmarkId::new("tasks", num_tasks),
            &yaml,
            |b, yaml_content| {
                b.iter(|| {
                    let result = Playbook::from_yaml(black_box(yaml_content), None);
                    black_box(result)
                })
            },
        );
    }

    // Test with tasks that have variables
    for vars_per_task in [0, 5, 10].iter() {
        let yaml = generate_playbook_yaml(50, *vars_per_task);

        group.bench_with_input(
            BenchmarkId::new("vars_per_task", vars_per_task),
            &yaml,
            |b, yaml_content| {
                b.iter(|| {
                    let result = Playbook::from_yaml(black_box(yaml_content), None);
                    black_box(result)
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// VARIABLE STORAGE MEMORY BENCHMARKS
// ============================================================================

fn bench_variables_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_variables_scaling");

    // Benchmark Variables with increasing number of variables
    for num_vars in [10, 100, 500, 1000].iter() {
        group.throughput(Throughput::Elements(*num_vars as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(num_vars),
            num_vars,
            |b, &num| {
                b.iter(|| {
                    let mut vars = Variables::new();
                    for i in 0..num {
                        vars.set(
                            format!("var_{}", i),
                            serde_json::Value::String(format!("value_{}", i)),
                        );
                    }
                    black_box(vars)
                })
            },
        );
    }

    group.finish();
}

fn bench_variables_merging(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_variables_merging");

    // Create base variables
    let mut base_vars = Variables::new();
    for i in 0..200 {
        base_vars.set(
            format!("base_var_{}", i),
            serde_json::Value::String(format!("base_value_{}", i)),
        );
    }

    // Benchmark merging variables
    group.bench_function("merge_200_vars", |b| {
        let mut override_vars = Variables::new();
        for i in 0..50 {
            override_vars.set(
                format!("override_var_{}", i),
                serde_json::Value::String(format!("override_value_{}", i)),
            );
        }

        b.iter(|| {
            let mut merged = base_vars.clone();
            merged.merge(&override_vars);
            black_box(merged)
        })
    });

    group.finish();
}

// ============================================================================
// INVENTORY MEMORY BENCHMARKS
// ============================================================================

fn bench_inventory_memory_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_inventory_scaling");
    group.sample_size(20);

    // Test with different inventory sizes
    for num_hosts in [10, 100, 500, 1000].iter() {
        if *num_hosts > 500 {
            group.sample_size(10);
        }

        let yaml = generate_inventory(*num_hosts, 5);
        group.throughput(Throughput::Elements(*num_hosts as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(num_hosts),
            &yaml,
            |b, yaml_content| {
                b.iter(|| {
                    use std::io::Write;
                    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
                    tmpfile.write_all(yaml_content.as_bytes()).unwrap();
                    tmpfile.flush().unwrap();
                    let result = Inventory::load(black_box(tmpfile.path()));
                    black_box(result)
                })
            },
        );
    }

    group.finish();
}

fn bench_host_vars_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_host_vars");

    // Test memory impact of variables per host
    for vars_per_host in [0, 5, 10, 20, 50].iter() {
        let yaml = generate_inventory(100, *vars_per_host);

        group.bench_with_input(
            BenchmarkId::from_parameter(vars_per_host),
            &yaml,
            |b, yaml_content| {
                b.iter(|| {
                    use std::io::Write;
                    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
                    tmpfile.write_all(yaml_content.as_bytes()).unwrap();
                    tmpfile.flush().unwrap();
                    let result = Inventory::load(black_box(tmpfile.path()));
                    black_box(result)
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// TASK RESULTS MEMORY BENCHMARKS
// ============================================================================

fn bench_task_result_accumulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_task_results");

    // Benchmark result storage
    for num_results in [100, 500, 1000, 5000].iter() {
        group.throughput(Throughput::Elements(*num_results as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(num_results),
            num_results,
            |b, &num| {
                b.iter(|| {
                    let mut results: Vec<(String, String, std::time::Duration, bool)> =
                        Vec::with_capacity(num);
                    for i in 0..num {
                        results.push((
                            format!("task_{}", i),
                            format!("host_{}", i % 100),
                            std::time::Duration::from_millis((i as u64) % 1000),
                            i % 10 != 0, // 90% success rate
                        ));
                    }
                    black_box(results)
                })
            },
        );
    }

    group.finish();
}

fn bench_hashmap_vs_indexmap(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_map_comparison");

    for num_entries in [100, 500, 1000].iter() {
        // HashMap benchmark
        group.bench_with_input(
            BenchmarkId::new("hashmap", num_entries),
            num_entries,
            |b, &num| {
                b.iter(|| {
                    let mut map: HashMap<String, serde_json::Value> = HashMap::with_capacity(num);
                    for i in 0..num {
                        map.insert(
                            format!("key_{}", i),
                            serde_json::json!(format!("value_{}", i)),
                        );
                    }
                    black_box(map)
                })
            },
        );

        // IndexMap benchmark
        group.bench_with_input(
            BenchmarkId::new("indexmap", num_entries),
            num_entries,
            |b, &num| {
                b.iter(|| {
                    let mut map: IndexMap<String, serde_json::Value> = IndexMap::with_capacity(num);
                    for i in 0..num {
                        map.insert(
                            format!("key_{}", i),
                            serde_json::json!(format!("value_{}", i)),
                        );
                    }
                    black_box(map)
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// STRING ALLOCATION BENCHMARKS
// ============================================================================

fn bench_string_allocation_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_string_allocation");

    // Compare String vs &str patterns
    group.bench_function("owned_strings", |b| {
        b.iter(|| {
            let mut vec: Vec<String> = Vec::with_capacity(100);
            for i in 0..100 {
                vec.push(format!("module_name_{}", i));
            }
            black_box(vec)
        })
    });

    // Arc<str> for shared strings
    group.bench_function("arc_str", |b| {
        use std::sync::Arc;
        b.iter(|| {
            let mut vec: Vec<Arc<str>> = Vec::with_capacity(100);
            for i in 0..100 {
                vec.push(Arc::from(format!("module_name_{}", i)));
            }
            black_box(vec)
        })
    });

    // Repeated string cloning (common pattern)
    let template = "ansible.builtin.command".to_string();
    group.bench_function("clone_repeated_100x", |b| {
        b.iter(|| {
            let mut vec: Vec<String> = Vec::with_capacity(100);
            for _ in 0..100 {
                vec.push(template.clone());
            }
            black_box(vec)
        })
    });

    // Arc sharing for repeated strings
    let template_arc: std::sync::Arc<str> = std::sync::Arc::from("ansible.builtin.command");
    group.bench_function("arc_shared_100x", |b| {
        b.iter(|| {
            let mut vec: Vec<std::sync::Arc<str>> = Vec::with_capacity(100);
            for _ in 0..100 {
                vec.push(std::sync::Arc::clone(&template_arc));
            }
            black_box(vec)
        })
    });

    group.finish();
}

// ============================================================================
// COLLECTION SIZING BENCHMARKS
// ============================================================================

fn bench_vec_presizing(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_vec_presizing");

    for size in [10, 100, 1000].iter() {
        // Without pre-sizing
        group.bench_with_input(BenchmarkId::new("dynamic", size), size, |b, &size| {
            b.iter(|| {
                let mut vec: Vec<String> = Vec::new();
                for i in 0..size {
                    vec.push(format!("item_{}", i));
                }
                black_box(vec)
            })
        });

        // With pre-sizing
        group.bench_with_input(BenchmarkId::new("presized", size), size, |b, &size| {
            b.iter(|| {
                let mut vec: Vec<String> = Vec::with_capacity(size);
                for i in 0..size {
                    vec.push(format!("item_{}", i));
                }
                black_box(vec)
            })
        });
    }

    group.finish();
}

// ============================================================================
// CRITERION GROUPS AND MAIN
// ============================================================================

criterion_group!(
    playbook_memory_benches,
    bench_task_creation_memory,
    bench_task_cloning_overhead,
    bench_playbook_parsing_memory,
);

criterion_group!(
    variables_memory_benches,
    bench_variables_scaling,
    bench_variables_merging,
);

criterion_group!(
    inventory_memory_benches,
    bench_inventory_memory_scaling,
    bench_host_vars_memory,
);

criterion_group!(
    results_memory_benches,
    bench_task_result_accumulation,
    bench_hashmap_vs_indexmap,
);

criterion_group!(
    allocation_benches,
    bench_string_allocation_patterns,
    bench_vec_presizing,
);

criterion_main!(
    playbook_memory_benches,
    variables_memory_benches,
    inventory_memory_benches,
    results_memory_benches,
    allocation_benches,
);
