//! Fleet dashboard command
//!
//! Provides a summary view of managed infrastructure status.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use super::CommandContext;

/// Arguments for the fleet command
#[derive(Parser, Debug, Clone)]
pub struct FleetArgs {
    #[command(subcommand)]
    pub action: FleetAction,
}

/// Fleet subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum FleetAction {
    /// Show fleet status overview
    Status(FleetStatusArgs),
    /// List all managed hosts with their status
    Hosts(FleetHostsArgs),
    /// Show recent execution history
    History(FleetHistoryArgs),
}

/// Arguments for fleet status
#[derive(Parser, Debug, Clone)]
pub struct FleetStatusArgs {
    /// Inventory file
    #[arg(short, long)]
    pub inventory: Option<PathBuf>,
    /// Output in JSON format
    #[arg(long)]
    pub json: bool,
}

/// Arguments for fleet hosts
#[derive(Parser, Debug, Clone)]
pub struct FleetHostsArgs {
    /// Inventory file
    #[arg(short, long)]
    pub inventory: Option<PathBuf>,
    /// Filter by group
    #[arg(short, long)]
    pub group: Option<String>,
    /// Output in JSON format
    #[arg(long)]
    pub json: bool,
}

/// Arguments for fleet history
#[derive(Parser, Debug, Clone)]
pub struct FleetHistoryArgs {
    /// Number of recent entries to show
    #[arg(short = 'n', long, default_value = "10")]
    pub limit: usize,
    /// Output in JSON format
    #[arg(long)]
    pub json: bool,
}

impl FleetArgs {
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.action {
            FleetAction::Status(args) => execute_status(args, ctx).await,
            FleetAction::Hosts(args) => execute_hosts(args, ctx).await,
            FleetAction::History(args) => execute_history(args, ctx).await,
        }
    }
}

async fn execute_status(args: &FleetStatusArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("FLEET STATUS");

    // Try to load inventory
    let inventory_path = args.inventory.clone().or_else(|| ctx.inventory().cloned());

    let (hosts, groups) = if let Some(inv_path) = &inventory_path {
        match load_inventory_summary(inv_path) {
            Ok((h, g)) => (h, g),
            Err(e) => {
                ctx.output
                    .warning(&format!("Could not load inventory: {}", e));
                (Vec::new(), Vec::new())
            }
        }
    } else {
        ctx.output
            .warning("No inventory specified. Use -i or set in config.");
        (Vec::new(), Vec::new())
    };

    if args.json {
        let summary = serde_json::json!({
            "total_hosts": hosts.len(),
            "total_groups": groups.len(),
            "hosts": hosts,
            "groups": groups,
        });
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        ctx.output.section("Infrastructure Summary");
        println!("Total hosts:  {}", hosts.len());
        println!("Total groups: {}", groups.len());

        if !groups.is_empty() {
            ctx.output.section("Groups");
            for group in &groups {
                println!("  - {}", group);
            }
        }

        if !hosts.is_empty() {
            ctx.output.section("Hosts");
            println!("{:<30} {:<20} {:<10}", "HOST", "GROUP", "STATUS");
            println!("{}", "-".repeat(60));
            for host in &hosts {
                println!("{:<30} {:<20} {:<10}", host.name, host.group, "unknown");
            }
        }
    }

    Ok(0)
}

async fn execute_hosts(args: &FleetHostsArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("FLEET HOSTS");

    let inventory_path = args.inventory.clone().or_else(|| ctx.inventory().cloned());

    let (hosts, _groups) = if let Some(inv_path) = &inventory_path {
        match load_inventory_summary(inv_path) {
            Ok((h, g)) => (h, g),
            Err(e) => {
                ctx.output
                    .error(&format!("Could not load inventory: {}", e));
                return Ok(1);
            }
        }
    } else {
        ctx.output
            .error("No inventory specified. Use -i or set in config.");
        return Ok(1);
    };

    let filtered: Vec<_> = if let Some(group) = &args.group {
        hosts.iter().filter(|h| h.group == *group).collect()
    } else {
        hosts.iter().collect()
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
    } else {
        println!(
            "{:<30} {:<20} {:<15} {:<10}",
            "HOST", "GROUP", "CONNECTION", "PORT"
        );
        println!("{}", "-".repeat(75));
        for host in &filtered {
            println!(
                "{:<30} {:<20} {:<15} {:<10}",
                host.name,
                host.group,
                host.connection.as_deref().unwrap_or("ssh"),
                host.port.unwrap_or(22)
            );
        }
        println!("\nTotal: {} host(s)", filtered.len());
    }

    Ok(0)
}

