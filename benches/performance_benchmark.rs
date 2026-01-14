//! Comprehensive Performance Benchmark Suite for Rustible
//!
//! This benchmark suite provides in-depth performance testing for:
//!
//! 1. EXECUTION ENGINE:
//!    - Single task execution latency
//!    - Parallel execution across 10/100/1000 hosts
//!    - Task scheduling overhead
//!    - Handler execution timing
//!
//! 2. CONNECTION PERFORMANCE:
//!    - Connection establishment time
//!    - Connection pool hit/miss ratio impact
//!    - Concurrent connection limits
//!
//! 3. TEMPLATE RENDERING:
//!    - Simple variable substitution
//!    - Complex nested templates
//!    - Large variable contexts
//!    - Filter chain performance
//!
//! 4. INVENTORY SCALING:
//!    - 10/100/1000/10000 hosts parsing
//!    - Group resolution performance
//!    - Pattern matching efficiency
//!    - Host variable merging
//!
//! 5. MODULE PERFORMANCE:
//!    - Module dispatch overhead
//!    - Parameter validation
//!    - Output serialization

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, Semaphore};

use rustible::connection::{ConnectionConfig, ConnectionFactory};
use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::{ExecutionContext, RuntimeContext};
use rustible::executor::task::{Handler, Task, TaskResult};
use rustible::executor::{ExecutionStats, ExecutionStrategy, Executor, ExecutorConfig, HostResult};
use rustible::inventory::{Group, Host, Inventory};
use rustible::modules::{ModuleContext, ModuleOutput, ModuleParams, ModuleRegistry};
use rustible::template::TemplateEngine;

// ============================================================================
// DATA GENERATORS
// ============================================================================

/// Generate a simple task for benchmarking
fn generate_simple_task(name: &str) -> Task {
    Task::new(name, "debug").arg("msg", serde_json::json!("Hello, World!"))
}

/// Generate a task with conditions and variables
fn generate_complex_task(name: &str) -> Task {
    Task::new(name, "template")
        .arg("src", serde_json::json!("template.j2"))
        .arg(
            "dest",
            serde_json::json!("/etc/config/{{ item.name }}.conf"),
        )
        .arg("owner", serde_json::json!("root"))
        .arg("mode", serde_json::json!("0644"))
        .when("ansible_os_family == 'Debian'")
        .notify("restart service")
        .register("template_result")
}

/// Generate playbook YAML for different complexity levels
fn generate_playbook_yaml(num_tasks: usize, num_handlers: usize) -> String {
    let mut yaml = String::from(
        r#"
- name: Performance Test Play
  hosts: all
  gather_facts: false
  vars:
    test_var: "test_value"
    items:
      - name: config1
        value: 100
      - name: config2
        value: 200
  tasks:
"#,
    );

    for i in 0..num_tasks {
        yaml.push_str(&format!(
            r#"    - name: Task {}
      debug:
        msg: "Executing task {}"
      register: task_{}_result
"#,
            i, i, i
        ));
    }

    if num_handlers > 0 {
        yaml.push_str("  handlers:\n");
        for i in 0..num_handlers {
            yaml.push_str(&format!(
                r#"    - name: handler {}
      debug:
        msg: "Running handler {}"
"#,
                i, i
            ));
        }
    }

    yaml
}

/// Generate inventory YAML with specified number of hosts and groups
fn generate_large_inventory_yaml(num_hosts: usize, num_groups: usize) -> String {
    let hosts_per_group = (num_hosts / num_groups).max(1);
    let mut yaml = String::from("all:\n  children:\n");

    for g in 0..num_groups {
        yaml.push_str(&format!("    group_{:04}:\n      hosts:\n", g));
        let start = g * hosts_per_group;
        let end = ((g + 1) * hosts_per_group).min(num_hosts);
        for h in start..end {
            yaml.push_str(&format!(
                "        host{:05}:\n          ansible_host: 10.{}.{}.{}\n          ansible_port: 22\n          http_port: {}\n",
                h,
                (h / 65536) % 256,
                (h / 256) % 256,
                h % 256,
                8080 + (h % 100)
            ));
        }
        yaml.push_str(&format!("      vars:\n        group_id: {}\n", g));
    }

    yaml.push_str("  vars:\n    global_var: production\n    max_connections: 1000\n");
    yaml
}

