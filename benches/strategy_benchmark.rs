//! Strategy Execution Benchmarks for Rustible
//!
//! This benchmark suite measures the performance characteristics of different
//! execution strategies (Linear, Free, HostPinned) to identify bottlenecks
//! and optimize execution patterns.
//!
//! ## Benchmarks Included:
//!
//! 1. **Linear vs Free Strategy Comparison**:
//!    - Serial execution overhead
//!    - Task synchronization cost
//!    - Host coordination patterns
//!
//! 2. **Batch Sizing Impact**:
//!    - Different batch sizes with serial execution
//!    - Optimal batch size detection
//!    - Memory pressure at different batch sizes
//!
//! 3. **Host Failure Handling Cost**:
//!    - Failure detection overhead
//!    - Skip propagation cost
//!    - Recovery patterns
//!
//! 4. **Adaptive Strategy Selection**:
//!    - Strategy selection heuristics
//!    - Workload characterization
//!    - Dynamic batch sizing
//!
//! 5. **Parallelization Manager Overhead**:
//!    - Lock acquisition costs
//!    - Semaphore contention
//!    - Rate limiting overhead

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;

use rustible::executor::{ExecutionStats, ExecutionStrategy, ExecutorConfig};
use rustible::playbook::SerialSpec;

// ============================================================================
// Mock Types for Benchmarking (Self-contained)
// ============================================================================

/// Simulated task execution result
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MockTaskResult {
    status: MockTaskStatus,
    changed: bool,
    duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum MockTaskStatus {
    Ok,
    Changed,
    Failed,
    Skipped,
}

impl MockTaskResult {
    fn ok() -> Self {
        Self {
            status: MockTaskStatus::Ok,
            changed: false,
            duration: Duration::from_micros(100),
        }
    }

    #[allow(dead_code)]
    fn changed() -> Self {
        Self {
            status: MockTaskStatus::Changed,
            changed: true,
            duration: Duration::from_micros(150),
        }
    }

    fn failed() -> Self {
        Self {
            status: MockTaskStatus::Failed,
            changed: false,
            duration: Duration::from_micros(50),
        }
    }
}

/// Simulated host state for execution tracking
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct HostState {
    name: String,
    failed: bool,
    unreachable: bool,
    stats: ExecutionStats,
}

impl HostState {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            failed: false,
            unreachable: false,
            stats: ExecutionStats::default(),
        }
    }

    fn record_result(&mut self, result: &MockTaskResult) {
        match result.status {
            MockTaskStatus::Ok => self.stats.ok += 1,
            MockTaskStatus::Changed => self.stats.changed += 1,
            MockTaskStatus::Failed => {
                self.stats.failed += 1;
                self.failed = true;
            }
            MockTaskStatus::Skipped => self.stats.skipped += 1,
        }
    }
}

/// Simple task representation
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MockTask {
    name: String,
    fail_on_host: Option<String>,
    simulate_duration: Duration,
}

impl MockTask {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fail_on_host: None,
            simulate_duration: Duration::from_micros(100),
        }
    }

    fn with_duration(mut self, duration: Duration) -> Self {
        self.simulate_duration = duration;
        self
    }

    fn fail_on(mut self, host: &str) -> Self {
        self.fail_on_host = Some(host.to_string());
        self
    }

    async fn execute(&self, host: &str) -> MockTaskResult {
        // Simulate work
        tokio::time::sleep(self.simulate_duration).await;

        if self.fail_on_host.as_deref() == Some(host) {
            MockTaskResult::failed()
        } else {
            MockTaskResult::ok()
        }
    }
}

// ============================================================================
// Strategy Execution Simulators
// ============================================================================

/// Simulate Linear strategy execution
async fn execute_linear(
    hosts: &[String],
    tasks: &[MockTask],
    forks: usize,
) -> HashMap<String, HostState> {
    let semaphore = Arc::new(Semaphore::new(forks));
    let mut results: HashMap<String, HostState> = hosts
        .iter()
        .map(|h| (h.clone(), HostState::new(h)))
        .collect();

    for task in tasks {
        // Get active hosts (not failed)
        let active_hosts: Vec<_> = hosts
            .iter()
            .filter(|h| !results.get(*h).map(|r| r.failed).unwrap_or(false))
            .cloned()
            .collect();

        if active_hosts.is_empty() {
            break;
        }

        // Execute task on all active hosts in parallel (limited by semaphore)
        let handles: Vec<_> = active_hosts
            .iter()
            .map(|host| {
                let host = host.clone();
                let task = task.clone();
                let sem = semaphore.clone();
                tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    let result = task.execute(&host).await;
                    (host, result)
                })
            })
            .collect();

        for handle in handles {
            if let Ok((host, result)) = handle.await {
                if let Some(state) = results.get_mut(&host) {
                    state.record_result(&result);
                }
            }
        }
    }

    results
}

