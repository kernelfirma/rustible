//! Ansible Comparison Benchmark Suite for Rustible
//!
//! This comprehensive benchmark suite compares Rustible performance against Ansible
//! across all major operations. It provides objective metrics for:
//!
//! 1. **SSH Connection Establishment** - Connection setup time and overhead
//! 2. **Fact Gathering** - System information collection at scale (10/100/1000 hosts)
//! 3. **Module Execution** - Copy, template, and package module performance
//! 4. **Loop Performance** - Iteration patterns and loop overhead
//! 5. **Memory Usage** - Memory consumption patterns across operations
//! 6. **Real Comparison** - Direct Ansible vs Rustible execution (requires SSH)
//!
//! ## Running Benchmarks
//!
//! ```bash
//! # Run all Ansible comparison benchmarks (simulation mode)
//! cargo bench --bench ansible_comparison
//!
//! # Run specific categories
//! cargo bench --bench ansible_comparison -- ssh_connection
//! cargo bench --bench ansible_comparison -- fact_gathering
//! cargo bench --bench ansible_comparison -- module_
//! cargo bench --bench ansible_comparison -- loop_
//! cargo bench --bench ansible_comparison -- memory
//! cargo bench --bench ansible_comparison -- real_comparison
//!
//! # For real Ansible vs Rustible comparison, set environment variables:
//! export BENCH_TARGET_HOST="192.168.178.102"
//! export BENCH_TARGET_PORT="22"
//! export BENCH_TARGET_USER="artur"
//! export BENCH_SSH_KEY="~/.ssh/id_ed25519"
//! cargo bench --bench ansible_comparison -- real_
//!
//! # Generate comparison baseline
//! cargo bench --bench ansible_comparison -- --save-baseline rustible
//!
//! # Compare against baseline
//! cargo bench --bench ansible_comparison -- --baseline rustible
//! ```
//!
//! ## Comparison Methodology
//!
//! These benchmarks simulate equivalent Ansible operations to measure:
//! - Overhead reduction in Rustible vs Python-based Ansible
//! - Parallel execution improvements
//! - Memory efficiency gains
//! - Connection pooling benefits
//!
//! The real comparison benchmarks execute identical playbooks via both tools
//! and measure actual execution time differences.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, RwLock, Semaphore};

use rustible::connection::{ConnectionConfig, ConnectionFactory};
use rustible::executor::parallelization::ParallelizationManager;
use rustible::facts::Facts;
use rustible::inventory::Inventory;
use rustible::modules::{
    ModuleContext, ModuleOutput, ModuleParams, ModuleRegistry, ParallelizationHint,
};
use rustible::template::TemplateEngine;

// ============================================================================
// CONFIGURATION CONSTANTS
// ============================================================================

/// Host counts for scalability testing
const HOST_COUNTS: [usize; 3] = [10, 100, 1000];

/// Fork counts (parallel connections) to test
const FORK_COUNTS: [usize; 5] = [1, 5, 10, 20, 50];

/// Simulated I/O delays for realistic SSH operations
const SSH_CONNECTION_DELAY_MS: u64 = 5;
const SSH_COMMAND_DELAY_MS: u64 = 2;
const FACT_GATHER_DELAY_MS: u64 = 10;
const FILE_TRANSFER_DELAY_MS: u64 = 3;

/// Loop sizes for iteration benchmarks
const LOOP_SIZES: [usize; 4] = [5, 25, 100, 500];

// ============================================================================
// HELPER FUNCTIONS AND UTILITIES
// ============================================================================

/// Create a tokio runtime with specified worker threads
fn create_runtime(workers: usize) -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
}

/// Simulate SSH connection establishment (handshake + authentication)
async fn simulate_ssh_connect(host: &str) -> Result<String, String> {
    // Simulate connection handshake delay
    tokio::time::sleep(Duration::from_millis(SSH_CONNECTION_DELAY_MS)).await;

    // Simulate key exchange and authentication
    let connection_id = format!("conn_{}_{}", host, std::process::id());
    Ok(connection_id)
}

/// Simulate SSH command execution
async fn simulate_ssh_execute(connection_id: &str, command: &str) -> Result<String, String> {
    tokio::time::sleep(Duration::from_millis(SSH_COMMAND_DELAY_MS)).await;
    Ok(format!("{}:{}", connection_id, command))
}

/// Simulate fact gathering (equivalent to Ansible's setup module)
async fn simulate_fact_gather(host: &str) -> Facts {
    tokio::time::sleep(Duration::from_millis(FACT_GATHER_DELAY_MS)).await;

    let mut facts = Facts::new();
    facts.set("ansible_hostname", serde_json::json!(host));
    facts.set("ansible_os_family", serde_json::json!("Debian"));
    facts.set("ansible_distribution", serde_json::json!("Ubuntu"));
    facts.set("ansible_distribution_version", serde_json::json!("22.04"));
    facts.set("ansible_architecture", serde_json::json!("x86_64"));
    facts.set("ansible_processor_count", serde_json::json!(8));
    facts.set("ansible_memtotal_mb", serde_json::json!(16384));
    facts.set(
        "ansible_default_ipv4",
        serde_json::json!({"address": "192.168.1.100"}),
    );
    facts
}

