//! Provision command for infrastructure provisioning (Terraform-like)
//!
//! This module provides CLI commands for infrastructure provisioning,
//! including plan, apply, destroy, import, show, refresh, and init operations.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use super::CommandContext;

#[cfg(feature = "provisioning")]
use rustible::provisioning::{InfrastructureConfig, ProvisioningExecutor, ResourceId};
#[cfg(feature = "provisioning")]
use rustible::provisioning::executor::ExecutorConfig;

/// Arguments for the provision command
#[derive(Parser, Debug, Clone)]
pub struct ProvisionArgs {
    /// Provisioning subcommand
    #[command(subcommand)]
    pub command: ProvisionCommands,
}

/// Provisioning subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum ProvisionCommands {
    /// Generate an execution plan
    Plan(PlanArgs),

    /// Apply infrastructure changes
    Apply(ApplyArgs),

    /// Destroy infrastructure
    Destroy(DestroyArgs),

    /// Import an existing resource
    Import(ImportArgs),

    /// Show current state
    Show(ShowArgs),

    /// Refresh state from cloud
    Refresh(RefreshArgs),

    /// Initialize provisioning for a project
    Init(InitArgs),
}

/// Arguments for plan command
#[derive(Parser, Debug, Clone)]
pub struct PlanArgs {
    /// Path to infrastructure configuration file
    #[arg(long, default_value = "infrastructure.rustible.yml")]
    pub config_file: PathBuf,

    /// Output plan to file
    #[arg(short = 'o', long)]
    pub out: Option<PathBuf>,

    /// Target specific resources
    #[arg(short = 't', long)]
    pub target: Vec<String>,

    /// Refresh state before planning
    #[arg(long, default_value = "true")]
    pub refresh: bool,

    /// Path to state file
    #[arg(long)]
    pub state: Option<PathBuf>,

    /// Generate destroy plan
    #[arg(long)]
    pub destroy: bool,
}

/// Arguments for apply command
#[derive(Parser, Debug, Clone)]
pub struct ApplyArgs {
    /// Path to infrastructure configuration file
    #[arg(long, default_value = "infrastructure.rustible.yml")]
    pub config_file: PathBuf,

    /// Auto-approve changes without confirmation
    #[arg(long)]
    pub auto_approve: bool,

    /// Target specific resources
    #[arg(short = 't', long)]
    pub target: Vec<String>,

    /// Maximum parallel operations
    #[arg(long, default_value = "10")]
    pub parallelism: usize,

    /// Path to state file
    #[arg(long)]
    pub state: Option<PathBuf>,

    /// Skip state backup
    #[arg(long)]
    pub no_backup: bool,

    /// Skip state locking
    #[arg(long)]
    pub no_lock: bool,
}

/// Arguments for destroy command
#[derive(Parser, Debug, Clone)]
pub struct DestroyArgs {
    /// Path to infrastructure configuration file
    #[arg(long, default_value = "infrastructure.rustible.yml")]
    pub config_file: PathBuf,

    /// Auto-approve destruction without confirmation
    #[arg(long)]
    pub auto_approve: bool,

    /// Target specific resources
    #[arg(short = 't', long)]
    pub target: Vec<String>,

    /// Path to state file
    #[arg(long)]
    pub state: Option<PathBuf>,
}

/// Arguments for import command
#[derive(Parser, Debug, Clone)]
pub struct ImportArgs {
    /// Path to infrastructure configuration file
    #[arg(long, default_value = "infrastructure.rustible.yml")]
    pub config_file: PathBuf,

    /// Resource address (e.g., aws_vpc.main)
    pub address: String,

    /// Cloud provider resource ID
    pub id: String,

    /// Path to state file
    #[arg(long)]
    pub state: Option<PathBuf>,
}

/// Arguments for show command
#[derive(Parser, Debug, Clone)]
pub struct ShowArgs {
    /// Path to state file
    #[arg(long, default_value = ".rustible/provisioning.state.json")]
    pub state: PathBuf,

    /// Show specific resource address
    #[arg(short = 'a', long)]
    pub address: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

/// Arguments for refresh command
#[derive(Parser, Debug, Clone)]
pub struct RefreshArgs {
    /// Path to infrastructure configuration file
    #[arg(long, default_value = "infrastructure.rustible.yml")]
    pub config_file: PathBuf,

    /// Target specific resources
    #[arg(short = 't', long)]
    pub target: Vec<String>,

    /// Path to state file
    #[arg(long)]
    pub state: Option<PathBuf>,
}

/// Arguments for init command
#[derive(Parser, Debug, Clone)]
pub struct InitArgs {
    /// Directory to initialize
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Backend type (local, s3, gcs, azurerm, http)
    #[arg(long, default_value = "local")]
    pub backend: String,

    /// Force reconfiguration even if already initialized
    #[arg(long)]
    pub reconfigure: bool,
}

impl PlanArgs {
    /// Execute the plan command
    #[cfg(feature = "provisioning")]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output.banner("INFRASTRUCTURE PLAN");

        if !self.config_file.exists() {
            ctx.output.error(&format!(
                "Configuration file not found: {}",
                self.config_file.display()
            ));
            return Ok(1);
        }

        ctx.output.info(&format!(
            "Loading configuration from: {}",
            self.config_file.display()
        ));

        let config = InfrastructureConfig::from_file(&self.config_file).await?;

        let mut executor_config = ExecutorConfig::default();
        executor_config.refresh_before_plan = self.refresh;
        executor_config.targets = self.target.clone();
        if let Some(ref state_path) = self.state {
            executor_config.state_path = state_path.clone();
        }

