//! Check command - Dry-run mode
//!
//! This module implements the `check` subcommand for running playbooks in dry-run mode.

use super::{CommandContext, Runnable};
use crate::cli::commands::run::RunArgs;
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

/// Arguments for the check command
#[derive(Parser, Debug, Clone)]
pub struct CheckArgs {
    /// Path to the playbook file
    #[arg(required = true)]
    pub playbook: PathBuf,

    /// Tags to run (only tasks with these tags)
    #[arg(long, short = 't', action = clap::ArgAction::Append)]
    pub tags: Vec<String>,

    /// Tags to skip (skip tasks with these tags)
    #[arg(long, action = clap::ArgAction::Append)]
    pub skip_tags: Vec<String>,

    /// Start at a specific task
    #[arg(long)]
    pub start_at_task: Option<String>,

    /// Ask for vault password
    #[arg(long)]
    pub ask_vault_pass: bool,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,

    /// Ask for SSH password
    #[arg(short = 'k', long = "ask-pass")]
    pub ask_pass: bool,

    /// Become (sudo/su)
    #[arg(short = 'b', long)]
    pub r#become: bool,

    /// Become method (sudo, su, etc.)
    #[arg(long, default_value = "sudo")]
    pub become_method: String,

    /// Become user
    #[arg(long, default_value = "root")]
    pub become_user: String,

    /// Remote user
    #[arg(short = 'u', long)]
    pub user: Option<String>,

    /// Private key file
    #[arg(long)]
    pub private_key: Option<PathBuf>,
}

impl CheckArgs {
    /// Execute the check command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        // Force check mode
        ctx.check_mode = true;
        // diff_mode is set by global --diff flag already

        // Convert to RunArgs and execute
        let run_args = RunArgs {
            playbook: self.playbook.clone(),
            tags: self.tags.clone(),
            skip_tags: self.skip_tags.clone(),
            start_at_task: self.start_at_task.clone(),
            step: false,
            ask_vault_pass: self.ask_vault_pass,
            vault_password_file: self.vault_password_file.clone(),
            ask_pass: self.ask_pass,
            r#become: self.r#become,
            become_method: self.r#become_method.clone(),
            become_user: self.r#become_user.clone(),
            ask_become_pass: false,
            user: self.user.clone(),
            private_key: self.private_key.clone(),
            ssh_common_args: None,
            plan: false, // check mode doesn't need plan mode
        };

        ctx.output.banner("CHECK MODE - DRY RUN");
        ctx.output
            .warning("No changes will be made to the target systems");

        run_args.execute(ctx).await
    }
}

#[async_trait::async_trait]
impl Runnable for CheckArgs {
    async fn run(&self, ctx: &mut CommandContext) -> Result<i32> {
        self.execute(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_args_parsing() {
        let args = CheckArgs::try_parse_from(["check", "playbook.yml"]).unwrap();
        assert_eq!(args.playbook, PathBuf::from("playbook.yml"));
    }

    // Note: --diff is now a global flag defined in the main CLI struct,
    // not a local flag in CheckArgs
}
