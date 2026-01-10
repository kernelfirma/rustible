//! Inventory commands - List hosts and tasks
//!
//! This module implements the `list-hosts` and `list-tasks` subcommands.

use super::{CommandContext, Runnable};
use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;

/// Arguments for list-hosts command
#[derive(Parser, Debug, Clone)]
pub struct ListHostsArgs {
    /// Host pattern to match
    #[arg(default_value = "all")]
    pub pattern: String,

    /// Show host variables
    #[arg(long)]
    pub vars: bool,

    /// Output as YAML
    #[arg(long)]
    pub yaml: bool,

    /// Group by groups instead of flat list
    #[arg(long)]
    pub graph: bool,
}

/// Arguments for list-tasks command
#[derive(Parser, Debug, Clone)]
pub struct ListTasksArgs {
    /// Path to the playbook file
    #[arg(required = true)]
    pub playbook: PathBuf,

    /// Show only tasks with these tags
    #[arg(long, short = 't', action = clap::ArgAction::Append)]
    pub tags: Vec<String>,

    /// Skip tasks with these tags
    #[arg(long, action = clap::ArgAction::Append)]
    pub skip_tags: Vec<String>,

    /// Include task details
    #[arg(long)]
    pub detailed: bool,
}

/// Host information
#[derive(Debug, Clone, serde::Serialize)]
pub struct HostInfo {
    pub name: String,
    pub groups: Vec<String>,
    pub vars: HashMap<String, serde_yaml::Value>,
}

/// Group information
#[derive(Debug, Clone, serde::Serialize)]
pub struct GroupInfo {
    pub name: String,
    pub hosts: Vec<String>,
    pub children: Vec<String>,
    pub vars: HashMap<String, serde_yaml::Value>,
}

/// Inventory structure
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Inventory {
    pub hosts: HashMap<String, HostInfo>,
    pub groups: HashMap<String, GroupInfo>,
}

impl Inventory {
    /// Load inventory from a file
    pub fn load(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read inventory file: {}", path.display()))?;

        // Determine format based on extension
        if path
            .extension()
            .is_some_and(|ext| ext == "yml" || ext == "yaml")
        {
            Self::parse_yaml(&content)
        } else if path.extension().is_some_and(|ext| ext == "json") {
            Self::parse_json(&content)
        } else {
            // Try YAML first, then INI format
            Self::parse_yaml(&content).or_else(|_| Self::parse_ini(&content))
        }
    }

    /// Parse YAML inventory
    fn parse_yaml(content: &str) -> Result<Self> {
        let yaml: serde_yaml::Value = serde_yaml::from_str(content)?;

        let mut inventory = Inventory::default();

        // Parse the 'all' group structure
        if let Some(all) = yaml.get("all") {
            inventory.parse_group("all", all)?;
        } else {
            // Maybe it's a simple host list
            if let Some(mapping) = yaml.as_mapping() {
                for (key, value) in mapping {
                    if let Some(group_name) = key.as_str() {
                        inventory.parse_group(group_name, value)?;
                    }
                }
            }
        }

        Ok(inventory)
    }

    /// Parse a group from YAML
    fn parse_group(&mut self, name: &str, value: &serde_yaml::Value) -> Result<()> {
        let mut group = GroupInfo {
            name: name.to_string(),
            hosts: Vec::new(),
            children: Vec::new(),
            vars: HashMap::new(),
        };

        // Parse hosts
        if let Some(hosts) = value.get("hosts") {
            if let Some(mapping) = hosts.as_mapping() {
                for (host_key, host_value) in mapping {
                    if let Some(host_name) = host_key.as_str() {
                        group.hosts.push(host_name.to_string());

                        // Parse host vars
                        let mut host_vars = HashMap::new();
                        if let Some(vars_mapping) = host_value.as_mapping() {
                            for (var_key, var_value) in vars_mapping {
                                if let Some(var_name) = var_key.as_str() {
                                    host_vars.insert(var_name.to_string(), var_value.clone());
                                }
                            }
                        }

                        let host_info = HostInfo {
                            name: host_name.to_string(),
                            groups: vec![name.to_string()],
                            vars: host_vars,
                        };

                        self.hosts
                            .entry(host_name.to_string())
                            .and_modify(|h| h.groups.push(name.to_string()))
                            .or_insert(host_info);
                    }
                }
            }
        }

        // Parse children (subgroups)
        if let Some(children) = value.get("children") {
            if let Some(mapping) = children.as_mapping() {
                for (child_key, child_value) in mapping {
                    if let Some(child_name) = child_key.as_str() {
                        group.children.push(child_name.to_string());
                        self.parse_group(child_name, child_value)?;
                    }
                }
            }
        }

        // Parse group vars
        if let Some(vars) = value.get("vars") {
            if let Some(mapping) = vars.as_mapping() {
                for (var_key, var_value) in mapping {
                    if let Some(var_name) = var_key.as_str() {
                        group.vars.insert(var_name.to_string(), var_value.clone());
                    }
                }
            }
        }

        self.groups.insert(name.to_string(), group);

        Ok(())
    }

