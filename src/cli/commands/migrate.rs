//! Migration CLI commands.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use super::CommandContext;

#[derive(Parser, Debug, Clone)]
pub struct MigrateArgs {
    #[command(subcommand)]
    pub command: MigrateCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum MigrateCommand {
    /// Verify Ansible playbook compatibility with Rustible
    #[command(name = "ansible-compat")]
    AnsibleCompat(AnsibleCompatArgs),
}

#[derive(Parser, Debug, Clone)]
pub struct AnsibleCompatArgs {
    /// Path to Ansible playbook
    pub playbook: PathBuf,

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
            MigrateCommand::AnsibleCompat(args) => execute_ansible_compat(args, ctx),
        }
    }
}

fn execute_ansible_compat(args: &AnsibleCompatArgs, ctx: &mut CommandContext) -> Result<i32> {
    use rustible::migration::ansible::compat::AnsibleCompatVerifier;

    ctx.output.banner("ANSIBLE COMPATIBILITY CHECK");

    let content = std::fs::read_to_string(&args.playbook)?;
    let verifier = AnsibleCompatVerifier::new(args.threshold);
    let report = verifier.verify_playbook(&content)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
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
        if let Some(ref s) = report.summary {
            ctx.output.info(&format!("Score: {:.1}%", s.score));
        }
        match report.outcome {
            Some(rustible::migration::MigrationOutcome::Pass) => ctx.output.success("Compatibility check PASSED"),
            Some(rustible::migration::MigrationOutcome::Fail) => ctx.output.error("Compatibility check FAILED"),
            None => {}
        }
    }

    match report.outcome {
        Some(rustible::migration::MigrationOutcome::Pass) => Ok(0),
        _ => Ok(1),
    }
}
