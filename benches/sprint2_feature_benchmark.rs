//! Sprint 2 Feature Performance Benchmarks
//!
//! This benchmark suite measures the performance of new Sprint 2 features:
//! - Include tasks/vars loading performance
//! - Delegation overhead (delegate_to)
//! - Serial vs Free strategy execution
//! - Plan mode performance
//! - Parallelization enforcement (semaphores, token buckets)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::runtime::Runtime;

// ============================================================================
// Test Data Generators for Sprint 2 Features
// ============================================================================

/// Generate a simple include_tasks YAML file
fn generate_include_tasks_file(num_tasks: usize) -> String {
    let mut yaml = String::new();
    for i in 0..num_tasks {
        yaml.push_str(&format!(
            r#"- name: Task {}
  debug:
    msg: "Executing task {}"
"#,
            i, i
        ));
    }
    yaml
}

/// Generate a variables file for include_vars
fn generate_include_vars_file(num_vars: usize) -> String {
    let mut yaml = String::new();
    for i in 0..num_vars {
        yaml.push_str(&format!("var_{}: \"value_{}\"\n", i, i));
    }
    yaml
}

/// Generate nested include structure (N levels deep)
fn generate_nested_includes(depth: usize, temp_dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for level in 0..depth {
        let file_path = temp_dir.join(format!("level_{}.yml", level));
        let content = if level < depth - 1 {
            format!(
                r#"- name: Level {} task
  debug:
    msg: "At level {}"
- name: Include next level
  include_tasks: level_{}.yml
"#,
                level,
                level,
                level + 1
            )
        } else {
            format!(
                r#"- name: Final level {} task
  debug:
    msg: "At final level {}"
"#,
                level, level
            )
        };
        std::fs::write(&file_path, content).unwrap();
        files.push(file_path);
    }

    files
}

/// Generate a playbook with serial execution
#[allow(dead_code)]
fn generate_serial_playbook(batch_size: usize, _num_hosts: usize) -> String {
    format!(
        r#"
- name: Serial execution test
  hosts: all
  serial: {}
  gather_facts: false
  tasks:
    - name: Task 1
      debug:
        msg: "Hello from {{ inventory_hostname }}"
    - name: Task 2
      command: echo "Processing on {{ inventory_hostname }}"
    - name: Task 3
      set_fact:
        processed: true
"#,
        batch_size
    )
}

/// Generate a playbook for plan mode testing
fn generate_plan_mode_playbook(num_tasks: usize) -> String {
    let mut yaml = r#"
- name: Plan mode test play
  hosts: all
  gather_facts: false
  vars:
    app_name: myapp
    version: 1.0.0
  tasks:
"#
    .to_string();

    for i in 0..num_tasks {
        yaml.push_str(&format!(
            r#"    - name: Task {} - Install package
      package:
        name: package_{}
        state: present
      when: install_packages | default(true)
"#,
            i, i
        ));
    }

    yaml
}

/// Generate inventory for testing
#[allow(dead_code)]
fn generate_test_inventory(num_hosts: usize) -> String {
    let mut yaml = "all:\n  hosts:\n".to_string();
    for i in 0..num_hosts {
        yaml.push_str(&format!(
            "    host{}:\n      ansible_host: 10.0.0.{}\n",
            i,
            i % 256
        ));
    }
    yaml
}

// ============================================================================
// Include Loading Performance Benchmarks
// ============================================================================

fn bench_include_tasks_loading(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("include_tasks_loading");

    for num_tasks in [5, 10, 50, 100] {
        let temp_dir = TempDir::new().unwrap();
        let tasks_file = temp_dir.path().join("tasks.yml");
        let content = generate_include_tasks_file(num_tasks);
        std::fs::write(&tasks_file, &content).unwrap();

        group.throughput(Throughput::Elements(num_tasks as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_tasks),
            &num_tasks,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    let content = tokio::fs::read_to_string(&tasks_file).await.unwrap();
                    let tasks: Vec<serde_yaml::Value> = serde_yaml::from_str(&content).unwrap();
                    black_box(tasks)
                })
            },
        );
    }

    group.finish();
}