        let executor = ProvisioningExecutor::with_config(config, executor_config).await?;

        let plan = if self.destroy {
            executor.plan_destroy().await?
        } else {
            executor.plan().await?
        };

        // Display plan summary
        ctx.output.section("Execution Plan");
        println!("{}", plan.summary());

        // Save plan to file if requested
        if let Some(ref out_path) = self.out {
            let plan_json = serde_json::to_string_pretty(&plan)?;
            std::fs::write(out_path, plan_json)?;
            ctx.output
                .info(&format!("Plan saved to: {}", out_path.display()));
        }

        if plan.has_changes() {
            Ok(2) // Exit code 2 indicates changes pending
        } else {
            Ok(0)
        }
    }

    #[cfg(not(feature = "provisioning"))]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output
            .error("Provisioning feature not enabled. Rebuild with --features provisioning");
        Ok(1)
    }
}

impl ApplyArgs {
    /// Execute the apply command
    #[cfg(feature = "provisioning")]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output.banner("INFRASTRUCTURE APPLY");

        if !self.config_file.exists() {
            ctx.output.error(&format!(
                "Configuration file not found: {}",
                self.config_file.display()
            ));
            return Ok(1);
        }

        let config = InfrastructureConfig::from_file(&self.config_file).await?;

        let mut executor_config = ExecutorConfig::default();
        executor_config.auto_approve = self.auto_approve;
        executor_config.parallelism = self.parallelism;
        executor_config.targets = self.target.clone();
        executor_config.backup_state = !self.no_backup;
        executor_config.lock_state = !self.no_lock;
        if let Some(ref state_path) = self.state {
            executor_config.state_path = state_path.clone();
        }

        let executor = ProvisioningExecutor::with_config(config, executor_config).await?;

        // Generate plan
        let plan = executor.plan().await?;

        if !plan.has_changes() {
            ctx.output
                .info("No changes to apply. Infrastructure is up-to-date.");
            return Ok(0);
        }

        // Display plan
        ctx.output.section("Execution Plan");
        println!("{}", plan.summary());

        // Confirm if not auto-approved
        if !self.auto_approve {
            ctx.output.warning("Do you want to perform these actions?");
            ctx.output.info("Only 'yes' will be accepted to approve.");

            let mut input = String::new();
            print!("  Enter a value: ");
            use std::io::Write;
            std::io::stdout().flush()?;
            std::io::stdin().read_line(&mut input)?;

            if input.trim() != "yes" {
                ctx.output.info("Apply cancelled.");
                return Ok(0);
            }
        }

        // Apply changes
        ctx.output.section("Applying Changes");
        let result = executor.apply(&plan).await?;

        println!("{}", result.summary());

        if result.success {
            Ok(0)
        } else {
            Ok(1)
        }
    }

    #[cfg(not(feature = "provisioning"))]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output
            .error("Provisioning feature not enabled. Rebuild with --features provisioning");
        Ok(1)
    }
}

impl DestroyArgs {
    /// Execute the destroy command
    #[cfg(feature = "provisioning")]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output.banner("INFRASTRUCTURE DESTROY");

        if !self.config_file.exists() {
            ctx.output.error(&format!(
                "Configuration file not found: {}",
                self.config_file.display()
            ));
            return Ok(1);
        }

        let config = InfrastructureConfig::from_file(&self.config_file).await?;

        let mut executor_config = ExecutorConfig::default();
        executor_config.auto_approve = self.auto_approve;
        executor_config.targets = self.target.clone();
        if let Some(ref state_path) = self.state {
            executor_config.state_path = state_path.clone();
        }

        let executor = ProvisioningExecutor::with_config(config, executor_config).await?;

        // Generate destroy plan
        let plan = executor.plan_destroy().await?;

        if !plan.has_changes() {
            ctx.output.info("No resources to destroy.");
            return Ok(0);
        }

        // Display plan
        ctx.output.section("Destroy Plan");
        println!("{}", plan.summary());

        // Confirm if not auto-approved
        if !self.auto_approve {
            ctx.output
                .warning("Do you really want to destroy all resources?");
            ctx.output.info("Only 'yes' will be accepted to approve.");

            let mut input = String::new();
            print!("  Enter a value: ");
            use std::io::Write;
            std::io::stdout().flush()?;
            std::io::stdin().read_line(&mut input)?;

            if input.trim() != "yes" {
                ctx.output.info("Destroy cancelled.");
                return Ok(0);
            }
        }

        // Apply destroy plan
        ctx.output.section("Destroying Resources");
        let result = executor.apply(&plan).await?;

        println!("{}", result.summary());

        if result.success {
            Ok(0)
        } else {
            Ok(1)
        }
    }

    #[cfg(not(feature = "provisioning"))]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output
            .error("Provisioning feature not enabled. Rebuild with --features provisioning");
        Ok(1)
    }
}

impl ImportArgs {
    /// Execute the import command
    #[cfg(feature = "provisioning")]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output.banner("INFRASTRUCTURE IMPORT");

        if !self.config_file.exists() {
            ctx.output.error(&format!(
                "Configuration file not found: {}",
                self.config_file.display()
            ));
            return Ok(1);
        }

