//! Connection Pool Performance Benchmarks
//!
//! This benchmark suite measures the performance of the RusshConnectionPool,
//! including connection creation time, pool exhaustion handling, and concurrent
//! connection scaling.
//!
//! # Benchmark Groups
//!
//! - **pool_creation**: Measures pool instantiation overhead
//! - **connection_creation**: Profiles SSH connection establishment time
//! - **pool_exhaustion**: Tests behavior when pool limits are reached
//! - **concurrent_scaling**: Measures performance under concurrent load
//! - **warmup**: Benchmarks connection pre-warming
//! - **health_checks**: Measures health check overhead
//!
//! # Running the Benchmarks
//!
//! For mock benchmarks (no SSH required):
//! ```bash
//! cargo bench --bench connection_pool_benchmark
//! ```
//!
//! For real SSH benchmarks:
//! ```bash
//! export SSH_BENCH_HOST="192.168.178.102"
//! export SSH_BENCH_PORT="22"
//! export SSH_BENCH_USER="artur"
//! export SSH_BENCH_KEY="~/.ssh/id_ed25519"
//! cargo bench --bench connection_pool_benchmark
//! ```

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

// ============================================================================
// Configuration
// ============================================================================

/// SSH benchmark configuration loaded from environment variables
struct SshBenchConfig {
    host: String,
    port: u16,
    user: String,
    key_path: String,
}

impl SshBenchConfig {
    /// Try to load configuration from environment variables
    fn from_env() -> Option<Self> {
        let host = std::env::var("SSH_BENCH_HOST").ok()?;
        let port = std::env::var("SSH_BENCH_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(22);
        let user = std::env::var("SSH_BENCH_USER").ok().unwrap_or_else(whoami);
        let key_path = std::env::var("SSH_BENCH_KEY")
            .ok()
            .unwrap_or_else(|| "~/.ssh/id_ed25519".to_string());
        let key_path = expand_path(&key_path);

        Some(Self {
            host,
            port,
            user,
            key_path,
        })
    }

    /// Check if the configuration is valid (key file exists)
    fn is_valid(&self) -> bool {
        Path::new(&self.key_path).exists()
    }
}

/// Get the current username
fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

/// Expand tilde in paths
fn expand_path(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}/{}", home, stripped);
        }
    }
    path.to_string()
}

// ============================================================================
// Mock Pool Benchmarks (No SSH Required)
// ============================================================================

/// Benchmark pool creation overhead
fn bench_pool_creation(c: &mut Criterion) {
    use rustible::connection::config::ConnectionConfig;
    use rustible::connection::russh_pool::{PoolConfig, RusshConnectionPool};

    let mut group = c.benchmark_group("pool_creation");

    // Default pool creation
    group.bench_function("default_config", |b| {
        b.iter(|| {
            let pool = RusshConnectionPool::new(ConnectionConfig::default());
            black_box(pool)
        })
    });

    // Custom pool configuration
    group.bench_function("custom_config", |b| {
        b.iter(|| {
            let pool_config = PoolConfig::new()
                .max_connections_per_host(10)
                .min_connections_per_host(2)
                .max_total_connections(100)
                .idle_timeout(Duration::from_secs(600))
                .health_check_interval(Duration::from_secs(30))
                .enable_health_checks(true);

            let pool = RusshConnectionPool::with_config(ConnectionConfig::default(), pool_config);
            black_box(pool)
        })
    });

    // Builder pattern
    group.bench_function("builder_pattern", |b| {
        use rustible::connection::russh_pool::RusshConnectionPoolBuilder;

        b.iter(|| {
            let pool = RusshConnectionPoolBuilder::new()
                .max_connections_per_host(10)
                .min_connections_per_host(2)
                .max_total_connections(100)
                .idle_timeout(Duration::from_secs(600))
                .health_check_interval(Duration::from_secs(30))
                .enable_health_checks(true)
                .build();
            black_box(pool)
        })
    });

    group.finish();
}

