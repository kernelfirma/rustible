//! Drift Detection Command
//!
//! This module provides a `drift` subcommand that compares the current system state
//! against the desired state defined in playbooks, similar to Terraform's plan output.
//!
//! ## Features
//!
//! - Detect configuration drift on managed hosts
//! - Show differences between current and desired state
//! - Generate remediation playbook
//! - Support for specific resource types or all managed resources
//!
//! ## Usage
//!
//! ```bash
//! # Check all hosts for drift
//! rustible drift playbook.yml
//!
//! # Check specific hosts
//! rustible drift playbook.yml --limit webservers
//!
//! # Generate remediation playbook
//! rustible drift playbook.yml --remediate > fix-drift.yml
//!
//! # Output in JSON format
//! rustible drift playbook.yml --json
//! ```

use anyhow::Result;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use reqwest;

use rustible::inventory::Inventory;
use rustible::modules::{ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleRegistry};

use super::CommandContext;

/// Modules that are non-stateful and should be skipped during drift detection.
const NON_DRIFT_MODULES: &[&str] = &[
    "command",
    "shell",
    "raw",
    "debug",
    "set_fact",
    "assert",
    "fail",
    "meta",
    "pause",
    "wait_for",
    "include_vars",
    "include_tasks",
    "import_tasks",
    "script",
    "facts",
];

/// Arguments for the drift command
#[derive(Parser, Debug, Clone)]
pub struct DriftArgs {
    /// Playbook file to check drift against
    pub playbook: PathBuf,

    /// Limit drift check to specific hosts (pattern)
    #[arg(short = 'l', long)]
    pub limit: Option<String>,

    /// Check only specific task/resource types
    #[arg(long)]
    pub resource_type: Option<String>,

    /// Generate remediation playbook output
    #[arg(long)]
    pub remediate: bool,

    /// Output in JSON format
    #[arg(long)]
    pub json: bool,

    /// Show detailed diff for each drifted resource
    #[arg(long)]
    pub detailed: bool,

    /// Include unchanged resources in output
    #[arg(long)]
    pub show_all: bool,

    /// Stop on first drift detected
    #[arg(long)]
    pub fail_fast: bool,

    /// Timeout for drift checks in seconds
    #[arg(long, default_value = "300")]
    pub timeout: u64,

    /// Maximum allowed drift percentage (0-100). Exit code 2 if breached.
    #[arg(long)]
    pub sla_max_drift_percent: Option<f64>,

    /// Webhook URL to POST drift alerts to when SLA is breached
    #[arg(long)]
    pub alert_webhook: Option<String>,

    /// Email address for drift alerts (stored in report; sending not implemented)
    #[arg(long)]
    pub alert_email: Option<String>,
}

/// Status of a resource's drift
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DriftStatus {
    /// Resource matches desired state
    InSync,
    /// Resource exists but differs from desired state
    Drifted,
    /// Resource should exist but doesn't
    Missing,
    /// Resource exists but shouldn't
    Extra,
    /// Unable to determine drift status
    Unknown,
}

impl std::fmt::Display for DriftStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriftStatus::InSync => write!(f, "in-sync"),
            DriftStatus::Drifted => write!(f, "DRIFTED"),
            DriftStatus::Missing => write!(f, "MISSING"),
            DriftStatus::Extra => write!(f, "EXTRA"),
            DriftStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// A single drift finding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftFinding {
    /// Host where drift was detected
    pub host: String,
    /// Task/resource name
    pub resource: String,
    /// Module type (file, package, service, etc.)
    pub resource_type: String,
    /// Drift status
    pub status: DriftStatus,
    /// Current state (actual system state)
    pub current_state: Option<serde_json::Value>,
    /// Desired state (from playbook)
    pub desired_state: Option<serde_json::Value>,
    /// Human-readable description of the drift
    pub description: String,
    /// Detailed differences
    pub diff: Option<DriftDiff>,
}

/// Detailed diff information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftDiff {
    /// Fields that differ
    pub changed_fields: Vec<FieldDiff>,
    /// Fields added in desired state
    pub added_fields: Vec<String>,
    /// Fields removed from desired state
    pub removed_fields: Vec<String>,
}

/// A single field difference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDiff {
    /// Field path (e.g., "owner", "mode", "content")
    pub field: String,
    /// Current value
    pub current: String,
    /// Desired value
    pub desired: String,
}

