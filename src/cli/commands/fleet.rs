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
    /// Show the cluster topology graph
    #[cfg(feature = "distributed")]
    Topology(FleetTopologyArgs),
    /// Show cluster health summary
    #[cfg(feature = "distributed")]
    Health(FleetHealthArgs),
    /// List cluster nodes with optional filters
    #[cfg(feature = "distributed")]
    Nodes(FleetNodesArgs),
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

/// Arguments for fleet topology
#[cfg(feature = "distributed")]
#[derive(Parser, Debug, Clone)]
pub struct FleetTopologyArgs {
    /// Output format: ascii, json, or table
    #[arg(short, long, default_value = "ascii")]
    pub format: String,
}

/// Arguments for fleet health
#[cfg(feature = "distributed")]
#[derive(Parser, Debug, Clone)]
pub struct FleetHealthArgs {
    /// Output in JSON format
    #[arg(long)]
    pub json: bool,
}

/// Arguments for fleet nodes
#[cfg(feature = "distributed")]
#[derive(Parser, Debug, Clone)]
pub struct FleetNodesArgs {
    /// Filter by node type (controller, worker, gateway, storage)
    #[arg(long, value_name = "TYPE")]
    pub r#type: Option<String>,
    /// Filter by node role (leader, follower, candidate, observer)
    #[arg(long)]
    pub role: Option<String>,
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
            #[cfg(feature = "distributed")]
            FleetAction::Topology(args) => execute_topology(args, ctx).await,
            #[cfg(feature = "distributed")]
            FleetAction::Health(args) => execute_health(args, ctx).await,
            #[cfg(feature = "distributed")]
            FleetAction::Nodes(args) => execute_nodes(args, ctx).await,
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

// ---------------------------------------------------------------------------
// Distributed-only subcommands (cluster topology, health, nodes)
// ---------------------------------------------------------------------------

#[cfg(feature = "distributed")]
fn build_demo_topology() -> rustible::distributed::topology::ClusterTopology {
    use rustible::distributed::topology::{
        ClusterTopology, EdgeType, NodeRole, NodeType, TopologyEdge, TopologyNode,
    };

    let mut topo = ClusterTopology::new();

    topo.add_node(
        TopologyNode::new(
            "ctrl-1",
            "Controller 1",
            NodeType::Controller,
            NodeRole::Leader,
        )
        .with_address("10.0.0.1:9000"),
    );
    topo.add_node(
        TopologyNode::new(
            "ctrl-2",
            "Controller 2",
            NodeType::Controller,
            NodeRole::Follower,
        )
        .with_address("10.0.0.2:9000"),
    );
    topo.add_node(
        TopologyNode::new("worker-1", "Worker 1", NodeType::Worker, NodeRole::Follower)
            .with_address("10.0.1.1:9001"),
    );
    topo.add_node(
        TopologyNode::new("worker-2", "Worker 2", NodeType::Worker, NodeRole::Follower)
            .with_address("10.0.1.2:9001"),
    );
    topo.add_node(
        TopologyNode::new("gw-1", "Gateway", NodeType::Gateway, NodeRole::Observer)
            .with_address("10.0.0.100:443"),
    );

    topo.add_edge(
        "ctrl-1",
        "ctrl-2",
        TopologyEdge::new(EdgeType::Control).with_latency(1),
    );
    topo.add_edge(
        "ctrl-1",
        "worker-1",
        TopologyEdge::new(EdgeType::Data).with_latency(3),
    );
    topo.add_edge(
        "ctrl-1",
        "worker-2",
        TopologyEdge::new(EdgeType::Data).with_latency(5),
    );
    topo.add_edge(
        "ctrl-2",
        "worker-1",
        TopologyEdge::new(EdgeType::Heartbeat).with_latency(2),
    );
    topo.add_edge(
        "ctrl-2",
        "worker-2",
        TopologyEdge::new(EdgeType::Heartbeat).with_latency(4),
    );
    topo.add_edge(
        "gw-1",
        "ctrl-1",
        TopologyEdge::new(EdgeType::Control).with_latency(1),
    );

    topo
}

#[cfg(feature = "distributed")]
async fn execute_topology(args: &FleetTopologyArgs, ctx: &mut CommandContext) -> Result<i32> {
    use rustible::distributed::topology::renderer::{RenderFormat, TopologyRenderer};

    ctx.output.banner("CLUSTER TOPOLOGY");

    let format = match args.format.to_lowercase().as_str() {
        "json" => RenderFormat::Json,
        "table" => RenderFormat::Table,
        _ => RenderFormat::Ascii,
    };

    let topo = build_demo_topology();
    let rendered = TopologyRenderer::render(&topo, format);
    println!("{}", rendered);

    Ok(0)
}

#[cfg(feature = "distributed")]
async fn execute_health(_args: &FleetHealthArgs, ctx: &mut CommandContext) -> Result<i32> {
    use rustible::distributed::topology::health::{
        ClusterHealthSummary, HealthAggregator, HealthCheck, HealthStatus, NodeHealth,
    };

    ctx.output.banner("CLUSTER HEALTH");

    let topo = build_demo_topology();

    // Build sample health checks for the demo topology
    let node_checks = vec![
        NodeHealth::new(
            "ctrl-1",
            vec![
                HealthCheck::new("raft", HealthStatus::Healthy),
                HealthCheck::new("disk", HealthStatus::Healthy),
            ],
        ),
        NodeHealth::new(
            "ctrl-2",
            vec![
                HealthCheck::new("raft", HealthStatus::Healthy),
                HealthCheck::new("disk", HealthStatus::Degraded).with_message("85% utilization"),
            ],
        ),
        NodeHealth::new(
            "worker-1",
            vec![HealthCheck::new("connectivity", HealthStatus::Healthy)],
        ),
        NodeHealth::new(
            "worker-2",
            vec![HealthCheck::new("connectivity", HealthStatus::Healthy)],
        ),
        NodeHealth::new("gw-1", vec![HealthCheck::new("tls", HealthStatus::Healthy)]),
    ];

    let summary: ClusterHealthSummary = HealthAggregator::aggregate(&topo, &node_checks);

    if _args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        ctx.output.section("Summary");
        println!("Total nodes:    {}", summary.total);
        println!("  Healthy:      {}", summary.healthy);
        println!("  Degraded:     {}", summary.degraded);
        println!("  Unhealthy:    {}", summary.unhealthy);
        println!("  Unknown:      {}", summary.unknown);
        println!("  Overall:      {}", summary.overall);

        ctx.output.section("Per-Node Health");
        println!("{:<15} {:<12} {:<30}", "NODE", "STATUS", "CHECKS");
        println!("{}", "-".repeat(57));
        for nh in &node_checks {
            let check_summary: Vec<String> = nh
                .checks
                .iter()
                .map(|c| {
                    let msg = c.message.as_deref().unwrap_or("");
                    if msg.is_empty() {
                        format!("{}={}", c.name, c.status)
                    } else {
                        format!("{}={} ({})", c.name, c.status, msg)
                    }
                })
                .collect();
            println!(
                "{:<15} {:<12} {}",
                nh.node_id,
                format!("{}", nh.status),
                check_summary.join(", "),
            );
        }
    }

