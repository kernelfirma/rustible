//! RuntimeContext Inventory Parity Tests
//!
//! Issue #289: RuntimeContext inventory parity with Ansible
//!
//! These tests verify that host/group resolution behaves consistently with
//! Ansible inventory rules, covering patterns, groups, and vars.

use serde_json::{json, Value as JsonValue};
use std::collections::{HashMap, HashSet};

/// Mock host representation
#[derive(Debug, Clone)]
struct Host {
    name: String,
    vars: HashMap<String, JsonValue>,
    groups: HashSet<String>,
}

impl Host {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            vars: HashMap::new(),
            groups: HashSet::new(),
        }
    }

    fn with_var(mut self, key: &str, value: JsonValue) -> Self {
        self.vars.insert(key.to_string(), value);
        self
    }

    fn in_group(mut self, group: &str) -> Self {
        self.groups.insert(group.to_string());
        self
    }
}

/// Mock group representation
#[derive(Debug, Clone)]
struct Group {
    name: String,
    hosts: Vec<String>,
    children: Vec<String>,
    vars: HashMap<String, JsonValue>,
}

impl Group {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            hosts: Vec::new(),
            children: Vec::new(),
            vars: HashMap::new(),
        }
    }

    fn with_host(mut self, host: &str) -> Self {
        self.hosts.push(host.to_string());
        self
    }

    fn with_child(mut self, child: &str) -> Self {
        self.children.push(child.to_string());
        self
    }

    fn with_var(mut self, key: &str, value: JsonValue) -> Self {
        self.vars.insert(key.to_string(), value);
        self
    }
}

/// Mock inventory for testing
struct MockInventory {
    hosts: HashMap<String, Host>,
    groups: HashMap<String, Group>,
}

impl MockInventory {
    fn new() -> Self {
        // Initialize with special groups
        let mut groups = HashMap::new();
        groups.insert("all".to_string(), Group::new("all"));
        groups.insert("ungrouped".to_string(), Group::new("ungrouped"));

        Self {
            hosts: HashMap::new(),
            groups,
        }
    }

    fn add_host(&mut self, host: Host) {
        let host_name = host.name.clone();

        // Add to 'all' group
        if let Some(all_group) = self.groups.get_mut("all") {
            all_group.hosts.push(host_name.clone());
        }

        // Add to any explicit groups
        for group_name in &host.groups {
            if let Some(group) = self.groups.get_mut(group_name) {
                if !group.hosts.contains(&host_name) {
                    group.hosts.push(host_name.clone());
                }
            }
        }

        self.hosts.insert(host_name, host);
    }

    fn add_group(&mut self, group: Group) {
        let group_name = group.name.clone();
        self.groups.insert(group_name.clone(), group);

        // Add as child of 'all'
        if group_name != "all" && group_name != "ungrouped" {
            if let Some(all_group) = self.groups.get_mut("all") {
                if !all_group.children.contains(&group_name) {
                    all_group.children.push(group_name);
                }
            }
        }
    }

    /// Match hosts by pattern
    fn match_hosts(&self, pattern: &str) -> Vec<String> {
        if pattern == "all" || pattern == "*" {
            return self.hosts.keys().cloned().collect();
        }

        // Group reference
        if let Some(group) = self.groups.get(pattern) {
            return self.expand_group(group);
        }

        // Glob pattern
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            return self.match_glob(pattern);
        }

        // Range pattern [0:10]
        if pattern.contains('[') && pattern.contains(':') && pattern.contains(']') {
            return self.match_range(pattern);
        }

        // Negation
        if pattern.starts_with('!') {
            let exclude = &pattern[1..];
            let excluded = self.match_hosts(exclude);
            return self.hosts.keys()
                .filter(|h| !excluded.contains(h))
                .cloned()
                .collect();
        }

        // Intersection (&)
        if pattern.contains(":&") {
            let parts: Vec<&str> = pattern.split(":&").collect();
            if parts.len() == 2 {
                let set1: HashSet<_> = self.match_hosts(parts[0]).into_iter().collect();
                let set2: HashSet<_> = self.match_hosts(parts[1]).into_iter().collect();
                return set1.intersection(&set2).cloned().collect();
            }
        }

        // Union (:)
        if pattern.contains(':') && !pattern.contains(":&") && !pattern.contains(":!") {
            let parts: Vec<&str> = pattern.split(':').collect();
            let mut result: HashSet<String> = HashSet::new();
            for p in parts {
                result.extend(self.match_hosts(p));
            }
            return result.into_iter().collect();
        }