/// Generate INI format inventory
fn generate_large_inventory_ini(num_hosts: usize) -> String {
    let mut ini = String::new();

    // Create multiple groups
    let groups = ["webservers", "databases", "caches", "loadbalancers"];
    let hosts_per_group = num_hosts / groups.len();

    for (idx, group) in groups.iter().enumerate() {
        ini.push_str(&format!("[{}]\n", group));
        for h in 0..hosts_per_group {
            let host_id = idx * hosts_per_group + h;
            ini.push_str(&format!(
                "{}_{:04} ansible_host=10.{}.{}.{} http_port={}\n",
                group,
                h,
                (host_id / 65536) % 256,
                (host_id / 256) % 256,
                host_id % 256,
                8080 + (h % 100)
            ));
        }
        ini.push('\n');
    }

    // Add group vars
    for group in groups.iter() {
        ini.push_str(&format!("[{}:vars]\napp_name={}_app\n\n", group, group));
    }

    // Add parent group
    ini.push_str("[production:children]\n");
    for group in groups.iter() {
        ini.push_str(&format!("{}\n", group));
    }

    ini
}

/// Generate template with varying complexity
fn generate_template(num_vars: usize, num_loops: usize, nested_depth: usize) -> String {
    let mut template = String::from("# Configuration Template\n\n");

    // Add simple variable substitutions
    for i in 0..num_vars {
        template.push_str(&format!("var_{}: {{{{ var_{} }}}}\n", i, i));
    }

    // Add loops
    for l in 0..num_loops {
        template.push_str(&format!(
            "\n# Loop {}\n{{% for item in loop_{}_items %}}\n  - name: {{{{ item.name }}}}\n    value: {{{{ item.value }}}}\n{{% endfor %}}\n",
            l, l
        ));
    }

    // Add nested structures
    for d in 0..nested_depth {
        template.push_str(&format!(
            "\nnested_{}: {{{{ config.level{}.setting }}}}\n",
            d, d
        ));
    }

    template
}

/// Generate variables for template rendering
fn generate_template_vars(
    num_vars: usize,
    num_loops: usize,
    nested_depth: usize,
) -> HashMap<String, serde_json::Value> {
    let mut vars = HashMap::new();

    // Simple variables
    for i in 0..num_vars {
        vars.insert(
            format!("var_{}", i),
            serde_json::json!(format!("value_{}", i)),
        );
    }

    // Loop items
    for l in 0..num_loops {
        let items: Vec<serde_json::Value> = (0..10)
            .map(|i| serde_json::json!({"name": format!("item_{}_{}", l, i), "value": i * 100}))
            .collect();
        vars.insert(format!("loop_{}_items", l), serde_json::json!(items));
    }

    // Nested config
    let mut config = serde_json::Map::new();
    for d in 0..nested_depth {
        let level = serde_json::json!({"setting": format!("level_{}_value", d)});
        config.insert(format!("level{}", d), level);
    }
    vars.insert("config".to_string(), serde_json::Value::Object(config));

    vars
}

// ============================================================================
// EXECUTION ENGINE BENCHMARKS
// ============================================================================

fn bench_single_task_execution_latency(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("execution_single_task");

    // Benchmark task creation
    group.bench_function("task_creation_simple", |b| {
        b.iter(|| {
            let task = generate_simple_task(black_box("test_task"));
            black_box(task)
        })
    });

    group.bench_function("task_creation_complex", |b| {
        b.iter(|| {
            let task = generate_complex_task(black_box("test_task"));
            black_box(task)
        })
    });

    // Benchmark task cloning (important for parallel execution)
    let simple_task = generate_simple_task("clone_test");
    let complex_task = generate_complex_task("clone_test");

    group.bench_function("task_clone_simple", |b| {
        b.iter(|| black_box(simple_task.clone()))
    });

    group.bench_function("task_clone_complex", |b| {
        b.iter(|| black_box(complex_task.clone()))
    });

    // Benchmark TaskResult creation
    group.bench_function("task_result_creation", |b| {
        b.iter(|| {
            let result = TaskResult::changed()
                .with_msg("Task completed successfully")
                .with_result(serde_json::json!({"key": "value", "count": 42}));
            black_box(result)
        })
    });

    group.finish();
}