    Ok(0)
}

#[cfg(feature = "distributed")]
async fn execute_nodes(args: &FleetNodesArgs, ctx: &mut CommandContext) -> Result<i32> {
    use rustible::distributed::topology::model::{NodeRole, NodeType};
    use rustible::distributed::topology::query::TopologyQuery;

    ctx.output.banner("CLUSTER NODES");

    let topo = build_demo_topology();

    // Apply optional filters
    let nodes: Vec<_> = if let Some(ref type_filter) = args.r#type {
        let nt = match type_filter.to_lowercase().as_str() {
            "controller" => NodeType::Controller,
            "worker" => NodeType::Worker,
            "gateway" => NodeType::Gateway,
            "storage" => NodeType::Storage,
            other => {
                ctx.output.error(&format!(
                    "Unknown node type: {}. Use controller, worker, gateway, or storage.",
                    other
                ));
                return Ok(1);
            }
        };
        TopologyQuery::nodes_by_type(&topo, nt)
    } else if let Some(ref role_filter) = args.role {
        let nr = match role_filter.to_lowercase().as_str() {
            "leader" => NodeRole::Leader,
            "follower" => NodeRole::Follower,
            "candidate" => NodeRole::Candidate,
            "observer" => NodeRole::Observer,
            other => {
                ctx.output.error(&format!(
                    "Unknown node role: {}. Use leader, follower, candidate, or observer.",
                    other
                ));
                return Ok(1);
            }
        };
        TopologyQuery::nodes_by_role(&topo, nr)
    } else {
        topo.nodes().collect()
    };

    if args.json {
        let items: Vec<serde_json::Value> = nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "name": n.name,
                    "type": format!("{}", n.node_type),
                    "role": format!("{}", n.role),
                    "address": n.address,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        println!(
            "{:<15} {:<20} {:<12} {:<10} {:<20}",
            "ID", "NAME", "TYPE", "ROLE", "ADDRESS",
        );
        println!("{}", "-".repeat(77));
        for n in &nodes {
            println!(
                "{:<15} {:<20} {:<12} {:<10} {:<20}",
                n.id,
                n.name,
                format!("{}", n.node_type),
                format!("{}", n.role),
                n.address.as_deref().unwrap_or("-"),
            );
        }
        println!("\nTotal: {} node(s)", nodes.len());
    }

    Ok(0)
}
