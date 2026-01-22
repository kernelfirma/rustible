//! Parallel Execution Performance Benchmarks for Rustible
//!
//! This benchmark suite focuses on parallel task execution performance:
//! - Task scheduling overhead with tokio::spawn
//! - Semaphore acquisition patterns and contention
//! - ParallelizationManager guard acquisition costs
//! - Context switching under different workloads
//! - Memory allocation per concurrent task
//! - Optimal fork count determination
//! - Work stealing simulation patterns
//! - Concurrent limit tuning
//!
//! Run with: cargo bench --bench parallel_benchmark

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rustible::executor::parallelization::{ParallelizationManager, ParallelizationStats};
use rustible::modules::ParallelizationHint;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, RwLock, Semaphore};

// ============================================================================
// Constants for benchmark configuration
// ============================================================================

/// Simulated work durations for different task types
const FAST_TASK_MICROS: u64 = 10;
const MEDIUM_TASK_MICROS: u64 = 100;
const SLOW_TASK_MICROS: u64 = 1000;

/// Maximum hosts to test scaling
const MAX_HOSTS: usize = 100;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a tokio runtime with the specified number of worker threads
fn create_runtime(workers: usize) -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .unwrap()
}

/// Simulate CPU-bound work
fn simulate_cpu_work(iterations: u64) {
    let mut sum: u64 = 0;
    for i in 0..iterations {
        sum = sum.wrapping_add(i.wrapping_mul(i));
    }
    black_box(sum);
}

/// Simulate async I/O-bound work
async fn simulate_io_work(duration_micros: u64) {
    tokio::time::sleep(Duration::from_micros(duration_micros)).await;
}

/// Simulate mixed workload
async fn simulate_mixed_work(cpu_iterations: u64, io_micros: u64) {
    simulate_cpu_work(cpu_iterations);
    simulate_io_work(io_micros).await;
}

// ============================================================================
// Benchmark: Task Scheduling Overhead
// ============================================================================

