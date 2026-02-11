//! CLI module for Rustible
//!
//! This module provides the command-line interface for Rustible,
//! including argument parsing, configuration loading, and subcommand handling.

pub mod change_detection;
pub mod commands;
pub mod completions;
pub mod diff;
pub mod interactive;
pub mod json_output;
pub mod output;
pub mod plan;
pub mod progress;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Rustible - An Ansible substitute written in Rust
///
/// A fast, safe, and modern configuration management and automation tool.
#[derive(Parser, Debug, Clone)]
#[command(name = "rustible")]
#[command(author = "Rustible Contributors")]
#[command(version)]
#[command(about = "An Ansible substitute written in Rust", long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Commands,

    /// Path to inventory file or directory
    #[arg(short = 'i', long, global = true, env = "RUSTIBLE_INVENTORY")]
    pub inventory: Option<PathBuf>,

    /// Extra variables (key=value or @file.yml)
    #[arg(short = 'e', long = "extra-vars", global = true, action = clap::ArgAction::Append)]
    pub extra_vars: Vec<String>,

    /// Verbosity level (-v, -vv, -vvv, -vvvv)
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Run in check mode (dry-run, don't make changes)
    #[arg(long = "check", global = true)]
    pub check_mode: bool,

    /// Run in diff mode (show differences)
    #[arg(long = "diff", global = true)]
    pub diff_mode: bool,

    /// Output format
    #[arg(long, global = true, default_value = "human")]
    pub output: OutputFormat,

    /// Limit execution to specific hosts (pattern)
    #[arg(short = 'l', long, global = true)]
    pub limit: Option<String>,

    /// Number of parallel processes (forks)
    #[arg(short = 'f', long, global = true, default_value = "5")]
    pub forks: usize,

    /// Connection timeout in seconds
    #[arg(long, global = true, default_value = "30")]
    pub timeout: u64,

    /// Path to configuration file
    #[arg(short = 'c', long, global = true, env = "RUSTIBLE_CONFIG")]
    pub config: Option<PathBuf>,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,
}

/// Output format for CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum OutputFormat {
    /// Human-readable output with colors
    #[default]
    Human,
    /// JSON output for scripting
    Json,
    /// YAML output
    Yaml,
    /// Minimal output (only errors)
    Minimal,
}

/// Available subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Run a playbook
    Run(commands::run::RunArgs),

    /// Run a playbook in check mode (dry-run)
    Check(commands::check::CheckArgs),

    /// List hosts in inventory
    #[command(name = "list-hosts")]
    ListHosts(commands::inventory::ListHostsArgs),

    /// List tasks in a playbook
    #[command(name = "list-tasks")]
    ListTasks(commands::inventory::ListTasksArgs),

    /// Vault operations (encrypt/decrypt secrets)
    Vault(commands::vault::VaultArgs),

    /// Galaxy operations (install/manage collections and roles)
    Galaxy(commands::galaxy::GalaxyArgs),

    /// Initialize a new Rustible project
    Init(InitArgs),

    /// Validate playbook syntax
    Validate(ValidateArgs),

    /// Infrastructure provisioning (Terraform-like)
    #[command(name = "provision")]
    Provision(commands::provision::ProvisionArgs),

    /// Detect configuration drift from desired state
    #[command(name = "drift")]
    Drift(commands::drift::DriftArgs),

    /// Manage lockfile for reproducible playbook execution
    #[command(name = "lock")]
    Lock(commands::lock::LockArgs),

    /// Terraform provisioner mode for local-exec integration
    #[command(name = "provisioner")]
    Provisioner(commands::provisioner::ProvisionerArgs),

    /// Manage providers (install, update, verify)
    #[command(name = "provider")]
    Provider(commands::provider::ProviderArgs),

    /// Explain an error code (like `rustc --explain`)
    #[command(name = "explain")]
    Explain(ExplainArgs),

    /// Manage state (list, show, pull, push, remove)
    #[command(name = "state")]
    State(commands::state::StateArgs),

    /// Agent operations (build, deploy, status)
    #[command(name = "agent")]
    Agent(AgentArgs),

    /// Show fleet infrastructure dashboard
    #[command(name = "fleet")]
    Fleet(commands::fleet::FleetArgs),

    /// Migration and compatibility tools
    #[cfg(feature = "provisioning")]
    #[command(name = "migrate")]
    Migrate(commands::migrate::MigrateArgs),
}

/// Arguments for agent command
#[derive(Parser, Debug, Clone)]
pub struct AgentArgs {
    /// Agent subcommand
    #[command(subcommand)]
    pub command: AgentCommand,
}

/// Agent subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum AgentCommand {
    /// Build agent binary for target architecture
    Build(AgentBuildArgs),

    /// Deploy agent to target hosts
    Deploy(AgentDeployArgs),

    /// Check agent status on hosts
    Status(AgentStatusArgs),

    /// Stop agent on hosts
    Stop(AgentStopArgs),
}