        // Parse resource address
        let resource_id = ResourceId::from_address(&self.address).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid resource address: {}. Expected format: resource_type.name",
                self.address
            )
        })?;

        ctx.output
            .info(&format!("Importing {} as {}", self.id, self.address));

        let config = InfrastructureConfig::from_file(&self.config_file).await?;

        let mut executor_config = ExecutorConfig::default();
        if let Some(ref state_path) = self.state {
            executor_config.state_path = state_path.clone();
        }

        let executor = ProvisioningExecutor::with_config(config, executor_config).await?;

        let resource_state = executor
            .import(&resource_id.resource_type, &resource_id.name, &self.id)
            .await?;

        ctx.output.section("Import Successful");
        ctx.output
            .info(&format!("Resource: {}", resource_state.id.address()));
        ctx.output
            .info(&format!("Cloud ID: {}", resource_state.cloud_id));
        ctx.output
            .info(&format!("Provider: {}", resource_state.provider));

        Ok(0)
    }

    #[cfg(not(feature = "provisioning"))]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output
            .error("Provisioning feature not enabled. Rebuild with --features provisioning");
        Ok(1)
    }
}

impl ShowArgs {
    /// Execute the show command
    #[cfg(feature = "provisioning")]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        use rustible::provisioning::ProvisioningState;

        if !self.state.exists() {
            ctx.output
                .warning("No state file found. Run 'provision init' or 'provision apply' first.");
            return Ok(0);
        }

        let state = ProvisioningState::load(&self.state).await?;

        if let Some(ref address) = self.address {
            // Show specific resource
            let resource_id = ResourceId::from_address(address)
                .ok_or_else(|| anyhow::anyhow!("Invalid resource address: {}", address))?;

            if let Some(resource) = state.get_resource(&resource_id) {
                if self.json {
                    println!("{}", serde_json::to_string_pretty(resource)?);
                } else {
                    ctx.output.banner(&format!("Resource: {}", address));
                    ctx.output.info(&format!("Cloud ID: {}", resource.cloud_id));
                    ctx.output.info(&format!("Provider: {}", resource.provider));
                    ctx.output.info(&format!("Tainted: {}", resource.tainted));
                    ctx.output.section("Attributes");
                    println!("{}", serde_json::to_string_pretty(&resource.attributes)?);
                }
            } else {
                ctx.output
                    .error(&format!("Resource not found: {}", address));
                return Ok(1);
            }
        } else {
            // Show all state
            if self.json {
                println!("{}", serde_json::to_string_pretty(&state)?);
            } else {
                ctx.output.banner("INFRASTRUCTURE STATE");
                println!("{}", state.summary());
            }
        }

        Ok(0)
    }

    #[cfg(not(feature = "provisioning"))]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output
            .error("Provisioning feature not enabled. Rebuild with --features provisioning");
        Ok(1)
    }
}

impl RefreshArgs {
    /// Execute the refresh command
    #[cfg(feature = "provisioning")]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output.banner("INFRASTRUCTURE REFRESH");

        if !self.config_file.exists() {
            ctx.output.error(&format!(
                "Configuration file not found: {}",
                self.config_file.display()
            ));
            return Ok(1);
        }

        ctx.output.info("Refreshing state from cloud providers...");

        let config = InfrastructureConfig::from_file(&self.config_file).await?;

        let mut executor_config = ExecutorConfig::default();
        executor_config.targets = self.target.clone();
        if let Some(ref state_path) = self.state {
            executor_config.state_path = state_path.clone();
        }

        let executor = ProvisioningExecutor::with_config(config, executor_config).await?;
        executor.refresh().await?;

        ctx.output.section("State refreshed successfully");
        println!("{}", executor.show());

        Ok(0)
    }

    #[cfg(not(feature = "provisioning"))]
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output
            .error("Provisioning feature not enabled. Rebuild with --features provisioning");
        Ok(1)
    }
}

impl InitArgs {
    /// Execute the init command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        ctx.output.banner("PROVISIONING INIT");

        // Create .rustible directory if needed
        let rustible_dir = self.path.join(".rustible");
        if !rustible_dir.exists() {
            std::fs::create_dir_all(&rustible_dir)?;
            ctx.output
                .info(&format!("Created: {}/", rustible_dir.display()));
        }

        // Create sample infrastructure config if not exists
        let config_path = self.path.join("infrastructure.rustible.yml");
        if !config_path.exists() || self.reconfigure {
            let sample_config = r#"# Rustible Infrastructure Configuration
# Terraform-like declarative infrastructure provisioning

# Provider configuration
providers:
  aws:
    region: us-east-1
    # profile: default  # Optional: AWS CLI profile

# Variables
variables:
  environment: development
  project_name: my-project

# Local values
locals:
  common_tags:
    Environment: "{{ variables.environment }}"
    Project: "{{ variables.project_name }}"
    ManagedBy: rustible

# Resources
resources:
  # Example VPC
  # aws_vpc:
  #   main:
  #     cidr_block: "10.0.0.0/16"
  #     enable_dns_hostnames: true
  #     enable_dns_support: true
  #     tags: "{{ locals.common_tags }}"

  # Example Subnet
  # aws_subnet:
  #   public_a:
  #     vpc_id: "{{ resources.aws_vpc.main.id }}"
  #     cidr_block: "10.0.1.0/24"
  #     availability_zone: us-east-1a
  #     map_public_ip_on_launch: true
  #     tags: "{{ locals.common_tags }}"

# Outputs
outputs: {}
"#;
            std::fs::write(&config_path, sample_config)?;
            ctx.output
                .info(&format!("Created: {}", config_path.display()));
        }