fn bench_task_spawn_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("task_spawn_overhead");

    for task_count in [1, 10, 50, 100, 500, 1000].iter() {
        let rt = create_runtime(4);
        group.throughput(Throughput::Elements(*task_count as u64));

        group.bench_with_input(
            BenchmarkId::new("tokio_spawn", task_count),
            task_count,
            |b, &count| {
                b.to_async(&rt).iter(|| async move {
                    let handles: Vec<_> = (0..count)
                        .map(|i| tokio::spawn(async move { black_box(i * 2) }))
                        .collect();

                    for handle in handles {
                        black_box(handle.await.ok());
                    }
                });
            },
        );

        // Compare with join_all
        group.bench_with_input(
            BenchmarkId::new("futures_join_all", task_count),
            task_count,
            |b, &count| {
                b.to_async(&rt).iter(|| async move {
                    let futures: Vec<_> = (0..count)
                        .map(|i| async move { black_box(i * 2) })
                        .collect();

                    let results = futures::future::join_all(futures).await;
                    black_box(results);
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark: Semaphore Acquisition Patterns
// ============================================================================

fn bench_semaphore_acquisition(c: &mut Criterion) {
    let mut group = c.benchmark_group("semaphore_acquisition");

    // Test different semaphore capacities (fork counts)
    for permits in [1, 5, 10, 20, 50].iter() {
        let rt = create_runtime(4);

        // Uncontended acquisition
        group.bench_with_input(
            BenchmarkId::new("uncontended", permits),
            permits,
            |b, &p| {
                b.to_async(&rt).iter(|| async move {
                    let sem = Semaphore::new(p);
                    for _ in 0..p {
                        let permit = sem.acquire().await.unwrap();
                        black_box(permit);
                    }
                });
            },
        );

        // Contended acquisition with waiters
        group.bench_with_input(BenchmarkId::new("contended", permits), permits, |b, &p| {
            b.to_async(&rt).iter(|| async move {
                let sem = Arc::new(Semaphore::new(p));
                let completed = Arc::new(AtomicUsize::new(0));
                let num_tasks = p * 3; // 3x oversubscription

                let handles: Vec<_> = (0..num_tasks)
                    .map(|_| {
                        let sem = Arc::clone(&sem);
                        let completed = Arc::clone(&completed);
                        tokio::spawn(async move {
                            let _permit = sem.acquire().await.unwrap();
                            // Minimal work while holding permit
                            simulate_io_work(FAST_TASK_MICROS).await;
                            completed.fetch_add(1, Ordering::Relaxed);
                        })
                    })
                    .collect();

                for handle in handles {
                    handle.await.ok();
                }
                assert_eq!(completed.load(Ordering::Relaxed), num_tasks);
            });
        });
    }

    group.finish();
}

// ============================================================================
// Benchmark: ParallelizationManager Performance
// ============================================================================

fn bench_parallelization_manager(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallelization_manager");
    let rt = create_runtime(4);

    // Test FullyParallel hint (no blocking)
    group.bench_function("fully_parallel_acquire", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = ParallelizationManager::new();
            for i in 0..100 {
                let _guard = manager
                    .acquire(
                        ParallelizationHint::FullyParallel,
                        &format!("host{}", i),
                        "test_module",
                    )
                    .await;
            }
        });
    });

    // Test HostExclusive hint with different hosts
    group.bench_function("host_exclusive_different_hosts", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = Arc::new(ParallelizationManager::new());
            let handles: Vec<_> = (0..20)
                .map(|i| {
                    let manager = Arc::clone(&manager);
                    let host = format!("host{}", i);
                    tokio::spawn(async move {
                        let _guard = manager
                            .acquire(ParallelizationHint::HostExclusive, &host, "test_module")
                            .await;
                        simulate_io_work(FAST_TASK_MICROS).await;
                    })
                })
                .collect();

            for handle in handles {
                handle.await.ok();
            }
        });
    });

    // Test HostExclusive hint with same host (serialized)
    group.bench_function("host_exclusive_same_host", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = Arc::new(ParallelizationManager::new());
            let handles: Vec<_> = (0..10)
                .map(|_| {
                    let manager = Arc::clone(&manager);
                    tokio::spawn(async move {
                        let _guard = manager
                            .acquire(
                                ParallelizationHint::HostExclusive,
                                "same_host",
                                "test_module",
                            )
                            .await;
                        // Minimal work while holding lock
                        black_box(42);
                    })
                })
                .collect();

            for handle in handles {
                handle.await.ok();
            }
        });
    });

    // Test GlobalExclusive hint
    group.bench_function("global_exclusive", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = Arc::new(ParallelizationManager::new());
            let handles: Vec<_> = (0..10)
                .map(|i| {
                    let manager = Arc::clone(&manager);
                    let host = format!("host{}", i);
                    tokio::spawn(async move {
                        let _guard = manager
                            .acquire(ParallelizationHint::GlobalExclusive, &host, "test_module")
                            .await;
                        black_box(42);
                    })
                })
                .collect();

            for handle in handles {
                handle.await.ok();
            }
        });
    });

    // Test RateLimited hint
    group.bench_function("rate_limited_10rps", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = ParallelizationManager::new();
            // Only request 2 tokens to avoid long waits
            for _ in 0..2 {
                let _guard = manager
                    .acquire(
                        ParallelizationHint::RateLimited {
                            requests_per_second: 10,
                        },
                        "host1",
                        "rate_limited_module",
                    )
                    .await;
            }
        });
    });

    // Test stats collection overhead
    group.bench_function("stats_collection", |b| {
        b.iter(|| {
            let manager = ParallelizationManager::new();
            for _ in 0..100 {
                let stats = manager.stats();
                black_box(stats);
            }
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark: Concurrent Limits and Fork Scaling
// ============================================================================

fn bench_fork_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("fork_scaling");
    group.sample_size(20); // Reduce sample size for longer benchmarks

    // Test throughput with different fork counts
    for forks in [1, 2, 5, 10, 20, 50].iter() {
        let rt = create_runtime(4);

        // Simulate task execution with semaphore-limited parallelism
        group.throughput(Throughput::Elements(100));
        group.bench_with_input(
            BenchmarkId::new("io_bound_tasks", forks),
            forks,
            |b, &fork_count| {
                b.to_async(&rt).iter(|| async move {
                    let sem = Arc::new(Semaphore::new(fork_count));
                    let completed = Arc::new(AtomicUsize::new(0));
                    let total_tasks = 100;

                    let handles: Vec<_> = (0..total_tasks)
                        .map(|_| {
                            let sem = Arc::clone(&sem);
                            let completed = Arc::clone(&completed);
                            tokio::spawn(async move {
                                let _permit = sem.acquire().await.unwrap();
                                simulate_io_work(MEDIUM_TASK_MICROS).await;
                                completed.fetch_add(1, Ordering::Relaxed);
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.await.ok();
                    }
                });
            },
        );

        // CPU-bound tasks
        group.bench_with_input(
            BenchmarkId::new("cpu_bound_tasks", forks),
            forks,
            |b, &fork_count| {
                b.to_async(&rt).iter(|| async move {
                    let sem = Arc::new(Semaphore::new(fork_count));
                    let total_tasks = 50;

                    let handles: Vec<_> = (0..total_tasks)
                        .map(|_| {
                            let sem = Arc::clone(&sem);
                            tokio::spawn(async move {
                                let _permit = sem.acquire().await.unwrap();
                                // Use spawn_blocking for CPU work to not block runtime
                                tokio::task::spawn_blocking(|| {
                                    simulate_cpu_work(10000);
                                })
                                .await
                                .ok();
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.await.ok();
                    }
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark: Context Switching Overhead
// ============================================================================

fn bench_context_switching(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_switching");
    let rt = create_runtime(4);

    // Measure overhead of frequent yield points
    for yield_count in [0, 10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("yield_overhead", yield_count),
            yield_count,
            |b, &yields| {
                b.to_async(&rt).iter(|| async move {
                    for _ in 0..yields {
                        tokio::task::yield_now().await;
                    }
                });
            },
        );
    }

    // Measure task switching with many concurrent tasks
    for task_count in [10, 50, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("task_switch_pressure", task_count),
            task_count,
            |b, &count| {
                b.to_async(&rt).iter(|| async move {
                    let barrier = Arc::new(tokio::sync::Barrier::new(count));
                    let handles: Vec<_> = (0..count)
                        .map(|_| {
                            let barrier = Arc::clone(&barrier);
                            tokio::spawn(async move {
                                // Force synchronization to measure switching
                                barrier.wait().await;
                                tokio::task::yield_now().await;
                                barrier.wait().await;
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.await.ok();
                    }
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark: Memory Per Concurrent Task
// ============================================================================

fn bench_memory_per_task(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_per_task");

    // Measure memory allocation patterns with different task sizes
    for task_count in [10, 100, 1000].iter() {
        let rt = create_runtime(4);

        // Minimal task (just a number)
        group.bench_with_input(
            BenchmarkId::new("minimal_task", task_count),
            task_count,
            |b, &count| {
                b.to_async(&rt).iter(|| async move {
                    let handles: Vec<_> =
                        (0..count).map(|i| tokio::spawn(async move { i })).collect();

                    for handle in handles {
                        black_box(handle.await.ok());
                    }
                });
            },
        );

        // Task with captured state (simulates ExecutionContext)
        group.bench_with_input(
            BenchmarkId::new("with_context", task_count),
            task_count,
            |b, &count| {
                b.to_async(&rt).iter(|| async move {
                    let shared_config = Arc::new(vec![0u8; 1024]); // 1KB shared config

                    let handles: Vec<_> = (0..count)
                        .map(|i| {
                            let config = Arc::clone(&shared_config);
                            let host = format!("host{:04}", i);
                            tokio::spawn(async move {
                                black_box(&config);
                                black_box(&host);
                                i
                            })
                        })
                        .collect();

                    for handle in handles {
                        black_box(handle.await.ok());
                    }
                });
            },
        );

        // Task with larger captured state
        group.bench_with_input(
            BenchmarkId::new("with_large_state", task_count),
            task_count,
            |b, &count| {
                b.to_async(&rt).iter(|| async move {
                    let handles: Vec<_> = (0..count)
                        .map(|i| {
                            // Each task captures its own data
                            let task_data = vec![i as u8; 4096]; // 4KB per task
                            tokio::spawn(async move {
                                black_box(&task_data);
                                i
                            })
                        })
                        .collect();

                    for handle in handles {
                        black_box(handle.await.ok());
                    }
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark: Work Stealing Patterns
// ============================================================================

fn bench_work_stealing(c: &mut Criterion) {
    let mut group = c.benchmark_group("work_stealing");
    group.sample_size(20);

    // Simulate work stealing with varying work distribution
    for worker_count in [1, 2, 4, 8].iter() {
        let rt = create_runtime(*worker_count);

        // Uniform work distribution
        group.bench_with_input(
            BenchmarkId::new("uniform_work", worker_count),
            worker_count,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    let handles: Vec<_> = (0..100)
                        .map(|_| {
                            tokio::spawn(async {
                                simulate_io_work(MEDIUM_TASK_MICROS).await;
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.await.ok();
                    }
                });
            },
        );

        // Skewed work distribution (some tasks much longer)
        group.bench_with_input(
            BenchmarkId::new("skewed_work", worker_count),
            worker_count,
            |b, _| {
                b.to_async(&rt).iter(|| async {
                    let handles: Vec<_> = (0..100)
                        .map(|i| {
                            let work_duration = if i % 10 == 0 {
                                SLOW_TASK_MICROS // 10% slow tasks
                            } else {
                                FAST_TASK_MICROS
                            };
                            tokio::spawn(async move {
                                simulate_io_work(work_duration).await;
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.await.ok();
                    }
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark: Lock Contention Patterns
// ============================================================================

fn bench_lock_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("lock_contention");
    let rt = create_runtime(4);

    // RwLock read-heavy workload
    group.bench_function("rwlock_read_heavy", |b| {
        b.to_async(&rt).iter(|| async {
            let data = Arc::new(RwLock::new(vec![0u64; 1000]));
            let handles: Vec<_> = (0..100)
                .map(|i| {
                    let data = Arc::clone(&data);
                    tokio::spawn(async move {
                        if i % 10 == 0 {
                            // 10% writes
                            let mut guard = data.write().await;
                            guard[0] = i;
                        } else {
                            // 90% reads
                            let guard = data.read().await;
                            black_box(guard[0]);
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.await.ok();
            }
        });
    });

    // RwLock write-heavy workload
    group.bench_function("rwlock_write_heavy", |b| {
        b.to_async(&rt).iter(|| async {
            let data = Arc::new(RwLock::new(vec![0u64; 1000]));
            let handles: Vec<_> = (0..100)
                .map(|i| {
                    let data = Arc::clone(&data);
                    tokio::spawn(async move {
                        if i % 2 == 0 {
                            // 50% writes
                            let mut guard = data.write().await;
                            guard[0] = i;
                        } else {
                            // 50% reads
                            let guard = data.read().await;
                            black_box(guard[0]);
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.await.ok();
            }
        });
    });

    // Mutex contention
    group.bench_function("mutex_contention", |b| {
        b.to_async(&rt).iter(|| async {
            let counter = Arc::new(Mutex::new(0u64));
            let handles: Vec<_> = (0..100)
                .map(|_| {
                    let counter = Arc::clone(&counter);
                    tokio::spawn(async move {
                        let mut guard = counter.lock().await;
                        *guard += 1;
                    })
                })
                .collect();

            for handle in handles {
                handle.await.ok();
            }
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark: Realistic Execution Scenarios
// ============================================================================

fn bench_realistic_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_scenarios");
    group.sample_size(20);

    let rt = create_runtime(4);

    // Simulate linear strategy: all hosts per task
    group.bench_function("linear_strategy_20_hosts", |b| {
        b.to_async(&rt).iter(|| async {
            let sem = Arc::new(Semaphore::new(5)); // 5 forks
            let tasks = 10; // 10 tasks
            let hosts = 20; // 20 hosts

            for _task in 0..tasks {
                let handles: Vec<_> = (0..hosts)
                    .map(|_| {
                        let sem = Arc::clone(&sem);
                        tokio::spawn(async move {
                            let _permit = sem.acquire().await.unwrap();
                            simulate_io_work(MEDIUM_TASK_MICROS).await;
                        })
                    })
                    .collect();

                // Wait for all hosts to complete this task
                for handle in handles {
                    handle.await.ok();
                }
            }
        });
    });

    // Simulate free strategy: each host runs independently
    group.bench_function("free_strategy_20_hosts", |b| {
        b.to_async(&rt).iter(|| async {
            let sem = Arc::new(Semaphore::new(5)); // 5 forks
            let tasks_per_host = 10;
            let hosts = 20;

            let handles: Vec<_> = (0..hosts)
                .map(|_| {
                    let sem = Arc::clone(&sem);
                    tokio::spawn(async move {
                        let _permit = sem.acquire().await.unwrap();
                        // Each host runs all its tasks sequentially
                        for _ in 0..tasks_per_host {
                            simulate_io_work(MEDIUM_TASK_MICROS).await;
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.await.ok();
            }
        });
    });

    // Simulate serial batching with max_fail_percentage
    group.bench_function("serial_batching", |b| {
        b.to_async(&rt).iter(|| async {
            let batch_size = 5;
            let total_hosts = 20;
            let tasks_per_batch = 10;

            for batch_start in (0..total_hosts).step_by(batch_size) {
                let batch_end = std::cmp::min(batch_start + batch_size, total_hosts);
                let hosts_in_batch = batch_end - batch_start;

                let handles: Vec<_> = (0..hosts_in_batch)
                    .map(|_| {
                        tokio::spawn(async move {
                            for _ in 0..tasks_per_batch {
                                simulate_io_work(FAST_TASK_MICROS).await;
                            }
                        })
                    })
                    .collect();

                for handle in handles {
                    handle.await.ok();
                }
            }
        });
    });

    // Mixed module parallelization hints
    group.bench_function("mixed_parallelization_hints", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = Arc::new(ParallelizationManager::new());
            let hosts = 10;

            // First, FullyParallel modules (e.g., debug, set_fact)
            let handles: Vec<_> = (0..hosts)
                .map(|i| {
                    let manager = Arc::clone(&manager);
                    let host = format!("host{}", i);
                    tokio::spawn(async move {
                        let _guard = manager
                            .acquire(ParallelizationHint::FullyParallel, &host, "debug")
                            .await;
                        simulate_io_work(FAST_TASK_MICROS).await;
                    })
                })
                .collect();
            for handle in handles {
                handle.await.ok();
            }

            // Then, HostExclusive modules (e.g., apt, package managers)
            let handles: Vec<_> = (0..hosts)
                .map(|i| {
                    let manager = Arc::clone(&manager);
                    let host = format!("host{}", i);
                    tokio::spawn(async move {
                        let _guard = manager
                            .acquire(ParallelizationHint::HostExclusive, &host, "apt")
                            .await;
                        simulate_io_work(MEDIUM_TASK_MICROS).await;
                    })
                })
                .collect();
            for handle in handles {
                handle.await.ok();
            }
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark: Optimal Fork Count Discovery
// ============================================================================

fn bench_optimal_forks(c: &mut Criterion) {
    let mut group = c.benchmark_group("optimal_fork_discovery");
    group.sample_size(15);

    // Test different fork counts to find optimal
    let rt = create_runtime(4);
    let hosts = 50;
    let tasks_per_host = 5;

    for forks in [1, 2, 3, 5, 8, 10, 15, 20, 25, 50].iter() {
        group.throughput(Throughput::Elements((hosts * tasks_per_host) as u64));
        group.bench_with_input(
            BenchmarkId::new("total_throughput", forks),
            forks,
            |b, &fork_count| {
                b.to_async(&rt).iter(|| async move {
                    let sem = Arc::new(Semaphore::new(fork_count));

                    let handles: Vec<_> = (0..hosts)
                        .flat_map(|h| {
                            let sem = Arc::clone(&sem);
                            (0..tasks_per_host).map(move |t| {
                                let sem = Arc::clone(&sem);
                                tokio::spawn(async move {
                                    let _permit = sem.acquire().await.unwrap();
                                    // Simulate realistic SSH command execution
                                    simulate_io_work(MEDIUM_TASK_MICROS).await;
                                    (h, t)
                                })
                            })
                        })
                        .collect();

                    for handle in handles {
                        black_box(handle.await.ok());
                    }
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark: Parallelization Guard Lifecycle
// ============================================================================

fn bench_guard_lifecycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("guard_lifecycle");
    let rt = create_runtime(4);

    // Test guard drop overhead
    group.bench_function("guard_acquire_drop_cycle", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = ParallelizationManager::new();
            for _ in 0..100 {
                let guard = manager
                    .acquire(ParallelizationHint::HostExclusive, "host1", "module1")
                    .await;
                drop(guard); // Explicit drop
            }
        });
    });

    // Test multiple guards in scope
    group.bench_function("multiple_guards_in_scope", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = ParallelizationManager::new();
            let mut guards = Vec::with_capacity(10);

            for i in 0..10 {
                let guard = manager
                    .acquire(
                        ParallelizationHint::HostExclusive,
                        &format!("host{}", i),
                        "module1",
                    )
                    .await;
                guards.push(guard);
            }

            // All guards dropped at once
            drop(guards);
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(spawn_benches, bench_task_spawn_overhead,);

criterion_group!(semaphore_benches, bench_semaphore_acquisition,);

criterion_group!(
    parallelization_benches,
    bench_parallelization_manager,
    bench_guard_lifecycle,
);

criterion_group!(scaling_benches, bench_fork_scaling, bench_optimal_forks,);

criterion_group!(
    overhead_benches,
    bench_context_switching,
    bench_memory_per_task,
);

criterion_group!(workload_benches, bench_work_stealing, bench_lock_contention,);

criterion_group!(scenario_benches, bench_realistic_scenarios,);

criterion_main!(
    spawn_benches,
    semaphore_benches,
    parallelization_benches,
    scaling_benches,
    overhead_benches,
    workload_benches,
    scenario_benches,
);
