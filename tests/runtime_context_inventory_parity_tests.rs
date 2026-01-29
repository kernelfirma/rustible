//! RuntimeContext Inventory Parity Tests
//!
//! Issue #289: RuntimeContext inventory parity with Ansible
//!
//! These tests exercise the production inventory matcher and runtime context
//! variable resolution.

use std::collections::HashSet;

use rustible::executor::runtime::RuntimeContext;
use rustible::inventory::{Group, Host, Inventory};
use serde_json::json;

fn host_set(hosts: Vec<&Host>) -> HashSet<String> {
    hosts.into_iter().map(|h| h.name().to_string()).collect()
}

fn add_host_with_group(
    inventory: &mut Inventory,
    host_name: &str,
    group_name: Option<&str>,
) {
    let mut host = Host::new(host_name);
    if let Some(group) = group_name {
        host.add_to_group(group);
    }
    inventory.add_host(host).unwrap();
}

// =============================================================================
// Basic Host/Group Resolution Tests
// =============================================================================

#[test]
fn test_all_and_star_match_all_hosts() {
    let mut inv = Inventory::new();
    add_host_with_group(&mut inv, "web1", None);
    add_host_with_group(&mut inv, "web2", None);
    add_host_with_group(&mut inv, "db1", None);

    let all = host_set(inv.get_hosts_for_pattern("all").unwrap());
    let star = host_set(inv.get_hosts_for_pattern("*").unwrap());

    assert_eq!(all.len(), 3);
    assert_eq!(all, star);
}

#[test]
fn test_group_and_host_match() {
    let mut inv = Inventory::new();
    let mut web = Group::new("web");
    web.add_host("web1");
    web.add_host("web2");
    inv.add_group(web).unwrap();
    add_host_with_group(&mut inv, "web1", Some("web"));
    add_host_with_group(&mut inv, "web2", Some("web"));
    add_host_with_group(&mut inv, "db1", None);

    let web_hosts = host_set(inv.get_hosts_for_pattern("web").unwrap());
    assert_eq!(web_hosts, HashSet::from(["web1".to_string(), "web2".to_string()]));

    let host = inv.get_hosts_for_pattern("web1").unwrap();
    assert_eq!(host.len(), 1);
    assert_eq!(host[0].name(), "web1");
}

// =============================================================================
// Pattern Matching Tests
// =============================================================================

#[test]
fn test_glob_patterns() {
    let mut inv = Inventory::new();
    add_host_with_group(&mut inv, "web1", None);
    add_host_with_group(&mut inv, "web2", None);
    add_host_with_group(&mut inv, "web10", None);
    add_host_with_group(&mut inv, "prod-web", None);
    add_host_with_group(&mut inv, "db1", None);

    let web_star = host_set(inv.get_hosts_for_pattern("web*").unwrap());
    assert_eq!(
        web_star,
        HashSet::from(["web1".to_string(), "web2".to_string(), "web10".to_string()])
    );

    let web_q = host_set(inv.get_hosts_for_pattern("web?").unwrap());
    assert_eq!(web_q, HashSet::from(["web1".to_string(), "web2".to_string()]));

    let suffix = host_set(inv.get_hosts_for_pattern("*-web").unwrap());
    assert_eq!(suffix, HashSet::from(["prod-web".to_string()]));
}

#[test]
fn test_glob_pattern_with_dots() {
    let mut inv = Inventory::new();
    add_host_with_group(&mut inv, "web.example.com", None);
    add_host_with_group(&mut inv, "db.example.com", None);

    let hosts = host_set(inv.get_hosts_for_pattern("*.example.com").unwrap());
    assert_eq!(
        hosts,
        HashSet::from(["web.example.com".to_string(), "db.example.com".to_string()])
    );
}

// =============================================================================
// Set Operations Tests
// =============================================================================