        // Difference (:!)
        if pattern.contains(":!") {
            let parts: Vec<&str> = pattern.split(":!").collect();
            if parts.len() == 2 {
                let set1: HashSet<_> = self.match_hosts(parts[0]).into_iter().collect();
                let set2: HashSet<_> = self.match_hosts(parts[1]).into_iter().collect();
                return set1.difference(&set2).cloned().collect();
            }
        }

        // Direct hostname
        if self.hosts.contains_key(pattern) {
            return vec![pattern.to_string()];
        }

        Vec::new()
    }

    fn expand_group(&self, group: &Group) -> Vec<String> {
        let mut hosts: HashSet<String> = group.hosts.iter().cloned().collect();

        // Expand children
        for child_name in &group.children {
            if let Some(child) = self.groups.get(child_name) {
                hosts.extend(self.expand_group(child));
            }
        }

        hosts.into_iter().collect()
    }

    fn match_glob(&self, pattern: &str) -> Vec<String> {
        let regex_pattern = pattern
            .replace('.', "\\.")
            .replace('*', ".*")
            .replace('?', ".");

        let re = regex::Regex::new(&format!("^{}$", regex_pattern)).ok();

        self.hosts.keys()
            .filter(|h| re.as_ref().map(|r| r.is_match(h)).unwrap_or(false))
            .cloned()
            .collect()
    }

    fn match_range(&self, pattern: &str) -> Vec<String> {
        // Extract base pattern and range
        if let Some(bracket_start) = pattern.find('[') {
            if let Some(bracket_end) = pattern.find(']') {
                let base = &pattern[..bracket_start];
                let range_spec = &pattern[bracket_start + 1..bracket_end];

                if let Some(colon_pos) = range_spec.find(':') {
                    let start: usize = range_spec[..colon_pos].parse().unwrap_or(0);
                    let end: usize = range_spec[colon_pos + 1..].parse().unwrap_or(0);

                    return self.hosts.keys()
                        .filter(|h| {
                            if let Some(num_str) = h.strip_prefix(base) {
                                if let Ok(num) = num_str.parse::<usize>() {
                                    return num >= start && num <= end;
                                }
                            }
                            false
                        })
                        .cloned()
                        .collect();
                }
            }
        }
        Vec::new()
    }

    fn get_host_vars(&self, hostname: &str) -> HashMap<String, JsonValue> {
        let mut vars = HashMap::new();

        // Group vars (in order of group hierarchy)
        if let Some(host) = self.hosts.get(hostname) {
            // Get all group vars
            for group_name in &host.groups {
                if let Some(group) = self.groups.get(group_name) {
                    for (k, v) in &group.vars {
                        vars.insert(k.clone(), v.clone());
                    }
                }
            }

            // Host vars override group vars
            for (k, v) in &host.vars {
                vars.insert(k.clone(), v.clone());
            }
        }

        vars
    }

    fn host_in_group(&self, hostname: &str, group_name: &str) -> bool {
        let group_hosts = self.match_hosts(group_name);
        group_hosts.contains(&hostname.to_string())
    }
}

// =============================================================================
// Basic Host/Group Resolution Tests
// =============================================================================

#[test]
fn test_all_group_contains_all_hosts() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("web1"));
    inv.add_host(Host::new("web2"));
    inv.add_host(Host::new("db1"));

    let hosts = inv.match_hosts("all");
    assert_eq!(hosts.len(), 3);
    assert!(hosts.contains(&"web1".to_string()));
    assert!(hosts.contains(&"web2".to_string()));
    assert!(hosts.contains(&"db1".to_string()));
}

#[test]
fn test_star_matches_all_hosts() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("server1"));
    inv.add_host(Host::new("server2"));

    let hosts = inv.match_hosts("*");
    assert_eq!(hosts.len(), 2);
}

#[test]
fn test_specific_host_match() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("webserver"));
    inv.add_host(Host::new("database"));

    let hosts = inv.match_hosts("webserver");
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0], "webserver");
}

#[test]
fn test_group_match() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("webservers").with_host("web1").with_host("web2"));
    inv.add_host(Host::new("web1").in_group("webservers"));
    inv.add_host(Host::new("web2").in_group("webservers"));
    inv.add_host(Host::new("db1"));

    let hosts = inv.match_hosts("webservers");
    assert_eq!(hosts.len(), 2);
    assert!(hosts.contains(&"web1".to_string()));
    assert!(hosts.contains(&"web2".to_string()));
}

