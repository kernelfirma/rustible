//! Inventory scale validation benchmarks
//!
//! Validates inventory operations at HPC scale: parsing, loading,
//! host lookup, and group operations for 1K-10K hosts.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::HashMap;
use std::time::Duration;

/// Simulated inventory host
#[derive(Clone, Debug)]
struct InventoryHost {
    name: String,
    ansible_host: String,
    groups: Vec<String>,
    hostvars: HashMap<String, String>,
}

/// Simulated inventory with groups
#[derive(Clone, Debug)]
struct Inventory {
    hosts: Vec<InventoryHost>,
    groups: HashMap<String, Vec<usize>>, // group name -> host indices
    host_index: HashMap<String, usize>,  // hostname -> index
}

impl Inventory {
    fn new() -> Self {
        Self {
            hosts: Vec::new(),
            groups: HashMap::new(),
            host_index: HashMap::new(),
        }
    }

    fn add_host(&mut self, host: InventoryHost) {
        let idx = self.hosts.len();
        self.host_index.insert(host.name.clone(), idx);
        for group in &host.groups {
            self.groups.entry(group.clone()).or_default().push(idx);
        }
        self.hosts.push(host);
    }

    fn lookup(&self, name: &str) -> Option<&InventoryHost> {
        self.host_index.get(name).map(|&idx| &self.hosts[idx])
    }

    fn group_members(&self, group: &str) -> Vec<&InventoryHost> {
        self.groups
            .get(group)
            .map(|indices| indices.iter().map(|&idx| &self.hosts[idx]).collect())
            .unwrap_or_default()
    }

    fn host_count(&self) -> usize {
        self.hosts.len()
    }

    fn group_count(&self) -> usize {
        self.groups.len()
    }
}

/// Generate mock inventory at specified scale
fn generate_inventory(host_count: usize) -> Inventory {
    let mut inventory = Inventory::new();

    for i in 0..host_count {
        let rack = i / 42;
        let pos = i % 42;
        let mut hostvars = HashMap::new();
        hostvars.insert("rack".to_string(), format!("rack{:03}", rack));
        hostvars.insert("position".to_string(), format!("u{:02}", pos + 1));
        hostvars.insert("serial".to_string(), format!("SN{:08X}", i));
        hostvars.insert(
            "bmc_ip".to_string(),
            format!("172.16.{}.{}", (i / 256) % 256, i % 256),
        );
        hostvars.insert(
            "ib_ip".to_string(),
            format!("10.0.{}.{}", (i / 256) % 256, i % 256),
        );

        let mut groups = vec![format!("rack{:03}", rack), "all".to_string()];

        if i % 4 == 0 {
            groups.push("gpu_nodes".to_string());
        }
        if i % 10 == 0 {
            groups.push("login_nodes".to_string());
        }
        if i < 2 {
            groups.push("head_nodes".to_string());
        }
        groups.push("compute".to_string());

        inventory.add_host(InventoryHost {
            name: format!("node{:05}", i),
            ansible_host: format!("192.168.{}.{}", (i / 256) % 256, i % 256),
            groups,
            hostvars,
        });
    }

    inventory
}

/// Simulate parsing INI-format inventory
fn parse_ini_inventory(content: &str) -> Inventory {
    let mut inventory = Inventory::new();
    let mut current_group = String::from("ungrouped");

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_group = trimmed[1..trimmed.len() - 1].to_string();
            continue;
        }
        let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
        let name = parts[0].to_string();
        let mut hostvars = HashMap::new();
        if parts.len() > 1 {
            for kv in parts[1].split_whitespace() {
                if let Some((k, v)) = kv.split_once('=') {
                    hostvars.insert(k.to_string(), v.to_string());
                }
            }
        }
        if inventory.host_index.contains_key(&name) {
            let idx = inventory.host_index[&name];
            inventory
                .groups
                .entry(current_group.clone())
                .or_default()
                .push(idx);
            inventory.hosts[idx].groups.push(current_group.clone());
        } else {
            inventory.add_host(InventoryHost {
                name,
                ansible_host: hostvars.get("ansible_host").cloned().unwrap_or_default(),
                groups: vec![current_group.clone()],
                hostvars,
            });
        }
    }

    inventory
}

/// Generate INI-format inventory string
fn generate_ini_content(host_count: usize) -> String {
    let mut content = String::with_capacity(host_count * 100);
    content.push_str("[all]\n");
    for i in 0..host_count {
        content.push_str(&format!(
            "node{:05} ansible_host=192.168.{}.{} rack=rack{:03}\n",
            i,
            (i / 256) % 256,
            i % 256,
            i / 42
        ));
    }
    content.push_str("\n[compute]\n");
    for i in 0..host_count {
        content.push_str(&format!("node{:05}\n", i));
    }
    content.push_str("\n[gpu_nodes]\n");
    for i in (0..host_count).step_by(4) {
        content.push_str(&format!("node{:05}\n", i));
    }
    content
}

fn bench_inventory_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_generation");
    group.measurement_time(Duration::from_secs(10));

    for size in [1_000, 5_000, 10_000] {
        group.bench_with_input(BenchmarkId::new("generate", size), &size, |b, &size| {
            b.iter(|| generate_inventory(black_box(size)));
        });
    }
    group.finish();
}

fn bench_inventory_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_parsing");
    group.measurement_time(Duration::from_secs(10));

    for size in [1_000, 5_000, 10_000] {
        let content = generate_ini_content(size);
        group.bench_with_input(
            BenchmarkId::new("parse_ini", size),
            &content,
            |b, content| {
                b.iter(|| parse_ini_inventory(black_box(content)));
            },
        );
    }
    group.finish();
}

fn bench_host_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("inventory_lookup");

    for size in [1_000, 5_000, 10_000] {
        let inventory = generate_inventory(size);
        let target = format!("node{:05}", size / 2);
        group.bench_with_input(
            BenchmarkId::new("by_name", size),
            &(inventory, target),
            |b, (inv, target)| {
                b.iter(|| inv.lookup(black_box(target)));
            },
        );
    }
    group.finish();
}

fn bench_group_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("group_operations");

    for size in [1_000, 5_000, 10_000] {
        let inventory = generate_inventory(size);
        group.bench_with_input(
            BenchmarkId::new("filter_group", size),
            &inventory,
            |b, inv| {
                b.iter(|| inv.group_members(black_box("gpu_nodes")));
            },
        );
    }
    group.finish();
}

fn bench_cache_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_simulation");

    for size in [1_000, 5_000, 10_000] {
        let inventory = generate_inventory(size);
        // Simulate repeated lookups (cache hit pattern)
        let targets: Vec<String> = (0..100)
            .map(|i| format!("node{:05}", i * (size / 100)))
            .collect();
        group.bench_with_input(
            BenchmarkId::new("repeated_lookup", size),
            &(inventory, targets),
            |b, (inv, targets)| {
                b.iter(|| {
                    for target in targets {
                        black_box(inv.lookup(target));
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_inventory_generation,
    bench_inventory_parsing,
    bench_host_lookup,
    bench_group_operations,
    bench_cache_simulation,
);
criterion_main!(benches);
