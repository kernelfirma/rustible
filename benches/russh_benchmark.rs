#![cfg(not(tarpaulin))]

//! SSH Library Comparison Benchmark (ssh2 vs russh)
//!
//! This benchmark suite compares the performance of ssh2 (libssh2) and russh
//! for various SSH operations.
//!
//! # Benchmark Groups
//!
//! - **connection**: Connection establishment overhead
//! - **execution**: Command running performance
//! - **transfer**: File upload/download operations (1KB, 1MB)
//! - **parallel**: Concurrent operations (10, 50, 100 connections)
//! - **multiplex**: Channel multiplexing (multiple commands on same connection)
//!
//! # Homelab Hosts
//!

//! The benchmark can test against multiple homelab hosts:
//! - svr-core: 192.168.178.102
//! - svr-host: 192.168.178.88
//! - svr-nas: 192.168.178.101
//!
//! # Running the Benchmarks
//!
//! These benchmarks require real SSH access to a test server.
//! Set the following environment variables before running:
//!
//! ```bash
//! export SSH_BENCH_HOST="192.168.178.102"  # Target SSH host (svr-core)
//! export SSH_BENCH_PORT="22"               # SSH port (default: 22)
//! export SSH_BENCH_USER="artur"            # SSH username
//! export SSH_BENCH_KEY="~/.ssh/id_ed25519" # Path to private key
//! cargo bench --bench russh_benchmark
//! ```
//!
//! # CI Considerations
//!
//! The benchmark will skip all tests if SSH_BENCH_HOST is not set, making it
//! safe to include in CI pipelines. Each benchmark group checks for the
//! environment variable and gracefully exits with a message if not configured.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;
use tokio::runtime::Runtime;

// ============================================================================
// Configuration and Environment
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
    if path.starts_with("~/") {
        if let Some(home) = std::env::var("HOME").ok() {
            return format!("{}/{}", home, &path[2..]);
        }
    }
    path.to_string()
}

/// Generate random test data of specified size
fn generate_test_data(size_bytes: usize) -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Use a deterministic pseudo-random generator for reproducibility
    let mut data = Vec::with_capacity(size_bytes);
    let mut hasher = DefaultHasher::new();
    for i in 0..size_bytes {
        i.hash(&mut hasher);
        data.push((hasher.finish() & 0xFF) as u8);
    }
    data
}

// ============================================================================
// SSH2 Benchmark Functions
// ============================================================================

/// Establish an ssh2 connection
fn ssh2_connect(config: &SshBenchConfig) -> ssh2::Session {
    let tcp = TcpStream::connect((&config.host[..], config.port)).unwrap();
    let mut session = ssh2::Session::new().unwrap();
    session.set_tcp_stream(tcp);
    session.handshake().unwrap();
    session
        .userauth_pubkey_file(&config.user, None, Path::new(&config.key_path), None)
        .unwrap();
    session
}

/// Execute a single command with ssh2
fn ssh2_execute(session: &ssh2::Session, command: &str) -> String {
    let mut channel = session.channel_session().unwrap();
    channel.exec(command).unwrap();
    let mut output = String::new();
    channel.read_to_string(&mut output).unwrap();
    channel.wait_close().unwrap();
    output
}

/// Upload data via ssh2 SFTP
fn ssh2_upload(session: &ssh2::Session, data: &[u8], remote_path: &str) {
    let sftp = session.sftp().unwrap();
    let mut remote_file = sftp.create(Path::new(remote_path)).unwrap();
    remote_file.write_all(data).unwrap();
}

/// Download data via ssh2 SFTP
fn ssh2_download(session: &ssh2::Session, remote_path: &str) -> Vec<u8> {
    let sftp = session.sftp().unwrap();
    let mut remote_file = sftp.open(Path::new(remote_path)).unwrap();
    let mut buffer = Vec::new();
    remote_file.read_to_end(&mut buffer).unwrap();
    buffer
}

// ============================================================================
// Russh Benchmark Functions (using async-ssh2-tokio wrapper)
// ============================================================================

/// Establish a russh connection using async-ssh2-tokio
async fn russh_connect(config: &SshBenchConfig) -> async_ssh2_tokio::client::Client {
    use async_ssh2_tokio::client::{AuthMethod, Client, ServerCheckMethod};

    let auth = AuthMethod::with_key_file(&config.key_path, None);
    Client::connect(
        (&config.host[..], config.port),
        &config.user,
        auth,
        ServerCheckMethod::NoCheck,
    )
    .await
    .unwrap()
}