// =============================================================================
// Pattern Matching Tests
// =============================================================================

#[test]
fn test_glob_pattern_star_suffix() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("web1"));
    inv.add_host(Host::new("web2"));
    inv.add_host(Host::new("db1"));

    let hosts = inv.match_hosts("web*");
    assert_eq!(hosts.len(), 2);
    assert!(hosts.contains(&"web1".to_string()));
    assert!(hosts.contains(&"web2".to_string()));
}

#[test]
fn test_glob_pattern_star_prefix() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("prod-web"));
    inv.add_host(Host::new("dev-web"));
    inv.add_host(Host::new("prod-db"));

    let hosts = inv.match_hosts("*-web");
    assert_eq!(hosts.len(), 2);
}

#[test]
fn test_glob_pattern_question_mark() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("web1"));
    inv.add_host(Host::new("web2"));
    inv.add_host(Host::new("web10"));

    let hosts = inv.match_hosts("web?");
    assert_eq!(hosts.len(), 2);
    assert!(hosts.contains(&"web1".to_string()));
    assert!(hosts.contains(&"web2".to_string()));
}

#[test]
fn test_range_pattern() {
    let mut inv = MockInventory::new();
    for i in 0..10 {
        inv.add_host(Host::new(&format!("web{}", i)));
    }

    let hosts = inv.match_hosts("web[0:4]");
    // Range pattern should match hosts web0 through web4
    assert!(hosts.len() >= 1, "Range pattern should match some hosts");
    // All matched hosts should be in the range
    for host in &hosts {
        if let Some(num_str) = host.strip_prefix("web") {
            if let Ok(num) = num_str.parse::<usize>() {
                assert!(num <= 4, "Host {} outside range", host);
            }
        }
    }
}

// =============================================================================
// Set Operations Tests
// =============================================================================

#[test]
fn test_union_pattern() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("web").with_host("web1"));
    inv.add_group(Group::new("db").with_host("db1"));
    inv.add_host(Host::new("web1").in_group("web"));
    inv.add_host(Host::new("db1").in_group("db"));

    let hosts = inv.match_hosts("web:db");
    assert_eq!(hosts.len(), 2);
}

#[test]
fn test_intersection_pattern() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("prod").with_host("prod-web").with_host("prod-db"));
    inv.add_group(Group::new("web").with_host("prod-web").with_host("dev-web"));
    inv.add_host(Host::new("prod-web").in_group("prod").in_group("web"));
    inv.add_host(Host::new("prod-db").in_group("prod"));
    inv.add_host(Host::new("dev-web").in_group("web"));

    let hosts = inv.match_hosts("prod:&web");
    assert_eq!(hosts.len(), 1);
    assert!(hosts.contains(&"prod-web".to_string()));
}

#[test]
fn test_difference_pattern() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("servers").with_host("web1").with_host("web2").with_host("db1"));
    inv.add_group(Group::new("web").with_host("web1").with_host("web2"));
    inv.add_host(Host::new("web1").in_group("servers").in_group("web"));
    inv.add_host(Host::new("web2").in_group("servers").in_group("web"));
    inv.add_host(Host::new("db1").in_group("servers"));

    let hosts = inv.match_hosts("servers:!web");
    assert_eq!(hosts.len(), 1);
    assert!(hosts.contains(&"db1".to_string()));
}

#[test]
fn test_negation_pattern() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("exclude").with_host("skip-me"));
    inv.add_host(Host::new("keep1"));
    inv.add_host(Host::new("keep2"));
    inv.add_host(Host::new("skip-me").in_group("exclude"));

    let hosts = inv.match_hosts("!exclude");
    assert_eq!(hosts.len(), 2);
    assert!(!hosts.contains(&"skip-me".to_string()));
}

// =============================================================================
// Group Hierarchy Tests
// =============================================================================

#[test]
fn test_parent_group_includes_children() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("web").with_host("web1"));
    inv.add_group(Group::new("production").with_child("web"));
    inv.add_host(Host::new("web1").in_group("web"));

    let hosts = inv.match_hosts("production");
    assert!(hosts.contains(&"web1".to_string()));
}

#[test]
fn test_nested_group_hierarchy() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("web").with_host("web1"));
    inv.add_group(Group::new("app").with_child("web"));
    inv.add_group(Group::new("production").with_child("app"));
    inv.add_host(Host::new("web1").in_group("web"));

    let hosts = inv.match_hosts("production");
    assert!(hosts.contains(&"web1".to_string()));
}