/// Simulate Free strategy execution
async fn execute_free(
    hosts: &[String],
    tasks: &[MockTask],
    forks: usize,
) -> HashMap<String, HostState> {
    let semaphore = Arc::new(Semaphore::new(forks));
    let tasks = Arc::new(tasks.to_vec());

    let handles: Vec<_> = hosts
        .iter()
        .map(|host| {
            let host = host.clone();
            let tasks = Arc::clone(&tasks);
            let sem = semaphore.clone();

            tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let mut state = HostState::new(&host);

                for task in tasks.iter() {
                    if state.failed {
                        break;
                    }
                    let result = task.execute(&host).await;
                    state.record_result(&result);
                }

                (host, state)
            })
        })
        .collect();

    let mut results = HashMap::new();
    for handle in handles {
        if let Ok((host, state)) = handle.await {
            results.insert(host, state);
        }
    }

    results
}

/// Simulate Host-Pinned strategy execution
async fn execute_host_pinned(
    hosts: &[String],
    tasks: &[MockTask],
    forks: usize,
) -> HashMap<String, HostState> {
    // Host-pinned is similar to free but with dedicated workers per host
    // For benchmarking purposes, we treat it similarly but measure the pattern
    execute_free(hosts, tasks, forks).await
}

/// Simulate serial batch execution
async fn execute_serial(
    hosts: &[String],
    tasks: &[MockTask],
    batch_size: usize,
    forks: usize,
) -> HashMap<String, HostState> {
    let mut all_results = HashMap::new();

    // Split hosts into batches
    for batch in hosts.chunks(batch_size) {
        let batch_hosts: Vec<String> = batch.to_vec();
        let batch_results = execute_linear(&batch_hosts, tasks, forks).await;

        for (host, state) in batch_results {
            all_results.insert(host, state);
        }
    }

    all_results
}

// ============================================================================
// 1. Linear vs Free Strategy Comparison
// ============================================================================

fn bench_linear_vs_free_strategy(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("linear_vs_free_strategy");
    group.sample_size(30);

    // Test with different host counts
    for num_hosts in [5, 10, 25, 50].iter() {
        let hosts: Vec<String> = (0..*num_hosts).map(|i| format!("host_{:04}", i)).collect();

        // Create simple tasks
        let tasks: Vec<MockTask> = (0..10)
            .map(|i| MockTask::new(&format!("task_{}", i)).with_duration(Duration::from_micros(50)))
            .collect();

        group.throughput(Throughput::Elements(*num_hosts as u64));

        // Linear strategy
        group.bench_with_input(BenchmarkId::new("linear", num_hosts), num_hosts, |b, _| {
            b.to_async(&rt).iter(|| {
                let hosts = hosts.clone();
                let tasks = tasks.clone();
                async move {
                    let results = execute_linear(&hosts, &tasks, 5).await;
                    black_box(results)
                }
            })
        });

        // Free strategy
        group.bench_with_input(BenchmarkId::new("free", num_hosts), num_hosts, |b, _| {
            b.to_async(&rt).iter(|| {
                let hosts = hosts.clone();
                let tasks = tasks.clone();
                async move {
                    let results = execute_free(&hosts, &tasks, 5).await;
                    black_box(results)
                }
            })
        });

        // Host-pinned strategy
        group.bench_with_input(
            BenchmarkId::new("host_pinned", num_hosts),
            num_hosts,
            |b, _| {
                b.to_async(&rt).iter(|| {
                    let hosts = hosts.clone();
                    let tasks = tasks.clone();
                    async move {
                        let results = execute_host_pinned(&hosts, &tasks, 5).await;
                        black_box(results)
                    }
                })
            },
        );
    }

    group.finish();
}

