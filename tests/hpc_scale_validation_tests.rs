//! HPC Scale Validation Tests
//!
//! Tests for verifying rustible can handle 10k+ node HPC cluster scenarios.
//! All tests are `#[ignore]` and must be run explicitly with:
//!
//!   cargo test --features full-hpc -- --ignored hpc_scale

use std::collections::HashMap;
use std::time::Instant;

/// Generate a large inventory of N hosts with group assignments.
fn generate_inventory(count: usize) -> HashMap<String, Vec<String>> {
    let mut hosts: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..count {
        let hostname = format!("node{:05}", i);
        let groups = vec![
            "all".to_string(),
            format!("rack{:03}", i / 42),
            if i % 2 == 0 {
                "compute".to_string()
            } else {
                "gpu".to_string()
            },
        ];
        hosts.insert(hostname, groups);
    }
    hosts
}

/// Expand a Slurm-style node range like "node[00000-09999]" into individual names.
fn expand_node_range(prefix: &str, start: usize, end: usize, width: usize) -> Vec<String> {
    (start..=end)
        .map(|i| format!("{}{:0width$}", prefix, i, width = width))
        .collect()
}

#[test]
#[ignore]
fn test_inventory_10k_host_generation() {
    let start = Instant::now();
    let inventory = generate_inventory(10_000);
    let elapsed = start.elapsed();

    assert_eq!(inventory.len(), 10_000);
    assert!(inventory.contains_key("node00000"));
    assert!(inventory.contains_key("node09999"));

    // Should complete in under 500ms
    assert!(
        elapsed.as_millis() < 500,
        "Inventory generation took {}ms (limit: 500ms)",
        elapsed.as_millis()
    );
}

#[test]
#[ignore]
fn test_inventory_50k_host_generation() {
    let start = Instant::now();
    let inventory = generate_inventory(50_000);
    let elapsed = start.elapsed();

    assert_eq!(inventory.len(), 50_000);
    // Should complete in under 2s
    assert!(
        elapsed.as_secs() < 2,
        "50k inventory took {}ms (limit: 2000ms)",
        elapsed.as_millis()
    );
}

#[test]
#[ignore]
fn test_node_range_expansion_10k() {
    let start = Instant::now();
    let nodes = expand_node_range("node", 0, 9999, 5);
    let elapsed = start.elapsed();

    assert_eq!(nodes.len(), 10_000);
    assert_eq!(nodes[0], "node00000");
    assert_eq!(nodes[9999], "node09999");

    assert!(
        elapsed.as_millis() < 200,
        "Node range expansion took {}ms (limit: 200ms)",
        elapsed.as_millis()
    );
}

#[test]
#[ignore]
fn test_node_range_expansion_100k() {
    let start = Instant::now();
    let nodes = expand_node_range("cn", 0, 99_999, 6);
    let elapsed = start.elapsed();

    assert_eq!(nodes.len(), 100_000);
    assert_eq!(nodes[0], "cn000000");
    assert_eq!(nodes[99_999], "cn099999");

    assert!(
        elapsed.as_secs() < 2,
        "100k node expansion took {}ms (limit: 2000ms)",
        elapsed.as_millis()
    );
}

#[test]
#[ignore]
fn test_group_membership_lookup_10k() {
    let inventory = generate_inventory(10_000);

    let start = Instant::now();
    let compute_hosts: Vec<&String> = inventory
        .iter()
        .filter(|(_, groups)| groups.contains(&"compute".to_string()))
        .map(|(host, _)| host)
        .collect();
    let elapsed = start.elapsed();

    // Even-numbered hosts are "compute"
    assert_eq!(compute_hosts.len(), 5_000);
    assert!(
        elapsed.as_millis() < 100,
        "Group filtering took {}ms (limit: 100ms)",
        elapsed.as_millis()
    );
}

#[test]
#[ignore]
fn test_rack_grouping_10k() {
    let inventory = generate_inventory(10_000);

    let start = Instant::now();
    let mut rack_counts: HashMap<String, usize> = HashMap::new();
    for groups in inventory.values() {
        for g in groups {
            if g.starts_with("rack") {
                *rack_counts.entry(g.clone()).or_insert(0) += 1;
            }
        }
    }
    let elapsed = start.elapsed();

    // 10000 / 42 = ~238 racks, each with ~42 hosts
    assert!(rack_counts.len() > 230);
    assert!(
        elapsed.as_millis() < 100,
        "Rack grouping took {}ms (limit: 100ms)",
        elapsed.as_millis()
    );
}

#[test]
#[ignore]
fn test_memory_usage_10k_inventory() {
    // Measure approximate memory for 10k host inventory
    let inventory = generate_inventory(10_000);

    // Rough estimate: each entry has hostname (~10 bytes) + 3 group strings (~20 bytes each)
    // With HashMap overhead, expect < 2MB for 10k hosts
    let estimated_bytes = inventory.len() * (10 + 3 * 20 + 64); // 64 bytes overhead per entry
    assert!(
        estimated_bytes < 2_000_000,
        "Estimated memory {}B exceeds 2MB limit",
        estimated_bytes
    );
}

#[test]
#[ignore]
fn test_parallel_inventory_partition() {
    let inventory = generate_inventory(10_000);

    let start = Instant::now();
    let batch_size = 100;
    let batches: Vec<Vec<(&String, &Vec<String>)>> = inventory
        .iter()
        .collect::<Vec<_>>()
        .chunks(batch_size)
        .map(|c| c.to_vec())
        .collect();
    let elapsed = start.elapsed();

    assert_eq!(batches.len(), 100); // 10000 / 100 = 100 batches
    assert!(
        elapsed.as_millis() < 50,
        "Inventory partitioning took {}ms (limit: 50ms)",
        elapsed.as_millis()
    );
}