/// Execute a single command with russh
async fn russh_execute(client: &async_ssh2_tokio::client::Client, command: &str) -> String {
    let result = client.execute(command).await.unwrap();
    result.stdout
}

/// Upload data via russh (using base64 encoding over command)
/// Note: async-ssh2-tokio doesn't expose SFTP directly, so we use command-based transfer
async fn russh_upload(client: &async_ssh2_tokio::client::Client, data: &[u8], remote_path: &str) {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    let cmd = format!("echo '{}' | base64 -d > {}", encoded, remote_path);
    let _ = client.execute(&cmd).await.unwrap();
}

/// Download data via russh (using base64 encoding over command)
async fn russh_download(client: &async_ssh2_tokio::client::Client, remote_path: &str) -> Vec<u8> {
    use base64::Engine;
    let cmd = format!("base64 < {}", remote_path);
    let result = client.execute(&cmd).await.unwrap();
    base64::engine::general_purpose::STANDARD
        .decode(result.stdout.trim())
        .unwrap_or_default()
}

// ============================================================================
// Connection Benchmarks
// ============================================================================

fn bench_connection(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping connection benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping connection benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("connection");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));

    // SSH2 connection establishment
    group.bench_function("ssh2_connect", |b| {
        b.iter(|| {
            let session = ssh2_connect(&config);
            black_box(session.authenticated());
            session.disconnect(None, "benchmark", None).ok();
        })
    });

    // Russh connection establishment
    group.bench_function("russh_connect", |b| {
        b.to_async(&rt).iter(|| async {
            let client = russh_connect(&config).await;
            black_box(client);
            // Client is dropped automatically
        })
    });

    group.finish();

    // Print comparison
    println!("\n=== Connection Benchmark Results ===");
    println!("Compare ssh2_connect vs russh_connect times in the report above.");
    println!("Lower is better. russh is async-native while ssh2 uses blocking I/O.");
}

// ============================================================================
// Command Execution Benchmarks
// ============================================================================

fn bench_execution(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping execution benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping execution benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("execution");
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(10));

    // Pre-establish connections for reuse tests
    let ssh2_session = ssh2_connect(&config);
    let russh_client = rt.block_on(russh_connect(&config));

    // Single command execution (connection reuse)
    group.bench_function("ssh2_single_command", |b| {
        b.iter(|| {
            let output = ssh2_execute(&ssh2_session, "echo hello");
            black_box(output)
        })
    });

    group.bench_function("russh_single_command", |b| {
        b.to_async(&rt).iter(|| async {
            let output = russh_execute(&russh_client, "echo hello").await;
            black_box(output)
        })
    });

    // Multiple sequential commands
    group.bench_function("ssh2_sequential_10_commands", |b| {
        b.iter(|| {
            for i in 0..10 {
                let output = ssh2_execute(&ssh2_session, &format!("echo {}", i));
                black_box(output);
            }
        })
    });

    group.bench_function("russh_sequential_10_commands", |b| {
        b.to_async(&rt).iter(|| async {
            for i in 0..10 {
                let output = russh_execute(&russh_client, &format!("echo {}", i)).await;
                black_box(output);
            }
        })
    });

    // Connect + execute (full round trip)
    group.bench_function("ssh2_connect_and_execute", |b| {
        b.iter(|| {
            let session = ssh2_connect(&config);
            let output = ssh2_execute(&session, "echo hello");
            black_box(output);
            session.disconnect(None, "benchmark", None).ok();
        })
    });

    group.bench_function("russh_connect_and_execute", |b| {
        b.to_async(&rt).iter(|| async {
            let client = russh_connect(&config).await;
            let output = russh_execute(&client, "echo hello").await;
            black_box(output);
        })
    });

    group.finish();

    // Cleanup
    ssh2_session.disconnect(None, "benchmark", None).ok();
}

// ============================================================================
// File Transfer Benchmarks
// ============================================================================

