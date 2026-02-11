//! Forensics Bundle CLI Command
//!
//! Provides the `forensics` subcommand for exporting and verifying
//! incident forensics bundles.
//!
//! ## Usage
//!
//! ```bash
//! # Export a forensics bundle
//! rustible forensics export --output bundle.json --from 2026-02-10T00:00:00Z --to 2026-02-11T00:00:00Z
//!
//! # Export with redaction and host filter
//! rustible forensics export --output bundle.json --host "web*" --redact
//!
//! # Verify a previously exported bundle
//! rustible forensics verify bundle.json
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use rustible::diagnostics::forensics::{
    BundleData, CollectorConfig, ForensicsBundle, ForensicsCollector, Redactor, TimeRange,
};

use super::CommandContext;

/// Arguments for the forensics command.
#[derive(Parser, Debug, Clone)]
pub struct ForensicsArgs {
    /// Forensics subcommand.
    #[command(subcommand)]
    pub command: ForensicsCommand,
}

/// Forensics subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum ForensicsCommand {
    /// Export a forensics bundle to a JSON file.
    Export {
        /// Output file path for the bundle.
        #[arg(short, long, default_value = "forensics-bundle.json")]
        output: PathBuf,

        /// Start of the time range (ISO-8601 timestamp).
        #[arg(long)]
        from: Option<String>,

        /// End of the time range (ISO-8601 timestamp).
        #[arg(long)]
        to: Option<String>,

        /// Filter data by host pattern.
        #[arg(long)]
        host: Option<String>,

        /// Apply built-in redaction rules to strip secrets.
        #[arg(long)]
        redact: bool,
    },

    /// Verify the structural integrity of a forensics bundle.
    Verify {
        /// Path to the bundle JSON file.
        path: PathBuf,
    },
}

impl ForensicsArgs {
    /// Execute the forensics subcommand.
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            ForensicsCommand::Export {
                output,
                from,
                to,
                host,
                redact,
            } => {
                ctx.output.banner("FORENSICS BUNDLE EXPORT");

                let time_range = match (from, to) {
                    (Some(f), Some(t)) => Some(TimeRange {
                        from: f.clone(),
                        to: t.clone(),
                    }),
                    _ => None,
                };

                let config = CollectorConfig {
                    include_audit: true,
                    include_state: true,
                    include_drift: true,
                    include_system_info: true,
                    time_range,
                    host_filter: host.clone(),
                };

                ctx.output.info("Collecting forensics data...");
                let collector = ForensicsCollector::new(config);
                let data = collector.collect();

                let json = if *redact {
                    ctx.output.info("Applying redaction rules...");
                    let rules = Redactor::builtin_rules();
                    let raw = ForensicsBundle::export_json(&data);
                    Redactor::redact(&raw, &rules)
                } else {
                    ForensicsBundle::export_json(&data)
                };

                // Verify before writing
                if !ForensicsBundle::verify_bundle(&json) && !*redact {
                    ctx.output
                        .error("Internal error: generated bundle failed verification");
                    return Ok(1);
                }

                std::fs::write(output, &json)?;

                ctx.output.success(&format!(
                    "Forensics bundle exported to: {}",
                    output.display()
                ));
                self.print_summary(&data, ctx);

                Ok(0)
            }
            ForensicsCommand::Verify { path } => {
                ctx.output.banner("FORENSICS BUNDLE VERIFY");

                if !path.exists() {
                    ctx.output
                        .error(&format!("Bundle not found: {}", path.display()));
                    return Ok(1);
                }

                let content = std::fs::read_to_string(path)?;

                if ForensicsBundle::verify_bundle(&content) {
                    ctx.output.success(&format!(
                        "Bundle is valid: {}",
                        path.display()
                    ));
                    Ok(0)
                } else {
                    ctx.output.error(&format!(
                        "Bundle verification failed: {}",
                        path.display()
                    ));
                    Ok(1)
                }
            }
        }
    }

    /// Print a summary of the collected bundle data.
    fn print_summary(&self, data: &BundleData, ctx: &CommandContext) {
        ctx.output.section("Bundle Summary");
        ctx.output.info(&format!(
            "  Version:         {}",
            data.manifest.version
        ));
        ctx.output.info(&format!(
            "  Created:         {}",
            data.manifest.created_at
        ));
        ctx.output.info(&format!(
            "  Audit events:    {}",
            data.manifest.contents.audit_events
        ));
        ctx.output.info(&format!(
            "  State snapshots: {}",
            data.manifest.contents.state_snapshots
        ));
        ctx.output.info(&format!(
            "  Drift reports:   {}",
            data.manifest.contents.drift_reports
        ));
        ctx.output.info(&format!(
            "  System info:     {}",
            data.manifest.contents.system_info
        ));
        if let Some(filter) = &data.manifest.host_filter {
            ctx.output
                .info(&format!("  Host filter:     {}", filter));
        }
    }
}
