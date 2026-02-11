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

    /// Import Warewulf container images and overlays into Rustible metadata.
    #[cfg(feature = "hpc")]
    #[command(name = "warewulf-images")]
    WarewulfImages(WarewulfImagesArgs),
}

/// Arguments for the warewulf-images migration subcommand.
#[cfg(feature = "hpc")]
#[derive(Parser, Debug, Clone)]
pub struct WarewulfImagesArgs {
    /// Path to a YAML file describing Warewulf images and overlays.
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
            MigrateCommand::WarewulfImages(args) => execute_warewulf_images(args, ctx).await,
        }
    }
}

/// Display available migration sources.
async fn execute_status(ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("MIGRATION STATUS");
    ctx.output.section("Available Migration Sources");

    #[cfg(feature = "hpc")]
    {
        println!("  warewulf-images   Import Warewulf container images and overlays");
    }

    #[cfg(not(feature = "hpc"))]
    {
        println!("  (no migration sources available with current features)");
        println!();
        println!("  Enable the 'hpc' feature for Warewulf migration support.");
    }

    Ok(0)
}

/// Execute the Warewulf image/overlay import.
#[cfg(feature = "hpc")]
async fn execute_warewulf_images(
    args: &WarewulfImagesArgs,
    ctx: &mut CommandContext,
) -> Result<i32> {
    use rustible::migration::warewulf::image::WarewulfImageImporter;

    ctx.output.banner("WAREWULF IMAGE + OVERLAY IMPORT");

    if !args.input.exists() {
        ctx.output
            .error(&format!("Input file not found: {}", args.input.display()));
        return Ok(1);
    }

    let content = std::fs::read_to_string(&args.input)?;
    ctx.output.info(&format!(
        "Parsing Warewulf image config from: {}",
        args.input.display()
    ));

    let importer = WarewulfImageImporter::new();
    let result = match importer.import_from_yaml(&content) {
        Ok(r) => r,
        Err(e) => {
            ctx.output.error(&format!("Failed to parse input: {}", e));
            return Ok(1);
        }
    };

    let summary = result.report.compute_summary();

    if args.json {
        let output = serde_json::json!({
            "images": result.images.len(),
            "overlays": result.overlays.len(),
            "template_count": result.template_count,
            "image_names": result.images.iter().map(|i| &i.name).collect::<Vec<_>>(),
            "overlay_names": result.overlays.iter().map(|o| &o.name).collect::<Vec<_>>(),
            "findings": summary.total_findings,
            "errors": summary.errors,
            "warnings": summary.warnings,
            "dry_run": args.dry_run,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        ctx.output.section("Import Results");
        println!("  Images imported:    {}", result.images.len());
        println!("  Overlays imported:  {}", result.overlays.len());
        println!("  Template files:     {}", result.template_count);

        if !result.images.is_empty() {
            ctx.output.section("Images");
            for img in &result.images {
                let size_mb = img.size_bytes / (1024 * 1024);
                let cksum = img
                    .checksum
                    .as_deref()
                    .map(|c| &c[..c.len().min(12)])
                    .unwrap_or("-");
                println!(
                    "  {:<20} {:<40} {}MB  sha256:{}",
                    img.name, img.container_name, size_mb, cksum
                );
            }
        }

        if !result.overlays.is_empty() {
            ctx.output.section("Overlays");
            for ovl in &result.overlays {
                let ww_count = ovl
                    .template_files
                    .iter()
                    .filter(|f| f.is_ww_template)
                    .count();
                println!(
                    "  {:<20} type={:<8} files={} (ww_templates={})",
                    ovl.name,
                    format!("{:?}", ovl.overlay_type).to_lowercase(),
                    ovl.template_files.len(),
                    ww_count
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