fn bench_transfer(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping transfer benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping transfer benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("transfer");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(15));

    // Pre-establish connections
    let ssh2_session = ssh2_connect(&config);
    let russh_client = rt.block_on(russh_connect(&config));

    // Test data sizes
    let sizes = [
        (1024, "1KB"),
        (1024 * 1024, "1MB"),
        // Note: 100MB tests are very slow with base64 encoding, commenting out by default
        // (100 * 1024 * 1024, "100MB"),
    ];

    for (size, label) in sizes {
        let test_data = generate_test_data(size);
        let remote_path = format!("/tmp/ssh_bench_test_{}.dat", size);

        group.throughput(Throughput::Bytes(size as u64));

        // SSH2 upload
        group.bench_with_input(
            BenchmarkId::new("ssh2_upload", label),
            &(&test_data, &remote_path),
            |b, (data, path)| {
                b.iter(|| {
                    ssh2_upload(&ssh2_session, data, path);
                })
            },
        );

        // Russh upload (base64 encoded, slower for large files)
        // Only benchmark for smaller files to avoid timeout
        if size <= 1024 * 100 {
            group.bench_with_input(
                BenchmarkId::new("russh_upload", label),
                &(&test_data, &remote_path),
                |b, (data, path)| {
                    b.to_async(&rt).iter(|| async {
                        russh_upload(&russh_client, data, path).await;
                    })
                },
            );
        }

        // Ensure file exists for download tests
        ssh2_upload(&ssh2_session, &test_data, &remote_path);

        // SSH2 download
        group.bench_with_input(
            BenchmarkId::new("ssh2_download", label),
            &remote_path,
            |b, path| {
                b.iter(|| {
                    let data = ssh2_download(&ssh2_session, path);
                    black_box(data)
                })
            },
        );

        // Russh download (base64 encoded)
        if size <= 1024 * 100 {
            group.bench_with_input(
                BenchmarkId::new("russh_download", label),
                &remote_path,
                |b, path| {
                    b.to_async(&rt).iter(|| async {
                        let data = russh_download(&russh_client, path).await;
                        black_box(data)
                    })
                },
            );
        }

        // Cleanup
        let _ = ssh2_execute(&ssh2_session, &format!("rm -f {}", remote_path));
    }

    group.finish();

    // Cleanup
    ssh2_session.disconnect(None, "benchmark", None).ok();
}

// ============================================================================
// Parallel Execution Benchmarks
// ============================================================================

fn bench_parallel(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping parallel benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping parallel benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("parallel");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(20));

    let concurrency_levels = [10, 50, 100];

    for &num_concurrent in &concurrency_levels {
        group.throughput(Throughput::Elements(num_concurrent as u64));

        // SSH2 parallel connections using spawn_blocking
        group.bench_with_input(
            BenchmarkId::new("ssh2_parallel", num_concurrent),
            &num_concurrent,
            |b, &num| {
                let config = SshBenchConfig::from_env().unwrap();
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::with_capacity(num);
                    for _ in 0..num {
                        let host = config.host.clone();
                        let port = config.port;
                        let user = config.user.clone();
                        let key_path = config.key_path.clone();

                        let handle = tokio::task::spawn_blocking(move || {
                            let tcp = TcpStream::connect((&host[..], port)).unwrap();
                            let mut session = ssh2::Session::new().unwrap();
                            session.set_tcp_stream(tcp);
                            session.handshake().unwrap();
                            session
                                .userauth_pubkey_file(&user, None, Path::new(&key_path), None)
                                .unwrap();

                            let mut channel = session.channel_session().unwrap();
                            channel.exec("echo hello").unwrap();
                            let mut output = String::new();
                            channel.read_to_string(&mut output).unwrap();
                            channel.wait_close().unwrap();

                            session.disconnect(None, "benchmark", None).ok();
                            output
                        });
                        handles.push(handle);
                    }

                    for handle in handles {
                        black_box(handle.await.unwrap());
                    }
                })
            },
        );

        // Russh parallel connections (native async)
        group.bench_with_input(
            BenchmarkId::new("russh_parallel", num_concurrent),
            &num_concurrent,
            |b, &num| {
                let config = SshBenchConfig::from_env().unwrap();
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::with_capacity(num);
                    for _ in 0..num {
                        let host = config.host.clone();
                        let port = config.port;
                        let user = config.user.clone();
                        let key_path = config.key_path.clone();

                        let handle = tokio::spawn(async move {
                            use async_ssh2_tokio::client::{AuthMethod, Client, ServerCheckMethod};

                            let auth = AuthMethod::with_key_file(&key_path, None);
                            let client = Client::connect(
                                (&host[..], port),
                                &user,
                                auth,
                                ServerCheckMethod::NoCheck,
                            )
                            .await
                            .unwrap();

                            let result = client.execute("echo hello").await.unwrap();
                            result.stdout
                        });
                        handles.push(handle);
                    }

                    for handle in handles {
                        black_box(handle.await.unwrap());
                    }
                })
            },
        );
    }

    group.finish();

    // Print comparison summary
    println!("\n=== Parallel Benchmark Results ===");
    println!("Compare ssh2_parallel vs russh_parallel for each concurrency level.");
    println!("russh uses native async, while ssh2 uses spawn_blocking.");
    println!("russh should show better scalability at higher concurrency levels.");
}

