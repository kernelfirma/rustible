//! Migrate command for importing infrastructure data from external tools.
//!
//! Provides subcommands for each supported migration source.

use anyhow::Result;
use clap::{Parser, Subcommand};

#[cfg(feature = "hpc")]
use std::path::PathBuf;

use super::CommandContext;

/// Arguments for the migrate command.
#[derive(Parser, Debug, Clone)]
pub struct MigrateArgs {
    /// Migration subcommand to execute.
    #[command(subcommand)]
    pub command: MigrateCommand,
}

/// Available migration subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum MigrateCommand {
    /// Show available migration sources and their status.
    Status,

    /// Import xCAT object definitions (nodes, groups) into Rustible inventory.
    #[cfg(feature = "hpc")]
    #[command(name = "xcat-objects")]
    XcatObjects(XcatObjectsArgs),
}

/// Arguments for the xcat-objects migration subcommand.
#[cfg(feature = "hpc")]
#[derive(Parser, Debug, Clone)]
pub struct XcatObjectsArgs {
    /// Path to a file containing `lsdef -l` output.
    pub input: PathBuf,

    /// Perform a dry-run without writing any output files.
    #[arg(long)]
    pub dry_run: bool,

    /// Emit output as JSON instead of human-readable text.
    #[arg(long)]
    pub json: bool,
}

impl MigrateArgs {
    /// Execute the selected migration subcommand.
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            MigrateCommand::Status => execute_status(ctx).await,
            #[cfg(feature = "hpc")]
            MigrateCommand::XcatObjects(args) => execute_xcat_objects(args, ctx).await,
        }
    }
}

/// Display available migration sources.
async fn execute_status(ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("MIGRATION STATUS");
    ctx.output.section("Available Migration Sources");

    #[cfg(feature = "hpc")]
    {
        println!("  xcat-objects   Import xCAT node/group definitions (lsdef output)");
    }

    #[cfg(not(feature = "hpc"))]
    {
        println!("  (no migration sources available with current features)");
        println!();
        println!("  Enable the 'hpc' feature for xCAT migration support.");
    }

    Ok(0)
}

/// Execute the xCAT objects import.
#[cfg(feature = "hpc")]
async fn execute_xcat_objects(args: &XcatObjectsArgs, ctx: &mut CommandContext) -> Result<i32> {
    use rustible::migration::xcat::objects::XcatObjectImporter;

    ctx.output.banner("XCAT OBJECT IMPORT");

    // Read input file
    if !args.input.exists() {
        ctx.output.error(&format!(
            "Input file not found: {}",
            args.input.display()
        ));
        return Ok(1);
    }

    let content = std::fs::read_to_string(&args.input)?;
    ctx.output.info(&format!(
        "Parsing xCAT lsdef output from: {}",
        args.input.display()
    ));

    let importer = XcatObjectImporter::new();

    // Parse the lsdef output
    let objects = match importer.parse_lsdef(&content) {
        Ok(objs) => objs,
        Err(e) => {
            ctx.output.error(&format!("Failed to parse input: {}", e));
            return Ok(1);
        }
    };

    ctx.output
        .info(&format!("Parsed {} xCAT object(s)", objects.len()));

    // Import into inventory structures
    let result = importer.import(&objects);
    let summary = result.report.compute_summary();

    if args.json {
        let output = serde_json::json!({
            "hosts": result.hosts.len(),
            "groups": result.groups.len(),
            "host_names": result.hosts.iter().map(|h| &h.name).collect::<Vec<_>>(),
            "group_names": result.groups.iter().map(|g| &g.name).collect::<Vec<_>>(),
            "findings": summary.total_findings,
            "errors": summary.errors,
            "warnings": summary.warnings,
            "dry_run": args.dry_run,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        ctx.output.section("Import Results");
        println!("  Hosts imported:  {}", result.hosts.len());
        println!("  Groups derived:  {}", result.groups.len());

        if !result.hosts.is_empty() {
            ctx.output.section("Hosts");
            for host in &result.hosts {
                let addr = host.ansible_host.as_deref().unwrap_or("-");
                let groups: Vec<&str> = host.groups.iter().map(|s| s.as_str()).collect();
                println!(
                    "  {:<25} {:<18} groups=[{}]",
                    host.name,
                    addr,
                    groups.join(", ")
                );
            }
        }

        if !result.groups.is_empty() {
            ctx.output.section("Groups");
            for group in &result.groups {
                println!(
                    "  {:<25} ({} host(s))",
                    group.name,
                    group.hosts.len()
                );
            }
        }

        if summary.total_findings > 0 {
            ctx.output.section("Diagnostics");
            println!(
                "  {} error(s), {} warning(s), {} info",
                summary.errors, summary.warnings, summary.info
            );
            for finding in &result.report.findings {
                let icon = match finding.severity {
                    rustible::migration::MigrationSeverity::Error => "ERROR",
                    rustible::migration::MigrationSeverity::Warning => "WARN ",
                    rustible::migration::MigrationSeverity::Info => "INFO ",
                };
                println!("  [{}] {}: {}", icon, finding.id, finding.title);
            }
        }

        if args.dry_run {
            println!();
            ctx.output
                .warning("Dry-run mode: no output files were written.");
        }
    }

    if summary.errors > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}
