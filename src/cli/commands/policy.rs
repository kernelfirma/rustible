//! Policy pack CLI commands.
//!
//! Provides subcommands to list, check, inspect, and initialise policy packs.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use super::CommandContext;

/// Arguments for the `policy` command.
#[derive(Parser, Debug, Clone)]
pub struct PolicyArgs {
    #[command(subcommand)]
    pub action: PolicyAction,
}

/// Policy subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum PolicyAction {
    /// List all available policy packs
    List,
    /// Check a playbook against all policy packs
    Check(PolicyCheckArgs),
    /// Inspect a specific policy pack
    Inspect(PolicyInspectArgs),
    /// Initialise a skeleton policy pack in a directory
    Init(PolicyInitArgs),
}

/// Arguments for `policy check`.
#[derive(Parser, Debug, Clone)]
pub struct PolicyCheckArgs {
    /// Path to the playbook file to check
    pub playbook: PathBuf,
}

/// Arguments for `policy inspect`.
#[derive(Parser, Debug, Clone)]
pub struct PolicyInspectArgs {
    /// Name of the policy pack to inspect
    pub pack_name: String,
}

/// Arguments for `policy init`.
#[derive(Parser, Debug, Clone)]
pub struct PolicyInitArgs {
    /// Output directory for the skeleton pack
    #[arg(default_value = "policy-pack")]
    pub output: PathBuf,
}

impl PolicyArgs {
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.action {
            PolicyAction::List => execute_list(ctx).await,
            PolicyAction::Check(args) => execute_check(args, ctx).await,
            PolicyAction::Inspect(args) => execute_inspect(args, ctx).await,
            PolicyAction::Init(args) => execute_init(args, ctx).await,
        }
    }
}

async fn execute_list(ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("POLICY PACKS");

    let mut registry = rustible::policy::pack::PackRegistry::new();
    registry.discover();

    let packs = registry.list();
    if packs.is_empty() {
        ctx.output.info("No policy packs found.");
        return Ok(0);
    }

    println!(
        "{:<25} {:<10} {:<15} {:<40}",
        "NAME", "VERSION", "CATEGORY", "DESCRIPTION"
    );
    println!("{}", "-".repeat(90));

    for pack in &packs {
        println!(
            "{:<25} {:<10} {:<15} {:<40}",
            pack.name, pack.version, pack.category, pack.description
        );
    }

    println!("\nTotal: {} pack(s)", packs.len());
    Ok(0)
}

async fn execute_check(args: &PolicyCheckArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("POLICY CHECK");

    if !args.playbook.exists() {
        ctx.output
            .error(&format!("Playbook not found: {}", args.playbook.display()));
        return Ok(1);
    }

    ctx.output
        .info(&format!("Checking: {}", args.playbook.display()));

    let content = std::fs::read_to_string(&args.playbook)?;
    let playbook_data: serde_json::Value = serde_yaml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse playbook YAML: {}", e))?;

    let mut registry = rustible::policy::pack::PackRegistry::new();
    registry.discover();

    let results = registry.evaluate_all(&playbook_data);

    let mut total_passed = 0usize;
    let mut total_failed = 0usize;
    let mut total_warnings = 0usize;

    for result in &results {
        ctx.output.section(&format!("Pack: {}", result.pack_name));
        println!(
            "  Passed: {}  Failed: {}  Warnings: {}",
            result.passed, result.failed, result.warnings
        );

        for detail in &result.details {
            println!("  {}", detail);
        }

        total_passed += result.passed;
        total_failed += result.failed;
        total_warnings += result.warnings;
    }

    ctx.output.section("Summary");
    println!(
        "Total: {} passed, {} failed, {} warnings",
        total_passed, total_failed, total_warnings
    );

    if total_failed > 0 {
        ctx.output.error("Policy check failed");
        Ok(1)
    } else if total_warnings > 0 {
        ctx.output.warning("Policy check passed with warnings");
        Ok(0)
    } else {
        ctx.output.success("All policy checks passed");
        Ok(0)
    }
}

async fn execute_inspect(args: &PolicyInspectArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("POLICY PACK INSPECT");

    let mut registry = rustible::policy::pack::PackRegistry::new();
    registry.discover();

    let pack = match registry.get(&args.pack_name) {
        Some(p) => p,
        None => {
            ctx.output.error(&format!(
                "Policy pack '{}' not found. Use 'policy list' to see available packs.",
                args.pack_name
            ));
            return Ok(1);
        }
    };

    println!("Name:        {}", pack.manifest.name);
    println!("Version:     {}", pack.manifest.version);
    println!("Category:    {}", pack.manifest.category);
    println!("Description: {}", pack.manifest.description);
    println!();

    println!("Rules ({}):", pack.rules.len());
    for rule in &pack.rules {
        let severity = match rule.severity {
            rustible::policy::RuleSeverity::Error => "ERROR",
            rustible::policy::RuleSeverity::Warning => "WARN",
            rustible::policy::RuleSeverity::Info => "INFO",
        };
        println!("  - {} [{}]: {}", rule.name, severity, rule.description);
    }

    if !pack.manifest.parameters.is_empty() {
        println!();
        println!("Parameters ({}):", pack.manifest.parameters.len());
        for param in &pack.manifest.parameters {
            let required_str = if param.required {
                "required"
            } else {
                "optional"
            };
            let default_str = param.default_value.as_deref().unwrap_or("none");
            println!(
                "  - {} ({}): {} [default: {}, {}]",
                param.name, param.param_type, param.description, default_str, required_str
            );
        }
    }

    Ok(0)
}

async fn execute_init(args: &PolicyInitArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("POLICY PACK INIT");

    let output = &args.output;

    if output.exists() {
        ctx.output.warning(&format!(
            "Directory '{}' already exists, files may be overwritten.",
            output.display()
        ));
    }

    std::fs::create_dir_all(output)?;

    let manifest_content = r#"---
name: my-policy-pack
version: "0.1.0"
description: "A custom policy pack"
category: !Custom "custom"
rules:
  - require-name
  - max-tasks
parameters:
  - name: max_tasks_per_play
    description: "Maximum number of tasks per play"
    param_type: integer
    default_value: "20"
    required: false
"#;

    let manifest_path = output.join("manifest.yml");
    std::fs::write(&manifest_path, manifest_content)?;
    ctx.output.created(&format!("{}", manifest_path.display()));

    ctx.output.success("Policy pack skeleton created");
    ctx.output.hint(&format!(
        "Edit '{}' to customise your policy pack, then load it with 'policy check'.",
        manifest_path.display()
    ));

    Ok(0)
}