// ============================================================================
// Channel Multiplexing Benchmarks
// ============================================================================

/// Benchmark channel multiplexing - multiple commands on same connection
fn bench_multiplex(c: &mut Criterion) {
    let Some(config) = SshBenchConfig::from_env() else {
        eprintln!("Skipping multiplex benchmarks: SSH_BENCH_HOST not set");
        return;
    };

    if !config.is_valid() {
        eprintln!(
            "Skipping multiplex benchmarks: key file {} not found",
            config.key_path
        );
        return;
    }

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("multiplex");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(15));

    // Pre-establish connections
    let ssh2_session = ssh2_connect(&config);
    let russh_client = rt.block_on(russh_connect(&config));

    let command_counts = [5, 10, 20];

    for &num_commands in &command_counts {
        group.throughput(Throughput::Elements(num_commands as u64));

        // SSH2 sequential commands on same connection (channel reuse)
        group.bench_with_input(
            BenchmarkId::new("ssh2_multiplex", num_commands),
            &num_commands,
            |b, &num| {
                b.iter(|| {
                    for i in 0..num {
                        let output = ssh2_execute(&ssh2_session, &format!("echo cmd_{}", i));
                        black_box(output);
                    }
                })
            },
        );

        // Russh sequential commands on same connection
        group.bench_with_input(
            BenchmarkId::new("russh_multiplex", num_commands),
            &num_commands,
            |b, &num| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..num {
                        let output = russh_execute(&russh_client, &format!("echo cmd_{}", i)).await;
                        black_box(output);
                    }
                })
            },
        );
    }

    group.finish();

    // Print comparison
    println!("\n=== Channel Multiplexing Benchmark Results ===");
    println!("Compare ssh2_multiplex vs russh_multiplex for sequential commands.");
    println!("This measures the overhead of opening new channels on an existing connection.");

    // Cleanup
    ssh2_session.disconnect(None, "benchmark", None).ok();
}

// ============================================================================
// Mock-Based Benchmarks (No SSH Required - Always Run)
// ============================================================================
// These benchmarks measure the overhead difference between spawn_blocking
// (ssh2 pattern) and native async (russh pattern) without requiring SSH.