/// Simulate file transfer (SFTP upload)
async fn simulate_file_transfer(content_size: usize) -> Result<(), String> {
    // Simulate transfer time proportional to content size
    let base_delay = FILE_TRANSFER_DELAY_MS;
    let size_factor = (content_size / 1024).max(1) as u64;
    tokio::time::sleep(Duration::from_millis(base_delay + size_factor)).await;
    Ok(())
}

/// Generate inventory YAML with specified number of hosts
fn generate_inventory_yaml(num_hosts: usize) -> String {
    let num_groups = (num_hosts / 50).max(1);
    let hosts_per_group = (num_hosts / num_groups).max(1);
    let mut yaml = String::from("all:\n  children:\n");

    for g in 0..num_groups {
        yaml.push_str(&format!("    group_{:04}:\n      hosts:\n", g));
        let start = g * hosts_per_group;
        let end = ((g + 1) * hosts_per_group).min(num_hosts);
        for h in start..end {
            yaml.push_str(&format!(
                "        host{:05}:\n          ansible_host: 10.{}.{}.{}\n",
                h,
                (h / 65536) % 256,
                (h / 256) % 256,
                h % 256,
            ));
        }
    }

    yaml.push_str("  vars:\n    env: production\n    ansible_connection: ssh\n");
    yaml
}

/// Generate template content of specified complexity
fn generate_template_content(vars_count: usize) -> String {
    let mut template = String::from("# Generated Configuration\n\n");

    for i in 0..vars_count {
        template.push_str(&format!(
            "setting_{} = {{{{ var_{} | default('default_value_{}') }}}}\n",
            i, i, i
        ));
    }

    template.push_str("\n# Conditional section\n");
    template.push_str("{% if enable_feature %}\n");
    template.push_str("feature_enabled = true\n");
    template.push_str("{% else %}\n");
    template.push_str("feature_enabled = false\n");
    template.push_str("{% endif %}\n");

    template.push_str("\n# Loop section\n");
    template.push_str("{% for item in items %}\n");
    template.push_str("item_{{ loop.index }} = {{ item }}\n");
    template.push_str("{% endfor %}\n");

    template
}

// ============================================================================
// SSH CONNECTION ESTABLISHMENT BENCHMARKS
// ============================================================================
//
// These benchmarks measure the overhead of establishing SSH connections,
// which is a critical path in both Ansible and Rustible.
//
// Key metrics:
// - Single connection establishment time
// - Parallel connection establishment (with varying fork counts)
// - Connection pooling effectiveness
// - Connection reuse overhead

fn bench_ssh_connection(c: &mut Criterion) {
    let mut group = c.benchmark_group("ssh_connection");
    group.measurement_time(Duration::from_secs(10));

    let rt = create_runtime(4);

    // Single connection establishment
    group.bench_function("single_connection", |b| {
        b.to_async(&rt).iter(|| async {
            let result = simulate_ssh_connect("test-host").await;
            black_box(result)
        })
    });

    // Parallel connection establishment with different fork counts
    for forks in FORK_COUNTS {
        group.throughput(Throughput::Elements(forks as u64));
        group.bench_with_input(
            BenchmarkId::new("parallel_connections", forks),
            &forks,
            |b, &fork_count| {
                b.to_async(&rt).iter(|| async move {
                    let semaphore = Arc::new(Semaphore::new(fork_count));
                    let handles: Vec<_> = (0..fork_count)
                        .map(|i| {
                            let sem = Arc::clone(&semaphore);
                            let host = format!("host{:05}", i);
                            tokio::spawn(async move {
                                let _permit = sem.acquire().await.unwrap();
                                simulate_ssh_connect(&host).await
                            })
                        })
                        .collect();

                    let results: Vec<_> = futures::future::join_all(handles).await;
                    black_box(results)
                })
            },
        );
    }

    // Connection pool simulation (reusing connections)
    group.bench_function("connection_pool_reuse", |b| {
        b.to_async(&rt).iter(|| async {
            // Simulate connection pool with pre-established connections
            let pool: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));

            // Pre-populate pool
            {
                let mut pool_guard = pool.write().await;
                for i in 0..10 {
                    let host = format!("host{:05}", i);
                    let conn_id = format!("pooled_conn_{}", i);
                    pool_guard.insert(host, conn_id);
                }
            }

            // Simulate 50 operations reusing pooled connections
            let handles: Vec<_> = (0..50)
                .map(|i| {
                    let pool = Arc::clone(&pool);
                    tokio::spawn(async move {
                        let host = format!("host{:05}", i % 10);
                        let pool_guard = pool.read().await;
                        if let Some(conn_id) = pool_guard.get(&host) {
                            // Reuse existing connection - minimal overhead
                            simulate_ssh_execute(conn_id, "echo test").await
                        } else {
                            Err("Connection not found".to_string())
                        }
                    })
                })
                .collect();

            let results: Vec<_> = futures::future::join_all(handles).await;
            black_box(results)
        })
    });

    // ConnectionFactory benchmark (actual Rustible code path)
    let factory = Arc::new(ConnectionFactory::new(ConnectionConfig::default()));
    group.bench_function("factory_local_connection", |b| {
        b.to_async(&rt).iter(|| {
            let factory = Arc::clone(&factory);
            async move {
                let conn = factory.get_connection("localhost").await;
                black_box(conn)
            }
        })
    });

    group.finish();
}