fn bench_parallel_host_execution(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("execution_parallel_hosts");

    // Configure for throughput measurement
    group.sample_size(50);

    for num_hosts in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*num_hosts as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(num_hosts),
            num_hosts,
            |b, &num| {
                b.to_async(&rt).iter(|| async move {
                    // Simulate parallel execution with semaphore limiting
                    let semaphore = Arc::new(Semaphore::new(5)); // 5 forks
                    let results = Arc::new(Mutex::new(HashMap::new()));

                    let handles: Vec<_> = (0..num)
                        .map(|i| {
                            let sem = Arc::clone(&semaphore);
                            let res = Arc::clone(&results);
                            tokio::spawn(async move {
                                let _permit = sem.acquire().await.unwrap();
                                // Simulate minimal task execution work
                                tokio::task::yield_now().await;
                                res.lock().await.insert(
                                    format!("host_{}", i),
                                    HostResult {
                                        host: format!("host_{}", i),
                                        stats: ExecutionStats::default(),
                                        failed: false,
                                        unreachable: false,
                                    },
                                );
                            })
                        })
                        .collect();

                    for handle in handles {
                        black_box(handle.await).ok();
                    }

                    black_box(Arc::try_unwrap(results).ok())
                })
            },
        );
    }

    group.finish();
}

fn bench_task_scheduling_overhead(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("execution_scheduling");

    // Benchmark executor creation
    group.bench_function("executor_creation", |b| {
        b.iter(|| {
            let config = ExecutorConfig {
                forks: 5,
                check_mode: false,
                diff_mode: false,
                verbosity: 0,
                strategy: ExecutionStrategy::Linear,
                task_timeout: 300,
                gather_facts: false,
                extra_vars: HashMap::new(),
                ..Default::default()
            };
            let executor = Executor::new(black_box(config));
            black_box(executor)
        })
    });

    // Benchmark runtime context operations
    group.bench_function("runtime_context_creation", |b| {
        b.iter(|| {
            let ctx = RuntimeContext::new();
            black_box(ctx)
        })
    });

    group.bench_function("runtime_context_set_vars", |b| {
        let mut ctx = RuntimeContext::new();
        b.iter(|| {
            ctx.set_global_var(
                black_box("test_var".to_string()),
                black_box(serde_json::json!("test_value")),
            );
        })
    });

    group.bench_function("runtime_context_get_merged_vars", |b| {
        let mut ctx = RuntimeContext::new();
        // Add some hosts and variables
        for i in 0..10 {
            ctx.add_host(format!("host_{}", i), Some("webservers"));
            ctx.set_host_var(
                &format!("host_{}", i),
                "http_port".to_string(),
                serde_json::json!(8080 + i),
            );
        }
        ctx.set_global_var("env".to_string(), serde_json::json!("production"));
        ctx.set_play_var("app_name".to_string(), serde_json::json!("myapp"));

        b.iter(|| {
            let vars = ctx.get_merged_vars(black_box("host_5"));
            black_box(vars)
        })
    });

    // Benchmark execution context
    group.bench_function("execution_context_creation", |b| {
        b.iter(|| {
            let ctx = ExecutionContext::new(black_box("test_host"))
                .with_check_mode(false)
                .with_diff_mode(true);
            black_box(ctx)
        })
    });

    group.finish();
}

fn bench_handler_execution_timing(c: &mut Criterion) {
    let mut group = c.benchmark_group("execution_handlers");

    // Benchmark handler notification tracking
    group.bench_function("handler_notification_set", |b| {
        b.iter(|| {
            let mut notified: HashSet<String> = HashSet::new();
            for i in 0..10 {
                notified.insert(black_box(format!("handler_{}", i)));
            }
            black_box(notified)
        })
    });

    group.bench_function("handler_lookup_hashmap", |b| {
        let mut handlers: HashMap<String, Handler> = HashMap::new();
        for i in 0..20 {
            handlers.insert(
                format!("handler_{}", i),
                Handler {
                    name: format!("handler_{}", i),
                    module: "service".to_string(),
                    args: IndexMap::new(),
                    when: None,
                    listen: vec![],
                },
            );
        }

        b.iter(|| {
            let handler = handlers.get(black_box("handler_10"));
            black_box(handler)
        })
    });

    // Benchmark handler cloning (needed when converting to tasks)
    let handler = Handler {
        name: "restart nginx".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("nginx"));
            args.insert("state".to_string(), serde_json::json!("restarted"));
            args
        },
        when: Some("ansible_os_family == 'Debian'".to_string()),
        listen: vec!["reload nginx".to_string(), "nginx changed".to_string()],
    };

    group.bench_function("handler_clone", |b| b.iter(|| black_box(handler.clone())));

    group.finish();
}