fn bench_strategy_with_varying_task_counts(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("strategy_varying_tasks");
    group.sample_size(20);

    let hosts: Vec<String> = (0..10).map(|i| format!("host_{:04}", i)).collect();

    for num_tasks in [5, 20, 50, 100].iter() {
        let tasks: Vec<MockTask> = (0..*num_tasks)
            .map(|i| MockTask::new(&format!("task_{}", i)).with_duration(Duration::from_micros(20)))
            .collect();

        group.throughput(Throughput::Elements(*num_tasks as u64));

        // Linear
        group.bench_with_input(BenchmarkId::new("linear", num_tasks), num_tasks, |b, _| {
            b.to_async(&rt).iter(|| {
                let hosts = hosts.clone();
                let tasks = tasks.clone();
                async move { black_box(execute_linear(&hosts, &tasks, 5).await) }
            })
        });

        // Free
        group.bench_with_input(BenchmarkId::new("free", num_tasks), num_tasks, |b, _| {
            b.to_async(&rt).iter(|| {
                let hosts = hosts.clone();
                let tasks = tasks.clone();
                async move { black_box(execute_free(&hosts, &tasks, 5).await) }
            })
        });
    }

    group.finish();
}

// ============================================================================
// 2. Batch Sizing Impact
// ============================================================================

fn bench_batch_sizing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("batch_sizing");
    group.sample_size(20);

    let hosts: Vec<String> = (0..50).map(|i| format!("host_{:04}", i)).collect();
    let tasks: Vec<MockTask> = (0..10)
        .map(|i| MockTask::new(&format!("task_{}", i)).with_duration(Duration::from_micros(30)))
        .collect();

    // Test different batch sizes
    for batch_size in [1, 2, 5, 10, 25, 50].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));

        group.bench_with_input(
            BenchmarkId::new("serial_batch", batch_size),
            batch_size,
            |b, &batch| {
                b.to_async(&rt).iter(|| {
                    let hosts = hosts.clone();
                    let tasks = tasks.clone();
                    async move { black_box(execute_serial(&hosts, &tasks, batch, 5).await) }
                })
            },
        );
    }

    // Compare to non-serial execution
    group.bench_function("no_serial_linear", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks.clone();
            async move { black_box(execute_linear(&hosts, &tasks, 5).await) }
        })
    });

    group.bench_function("no_serial_free", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks.clone();
            async move { black_box(execute_free(&hosts, &tasks, 5).await) }
        })
    });

    group.finish();
}

fn bench_serial_spec_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("serial_spec_calculation");

    // Fixed batch calculation
    group.bench_function("fixed_batch_calculate", |b| {
        let spec = SerialSpec::Fixed(5);
        b.iter(|| {
            let batches = spec.calculate_batches(black_box(100));
            black_box(batches)
        })
    });

    // Percentage batch calculation
    group.bench_function("percentage_batch_calculate", |b| {
        let spec = SerialSpec::Percentage("25%".to_string());
        b.iter(|| {
            let batches = spec.calculate_batches(black_box(100));
            black_box(batches)
        })
    });

    // Progressive batch calculation
    group.bench_function("progressive_batch_calculate", |b| {
        let spec = SerialSpec::Progressive(vec![
            SerialSpec::Fixed(1),
            SerialSpec::Fixed(5),
            SerialSpec::Fixed(10),
        ]);
        b.iter(|| {
            let batches = spec.calculate_batches(black_box(100));
            black_box(batches)
        })
    });

    // Host batching
    let hosts: Vec<String> = (0..100).map(|i| format!("host_{:04}", i)).collect();

    group.bench_function("batch_hosts_fixed", |b| {
        let spec = SerialSpec::Fixed(10);
        b.iter(|| {
            let batches = spec.batch_hosts(black_box(&hosts));
            black_box(batches)
        })
    });

    group.bench_function("batch_hosts_percentage", |b| {
        let spec = SerialSpec::Percentage("20%".to_string());
        b.iter(|| {
            let batches = spec.batch_hosts(black_box(&hosts));
            black_box(batches)
        })
    });

    group.finish();
}

// ============================================================================
// 3. Host Failure Handling Cost
// ============================================================================