/// Benchmark the overhead difference between spawn_blocking and native async
fn bench_mock_overhead(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("mock_overhead");

    // Minimal spawn_blocking overhead (ssh2 pattern)
    group.bench_function("spawn_blocking_minimal", |b| {
        b.to_async(&rt).iter(|| async {
            let result = tokio::task::spawn_blocking(|| 42).await.unwrap();
            black_box(result)
        })
    });

    // Minimal native async overhead (russh pattern)
    group.bench_function("native_async_minimal", |b| {
        b.to_async(&rt).iter(|| async {
            tokio::task::yield_now().await;
            black_box(42)
        })
    });

    // spawn_blocking with simulated work
    group.bench_function("spawn_blocking_with_work", |b| {
        b.to_async(&rt).iter(|| async {
            let result = tokio::task::spawn_blocking(|| {
                let mut sum = 0u64;
                for i in 0..1000 {
                    sum = sum.wrapping_add(i);
                }
                sum
            })
            .await
            .unwrap();
            black_box(result)
        })
    });

    // Native async with equivalent work
    group.bench_function("native_async_with_work", |b| {
        b.to_async(&rt).iter(|| async {
            let mut sum = 0u64;
            for i in 0..1000 {
                sum = sum.wrapping_add(i);
            }
            black_box(sum)
        })
    });

    // spawn_blocking with simulated network latency
    for latency_us in [10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("spawn_blocking_latency_us", latency_us),
            latency_us,
            |b, &lat| {
                b.to_async(&rt).iter(|| async move {
                    tokio::task::spawn_blocking(move || {
                        std::thread::sleep(Duration::from_micros(lat));
                        42
                    })
                    .await
                    .unwrap()
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("native_async_latency_us", latency_us),
            latency_us,
            |b, &lat| {
                b.to_async(&rt).iter(|| async move {
                    tokio::time::sleep(Duration::from_micros(lat)).await;
                    42
                })
            },
        );
    }

    group.finish();
}

/// Benchmark parallel execution patterns with mocks
fn bench_mock_parallel(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("mock_parallel");
    group.sample_size(30);

    let latency_ms = 5u32;

    for num_parallel in [10, 50, 100, 200].iter() {
        group.throughput(Throughput::Elements(*num_parallel as u64));

        // ssh2 pattern: spawn_blocking for each operation
        group.bench_with_input(
            BenchmarkId::new("ssh2_pattern", num_parallel),
            num_parallel,
            |b, &num| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::with_capacity(num as usize);
                    for i in 0..num {
                        handles.push(tokio::spawn(async move {
                            tokio::task::spawn_blocking(move || {
                                std::thread::sleep(Duration::from_millis(latency_ms as u64));
                                format!("result_{}", i)
                            })
                            .await
                        }));
                    }
                    for handle in handles {
                        black_box(handle.await.unwrap()).ok();
                    }
                })
            },
        );

        // russh pattern: native async for each operation
        group.bench_with_input(
            BenchmarkId::new("russh_pattern", num_parallel),
            num_parallel,
            |b, &num| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::with_capacity(num as usize);
                    for i in 0..num {
                        handles.push(tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(latency_ms as u64)).await;
                            format!("result_{}", i)
                        }));
                    }
                    for handle in handles {
                        black_box(handle.await.unwrap());
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark concurrent connection scaling with mocks
fn bench_mock_concurrent(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("mock_concurrent");
    group.sample_size(20);

    let latency_ms = 2u32;

    for concurrent in [50, 100, 200, 500].iter() {
        group.throughput(Throughput::Elements(*concurrent as u64));

        // ssh2 pattern: limited by blocking thread pool
        group.bench_with_input(
            BenchmarkId::new("ssh2_concurrent", concurrent),
            concurrent,
            |b, &num| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::with_capacity(num as usize);
                    for _ in 0..num {
                        handles.push(tokio::spawn(async move {
                            tokio::task::spawn_blocking(move || {
                                std::thread::sleep(Duration::from_millis(latency_ms as u64));
                            })
                            .await
                        }));
                    }
                    for handle in handles {
                        black_box(handle.await.unwrap()).ok();
                    }
                })
            },
        );

        // russh pattern: no thread pool limit
        group.bench_with_input(
            BenchmarkId::new("russh_concurrent", concurrent),
            concurrent,
            |b, &num| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::with_capacity(num as usize);
                    for _ in 0..num {
                        handles.push(tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(latency_ms as u64)).await;
                        }));
                    }
                    for handle in handles {
                        handle.await.unwrap();
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark latency sensitivity with mocks
fn bench_mock_latency(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("mock_latency");
    group.sample_size(30);

    let num_operations = 10;

    // Test how each approach handles varying latencies
    for latency_ms in [0, 1, 5, 10, 25, 50].iter() {
        // ssh2 pattern
        group.bench_with_input(
            BenchmarkId::new("ssh2_latency_ms", latency_ms),
            latency_ms,
            |b, &lat| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::with_capacity(num_operations);
                    for _ in 0..num_operations {
                        handles.push(tokio::spawn(async move {
                            tokio::task::spawn_blocking(move || {
                                std::thread::sleep(Duration::from_millis(lat as u64));
                            })
                            .await
                        }));
                    }
                    for handle in handles {
                        black_box(handle.await.unwrap()).ok();
                    }
                })
            },
        );

        // russh pattern
        group.bench_with_input(
            BenchmarkId::new("russh_latency_ms", latency_ms),
            latency_ms,
            |b, &lat| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::with_capacity(num_operations);
                    for _ in 0..num_operations {
                        handles.push(tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(lat as u64)).await;
                        }));
                    }
                    for handle in handles {
                        handle.await.unwrap();
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark multi-host playbook execution simulation
fn bench_mock_playbook(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("mock_playbook");
    group.sample_size(20);

    let connection_time_ms = 20u64;
    let command_time_ms = 5u64;
    let tasks_per_host = 5;

    for num_hosts in [10, 25, 50, 100].iter() {
        group.throughput(Throughput::Elements((*num_hosts * tasks_per_host) as u64));

        // ssh2 pattern: spawn_blocking per host
        group.bench_with_input(
            BenchmarkId::new("ssh2_playbook_hosts", num_hosts),
            num_hosts,
            |b, &hosts| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::with_capacity(hosts as usize);

                    for _ in 0..hosts {
                        handles.push(tokio::spawn(async move {
                            // Connect
                            tokio::task::spawn_blocking(move || {
                                std::thread::sleep(Duration::from_millis(connection_time_ms));
                            })
                            .await
                            .unwrap();

                            // Execute tasks
                            for _ in 0..tasks_per_host {
                                tokio::task::spawn_blocking(move || {
                                    std::thread::sleep(Duration::from_millis(command_time_ms));
                                })
                                .await
                                .unwrap();
                            }
                        }));
                    }

                    for handle in handles {
                        handle.await.unwrap();
                    }
                })
            },
        );

        // russh pattern: native async per host
        group.bench_with_input(
            BenchmarkId::new("russh_playbook_hosts", num_hosts),
            num_hosts,
            |b, &hosts| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = Vec::with_capacity(hosts as usize);

                    for _ in 0..hosts {
                        handles.push(tokio::spawn(async move {
                            // Connect
                            tokio::time::sleep(Duration::from_millis(connection_time_ms)).await;

                            // Execute tasks
                            for _ in 0..tasks_per_host {
                                tokio::time::sleep(Duration::from_millis(command_time_ms)).await;
                            }
                        }));
                    }

                    for handle in handles {
                        handle.await.unwrap();
                    }
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Criterion Setup
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
    name = connection_benches;
    config = criterion_config();
    targets = bench_connection
}

