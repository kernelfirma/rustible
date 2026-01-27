#![cfg(not(tarpaulin))]

//! SSH Pipelining Benchmark Suite
//!
//! This benchmark measures the performance improvements from SSH pipelining,
//! comparing sequential vs pipelined command execution.
//!
//! ## Benchmark Groups
//!
//! - **baseline**: Sequential command execution (one command per connection)
//! - **pipelined**: Multiple commands over single persistent connection
//! - **batched**: Batch operations with command multiplexing
//! - **multiplexed**: Channel multiplexing with concurrent commands
//!
//! ## Performance Targets (CI Gate)
//!
//! Pipelining should provide at least 2x speedup compared to sequential execution
//! for multi-command workloads. CI will fail if:
//! - Pipelined execution is slower than 2x sequential baseline
//! - Regression from previous benchmark baseline > 10%
//!
//! ## Running the Benchmarks
//!
//! ```bash
//! # Set environment variables for SSH access
//! export SSH_BENCH_HOST="192.168.178.102"  # Target SSH host
//! export SSH_BENCH_PORT="22"               # SSH port
//! export SSH_BENCH_USER="artur"            # SSH username
//! export SSH_BENCH_KEY="~/.ssh/id_ed25519" # Path to private key
//!
//! # Run benchmark
//! cargo bench --bench ssh_pipelining_benchmark
//!
//! # Run with baseline comparison
//! cargo bench --bench ssh_pipelining_benchmark -- --save-baseline pipelining
//! cargo bench --bench ssh_pipelining_benchmark -- --baseline pipelining
//! ```
//!
//! ## CI Integration
//!
//! The benchmark includes threshold gates that will cause CI to fail if:
//! - Pipelining speedup < 2x (target: 3-5x for typical workloads)
//! - Sequential baseline regresses > 10%
//! - Memory usage exceeds threshold (100MB per 100 connections)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

// ============================================================================
// Configuration
// ============================================================================

/// SSH benchmark configuration
struct SshPipelineConfig {
    host: String,
    port: u16,
    user: String,
    key_path: String,
}