/// Complete drift report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftReport {
    /// Playbook that was analyzed
    pub playbook: String,
    /// Hosts that were checked
    pub hosts_checked: Vec<String>,
    /// All drift findings
    pub findings: Vec<DriftFinding>,
    /// Summary statistics
    pub summary: DriftSummary,
    /// Timestamp of the report
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Summary statistics for drift report
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriftSummary {
    /// Total resources checked
    pub total_resources: usize,
    /// Resources in sync
    pub in_sync: usize,
    /// Resources with drift
    pub drifted: usize,
    /// Missing resources
    pub missing: usize,
    /// Extra (unexpected) resources
    pub extra: usize,
    /// Unknown status
    pub unknown: usize,
}

/// Convert YAML module args to ModuleParams (HashMap<String, serde_json::Value>)
fn yaml_to_module_params(args: &serde_yaml::Value) -> ModuleParams {
    let mut params = ModuleParams::new();
    match args {
        serde_yaml::Value::Mapping(map) => {
            for (key, value) in map {
                if let Some(key_str) = key.as_str() {
                    if let Ok(json_val) = serde_json::to_value(value) {
                        params.insert(key_str.to_string(), json_val);
                    }
                }
            }
        }
        serde_yaml::Value::String(s) => {
            // String shorthand: treat as the "name" parameter (e.g. "package: nginx")
            params.insert("name".to_string(), serde_json::Value::String(s.clone()));
        }
        serde_yaml::Value::Null => {}
        other => {
            // For scalar values, store as free_form
            if let Ok(json_val) = serde_json::to_value(other) {
                params.insert("free_form".to_string(), json_val);
            }
        }
    }
    params
}

/// Convert module output diff to the CLI DriftDiff type
fn module_output_to_drift_diff(output: &ModuleOutput) -> Option<DriftDiff> {
    let mut changed_fields = Vec::new();

    if let Some(ref diff) = output.diff {
        if !diff.before.is_empty() || !diff.after.is_empty() {
            changed_fields.push(FieldDiff {
                field: "state".to_string(),
                current: diff.before.clone(),
                desired: diff.after.clone(),
            });
        }
        // If there are details, add them as an additional field diff
        if let Some(ref details) = diff.details {
            if !details.is_empty() {
                changed_fields.push(FieldDiff {
                    field: "details".to_string(),
                    current: String::new(),
                    desired: details.clone(),
                });
            }
        }
    }

    // Also inspect data for common field-level details
    for (key, value) in &output.data {
        match key.as_str() {
            "path" | "mode" | "owner" | "group" | "state" | "version" => {
                changed_fields.push(FieldDiff {
                    field: key.clone(),
                    current: String::new(),
                    desired: value.to_string(),
                });
            }
            _ => {}
        }
    }

    if changed_fields.is_empty() {
        None
    } else {
        Some(DriftDiff {
            changed_fields,
            added_fields: Vec::new(),
            removed_fields: Vec::new(),
        })
    }
}

impl DriftArgs {
    /// Execute the drift command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output.banner("DRIFT DETECTION");

        // Load the playbook
        ctx.output
            .info(&format!("Analyzing playbook: {}", self.playbook.display()));

        let playbook_content = std::fs::read_to_string(&self.playbook)?;
        let plays: Vec<serde_yaml::Value> = serde_yaml::from_str(&playbook_content)?;

        // Build inventory
        let inventory_path = ctx.inventory();
        if inventory_path.is_none() {
            ctx.output
                .error("No inventory specified. Use -i <inventory>");
            return Ok(1);
        }

        ctx.output
            .info("Connecting to hosts and gathering state...");

        // Perform drift detection
        let report = self.detect_drift(ctx, &plays).await?;

        // Output results
        if self.json {
            let json_output = serde_json::to_string_pretty(&report)?;
            println!("{}", json_output);
        } else if self.remediate {
            self.generate_remediation(ctx, &report)?;
        } else {
            self.print_report(ctx, &report);
        }

        // Determine base exit code from drift status
        let mut exit_code = if report.summary.drifted > 0 || report.summary.missing > 0 {
            2 // Drift detected
        } else {
            0 // No drift
        };