// ============================================================================
// CONNECTION PERFORMANCE BENCHMARKS
// ============================================================================

fn bench_connection_establishment(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("connection_establishment");

    // Benchmark connection factory creation
    group.bench_function("factory_creation", |b| {
        b.iter(|| {
            let config = ConnectionConfig::default();
            let factory = ConnectionFactory::new(black_box(config));
            black_box(factory)
        })
    });

    group.bench_function("factory_creation_with_pool", |b| {
        b.iter(|| {
            let config = ConnectionConfig::default();
            let factory = ConnectionFactory::with_pool_size(black_box(config), 20);
            black_box(factory)
        })
    });

    // Benchmark local connection (fastest path)
    group.bench_function("local_connection", |b| {
        let factory = Arc::new(ConnectionFactory::new(ConnectionConfig::default()));
        b.to_async(&rt).iter(|| {
            let factory = Arc::clone(&factory);
            async move {
                let conn = factory.get_connection(black_box("localhost")).await;
                black_box(conn)
            }
        })
    });

    group.finish();
}

fn bench_connection_pool_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("connection_pool");

    // Benchmark pool hit (reusing connection)
    group.bench_function("pool_hit", |b| {
        let factory = Arc::new(ConnectionFactory::new(ConnectionConfig::default()));

        // Pre-populate the pool
        rt.block_on(async {
            let _ = factory.get_connection("localhost").await;
        });

        b.to_async(&rt).iter(|| {
            let factory = Arc::clone(&factory);
            async move {
                // This should hit the pool
                let conn = factory.get_connection(black_box("localhost")).await;
                black_box(conn)
            }
        })
    });

    // Benchmark pool stats
    group.bench_function("pool_stats", |b| {
        let factory = ConnectionFactory::new(ConnectionConfig::default());
        b.iter(|| {
            let stats = factory.pool_stats();
            black_box(stats)
        })
    });

    group.finish();
}

