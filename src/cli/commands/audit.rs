//! CLI command for audit log operations
//!
//! Provides subcommands for verifying and inspecting the immutable audit log.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use super::CommandContext;

/// Arguments for the `audit` command.
#[derive(Parser, Debug, Clone)]
pub struct AuditArgs {
    /// Audit subcommand
    #[command(subcommand)]
    pub command: AuditCommands,
}

/// Available audit subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum AuditCommands {
    /// Verify integrity of an audit log file
    Verify(VerifyArgs),

    /// Show audit system status and configuration
    Status,
}

/// Arguments for the `audit verify` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct VerifyArgs {
    /// Path to the audit log file to verify
    #[arg(long = "log-file")]
    pub log_file: PathBuf,
}

impl AuditArgs {
    /// Execute the audit command.
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            AuditCommands::Verify(args) => execute_verify(args, ctx).await,
            AuditCommands::Status => execute_status(ctx).await,
        }
    }
}

/// Verify the integrity of an audit log file.
async fn execute_verify(args: &VerifyArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("AUDIT LOG VERIFICATION");
    ctx.output
        .info(&format!("Verifying: {}", args.log_file.display()));

    if !args.log_file.exists() {
        ctx.output
            .error(&format!("Audit log file not found: {}", args.log_file.display()));
        return Ok(1);
    }

    let report = rustible::audit::verify::AuditVerifier::verify_file(&args.log_file)
        .map_err(|e| anyhow::anyhow!("Failed to read audit log: {}", e))?;

    if report.valid {
        ctx.output.success(&format!(
            "Audit log is VALID ({} entries verified)",
            report.entries_checked
        ));
        Ok(0)
    } else {
        ctx.output.error(&format!(
            "Audit log is INVALID: first bad entry at sequence {} ({} entries checked)",
            report.first_invalid.unwrap_or(0),
            report.entries_checked
        ));
        Ok(1)
    }
}

/// Display audit system status.
async fn execute_status(ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("AUDIT SYSTEM STATUS");
    ctx.output.info("Immutable audit log pipeline: enabled");
    ctx.output.info("Hash algorithm: BLAKE3");
    ctx.output.info("Storage backend: file (JSON-lines, append-only)");
    ctx.output
        .info("Sink pipeline: file, http (stub), syslog (stub)");
    Ok(0)
}