/// Arguments for agent build
#[derive(Parser, Debug, Clone)]
pub struct AgentBuildArgs {
    /// Target triple (e.g., x86_64-unknown-linux-gnu)
    #[arg(long, short = 't')]
    pub target: Option<String>,

    /// Build in debug mode (default: release)
    #[arg(long)]
    pub debug: bool,

    /// Output directory for agent binary
    #[arg(long, short = 'o', default_value = "target/agent")]
    pub output: PathBuf,

    /// Strip binary symbols for smaller size
    #[arg(long, default_value = "true")]
    pub strip: bool,
}

/// Arguments for agent deploy
#[derive(Parser, Debug, Clone)]
pub struct AgentDeployArgs {
    /// Path to agent binary (or use --build to build first)
    #[arg(long)]
    pub binary: Option<PathBuf>,

    /// Build agent before deploying
    #[arg(long)]
    pub build: bool,

    /// Target triple for build
    #[arg(long)]
    pub target: Option<String>,

    /// Remote path to install agent
    #[arg(long, default_value = "/usr/local/bin/rustible-agent")]
    pub remote_path: String,
}

/// Arguments for agent status
#[derive(Parser, Debug, Clone)]
pub struct AgentStatusArgs {
    /// Show detailed status
    #[arg(long, short = 'd')]
    pub detailed: bool,
}

/// Arguments for agent stop
#[derive(Parser, Debug, Clone)]
pub struct AgentStopArgs {
    /// Force stop without graceful shutdown
    #[arg(long)]
    pub force: bool,
}

/// Arguments for explain command
#[derive(Parser, Debug, Clone)]
pub struct ExplainArgs {
    /// Error code to explain (e.g., E0001)
    pub code: Option<String>,

    /// List all error codes
    #[arg(long)]
    pub list: bool,
}

/// Arguments for init command
#[derive(Parser, Debug, Clone)]
pub struct InitArgs {
    /// Directory to initialize (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Project template to use
    #[arg(long, default_value = "basic")]
    pub template: String,
}

/// Arguments for validate command
#[derive(Parser, Debug, Clone)]
pub struct ValidateArgs {
    /// Playbook file to validate
    pub playbook: PathBuf,
}

impl Cli {
    /// Parse command-line arguments
    pub fn parse_args() -> Self {
        Cli::parse()
    }

    /// Get the effective verbosity level (0-4)
    pub fn verbosity(&self) -> u8 {
        self.verbose.min(4)
    }

    /// Check if running in quiet mode
    #[allow(dead_code)]
    pub fn is_quiet(&self) -> bool {
        matches!(self.output, OutputFormat::Minimal)
    }

    /// Check if JSON output is requested
    pub fn is_json(&self) -> bool {
        matches!(self.output, OutputFormat::Json)
    }
}

/// Environment variable helper functions
pub mod env {
    use std::env;
    use std::path::PathBuf;

    /// Get the Rustible home directory
    #[allow(dead_code)]
    pub fn rustible_home() -> Option<PathBuf> {
        env::var("RUSTIBLE_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".rustible")))
    }

    /// Get the default inventory path
    #[allow(dead_code)]
    pub fn default_inventory() -> Option<PathBuf> {
        env::var("RUSTIBLE_INVENTORY").ok().map(PathBuf::from)
    }

    /// Get the vault password file path
    #[allow(dead_code)]
    pub fn vault_password_file() -> Option<PathBuf> {
        env::var("RUSTIBLE_VAULT_PASSWORD_FILE")
            .ok()
            .map(PathBuf::from)
    }

    /// Check if colors should be disabled
    #[allow(dead_code)]
    pub fn no_color() -> bool {
        env::var("NO_COLOR").is_ok() || env::var("RUSTIBLE_NO_COLOR").is_ok()
    }

    /// Get the SSH private key path
    #[allow(dead_code)]
    pub fn ssh_private_key() -> Option<PathBuf> {
        env::var("RUSTIBLE_SSH_KEY").ok().map(PathBuf::from)
    }

    /// Get the SSH password from environment
    #[allow(dead_code)]
    pub fn ssh_password() -> Option<String> {
        env::var("RUSTIBLE_SSH_PASSWORD")
            .ok()
            .or_else(|| env::var("RUSTIBLE_SSH_PASS").ok())
    }

    /// Get the remote user
    #[allow(dead_code)]
    pub fn remote_user() -> Option<String> {
        env::var("RUSTIBLE_REMOTE_USER").ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::try_parse_from(["rustible", "run", "playbook.yml"]).unwrap();
        assert!(matches!(cli.command, Commands::Run(_)));
    }

    #[test]
    fn test_verbosity() {
        let cli = Cli::try_parse_from(["rustible", "-vvv", "run", "playbook.yml"]).unwrap();
        assert_eq!(cli.verbosity(), 3);
    }

    #[test]
    fn test_extra_vars() {
        let cli = Cli::try_parse_from([
            "rustible",
            "-e",
            "key1=value1",
            "-e",
            "key2=value2",
            "run",
            "playbook.yml",
        ])
        .unwrap();
        assert_eq!(cli.extra_vars.len(), 2);
    }
}