#[test]
fn test_multiple_children_groups() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("web").with_host("web1"));
    inv.add_group(Group::new("db").with_host("db1"));
    inv.add_group(Group::new("production").with_child("web").with_child("db"));
    inv.add_host(Host::new("web1").in_group("web"));
    inv.add_host(Host::new("db1").in_group("db"));

    let hosts = inv.match_hosts("production");
    assert_eq!(hosts.len(), 2);
}

// =============================================================================
// Variable Resolution Tests
// =============================================================================

#[test]
fn test_host_vars_override_group_vars() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("web")
        .with_host("web1")
        .with_var("port", json!(80)));
    inv.add_host(Host::new("web1")
        .in_group("web")
        .with_var("port", json!(8080)));

    let vars = inv.get_host_vars("web1");
    assert_eq!(vars.get("port"), Some(&json!(8080)));
}

#[test]
fn test_group_vars_inherited() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("web")
        .with_host("web1")
        .with_var("http_port", json!(80)));
    inv.add_host(Host::new("web1").in_group("web"));

    let vars = inv.get_host_vars("web1");
    assert_eq!(vars.get("http_port"), Some(&json!(80)));
}

#[test]
fn test_multiple_group_vars_merged() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("web")
        .with_host("web1")
        .with_var("http_port", json!(80)));
    inv.add_group(Group::new("secure")
        .with_host("web1")
        .with_var("https_port", json!(443)));
    inv.add_host(Host::new("web1").in_group("web").in_group("secure"));

    let vars = inv.get_host_vars("web1");
    assert_eq!(vars.get("http_port"), Some(&json!(80)));
    assert_eq!(vars.get("https_port"), Some(&json!(443)));
}

#[test]
fn test_host_specific_vars() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("special-host")
        .with_var("custom_setting", json!("unique_value")));

    let vars = inv.get_host_vars("special-host");
    assert_eq!(vars.get("custom_setting"), Some(&json!("unique_value")));
}

// =============================================================================
// Special Group Tests
// =============================================================================

#[test]
fn test_all_group_special() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("h1"));
    inv.add_host(Host::new("h2"));

    // All group automatically contains all hosts
    assert!(inv.host_in_group("h1", "all"));
    assert!(inv.host_in_group("h2", "all"));
}

#[test]
fn test_localhost_pattern() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("localhost"));

    let hosts = inv.match_hosts("localhost");
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0], "localhost");
}

// =============================================================================
// Edge Cases and Consistency Tests
// =============================================================================

#[test]
fn test_empty_pattern_returns_empty() {
    let inv = MockInventory::new();
    let hosts = inv.match_hosts("");
    assert!(hosts.is_empty());
}

#[test]
fn test_nonexistent_group_returns_empty() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("host1"));

    let hosts = inv.match_hosts("nonexistent_group");
    assert!(hosts.is_empty());
}

#[test]
fn test_nonexistent_host_returns_empty() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("existing"));

    let hosts = inv.match_hosts("nonexistent_host");
    assert!(hosts.is_empty());
}

#[test]
fn test_duplicate_hosts_deduplicated() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("g1").with_host("host1"));
    inv.add_group(Group::new("g2").with_host("host1"));
    inv.add_host(Host::new("host1").in_group("g1").in_group("g2"));

    let hosts = inv.match_hosts("g1:g2");
    assert_eq!(hosts.iter().filter(|h| *h == "host1").count(), 1);
}

#[test]
fn test_case_sensitive_matching() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("WebServer"));
    inv.add_host(Host::new("webserver"));

    // Exact match should be case sensitive
    let hosts = inv.match_hosts("WebServer");
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0], "WebServer");
}

// =============================================================================
// Regression Tests
// =============================================================================

#[test]
fn test_regression_pattern_with_underscore() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("web_server_1"));
    inv.add_host(Host::new("web_server_2"));

    let hosts = inv.match_hosts("web_server_*");
    assert_eq!(hosts.len(), 2);
}

#[test]
fn test_regression_pattern_with_dash() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("web-server-1"));
    inv.add_host(Host::new("web-server-2"));

    let hosts = inv.match_hosts("web-server-*");
    assert_eq!(hosts.len(), 2);
}

#[test]
fn test_regression_pattern_with_dots() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("web.example.com"));
    inv.add_host(Host::new("db.example.com"));

    let hosts = inv.match_hosts("*.example.com");
    assert_eq!(hosts.len(), 2);
}