async fn execute_history(args: &FleetHistoryArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("EXECUTION HISTORY");

    // Check for log directory
    let log_dir = std::path::Path::new(".rustible/logs");
    if !log_dir.exists() {
        println!("No execution history found.");
        println!("Run playbooks to generate execution history.");
        return Ok(0);
    }

    // List recent log files
    let mut entries: Vec<_> = std::fs::read_dir(log_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "json" || ext == "log")
        })
        .collect();

    entries.sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));
    entries.truncate(args.limit);

    if entries.is_empty() {
        println!("No execution history found.");
        return Ok(0);
    }

    if args.json {
        let items: Vec<_> = entries
            .iter()
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                let modified = metadata
                    .modified()
                    .ok()
                    .map(|t| format_duration_ago(t.elapsed().unwrap_or_default()));
                Some(serde_json::json!({
                    "file": entry.file_name().to_string_lossy(),
                    "modified": modified,
                    "size": metadata.len(),
                }))
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        println!("{:<25} {:<40} {:<10}", "TIMESTAMP", "FILE", "SIZE");
        println!("{}", "-".repeat(75));
        for entry in &entries {
            let metadata = entry.metadata()?;
            let modified = metadata
                .modified()
                .map(|t| {
                    let duration = t.elapsed().unwrap_or_default();
                    format_duration_ago(duration)
                })
                .unwrap_or_else(|_| "unknown".to_string());
            let size = metadata.len();
            println!(
                "{:<25} {:<40} {:<10}",
                modified,
                entry.file_name().to_string_lossy(),
                format_size(size)
            );
        }
    }

    Ok(0)
}

#[derive(Debug, Clone, serde::Serialize)]
struct HostInfo {
    name: String,
    group: String,
    connection: Option<String>,
    port: Option<u16>,
}

fn load_inventory_summary(path: &std::path::Path) -> Result<(Vec<HostInfo>, Vec<String>)> {
    let content = std::fs::read_to_string(path)?;
    let value: serde_yaml::Value = serde_yaml::from_str(&content)?;

    let mut hosts = Vec::new();
    let mut groups = Vec::new();

    if let Some(mapping) = value.as_mapping() {
        for (key, group_val) in mapping {
            let group_name = key.as_str().unwrap_or("unknown").to_string();

            // Collect hosts from this group
            if let Some(hosts_map) = group_val.get("hosts").and_then(|h| h.as_mapping()) {
                groups.push(group_name.clone());
                for (host_key, host_val) in hosts_map {
                    let host_name = host_key.as_str().unwrap_or("unknown").to_string();
                    let connection = host_val
                        .as_mapping()
                        .and_then(|m| {
                            m.get(&serde_yaml::Value::String("ansible_connection".to_string()))
                        })
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let port = host_val
                        .as_mapping()
                        .and_then(|m| m.get(&serde_yaml::Value::String("ansible_port".to_string())))
                        .and_then(|v| v.as_u64())
                        .map(|p| p as u16);
                    hosts.push(HostInfo {
                        name: host_name,
                        group: group_name.clone(),
                        connection,
                        port,
                    });
                }
            }

            // Recurse into children
            if let Some(children) = group_val.get("children").and_then(|c| c.as_mapping()) {
                for (child_key, child_val) in children {
                    let child_name = child_key.as_str().unwrap_or("unknown").to_string();
                    if let Some(child_hosts) = child_val.get("hosts").and_then(|h| h.as_mapping()) {
                        groups.push(child_name.clone());
                        for (host_key, host_val) in child_hosts {
                            let host_name = host_key.as_str().unwrap_or("unknown").to_string();
                            let connection = host_val
                                .as_mapping()
                                .and_then(|m| {
                                    m.get(&serde_yaml::Value::String(
                                        "ansible_connection".to_string(),
                                    ))
                                })
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            let port = host_val
                                .as_mapping()
                                .and_then(|m| {
                                    m.get(&serde_yaml::Value::String("ansible_port".to_string()))
                                })
                                .and_then(|v| v.as_u64())
                                .map(|p| p as u16);
                            hosts.push(HostInfo {
                                name: host_name,
                                group: child_name.clone(),
                                connection,
                                port,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok((hosts, groups))
}

fn format_duration_ago(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
