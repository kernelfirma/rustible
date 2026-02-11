//! Migrate command for importing configuration from external systems.
//!
//! Provides subcommands for importing node profiles and inventory data
//! from HPC cluster management tools into Rustible.
//!
//! ## Usage
//!
//! ```bash
//! # Show migration status / help
//! rustible migrate status
//!
//! # Import Warewulf profiles (requires --features hpc)
//! rustible migrate warewulf-profiles /etc/warewulf/nodes.conf
//! ```

use clap::{Parser, Subcommand};

/// Arguments for the `migrate` command.
#[derive(Parser, Debug, Clone)]
pub struct MigrateArgs {
    /// Migration subcommand.
    #[command(subcommand)]
    pub command: MigrateCommands,
}

/// Available migration subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum MigrateCommands {
    /// Show migration framework status and available importers.
    Status,

    /// Import Warewulf 4 node profiles into Rustible inventory.
    #[cfg(feature = "hpc")]
    #[command(name = "warewulf-profiles")]
    WarewulfProfiles {
        /// Path to the Warewulf nodes YAML file (e.g. /etc/warewulf/nodes.conf).
        path: std::path::PathBuf,
    },
}

impl MigrateArgs {
    /// Execute the migrate command.
    pub async fn execute(
        &self,
        ctx: &mut super::CommandContext,
    ) -> anyhow::Result<i32> {
        match &self.command {
            MigrateCommands::Status => {
                ctx.output.banner("MIGRATION STATUS");
                ctx.output.info("Available importers:");
                #[cfg(feature = "hpc")]
                ctx.output.info("  - warewulf-profiles: Import Warewulf 4 node profiles");
                #[cfg(not(feature = "hpc"))]
                ctx.output
                    .info("  (enable --features hpc for Warewulf importers)");
                Ok(0)
            }
            #[cfg(feature = "hpc")]
            MigrateCommands::WarewulfProfiles { path } => {
                ctx.output.banner("WAREWULF PROFILE IMPORT");
                ctx.output
                    .info(&format!("Importing from: {}", path.display()));

                if !path.exists() {
                    ctx.output
                        .error(&format!("File not found: {}", path.display()));
                    return Ok(1);
                }

                match rustible::migration::warewulf::WarewulfProfileImporter::import_from_yaml(path)
                {
                    Ok(result) => {
                        ctx.output.section("Imported Hosts");
                        for host in &result.hosts {
                            let ip = host
                                .ansible_host
                                .as_deref()
                                .unwrap_or("(no IP)");
                            ctx.output.info(&format!(
                                "  {} -> {} [groups: {}]",
                                host.name,
                                ip,
                                host.groups.join(", ")
                            ));
                        }

                        ctx.output.section("Imported Groups");
                        for group in &result.groups {
                            ctx.output.info(&format!(
                                "  {} ({} hosts)",
                                group.name,
                                group.hosts.len()
                            ));
                        }

                        ctx.output.section("Report");
                        ctx.output.info(&format!("{}", result.report));

                        if result.report.outcome
                            == Some(rustible::migration::MigrationOutcome::Failed)
                        {
                            Ok(1)
                        } else {
                            Ok(0)
                        }
                    }
                    Err(e) => {
                        ctx.output
                            .error(&format!("Migration failed: {}", e));
                        Ok(1)
                    }
                }
            }
        }
    }
}