fn bench_concurrent_connection_limits(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("connection_concurrency");
    group.sample_size(30);

    for concurrent in [5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(concurrent),
            concurrent,
            |b, &num| {
                let factory = Arc::new(ConnectionFactory::with_pool_size(
                    ConnectionConfig::default(),
                    num,
                ));

                b.to_async(&rt).iter(|| {
                    let factory = Arc::clone(&factory);
                    async move {
                        // Sequential connection gets to avoid Send issues with parking_lot
                        for _ in 0..num {
                            let conn = factory.get_connection("localhost").await;
                            black_box(conn).ok();
                        }
                    }
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// TEMPLATE RENDERING BENCHMARKS
// ============================================================================

fn bench_template_simple_substitution(c: &mut Criterion) {
    let mut group = c.benchmark_group("template_simple");

    let engine = TemplateEngine::new();

    // Very simple template
    let simple_template = "Hello {{ name }}!";
    let mut simple_vars = HashMap::new();
    simple_vars.insert("name".to_string(), serde_json::json!("World"));

    group.bench_function("single_variable", |b| {
        b.iter(|| {
            let result = engine.render(black_box(simple_template), black_box(&simple_vars));
            black_box(result)
        })
    });

    // Multiple variables
    let multi_template = "Server: {{ server }}, Port: {{ port }}, User: {{ user }}";
    let mut multi_vars = HashMap::new();
    multi_vars.insert("server".to_string(), serde_json::json!("localhost"));
    multi_vars.insert("port".to_string(), serde_json::json!(8080));
    multi_vars.insert("user".to_string(), serde_json::json!("admin"));

    group.bench_function("multiple_variables", |b| {
        b.iter(|| {
            let result = engine.render(black_box(multi_template), black_box(&multi_vars));
            black_box(result)
        })
    });

    // Benchmark template detection
    group.bench_function("is_template_true", |b| {
        b.iter(|| TemplateEngine::is_template(black_box("Hello {{ name }}")))
    });

    group.bench_function("is_template_false", |b| {
        b.iter(|| TemplateEngine::is_template(black_box("Hello World")))
    });

    group.finish();
}

fn bench_template_complex_nested(c: &mut Criterion) {
    let mut group = c.benchmark_group("template_complex");

    let engine = TemplateEngine::new();

    // Test with different complexity levels
    for (num_vars, num_loops, nested_depth) in [(10, 2, 3), (50, 5, 5), (100, 10, 10)].iter() {
        let template = generate_template(*num_vars, *num_loops, *nested_depth);
        let vars = generate_template_vars(*num_vars, *num_loops, *nested_depth);

        let label = format!("vars{}_loops{}_depth{}", num_vars, num_loops, nested_depth);
        group.bench_function(&label, |b| {
            b.iter(|| {
                let result = engine.render(black_box(&template), black_box(&vars));
                black_box(result)
            })
        });
    }

    group.finish();
}

fn bench_template_large_context(c: &mut Criterion) {
    let mut group = c.benchmark_group("template_large_context");

    let engine = TemplateEngine::new();
    let simple_template = "Value: {{ var_50 }}";

    // Test with different context sizes
    for num_vars in [100, 500, 1000].iter() {
        let vars = generate_template_vars(*num_vars, 0, 0);

        group.throughput(Throughput::Elements(*num_vars as u64));
        group.bench_with_input(BenchmarkId::from_parameter(num_vars), num_vars, |b, _| {
            b.iter(|| {
                let result = engine.render(black_box(simple_template), black_box(&vars));
                black_box(result)
            })
        });
    }

    group.finish();
}

// ============================================================================
// INVENTORY SCALING BENCHMARKS
// ============================================================================

fn bench_inventory_parsing_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_parsing_scale");
    group.sample_size(20);

    // Test YAML parsing at different scales
    for num_hosts in [10, 100, 1000, 10000].iter() {
        if *num_hosts > 1000 {
            group.sample_size(10); // Fewer samples for very large inventories
        }

        let yaml = generate_large_inventory_yaml(*num_hosts, (*num_hosts / 50).max(1));
        group.throughput(Throughput::Elements(*num_hosts as u64));

        group.bench_with_input(
            BenchmarkId::new("yaml", num_hosts),
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

    // Test INI parsing at different scales
    for num_hosts in [10, 100, 1000].iter() {
        let ini = generate_large_inventory_ini(*num_hosts);
        group.throughput(Throughput::Elements(*num_hosts as u64));

        group.bench_with_input(
            BenchmarkId::new("ini", num_hosts),
            &ini,
            |b, ini_content| {
                b.iter(|| {
                    use std::io::Write;
                    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
                    tmpfile.write_all(ini_content.as_bytes()).unwrap();
                    tmpfile.flush().unwrap();
                    let result = Inventory::load(black_box(tmpfile.path()));
                    black_box(result)
                })
            },
        );
    }

    group.finish();
}

fn bench_group_resolution(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_group_resolution");

    // Create a large inventory
    let yaml = generate_large_inventory_yaml(1000, 20);
    use std::io::Write;
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    tmpfile.write_all(yaml.as_bytes()).unwrap();
    tmpfile.flush().unwrap();
    let inv = Inventory::load(tmpfile.path()).unwrap();

    // Benchmark group lookup
    group.bench_function("get_group", |b| {
        b.iter(|| {
            let g = inv.get_group(black_box("group_0010"));
            black_box(g)
        })
    });

    // Benchmark host lookup
    group.bench_function("get_host", |b| {
        b.iter(|| {
            let h = inv.get_host(black_box("host00500"));
            black_box(h)
        })
    });

    // Benchmark host iteration
    group.bench_function("iterate_hosts", |b| {
        b.iter(|| {
            let count = inv.hosts().count();
            black_box(count)
        })
    });

    // Benchmark group hierarchy
    group.bench_function("host_group_hierarchy", |b| {
        let host = inv.get_host("host00500").unwrap();
        b.iter(|| {
            let hierarchy = inv.get_host_group_hierarchy(black_box(host));
            black_box(hierarchy)
        })
    });

    group.finish();
}

fn bench_pattern_matching_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_pattern_matching");

    // Create inventory
    let yaml = generate_large_inventory_yaml(1000, 20);
    use std::io::Write;
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    tmpfile.write_all(yaml.as_bytes()).unwrap();
    tmpfile.flush().unwrap();
    let inv = Inventory::load(tmpfile.path()).unwrap();

    // Test different pattern types
    let patterns = vec![
        ("all", "all hosts"),
        ("group_0010", "single group"),
        ("host00*", "glob wildcard"),
        ("~host00\\d{3}", "regex pattern"),
        ("group_0001:group_0002", "union"),
        ("all:!group_0010", "exclusion"),
    ];

    for (pattern, name) in patterns {
        group.bench_function(name, |b| {
            b.iter(|| {
                let result = inv.get_hosts_for_pattern(black_box(pattern));
                black_box(result)
            })
        });
    }

    group.finish();
}

fn bench_host_variable_merging(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_var_merging");

    // Create inventory with hierarchical groups and vars
    let mut inv = Inventory::new();

    // Create group hierarchy: all -> production -> webservers -> web001
    let mut all_group = Group::new("all");
    all_group.set_var("global_var", serde_yaml::Value::String("global".into()));
    all_group.add_child("production".to_string());
    inv.add_group(all_group).unwrap();

    let mut prod_group = Group::new("production");
    prod_group.set_var("env", serde_yaml::Value::String("prod".into()));
    prod_group.set_var(
        "global_var",
        serde_yaml::Value::String("prod_override".into()),
    );
    prod_group.add_child("webservers".to_string());
    inv.add_group(prod_group).unwrap();

    let mut web_group = Group::new("webservers");
    web_group.set_var("http_port", serde_yaml::Value::Number(80.into()));
    web_group.set_var("max_clients", serde_yaml::Value::Number(100.into()));
    inv.add_group(web_group).unwrap();

    // Add hosts
    for i in 0..100 {
        let mut host = Host::new(format!("web{:03}", i));
        host.set_var(
            "ansible_host",
            serde_yaml::Value::String(format!("10.0.0.{}", i)),
        );
        host.set_var("host_id", serde_yaml::Value::Number(i.into()));
        host.add_to_group("webservers".to_string());
        host.add_to_group("production".to_string());
        host.add_to_group("all".to_string());
        inv.add_host(host).unwrap();
    }

    // Benchmark variable merging
    let host = inv.get_host("web050").unwrap();
    group.bench_function("get_host_vars", |b| {
        b.iter(|| {
            let vars = inv.get_host_vars(black_box(host));
            black_box(vars)
        })
    });

    group.finish();
}

// ============================================================================
// MODULE PERFORMANCE BENCHMARKS
// ============================================================================

fn bench_module_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_dispatch");

    let registry = ModuleRegistry::with_builtins();

    // Benchmark module lookup
    group.bench_function("module_lookup_exists", |b| {
        b.iter(|| {
            let module = registry.get(black_box("command"));
            black_box(module)
        })
    });

    group.bench_function("module_lookup_missing", |b| {
        b.iter(|| {
            let module = registry.get(black_box("nonexistent_module"));
            black_box(module)
        })
    });

    // Benchmark module contains check
    group.bench_function("module_contains", |b| {
        b.iter(|| registry.contains(black_box("shell")))
    });

    // Benchmark listing modules
    group.bench_function("module_list_names", |b| {
        b.iter(|| {
            let names = registry.names();
            black_box(names)
        })
    });

    group.finish();
}

fn bench_module_execution_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_execution");

    let _registry = ModuleRegistry::with_builtins();
    let _context = ModuleContext::new().with_check_mode(true);

    // Benchmark parameter creation
    group.bench_function("params_creation", |b| {
        b.iter(|| {
            let mut params: ModuleParams = HashMap::new();
            params.insert("cmd".to_string(), serde_json::json!("echo hello"));
            params.insert("chdir".to_string(), serde_json::json!("/tmp"));
            params.insert("creates".to_string(), serde_json::json!("/tmp/marker"));
            black_box(params)
        })
    });

    // Benchmark module output creation
    group.bench_function("output_creation_ok", |b| {
        b.iter(|| {
            let output = ModuleOutput::ok(black_box("Command succeeded"));
            black_box(output)
        })
    });

    group.bench_function("output_creation_changed", |b| {
        b.iter(|| {
            let output = ModuleOutput::changed(black_box("File modified"))
                .with_data("path", serde_json::json!("/etc/config.conf"))
                .with_data("mode", serde_json::json!("0644"));
            black_box(output)
        })
    });

    // Benchmark context creation
    group.bench_function("context_creation", |b| {
        b.iter(|| {
            let ctx = ModuleContext::new()
                .with_check_mode(true)
                .with_diff_mode(true)
                .with_vars(HashMap::new())
                .with_facts(HashMap::new());
            black_box(ctx)
        })
    });

    group.finish();
}

fn bench_output_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_serialization");

    // Create various outputs
    let simple_output = ModuleOutput::ok("Success");
    let complex_output = ModuleOutput::changed("Configuration updated")
        .with_data("path", serde_json::json!("/etc/nginx/nginx.conf"))
        .with_data("owner", serde_json::json!("root"))
        .with_data("group", serde_json::json!("root"))
        .with_data("mode", serde_json::json!("0644"))
        .with_data("backup", serde_json::json!("/etc/nginx/nginx.conf.backup"))
        .with_command_output(
            Some("nginx: configuration file /etc/nginx/nginx.conf test is successful".to_string()),
            None,
            Some(0),
        );

    group.bench_function("serialize_simple", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&simple_output));
            black_box(json)
        })
    });

    group.bench_function("serialize_complex", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&complex_output));
            black_box(json)
        })
    });

    group.finish();
}