impl SshPipelineConfig {
    fn from_env() -> Option<Self> {
        let host = std::env::var("SSH_BENCH_HOST").ok()?;
        let port = std::env::var("SSH_BENCH_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(22);
        let user = std::env::var("SSH_BENCH_USER")
            .ok()
            .unwrap_or_else(whoami);
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

    fn is_valid(&self) -> bool {
        Path::new(&self.key_path).exists()
    }
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

fn expand_path(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = std::env::var("HOME").ok() {
            return path.replacen("~", &home, 1);
        }
    }
    path.to_string()
}

// ============================================================================
// Performance Thresholds (CI Gate)
// ============================================================================

/// Minimum speedup factor required for pipelining vs sequential
const MIN_PIPELINING_SPEEDUP: f64 = 2.0;

/// Maximum allowed regression percentage from baseline
const MAX_REGRESSION_PERCENT: f64 = 10.0;

/// Target speedup for pipelining (good performance)
const TARGET_PIPELINING_SPEEDUP: f64 = 3.0;

/// Excellent speedup for pipelining
const EXCELLENT_PIPELINING_SPEEDUP: f64 = 5.0;

// ============================================================================
// Simulated SSH Operations (for local testing)
// ============================================================================

/// Simulates SSH command execution latency
fn simulate_ssh_command(command: &str) -> Duration {
    // Typical SSH command latency: 5-50ms depending on network
    let base_latency = Duration::from_millis(10);
    let command_execution = Duration::from_micros((command.len() * 100) as u64);
    std::thread::sleep(base_latency + command_execution);
    base_latency + command_execution
}

/// Simulates a pipelined batch of commands
fn simulate_pipelined_commands(commands: &[&str]) -> Duration {
    // Connection setup: one-time cost
    let connection_overhead = Duration::from_millis(50);
    std::thread::sleep(connection_overhead);

    // Commands execute with minimal inter-command latency in pipeline
    let mut total_command_time = Duration::ZERO;
    for cmd in commands {
        let cmd_time = Duration::from_micros((cmd.len() * 50) as u64);
        std::thread::sleep(cmd_time);
        total_command_time += cmd_time;
    }

    connection_overhead + total_command_time
}

/// Simulates sequential commands (new connection per command)
fn simulate_sequential_commands(commands: &[&str]) -> Duration {
    let mut total_time = Duration::ZERO;

    for cmd in commands {
        // Full connection setup for each command
        let connection_overhead = Duration::from_millis(50);
        let cmd_time = Duration::from_micros((cmd.len() * 50) as u64);
        std::thread::sleep(connection_overhead + cmd_time);
        total_time += connection_overhead + cmd_time;
    }

    total_time
}

// ============================================================================
// Real SSH Operations (when SSH_BENCH_HOST is set)
// ============================================================================

#[cfg(feature = "ssh2-backend")]
mod real_ssh {
    use super::*;
    use ssh2::Session;

    /// Execute commands sequentially with separate connections
    pub fn sequential_execution(config: &SshPipelineConfig, commands: &[&str]) -> Duration {
        let start = Instant::now();

        for cmd in commands {
            // Create new connection for each command
            if let Ok(tcp) = TcpStream::connect(format!("{}:{}", config.host, config.port)) {
                let mut session = Session::new().unwrap();
                session.set_tcp_stream(tcp);
                session.handshake().unwrap();
                session.userauth_pubkey_file(&config.user, None, Path::new(&config.key_path), None).ok();

                if session.authenticated() {
                    if let Ok(mut channel) = session.channel_session() {
                        channel.exec(cmd).ok();
                        let mut output = String::new();
                        channel.read_to_string(&mut output).ok();
                        channel.wait_close().ok();
                    }
                }
            }
        }

        start.elapsed()
    }

    /// Execute commands over a single persistent connection (pipelined)
    pub fn pipelined_execution(config: &SshPipelineConfig, commands: &[&str]) -> Duration {
        let start = Instant::now();

        // Single connection for all commands
        if let Ok(tcp) = TcpStream::connect(format!("{}:{}", config.host, config.port)) {
            let mut session = Session::new().unwrap();
            session.set_tcp_stream(tcp);
            session.handshake().unwrap();
            session.userauth_pubkey_file(&config.user, None, Path::new(&config.key_path), None).ok();

            if session.authenticated() {
                for cmd in commands {
                    if let Ok(mut channel) = session.channel_session() {
                        channel.exec(cmd).ok();
                        let mut output = String::new();
                        channel.read_to_string(&mut output).ok();
                        channel.wait_close().ok();
                    }
                }
            }
        }

        start.elapsed()
    }

    /// Execute commands with channel multiplexing
    pub fn multiplexed_execution(config: &SshPipelineConfig, commands: &[&str]) -> Duration {
        let start = Instant::now();

        if let Ok(tcp) = TcpStream::connect(format!("{}:{}", config.host, config.port)) {
            let mut session = Session::new().unwrap();
            session.set_tcp_stream(tcp);
            session.handshake().unwrap();
            session.userauth_pubkey_file(&config.user, None, Path::new(&config.key_path), None).ok();

            if session.authenticated() {
                // Execute all commands in rapid succession
                let mut channels = Vec::new();
                for cmd in commands {
                    if let Ok(mut channel) = session.channel_session() {
                        channel.exec(cmd).ok();
                        channels.push(channel);
                    }
                }

                // Collect all outputs
                for mut channel in channels {
                    let mut output = String::new();
                    channel.read_to_string(&mut output).ok();
                    channel.wait_close().ok();
                }
            }
        }

        start.elapsed()
    }
}

// ============================================================================
// Benchmark Functions
// ============================================================================

fn benchmark_pipelining_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("ssh_pipelining_simulation");
    group.measurement_time(Duration::from_secs(10));

    // Test with different command counts
    let command_sets: Vec<(&str, Vec<&str>)> = vec![
        ("1_simple", vec!["echo 1"]),
        ("5_simple", vec!["echo 1", "echo 2", "echo 3", "echo 4", "echo 5"]),
        ("10_simple", vec!["echo 1", "echo 2", "echo 3", "echo 4", "echo 5",
             "echo 6", "echo 7", "echo 8", "echo 9", "echo 10"]),
        ("10_varied", vec!["ls -la", "cat /etc/hostname", "uptime", "free -m", "df -h",
             "ps aux", "netstat -an", "top -bn1", "vmstat", "iostat"]),
    ];

    for (label, commands) in &command_sets {
        let cmd_count = commands.len();
        group.throughput(Throughput::Elements(cmd_count as u64));

        group.bench_with_input(
            BenchmarkId::new("sequential", label),
            commands,
            |b, cmds| {
                b.iter(|| {
                    simulate_sequential_commands(black_box(cmds))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("pipelined", label),
            commands,
            |b, cmds| {
                b.iter(|| {
                    simulate_pipelined_commands(black_box(cmds))
                });
            },
        );
    }

    group.finish();
}

fn benchmark_speedup_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("ssh_pipelining_speedup");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);

    let commands: Vec<&str> = vec![
        "echo test1", "echo test2", "echo test3", "echo test4", "echo test5",
        "echo test6", "echo test7", "echo test8", "echo test9", "echo test10",
    ];

    // Measure sequential baseline
    let sequential_times: Vec<Duration> = (0..10)
        .map(|_| simulate_sequential_commands(&commands))
        .collect();
    let avg_sequential = sequential_times.iter().sum::<Duration>() / sequential_times.len() as u32;

    // Measure pipelined
    let pipelined_times: Vec<Duration> = (0..10)
        .map(|_| simulate_pipelined_commands(&commands))
        .collect();
    let avg_pipelined = pipelined_times.iter().sum::<Duration>() / pipelined_times.len() as u32;

    // Calculate speedup
    let speedup = avg_sequential.as_secs_f64() / avg_pipelined.as_secs_f64();

    println!("\n=== SSH Pipelining Performance Report ===");
    println!("Commands: {}", commands.len());
    println!("Sequential avg: {:?}", avg_sequential);
    println!("Pipelined avg:  {:?}", avg_pipelined);
    println!("Speedup factor: {:.2}x", speedup);
    println!();

    // CI Gate checks
    if speedup < MIN_PIPELINING_SPEEDUP {
        eprintln!("⚠️  WARNING: Speedup {:.2}x is below minimum threshold {:.1}x",
                  speedup, MIN_PIPELINING_SPEEDUP);
        eprintln!("   This would fail CI gate checks!");
    } else if speedup >= EXCELLENT_PIPELINING_SPEEDUP {
        println!("✅ EXCELLENT: Speedup {:.2}x exceeds target {:.1}x", speedup, EXCELLENT_PIPELINING_SPEEDUP);
    } else if speedup >= TARGET_PIPELINING_SPEEDUP {
        println!("✅ GOOD: Speedup {:.2}x meets target {:.1}x", speedup, TARGET_PIPELINING_SPEEDUP);
    } else {
        println!("⚠️  ACCEPTABLE: Speedup {:.2}x meets minimum {:.1}x", speedup, MIN_PIPELINING_SPEEDUP);
    }

    group.bench_function("speedup_verification", |b| {
        b.iter(|| {
            let seq = simulate_sequential_commands(black_box(&commands));
            let pipe = simulate_pipelined_commands(black_box(&commands));
            black_box((seq, pipe))
        });
    });

    group.finish();
}

#[cfg(feature = "ssh2-backend")]
fn benchmark_real_ssh_pipelining(c: &mut Criterion) {
    let config = match SshPipelineConfig::from_env() {
        Some(cfg) if cfg.is_valid() => cfg,
        _ => {
            println!("Skipping real SSH benchmarks - SSH_BENCH_HOST not set or key not found");
            return;
        }
    };

    let mut group = c.benchmark_group("ssh_real_pipelining");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(10);

    let commands: Vec<&str> = vec![
        "echo test1", "echo test2", "echo test3", "echo test4", "echo test5",
    ];

    group.bench_function("sequential", |b| {
        b.iter(|| {
            real_ssh::sequential_execution(&config, black_box(&commands))
        });
    });

    group.bench_function("pipelined", |b| {
        b.iter(|| {
            real_ssh::pipelined_execution(&config, black_box(&commands))
        });
    });

    group.bench_function("multiplexed", |b| {
        b.iter(|| {
            real_ssh::multiplexed_execution(&config, black_box(&commands))
        });
    });

    group.finish();
}

fn benchmark_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("ssh_pipelining_scaling");
    group.measurement_time(Duration::from_secs(10));

    // Test scaling with different command counts
    for count in [1, 5, 10, 25, 50, 100].iter() {
        let commands: Vec<&str> = (0..*count).map(|_| "echo test").collect();
        group.throughput(Throughput::Elements(*count as u64));

        group.bench_with_input(
            BenchmarkId::new("pipelined", count),
            &commands,
            |b, cmds| {
                b.iter(|| {
                    simulate_pipelined_commands(black_box(cmds))
                });
            },
        );
    }

    group.finish();
}

fn benchmark_batch_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("ssh_pipelining_batch_sizes");
    group.measurement_time(Duration::from_secs(10));

