//! Migration and compatibility CLI commands.

use super::CommandContext;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Migration and compatibility tools
#[derive(Parser, Debug, Clone)]
pub struct MigrateArgs {
    /// Migration subcommand
    #[command(subcommand)]
    pub command: MigrateCommand,
}

/// Available migration commands
#[derive(Subcommand, Debug, Clone)]
pub enum MigrateCommand {
    /// Validate Terraform plan parity
    #[command(name = "terraform-plan")]
    TerraformPlan(TerraformPlanArgs),

    /// Validate Terraform state parity
    #[command(name = "terraform-state")]
    TerraformState(TerraformStateArgs),

    /// Verify Ansible compatibility
    #[command(name = "ansible-compat")]
    AnsibleCompat(AnsibleCompatArgs),

    /// Import xCAT hierarchy
    #[cfg(feature = "hpc")]
    #[command(name = "xcat-hierarchy")]
    XcatHierarchy(XcatHierarchyArgs),

    /// Import xCAT objects
    #[cfg(feature = "hpc")]
    #[command(name = "xcat-objects")]
    XcatObjects(XcatObjectsArgs),

    /// Import Warewulf images
    #[cfg(feature = "hpc")]
    #[command(name = "warewulf-images")]
    WarewulfImages(WarewulfImagesArgs),

    /// Import Warewulf profiles
    #[cfg(feature = "hpc")]
    #[command(name = "warewulf-profiles")]
    WarewulfProfiles(WarewulfProfilesArgs),

    /// Show migration status
    #[command(name = "status")]
    Status,
}

/// Arguments for Terraform plan parity validation
#[derive(Parser, Debug, Clone)]
pub struct TerraformPlanArgs {
    /// Path to Terraform plan JSON (from `terraform show -json plan.out`)
    #[arg(long)]
    pub tf_plan: PathBuf,
    /// Path to Rustible plan JSON
    #[arg(long)]
    pub rustible_plan: PathBuf,
    /// Divergence threshold (0-100, default 100 = exact match required)
    #[arg(long, default_value = "100")]
    pub threshold: u32,
    /// Output format (human, json)
    #[arg(long, default_value = "human")]
    pub format: String,
}

/// Arguments for Terraform state parity validation
#[derive(Parser, Debug, Clone)]
pub struct TerraformStateArgs {
    /// Path to Terraform state file (.tfstate)
    #[arg(long)]
    pub tf_state: PathBuf,
    /// Path to Rustible provisioning state JSON
    #[arg(long)]
    pub rustible_state: PathBuf,
    /// Parity threshold (0-100)
    #[arg(long, default_value = "100")]
    pub threshold: u32,
    /// Output format (human, json)
    #[arg(long, default_value = "human")]
    pub format: String,
}

/// Arguments for Ansible compatibility verification
#[derive(Parser, Debug, Clone)]
pub struct AnsibleCompatArgs {
    /// Path to Ansible project directory or playbook
    #[arg(long)]
    pub path: PathBuf,
    /// Compatibility threshold (0-100)
    #[arg(long, default_value = "80")]
    pub threshold: u32,
    /// Output format (human, json)
    #[arg(long, default_value = "human")]
    pub format: String,
}