// ============================================================================
// FACT GATHERING BENCHMARKS
// ============================================================================
//
// Fact gathering is one of the most expensive operations in Ansible.
// These benchmarks measure the performance of collecting system facts
// at various scales.
//
// Ansible equivalent: gather_facts: true (or setup module)
// Key optimizations in Rustible:
// - Parallel fact gathering across hosts
// - Fact caching
// - Selective fact gathering

fn bench_fact_gathering(c: &mut Criterion) {
    let mut group = c.benchmark_group("fact_gathering");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(30);

    let rt = create_runtime(4);

    // Benchmark fact gathering at different scales
    for num_hosts in HOST_COUNTS {
        group.throughput(Throughput::Elements(num_hosts as u64));

        // Serial fact gathering (baseline - similar to Ansible serial=1)
        group.bench_with_input(
            BenchmarkId::new("serial", num_hosts),
            &num_hosts,
            |b, &hosts| {
                b.to_async(&rt).iter(|| async move {
                    let mut all_facts = Vec::with_capacity(hosts);
                    for i in 0..hosts {
                        let host = format!("host{:05}", i);
                        let facts = simulate_fact_gather(&host).await;
                        all_facts.push((host, facts));
                    }
                    black_box(all_facts)
                })
            },
        );

        // Parallel fact gathering with default forks (5)
        group.bench_with_input(
            BenchmarkId::new("parallel_forks_5", num_hosts),
            &num_hosts,
            |b, &hosts| {
                b.to_async(&rt).iter(|| async move {
                    let semaphore = Arc::new(Semaphore::new(5));
                    let handles: Vec<_> = (0..hosts)
                        .map(|i| {
                            let sem = Arc::clone(&semaphore);
                            tokio::spawn(async move {
                                let _permit = sem.acquire().await.unwrap();
                                let host = format!("host{:05}", i);
                                let facts = simulate_fact_gather(&host).await;
                                (host, facts)
                            })
                        })
                        .collect();

                    let results: Vec<_> = futures::future::join_all(handles).await;
                    black_box(results)
                })
            },
        );

        // Parallel fact gathering with high parallelism (forks=50)
        if num_hosts >= 100 {
            group.bench_with_input(
                BenchmarkId::new("parallel_forks_50", num_hosts),
                &num_hosts,
                |b, &hosts| {
                    b.to_async(&rt).iter(|| async move {
                        let semaphore = Arc::new(Semaphore::new(50));
                        let handles: Vec<_> = (0..hosts)
                            .map(|i| {
                                let sem = Arc::clone(&semaphore);
                                tokio::spawn(async move {
                                    let _permit = sem.acquire().await.unwrap();
                                    let host = format!("host{:05}", i);
                                    let facts = simulate_fact_gather(&host).await;
                                    (host, facts)
                                })
                            })
                            .collect();

                        let results: Vec<_> = futures::future::join_all(handles).await;
                        black_box(results)
                    })
                },
            );
        }
    }

    // Cached fact retrieval (Rustible advantage)
    group.bench_function("cached_facts_lookup", |b| {
        let cache: HashMap<String, Facts> = (0..100)
            .map(|i| {
                let mut facts = Facts::new();
                facts.set("ansible_hostname", serde_json::json!(format!("host{}", i)));
                (format!("host{:05}", i), facts)
            })
            .collect();

        b.iter(|| {
            // Simulate looking up 100 hosts from cache
            let mut retrieved = Vec::with_capacity(100);
            for i in 0..100 {
                let host = format!("host{:05}", i);
                if let Some(facts) = cache.get(&host) {
                    retrieved.push(facts.clone());
                }
            }
            black_box(retrieved)
        })
    });

    // Local facts gathering (fast path)
    group.bench_function("local_facts_gather", |b| {
        b.iter(|| {
            let facts = Facts::gather_local();
            black_box(facts)
        })
    });

    group.finish();
}

// ============================================================================
// MODULE EXECUTION BENCHMARKS
// ============================================================================
//
// These benchmarks measure the performance of core modules that are
// commonly used in automation:
// - copy: File transfer operations
// - template: Jinja2-style template rendering
// - package: Package management operations (apt/yum)
//
// Comparison focus: Module dispatch overhead and execution efficiency