/// Benchmark stats retrieval
fn bench_stats_retrieval(c: &mut Criterion) {
    use rustible::connection::config::ConnectionConfig;
    use rustible::connection::russh_pool::RusshConnectionPool;

    let rt = Runtime::new().unwrap();
    let pool = Arc::new(RusshConnectionPool::new(ConnectionConfig::default()));

    let mut group = c.benchmark_group("stats_retrieval");

    // Stats retrieval from empty pool
    group.bench_function("empty_pool", |b| {
        let pool = Arc::clone(&pool);
        b.to_async(&rt).iter(|| async {
            let stats = pool.stats().await;
            black_box(stats)
        })
    });

    // Stats calculation methods
    group.bench_function("stats_calculations", |b| {
        b.iter(|| {
            use rustible::connection::russh_pool::PoolStats;

            let stats = PoolStats {
                total_connections: 50,
                active_connections: 30,
                idle_connections: 20,
                hits: 1000,
                misses: 100,
                total_connection_time_ns: 5_000_000_000, // 5 seconds
                connection_creation_count: 100,
                max_connection_time_ns: 200_000_000,
                min_connection_time_ns: 10_000_000,
                total_wait_time_ns: 1_000_000_000,
                wait_count: 50,
                peak_active_connections: 45,
                ..Default::default()
            };

            let avg_conn = stats.avg_connection_time_ms();
            let avg_wait = stats.avg_wait_time_ms();
            let hit_rate = stats.hit_rate();
            let utilization = stats.utilization();

            black_box((avg_conn, avg_wait, hit_rate, utilization))
        })
    });

    group.finish();
}

/// Benchmark pool key generation
fn bench_pool_key_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_key_generation");

    let hosts = vec![
        ("192.168.1.1", 22, "admin"),
        ("example.com", 2222, "user"),
        ("long-hostname.subdomain.example.org", 22, "automation"),
        ("[::1]", 22, "root"),
    ];

    for (host, port, user) in hosts {
        group.bench_with_input(
            BenchmarkId::new("connection_key", format!("{}:{}", host, port)),
            &(host, port, user),
            |b, (h, p, u)| {
                b.iter(|| {
                    let key = format!("ssh://{}@{}:{}", u, h, p);
                    black_box(key)
                })
            },
        );
    }

    group.finish();
}