fn bench_host_failure_handling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("host_failure_handling");
    group.sample_size(20);

    let hosts: Vec<String> = (0..20).map(|i| format!("host_{:04}", i)).collect();

    // No failures
    let tasks_no_fail: Vec<MockTask> = (0..10)
        .map(|i| MockTask::new(&format!("task_{}", i)).with_duration(Duration::from_micros(30)))
        .collect();

    group.bench_function("no_failures_linear", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks_no_fail.clone();
            async move { black_box(execute_linear(&hosts, &tasks, 5).await) }
        })
    });

    // 10% failure rate (2 hosts fail)
    let mut tasks_10_pct_fail = tasks_no_fail.clone();
    tasks_10_pct_fail[0] = tasks_10_pct_fail[0].clone().fail_on("host_0000");
    tasks_10_pct_fail[0] = tasks_10_pct_fail[0].clone().fail_on("host_0001");

    group.bench_function("10pct_failures_linear", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks_10_pct_fail.clone();
            async move { black_box(execute_linear(&hosts, &tasks, 5).await) }
        })
    });

    // 50% failure rate (10 hosts fail)
    let tasks_50_pct_fail: Vec<MockTask> = (0..10)
        .map(|i| {
            let mut task = MockTask::new(&format!("task_{}", i));
            if i == 0 {
                // First task fails on half the hosts
                for j in 0..10 {
                    task = task.fail_on(&format!("host_{:04}", j));
                }
            }
            task.with_duration(Duration::from_micros(30))
        })
        .collect();

    group.bench_function("50pct_failures_linear", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks_50_pct_fail.clone();
            async move { black_box(execute_linear(&hosts, &tasks, 5).await) }
        })
    });

    // Compare free strategy with failures
    group.bench_function("10pct_failures_free", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks_10_pct_fail.clone();
            async move { black_box(execute_free(&hosts, &tasks, 5).await) }
        })
    });

    group.finish();
}