        // Create empty state file if not exists
        let state_path = rustible_dir.join("provisioning.state.json");
        if !state_path.exists() {
            let empty_state = r#"{
  "version": "2.0.0",
  "serial": 0,
  "resources": {},
  "outputs": {}
}"#;
            std::fs::write(&state_path, empty_state)?;
            ctx.output
                .info(&format!("Created: {}", state_path.display()));
        }

        ctx.output.section("Provisioning initialized successfully!");
        ctx.output.info(&format!("Backend: {}", self.backend));
        ctx.output.info("");
        ctx.output.info("Next steps:");
        ctx.output
            .info("  1. Edit infrastructure.rustible.yml to define your resources");
        ctx.output
            .info("  2. Run 'rustible provision plan' to see what will be created");
        ctx.output
            .info("  3. Run 'rustible provision apply' to create resources");

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // Test wrapper structs for parsing subcommands
    #[derive(Parser, Debug)]
    #[command(name = "test")]
    struct TestPlanCli {
        #[command(flatten)]
        args: PlanArgs,
    }

    #[derive(Parser, Debug)]
    #[command(name = "test")]
    struct TestApplyCli {
        #[command(flatten)]
        args: ApplyArgs,
    }

    #[derive(Parser, Debug)]
    #[command(name = "test")]
    struct TestDestroyCli {
        #[command(flatten)]
        args: DestroyArgs,
    }

    #[derive(Parser, Debug)]
    #[command(name = "test")]
    struct TestImportCli {
        #[command(flatten)]
        args: ImportArgs,
    }

    #[derive(Parser, Debug)]
    #[command(name = "test")]
    struct TestShowCli {
        #[command(flatten)]
        args: ShowArgs,
    }

    #[derive(Parser, Debug)]
    #[command(name = "test")]
    struct TestRefreshCli {
        #[command(flatten)]
        args: RefreshArgs,
    }

    #[derive(Parser, Debug)]
    #[command(name = "test")]
    struct TestInitCli {
        #[command(flatten)]
        args: InitArgs,
    }

    // ==================== PlanArgs Tests ====================

    #[test]
    fn test_plan_args_default() {
        let cli = TestPlanCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("infrastructure.rustible.yml")
        );
        assert!(cli.args.out.is_none());
        assert!(cli.args.target.is_empty());
        assert!(cli.args.refresh);
        assert!(cli.args.state.is_none());
        assert!(!cli.args.destroy);
    }

    #[test]
    fn test_plan_args_with_config_file() {
        let cli =
            TestPlanCli::try_parse_from(["test", "--config-file", "custom.yml"]).unwrap();
        assert_eq!(cli.args.config_file, PathBuf::from("custom.yml"));
    }

    #[test]
    fn test_plan_args_with_output_file() {
        let cli = TestPlanCli::try_parse_from(["test", "-o", "plan.json"]).unwrap();
        assert_eq!(cli.args.out, Some(PathBuf::from("plan.json")));

        let cli = TestPlanCli::try_parse_from(["test", "--out", "plan.json"]).unwrap();
        assert_eq!(cli.args.out, Some(PathBuf::from("plan.json")));
    }

    #[test]
    fn test_plan_args_with_targets() {
        let cli = TestPlanCli::try_parse_from([
            "test",
            "-t",
            "aws_vpc.main",
            "-t",
            "aws_subnet.public",
        ])
        .unwrap();
        assert_eq!(cli.args.target.len(), 2);
        assert_eq!(cli.args.target[0], "aws_vpc.main");
        assert_eq!(cli.args.target[1], "aws_subnet.public");
    }

    #[test]
    fn test_plan_args_with_target_long() {
        let cli = TestPlanCli::try_parse_from([
            "test",
            "--target",
            "aws_vpc.main",
            "--target",
            "aws_subnet.public",
        ])
        .unwrap();
        assert_eq!(cli.args.target.len(), 2);
    }

    #[test]
    fn test_plan_args_refresh_flag_present() {
        // With default_value = "true", the --refresh flag is already true by default
        // and is treated as a boolean flag without taking a value
        // The flag being present doesn't change the behavior (it's already true)
        let cli = TestPlanCli::try_parse_from(["test", "--refresh"]).unwrap();
        assert!(cli.args.refresh);
    }

    #[test]
    fn test_plan_args_refresh_with_value_fails() {
        // Clap treats default_value="true" bool as a flag without value
        // Passing a value like --refresh=false should fail
        let result = TestPlanCli::try_parse_from(["test", "--refresh=false"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_args_with_state() {
        let cli = TestPlanCli::try_parse_from(["test", "--state", "/custom/state.json"]).unwrap();
        assert_eq!(cli.args.state, Some(PathBuf::from("/custom/state.json")));
    }

    #[test]
    fn test_plan_args_destroy_mode() {
        let cli = TestPlanCli::try_parse_from(["test", "--destroy"]).unwrap();
        assert!(cli.args.destroy);
    }

    #[test]
    fn test_plan_args_combined() {
        let cli = TestPlanCli::try_parse_from([
            "test",
            "--config-file",
            "prod.yml",
            "-o",
            "plan.json",
            "-t",
            "aws_vpc.main",
            "--state",
            "state.json",
            "--destroy",
        ])
        .unwrap();
        assert_eq!(cli.args.config_file, PathBuf::from("prod.yml"));
        assert_eq!(cli.args.out, Some(PathBuf::from("plan.json")));
        assert_eq!(cli.args.target, vec!["aws_vpc.main"]);
        assert_eq!(cli.args.state, Some(PathBuf::from("state.json")));
        assert!(cli.args.destroy);
    }

    // ==================== ApplyArgs Tests ====================

    #[test]
    fn test_apply_args_default() {
        let cli = TestApplyCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("infrastructure.rustible.yml")
        );
        assert!(!cli.args.auto_approve);
        assert!(cli.args.target.is_empty());
        assert_eq!(cli.args.parallelism, 10);
        assert!(cli.args.state.is_none());
        assert!(!cli.args.no_backup);
        assert!(!cli.args.no_lock);
    }

    #[test]
    fn test_apply_args_auto_approve() {
        let cli = TestApplyCli::try_parse_from(["test", "--auto-approve"]).unwrap();
        assert!(cli.args.auto_approve);
    }

    #[test]
    fn test_apply_args_with_targets() {
        let cli = TestApplyCli::try_parse_from([
            "test",
            "-t",
            "aws_vpc.main",
            "-t",
            "aws_subnet.public",
        ])
        .unwrap();
        assert_eq!(cli.args.target.len(), 2);
        assert_eq!(cli.args.target[0], "aws_vpc.main");
        assert_eq!(cli.args.target[1], "aws_subnet.public");
    }

    #[test]
    fn test_apply_args_parallelism() {
        let cli = TestApplyCli::try_parse_from(["test", "--parallelism", "5"]).unwrap();
        assert_eq!(cli.args.parallelism, 5);
    }

    #[test]
    fn test_apply_args_parallelism_high() {
        let cli = TestApplyCli::try_parse_from(["test", "--parallelism", "100"]).unwrap();
        assert_eq!(cli.args.parallelism, 100);
    }

    #[test]
    fn test_apply_args_no_backup() {
        let cli = TestApplyCli::try_parse_from(["test", "--no-backup"]).unwrap();
        assert!(cli.args.no_backup);
    }

    #[test]
    fn test_apply_args_no_lock() {
        let cli = TestApplyCli::try_parse_from(["test", "--no-lock"]).unwrap();
        assert!(cli.args.no_lock);
    }

    #[test]
    fn test_apply_args_with_state() {
        let cli = TestApplyCli::try_parse_from(["test", "--state", "/custom/state.json"]).unwrap();
        assert_eq!(cli.args.state, Some(PathBuf::from("/custom/state.json")));
    }

    #[test]
    fn test_apply_args_combined() {
        let cli = TestApplyCli::try_parse_from([
            "test",
            "--config-file",
            "prod.yml",
            "--auto-approve",
            "-t",
            "aws_vpc.main",
            "--parallelism",
            "20",
            "--state",
            "state.json",
            "--no-backup",
            "--no-lock",
        ])
        .unwrap();
        assert_eq!(cli.args.config_file, PathBuf::from("prod.yml"));
        assert!(cli.args.auto_approve);
        assert_eq!(cli.args.target, vec!["aws_vpc.main"]);
        assert_eq!(cli.args.parallelism, 20);
        assert_eq!(cli.args.state, Some(PathBuf::from("state.json")));
        assert!(cli.args.no_backup);
        assert!(cli.args.no_lock);
    }

    // ==================== DestroyArgs Tests ====================

    #[test]
    fn test_destroy_args_default() {
        let cli = TestDestroyCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("infrastructure.rustible.yml")
        );
        assert!(!cli.args.auto_approve);
        assert!(cli.args.target.is_empty());
        assert!(cli.args.state.is_none());
    }

    #[test]
    fn test_destroy_args_auto_approve() {
        let cli = TestDestroyCli::try_parse_from(["test", "--auto-approve"]).unwrap();
        assert!(cli.args.auto_approve);
    }

    #[test]
    fn test_destroy_args_with_targets() {
        let cli = TestDestroyCli::try_parse_from([
            "test",
            "-t",
            "aws_vpc.main",
            "-t",
            "aws_subnet.public",
        ])
        .unwrap();
        assert_eq!(cli.args.target.len(), 2);
    }

    #[test]
    fn test_destroy_args_with_state() {
        let cli =
            TestDestroyCli::try_parse_from(["test", "--state", "/custom/state.json"]).unwrap();
        assert_eq!(cli.args.state, Some(PathBuf::from("/custom/state.json")));
    }

    #[test]
    fn test_destroy_args_combined() {
        let cli = TestDestroyCli::try_parse_from([
            "test",
            "--config-file",
            "prod.yml",
            "--auto-approve",
            "-t",
            "aws_vpc.main",
            "--state",
            "state.json",
        ])
        .unwrap();
        assert_eq!(cli.args.config_file, PathBuf::from("prod.yml"));
        assert!(cli.args.auto_approve);
        assert_eq!(cli.args.target, vec!["aws_vpc.main"]);
        assert_eq!(cli.args.state, Some(PathBuf::from("state.json")));
    }

    // ==================== ImportArgs Tests ====================

    #[test]
    fn test_import_args_required_positional() {
        let cli =
            TestImportCli::try_parse_from(["test", "aws_vpc.main", "vpc-12345"]).unwrap();
        assert_eq!(cli.args.address, "aws_vpc.main");
        assert_eq!(cli.args.id, "vpc-12345");
    }

    #[test]
    fn test_import_args_with_config() {
        let cli = TestImportCli::try_parse_from([
            "test",
            "--config-file",
            "custom.yml",
            "aws_vpc.main",
            "vpc-12345",
        ])
        .unwrap();
        assert_eq!(cli.args.config_file, PathBuf::from("custom.yml"));
        assert_eq!(cli.args.address, "aws_vpc.main");
        assert_eq!(cli.args.id, "vpc-12345");
    }

    #[test]
    fn test_import_args_with_state() {
        let cli = TestImportCli::try_parse_from([
            "test",
            "--state",
            "custom-state.json",
            "aws_vpc.main",
            "vpc-12345",
        ])
        .unwrap();
        assert_eq!(
            cli.args.state,
            Some(PathBuf::from("custom-state.json"))
        );
    }

    #[test]
    fn test_import_args_missing_address() {
        let result = TestImportCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_import_args_missing_id() {
        let result = TestImportCli::try_parse_from(["test", "aws_vpc.main"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_import_args_various_resources() {
        // Test different resource types
        let resources = [
            ("aws_vpc.main", "vpc-12345"),
            ("aws_instance.web", "i-abcdef123"),
            ("aws_s3_bucket.data", "my-data-bucket"),
            ("google_compute_instance.server", "projects/proj/zones/us-west1-a/instances/server"),
            ("azurerm_virtual_network.vnet", "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/vnet"),
        ];

        for (address, id) in resources {
            let cli =
                TestImportCli::try_parse_from(["test", address, id]).unwrap();
            assert_eq!(cli.args.address, address);
            assert_eq!(cli.args.id, id);
        }
    }

    // ==================== ShowArgs Tests ====================

    #[test]
    fn test_show_args_default() {
        let cli = TestShowCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.state,
            PathBuf::from(".rustible/provisioning.state.json")
        );
        assert!(cli.args.address.is_none());
        assert!(!cli.args.json);
    }

    #[test]
    fn test_show_args_with_state() {
        let cli = TestShowCli::try_parse_from(["test", "--state", "custom-state.json"]).unwrap();
        assert_eq!(cli.args.state, PathBuf::from("custom-state.json"));
    }

    #[test]
    fn test_show_args_with_address() {
        let cli = TestShowCli::try_parse_from(["test", "-a", "aws_vpc.main"]).unwrap();
        assert_eq!(cli.args.address, Some("aws_vpc.main".to_string()));

        let cli = TestShowCli::try_parse_from(["test", "--address", "aws_vpc.main"]).unwrap();
        assert_eq!(cli.args.address, Some("aws_vpc.main".to_string()));
    }

    #[test]
    fn test_show_args_json_output() {
        let cli = TestShowCli::try_parse_from(["test", "--json"]).unwrap();
        assert!(cli.args.json);
    }

    #[test]
    fn test_show_args_combined() {
        let cli = TestShowCli::try_parse_from([
            "test",
            "--state",
            "prod.state.json",
            "-a",
            "aws_vpc.main",
            "--json",
        ])
        .unwrap();
        assert_eq!(cli.args.state, PathBuf::from("prod.state.json"));
        assert_eq!(cli.args.address, Some("aws_vpc.main".to_string()));
        assert!(cli.args.json);
    }

    // ==================== RefreshArgs Tests ====================

    #[test]
    fn test_refresh_args_default() {
        let cli = TestRefreshCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("infrastructure.rustible.yml")
        );
        assert!(cli.args.target.is_empty());
        assert!(cli.args.state.is_none());
    }

    #[test]
    fn test_refresh_args_with_config() {
        let cli =
            TestRefreshCli::try_parse_from(["test", "--config-file", "custom.yml"]).unwrap();
        assert_eq!(cli.args.config_file, PathBuf::from("custom.yml"));
    }

    #[test]
    fn test_refresh_args_with_targets() {
        let cli = TestRefreshCli::try_parse_from([
            "test",
            "-t",
            "aws_vpc.main",
            "-t",
            "aws_subnet.public",
        ])
        .unwrap();
        assert_eq!(cli.args.target.len(), 2);
    }

    #[test]
    fn test_refresh_args_with_state() {
        let cli =
            TestRefreshCli::try_parse_from(["test", "--state", "custom-state.json"]).unwrap();
        assert_eq!(
            cli.args.state,
            Some(PathBuf::from("custom-state.json"))
        );
    }

    #[test]
    fn test_refresh_args_combined() {
        let cli = TestRefreshCli::try_parse_from([
            "test",
            "--config-file",
            "prod.yml",
            "-t",
            "aws_vpc.main",
            "--state",
            "prod.state.json",
        ])
        .unwrap();
        assert_eq!(cli.args.config_file, PathBuf::from("prod.yml"));
        assert_eq!(cli.args.target, vec!["aws_vpc.main"]);
        assert_eq!(cli.args.state, Some(PathBuf::from("prod.state.json")));
    }

    // ==================== InitArgs Tests ====================

    #[test]
    fn test_init_args_default() {
        let cli = TestInitCli::try_parse_from(["test"]).unwrap();
        assert_eq!(cli.args.path, PathBuf::from("."));
        assert_eq!(cli.args.backend, "local");
        assert!(!cli.args.reconfigure);
    }

    #[test]
    fn test_init_args_with_path() {
        let cli = TestInitCli::try_parse_from(["test", "/path/to/project"]).unwrap();
        assert_eq!(cli.args.path, PathBuf::from("/path/to/project"));
    }

    #[test]
    fn test_init_args_with_backend() {
        let cli = TestInitCli::try_parse_from(["test", "--backend", "s3"]).unwrap();
        assert_eq!(cli.args.backend, "s3");
    }

    #[test]
    fn test_init_args_various_backends() {
        let backends = ["local", "s3", "gcs", "azurerm", "http"];
        for backend in backends {
            let cli =
                TestInitCli::try_parse_from(["test", "--backend", backend]).unwrap();
            assert_eq!(cli.args.backend, backend);
        }
    }

    #[test]
    fn test_init_args_reconfigure() {
        let cli = TestInitCli::try_parse_from(["test", "--reconfigure"]).unwrap();
        assert!(cli.args.reconfigure);
    }

    #[test]
    fn test_init_args_combined() {
        let cli = TestInitCli::try_parse_from([
            "test",
            "/my/project",
            "--backend",
            "s3",
            "--reconfigure",
        ])
        .unwrap();
        assert_eq!(cli.args.path, PathBuf::from("/my/project"));
        assert_eq!(cli.args.backend, "s3");
        assert!(cli.args.reconfigure);
    }

    // ==================== ProvisionCommands Enum Tests ====================

    #[derive(Parser, Debug)]
    #[command(name = "rustible")]
    struct TestProvisionCli {
        #[command(subcommand)]
        command: ProvisionCommands,
    }

    #[test]
    fn test_provision_commands_plan() {
        let cli = TestProvisionCli::try_parse_from(["rustible", "plan"]).unwrap();
        assert!(matches!(cli.command, ProvisionCommands::Plan(_)));
    }

    #[test]
    fn test_provision_commands_apply() {
        let cli = TestProvisionCli::try_parse_from(["rustible", "apply"]).unwrap();
        assert!(matches!(cli.command, ProvisionCommands::Apply(_)));
    }

    #[test]
    fn test_provision_commands_destroy() {
        let cli = TestProvisionCli::try_parse_from(["rustible", "destroy"]).unwrap();
        assert!(matches!(cli.command, ProvisionCommands::Destroy(_)));
    }

    #[test]
    fn test_provision_commands_import() {
        let cli = TestProvisionCli::try_parse_from([
            "rustible",
            "import",
            "aws_vpc.main",
            "vpc-12345",
        ])
        .unwrap();
        assert!(matches!(cli.command, ProvisionCommands::Import(_)));
    }

    #[test]
    fn test_provision_commands_show() {
        let cli = TestProvisionCli::try_parse_from(["rustible", "show"]).unwrap();
        assert!(matches!(cli.command, ProvisionCommands::Show(_)));
    }

    #[test]
    fn test_provision_commands_refresh() {
        let cli = TestProvisionCli::try_parse_from(["rustible", "refresh"]).unwrap();
        assert!(matches!(cli.command, ProvisionCommands::Refresh(_)));
    }

    #[test]
    fn test_provision_commands_init() {
        let cli = TestProvisionCli::try_parse_from(["rustible", "init"]).unwrap();
        assert!(matches!(cli.command, ProvisionCommands::Init(_)));
    }

    #[test]
    fn test_provision_commands_invalid() {
        let result = TestProvisionCli::try_parse_from(["rustible", "invalid"]);
        assert!(result.is_err());
    }

    // ==================== ProvisionArgs Tests ====================

    #[test]
    fn test_provision_args_plan_with_options() {
        let cli = TestProvisionCli::try_parse_from([
            "rustible",
            "plan",
            "--config-file",
            "prod.yml",
            "-o",
            "plan.json",
            "-t",
            "aws_vpc.main",
            "--destroy",
        ])
        .unwrap();

        if let ProvisionCommands::Plan(args) = cli.command {
            assert_eq!(args.config_file, PathBuf::from("prod.yml"));
            assert_eq!(args.out, Some(PathBuf::from("plan.json")));
            assert_eq!(args.target, vec!["aws_vpc.main"]);
            assert!(args.destroy);
        } else {
            panic!("Expected Plan command");
        }
    }

    #[test]
    fn test_provision_args_apply_with_options() {
        let cli = TestProvisionCli::try_parse_from([
            "rustible",
            "apply",
            "--auto-approve",
            "--parallelism",
            "20",
            "--no-backup",
        ])
        .unwrap();

        if let ProvisionCommands::Apply(args) = cli.command {
            assert!(args.auto_approve);
            assert_eq!(args.parallelism, 20);
            assert!(args.no_backup);
        } else {
            panic!("Expected Apply command");
        }
    }

    // ==================== Edge Cases Tests ====================

    #[test]
    fn test_plan_args_empty_target_list() {
        let cli = TestPlanCli::try_parse_from(["test"]).unwrap();
        assert!(cli.args.target.is_empty());
    }

    #[test]
    fn test_apply_args_parallelism_one() {
        let cli = TestApplyCli::try_parse_from(["test", "--parallelism", "1"]).unwrap();
        assert_eq!(cli.args.parallelism, 1);
    }

    #[test]
    fn test_show_args_empty_address() {
        // Address is optional
        let cli = TestShowCli::try_parse_from(["test"]).unwrap();
        assert!(cli.args.address.is_none());
    }

    #[test]
    fn test_import_args_special_characters_in_id() {
        let cli = TestImportCli::try_parse_from([
            "test",
            "aws_vpc.main",
            "arn:aws:ec2:us-east-1:123456789012:vpc/vpc-12345",
        ])
        .unwrap();
        assert_eq!(
            cli.args.id,
            "arn:aws:ec2:us-east-1:123456789012:vpc/vpc-12345"
        );
    }

    #[test]
    fn test_init_args_path_with_spaces() {
        let cli = TestInitCli::try_parse_from(["test", "/path/with spaces/project"]).unwrap();
        assert_eq!(
            cli.args.path,
            PathBuf::from("/path/with spaces/project")
        );
    }

    #[test]
    fn test_plan_args_multiple_targets_same() {
        // Duplicate targets are allowed by CLI
        let cli = TestPlanCli::try_parse_from([
            "test",
            "-t",
            "aws_vpc.main",
            "-t",
            "aws_vpc.main",
        ])
        .unwrap();
        assert_eq!(cli.args.target.len(), 2);
    }

    // ==================== Path Handling Tests ====================

    #[test]
    fn test_plan_args_relative_path() {
        let cli = TestPlanCli::try_parse_from(["test", "--config-file", "./config/prod.yml"])
            .unwrap();
        assert_eq!(cli.args.config_file, PathBuf::from("./config/prod.yml"));
    }

    #[test]
    fn test_plan_args_absolute_path() {
        let cli =
            TestPlanCli::try_parse_from(["test", "--config-file", "/etc/rustible/config.yml"])
                .unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("/etc/rustible/config.yml")
        );
    }

    #[test]
    fn test_show_args_relative_state_path() {
        let cli =
            TestShowCli::try_parse_from(["test", "--state", "states/prod.state.json"]).unwrap();
        assert_eq!(cli.args.state, PathBuf::from("states/prod.state.json"));
    }

    // ==================== Default Values Verification Tests ====================

    #[test]
    fn test_all_defaults_plan() {
        let cli = TestPlanCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("infrastructure.rustible.yml")
        );
        assert!(cli.args.out.is_none());
        assert!(cli.args.target.is_empty());
        assert!(cli.args.refresh); // default is true
        assert!(cli.args.state.is_none());
        assert!(!cli.args.destroy);
    }

    #[test]
    fn test_all_defaults_apply() {
        let cli = TestApplyCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("infrastructure.rustible.yml")
        );
        assert!(!cli.args.auto_approve);
        assert!(cli.args.target.is_empty());
        assert_eq!(cli.args.parallelism, 10);
        assert!(cli.args.state.is_none());
        assert!(!cli.args.no_backup);
        assert!(!cli.args.no_lock);
    }

    #[test]
    fn test_all_defaults_destroy() {
        let cli = TestDestroyCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("infrastructure.rustible.yml")
        );
        assert!(!cli.args.auto_approve);
        assert!(cli.args.target.is_empty());
        assert!(cli.args.state.is_none());
    }

    #[test]
    fn test_all_defaults_show() {
        let cli = TestShowCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.state,
            PathBuf::from(".rustible/provisioning.state.json")
        );
        assert!(cli.args.address.is_none());
        assert!(!cli.args.json);
    }

    #[test]
    fn test_all_defaults_refresh() {
        let cli = TestRefreshCli::try_parse_from(["test"]).unwrap();
        assert_eq!(
            cli.args.config_file,
            PathBuf::from("infrastructure.rustible.yml")
        );
        assert!(cli.args.target.is_empty());
        assert!(cli.args.state.is_none());
    }

    #[test]
    fn test_all_defaults_init() {
        let cli = TestInitCli::try_parse_from(["test"]).unwrap();
        assert_eq!(cli.args.path, PathBuf::from("."));
        assert_eq!(cli.args.backend, "local");
        assert!(!cli.args.reconfigure);
    }

    // ==================== Clone and Debug Derive Tests ====================

    #[test]
    fn test_plan_args_clone() {
        let args = PlanArgs {
            config_file: PathBuf::from("test.yml"),
            out: Some(PathBuf::from("plan.json")),
            target: vec!["aws_vpc.main".to_string()],
            refresh: false,
            state: Some(PathBuf::from("state.json")),
            destroy: true,
        };
        let cloned = args.clone();
        assert_eq!(cloned.config_file, args.config_file);
        assert_eq!(cloned.out, args.out);
        assert_eq!(cloned.target, args.target);
        assert_eq!(cloned.refresh, args.refresh);
        assert_eq!(cloned.state, args.state);
        assert_eq!(cloned.destroy, args.destroy);
    }

    #[test]
    fn test_apply_args_clone() {
        let args = ApplyArgs {
            config_file: PathBuf::from("test.yml"),
            auto_approve: true,
            target: vec!["aws_vpc.main".to_string()],
            parallelism: 5,
            state: Some(PathBuf::from("state.json")),
            no_backup: true,
            no_lock: true,
        };
        let cloned = args.clone();
        assert_eq!(cloned.parallelism, args.parallelism);
        assert_eq!(cloned.auto_approve, args.auto_approve);
    }

    #[test]
    fn test_provision_commands_debug() {
        let plan = ProvisionCommands::Plan(PlanArgs {
            config_file: PathBuf::from("test.yml"),
            out: None,
            target: vec![],
            refresh: true,
            state: None,
            destroy: false,
        });
        let debug_str = format!("{:?}", plan);
        assert!(debug_str.contains("Plan"));
    }

    // ==================== Error Handling Tests ====================

    #[test]
    fn test_plan_args_refresh_no_value_expected() {
        // With default_value="true", --refresh is a flag that doesn't take a value
        // Passing any value after --refresh is treated as a positional arg and fails
        let result = TestPlanCli::try_parse_from(["test", "--refresh", "invalid"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_args_invalid_parallelism() {
        // Test that non-numeric parallelism fails
        let result = TestApplyCli::try_parse_from(["test", "--parallelism", "abc"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_args_negative_parallelism() {
        // Test that negative parallelism fails (usize cannot be negative)
        let result = TestApplyCli::try_parse_from(["test", "--parallelism", "-1"]);
        assert!(result.is_err());
    }
}