/// Benchmark mock concurrent access patterns
fn bench_mock_concurrent_access(c: &mut Criterion) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("mock_concurrent_access");
    group.sample_size(20);

    // Simulate pool access patterns
    for num_concurrent in [10, 50, 100, 200] {
        group.throughput(Throughput::Elements(num_concurrent as u64));

        // Simulate connection acquisition with contention
        group.bench_with_input(
            BenchmarkId::new("acquire_contention", num_concurrent),
            &num_concurrent,
            |b, &num| {
                let counter = Arc::new(AtomicUsize::new(0));

                b.to_async(&rt).iter(|| {
                    let counter = Arc::clone(&counter);

                    async move {
                        let mut handles = Vec::with_capacity(num);

                        for _ in 0..num {
                            let c = Arc::clone(&counter);
                            handles.push(tokio::spawn(async move {
                                // Simulate connection acquisition
                                let id = c.fetch_add(1, Ordering::SeqCst);
                                tokio::task::yield_now().await;
                                id
                            }));
                        }

                        for handle in handles {
                            black_box(handle.await.unwrap());
                        }
                    }
                })
            },
        );

        // Simulate RwLock contention (pool pattern)
        group.bench_with_input(
            BenchmarkId::new("rwlock_contention", num_concurrent),
            &num_concurrent,
            |b, &num| {
                use tokio::sync::RwLock;

                let data = Arc::new(RwLock::new(Vec::<usize>::new()));

                b.to_async(&rt).iter(|| {
                    let data = Arc::clone(&data);

                    async move {
                        let mut handles = Vec::with_capacity(num);

                        for i in 0..num {
                            let d = Arc::clone(&data);
                            handles.push(tokio::spawn(async move {
                                if i % 5 == 0 {
                                    // 20% writes
                                    let mut guard = d.write().await;
                                    guard.push(i);
                                } else {
                                    // 80% reads
                                    let guard = d.read().await;
                                    black_box(guard.len());
                                }
                            }));
                        }

                        for handle in handles {
                            handle.await.unwrap();
                        }
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark warmup result calculations
fn bench_warmup_calculations(c: &mut Criterion) {
    use rustible::connection::russh_pool::WarmupResult;

    let mut group = c.benchmark_group("warmup_calculations");

    group.bench_function("success_check", |b| {
        let result = WarmupResult {
            total_hosts: 10,
            successful_hosts: 10,
            failed_hosts: 0,
            total_connections: 50,
            successful_connections: 50,
            failed_connections: 0,
            warmup_duration_ms: 1500.0,
        };

        b.iter(|| {
            let is_success = result.is_success();
            let success_rate = result.success_rate();
            black_box((is_success, success_rate))
        })
    });

    group.bench_function("partial_failure", |b| {
        let result = WarmupResult {
            total_hosts: 10,
            successful_hosts: 8,
            failed_hosts: 2,
            total_connections: 50,
            successful_connections: 40,
            failed_connections: 10,
            warmup_duration_ms: 2500.0,
        };

        b.iter(|| {
            let is_success = result.is_success();
            let success_rate = result.success_rate();
            black_box((is_success, success_rate))
        })
    });

    group.finish();
}

/// Benchmark health check result calculations
fn bench_health_check_calculations(c: &mut Criterion) {
    use rustible::connection::russh_pool::HealthCheckResult;

    let mut group = c.benchmark_group("health_check_calculations");

    group.bench_function("all_healthy", |b| {
        let result = HealthCheckResult {
            healthy_connections: 50,
            unhealthy_connections: 0,
            removed_connections: 0,
            check_duration_ms: 100.0,
        };

        b.iter(|| {
            let all_healthy = result.all_healthy();
            let health_rate = result.health_rate();
            black_box((all_healthy, health_rate))
        })
    });

    group.bench_function("some_unhealthy", |b| {
        let result = HealthCheckResult {
            healthy_connections: 45,
            unhealthy_connections: 5,
            removed_connections: 5,
            check_duration_ms: 150.0,
        };

        b.iter(|| {
            let all_healthy = result.all_healthy();
            let health_rate = result.health_rate();
            black_box((all_healthy, health_rate))
        })
    });

    group.finish();
}

// ============================================================================
// Real SSH Benchmarks (Require SSH_BENCH_HOST)
// ============================================================================

/// Benchmark real connection creation time
fn bench_real_connection_creation(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping real connection benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping real connection benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    use rustible::connection::config::{ConnectionConfig, HostConfig};
    use rustible::connection::russh_pool::{PoolConfig, RusshConnectionPool};

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("real_connection_creation");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(30));

    let host_config = HostConfig {
        identity_file: Some(config.key_path.clone()),
        ..Default::default()
    };

    let pool_config = PoolConfig::new()
        .max_connections_per_host(20)
        .max_total_connections(100)
        .enable_health_checks(false);

    let conn_config = ConnectionConfig::default();
    let pool = Arc::new(RusshConnectionPool::with_config(conn_config, pool_config));

    // Single connection creation (cold)
    group.bench_function("single_connection_cold", |b| {
        let pool = Arc::clone(&pool);
        let hc = Some(host_config.clone());
        let host = config.host.clone();
        let port = config.port;
        let user = config.user.clone();

        b.to_async(&rt).iter(|| {
            let p = Arc::clone(&pool);
            let h = host.clone();
            let u = user.clone();
            let hconf = hc.clone();

            async move {
                // Close all first to ensure cold start
                let _ = p.close_all().await;

                // Create fresh pool
                let fresh_pool = Arc::new(RusshConnectionPool::new(ConnectionConfig::default()));

                let result = fresh_pool.get_with_config(&h, port, &u, hconf).await;
                black_box(result)
            }
        })
    });

    // Connection reuse (warm pool)
    group.bench_function("connection_reuse_warm", |b| {
        let pool = Arc::clone(&pool);
        let hc = Some(host_config.clone());
        let host = config.host.clone();
        let port = config.port;
        let user = config.user.clone();

        // Pre-create a connection
        rt.block_on(async {
            let _ = pool.get_with_config(&host, port, &user, hc.clone()).await;
        });

        b.to_async(&rt).iter(|| {
            let p = Arc::clone(&pool);
            let h = host.clone();
            let u = user.clone();
            let hconf = hc.clone();

            async move {
                let result = p.get_with_config(&h, port, &u, hconf).await;
                black_box(result)
            }
        })
    });

    group.finish();
}

/// Benchmark pool exhaustion handling
fn bench_pool_exhaustion(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping pool exhaustion benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping pool exhaustion benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    use rustible::connection::config::{ConnectionConfig, HostConfig};
    use rustible::connection::russh_pool::{PoolConfig, RusshConnectionPool};

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("pool_exhaustion");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(60));

    let host_config = HostConfig {
        identity_file: Some(config.key_path.clone()),
        ..Default::default()
    };

    // Small pool to trigger exhaustion
    for max_connections in [2, 5, 10] {
        group.bench_with_input(
            BenchmarkId::new("concurrent_requests", max_connections),
            &max_connections,
            |b, &max| {
                let pool_config = PoolConfig::new()
                    .max_connections_per_host(max)
                    .max_total_connections(max * 2)
                    .enable_health_checks(false);

                let pool = Arc::new(RusshConnectionPool::with_config(
                    ConnectionConfig::default(),
                    pool_config,
                ));

                let host = config.host.clone();
                let port = config.port;
                let user = config.user.clone();
                let hc = Some(host_config.clone());

                // Requests exceeding pool capacity
                let num_requests = max * 3;

                b.to_async(&rt).iter(|| {
                    let p = Arc::clone(&pool);
                    let h = host.clone();
                    let u = user.clone();
                    let hconf = hc.clone();

                    async move {
                        let mut handles = Vec::with_capacity(num_requests);

                        for _ in 0..num_requests {
                            let pool = Arc::clone(&p);
                            let host = h.clone();
                            let user = u.clone();
                            let hc = hconf.clone();

                            handles.push(tokio::spawn(async move {
                                let result = pool.get_or_create(&host, port, &user, hc).await;
                                // Simulate some work
                                tokio::time::sleep(Duration::from_millis(10)).await;
                                result
                            }));
                        }

                        let mut results = Vec::new();
                        for handle in handles {
                            if let Ok(r) = handle.await {
                                results.push(r.is_ok());
                            }
                        }
                        black_box(results)
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark concurrent connection scaling
fn bench_concurrent_scaling(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping concurrent scaling benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping concurrent scaling benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    use rustible::connection::config::{ConnectionConfig, HostConfig};
    use rustible::connection::russh_pool::{PoolConfig, RusshConnectionPool};

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_scaling");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(60));

    let host_config = HostConfig {
        identity_file: Some(config.key_path.clone()),
        ..Default::default()
    };

    let pool_config = PoolConfig::new()
        .max_connections_per_host(20)
        .max_total_connections(100)
        .enable_health_checks(false);

    let pool = Arc::new(RusshConnectionPool::with_config(
        ConnectionConfig::default(),
        pool_config,
    ));

    for num_concurrent in [5, 10, 20] {
        group.throughput(Throughput::Elements(num_concurrent as u64));

        group.bench_with_input(
            BenchmarkId::new("parallel_connections", num_concurrent),
            &num_concurrent,
            |b, &num| {
                let pool = Arc::clone(&pool);
                let host = config.host.clone();
                let port = config.port;
                let user = config.user.clone();
                let hc = Some(host_config.clone());

                b.to_async(&rt).iter(|| {
                    let p = Arc::clone(&pool);
                    let h = host.clone();
                    let u = user.clone();
                    let hconf = hc.clone();

                    async move {
                        let mut handles = Vec::with_capacity(num);

                        for _ in 0..num {
                            let pool = Arc::clone(&p);
                            let host = h.clone();
                            let user = u.clone();
                            let hc = hconf.clone();

                            handles.push(tokio::spawn(async move {
                                let result = pool.get_or_create(&host, port, &user, hc).await;
                                result.is_ok()
                            }));
                        }

                        let mut successes = 0;
                        for handle in handles {
                            if let Ok(true) = handle.await {
                                successes += 1;
                            }
                        }
                        black_box(successes)
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark connection warmup
fn bench_warmup(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping warmup benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping warmup benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    use rustible::connection::config::{ConnectionConfig, HostConfig};
    use rustible::connection::russh_pool::{PoolConfig, RusshConnectionPool};

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("warmup");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(60));

    let host_config = HostConfig {
        identity_file: Some(config.key_path.clone()),
        ..Default::default()
    };

    for warmup_count in [1, 3, 5] {
        group.bench_with_input(
            BenchmarkId::new("warmup_connections", warmup_count),
            &warmup_count,
            |b, &count| {
                let host = config.host.clone();
                let port = config.port;
                let user = config.user.clone();
                let hc = Some(host_config.clone());

                b.to_async(&rt).iter(|| {
                    let h = host.clone();
                    let u = user.clone();
                    let hconf = hc.clone();

                    async move {
                        let pool_config = PoolConfig::new()
                            .max_connections_per_host(10)
                            .max_total_connections(50)
                            .enable_health_checks(false);

                        let pool = Arc::new(RusshConnectionPool::with_config(
                            ConnectionConfig::default(),
                            pool_config,
                        ));

                        let hosts = vec![(h, port, u, hconf)];
                        let result = pool.warmup(&hosts, count).await;
                        black_box(result)
                    }
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

fn criterion_config() -> Criterion {
    Criterion::default()
        .significance_level(0.05)
        .sample_size(50)
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(10))
        .with_output_color(true)
}

criterion_group! {
    name = pool_creation_benches;
    config = criterion_config();
    targets = bench_pool_creation
}

criterion_group! {
    name = stats_benches;
    config = criterion_config();
    targets = bench_stats_retrieval
}

criterion_group! {
    name = key_generation_benches;
    config = criterion_config();
    targets = bench_pool_key_generation
}

criterion_group! {
    name = concurrent_access_benches;
    config = criterion_config();
    targets = bench_mock_concurrent_access
}

criterion_group! {
    name = calculation_benches;
    config = criterion_config();
    targets = bench_warmup_calculations, bench_health_check_calculations
}

criterion_group! {
    name = real_connection_benches;
    config = criterion_config();
    targets = bench_real_connection_creation
}

criterion_group! {
    name = exhaustion_benches;
    config = criterion_config();
    targets = bench_pool_exhaustion
}

criterion_group! {
    name = scaling_benches;
    config = criterion_config();
    targets = bench_concurrent_scaling
}

criterion_group! {
    name = warmup_benches;
    config = criterion_config();
    targets = bench_warmup
}

criterion_main!(
    // Mock benchmarks (always run, no SSH required)
    pool_creation_benches,
    stats_benches,
    key_generation_benches,
    concurrent_access_benches,
    calculation_benches,
    // Real SSH benchmarks (require SSH_BENCH_HOST)
    real_connection_benches,
    exhaustion_benches,
    scaling_benches,
    warmup_benches
);

#[cfg(test)]
mod tests {

    #[test]
    fn test_expand_path() {
        let expanded = expand_path("~/.ssh/id_ed25519");
        assert!(!expanded.starts_with("~"));
    }

    #[test]
    fn test_whoami() {
        let user = whoami();
        assert!(!user.is_empty());
    }
}