fn bench_failure_skip_propagation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("failure_skip_propagation");
    group.sample_size(20);

    // Test the cost of checking failed hosts and skipping tasks
    for num_hosts in [10, 50, 100].iter() {
        let hosts: Vec<String> = (0..*num_hosts).map(|i| format!("host_{:04}", i)).collect();

        // First task fails on all hosts, remaining tasks should be skipped
        let mut tasks: Vec<MockTask> =
            vec![MockTask::new("failing_task").with_duration(Duration::from_micros(10))];

        // Add more tasks that will be skipped
        for i in 1..20 {
            tasks.push(
                MockTask::new(&format!("skipped_task_{}", i))
                    .with_duration(Duration::from_micros(10)),
            );
        }

        group.throughput(Throughput::Elements(*num_hosts as u64));

        group.bench_with_input(
            BenchmarkId::new("skip_after_failure", num_hosts),
            num_hosts,
            |b, _| {
                b.to_async(&rt).iter(|| {
                    let hosts = hosts.clone();
                    let tasks = tasks.clone();
                    async move { black_box(execute_linear(&hosts, &tasks, 5).await) }
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// 4. Strategy Selection Heuristics
// ============================================================================

/// Strategy selection based on workload characteristics
#[derive(Debug, Clone)]
struct WorkloadCharacteristics {
    num_hosts: usize,
    num_tasks: usize,
    avg_task_duration_ms: u64,
    failure_rate: f64,
    has_dependencies: bool,
}

impl WorkloadCharacteristics {
    /// Select optimal strategy based on workload characteristics
    fn select_strategy(&self) -> ExecutionStrategy {
        // Heuristic 1: For small host counts, linear is fine
        if self.num_hosts <= 3 {
            return ExecutionStrategy::Linear;
        }

        // Heuristic 2: For tasks with dependencies, must use linear
        if self.has_dependencies {
            return ExecutionStrategy::Linear;
        }

        // Heuristic 3: High failure rates benefit from linear (easier to track)
        if self.failure_rate > 0.3 {
            return ExecutionStrategy::Linear;
        }

        // Heuristic 4: Many hosts with independent tasks benefit from free
        if self.num_hosts > 10 && self.avg_task_duration_ms > 100 {
            return ExecutionStrategy::Free;
        }

        // Heuristic 5: Many short tasks with many hosts - host_pinned reduces context switching
        if self.num_hosts > 20 && self.num_tasks > 50 && self.avg_task_duration_ms < 50 {
            return ExecutionStrategy::HostPinned;
        }

        // Default to linear for predictability
        ExecutionStrategy::Linear
    }

    /// Calculate optimal batch size for serial execution
    fn optimal_batch_size(&self) -> usize {
        // Start with sqrt of hosts for balanced batching
        let base = (self.num_hosts as f64).sqrt().ceil() as usize;

        // Adjust based on failure rate (smaller batches with higher failure rates)
        let adjusted = if self.failure_rate > 0.1 {
            (base as f64 * (1.0 - self.failure_rate)).ceil() as usize
        } else {
            base
        };

        // Ensure at least 1, at most num_hosts
        adjusted.max(1).min(self.num_hosts)
    }
}

fn bench_strategy_selection_heuristics(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_selection_heuristics");

    // Benchmark strategy selection algorithm
    group.bench_function("select_strategy_small", |b| {
        let workload = WorkloadCharacteristics {
            num_hosts: 3,
            num_tasks: 10,
            avg_task_duration_ms: 100,
            failure_rate: 0.0,
            has_dependencies: false,
        };
        b.iter(|| {
            let strategy = workload.select_strategy();
            black_box(strategy)
        })
    });

    group.bench_function("select_strategy_large", |b| {
        let workload = WorkloadCharacteristics {
            num_hosts: 100,
            num_tasks: 50,
            avg_task_duration_ms: 200,
            failure_rate: 0.05,
            has_dependencies: false,
        };
        b.iter(|| {
            let strategy = workload.select_strategy();
            black_box(strategy)
        })
    });

    group.bench_function("optimal_batch_size", |b| {
        let workload = WorkloadCharacteristics {
            num_hosts: 100,
            num_tasks: 50,
            avg_task_duration_ms: 100,
            failure_rate: 0.1,
            has_dependencies: false,
        };
        b.iter(|| {
            let batch = workload.optimal_batch_size();
            black_box(batch)
        })
    });

    // Benchmark different workload characterizations
    for num_hosts in [10, 50, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("characterize_workload", num_hosts),
            num_hosts,
            |b, &n| {
                b.iter(|| {
                    let workload = WorkloadCharacteristics {
                        num_hosts: n,
                        num_tasks: 20,
                        avg_task_duration_ms: 100,
                        failure_rate: 0.05,
                        has_dependencies: false,
                    };
                    let strategy = workload.select_strategy();
                    let batch = workload.optimal_batch_size();
                    black_box((strategy, batch))
                })
            },
        );
    }

    group.finish();
}

fn bench_adaptive_batching(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("adaptive_batching");
    group.sample_size(20);

    let hosts: Vec<String> = (0..50).map(|i| format!("host_{:04}", i)).collect();
    let tasks: Vec<MockTask> = (0..15)
        .map(|i| MockTask::new(&format!("task_{}", i)).with_duration(Duration::from_micros(50)))
        .collect();

    // Fixed batch size
    group.bench_function("fixed_batch_5", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks.clone();
            async move { black_box(execute_serial(&hosts, &tasks, 5, 5).await) }
        })
    });

    // Sqrt-based adaptive batch
    let adaptive_batch = (hosts.len() as f64).sqrt().ceil() as usize;
    group.bench_function("adaptive_sqrt_batch", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks.clone();
            async move { black_box(execute_serial(&hosts, &tasks, adaptive_batch, 5).await) }
        })
    });

    // Progressive batching (1, 5, 10)
    group.bench_function("progressive_batch", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = hosts.clone();
            let tasks = tasks.clone();
            async move {
                let mut results = HashMap::new();
                let mut remaining_hosts = hosts.as_slice();
                let batch_sizes = [1usize, 5, 10];
                let mut batch_idx = 0;

                while !remaining_hosts.is_empty() {
                    let size = batch_sizes[batch_idx.min(batch_sizes.len() - 1)];
                    let actual_size = size.min(remaining_hosts.len());
                    let (batch, rest) = remaining_hosts.split_at(actual_size);

                    let batch_vec: Vec<String> = batch.to_vec();
                    let batch_results = execute_linear(&batch_vec, &tasks, 5).await;

                    for (host, state) in batch_results {
                        results.insert(host, state);
                    }

                    remaining_hosts = rest;
                    batch_idx += 1;
                }

                black_box(results)
            }
        })
    });

    group.finish();
}

// ============================================================================
// 5. Parallelization Manager Overhead
// ============================================================================