fn bench_include_vars_parsing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("include_vars_parsing");

    for num_vars in [10, 50, 100, 500] {
        let temp_dir = TempDir::new().unwrap();
        let vars_file = temp_dir.path().join("vars.yml");
        let content = generate_include_vars_file(num_vars);
        std::fs::write(&vars_file, &content).unwrap();

        group.throughput(Throughput::Elements(num_vars as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_vars), &num_vars, |b, _| {
            b.to_async(&rt).iter(|| async {
                let content = tokio::fs::read_to_string(&vars_file).await.unwrap();
                let vars: indexmap::IndexMap<String, serde_yaml::Value> =
                    serde_yaml::from_str(&content).unwrap();
                black_box(vars)
            })
        });
    }

    group.finish();
}

fn bench_nested_includes(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("nested_includes");

    for depth in [2, 3, 4, 5] {
        let temp_dir = TempDir::new().unwrap();
        let files = generate_nested_includes(depth, temp_dir.path());
        let root_file = files[0].clone();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_levels", depth)),
            &depth,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    // Simulate recursive include loading
                    let mut loaded_tasks = Vec::new();
                    let mut current_file = root_file.clone();

                    for _ in 0..depth {
                        let content = tokio::fs::read_to_string(&current_file).await.unwrap();
                        let tasks: Vec<serde_yaml::Value> = serde_yaml::from_str(&content).unwrap();
                        loaded_tasks.extend(tasks.clone());

                        // Find next include
                        for task in &tasks {
                            if let Some(include) = task.get("include_tasks") {
                                if let Some(file_str) = include.as_str() {
                                    current_file = current_file.parent().unwrap().join(file_str);
                                    break;
                                }
                            }
                        }
                    }

                    black_box(loaded_tasks)
                })
            },
        );
    }

    group.finish();
}

fn bench_inline_vs_include_comparison(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("inline_vs_include");

    let num_tasks = 20;

    // Inline definition
    let inline_yaml = {
        let mut yaml = r#"
- name: Inline play
  hosts: all
  gather_facts: false
  tasks:
"#
        .to_string();
        for i in 0..num_tasks {
            yaml.push_str(&format!(
                "    - name: Inline task {}\n      debug:\n        msg: \"Task {}\"\n",
                i, i
            ));
        }
        yaml
    };

    // Include-based (external file)
    let temp_dir = TempDir::new().unwrap();
    let tasks_file = temp_dir.path().join("tasks.yml");
    std::fs::write(&tasks_file, generate_include_tasks_file(num_tasks)).unwrap();

    group.bench_function("inline_tasks", |b| {
        b.iter(|| {
            let playbook: Vec<serde_yaml::Value> = serde_yaml::from_str(&inline_yaml).unwrap();
            black_box(playbook)
        })
    });

    group.bench_function("include_tasks", |b| {
        b.to_async(&rt).iter(|| async {
            let content = tokio::fs::read_to_string(&tasks_file).await.unwrap();
            let tasks: Vec<serde_yaml::Value> = serde_yaml::from_str(&content).unwrap();
            black_box(tasks)
        })
    });

    group.finish();
}

// ============================================================================
// Delegation Overhead Benchmarks
// ============================================================================

fn bench_delegate_to_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("delegate_to_parsing");

    // Task with delegate_to
    let delegated_task = r#"
- name: Delegated task
  delegate_to: localhost
  delegate_facts: true
  command: echo "Running on delegate"
"#;

    // Task without delegate_to
    let normal_task = r#"
- name: Normal task
  command: echo "Running normally"
"#;

    group.bench_function("with_delegation", |b| {
        b.iter(|| {
            let tasks: Vec<serde_yaml::Value> = serde_yaml::from_str(delegated_task).unwrap();
            // Extract delegate_to field
            for task in &tasks {
                let delegate_to = task.get("delegate_to");
                let delegate_facts = task.get("delegate_facts");
                black_box((delegate_to, delegate_facts));
            }
            black_box(tasks)
        })
    });

    group.bench_function("without_delegation", |b| {
        b.iter(|| {
            let tasks: Vec<serde_yaml::Value> = serde_yaml::from_str(normal_task).unwrap();
            black_box(tasks)
        })
    });

    group.finish();
}