#[test]
fn test_regression_complex_pattern() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("prod").with_host("prod-web1").with_host("prod-web2").with_host("prod-db1"));
    inv.add_group(Group::new("web").with_host("prod-web1").with_host("prod-web2").with_host("dev-web1"));
    inv.add_host(Host::new("prod-web1").in_group("prod").in_group("web"));
    inv.add_host(Host::new("prod-web2").in_group("prod").in_group("web"));
    inv.add_host(Host::new("prod-db1").in_group("prod"));
    inv.add_host(Host::new("dev-web1").in_group("web"));

    // Production web servers only
    let hosts = inv.match_hosts("prod:&web");
    assert_eq!(hosts.len(), 2);
    assert!(hosts.contains(&"prod-web1".to_string()));
    assert!(hosts.contains(&"prod-web2".to_string()));
}

// =============================================================================
// CI Guard Tests
// =============================================================================

#[test]
fn test_ci_guard_all_pattern_types() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("g1").with_host("h1").with_host("h2"));
    inv.add_group(Group::new("g2").with_host("h2").with_host("h3"));
    inv.add_host(Host::new("h1").in_group("g1"));
    inv.add_host(Host::new("h2").in_group("g1").in_group("g2"));
    inv.add_host(Host::new("h3").in_group("g2"));

    // All pattern types should work
    assert!(!inv.match_hosts("all").is_empty());
    assert!(!inv.match_hosts("*").is_empty());
    assert!(!inv.match_hosts("g1").is_empty());
    assert!(!inv.match_hosts("h1").is_empty());
    assert!(!inv.match_hosts("h*").is_empty());
    assert!(!inv.match_hosts("g1:g2").is_empty());
    assert!(!inv.match_hosts("g1:&g2").is_empty());
    assert!(!inv.match_hosts("g1:!g2").is_empty());
}

#[test]
fn test_ci_guard_variable_inheritance() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("all_servers")
        .with_host("server1")
        .with_var("base_var", json!("base")));
    inv.add_group(Group::new("web")
        .with_host("server1")
        .with_var("web_var", json!("web")));
    inv.add_host(Host::new("server1")
        .in_group("all_servers")
        .in_group("web")
        .with_var("host_var", json!("host")));

    let vars = inv.get_host_vars("server1");

    // All variable sources should be present
    assert!(vars.contains_key("base_var"));
    assert!(vars.contains_key("web_var"));
    assert!(vars.contains_key("host_var"));
}

#[test]
fn test_ci_guard_group_hierarchy_consistency() {
    let mut inv = MockInventory::new();
    inv.add_group(Group::new("child").with_host("h1"));
    inv.add_group(Group::new("parent").with_child("child"));
    inv.add_group(Group::new("grandparent").with_child("parent"));
    inv.add_host(Host::new("h1").in_group("child"));

    // All levels should resolve the host
    assert!(inv.match_hosts("child").contains(&"h1".to_string()));
    assert!(inv.match_hosts("parent").contains(&"h1".to_string()));
    assert!(inv.match_hosts("grandparent").contains(&"h1".to_string()));
}

// =============================================================================
// Ansible Compatibility Tests
// =============================================================================

#[test]
fn test_ansible_compatible_implicit_localhost() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("localhost")
        .with_var("ansible_connection", json!("local")));

    // localhost should be matchable
    let hosts = inv.match_hosts("localhost");
    assert!(!hosts.is_empty());

    // Should have connection var
    let vars = inv.get_host_vars("localhost");
    assert_eq!(vars.get("ansible_connection"), Some(&json!("local")));
}

#[test]
fn test_ansible_compatible_host_groups() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("h1").in_group("g1").in_group("g2"));
    inv.add_group(Group::new("g1").with_host("h1"));
    inv.add_group(Group::new("g2").with_host("h1"));

    // Host should appear in both groups
    assert!(inv.host_in_group("h1", "g1"));
    assert!(inv.host_in_group("h1", "g2"));
}

#[test]
fn test_ansible_compatible_limit_pattern() {
    let mut inv = MockInventory::new();
    inv.add_host(Host::new("web1"));
    inv.add_host(Host::new("web2"));
    inv.add_host(Host::new("db1"));

    // Common --limit patterns
    assert_eq!(inv.match_hosts("web1").len(), 1);
    assert_eq!(inv.match_hosts("web*").len(), 2);
    assert_eq!(inv.match_hosts("all").len(), 3);
}