    /// Parse JSON inventory
    fn parse_json(content: &str) -> Result<Self> {
        let json: serde_json::Value = serde_json::from_str(content)?;

        // Convert to YAML Value and use YAML parser
        let yaml_str = serde_yaml::to_string(&json)?;
        Self::parse_yaml(&yaml_str)
    }

    /// Parse INI-style inventory
    fn parse_ini(content: &str) -> Result<Self> {
        let mut inventory = Inventory::default();
        let mut current_group = "ungrouped".to_string();

        // Create default groups
        inventory.groups.insert(
            "all".to_string(),
            GroupInfo {
                name: "all".to_string(),
                hosts: Vec::new(),
                children: vec!["ungrouped".to_string()],
                vars: HashMap::new(),
            },
        );

        inventory.groups.insert(
            "ungrouped".to_string(),
            GroupInfo {
                name: "ungrouped".to_string(),
                hosts: Vec::new(),
                children: Vec::new(),
                vars: HashMap::new(),
            },
        );

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            // Check for group header
            if line.starts_with('[') && line.ends_with(']') {
                let group_name = &line[1..line.len() - 1];

                // Check for :vars or :children suffix
                if let Some(name) = group_name.strip_suffix(":vars") {
                    current_group = format!("{}_vars", name);
                } else if let Some(name) = group_name.strip_suffix(":children") {
                    current_group = format!("{}_children", name);
                } else {
                    current_group = group_name.to_string();
                    if !inventory.groups.contains_key(&current_group) {
                        inventory.groups.insert(
                            current_group.clone(),
                            GroupInfo {
                                name: current_group.clone(),
                                hosts: Vec::new(),
                                children: Vec::new(),
                                vars: HashMap::new(),
                            },
                        );

                        // Add to all group's children
                        if let Some(all_group) = inventory.groups.get_mut("all") {
                            if !all_group.children.contains(&current_group) {
                                all_group.children.push(current_group.clone());
                            }
                        }
                    }
                }
                continue;
            }

            // Parse host line
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let host_name = parts[0];

            // Parse host variables (key=value pairs)
            let mut host_vars = HashMap::new();
            for part in parts.iter().skip(1) {
                if let Some((key, value)) = part.split_once('=') {
                    let parsed_value: serde_yaml::Value = serde_yaml::from_str(value)
                        .unwrap_or_else(|_| serde_yaml::Value::String(value.to_string()));
                    host_vars.insert(key.to_string(), parsed_value);
                }
            }

            // Add host to group
            if let Some(group) = inventory.groups.get_mut(&current_group) {
                if !group.hosts.contains(&host_name.to_string()) {
                    group.hosts.push(host_name.to_string());
                }
            }

            // Add to all group's hosts
            if let Some(all_group) = inventory.groups.get_mut("all") {
                if !all_group.hosts.contains(&host_name.to_string()) {
                    all_group.hosts.push(host_name.to_string());
                }
            }

            // Add/update host info
            inventory
                .hosts
                .entry(host_name.to_string())
                .and_modify(|h| {
                    if !h.groups.contains(&current_group) {
                        h.groups.push(current_group.clone());
                    }
                    h.vars.extend(host_vars.clone());
                })
                .or_insert(HostInfo {
                    name: host_name.to_string(),
                    groups: vec![current_group.clone()],
                    vars: host_vars,
                });
        }

        Ok(inventory)
    }

    /// Get all hosts matching a pattern
    pub fn get_hosts(&self, pattern: &str) -> Vec<&HostInfo> {
        if pattern == "all" {
            return self.hosts.values().collect();
        }

        // Check if pattern is a group name
        if let Some(group) = self.groups.get(pattern) {
            return group
                .hosts
                .iter()
                .filter_map(|h| self.hosts.get(h))
                .collect();
        }

        // Check if pattern matches a host name
        self.hosts
            .values()
            .filter(|h| {
                h.name == pattern || h.name.contains(pattern) || glob_match(&h.name, pattern)
            })
            .collect()
    }
}