fn bench_semaphore_overhead(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("semaphore_overhead");

    // Basic semaphore acquire/release
    group.bench_function("semaphore_acquire_release", |b| {
        let sem = Arc::new(Semaphore::new(10));
        b.to_async(&rt).iter(|| {
            let sem = sem.clone();
            async move {
                let _permit = sem.acquire().await.unwrap();
                black_box(());
            }
        })
    });

    // Contention with multiple acquires
    for concurrency in [1, 5, 10, 20].iter() {
        let sem = Arc::new(Semaphore::new(*concurrency));

        group.bench_with_input(
            BenchmarkId::new("concurrent_acquire", concurrency),
            concurrency,
            |b, _| {
                b.to_async(&rt).iter(|| {
                    let sem = sem.clone();
                    async move {
                        let mut handles = vec![];
                        for _ in 0..20 {
                            let s = sem.clone();
                            handles.push(tokio::spawn(async move {
                                let _permit = s.acquire().await.unwrap();
                                tokio::task::yield_now().await;
                            }));
                        }
                        for h in handles {
                            h.await.ok();
                        }
                    }
                })
            },
        );
    }

    group.finish();
}

fn bench_forks_limit_impact(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("forks_limit_impact");
    group.sample_size(20);

    let hosts: Vec<String> = (0..20).map(|i| format!("host_{:04}", i)).collect();
    let tasks: Vec<MockTask> = (0..10)
        .map(|i| MockTask::new(&format!("task_{}", i)).with_duration(Duration::from_micros(100)))
        .collect();

    // Different fork limits
    for forks in [1, 2, 5, 10, 20].iter() {
        group.throughput(Throughput::Elements(*forks as u64));

        group.bench_with_input(BenchmarkId::new("linear_forks", forks), forks, |b, &f| {
            b.to_async(&rt).iter(|| {
                let hosts = hosts.clone();
                let tasks = tasks.clone();
                async move { black_box(execute_linear(&hosts, &tasks, f).await) }
            })
        });

        group.bench_with_input(BenchmarkId::new("free_forks", forks), forks, |b, &f| {
            b.to_async(&rt).iter(|| {
                let hosts = hosts.clone();
                let tasks = tasks.clone();
                async move { black_box(execute_free(&hosts, &tasks, f).await) }
            })
        });
    }

    group.finish();
}

// ============================================================================
// 6. Strategy Type Operations
// ============================================================================

fn bench_strategy_type_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_type_operations");

    // Strategy enum operations
    group.bench_function("strategy_default", |b| {
        b.iter(|| {
            let s = ExecutionStrategy::Linear; // Using Linear as default for bench
            black_box(s)
        })
    });

    group.bench_function("strategy_display_linear", |b| {
        let s = ExecutionStrategy::Linear;
        b.iter(|| {
            let display = format!("{:?}", s);
            black_box(display)
        })
    });

    group.bench_function("strategy_display_free", |b| {
        let s = ExecutionStrategy::Free;
        b.iter(|| {
            let display = format!("{:?}", s);
            black_box(display)
        })
    });

    group.bench_function("strategy_clone", |b| {
        let s = ExecutionStrategy::Linear;
        b.iter(|| {
            let cloned = s;
            black_box(cloned)
        })
    });

    group.bench_function("strategy_eq", |b| {
        let s1 = ExecutionStrategy::Linear;
        let s2 = ExecutionStrategy::Linear;
        b.iter(|| {
            let eq = s1 == s2;
            black_box(eq)
        })
    });

    // ExecutorConfig operations
    group.bench_function("executor_config_default", |b| {
        b.iter(|| {
            let config = ExecutorConfig::default();
            black_box(config)
        })
    });

    group.bench_function("executor_config_clone", |b| {
        let config = ExecutorConfig::default();
        b.iter(|| {
            let cloned = config.clone();
            black_box(cloned)
        })
    });

    group.finish();
}

// ============================================================================
// 7. Execution Stats Operations
// ============================================================================

