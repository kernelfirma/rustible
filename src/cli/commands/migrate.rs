//! Migration CLI commands.

use anyhow::Result;
use clap::{Parser, Subcommand};
#[cfg(feature = "hpc")]
use std::path::PathBuf;

use super::CommandContext;

#[derive(Parser, Debug, Clone)]
pub struct MigrateArgs {
    #[command(subcommand)]
    pub command: MigrateCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum MigrateCommand {
    /// Show available migration sources
    #[command(name = "status")]
    Status,

    /// Import xCAT service-node hierarchy into Rustible inventory
    #[cfg(feature = "hpc")]
    #[command(name = "xcat-hierarchy")]
    XcatHierarchy(XcatHierarchyArgs),
}

#[cfg(feature = "hpc")]
#[derive(Parser, Debug, Clone)]
pub struct XcatHierarchyArgs {
    /// Path to xCAT hierarchy YAML file
    pub input: PathBuf,
    /// Dry run (validate without writing)
    #[arg(long)]
    pub dry_run: bool,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

impl MigrateArgs {
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            MigrateCommand::Status => {
                ctx.output.banner("MIGRATION STATUS");
                ctx.output.info("Available migration sources:");
                #[cfg(feature = "hpc")]
                ctx.output.info("  - xcat-hierarchy: Import xCAT service-node topology");
                #[cfg(not(feature = "hpc"))]
                ctx.output.info("  (enable 'hpc' feature for xCAT/Warewulf importers)");
                Ok(0)
            }
            #[cfg(feature = "hpc")]
            MigrateCommand::XcatHierarchy(args) => {
                use rustible::migration::xcat::hierarchy::XcatHierarchyImporter;
                ctx.output.banner("XCAT HIERARCHY IMPORT");
                let content = std::fs::read_to_string(&args.input)?;
                let importer = XcatHierarchyImporter::new(args.dry_run);
                let (result, report) = importer.import_from_yaml(&content)?;
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    ctx.output.info(&format!("Service nodes: {}", result.service_node_count));
                    ctx.output.info(&format!("Compute nodes: {}", result.compute_node_count));
                    ctx.output.info(&format!("Total nodes: {}", result.total_nodes));
                    if result.unassigned_count > 0 {
                        ctx.output.warning(&format!("Unassigned: {}", result.unassigned_count));
                    }
                    for finding in &report.findings {
                        let status = match finding.status {
                            rustible::migration::FindingStatus::Pass => "PASS",
                            rustible::migration::FindingStatus::Fail => "FAIL",
                            rustible::migration::FindingStatus::Partial => "PARTIAL",
                            rustible::migration::FindingStatus::Skipped => "SKIP",
                        };
                        ctx.output.info(&format!("[{}] {}", status, finding.name));
                    }
                    if args.dry_run {
                        ctx.output.info("(dry-run mode - no files written)");
                    }
                    match report.outcome {
                        Some(rustible::migration::MigrationOutcome::Pass) => ctx.output.success("Import PASSED"),
                        Some(rustible::migration::MigrationOutcome::Fail) => ctx.output.error("Import FAILED"),
                        None => {}
                    }
                }
                match report.outcome {
                    Some(rustible::migration::MigrationOutcome::Pass) => Ok(0),
                    _ => Ok(1),
                }
            }
        }
    }
}