/// Simple glob matching
fn glob_match(name: &str, pattern: &str) -> bool {
    if pattern.contains('*') {
        let regex_pattern = pattern.replace('.', "\\.").replace('*', ".*");
        regex::Regex::new(&format!("^{}$", regex_pattern))
            .map(|re| re.is_match(name))
            .unwrap_or(false)
    } else {
        name == pattern
    }
}

impl ListHostsArgs {
    /// Execute the list-hosts command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        // Get inventory path
        let inventory_path = match ctx.inventory() {
            Some(path) => path.clone(),
            None => {
                ctx.output
                    .error("No inventory specified. Use -i to specify an inventory file.");
                return Ok(1);
            }
        };

        // Load inventory
        let inventory = Inventory::load(&inventory_path)?;

        // Get matching hosts
        let hosts = inventory.get_hosts(&self.pattern);

        if hosts.is_empty() {
            ctx.output
                .warning(&format!("No hosts matched pattern: {}", self.pattern));
            return Ok(0);
        }

        if self.yaml {
            // YAML output
            let output: Vec<_> = hosts
                .iter()
                .map(|h| {
                    let mut map = serde_yaml::Mapping::new();
                    map.insert(
                        serde_yaml::Value::String("name".to_string()),
                        serde_yaml::Value::String(h.name.clone()),
                    );
                    if self.vars && !h.vars.is_empty() {
                        map.insert(
                            serde_yaml::Value::String("vars".to_string()),
                            serde_yaml::to_value(&h.vars).unwrap_or_default(),
                        );
                    }
                    map
                })
                .collect();

            println!("{}", serde_yaml::to_string(&output)?);
        } else if self.graph {
            // Graph output (grouped by groups)
            ctx.output
                .section(&format!("Hosts matching pattern: {}", self.pattern));

            let mut groups_shown: HashMap<String, Vec<String>> = HashMap::new();
            for host in &hosts {
                for group in &host.groups {
                    groups_shown
                        .entry(group.clone())
                        .or_default()
                        .push(host.name.clone());
                }
            }

            for (group, group_hosts) in groups_shown {
                ctx.output.list(&format!("@{}", group), &group_hosts);
            }
        } else {
            // Simple list output
            let host_names: Vec<String> = hosts.iter().map(|h| h.name.clone()).collect();
            ctx.output
                .list(&format!("Hosts ({})", hosts.len()), &host_names);

            if self.vars && !ctx.output.is_json() {
                println!();
                for host in hosts {
                    if !host.vars.is_empty() {
                        println!("{}:", host.name);
                        for (key, value) in &host.vars {
                            println!("  {}: {}", key, serde_yaml::to_string(value)?.trim());
                        }
                    }
                }
            }
        }

        Ok(0)
    }
}

impl ListTasksArgs {
    /// Execute the list-tasks command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        // Validate playbook exists
        if !self.playbook.exists() {
            ctx.output.error(&format!(
                "Playbook file not found: {}",
                self.playbook.display()
            ));
            return Ok(1);
        }

        // Load playbook
        let content = std::fs::read_to_string(&self.playbook)?;
        let playbook: serde_yaml::Value = serde_yaml::from_str(&content)?;

        ctx.output
            .section(&format!("Tasks in playbook: {}", self.playbook.display()));

        let mut task_count = 0;