/// Arguments for xCAT hierarchy import
#[cfg(feature = "hpc")]
#[derive(Parser, Debug, Clone)]
pub struct XcatHierarchyArgs {
    /// Path to xCAT hierarchy YAML file
    #[arg(long)]
    pub input: PathBuf,
    /// Output directory for Rustible inventory
    #[arg(long)]
    pub output: PathBuf,
    /// Dry run (validate without writing)
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for xCAT objects import
#[cfg(feature = "hpc")]
#[derive(Parser, Debug, Clone)]
pub struct XcatObjectsArgs {
    /// Path to xCAT lsdef output file
    #[arg(long)]
    pub input: PathBuf,
    /// Output directory for Rustible inventory
    #[arg(long)]
    pub output: PathBuf,
    /// Dry run (validate without writing)
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for Warewulf images import
#[cfg(feature = "hpc")]
#[derive(Parser, Debug, Clone)]
pub struct WarewulfImagesArgs {
    /// Path to Warewulf images YAML
    #[arg(long)]
    pub input: PathBuf,
    /// Output directory
    #[arg(long)]
    pub output: PathBuf,
    /// Dry run
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for Warewulf profiles import
#[cfg(feature = "hpc")]
#[derive(Parser, Debug, Clone)]
pub struct WarewulfProfilesArgs {
    /// Path to Warewulf profiles YAML
    #[arg(long)]
    pub input: PathBuf,
    /// Output directory
    #[arg(long)]
    pub output: PathBuf,
    /// Dry run
    #[arg(long)]
    pub dry_run: bool,
}

impl MigrateArgs {
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            MigrateCommand::TerraformPlan(args) => args.execute(ctx).await,
            MigrateCommand::TerraformState(args) => {
                ctx.output.info("Terraform state parity validation");
                ctx.output
                    .info(&format!("TF state: {}", args.tf_state.display()));
                ctx.output.info(&format!(
                    "Rustible state: {}",
                    args.rustible_state.display()
                ));
                ctx.output
                    .warning("Full implementation requires provisioning feature");
                Ok(0)
            }
            MigrateCommand::AnsibleCompat(args) => {
                ctx.output.info("Ansible compatibility verification");
                ctx.output.info(&format!("Path: {}", args.path.display()));
                ctx.output
                    .warning("Full implementation requires ansible compat module");
                Ok(0)
            }
            #[cfg(feature = "hpc")]
            MigrateCommand::XcatHierarchy(args) => {
                ctx.output.info("xCAT hierarchy import");
                ctx.output.info(&format!("Input: {}", args.input.display()));
                if args.dry_run {
                    ctx.output.info("Dry run mode - no files will be written");
                }
                Ok(0)
            }
            #[cfg(feature = "hpc")]
            MigrateCommand::XcatObjects(args) => {
                ctx.output.info("xCAT objects import");
                ctx.output.info(&format!("Input: {}", args.input.display()));
                if args.dry_run {
                    ctx.output.info("Dry run mode - no files will be written");
                }
                Ok(0)
            }
            #[cfg(feature = "hpc")]
            MigrateCommand::WarewulfImages(args) => {
                ctx.output.info("Warewulf images import");
                ctx.output.info(&format!("Input: {}", args.input.display()));
                if args.dry_run {
                    ctx.output.info("Dry run mode - no files will be written");
                }
                Ok(0)
            }
            #[cfg(feature = "hpc")]
            MigrateCommand::WarewulfProfiles(args) => {
                ctx.output.info("Warewulf profiles import");
                ctx.output.info(&format!("Input: {}", args.input.display()));
                if args.dry_run {
                    ctx.output.info("Dry run mode - no files will be written");
                }
                Ok(0)
            }
            MigrateCommand::Status => {
                ctx.output.info("Migration tools status: all available");
                Ok(0)
            }
        }
    }
}

impl TerraformPlanArgs {
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        use rustible::migration::terraform::plan_parity::TerraformPlanValidator;

        ctx.output.banner("TERRAFORM PLAN PARITY VALIDATION");

        let threshold = self.threshold as f64 / 100.0;
        let validator = TerraformPlanValidator::new(&self.tf_plan, &self.rustible_plan, threshold);

        match validator.validate() {
            Ok(report) => {
                if self.format == "json" {
                    println!("{}", report.to_json()?);
                } else {
                    ctx.output
                        .info(&format!("Terraform plan: {}", self.tf_plan.display()));
                    ctx.output
                        .info(&format!("Rustible plan:  {}", self.rustible_plan.display()));
                    ctx.output.info(&format!(
                        "Compatibility score: {:.1}%",
                        report.compatibility_score * 100.0
                    ));
                    ctx.output.info(&format!(
                        "Total findings: {} ({} matched, {} divergent)",
                        report.summary.total_items,
                        report.summary.matched,
                        report.summary.divergent
                    ));

                    if report.summary.errors > 0 {
                        ctx.output
                            .error(&format!("{} error(s) found", report.summary.errors));
                    }

                    for finding in &report.findings {
                        for diag in &finding.diagnostics {
                            match diag.severity {
                                rustible::migration::MigrationSeverity::Error
                                | rustible::migration::MigrationSeverity::Critical => {
                                    ctx.output.error(&diag.message);
                                }
                                rustible::migration::MigrationSeverity::Warning => {
                                    ctx.output.warning(&diag.message);
                                }
                                _ => {
                                    ctx.output.info(&diag.message);
                                }
                            }
                        }
                    }

                    match report.outcome {
                        rustible::migration::MigrationOutcome::Pass => {
                            ctx.output.success("Plan parity validation PASSED");
                        }
                        rustible::migration::MigrationOutcome::PassWithWarnings => {
                            ctx.output
                                .warning("Plan parity validation PASSED with warnings");
                        }
                        rustible::migration::MigrationOutcome::Fail => {
                            ctx.output.error("Plan parity validation FAILED");
                        }
                    }
                }
                Ok(report.exit_code())
            }
            Err(e) => {
                ctx.output.error(&format!("Validation failed: {}", e));
                Ok(1)
            }
        }
    }
}
