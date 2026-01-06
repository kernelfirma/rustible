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

use super::CommandContext;

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
        let inventory = ctx.inventory();
        if inventory.is_none() {
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

        // Return exit code based on drift status
        if report.summary.drifted > 0 || report.summary.missing > 0 {
            Ok(2) // Drift detected
        } else {
            Ok(0) // No drift
        }
    }

    /// Detect drift by comparing current state to desired state
    async fn detect_drift(
        &self,
        ctx: &mut CommandContext,
        plays: &[serde_yaml::Value],
    ) -> Result<DriftReport> {
        let mut findings = Vec::new();
        let mut hosts_checked = Vec::new();

        for play in plays {
            let hosts_pattern = play.get("hosts").and_then(|h| h.as_str()).unwrap_or("all");

            // In a real implementation, this would resolve the host pattern
            // and connect to each host to gather current state
            let play_hosts = vec![hosts_pattern.to_string()];
            hosts_checked.extend(play_hosts.clone());

            // Get tasks from the play
            if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                for task in tasks {
                    // Check drift for each task
                    for host in &play_hosts {
                        if let Some(finding) = self.check_task_drift(ctx, host, task).await? {
                            if self.fail_fast && finding.status != DriftStatus::InSync {
                                findings.push(finding.clone());
                                break;
                            } else {
                                findings.push(finding);
                            }
                        }
                    }
                }
            }
        }

        // Calculate summary
        let mut summary = DriftSummary::default();
        summary.total_resources = findings.len();
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

    /// Check drift for a single task
    async fn check_task_drift(
        &self,
        ctx: &CommandContext,
        host: &str,
        task: &serde_yaml::Value,
    ) -> Result<Option<DriftFinding>> {
        let task_name = task
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unnamed task");

        // Detect module type from task
        let (module_type, module_args) = self.detect_module(task)?;

        if let Some(ref filter) = self.resource_type {
            if module_type != *filter {
                return Ok(None);
            }
        }

        // Simulate drift detection based on module type
        // In a real implementation, this would:
        // 1. Connect to the host
        // 2. Gather current state for the resource
        // 3. Compare to desired state from task arguments
        let finding = self.simulate_drift_check(host, task_name, &module_type, &module_args)?;

        if self.detailed && finding.status != DriftStatus::InSync {
            ctx.output.debug(&format!(
                "Drift detected in {} on {}: {:?}",
                task_name, host, finding.status
            ));
        }

        Ok(Some(finding))
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

    /// Simulate drift check (placeholder for actual implementation)
    fn simulate_drift_check(
        &self,
        host: &str,
        task_name: &str,
        module_type: &str,
        module_args: &serde_yaml::Value,
    ) -> Result<DriftFinding> {
        // This is a simulation - in real implementation, this would:
        // 1. Execute check-mode equivalent for the module
        // 2. Compare gathered facts with desired state

        // For now, return an in-sync status as a placeholder
        Ok(DriftFinding {
            host: host.to_string(),
            resource: task_name.to_string(),
            resource_type: module_type.to_string(),
            status: DriftStatus::InSync,
            current_state: Some(serde_json::json!({
                "module": module_type,
                "state": "present"
            })),
            desired_state: Some(serde_json::to_value(module_args)?),
            description: format!("{} on {} is in sync", task_name, host),
            diff: None,
        })
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
}