criterion_group! {
    name = execution_benches;
    config = criterion_config();
    targets = bench_execution
}

criterion_group! {
    name = transfer_benches;
    config = criterion_config();
    targets = bench_transfer
}

criterion_group! {
    name = parallel_benches;
    config = criterion_config();
    targets = bench_parallel
}

criterion_group! {
    name = multiplex_benches;
    config = criterion_config();
    targets = bench_multiplex
}

criterion_group! {
    name = mock_overhead_benches;
    config = criterion_config();
    targets = bench_mock_overhead
}

criterion_group! {
    name = mock_parallel_benches;
    config = criterion_config();
    targets = bench_mock_parallel
}

criterion_group! {
    name = mock_concurrent_benches;
    config = criterion_config();
    targets = bench_mock_concurrent
}

criterion_group! {
    name = mock_latency_benches;
    config = criterion_config();
    targets = bench_mock_latency
}

criterion_group! {
    name = mock_playbook_benches;
    config = criterion_config();
    targets = bench_mock_playbook
}

criterion_main!(
    // Mock benchmarks (always run, no SSH required)
    mock_overhead_benches,
    mock_parallel_benches,
    mock_concurrent_benches,
    mock_latency_benches,
    mock_playbook_benches,
    // Real SSH benchmarks (require SSH_BENCH_HOST)
    connection_benches,
    execution_benches,
    transfer_benches,
    parallel_benches,
    multiplex_benches
);

// ============================================================================
// Additional Test Module for Unit Testing Benchmark Functions
// ============================================================================

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_generate_test_data() {
        let data = generate_test_data(1024);
        assert_eq!(data.len(), 1024);

        // Should be deterministic
        let data2 = generate_test_data(1024);
        assert_eq!(data, data2);
    }

    #[test]
    fn test_expand_path() {
        let expanded = expand_path("~/.ssh/id_ed25519");
        assert!(!expanded.starts_with("~"));

        let unchanged = expand_path("/absolute/path");
        assert_eq!(unchanged, "/absolute/path");
    }

    #[test]
    fn test_config_from_env_missing() {
        // This test verifies graceful handling when env vars are not set
        // In CI, SSH_BENCH_HOST should not be set
        std::env::remove_var("SSH_BENCH_HOST");
        let config = SshBenchConfig::from_env();
        assert!(config.is_none());
    }

    #[test]
    fn test_whoami() {
        let user = whoami();
        assert!(!user.is_empty());
    }
}