fn bench_fact_assignment_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("fact_assignment");

    // Simulate fact assignment with different sizes
    for num_facts in [10, 50, 100, 500] {
        let mut facts: HashMap<String, serde_json::Value> = HashMap::new();
        for i in 0..num_facts {
            facts.insert(
                format!("fact_{}", i),
                serde_json::json!(format!("value_{}", i)),
            );
        }

        group.throughput(Throughput::Elements(num_facts as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_facts),
            &num_facts,
            |b, _| {
                b.iter(|| {
                    // Simulate assigning facts to delegated host
                    let mut host_facts: HashMap<String, HashMap<String, serde_json::Value>> =
                        HashMap::new();
                    host_facts.insert("delegate_host".to_string(), facts.clone());
                    black_box(host_facts)
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Serial vs Free Strategy Benchmarks
// ============================================================================

fn bench_serial_spec_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("serial_spec_calculation");

    // Test batch size calculation for different serial specs
    let total_hosts = 100;

    // Fixed batch size
    group.bench_function("fixed_batch_10", |b| {
        b.iter(|| {
            let batch_size = 10;
            let mut remaining = total_hosts;
            let mut batches = Vec::new();
            while remaining > 0 {
                let batch = remaining.min(batch_size);
                batches.push(batch);
                remaining -= batch;
            }
            black_box(batches)
        })
    });

    // Percentage-based
    group.bench_function("percentage_25", |b| {
        b.iter(|| {
            let percentage = 25.0;
            let batch_size = ((total_hosts as f64 * percentage / 100.0).ceil() as usize).max(1);
            let mut remaining = total_hosts;
            let mut batches = Vec::new();
            while remaining > 0 {
                let batch = remaining.min(batch_size);
                batches.push(batch);
                remaining -= batch;
            }
            black_box(batches)
        })
    });

    // Progressive batch sizes [1, 5, 10, 25%]
    group.bench_function("progressive", |b| {
        b.iter(|| {
            let progressive = [1, 5, 10, 25];
            let mut remaining = total_hosts;
            let mut batches = Vec::new();
            let mut idx = 0;
            while remaining > 0 {
                let batch_size = if idx < progressive.len() - 1 {
                    progressive[idx]
                } else {
                    // Last is percentage
                    ((total_hosts as f64 * progressive[idx] as f64 / 100.0).ceil() as usize).max(1)
                };
                let batch = remaining.min(batch_size);
                batches.push(batch);
                remaining -= batch;
                if idx < progressive.len() - 1 {
                    idx += 1;
                }
            }
            black_box(batches)
        })
    });

    group.finish();
}

fn bench_batch_host_splitting(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_host_splitting");

    for num_hosts in [10, 50, 100, 500] {
        let hosts: Vec<String> = (0..num_hosts).map(|i| format!("host{}", i)).collect();
        let batch_size = 10;

        group.throughput(Throughput::Elements(num_hosts as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_hosts),
            &num_hosts,
            |b, _| {
                b.iter(|| {
                    let mut batches = Vec::new();
                    for chunk in hosts.chunks(batch_size) {
                        batches.push(chunk.to_vec());
                    }
                    black_box(batches)
                })
            },
        );
    }

    group.finish();
}

fn bench_serial_vs_free_simulation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("serial_vs_free");

    let num_hosts = 20;
    let hosts: Vec<String> = (0..num_hosts).map(|i| format!("host{}", i)).collect();

    // Simulate free (fully parallel) execution
    group.bench_function("free_strategy", |b| {
        b.to_async(&rt).iter(|| async {
            let mut handles = Vec::new();
            for host in hosts.clone() {
                handles.push(tokio::spawn(async move {
                    // Simulate task execution
                    tokio::time::sleep(Duration::from_micros(100)).await;
                    host
                }));
            }
            let results: Vec<_> = futures::future::join_all(handles)
                .await
                .into_iter()
                .map(|r| r.unwrap())
                .collect();
            black_box(results)
        })
    });

    // Simulate serial (batch of 5) execution
    group.bench_function("serial_batch_5", |b| {
        b.to_async(&rt).iter(|| async {
            let batch_size = 5;
            let mut all_results = Vec::new();

            for batch in hosts.chunks(batch_size) {
                let mut handles = Vec::new();
                for host in batch {
                    let host = host.clone();
                    handles.push(tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_micros(100)).await;
                        host
                    }));
                }
                let results: Vec<_> = futures::future::join_all(handles)
                    .await
                    .into_iter()
                    .map(|r| r.unwrap())
                    .collect();
                all_results.extend(results);
            }

            black_box(all_results)
        })
    });

    // Simulate serial (batch of 1) execution
    group.bench_function("serial_batch_1", |b| {
        b.to_async(&rt).iter(|| async {
            let mut all_results = Vec::new();

            for host in &hosts {
                tokio::time::sleep(Duration::from_micros(100)).await;
                all_results.push(host.clone());
            }

            black_box(all_results)
        })
    });

    group.finish();
}