        // Process plays
        if let Some(plays) = playbook.as_sequence() {
            for (play_idx, play) in plays.iter().enumerate() {
                let play_name = play
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("Unnamed play");

                ctx.output
                    .section(&format!("Play #{}: {}", play_idx + 1, play_name));

                // Get tasks
                if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                    for (task_idx, task) in tasks.iter().enumerate() {
                        let task_name = task
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("Unnamed task");

                        // Get task tags for filtering
                        let task_tags: Vec<String> = task
                            .get("tags")
                            .and_then(|t| {
                                if let Some(s) = t.as_str() {
                                    Some(vec![s.to_string()])
                                } else {
                                    t.as_sequence().map(|seq| {
                                        seq.iter()
                                            .filter_map(|v| v.as_str().map(String::from))
                                            .collect()
                                    })
                                }
                            })
                            .unwrap_or_default();

                        // Check skip_tags filter first - skip if any skip_tag matches
                        if !self.skip_tags.is_empty() {
                            let should_skip = self.skip_tags.iter().any(|t| task_tags.contains(t));
                            if should_skip {
                                continue;
                            }
                        }

                        // Check tags filter - only include if tags match
                        if !self.tags.is_empty() {
                            let matches = self.tags.iter().any(|t| task_tags.contains(t));
                            if !matches {
                                continue;
                            }
                        }

                        task_count += 1;

                        // Detect module
                        let module = detect_module(task);

                        if self.detailed {
                            println!("  {:>3}. {} [{}]", task_idx + 1, task_name, module);

                            // Show tags
                            if let Some(tags) = task.get("tags") {
                                println!("       Tags: {}", format_value(tags));
                            }

                            // Show when condition
                            if let Some(when) = task.get("when") {
                                println!("       When: {}", format_value(when));
                            }
                        } else {
                            println!("  {:>3}. {}", task_idx + 1, task_name);
                        }
                    }
                }

                // Handle pre_tasks
                if let Some(tasks) = play.get("pre_tasks").and_then(|t| t.as_sequence()) {
                    println!("\n  Pre-tasks:");
                    for (task_idx, task) in tasks.iter().enumerate() {
                        let task_name = task
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("Unnamed task");
                        println!("    {:>3}. {}", task_idx + 1, task_name);
                        task_count += 1;
                    }
                }

                // Handle post_tasks
                if let Some(tasks) = play.get("post_tasks").and_then(|t| t.as_sequence()) {
                    println!("\n  Post-tasks:");
                    for (task_idx, task) in tasks.iter().enumerate() {
                        let task_name = task
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("Unnamed task");
                        println!("    {:>3}. {}", task_idx + 1, task_name);
                        task_count += 1;
                    }
                }

                // Handle handlers
                if let Some(handlers) = play.get("handlers").and_then(|h| h.as_sequence()) {
                    println!("\n  Handlers:");
                    for (handler_idx, handler) in handlers.iter().enumerate() {
                        let handler_name = handler
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("Unnamed handler");
                        println!("    {:>3}. {}", handler_idx + 1, handler_name);
                    }
                }
            }
        }

        println!("\n{}", "=".repeat(40));
        println!("Total tasks: {}", task_count);

        Ok(0)
    }
}

/// Detect the module used in a task
fn detect_module(task: &serde_yaml::Value) -> &str {
    let modules = [
        "command",
        "shell",
        "copy",
        "file",
        "template",
        "package",
        "apt",
        "yum",
        "dnf",
        "pip",
        "service",
        "systemd",
        "user",
        "group",
        "git",
        "debug",
        "set_fact",
        "include_tasks",
        "import_tasks",
        "include_role",
        "import_role",
        "block",
        "assert",
        "fail",
        "meta",
        "pause",
        "wait_for",
        "uri",
        "get_url",
        "unarchive",
        "synchronize",
        "lineinfile",
        "blockinfile",
        "replace",
        "stat",
        "find",
        "fetch",
        "raw",
        "script",
        "setup",
    ];

    for module in modules {
        if task.get(module).is_some() {
            return module;
        }
    }

    "unknown"
}

/// Format a YAML value for display
fn format_value(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Sequence(seq) => {
            let items: Vec<String> = seq
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            format!("[{}]", items.join(", "))
        }
        _ => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

#[async_trait::async_trait]
impl Runnable for ListHostsArgs {
    async fn run(&self, ctx: &mut CommandContext) -> Result<i32> {
        self.execute(ctx).await
    }
}

#[async_trait::async_trait]
impl Runnable for ListTasksArgs {
    async fn run(&self, ctx: &mut CommandContext) -> Result<i32> {
        self.execute(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_hosts_args() {
        let args = ListHostsArgs::try_parse_from(["list-hosts", "webservers"]).unwrap();
        assert_eq!(args.pattern, "webservers");
    }

    #[test]
    fn test_list_tasks_args() {
        let args = ListTasksArgs::try_parse_from(["list-tasks", "playbook.yml"]).unwrap();
        assert_eq!(args.playbook, PathBuf::from("playbook.yml"));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("web01.example.com", "web*.example.com"));
        assert!(glob_match("db01", "db*"));
        assert!(!glob_match("web01", "db*"));
    }
}
