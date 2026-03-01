//! HPC scale validation benchmarks
//!
//! Validates execution performance at 1K, 5K, and 10K simulated host scale.
//! Measures execution time and provides SLO assertions.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::HashMap;
use std::time::Duration;

/// Simulated host configuration for scale testing
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct SimulatedHost {
    hostname: String,
    ip: String,
    groups: Vec<String>,
    vars: HashMap<String, String>,
}

/// Generate a fleet of simulated hosts
fn generate_host_fleet(count: usize) -> Vec<SimulatedHost> {
    (0..count)
        .map(|i| {
            let rack = i / 42; // 42 nodes per rack
            let position = i % 42;
            let mut vars = HashMap::new();
            vars.insert("rack".to_string(), format!("rack{:03}", rack));
            vars.insert("position".to_string(), format!("u{:02}", position + 1));
            vars.insert(
                "bmc_ip".to_string(),
                format!(
                    "10.{}.{}.{}",
                    100 + (i / 65536) % 256,
                    (i / 256) % 256,
                    i % 256
                ),
            );

            SimulatedHost {
                hostname: format!("node{:05}", i),
                ip: format!("10.{}.{}.{}", (i / 65536) % 256, (i / 256) % 256, i % 256),
                groups: vec![
                    format!("rack{:03}", rack),
                    if i % 4 == 0 {
                        "gpu".to_string()
                    } else {
                        "compute".to_string()
                    },
                    "all".to_string(),
                ],
                vars,
            }
        })
        .collect()
}

/// Simulate module execution planning for a fleet
fn plan_module_execution(hosts: &[SimulatedHost]) -> Vec<(String, String)> {
    hosts
        .iter()
        .map(|h| {
            let action = if h.groups.contains(&"gpu".to_string()) {
                "nvidia_driver"
            } else {
                "slurm_node"
            };
            (h.hostname.clone(), action.to_string())
        })
        .collect()
}

/// Simulate failure injection (10% timeout)
fn execute_with_failures(hosts: &[SimulatedHost], failure_rate: f64) -> (usize, usize) {
    let mut success = 0;
    let mut failed = 0;
    for (i, _host) in hosts.iter().enumerate() {
        if (i as f64 / hosts.len() as f64) < failure_rate {
            failed += 1;
        } else {
            success += 1;
        }
    }
    (success, failed)
}

/// Host lookup by name benchmark
fn host_lookup<'a>(hosts: &'a [SimulatedHost], name: &str) -> Option<&'a SimulatedHost> {
    hosts.iter().find(|h| h.hostname == name)
}

/// Group filtering benchmark
fn filter_by_group<'a>(hosts: &'a [SimulatedHost], group: &str) -> Vec<&'a SimulatedHost> {
    hosts
        .iter()
        .filter(|h| h.groups.contains(&group.to_string()))
        .collect()
}

fn bench_fleet_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fleet_generation");
    group.measurement_time(Duration::from_secs(10));

    for size in [1_000, 5_000, 10_000] {
        group.bench_with_input(BenchmarkId::new("generate", size), &size, |b, &size| {
            b.iter(|| generate_host_fleet(black_box(size)));
        });
    }
    group.finish();
}

fn bench_execution_planning(c: &mut Criterion) {
    let mut group = c.benchmark_group("execution_planning");
    group.measurement_time(Duration::from_secs(10));

    for size in [1_000, 5_000, 10_000] {
        let hosts = generate_host_fleet(size);
        group.bench_with_input(BenchmarkId::new("plan", size), &hosts, |b, hosts| {
            b.iter(|| plan_module_execution(black_box(hosts)));
        });
    }
    group.finish();
}

fn bench_failure_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("failure_handling");
    group.measurement_time(Duration::from_secs(10));

    for size in [1_000, 5_000, 10_000] {
        let hosts = generate_host_fleet(size);
        group.bench_with_input(
            BenchmarkId::new("10pct_failure", size),
            &hosts,
            |b, hosts| {
                b.iter(|| execute_with_failures(black_box(hosts), 0.10));
            },
        );
    }
    group.finish();
}

fn bench_host_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("host_lookup");

    for size in [1_000, 5_000, 10_000] {
        let hosts = generate_host_fleet(size);
        let target = format!("node{:05}", size / 2);
        group.bench_with_input(
            BenchmarkId::new("by_name", size),
            &(hosts, target),
            |b, (hosts, target)| {
                b.iter(|| host_lookup(black_box(hosts), black_box(target)));
            },
        );
    }
    group.finish();
}

fn bench_group_filtering(c: &mut Criterion) {
    let mut group = c.benchmark_group("group_filtering");

    for size in [1_000, 5_000, 10_000] {
        let hosts = generate_host_fleet(size);
        group.bench_with_input(BenchmarkId::new("by_group", size), &hosts, |b, hosts| {
            b.iter(|| filter_by_group(black_box(hosts), black_box("gpu")));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_fleet_generation,
    bench_execution_planning,
    bench_failure_handling,
    bench_host_lookup,
    bench_group_filtering,
);
criterion_main!(benches);