// ============================================================================
// PLAYBOOK PARSING BENCHMARKS
// ============================================================================

fn bench_playbook_parsing_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("playbook_parsing");

    // Test with different numbers of tasks
    for (num_tasks, num_handlers) in [(5, 2), (20, 5), (50, 10), (100, 20)].iter() {
        let yaml = generate_playbook_yaml(*num_tasks, *num_handlers);

        group.bench_with_input(
            BenchmarkId::new("tasks", num_tasks),
            &yaml,
            |b, yaml_content| {
                b.iter(|| {
                    let result = Playbook::parse(black_box(yaml_content), None);
                    black_box(result)
                })
            },
        );
    }

    group.finish();
}

fn bench_play_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("play_construction");

    group.bench_function("play_new", |b| {
        b.iter(|| {
            let play = Play::new(black_box("Configure webservers"), black_box("webservers"));
            black_box(play)
        })
    });

    // Benchmark play with tasks
    group.bench_function("play_with_tasks", |b| {
        b.iter(|| {
            let mut play = Play::new("Test Play", "all");
            for i in 0..10 {
                play.add_task(generate_simple_task(&format!("task_{}", i)));
            }
            black_box(play)
        })
    });

    group.finish();
}

// ============================================================================
// CRITERION GROUPS AND MAIN
// ============================================================================

criterion_group!(
    execution_benches,
    bench_single_task_execution_latency,
    bench_parallel_host_execution,
    bench_task_scheduling_overhead,
    bench_handler_execution_timing,
);

criterion_group!(
    connection_benches,
    bench_connection_establishment,
    bench_connection_pool_operations,
    bench_concurrent_connection_limits,
);

criterion_group!(
    template_benches,
    bench_template_simple_substitution,
    bench_template_complex_nested,
    bench_template_large_context,
);

criterion_group!(
    inventory_benches,
    bench_inventory_parsing_scale,
    bench_group_resolution,
    bench_pattern_matching_efficiency,
    bench_host_variable_merging,
);

criterion_group!(
    module_benches,
    bench_module_dispatch,
    bench_module_execution_overhead,
    bench_output_serialization,
);

criterion_group!(
    playbook_benches,
    bench_playbook_parsing_scaling,
    bench_play_construction,
);

criterion_main!(
    execution_benches,
    connection_benches,
    template_benches,
    inventory_benches,
    module_benches,
    playbook_benches,
);