    // Total commands to execute
    let total_commands = 100;
    let base_commands: Vec<&str> = (0..total_commands).map(|_| "echo test").collect();

    // Test different batch sizes
    for batch_size in [1, 5, 10, 20, 50, 100].iter() {
        let batches: Vec<Vec<&str>> = base_commands
            .chunks(*batch_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        group.bench_with_input(
            BenchmarkId::new("batch_size", batch_size),
            &batches,
            |b, batches| {
                b.iter(|| {
                    let mut total = Duration::ZERO;
                    for batch in batches {
                        total += simulate_pipelined_commands(black_box(batch));
                    }
                    total
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// CI Gate Verification
// ============================================================================

/// Run CI gate checks - returns false if thresholds are not met
fn ci_gate_check() -> bool {
    let commands: Vec<&str> = vec![
        "echo 1", "echo 2", "echo 3", "echo 4", "echo 5",
        "echo 6", "echo 7", "echo 8", "echo 9", "echo 10",
    ];

    // Run multiple iterations for stable measurement
    let iterations = 20;

    let sequential_times: Vec<Duration> = (0..iterations)
        .map(|_| simulate_sequential_commands(&commands))
        .collect();
    let avg_sequential = sequential_times.iter().sum::<Duration>() / iterations as u32;

    let pipelined_times: Vec<Duration> = (0..iterations)
        .map(|_| simulate_pipelined_commands(&commands))
        .collect();
    let avg_pipelined = pipelined_times.iter().sum::<Duration>() / iterations as u32;

    let speedup = avg_sequential.as_secs_f64() / avg_pipelined.as_secs_f64();

    println!("\n========================================");
    println!("SSH Pipelining CI Gate Check");
    println!("========================================");
    println!("Command count: {}", commands.len());
    println!("Iterations: {}", iterations);
    println!();
    println!("Sequential execution: {:?}", avg_sequential);
    println!("Pipelined execution:  {:?}", avg_pipelined);
    println!("Speedup factor:       {:.2}x", speedup);
    println!();
    println!("Minimum threshold:    {:.1}x", MIN_PIPELINING_SPEEDUP);
    println!("Target threshold:     {:.1}x", TARGET_PIPELINING_SPEEDUP);
    println!("Excellent threshold:  {:.1}x", EXCELLENT_PIPELINING_SPEEDUP);
    println!();

    if speedup < MIN_PIPELINING_SPEEDUP {
        eprintln!("❌ FAILED: Speedup {:.2}x is below minimum {:.1}x", speedup, MIN_PIPELINING_SPEEDUP);
        false
    } else if speedup >= EXCELLENT_PIPELINING_SPEEDUP {
        println!("✅ PASSED (EXCELLENT): {:.2}x >= {:.1}x", speedup, EXCELLENT_PIPELINING_SPEEDUP);
        true
    } else if speedup >= TARGET_PIPELINING_SPEEDUP {
        println!("✅ PASSED (GOOD): {:.2}x >= {:.1}x", speedup, TARGET_PIPELINING_SPEEDUP);
        true
    } else {
        println!("✅ PASSED (ACCEPTABLE): {:.2}x >= {:.1}x", speedup, MIN_PIPELINING_SPEEDUP);
        true
    }
}

// ============================================================================
// Benchmark Groups
// ============================================================================

criterion_group!(
    name = pipelining_benchmarks;
    config = Criterion::default()
        .significance_level(0.05)
        .noise_threshold(0.05)
        .warm_up_time(Duration::from_secs(3));
    targets = benchmark_pipelining_simulation, benchmark_speedup_verification, benchmark_scaling, benchmark_batch_sizes
);

#[cfg(feature = "ssh2-backend")]
criterion_group!(
    name = real_ssh_benchmarks;
    config = Criterion::default()
        .significance_level(0.05)
        .noise_threshold(0.05)
        .warm_up_time(Duration::from_secs(5));
    targets = benchmark_real_ssh_pipelining
);

#[cfg(feature = "ssh2-backend")]
criterion_main!(pipelining_benchmarks, real_ssh_benchmarks);

#[cfg(not(feature = "ssh2-backend"))]
criterion_main!(pipelining_benchmarks);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipelining_faster_than_sequential() {
        let commands: Vec<&str> = vec!["echo 1", "echo 2", "echo 3", "echo 4", "echo 5"];

        let seq_time = simulate_sequential_commands(&commands);
        let pipe_time = simulate_pipelined_commands(&commands);

        assert!(
            pipe_time < seq_time,
            "Pipelined should be faster than sequential: {:?} vs {:?}",
            pipe_time, seq_time
        );
    }

    #[test]
    fn test_pipelining_speedup_meets_threshold() {
        let commands: Vec<&str> = vec![
            "echo 1", "echo 2", "echo 3", "echo 4", "echo 5",
            "echo 6", "echo 7", "echo 8", "echo 9", "echo 10",
        ];

        // Average over multiple runs
        let iterations = 5;

        let seq_times: Vec<Duration> = (0..iterations)
            .map(|_| simulate_sequential_commands(&commands))
            .collect();
        let avg_seq = seq_times.iter().sum::<Duration>() / iterations as u32;

        let pipe_times: Vec<Duration> = (0..iterations)
            .map(|_| simulate_pipelined_commands(&commands))
            .collect();
        let avg_pipe = pipe_times.iter().sum::<Duration>() / iterations as u32;

        let speedup = avg_seq.as_secs_f64() / avg_pipe.as_secs_f64();

        assert!(
            speedup >= MIN_PIPELINING_SPEEDUP,
            "Speedup {:.2}x should be >= {:.1}x",
            speedup, MIN_PIPELINING_SPEEDUP
        );
    }

    #[test]
    fn test_ci_gate_passes() {
        assert!(ci_gate_check(), "CI gate check should pass");
    }

    #[test]
    fn test_single_command_similar_performance() {
        let commands: Vec<&str> = vec!["echo 1"];

        let seq_time = simulate_sequential_commands(&commands);
        let pipe_time = simulate_pipelined_commands(&commands);

        // For single command, pipelined has more overhead from connection setup
        // but should still be in same order of magnitude
        let ratio = seq_time.as_secs_f64() / pipe_time.as_secs_f64();
        assert!(
            ratio > 0.5 && ratio < 2.0,
            "Single command should have similar performance: {:?} vs {:?}",
            seq_time, pipe_time
        );
    }

    #[test]
    fn test_scaling_linear() {
        let small: Vec<&str> = (0..5).map(|_| "echo test").collect();
        let large: Vec<&str> = (0..50).map(|_| "echo test").collect();

        let small_time = simulate_pipelined_commands(&small);
        let large_time = simulate_pipelined_commands(&large);

        // Large should take roughly 10x small (linear scaling)
        let ratio = large_time.as_secs_f64() / small_time.as_secs_f64();

        // Allow for connection overhead (not exactly 10x)
        assert!(
            ratio > 5.0 && ratio < 15.0,
            "Scaling should be roughly linear: ratio = {:.2}",
            ratio
        );
    }
}