fn bench_execution_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("execution_stats");

    group.bench_function("stats_default", |b| {
        b.iter(|| {
            let stats = ExecutionStats::default();
            black_box(stats)
        })
    });

    group.bench_function("stats_merge", |b| {
        let mut stats1 = ExecutionStats {
            ok: 10,
            changed: 5,
            failed: 1,
            skipped: 3,
            unreachable: 0,
        };
        let stats2 = ExecutionStats {
            ok: 8,
            changed: 4,
            failed: 2,
            skipped: 1,
            unreachable: 1,
        };
        b.iter(|| {
            stats1.merge(black_box(&stats2));
            black_box(&stats1);
        })
    });

    // Aggregating stats from many hosts
    for num_hosts in [10, 100, 500].iter() {
        let host_stats: Vec<ExecutionStats> = (0..*num_hosts)
            .map(|i| ExecutionStats {
                ok: (i % 20) as usize,
                changed: (i % 10) as usize,
                failed: (i % 50 == 0) as usize,
                skipped: (i % 5) as usize,
                unreachable: 0,
            })
            .collect();

        group.bench_with_input(
            BenchmarkId::new("aggregate_stats", num_hosts),
            num_hosts,
            |b, _| {
                b.iter(|| {
                    let mut total = ExecutionStats::default();
                    for stats in &host_stats {
                        total.merge(stats);
                    }
                    black_box(total)
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// 8. Real-World Scenario Benchmarks
// ============================================================================

fn bench_realistic_playbook_scenarios(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("realistic_scenarios");
    group.sample_size(15);

    // Scenario 1: Small cluster deployment (5 hosts, 20 tasks)
    let small_cluster_hosts: Vec<String> = (0..5).map(|i| format!("web_{}", i)).collect();
    let deployment_tasks: Vec<MockTask> = (0..20)
        .map(|i| {
            MockTask::new(&format!("deploy_step_{}", i)).with_duration(Duration::from_micros(100))
        })
        .collect();

    group.bench_function("small_cluster_linear", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = small_cluster_hosts.clone();
            let tasks = deployment_tasks.clone();
            async move { black_box(execute_linear(&hosts, &tasks, 5).await) }
        })
    });

    group.bench_function("small_cluster_free", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = small_cluster_hosts.clone();
            let tasks = deployment_tasks.clone();
            async move { black_box(execute_free(&hosts, &tasks, 5).await) }
        })
    });

    // Scenario 2: Large fleet update (100 hosts, 10 tasks, serial batches)
    let large_fleet_hosts: Vec<String> = (0..100).map(|i| format!("server_{:04}", i)).collect();
    let update_tasks: Vec<MockTask> = (0..10)
        .map(|i| {
            MockTask::new(&format!("update_step_{}", i)).with_duration(Duration::from_micros(50))
        })
        .collect();

    group.bench_function("large_fleet_serial_10", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = large_fleet_hosts.clone();
            let tasks = update_tasks.clone();
            async move { black_box(execute_serial(&hosts, &tasks, 10, 10).await) }
        })
    });

    group.bench_function("large_fleet_serial_25", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = large_fleet_hosts.clone();
            let tasks = update_tasks.clone();
            async move { black_box(execute_serial(&hosts, &tasks, 25, 10).await) }
        })
    });

    // Scenario 3: Database cluster with strict ordering (3 hosts, sequential)
    let db_cluster_hosts: Vec<String> = vec![
        "db_primary".to_string(),
        "db_replica_1".to_string(),
        "db_replica_2".to_string(),
    ];
    let db_tasks: Vec<MockTask> = (0..15)
        .map(|i| {
            MockTask::new(&format!("db_operation_{}", i)).with_duration(Duration::from_micros(200))
        })
        .collect();

    group.bench_function("db_cluster_linear", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = db_cluster_hosts.clone();
            let tasks = db_tasks.clone();
            async move { black_box(execute_linear(&hosts, &tasks, 1).await) }
        })
    });

    group.bench_function("db_cluster_serial_1", |b| {
        b.to_async(&rt).iter(|| {
            let hosts = db_cluster_hosts.clone();
            let tasks = db_tasks.clone();
            async move { black_box(execute_serial(&hosts, &tasks, 1, 1).await) }
        })
    });

    group.finish();
}

// ============================================================================
// Criterion Groups and Main
// ============================================================================

criterion_group!(
    strategy_comparison,
    bench_linear_vs_free_strategy,
    bench_strategy_with_varying_task_counts,
);

criterion_group!(
    batch_sizing,
    bench_batch_sizing,
    bench_serial_spec_calculation,
);

criterion_group!(
    failure_handling,
    bench_host_failure_handling,
    bench_failure_skip_propagation,
);

criterion_group!(
    strategy_selection,
    bench_strategy_selection_heuristics,
    bench_adaptive_batching,
);

criterion_group!(
    parallelization,
    bench_semaphore_overhead,
    bench_forks_limit_impact,
);

criterion_group!(
    type_operations,
    bench_strategy_type_operations,
    bench_execution_stats,
);

criterion_group!(realistic_scenarios, bench_realistic_playbook_scenarios,);

criterion_main!(
    strategy_comparison,
    batch_sizing,
    failure_handling,
    strategy_selection,
    parallelization,
    type_operations,
    realistic_scenarios,
);