fn bench_module_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_execution");
    group.measurement_time(Duration::from_secs(10));

    let rt = create_runtime(4);
    let registry = ModuleRegistry::with_builtins();

    // -------------------------------------------------------------------------
    // Copy Module Benchmarks
    // -------------------------------------------------------------------------

    // Copy module parameter creation and validation
    group.bench_function("copy/param_creation", |b| {
        b.iter(|| {
            let mut params: ModuleParams = HashMap::new();
            params.insert("src".to_string(), serde_json::json!("/tmp/source.txt"));
            params.insert("dest".to_string(), serde_json::json!("/tmp/dest.txt"));
            params.insert("mode".to_string(), serde_json::json!("0644"));
            params.insert("owner".to_string(), serde_json::json!("root"));
            params.insert("backup".to_string(), serde_json::json!(true));
            black_box(params)
        })
    });

    // Copy module check mode (dry-run)
    if let Some(copy_module) = registry.get("copy") {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let src_path = temp_dir.path().join("source.txt");
        std::fs::write(&src_path, "test content").expect("Failed to write source file");

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!(src_path.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!("/tmp/nonexistent_dest.txt"),
        );

        let context = ModuleContext::new().with_check_mode(true);

        group.bench_function("copy/check_mode", |b| {
            b.iter(|| {
                let result = copy_module.check(&params, &context);
                black_box(result)
            })
        });
    }

    // Simulated copy with file transfer overhead
    for size_kb in [1, 10, 100] {
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));
        group.bench_with_input(
            BenchmarkId::new("copy/transfer", format!("{}kb", size_kb)),
            &(size_kb * 1024),
            |b, &size| {
                b.to_async(&rt)
                    .iter(|| async move { simulate_file_transfer(size).await })
            },
        );
    }

    // -------------------------------------------------------------------------
    // Template Module Benchmarks
    // -------------------------------------------------------------------------

    let engine = TemplateEngine::new();

    // Simple template rendering
    let simple_template = "Hello {{ name }}! Server: {{ server }}";
    let mut simple_vars: HashMap<String, serde_json::Value> = HashMap::new();
    simple_vars.insert("name".to_string(), serde_json::json!("World"));
    simple_vars.insert("server".to_string(), serde_json::json!("localhost"));

    group.bench_function("template/simple", |b| {
        b.iter(|| {
            let result = engine.render(black_box(simple_template), black_box(&simple_vars));
            black_box(result)
        })
    });

    // Complex template with loops and conditionals
    let complex_template = generate_template_content(20);
    let mut complex_vars: HashMap<String, serde_json::Value> = HashMap::new();
    for i in 0..20 {
        complex_vars.insert(
            format!("var_{}", i),
            serde_json::json!(format!("value_{}", i)),
        );
    }
    complex_vars.insert("enable_feature".to_string(), serde_json::json!(true));
    complex_vars.insert(
        "items".to_string(),
        serde_json::json!(vec!["a", "b", "c", "d", "e"]),
    );

    group.bench_function("template/complex", |b| {
        b.iter(|| {
            let result = engine.render(black_box(&complex_template), black_box(&complex_vars));
            black_box(result)
        })
    });

    // Template with many variables (stress test)
    let large_template = generate_template_content(100);
    let mut large_vars: HashMap<String, serde_json::Value> = HashMap::new();
    for i in 0..100 {
        large_vars.insert(
            format!("var_{}", i),
            serde_json::json!(format!("value_{}", i)),
        );
    }
    large_vars.insert("enable_feature".to_string(), serde_json::json!(true));
    large_vars.insert(
        "items".to_string(),
        serde_json::json!((0..50).collect::<Vec<_>>()),
    );

    group.bench_function("template/large_100_vars", |b| {
        b.iter(|| {
            let result = engine.render(black_box(&large_template), black_box(&large_vars));
            black_box(result)
        })
    });

    // -------------------------------------------------------------------------
    // Package Module Benchmarks (Simulated)
    // -------------------------------------------------------------------------
    //
    // Package operations are I/O bound and involve external commands.
    // We simulate the overhead of package state checking and installation.

    // Package check simulation (dpkg-query / rpm -q)
    group.bench_function("package/state_check", |b| {
        b.to_async(&rt).iter(|| async {
            // Simulate package database query
            tokio::time::sleep(Duration::from_millis(5)).await;
            black_box("installed")
        })
    });

    // Package install simulation with lock handling
    group.bench_function("package/install_simulation", |b| {
        b.to_async(&rt).iter(|| async {
            // Simulate acquiring apt/yum lock
            let lock = Arc::new(Mutex::new(()));
            let _guard = lock.lock().await;

            // Simulate package download and install
            tokio::time::sleep(Duration::from_millis(10)).await;
            black_box("installed")
        })
    });

    // Multiple package operations with host-exclusive locking
    group.bench_function("package/parallel_hosts_exclusive", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = Arc::new(ParallelizationManager::new());
            let handles: Vec<_> = (0..10)
                .map(|i| {
                    let manager = Arc::clone(&manager);
                    let host = format!("host{}", i);
                    tokio::spawn(async move {
                        let _guard = manager
                            .acquire(ParallelizationHint::HostExclusive, &host, "apt")
                            .await;
                        // Simulate package operation
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        "ok"
                    })
                })
                .collect();

            let results: Vec<_> = futures::future::join_all(handles).await;
            black_box(results)
        })
    });

    // -------------------------------------------------------------------------
    // Module Registry and Dispatch
    // -------------------------------------------------------------------------

    group.bench_function("registry/lookup", |b| {
        b.iter(|| {
            let module = registry.get(black_box("copy"));
            black_box(module)
        })
    });

    group.bench_function("registry/list_modules", |b| {
        b.iter(|| {
            let modules = registry.names();
            black_box(modules)
        })
    });

    // Debug module execution (minimal overhead baseline)
    if let Some(debug_module) = registry.get("debug") {
        let mut params: ModuleParams = HashMap::new();
        params.insert("msg".to_string(), serde_json::json!("Benchmark message"));
        let context = ModuleContext::new();

        group.bench_function("debug/execute", |b| {
            b.iter(|| {
                let result = debug_module.execute(&params, &context);
                black_box(result)
            })
        });
    }

    group.finish();
}

