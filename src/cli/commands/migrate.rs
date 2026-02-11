//! Migration CLI commands.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use super::CommandContext;

/// Arguments for the migrate command.
#[derive(Parser, Debug, Clone)]
pub struct MigrateArgs {
    #[command(subcommand)]
    pub command: MigrateCommand,
}

/// Available migration subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum MigrateCommand {
    /// Validate Terraform state parity with Rustible state
    #[command(name = "terraform-state")]
    TerraformState(TerraformStateArgs),
}

/// Arguments for terraform-state subcommand.
#[derive(Parser, Debug, Clone)]
pub struct TerraformStateArgs {
    /// Path to Terraform state file (terraform.tfstate)
    #[arg(long)]
    pub tf_state: PathBuf,

    /// Path to Rustible provisioning state file
    #[arg(long)]
    pub rustible_state: PathBuf,

    /// Pass threshold (0-100)
    #[arg(long, default_value = "80")]
    pub threshold: f64,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

impl MigrateArgs {
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            #[cfg(feature = "provisioning")]
            MigrateCommand::TerraformState(args) => execute_terraform_state(args, ctx),
            #[cfg(not(feature = "provisioning"))]
            MigrateCommand::TerraformState(_) => {
                ctx.output.error("Terraform state validation requires the 'provisioning' feature");
                Ok(1)
            }
        }
    }
}

#[cfg(feature = "provisioning")]
fn execute_terraform_state(args: &TerraformStateArgs, ctx: &mut CommandContext) -> Result<i32> {
    use rustible::migration::terraform::state_parity::TerraformStateValidator;

    ctx.output.banner("TERRAFORM STATE PARITY");

    let validator = TerraformStateValidator::new(args.threshold);
    let report = validator.validate(&args.tf_state, &args.rustible_state)?;

    if args.json {
        let json = serde_json::to_string_pretty(&report)?;
        println!("{}", json);
    } else {
        ctx.output.info(&format!("Source: {}", report.source));
        ctx.output.info(&format!("Target: {}", report.target));

        for finding in &report.findings {
            let status = match finding.status {
                rustible::migration::FindingStatus::Pass => "PASS",
                rustible::migration::FindingStatus::Fail => "FAIL",
                rustible::migration::FindingStatus::Partial => "PARTIAL",
                rustible::migration::FindingStatus::Skipped => "SKIP",
            };
            ctx.output.info(&format!("[{}] {}", status, finding.name));
            for diag in &finding.diagnostics {
                ctx.output.warning(&format!("  - {}", diag.message));
            }
        }

        if let Some(ref summary) = report.summary {
            ctx.output.section("Summary");
            ctx.output.info(&format!(
                "Score: {:.1}% ({}/{} passed)",
                summary.score, summary.passed, summary.total
            ));
        }

        match report.outcome {
            Some(rustible::migration::MigrationOutcome::Pass) => {
                ctx.output.success("State parity check PASSED");
            }
            Some(rustible::migration::MigrationOutcome::Fail) => {
                ctx.output.error("State parity check FAILED");
            }
            None => {}
        }
    }

    match report.outcome {
        Some(rustible::migration::MigrationOutcome::Pass) => Ok(0),
        _ => Ok(1),
    }
}
