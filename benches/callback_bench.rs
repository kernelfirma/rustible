//! Comprehensive Callback System Benchmarks
//!
//! This benchmark suite measures performance characteristics of the Rustible
//! callback system, including:
//!
//! 1. **Callback Dispatch Overhead** - Measuring the cost of dispatching events
//! 2. **Plugin Creation Time** - How long it takes to create callback plugins
//! 3. **Event Serialization** - ExecutionResult serialization costs
//! 4. **Multiple Plugin Overhead** - Scaling behavior with multiple callbacks
//! 5. **Memory Usage** - Allocation patterns during callback operations
//!
//! # Running the Benchmarks
//!
//! ```bash
//! cargo bench --bench callback_bench
//! ```
//!
//! # Benchmark Groups
//!
//! - `callback_dispatch` - Event dispatch performance
//! - `plugin_lifecycle` - Plugin creation/registration
//! - `event_serialization` - Serialization of execution results
//! - `multi_plugin_scaling` - Performance with multiple plugins
//! - `memory_patterns` - Memory allocation benchmarks
//! - `concurrent_dispatch` - Thread-safe dispatch under contention

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

use async_trait::async_trait;
use parking_lot::RwLock;

// NOTE: The callback module imports are commented out as the library has
// compilation issues in other modules. Once fixed, uncomment and use:
// use rustible::callback::prelude::*;
// use rustible::callback::{ProgressCallback, SummaryCallback, NullCallback};

use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Test Plugin Implementations
// ============================================================================

/// A minimal no-op callback plugin for measuring dispatch overhead
#[derive(Debug, Default)]
struct NoOpCallback;

#[async_trait]
impl ExecutionCallback for NoOpCallback {
    async fn on_playbook_start(&self, _name: &str) {}
    async fn on_playbook_end(&self, _name: &str, _success: bool) {}
    async fn on_play_start(&self, _name: &str, _hosts: &[String]) {}
    async fn on_play_end(&self, _name: &str, _success: bool) {}
    async fn on_task_start(&self, _name: &str, _host: &str) {}
    async fn on_task_complete(&self, _result: &ExecutionResult) {}
    async fn on_handler_triggered(&self, _name: &str) {}
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {}
}

/// A callback that tracks invocation counts (simulates real-world state tracking)
#[derive(Debug, Default)]
struct CountingCallback {
    playbook_starts: AtomicU32,
    playbook_ends: AtomicU32,
    play_starts: AtomicU32,
    play_ends: AtomicU32,
    task_starts: AtomicU32,
    task_completes: AtomicU32,
    handlers_triggered: AtomicU32,
    facts_gathered: AtomicU32,
}

impl CountingCallback {
    fn new() -> Self {
        Self::default()
    }