#[test]
fn test_union_intersection_and_difference() {
    let mut inv = Inventory::new();

    let mut servers = Group::new("servers");
    servers.add_host("web1");
    servers.add_host("web2");
    servers.add_host("db1");
    inv.add_group(servers).unwrap();

    let mut web = Group::new("web");
    web.add_host("web1");
    web.add_host("web2");
    inv.add_group(web).unwrap();

    add_host_with_group(&mut inv, "web1", Some("servers"));
    add_host_with_group(&mut inv, "web2", Some("servers"));
    add_host_with_group(&mut inv, "db1", Some("servers"));

    let union = host_set(inv.get_hosts_for_pattern("web:servers").unwrap());
    assert_eq!(
        union,
        HashSet::from(["web1".to_string(), "web2".to_string(), "db1".to_string()])
    );

    let intersection = host_set(inv.get_hosts_for_pattern("servers:&web").unwrap());
    assert_eq!(
        intersection,
        HashSet::from(["web1".to_string(), "web2".to_string()])
    );

    let difference = host_set(inv.get_hosts_for_pattern("servers:!web").unwrap());
    assert_eq!(difference, HashSet::from(["db1".to_string()]));
}

#[test]
fn test_negation_with_wildcards() {
    let mut inv = Inventory::new();
    add_host_with_group(&mut inv, "web1", None);
    add_host_with_group(&mut inv, "web2", None);
    add_host_with_group(&mut inv, "db1", None);

    let hosts = host_set(inv.get_hosts_for_pattern("all:!web*").unwrap());
    assert_eq!(hosts, HashSet::from(["db1".to_string()]));
}

// =============================================================================
// Group Hierarchy Tests
// =============================================================================

#[test]
fn test_group_hierarchy_includes_children() {
    let mut inv = Inventory::new();

    let mut web = Group::new("web");
    web.add_host("web1");
    inv.add_group(web).unwrap();

    let mut prod = Group::new("production");
    prod.add_child("web");
    inv.add_group(prod).unwrap();

    add_host_with_group(&mut inv, "web1", Some("web"));

    let hosts = host_set(inv.get_hosts_for_pattern("production").unwrap());
    assert_eq!(hosts, HashSet::from(["web1".to_string()]));
}

// =============================================================================
// Variable Resolution Tests
// =============================================================================

#[test]
fn test_host_vars_override_group_vars() {
    let mut inv = Inventory::new();

    let mut web = Group::new("web");
    web.add_host("web1");
    web.set_var("port", serde_yaml::to_value(80).unwrap());
    inv.add_group(web).unwrap();

    let mut host = Host::new("web1");
    host.add_to_group("web");
    host.set_var("port", serde_yaml::to_value(8080).unwrap());
    inv.add_host(host).unwrap();

    let runtime = RuntimeContext::from_inventory(&inv);
    let vars = runtime.get_merged_vars("web1");

    assert_eq!(vars.get("port"), Some(&json!(8080)));
}

#[test]
fn test_multiple_group_vars_merged() {
    let mut inv = Inventory::new();

    let mut web = Group::new("web");
    web.add_host("web1");
    web.set_var("http_port", serde_yaml::to_value(80).unwrap());
    inv.add_group(web).unwrap();

    let mut secure = Group::new("secure");
    secure.add_host("web1");
    secure.set_var("https_port", serde_yaml::to_value(443).unwrap());
    inv.add_group(secure).unwrap();

    let mut host = Host::new("web1");
    host.add_to_group("web");
    host.add_to_group("secure");
    inv.add_host(host).unwrap();

    let runtime = RuntimeContext::from_inventory(&inv);
    let vars = runtime.get_merged_vars("web1");

    assert_eq!(vars.get("http_port"), Some(&json!(80)));
    assert_eq!(vars.get("https_port"), Some(&json!(443)));
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_nonexistent_pattern_errors() {
    let mut inv = Inventory::new();
    add_host_with_group(&mut inv, "host1", None);

    assert!(inv.get_hosts_for_pattern("nonexistent").is_err());
}