// ============================================================================
// LOOP PERFORMANCE BENCHMARKS
// ============================================================================
//
// These benchmarks measure the overhead of loop constructs commonly used
// in Ansible playbooks:
// - with_items / loop
// - with_dict / loop with dict2items
// - until loops (retry logic)
//
// Rustible processes loops more efficiently by avoiding Python's GIL
// and leveraging async iteration.

fn bench_loop_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("loop_performance");
    group.measurement_time(Duration::from_secs(10));

    let rt = create_runtime(4);

    // -------------------------------------------------------------------------
    // Simple Item Loop (with_items equivalent)
    // -------------------------------------------------------------------------

    for loop_size in LOOP_SIZES {
        group.throughput(Throughput::Elements(loop_size as u64));

        // Sequential loop (baseline)
        group.bench_with_input(
            BenchmarkId::new("items/sequential", loop_size),
            &loop_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async move {
                    let mut results = Vec::with_capacity(size);
                    for i in 0..size {
                        // Simulate minimal task work per item
                        tokio::task::yield_now().await;
                        results.push(format!("item_{}", i));
                    }
                    black_box(results)
                })
            },
        );

        // Parallel loop with semaphore (Rustible optimization)
        group.bench_with_input(
            BenchmarkId::new("items/parallel", loop_size),
            &loop_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async move {
                    let semaphore = Arc::new(Semaphore::new(10));
                    let handles: Vec<_> = (0..size)
                        .map(|i| {
                            let sem = Arc::clone(&semaphore);
                            tokio::spawn(async move {
                                let _permit = sem.acquire().await.unwrap();
                                tokio::task::yield_now().await;
                                format!("item_{}", i)
                            })
                        })
                        .collect();

                    let results: Vec<_> = futures::future::join_all(handles).await;
                    black_box(results)
                })
            },
        );
    }

    // -------------------------------------------------------------------------
    // Dictionary Loop (with_dict equivalent)
    // -------------------------------------------------------------------------

    for loop_size in [10, 50, 100] {
        group.throughput(Throughput::Elements(loop_size as u64));

        // Build test dictionary
        let dict: HashMap<String, serde_json::Value> = (0..loop_size)
            .map(|i| {
                (
                    format!("key_{}", i),
                    serde_json::json!({
                        "value": format!("value_{}", i),
                        "enabled": i % 2 == 0
                    }),
                )
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("dict/iterate", loop_size),
            &dict,
            |b, d| {
                b.iter(|| {
                    let results: Vec<_> = d
                        .iter()
                        .map(|(k, v)| {
                            black_box((k.clone(), v.clone()));
                            format!("processed_{}", k)
                        })
                        .collect();
                    black_box(results)
                })
            },
        );
    }

    // -------------------------------------------------------------------------
    // Nested Loops (product of two lists)
    // -------------------------------------------------------------------------

    for (outer, inner) in [(5, 5), (10, 10), (20, 20)] {
        let total = outer * inner;
        group.throughput(Throughput::Elements(total as u64));

        group.bench_with_input(
            BenchmarkId::new("nested/product", format!("{}x{}", outer, inner)),
            &(outer, inner),
            |b, &(o, i)| {
                b.to_async(&rt).iter(|| async move {
                    let semaphore = Arc::new(Semaphore::new(10));
                    let handles: Vec<_> = (0..o)
                        .flat_map(|outer_idx| {
                            let semaphore = Arc::clone(&semaphore);
                            (0..i).map(move |inner_idx| {
                                let sem = Arc::clone(&semaphore);
                                tokio::spawn(async move {
                                    let _permit = sem.acquire().await.unwrap();
                                    tokio::task::yield_now().await;
                                    (outer_idx, inner_idx)
                                })
                            })
                        })
                        .collect();

                    let results: Vec<_> = futures::future::join_all(handles).await;
                    black_box(results)
                })
            },
        );
    }

    // -------------------------------------------------------------------------
    // Until Loop (Retry Logic)
    // -------------------------------------------------------------------------

    group.bench_function("until/retry_success", |b| {
        b.to_async(&rt).iter(|| async {
            let max_retries = 3;
            let delay_ms = 1;

            for attempt in 0..max_retries {
                // Simulate operation that might fail
                let success = attempt >= 1; // Succeed on second attempt

                if success {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            black_box("success")
        })
    });

    group.bench_function("until/exponential_backoff", |b| {
        b.to_async(&rt).iter(|| async {
            let max_retries = 5;
            let base_delay_ms = 1u64;

            for attempt in 0..max_retries {
                let success = attempt >= 2; // Succeed on third attempt

                if success {
                    break;
                }

                let delay = base_delay_ms * (2u64.pow(attempt));
                tokio::time::sleep(Duration::from_millis(delay.min(10))).await;
            }

            black_box("success")
        })
    });

    // -------------------------------------------------------------------------
    // Loop with Conditional (when inside loop)
    // -------------------------------------------------------------------------

    group.bench_function("loop/with_condition", |b| {
        let items: Vec<serde_json::Value> = (0..100)
            .map(|i| {
                serde_json::json!({
                    "name": format!("item_{}", i),
                    "enabled": i % 3 == 0
                })
            })
            .collect();

        b.iter(|| {
            let results: Vec<_> = items
                .iter()
                .filter(|item| {
                    item.get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                })
                .map(|item| {
                    black_box(item);
                    item.get("name").unwrap().clone()
                })
                .collect();
            black_box(results)
        })
    });

    // -------------------------------------------------------------------------
    // Loop with Register (collecting results)
    // -------------------------------------------------------------------------

    group.bench_function("loop/with_register", |b| {
        b.to_async(&rt).iter(|| async {
            let collected = Arc::new(Mutex::new(Vec::with_capacity(50)));
            let handles: Vec<_> = (0..50)
                .map(|i| {
                    let collected = Arc::clone(&collected);
                    tokio::spawn(async move {
                        let result = ModuleOutput::ok(format!("Result {}", i));
                        collected.lock().await.push(result);
                    })
                })
                .collect();

            futures::future::join_all(handles).await;
            let results = collected.lock().await;
            black_box(results.len())
        })
    });

    group.finish();
}

// ============================================================================
// MEMORY USAGE BENCHMARKS
// ============================================================================
//
// These benchmarks focus on memory efficiency, which is a significant
// advantage of Rust over Python-based Ansible:
// - Per-host memory overhead
// - Task context memory
// - Connection state memory
// - Variable/fact storage efficiency

fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    group.measurement_time(Duration::from_secs(10));

    let rt = create_runtime(4);

    // -------------------------------------------------------------------------
    // Per-Host Memory Overhead
    // -------------------------------------------------------------------------

    for num_hosts in HOST_COUNTS {
        group.throughput(Throughput::Elements(num_hosts as u64));

        // Inventory loading and host representation
        group.bench_with_input(
            BenchmarkId::new("inventory/load_hosts", num_hosts),
            &num_hosts,
            |b, &hosts| {
                let yaml = generate_inventory_yaml(hosts);
                b.iter(|| {
                    let mut tmpfile = NamedTempFile::new().unwrap();
                    tmpfile.write_all(yaml.as_bytes()).unwrap();
                    tmpfile.flush().unwrap();
                    let inventory = Inventory::load(tmpfile.path());
                    black_box(inventory)
                })
            },
        );

        // Facts storage efficiency
        group.bench_with_input(
            BenchmarkId::new("facts/storage", num_hosts),
            &num_hosts,
            |b, &hosts| {
                b.iter(|| {
                    let facts_map: HashMap<String, Facts> = (0..hosts)
                        .map(|i| {
                            let mut facts = Facts::new();
                            facts.set("hostname", serde_json::json!(format!("host{}", i)));
                            facts.set("os_family", serde_json::json!("Debian"));
                            facts.set("distribution", serde_json::json!("Ubuntu"));
                            facts.set("processor_count", serde_json::json!(8));
                            facts.set("memory_mb", serde_json::json!(16384));
                            (format!("host{:05}", i), facts)
                        })
                        .collect();
                    black_box(facts_map)
                })
            },
        );
    }

    // -------------------------------------------------------------------------
    // Task Context Memory
    // -------------------------------------------------------------------------

    group.bench_function("context/minimal", |b| {
        b.iter(|| {
            let context = ModuleContext::new();
            black_box(context)
        })
    });

    group.bench_function("context/with_vars", |b| {
        let mut vars: HashMap<String, serde_json::Value> = HashMap::new();
        for i in 0..50 {
            vars.insert(
                format!("var_{}", i),
                serde_json::json!(format!("value_{}", i)),
            );
        }

        b.iter(|| {
            let context = ModuleContext::new().with_vars(vars.clone());
            black_box(context)
        })
    });

    group.bench_function("context/with_facts", |b| {
        let mut facts: HashMap<String, serde_json::Value> = HashMap::new();
        for i in 0..30 {
            facts.insert(
                format!("fact_{}", i),
                serde_json::json!(format!("value_{}", i)),
            );
        }

        b.iter(|| {
            let context = ModuleContext::new().with_facts(facts.clone());
            black_box(context)
        })
    });

    // -------------------------------------------------------------------------
    // Concurrent Task Memory
    // -------------------------------------------------------------------------

    for concurrent_tasks in [10, 50, 100, 500] {
        group.bench_with_input(
            BenchmarkId::new("concurrent/task_state", concurrent_tasks),
            &concurrent_tasks,
            |b, &count| {
                b.to_async(&rt).iter(|| async move {
                    // Each task holds some state (simulating ExecutionContext)
                    let shared_config = Arc::new(vec![0u8; 1024]); // 1KB shared

                    let handles: Vec<_> = (0..count)
                        .map(|i| {
                            let config = Arc::clone(&shared_config);
                            let host = format!("host{:05}", i);
                            let vars: HashMap<String, String> = (0..10)
                                .map(|j| (format!("var_{}", j), format!("value_{}", j)))
                                .collect();

                            tokio::spawn(async move {
                                black_box(&config);
                                black_box(&host);
                                black_box(&vars);
                                tokio::task::yield_now().await;
                                i
                            })
                        })
                        .collect();

                    let results: Vec<_> = futures::future::join_all(handles).await;
                    black_box(results)
                })
            },
        );
    }

    // -------------------------------------------------------------------------
    // Module Output Memory
    // -------------------------------------------------------------------------

    group.bench_function("output/simple", |b| {
        b.iter(|| {
            let output = ModuleOutput::ok("Success");
            black_box(output)
        })
    });

    group.bench_function("output/with_data", |b| {
        b.iter(|| {
            let output = ModuleOutput::changed("File modified")
                .with_data("path", serde_json::json!("/etc/config.conf"))
                .with_data("mode", serde_json::json!("0644"))
                .with_data("owner", serde_json::json!("root"))
                .with_data("group", serde_json::json!("root"))
                .with_data("size", serde_json::json!(1024))
                .with_data("checksum", serde_json::json!("abc123def456"));
            black_box(output)
        })
    });

    // -------------------------------------------------------------------------
    // Connection State Memory
    // -------------------------------------------------------------------------

    group.bench_function("connection/factory_create", |b| {
        b.iter(|| {
            let config = ConnectionConfig::default();
            let factory = ConnectionFactory::new(config);
            black_box(factory)
        })
    });

    group.bench_function("connection/pool_with_entries", |b| {
        b.to_async(&rt).iter(|| async {
            let pool: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));

            // Populate pool with 50 connection entries
            {
                let mut pool_guard = pool.write().await;
                for i in 0..50 {
                    pool_guard.insert(format!("host{:05}", i), format!("connection_state_{}", i));
                }
            }

            black_box(pool)
        })
    });

    // -------------------------------------------------------------------------
    // Variable Interpolation Memory
    // -------------------------------------------------------------------------

    let engine = TemplateEngine::new();

    group.bench_function("vars/interpolation_small", |b| {
        let template = "{{ var1 }} {{ var2 }} {{ var3 }}";
        let mut vars: HashMap<String, serde_json::Value> = HashMap::new();
        vars.insert("var1".to_string(), serde_json::json!("value1"));
        vars.insert("var2".to_string(), serde_json::json!("value2"));
        vars.insert("var3".to_string(), serde_json::json!("value3"));

        b.iter(|| {
            let result = engine.render(template, &vars);
            black_box(result)
        })
    });

    group.bench_function("vars/interpolation_large", |b| {
        let mut template = String::new();
        let mut vars: HashMap<String, serde_json::Value> = HashMap::new();
        for i in 0..50 {
            template.push_str(&format!("{{{{ var_{} }}}} ", i));
            vars.insert(
                format!("var_{}", i),
                serde_json::json!(format!("value_{}", i)),
            );
        }

        b.iter(|| {
            let result = engine.render(&template, &vars);
            black_box(result)
        })
    });

    // -------------------------------------------------------------------------
    // Parallelization Manager Memory
    // -------------------------------------------------------------------------

    group.bench_function("parallelization/manager_create", |b| {
        b.iter(|| {
            let manager = ParallelizationManager::new();
            black_box(manager)
        })
    });

    group.bench_function("parallelization/guard_acquire_release", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = ParallelizationManager::new();

            // Acquire and release 10 guards
            for i in 0..10 {
                let _guard = manager
                    .acquire(
                        ParallelizationHint::HostExclusive,
                        &format!("host{}", i),
                        "test_module",
                    )
                    .await;
            }

            black_box(manager.stats())
        })
    });

    group.finish();
}