    fn total_events(&self) -> u32 {
        self.playbook_starts.load(Ordering::Relaxed)
            + self.playbook_ends.load(Ordering::Relaxed)
            + self.play_starts.load(Ordering::Relaxed)
            + self.play_ends.load(Ordering::Relaxed)
            + self.task_starts.load(Ordering::Relaxed)
            + self.task_completes.load(Ordering::Relaxed)
            + self.handlers_triggered.load(Ordering::Relaxed)
            + self.facts_gathered.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl ExecutionCallback for CountingCallback {
    async fn on_playbook_start(&self, _name: &str) {
        self.playbook_starts.fetch_add(1, Ordering::Relaxed);
    }

    async fn on_playbook_end(&self, _name: &str, _success: bool) {
        self.playbook_ends.fetch_add(1, Ordering::Relaxed);
    }

    async fn on_play_start(&self, _name: &str, _hosts: &[String]) {
        self.play_starts.fetch_add(1, Ordering::Relaxed);
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {
        self.play_ends.fetch_add(1, Ordering::Relaxed);
    }

    async fn on_task_start(&self, _name: &str, _host: &str) {
        self.task_starts.fetch_add(1, Ordering::Relaxed);
    }

    async fn on_task_complete(&self, _result: &ExecutionResult) {
        self.task_completes.fetch_add(1, Ordering::Relaxed);
    }

    async fn on_handler_triggered(&self, _name: &str) {
        self.handlers_triggered.fetch_add(1, Ordering::Relaxed);
    }

    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        self.facts_gathered.fetch_add(1, Ordering::Relaxed);
    }
}

/// A callback that simulates heavyweight operations (stats aggregation)
#[derive(Debug)]
struct HeavyweightCallback {
    stats: RwLock<HashMap<String, Vec<Duration>>>,
}

impl HeavyweightCallback {
    fn new() -> Self {
        Self {
            stats: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for HeavyweightCallback {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionCallback for HeavyweightCallback {
    async fn on_task_complete(&self, result: &ExecutionResult) {
        // Simulate aggregation work
        let mut stats = self.stats.write();
        let entry = stats.entry(result.task_name.clone()).or_default();
        entry.push(result.duration);
    }
}

/// Composite callback that dispatches to multiple callbacks
struct CompositeCallback {
    callbacks: Vec<Arc<dyn ExecutionCallback>>,
}

impl CompositeCallback {
    fn new(callbacks: Vec<Arc<dyn ExecutionCallback>>) -> Self {
        Self { callbacks }
    }
}

#[async_trait]
impl ExecutionCallback for CompositeCallback {
    async fn on_playbook_start(&self, name: &str) {
        for cb in &self.callbacks {
            cb.on_playbook_start(name).await;
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        for cb in &self.callbacks {
            cb.on_playbook_end(name, success).await;
        }
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        for cb in &self.callbacks {
            cb.on_play_start(name, hosts).await;
        }
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        for cb in &self.callbacks {
            cb.on_play_end(name, success).await;
        }
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        for cb in &self.callbacks {
            cb.on_task_start(name, host).await;
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        for cb in &self.callbacks {
            cb.on_task_complete(result).await;
        }
    }

    async fn on_handler_triggered(&self, name: &str) {
        for cb in &self.callbacks {
            cb.on_handler_triggered(name).await;
        }
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        for cb in &self.callbacks {
            cb.on_facts_gathered(host, facts).await;
        }
    }
}

// ============================================================================
// Test Data Generators
// ============================================================================

/// Generate a simple ExecutionResult for benchmarking
fn create_execution_result(host: &str, task_name: &str) -> ExecutionResult {
    ExecutionResult {
        host: host.to_string(),
        task_name: task_name.to_string(),
        result: ModuleResult::ok("Success"),
        duration: Duration::from_millis(100),
        notify: vec![],
    }
}

/// Generate an ExecutionResult with data payload
fn create_execution_result_with_data(
    host: &str,
    task_name: &str,
    data_size: usize,
) -> ExecutionResult {
    let data = serde_json::json!({
        "stdout": "x".repeat(data_size),
        "stderr": "",
        "rc": 0,
        "changed": true,
        "diff": {
            "before": "old content",
            "after": "new content",
        }
    });

    ExecutionResult {
        host: host.to_string(),
        task_name: task_name.to_string(),
        result: ModuleResult::changed("Changed successfully").with_data(data),
        duration: Duration::from_millis(150),
        notify: vec!["restart nginx".to_string(), "reload config".to_string()],
    }
}

/// Generate a list of host names
fn generate_hosts(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("host{:04}", i)).collect()
}

// ============================================================================
// 1. Callback Dispatch Overhead Benchmarks
// ============================================================================

fn bench_dispatch_overhead(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("callback_dispatch");

    // NoOp callback dispatch
    group.bench_function("noop_playbook_start", |b| {
        let callback = NoOpCallback;
        b.to_async(&rt)
            .iter(|| async { callback.on_playbook_start(black_box("test_playbook")).await })
    });

    // Counting callback dispatch
    group.bench_function("counting_playbook_start", |b| {
        let callback = CountingCallback::new();
        b.to_async(&rt)
            .iter(|| async { callback.on_playbook_start(black_box("test_playbook")).await })
    });

    // on_task_complete with ExecutionResult (minimal)
    group.bench_function("noop_task_complete", |b| {
        let callback = NoOpCallback;
        let result = create_execution_result("localhost", "test_task");
        b.to_async(&rt)
            .iter(|| async { callback.on_task_complete(black_box(&result)).await })
    });

    // on_task_complete with counting (atomic operations)
    group.bench_function("counting_task_complete", |b| {
        let callback = CountingCallback::new();
        let result = create_execution_result("localhost", "test_task");
        b.to_async(&rt)
            .iter(|| async { callback.on_task_complete(black_box(&result)).await })
    });

    // on_task_complete with heavyweight operations
    group.bench_function("heavyweight_task_complete", |b| {
        let callback = HeavyweightCallback::new();
        let result = create_execution_result("localhost", "test_task");
        b.to_async(&rt)
            .iter(|| async { callback.on_task_complete(black_box(&result)).await })
    });

    // on_task_complete with large data payload
    group.bench_function("task_complete_large_data", |b| {
        let callback = CountingCallback::new();
        let result = create_execution_result_with_data("localhost", "test_task", 10_000);
        b.to_async(&rt)
            .iter(|| async { callback.on_task_complete(black_box(&result)).await })
    });

    // on_play_start with multiple hosts
    for host_count in [1, 10, 100, 500].iter() {
        let hosts = generate_hosts(*host_count);
        group.throughput(Throughput::Elements(*host_count as u64));
        group.bench_with_input(
            BenchmarkId::new("play_start_hosts", host_count),
            &hosts,
            |b, hosts| {
                let callback = NoOpCallback;
                b.to_async(&rt).iter(|| async {
                    callback
                        .on_play_start(black_box("test_play"), black_box(hosts))
                        .await
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// 2. Plugin Creation Time Benchmarks
// ============================================================================

fn bench_plugin_lifecycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("plugin_lifecycle");

    // NoOp callback creation
    group.bench_function("noop_creation", |b| {
        b.iter(|| {
            let callback = NoOpCallback;
            black_box(callback)
        })
    });

    // Counting callback creation
    group.bench_function("counting_creation", |b| {
        b.iter(|| {
            let callback = CountingCallback::new();
            black_box(callback)
        })
    });

    // Heavyweight callback creation
    group.bench_function("heavyweight_creation", |b| {
        b.iter(|| {
            let callback = HeavyweightCallback::new();
            black_box(callback)
        })
    });

    // Arc wrapping (for shared callbacks)
    group.bench_function("arc_wrapping", |b| {
        b.iter(|| {
            let callback = Arc::new(CountingCallback::new());
            black_box(callback)
        })
    });

    // CompositeCallback creation with 5 callbacks
    group.bench_function("composite_5_callbacks", |b| {
        b.iter(|| {
            let callbacks: Vec<Arc<dyn ExecutionCallback>> = vec![
                Arc::new(NoOpCallback),
                Arc::new(CountingCallback::new()),
                Arc::new(NoOpCallback),
                Arc::new(CountingCallback::new()),
                Arc::new(NoOpCallback),
            ];
            let composite = CompositeCallback::new(callbacks);
            black_box(composite)
        })
    });

    group.finish();
}

// ============================================================================
// 3. Event Serialization Benchmarks
// ============================================================================

fn bench_event_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_serialization");

    // ExecutionResult creation (minimal)
    group.bench_function("result_creation_minimal", |b| {
        b.iter(|| {
            let result = create_execution_result(black_box("localhost"), black_box("test_task"));
            black_box(result)
        })
    });

    // ExecutionResult creation (with data)
    for data_size in [100, 1_000, 10_000, 100_000].iter() {
        group.throughput(Throughput::Bytes(*data_size as u64));
        group.bench_with_input(
            BenchmarkId::new("result_creation_with_data", data_size),
            data_size,
            |b, &size| {
                b.iter(|| {
                    let result = create_execution_result_with_data(
                        black_box("localhost"),
                        black_box("test_task"),
                        size,
                    );
                    black_box(result)
                })
            },
        );
    }

    // ExecutionResult cloning (important for dispatch)
    group.bench_function("result_clone_minimal", |b| {
        let result = create_execution_result("localhost", "test_task");
        b.iter(|| {
            let cloned = black_box(&result).clone();
            black_box(cloned)
        })
    });

    // ExecutionResult cloning with large data
    for data_size in [100, 1_000, 10_000].iter() {
        let result = create_execution_result_with_data("localhost", "test_task", *data_size);
        group.throughput(Throughput::Bytes(*data_size as u64));
        group.bench_with_input(
            BenchmarkId::new("result_clone_with_data", data_size),
            &result,
            |b, result| {
                b.iter(|| {
                    let cloned = black_box(result).clone();
                    black_box(cloned)
                })
            },
        );
    }

    // ModuleResult JSON serialization
    group.bench_function("module_result_to_json", |b| {
        let result = ModuleResult::changed("Test message")
            .with_data(serde_json::json!({"key": "value", "count": 42}));
        b.iter(|| {
            let json = serde_json::to_string(black_box(&result)).unwrap();
            black_box(json)
        })
    });

    // Full ExecutionResult JSON serialization
    group.bench_function("execution_result_to_json", |b| {
        let result = create_execution_result_with_data("localhost", "test_task", 1000);
        b.iter(|| {
            let json = serde_json::to_string(black_box(&result.result)).unwrap();
            black_box(json)
        })
    });

    group.finish();
}

// ============================================================================
// 4. Multiple Plugin Overhead Benchmarks
// ============================================================================

fn bench_multi_plugin_scaling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("multi_plugin_scaling");

    // Test scaling with increasing number of callbacks
    for callback_count in [1, 2, 5, 10, 20, 50].iter() {
        group.throughput(Throughput::Elements(*callback_count as u64));

        // Create composite callback
        group.bench_with_input(
            BenchmarkId::new("create_composite", callback_count),
            callback_count,
            |b, &count| {
                b.iter(|| {
                    let callbacks: Vec<Arc<dyn ExecutionCallback>> = (0..count)
                        .map(|_| Arc::new(NoOpCallback) as Arc<dyn ExecutionCallback>)
                        .collect();
                    let composite = CompositeCallback::new(callbacks);
                    black_box(composite)
                })
            },
        );

        // Dispatch playbook_start to multiple callbacks
        group.bench_with_input(
            BenchmarkId::new("dispatch_playbook_start", callback_count),
            callback_count,
            |b, &count| {
                let callbacks: Vec<Arc<dyn ExecutionCallback>> = (0..count)
                    .map(|_| Arc::new(NoOpCallback) as Arc<dyn ExecutionCallback>)
                    .collect();
                let composite = CompositeCallback::new(callbacks);

                b.to_async(&rt)
                    .iter(|| async { composite.on_playbook_start(black_box("test")).await })
            },
        );

        // Dispatch task_complete to multiple callbacks
        group.bench_with_input(
            BenchmarkId::new("dispatch_task_complete", callback_count),
            callback_count,
            |b, &count| {
                let callbacks: Vec<Arc<dyn ExecutionCallback>> = (0..count)
                    .map(|_| Arc::new(CountingCallback::new()) as Arc<dyn ExecutionCallback>)
                    .collect();
                let composite = CompositeCallback::new(callbacks);
                let result = create_execution_result("localhost", "test_task");

                b.to_async(&rt)
                    .iter(|| async { composite.on_task_complete(black_box(&result)).await })
            },
        );
    }

    // Heavyweight callbacks scaling
    group.bench_function("heavyweight_callbacks_5", |b| {
        let callbacks: Vec<Arc<dyn ExecutionCallback>> = (0..5)
            .map(|_| Arc::new(HeavyweightCallback::new()) as Arc<dyn ExecutionCallback>)
            .collect();
        let composite = CompositeCallback::new(callbacks);
        let result = create_execution_result("localhost", "test_task");

        b.to_async(&rt)
            .iter(|| async { composite.on_task_complete(black_box(&result)).await })
    });

    group.finish();
}

// ============================================================================
// 5. Memory Usage Pattern Benchmarks
// ============================================================================

fn bench_memory_patterns(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("memory_patterns");

    // Callback Arc allocation
    group.bench_function("callback_arc_alloc", |b| {
        b.iter(|| {
            let callback = Arc::new(CountingCallback::new());
            black_box(callback)
        })
    });

    // ExecutionResult vector allocation (simulates batched results)
    for batch_size in [10, 50, 100, 500].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("result_batch_alloc", batch_size),
            batch_size,
            |b, &size| {
                b.iter(|| {
                    let results: Vec<ExecutionResult> = (0..size)
                        .map(|i| {
                            create_execution_result(&format!("host{}", i), &format!("task{}", i))
                        })
                        .collect();
                    black_box(results)
                })
            },
        );
    }

    // Host vector allocation (for on_play_start)
    for host_count in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*host_count as u64));
        group.bench_with_input(
            BenchmarkId::new("host_vec_alloc", host_count),
            host_count,
            |b, &count| {
                b.iter(|| {
                    let hosts = generate_hosts(count);
                    black_box(hosts)
                })
            },
        );
    }

    // Full callback lifecycle simulation (using CountingCallback)
    group.bench_function("counting_callback_lifecycle", |b| {
        b.to_async(&rt).iter(|| async {
            let callback = CountingCallback::new();
            callback.on_playbook_start("test").await;
            callback
                .on_play_start("play", &["host1".to_string(), "host2".to_string()])
                .await;
            callback.on_task_start("task", "host1").await;
            let result = create_execution_result("host1", "task");
            callback.on_task_complete(&result).await;
            callback.on_play_end("play", true).await;
            callback.on_playbook_end("test", true).await;
            black_box(callback)
        })
    });

    group.finish();
}

// ============================================================================
// 6. Concurrent Dispatch Benchmarks
// ============================================================================

fn bench_concurrent_dispatch(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_dispatch");
    group.sample_size(20);

    // Concurrent dispatch to shared callback
    for concurrency in [2, 4, 8, 16].iter() {
        group.throughput(Throughput::Elements(*concurrency as u64));
        group.bench_with_input(
            BenchmarkId::new("concurrent_playbook_start", concurrency),
            concurrency,
            |b, &num_tasks| {
                let callback = Arc::new(CountingCallback::new());

                b.to_async(&rt).iter(|| {
                    let callback = Arc::clone(&callback);
                    async move {
                        let mut handles = Vec::with_capacity(num_tasks);
                        for i in 0..num_tasks {
                            let cb = Arc::clone(&callback);
                            let name = format!("playbook_{}", i);
                            handles.push(tokio::spawn(
                                async move { cb.on_playbook_start(&name).await },
                            ));
                        }
                        for handle in handles {
                            let _: () = handle.await.unwrap();
                            black_box(());
                        }
                    }
                })
            },
        );
    }

    // Concurrent task completions (more realistic workload)
    for concurrency in [4, 8, 16, 32].iter() {
        group.throughput(Throughput::Elements(*concurrency as u64));
        group.bench_with_input(
            BenchmarkId::new("concurrent_task_complete", concurrency),
            concurrency,
            |b, &num_tasks| {
                let callback = Arc::new(CountingCallback::new());

                b.to_async(&rt).iter(|| {
                    let callback = Arc::clone(&callback);
                    async move {
                        let mut handles = Vec::with_capacity(num_tasks);
                        for i in 0..num_tasks {
                            let cb = Arc::clone(&callback);
                            let result = create_execution_result(
                                &format!("host{}", i),
                                &format!("task{}", i),
                            );
                            handles.push(tokio::spawn(async move {
                                cb.on_task_complete(&result).await
                            }));
                        }
                        for handle in handles {
                            let _: () = handle.await.unwrap();
                            black_box(());
                        }
                    }
                })
            },
        );
    }

    // Concurrent heavyweight callbacks
    group.bench_function("concurrent_heavyweight", |b| {
        let callback = Arc::new(HeavyweightCallback::new());

        b.to_async(&rt).iter(|| {
            let callback = Arc::clone(&callback);
            async move {
                let mut handles = Vec::with_capacity(16);
                for i in 0..16 {
                    let cb = Arc::clone(&callback);
                    let result = create_execution_result(&format!("host{}", i), "task");
                    handles.push(tokio::spawn(
                        async move { cb.on_task_complete(&result).await },
                    ));
                }
                for handle in handles {
                    handle.await.unwrap();
                }
            }
        })
    });

    group.finish();
}

// ============================================================================
// 7. Full Playbook Simulation Benchmark
// ============================================================================

fn bench_playbook_simulation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("playbook_simulation");
    group.sample_size(20);

    // Simulate a small playbook execution with NoOp callback
    group.bench_function("small_playbook_noop_5h_10t", |b| {
        let callback = NoOpCallback;
        let hosts = generate_hosts(5);
        let num_tasks = 10;

        b.to_async(&rt).iter(|| {
            let callback = &callback;
            let hosts = hosts.clone();
            async move {
                callback.on_playbook_start("test_playbook").await;
                callback.on_play_start("test_play", &hosts).await;

                for task_idx in 0..num_tasks {
                    for host in &hosts {
                        let task_name = format!("task_{}", task_idx);
                        callback.on_task_start(&task_name, host).await;
                        let result = create_execution_result(host, &task_name);
                        callback.on_task_complete(&result).await;
                    }
                }

                callback.on_play_end("test_play", true).await;
                callback.on_playbook_end("test_playbook", true).await;
            }
        })
    });

    // Simulate a small playbook execution with Counting callback
    group.bench_function("small_playbook_counting_5h_10t", |b| {
        let callback = CountingCallback::new();
        let hosts = generate_hosts(5);
        let num_tasks = 10;

        b.to_async(&rt).iter(|| {
            let callback = &callback;
            let hosts = hosts.clone();
            async move {
                callback.on_playbook_start("test_playbook").await;
                callback.on_play_start("test_play", &hosts).await;

                for task_idx in 0..num_tasks {
                    for host in &hosts {
                        let task_name = format!("task_{}", task_idx);
                        callback.on_task_start(&task_name, host).await;
                        let result = create_execution_result(host, &task_name);
                        callback.on_task_complete(&result).await;
                    }
                }

                callback.on_play_end("test_play", true).await;
                callback.on_playbook_end("test_playbook", true).await;
            }
        })
    });

    // Simulate a larger playbook with composite callbacks
    group.bench_function("medium_playbook_composite_20h_25t", |b| {
        let callbacks: Vec<Arc<dyn ExecutionCallback>> = vec![
            Arc::new(CountingCallback::new()),
            Arc::new(NoOpCallback),
            Arc::new(NoOpCallback),
        ];
        let composite = CompositeCallback::new(callbacks);
        let hosts = generate_hosts(20);
        let num_tasks = 25;

        b.to_async(&rt).iter(|| {
            let composite = &composite;
            let hosts = hosts.clone();
            async move {
                composite.on_playbook_start("test_playbook").await;
                composite.on_play_start("test_play", &hosts).await;

                for task_idx in 0..num_tasks {
                    for host in &hosts {
                        let task_name = format!("task_{}", task_idx);
                        composite.on_task_start(&task_name, host).await;
                        let result = create_execution_result(host, &task_name);
                        composite.on_task_complete(&result).await;
                    }
                }

                composite.on_play_end("test_play", true).await;
                composite.on_playbook_end("test_playbook", true).await;
            }
        })
    });

    group.finish();
}

// ============================================================================
// 8. Large Scale Simulation Benchmarks
// ============================================================================

fn bench_large_scale(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("large_scale");
    group.sample_size(10);

    // Large playbook simulation (50 hosts, 50 tasks = 2500 task executions)
    group.bench_function("large_playbook_50h_50t", |b| {
        let hosts = generate_hosts(50);
        let num_tasks = 50;
        let callback = CountingCallback::new();

        b.to_async(&rt).iter(|| {
            let callback = &callback;
            let hosts = hosts.clone();
            async move {
                callback.on_playbook_start("large_playbook").await;
                callback.on_play_start("large_play", &hosts).await;

                for task_idx in 0..num_tasks {
                    for host in &hosts {
                        let task_name = format!("task_{}", task_idx);
                        callback.on_task_start(&task_name, host).await;
                        let result = create_execution_result(host, &task_name);
                        callback.on_task_complete(&result).await;
                    }
                }

                callback.on_play_end("large_play", true).await;
                callback.on_playbook_end("large_playbook", true).await;
            }
        })
    });

    // Multi-play simulation (3 plays, 20 hosts each, 10 tasks)
    group.bench_function("multi_play_3p_20h_10t", |b| {
        let callback = CountingCallback::new();

        b.to_async(&rt).iter(|| async {
            callback.on_playbook_start("multi_play_book").await;

            for play_idx in 0..3 {
                let hosts = generate_hosts(20);
                let play_name = format!("play_{}", play_idx);
                callback.on_play_start(&play_name, &hosts).await;

                for task_idx in 0..10 {
                    for host in &hosts {
                        let task_name = format!("task_{}_{}", play_idx, task_idx);
                        callback.on_task_start(&task_name, host).await;
                        let result = create_execution_result(host, &task_name);
                        callback.on_task_complete(&result).await;
                    }
                }

                callback.on_play_end(&play_name, true).await;
            }

            callback.on_playbook_end("multi_play_book", true).await;
        })
    });

    // Heavyweight callback under large load
    group.bench_function("heavyweight_50h_20t", |b| {
        let hosts = generate_hosts(50);
        let num_tasks = 20;
        let callback = HeavyweightCallback::new();

        b.to_async(&rt).iter(|| {
            let callback = &callback;
            let hosts = hosts.clone();
            async move {
                callback.on_playbook_start("heavyweight_book").await;
                callback.on_play_start("heavyweight_play", &hosts).await;

                for task_idx in 0..num_tasks {
                    for host in &hosts {
                        let task_name = format!("task_{}", task_idx);
                        callback.on_task_start(&task_name, host).await;
                        let result = create_execution_result(host, &task_name);
                        callback.on_task_complete(&result).await;
                    }
                }

                callback.on_play_end("heavyweight_play", true).await;
                callback.on_playbook_end("heavyweight_book", true).await;
            }
        })
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration and Main
// ============================================================================

fn criterion_config() -> Criterion {
    Criterion::default()
        .significance_level(0.05)
        .sample_size(100)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5))
        .with_output_color(true)
}

criterion_group! {
    name = dispatch_benches;
    config = criterion_config();
    targets = bench_dispatch_overhead
}

criterion_group! {
    name = lifecycle_benches;
    config = criterion_config();
    targets = bench_plugin_lifecycle
}

criterion_group! {
    name = serialization_benches;
    config = criterion_config();
    targets = bench_event_serialization
}

criterion_group! {
    name = scaling_benches;
    config = criterion_config();
    targets = bench_multi_plugin_scaling
}

criterion_group! {
    name = memory_benches;
    config = criterion_config();
    targets = bench_memory_patterns
}

criterion_group! {
    name = concurrent_benches;
    config = criterion_config();
    targets = bench_concurrent_dispatch
}

criterion_group! {
    name = simulation_benches;
    config = criterion_config();
    targets = bench_playbook_simulation
}

criterion_group! {
    name = large_scale_benches;
    config = criterion_config();
    targets = bench_large_scale
}

criterion_main!(
    dispatch_benches,
    lifecycle_benches,
    serialization_benches,
    scaling_benches,
    memory_benches,
    concurrent_benches,
    simulation_benches,
    large_scale_benches
);

// ============================================================================
// Unit Tests for Benchmark Helper Functions
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_execution_result() {
        let result = create_execution_result("host1", "task1");
        assert_eq!(result.host, "host1");
        assert_eq!(result.task_name, "task1");
        assert!(result.result.success);
    }

    #[test]
    fn test_create_execution_result_with_data() {
        let result = create_execution_result_with_data("host1", "task1", 1000);
        assert_eq!(result.host, "host1");
        assert!(result.result.data.is_some());
        assert!(result.result.changed);
    }

    #[test]
    fn test_generate_hosts() {
        let hosts = generate_hosts(5);
        assert_eq!(hosts.len(), 5);
        assert_eq!(hosts[0], "host0000");
        assert_eq!(hosts[4], "host0004");
    }

    #[tokio::test]
    async fn test_noop_callback() {
        let callback = NoOpCallback;
        // Should not panic
        callback.on_playbook_start("test").await;
        callback
            .on_task_complete(&create_execution_result("h", "t"))
            .await;
    }

    #[tokio::test]
    async fn test_counting_callback() {
        let callback = CountingCallback::new();

        callback.on_playbook_start("test").await;
        callback.on_playbook_start("test").await;
        callback.on_task_start("task", "host").await;

        assert_eq!(callback.playbook_starts.load(Ordering::Relaxed), 2);
        assert_eq!(callback.task_starts.load(Ordering::Relaxed), 1);
        assert_eq!(callback.total_events(), 3);
    }

    #[tokio::test]
    async fn test_heavyweight_callback() {
        let callback = HeavyweightCallback::new();
        let result = create_execution_result("host1", "task1");

        callback.on_task_complete(&result).await;
        callback.on_task_complete(&result).await;

        let stats = callback.stats.read();
        assert!(stats.contains_key("task1"));
        assert_eq!(stats.get("task1").unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_composite_callback() {
        let counting = Arc::new(CountingCallback::new());
        let callbacks: Vec<Arc<dyn ExecutionCallback>> = vec![
            Arc::clone(&counting) as Arc<dyn ExecutionCallback>,
            Arc::new(NoOpCallback),
        ];
        let composite = CompositeCallback::new(callbacks);

        composite.on_playbook_start("test").await;
        assert_eq!(counting.playbook_starts.load(Ordering::Relaxed), 1);
    }
}