fn bench_max_fail_percentage_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("max_fail_percentage");

    let num_hosts = 100;

    for max_fail_pct in [0, 10, 25, 50] {
        group.bench_with_input(
            BenchmarkId::from_parameter(max_fail_pct),
            &max_fail_pct,
            |b, &max_fail| {
                b.iter(|| {
                    // Simulate checking failure percentage during execution
                    let mut failed = 0;
                    let mut succeeded = 0;

                    for i in 0..num_hosts {
                        // Simulate random failures (10% failure rate)
                        if i % 10 == 0 {
                            failed += 1;
                        } else {
                            succeeded += 1;
                        }

                        // Check if we've exceeded max_fail_percentage
                        let total = failed + succeeded;
                        if total > 0 {
                            let current_fail_pct = (failed as f64 / total as f64) * 100.0;
                            if max_fail > 0 && current_fail_pct > max_fail as f64 {
                                break;
                            }
                        }
                    }

                    black_box((failed, succeeded))
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Plan Mode Performance Benchmarks
// ============================================================================

fn bench_plan_mode_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan_mode");

    for num_tasks in [5, 20, 50, 100] {
        let playbook_yaml = generate_plan_mode_playbook(num_tasks);

        group.throughput(Throughput::Elements(num_tasks as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_tasks),
            &num_tasks,
            |b, _| {
                b.iter(|| {
                    // Parse playbook
                    let playbook: Vec<serde_yaml::Value> =
                        serde_yaml::from_str(&playbook_yaml).unwrap();

                    // Simulate plan mode - extract task info without execution
                    let mut plan = Vec::new();
                    for play in &playbook {
                        if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                            for task in tasks {
                                let name = task.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                let when = task.get("when").and_then(|w| w.as_str());

                                // Find module
                                let module = ["package", "command", "debug", "copy", "file"]
                                    .iter()
                                    .find(|&m| task.get(*m).is_some())
                                    .unwrap_or(&"unknown");

                                plan.push((
                                    name.to_string(),
                                    module.to_string(),
                                    when.map(String::from),
                                ));
                            }
                        }
                    }

                    black_box(plan)
                })
            },
        );
    }

    group.finish();
}

fn bench_variable_resolution_plan_mode(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan_mode_vars");

    // Template with variables
    let template =
        "Installing {{ package_name }} version {{ version }} on {{ inventory_hostname }}";

    for num_vars in [5, 20, 50] {
        let mut vars: HashMap<String, serde_json::Value> = HashMap::new();
        vars.insert("package_name".to_string(), serde_json::json!("nginx"));
        vars.insert("version".to_string(), serde_json::json!("1.20.0"));
        vars.insert(
            "inventory_hostname".to_string(),
            serde_json::json!("webserver01"),
        );

        for i in 0..num_vars {
            vars.insert(
                format!("extra_var_{}", i),
                serde_json::json!(format!("value_{}", i)),
            );
        }

        group.throughput(Throughput::Elements(num_vars as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_vars), &num_vars, |b, _| {
            b.iter(|| {
                // Simulate variable resolution for plan mode
                let mut resolved = template.to_string();
                for (key, value) in &vars {
                    let pattern = format!("{{{{ {} }}}}", key);
                    if let Some(s) = value.as_str() {
                        resolved = resolved.replace(&pattern, s);
                    }
                }
                black_box(resolved)
            })
        });
    }

    group.finish();
}

fn bench_plan_vs_full_execution_overhead(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("plan_vs_full");

    let playbook_yaml = generate_plan_mode_playbook(20);
    let num_hosts = 5;
    let hosts: Vec<String> = (0..num_hosts).map(|i| format!("host{}", i)).collect();

    // Plan mode - just parse and analyze
    group.bench_function("plan_mode", |b| {
        b.iter(|| {
            let playbook: Vec<serde_yaml::Value> = serde_yaml::from_str(&playbook_yaml).unwrap();

            let mut plan_output = Vec::new();
            for play in &playbook {
                let play_name = play.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                    for task in tasks {
                        for host in &hosts {
                            let task_name = task.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            plan_output.push(format!("[{}] {} - {}", host, play_name, task_name));
                        }
                    }
                }
            }

            black_box(plan_output)
        })
    });

    // Simulated full execution (without actual SSH)
    group.bench_function("simulated_execution", |b| {
        b.to_async(&rt).iter(|| async {
            let playbook: Vec<serde_yaml::Value> = serde_yaml::from_str(&playbook_yaml).unwrap();

            let mut results = Vec::new();
            for play in &playbook {
                if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                    for task in tasks {
                        // Parallel execution across hosts
                        let mut handles = Vec::new();
                        for host in &hosts {
                            let host = host.clone();
                            let task_name = task
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string();

                            handles.push(tokio::spawn(async move {
                                // Simulate minimal task execution overhead
                                tokio::time::sleep(Duration::from_micros(10)).await;
                                (host, task_name, "ok")
                            }));
                        }

                        let task_results: Vec<_> = futures::future::join_all(handles)
                            .await
                            .into_iter()
                            .map(|r| r.unwrap())
                            .collect();
                        results.extend(task_results);
                    }
                }
            }

            black_box(results)
        })
    });

    group.finish();
}

// ============================================================================
// Parallelization Enforcement Benchmarks
// ============================================================================

fn bench_host_exclusive_semaphore(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("host_exclusive_semaphore");

    use parking_lot::Mutex;
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    for num_hosts in [5, 10, 20] {
        let semaphores: Arc<Mutex<HashMap<String, Arc<Semaphore>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Pre-create semaphores
        for i in 0..num_hosts {
            let mut sems = semaphores.lock();
            sems.insert(format!("host{}", i), Arc::new(Semaphore::new(1)));
        }

        group.throughput(Throughput::Elements(num_hosts as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_hosts),
            &num_hosts,
            |b, _| {
                b.to_async(&rt).iter(|| {
                    let semaphores = semaphores.clone();
                    async move {
                        let mut handles = Vec::new();

                        for i in 0..num_hosts {
                            let semaphores = semaphores.clone();
                            let host = format!("host{}", i);

                            handles.push(tokio::spawn(async move {
                                let sem = {
                                    let sems = semaphores.lock();
                                    sems.get(&host).unwrap().clone()
                                };

                                let _permit = sem.acquire().await.unwrap();
                                // Simulate work
                                tokio::time::sleep(Duration::from_micros(10)).await;
                                host
                            }));
                        }

                        let results: Vec<_> = futures::future::join_all(handles)
                            .await
                            .into_iter()
                            .map(|r| r.unwrap())
                            .collect();
                        black_box(results)
                    }
                })
            },
        );
    }

    group.finish();
}

fn bench_rate_limited_token_bucket(c: &mut Criterion) {
    let mut group = c.benchmark_group("rate_limited_token_bucket");

    #[derive(Clone)]
    struct TokenBucket {
        capacity: u32,
        tokens: f64,
        refill_rate: f64,
        last_refill: Instant,
    }

    impl TokenBucket {
        fn new(requests_per_second: u32) -> Self {
            Self {
                capacity: requests_per_second,
                tokens: requests_per_second as f64,
                refill_rate: requests_per_second as f64,
                last_refill: Instant::now(),
            }
        }

        fn try_acquire(&mut self) -> bool {
            self.refill();
            if self.tokens >= 1.0 {
                self.tokens -= 1.0;
                true
            } else {
                false
            }
        }

        fn refill(&mut self) {
            let now = Instant::now();
            let elapsed = now.duration_since(self.last_refill).as_secs_f64();
            self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity as f64);
            self.last_refill = now;
        }
    }

    for rate in [10, 50, 100, 500] {
        group.throughput(Throughput::Elements(rate as u64));
        group.bench_with_input(BenchmarkId::from_parameter(rate), &rate, |b, &rate| {
            b.iter(|| {
                let mut bucket = TokenBucket::new(rate);
                let mut acquired = 0;

                // Try to acquire tokens (should get up to capacity)
                for _ in 0..(rate * 2) {
                    if bucket.try_acquire() {
                        acquired += 1;
                    }
                }

                black_box(acquired)
            })
        });
    }

    group.finish();
}