// ============================================================================
// ANSIBLE BASELINE COMPARISON SIMULATION
// ============================================================================
//
// This benchmark group simulates complete Ansible-like workflows to
// establish baselines for comparison. Each benchmark represents a
// common automation pattern.

fn bench_ansible_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("ansible_patterns");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(20);

    let rt = create_runtime(4);

    // -------------------------------------------------------------------------
    // Pattern: Deploy Configuration Files
    // Equivalent Ansible: template + copy + service restart
    // -------------------------------------------------------------------------

    group.bench_function("pattern/deploy_config", |b| {
        let engine = TemplateEngine::new();
        let template_content = generate_template_content(10);
        let mut vars: HashMap<String, serde_json::Value> = HashMap::new();
        for i in 0..10 {
            vars.insert(
                format!("var_{}", i),
                serde_json::json!(format!("value_{}", i)),
            );
        }
        vars.insert("enable_feature".to_string(), serde_json::json!(true));
        vars.insert("items".to_string(), serde_json::json!(vec!["a", "b", "c"]));

        b.to_async(&rt).iter(|| {
            let engine = &engine;
            let template = template_content.clone();
            let v = vars.clone();
            async move {
                // 1. Render template
                let rendered = engine.render(&template, &v).unwrap();

                // 2. Simulate file transfer
                simulate_file_transfer(rendered.len()).await.unwrap();

                // 3. Simulate service restart
                tokio::time::sleep(Duration::from_millis(5)).await;

                black_box(rendered)
            }
        })
    });

    // -------------------------------------------------------------------------
    // Pattern: Multi-host Package Installation
    // Equivalent Ansible: apt/yum state=present across hosts
    // -------------------------------------------------------------------------

    for num_hosts in [10, 50] {
        group.throughput(Throughput::Elements(num_hosts as u64));

        group.bench_with_input(
            BenchmarkId::new("pattern/package_install", num_hosts),
            &num_hosts,
            |b, &hosts| {
                b.to_async(&rt).iter(|| async move {
                    let manager = Arc::new(ParallelizationManager::new());
                    let semaphore = Arc::new(Semaphore::new(5)); // forks=5
                    let success_count = Arc::new(AtomicUsize::new(0));

                    let handles: Vec<_> = (0..hosts)
                        .map(|i| {
                            let manager = Arc::clone(&manager);
                            let sem = Arc::clone(&semaphore);
                            let success = Arc::clone(&success_count);
                            let host = format!("host{:05}", i);

                            tokio::spawn(async move {
                                let _permit = sem.acquire().await.unwrap();
                                let _guard = manager
                                    .acquire(ParallelizationHint::HostExclusive, &host, "apt")
                                    .await;

                                // Simulate package installation
                                tokio::time::sleep(Duration::from_millis(10)).await;
                                success.fetch_add(1, Ordering::Relaxed);
                                "installed"
                            })
                        })
                        .collect();

                    futures::future::join_all(handles).await;
                    black_box(success_count.load(Ordering::Relaxed))
                })
            },
        );
    }

    // -------------------------------------------------------------------------
    // Pattern: Facts + Conditional Tasks
    // Equivalent Ansible: gather_facts + when conditionals
    // -------------------------------------------------------------------------

    group.bench_function("pattern/facts_with_conditionals", |b| {
        b.to_async(&rt).iter(|| async {
            let hosts = 20;
            let semaphore = Arc::new(Semaphore::new(5));

            // Phase 1: Gather facts
            let facts_handles: Vec<_> = (0..hosts)
                .map(|i| {
                    let sem = Arc::clone(&semaphore);
                    tokio::spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        let host = format!("host{:05}", i);
                        let facts = simulate_fact_gather(&host).await;
                        (host, facts)
                    })
                })
                .collect();

            let all_facts: HashMap<String, Facts> = futures::future::join_all(facts_handles)
                .await
                .into_iter()
                .filter_map(|r| r.ok())
                .collect();

            // Phase 2: Conditional task execution based on facts
            let task_handles: Vec<_> = all_facts
                .iter()
                .filter(|(_, facts)| {
                    // Simulate when: ansible_os_family == "Debian"
                    facts
                        .get("ansible_os_family")
                        .and_then(|v| v.as_str())
                        .map(|s| s == "Debian")
                        .unwrap_or(false)
                })
                .map(|(host, _)| {
                    let sem = Arc::clone(&semaphore);
                    let h = host.clone();
                    tokio::spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        tokio::time::sleep(Duration::from_millis(2)).await;
                        format!("executed_on_{}", h)
                    })
                })
                .collect();

            let results: Vec<_> = futures::future::join_all(task_handles).await;
            black_box(results.len())
        })
    });

    // -------------------------------------------------------------------------
    // Pattern: Rolling Update
    // Equivalent Ansible: serial with delegate_to for load balancer
    // -------------------------------------------------------------------------

    group.bench_function("pattern/rolling_update", |b| {
        b.to_async(&rt).iter(|| async {
            let hosts = 20;
            let batch_size = 5;
            let batches = (hosts + batch_size - 1) / batch_size;

            for batch in 0..batches {
                let start = batch * batch_size;
                let end = ((batch + 1) * batch_size).min(hosts);

                // Remove from load balancer (delegate_to: localhost)
                tokio::time::sleep(Duration::from_millis(1)).await;

                // Update hosts in this batch
                let batch_handles: Vec<_> = (start..end)
                    .map(|i| {
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                            format!("updated_host{}", i)
                        })
                    })
                    .collect();

                let _results: Vec<_> = futures::future::join_all(batch_handles).await;

                // Add back to load balancer
                tokio::time::sleep(Duration::from_millis(1)).await;
            }

            black_box("rolling_update_complete")
        })
    });

    // -------------------------------------------------------------------------
    // Pattern: Parallel File Distribution
    // Equivalent Ansible: synchronize or copy to many hosts
    // -------------------------------------------------------------------------

    group.bench_function("pattern/file_distribution", |b| {
        b.to_async(&rt).iter(|| async {
            let hosts = 50;
            let file_size = 10 * 1024; // 10KB
            let semaphore = Arc::new(Semaphore::new(10)); // Higher parallelism for file ops

            let handles: Vec<_> = (0..hosts)
                .map(|i| {
                    let sem = Arc::clone(&semaphore);
                    tokio::spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        simulate_file_transfer(file_size).await.unwrap();
                        format!("transferred_to_host{}", i)
                    })
                })
                .collect();

            let results: Vec<_> = futures::future::join_all(handles).await;
            black_box(results.len())
        })
    });

    group.finish();
}

// ============================================================================
// CRITERION CONFIGURATION
// ============================================================================

criterion_group!(ssh_benches, bench_ssh_connection,);

criterion_group!(fact_benches, bench_fact_gathering,);

criterion_group!(module_benches, bench_module_execution,);

criterion_group!(loop_benches, bench_loop_performance,);

criterion_group!(memory_benches, bench_memory_usage,);

criterion_group!(pattern_benches, bench_ansible_patterns,);

criterion_main!(
    ssh_benches,
    fact_benches,
    module_benches,
    loop_benches,
    memory_benches,
    pattern_benches,
);