        // SLA tracking: check if drift percentage exceeds threshold
        if let Some(sla_threshold) = self.sla_max_drift_percent {
            let total = report.summary.total_resources as f64;
            let drift_percent = if total > 0.0 {
                (report.summary.drifted + report.summary.missing + report.summary.extra) as f64
                    / total
                    * 100.0
            } else {
                0.0
            };

            if drift_percent > sla_threshold {
                ctx.output.error(&format!(
                    "SLA BREACH: drift {:.1}% exceeds threshold {:.1}%",
                    drift_percent, sla_threshold
                ));

                if let Some(ref email) = self.alert_email {
                    ctx.output.warning(&format!(
                        "Alert email registered: {} (sending not implemented)",
                        email
                    ));
                }

                if let Some(ref webhook_url) = self.alert_webhook {
                    let payload = serde_json::json!({
                        "event": "drift_sla_breach",
                        "drift_percent": drift_percent,
                        "sla_threshold": sla_threshold,
                        "total_resources": report.summary.total_resources,
                        "drifted_resources": report.summary.drifted,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });

                    ctx.output
                        .info(&format!("Sending SLA breach alert to {}", webhook_url));

                    let client = reqwest::Client::new();
                    match client.post(webhook_url).json(&payload).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                ctx.output.success("Webhook alert sent successfully");
                            } else {
                                ctx.output.warning(&format!(
                                    "Webhook returned status: {}",
                                    resp.status()
                                ));
                            }
                        }
                        Err(e) => {
                            ctx.output
                                .warning(&format!("Failed to send webhook alert: {}", e));
                        }
                    }
                }

                exit_code = 2;
            }
        }

        Ok(exit_code)
    }

    /// Detect drift by comparing current state to desired state
    async fn detect_drift(
        &self,
        ctx: &mut CommandContext,
        plays: &[serde_yaml::Value],
    ) -> Result<DriftReport> {
        let registry = ModuleRegistry::with_builtins();

        // Load inventory
        let inventory_path = ctx
            .inventory()
            .cloned()
            .expect("inventory checked in execute()");
        let inventory = Inventory::load(&inventory_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to load inventory from {}: {}",
                inventory_path.display(),
                e
            )
        })?;

        let mut findings = Vec::new();
        let mut hosts_checked = Vec::new();

        for play in plays {
            let hosts_pattern = play.get("hosts").and_then(|h| h.as_str()).unwrap_or("all");

            // Resolve hosts from the inventory using the pattern
            let play_hosts: Vec<String> = match inventory.get_hosts_for_pattern(hosts_pattern) {
                Ok(hosts) => hosts.iter().map(|h| h.name.clone()).collect(),
                Err(e) => {
                    ctx.output.warning(&format!(
                        "Could not resolve host pattern '{}': {}",
                        hosts_pattern, e
                    ));
                    continue;
                }
            };

            // Apply limit filter if set
            let play_hosts: Vec<String> = if let Some(ref limit) = self.limit {
                match inventory.get_hosts_for_pattern(limit) {
                    Ok(limit_hosts) => {
                        let limit_names: std::collections::HashSet<&str> =
                            limit_hosts.iter().map(|h| h.name.as_str()).collect();
                        play_hosts
                            .into_iter()
                            .filter(|h| limit_names.contains(h.as_str()))
                            .collect()
                    }
                    Err(_) => {
                        // Fall back to simple string match against limit
                        play_hosts
                            .into_iter()
                            .filter(|h| h.contains(limit.as_str()))
                            .collect()
                    }
                }
            } else {
                play_hosts
            };

            hosts_checked.extend(play_hosts.clone());

            // Get tasks from the play
            if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                for task in tasks {
                    for host in &play_hosts {
                        if let Some(finding) =
                            self.check_task_drift(ctx, host, task, &registry).await?
                        {
                            let status = finding.status.clone();
                            findings.push(finding);

                            if self.fail_fast && status != DriftStatus::InSync {
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Calculate summary
        let mut summary = DriftSummary {
            total_resources: findings.len(),
            ..Default::default()
        };
        for finding in &findings {
            match finding.status {
                DriftStatus::InSync => summary.in_sync += 1,
                DriftStatus::Drifted => summary.drifted += 1,
                DriftStatus::Missing => summary.missing += 1,
                DriftStatus::Extra => summary.extra += 1,
                DriftStatus::Unknown => summary.unknown += 1,
            }
        }

        Ok(DriftReport {
            playbook: self.playbook.display().to_string(),
            hosts_checked: hosts_checked.into_iter().collect(),
            findings: if self.show_all {
                findings
            } else {
                findings
                    .into_iter()
                    .filter(|f| f.status != DriftStatus::InSync)
                    .collect()
            },
            summary,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Check drift for a single task by executing the module in check_mode
    async fn check_task_drift(
        &self,
        ctx: &CommandContext,
        host: &str,
        task: &serde_yaml::Value,
        registry: &ModuleRegistry,
    ) -> Result<Option<DriftFinding>> {
        let task_name = task
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unnamed task");

        // Detect module type from task
        let (module_type, module_args) = self.detect_module(task)?;

        // Skip non-stateful modules that can't have drift
        if NON_DRIFT_MODULES.contains(&module_type.as_str()) || module_type == "unknown" {
            return Ok(None);
        }

        if let Some(ref filter) = self.resource_type {
            if module_type != *filter {
                return Ok(None);
            }
        }

        // Execute the module in check_mode to detect drift
        let finding =
            self.execute_drift_check(host, task_name, &module_type, &module_args, registry)?;

        if self.detailed && finding.status != DriftStatus::InSync {
            ctx.output.debug(&format!(
                "Drift detected in {} on {}: {:?}",
                task_name, host, finding.status
            ));
        }

        Ok(Some(finding))
    }

    /// Execute a module in check_mode to detect drift
    fn execute_drift_check(
        &self,
        host: &str,
        task_name: &str,
        module_type: &str,
        module_args: &serde_yaml::Value,
        registry: &ModuleRegistry,
    ) -> Result<DriftFinding> {
        let params = yaml_to_module_params(module_args);
        let desired_state = serde_json::to_value(module_args).ok();

        // Build module context with check_mode and diff_mode enabled
        let module_ctx = ModuleContext::new()
            .with_check_mode(true)
            .with_diff_mode(true);

        // Execute with timeout
        let timeout_duration = Duration::from_secs(self.timeout);
        let result = {
            let start = std::time::Instant::now();
            let res = registry.execute(module_type, &params, &module_ctx);
            if start.elapsed() > timeout_duration {
                return Ok(DriftFinding {
                    host: host.to_string(),
                    resource: task_name.to_string(),
                    resource_type: module_type.to_string(),
                    status: DriftStatus::Unknown,
                    current_state: None,
                    desired_state,
                    description: format!(
                        "{} on {} timed out after {}s",
                        task_name, host, self.timeout
                    ),
                    diff: None,
                });
            }
            res
        };

        match result {
            Ok(output) => {
                if output.changed {
                    // Module reports it would change something -> resource has drifted
                    let drift_diff = module_output_to_drift_diff(&output);
                    Ok(DriftFinding {
                        host: host.to_string(),
                        resource: task_name.to_string(),
                        resource_type: module_type.to_string(),
                        status: DriftStatus::Drifted,
                        current_state: Some(output.to_result_json()),
                        desired_state,
                        description: format!(
                            "{} on {} has drifted: {}",
                            task_name, host, output.msg
                        ),
                        diff: drift_diff,
                    })
                } else {
                    // Module reports no changes needed -> in sync
                    Ok(DriftFinding {
                        host: host.to_string(),
                        resource: task_name.to_string(),
                        resource_type: module_type.to_string(),
                        status: DriftStatus::InSync,
                        current_state: Some(output.to_result_json()),
                        desired_state,
                        description: format!("{} on {} is in sync", task_name, host),
                        diff: None,
                    })
                }
            }
            Err(ModuleError::NotFound(_)) => {
                // Unknown module - can't check drift
                Ok(DriftFinding {
                    host: host.to_string(),
                    resource: task_name.to_string(),
                    resource_type: module_type.to_string(),
                    status: DriftStatus::Unknown,
                    current_state: None,
                    desired_state,
                    description: format!(
                        "Module '{}' not found in registry, cannot check drift for {}",
                        module_type, task_name
                    ),
                    diff: None,
                })
            }
            Err(e) => {
                // Execution error - report as unknown drift status
                Ok(DriftFinding {
                    host: host.to_string(),
                    resource: task_name.to_string(),
                    resource_type: module_type.to_string(),
                    status: DriftStatus::Unknown,
                    current_state: None,
                    desired_state,
                    description: format!(
                        "Error checking drift for {} on {}: {}",
                        task_name, host, e
                    ),
                    diff: None,
                })
            }
        }
    }

    /// Detect which module a task uses
    fn detect_module(&self, task: &serde_yaml::Value) -> Result<(String, serde_yaml::Value)> {
        // List of known module names
        let known_modules = [
            "file",
            "copy",
            "template",
            "stat",
            "lineinfile",
            "blockinfile",
            "package",
            "apt",
            "yum",
            "dnf",
            "pip",
            "service",
            "systemd",
            "systemd_unit",
            "user",
            "group",
            "command",
            "shell",
            "raw",
            "get_url",
            "uri",
            "cron",
            "debug",
            "set_fact",
            "assert",
        ];

        for module in known_modules {
            if let Some(args) = task.get(module) {
                return Ok((module.to_string(), args.clone()));
            }
        }

        // Check for FQCN modules
        if let Some(mapping) = task.as_mapping() {
            for (key, value) in mapping {
                if let Some(key_str) = key.as_str() {
                    if key_str.contains('.')
                        && !["name", "when", "register", "tags", "notify", "become"]
                            .contains(&key_str)
                    {
                        return Ok((key_str.to_string(), value.clone()));
                    }
                }
            }
        }

        Ok(("unknown".to_string(), serde_yaml::Value::Null))
    }

    /// Print the drift report
    fn print_report(&self, ctx: &CommandContext, report: &DriftReport) {
        ctx.output.section("DRIFT SUMMARY");

        ctx.output.info(&format!("Playbook: {}", report.playbook));
        ctx.output.info(&format!(
            "Hosts checked: {}",
            report.hosts_checked.join(", ")
        ));
        ctx.output.info(&format!(
            "Checked at: {}",
            report.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        ));

        println!();

        // Summary table
        ctx.output.info(&format!(
            "Total Resources: {}",
            report.summary.total_resources
        ));

        if report.summary.in_sync > 0 {
            ctx.output
                .info(&format!("  In Sync: {}", report.summary.in_sync));
        }
        if report.summary.drifted > 0 {
            ctx.output
                .warning(&format!("  Drifted: {}", report.summary.drifted));
        }
        if report.summary.missing > 0 {
            ctx.output
                .error(&format!("  Missing: {}", report.summary.missing));
        }
        if report.summary.extra > 0 {
            ctx.output
                .warning(&format!("  Extra: {}", report.summary.extra));
        }
        if report.summary.unknown > 0 {
            ctx.output
                .debug(&format!("  Unknown: {}", report.summary.unknown));
        }

        println!();

        // Detailed findings
        if !report.findings.is_empty() && (report.summary.drifted > 0 || report.summary.missing > 0)
        {
            ctx.output.section("DRIFT DETAILS");

            for finding in &report.findings {
                match finding.status {
                    DriftStatus::Drifted => {
                        ctx.output.warning(&format!(
                            "~ {} [{}] on {}",
                            finding.resource, finding.resource_type, finding.host
                        ));
                    }
                    DriftStatus::Missing => {
                        ctx.output.error(&format!(
                            "- {} [{}] on {}",
                            finding.resource, finding.resource_type, finding.host
                        ));
                    }
                    DriftStatus::Extra => {
                        ctx.output.warning(&format!(
                            "+ {} [{}] on {}",
                            finding.resource, finding.resource_type, finding.host
                        ));
                    }
                    _ => {}
                }

                if self.detailed {
                    ctx.output.info(&format!("    {}", finding.description));
                    if let Some(ref diff) = finding.diff {
                        for field_diff in &diff.changed_fields {
                            ctx.output.debug(&format!(
                                "      {}: {} -> {}",
                                field_diff.field, field_diff.current, field_diff.desired
                            ));
                        }
                    }
                }
            }
        }

        // Final status
        println!();
        if report.summary.drifted == 0 && report.summary.missing == 0 {
            ctx.output
                .success("No drift detected. System is in sync with desired state.");
        } else {
            ctx.output.warning(&format!(
                "Drift detected: {} drifted, {} missing resources",
                report.summary.drifted, report.summary.missing
            ));
            ctx.output
                .hint("Run with --remediate to generate a fix playbook");
        }
    }

    /// Generate remediation playbook
    fn generate_remediation(&self, ctx: &CommandContext, report: &DriftReport) -> Result<()> {
        let mut remediation_tasks: HashMap<String, Vec<serde_yaml::Value>> = HashMap::new();

        for finding in &report.findings {
            if finding.status == DriftStatus::InSync {
                continue;
            }

            // Create remediation task
            let mut task = serde_yaml::Mapping::new();
            task.insert(
                serde_yaml::Value::String("name".to_string()),
                serde_yaml::Value::String(format!("Fix drift: {}", finding.resource)),
            );

            if let Some(ref desired) = finding.desired_state {
                let module_args = serde_yaml::to_value(desired)?;
                task.insert(
                    serde_yaml::Value::String(finding.resource_type.clone()),
                    module_args,
                );
            }

            remediation_tasks
                .entry(finding.host.clone())
                .or_default()
                .push(serde_yaml::Value::Mapping(task));
        }

        // Generate playbook
        let mut plays = Vec::new();
        for (host, tasks) in remediation_tasks {
            let mut play = serde_yaml::Mapping::new();
            play.insert(
                serde_yaml::Value::String("name".to_string()),
                serde_yaml::Value::String(format!("Remediate drift on {}", host)),
            );
            play.insert(
                serde_yaml::Value::String("hosts".to_string()),
                serde_yaml::Value::String(host),
            );
            play.insert(
                serde_yaml::Value::String("tasks".to_string()),
                serde_yaml::Value::Sequence(tasks),
            );
            plays.push(serde_yaml::Value::Mapping(play));
        }

        let playbook = serde_yaml::to_string(&plays)?;

        println!("# Remediation playbook generated by rustible drift");
        println!(
            "# Generated: {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!("# Original playbook: {}", report.playbook);
        println!("---");
        println!("{}", playbook);

        ctx.output.debug(&format!(
            "Generated remediation for {} resources",
            report.findings.len()
        ));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drift_status_display() {
        assert_eq!(DriftStatus::InSync.to_string(), "in-sync");
        assert_eq!(DriftStatus::Drifted.to_string(), "DRIFTED");
        assert_eq!(DriftStatus::Missing.to_string(), "MISSING");
    }

    #[test]
    fn test_drift_summary_default() {
        let summary = DriftSummary::default();
        assert_eq!(summary.total_resources, 0);
        assert_eq!(summary.in_sync, 0);
        assert_eq!(summary.drifted, 0);
    }

    #[test]
    fn test_drift_args_parse() {
        use clap::Parser;

        let args = DriftArgs::try_parse_from([
            "drift",
            "playbook.yml",
            "--limit",
            "webservers",
            "--detailed",
        ])
        .unwrap();

        assert_eq!(args.playbook, PathBuf::from("playbook.yml"));
        assert_eq!(args.limit, Some("webservers".to_string()));
        assert!(args.detailed);
    }

    #[test]
    fn test_yaml_to_module_params_mapping() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
            path: /etc/hosts
            owner: root
            mode: "0644"
            "#,
        )
        .unwrap();

        let params = yaml_to_module_params(&yaml);
        assert_eq!(
            params.get("path"),
            Some(&serde_json::Value::String("/etc/hosts".to_string()))
        );
        assert_eq!(
            params.get("owner"),
            Some(&serde_json::Value::String("root".to_string()))
        );
        assert_eq!(
            params.get("mode"),
            Some(&serde_json::Value::String("0644".to_string()))
        );
    }

    #[test]
    fn test_yaml_to_module_params_string() {
        let yaml = serde_yaml::Value::String("nginx".to_string());
        let params = yaml_to_module_params(&yaml);
        assert_eq!(
            params.get("name"),
            Some(&serde_json::Value::String("nginx".to_string()))
        );
    }

    #[test]
    fn test_yaml_to_module_params_null() {
        let yaml = serde_yaml::Value::Null;
        let params = yaml_to_module_params(&yaml);
        assert!(params.is_empty());
    }

    #[test]
    fn test_non_drift_modules_skipped() {
        for module in NON_DRIFT_MODULES {
            assert!(
                NON_DRIFT_MODULES.contains(module),
                "{} should be in skip list",
                module
            );
        }
        // Ensure stateful modules are NOT in the skip list
        assert!(!NON_DRIFT_MODULES.contains(&"file"));
        assert!(!NON_DRIFT_MODULES.contains(&"package"));
        assert!(!NON_DRIFT_MODULES.contains(&"service"));
        assert!(!NON_DRIFT_MODULES.contains(&"user"));
        assert!(!NON_DRIFT_MODULES.contains(&"copy"));
    }
}