fn bench_global_exclusive_mutex(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("global_exclusive_mutex");

    use tokio::sync::Semaphore;

    for concurrent_tasks in [5, 10, 20] {
        let global_mutex = Arc::new(Semaphore::new(1));

        group.bench_with_input(
            BenchmarkId::from_parameter(concurrent_tasks),
            &concurrent_tasks,
            |b, &num_tasks| {
                b.to_async(&rt).iter(|| {
                    let mutex = global_mutex.clone();
                    async move {
                        let mut handles = Vec::new();

                        for i in 0..num_tasks {
                            let mutex = mutex.clone();
                            handles.push(tokio::spawn(async move {
                                let _permit = mutex.acquire().await.unwrap();
                                // Minimal work while holding lock
                                tokio::time::sleep(Duration::from_micros(1)).await;
                                i
                            }));
                        }

                        let results: Vec<_> = futures::future::join_all(handles)
                            .await
                            .into_iter()
                            .map(|r| r.unwrap())
                            .collect();
                        black_box(results)
                    }
                })
            },
        );
    }

    group.finish();
}

fn bench_no_mutex_baseline(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("no_mutex_baseline");

    for concurrent_tasks in [5, 10, 20] {
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrent_tasks),
            &concurrent_tasks,
            |b, &num_tasks| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::new();

                    for i in 0..num_tasks {
                        handles.push(tokio::spawn(async move {
                            // Same work without any mutex
                            tokio::time::sleep(Duration::from_micros(1)).await;
                            i
                        }));
                    }

                    let results: Vec<_> = futures::future::join_all(handles)
                        .await
                        .into_iter()
                        .map(|r| r.unwrap())
                        .collect();
                    black_box(results)
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Criterion Groups and Main
// ============================================================================

criterion_group!(
    include_benches,
    bench_include_tasks_loading,
    bench_include_vars_parsing,
    bench_nested_includes,
    bench_inline_vs_include_comparison,
);

criterion_group!(
    delegation_benches,
    bench_delegate_to_parsing,
    bench_fact_assignment_overhead,
);

criterion_group!(
    serial_strategy_benches,
    bench_serial_spec_calculation,
    bench_batch_host_splitting,
    bench_serial_vs_free_simulation,
    bench_max_fail_percentage_check,
);

criterion_group!(
    plan_mode_benches,
    bench_plan_mode_execution,
    bench_variable_resolution_plan_mode,
    bench_plan_vs_full_execution_overhead,
);

criterion_group!(
    parallelization_benches,
    bench_host_exclusive_semaphore,
    bench_rate_limited_token_bucket,
    bench_global_exclusive_mutex,
    bench_no_mutex_baseline,
);

criterion_main!(
    include_benches,
    delegation_benches,
    serial_strategy_benches,
    plan_mode_benches,
    parallelization_benches,
);
