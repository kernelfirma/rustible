//! Terraform provisioner CLI command
//!
//! This module implements the `rustible provisioner` subcommand for Terraform
//! local-exec integration. It allows Terraform to call Rustible as a provisioner
//! to configure newly created infrastructure.
//!
//! # Example Usage
//!
//! In Terraform:
//! ```hcl
//! resource "aws_instance" "web" {
//!   # ...
//!   provisioner "local-exec" {
//!     command = <<-EOT
//!       rustible provisioner \
//!         --resource-type aws_instance \
//!         --resource-name web \
//!         --playbook configure.yml \
//!         --host ${self.public_ip} \
//!         --user ec2-user \
//!         --private-key ~/.ssh/id_rsa \
//!         --extra-vars '${jsonencode({instance_id = self.id})}'
//!     EOT
//!   }
//! }
//! ```

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Terraform provisioner mode - run playbooks on newly created infrastructure
#[derive(Parser, Debug, Clone)]
#[command(name = "provisioner")]
pub struct ProvisionerArgs {
    /// Playbook to execute on the target host
    #[arg(short, long)]
    pub playbook: PathBuf,

    /// Target host IP address or hostname
    #[arg(long)]
    pub host: String,

    /// SSH user for connection
    #[arg(long, default_value = "root", env = "RUSTIBLE_SSH_USER")]
    pub user: String,

    /// Path to SSH private key file
    #[arg(long, env = "RUSTIBLE_SSH_KEY")]
    pub private_key: Option<PathBuf>,

    /// SSH port
    #[arg(long, default_value = "22")]
    pub port: u16,

    /// Terraform resource type (e.g., aws_instance)
    #[arg(long, env = "TF_RESOURCE_TYPE")]
    pub resource_type: String,

    /// Terraform resource name from configuration
    #[arg(long, env = "TF_RESOURCE_NAME")]
    pub resource_name: String,

    /// Extra variables as JSON string
    #[arg(short = 'e', long = "extra-vars")]
    pub extra_vars: Option<String>,

    /// Connection timeout in seconds
    #[arg(long, default_value = "30")]
    pub timeout: u64,

    /// Maximum number of connection retries
    #[arg(long, default_value = "3")]
    pub retries: u32,

    /// Delay between retries in seconds
    #[arg(long, default_value = "10")]
    pub retry_delay: u64,

    /// Run in check mode (dry-run, no changes made)
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Show differences when making changes
    #[arg(long = "show-diff")]
    pub show_diff: bool,

    /// Verbosity level
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Connection type (ssh, local, docker)
    #[arg(long, default_value = "ssh")]
    pub connection: String,

    /// Become user for privilege escalation
    #[arg(long)]
    pub become_user: Option<String>,

    /// Enable privilege escalation (sudo)
    #[arg(long = "become")]
    pub r#become: bool,

    /// Tags to limit which tasks are executed
    #[arg(long)]
    pub tags: Option<String>,

    /// Skip tasks with these tags
    #[arg(long)]
    pub skip_tags: Option<String>,

    /// Start at a specific task name
    #[arg(long)]
    pub start_at_task: Option<String>,
}

/// Provisioner execution context with Terraform metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionerContext {
    /// Terraform resource type
    pub resource_type: String,
    /// Terraform resource name
    pub resource_name: String,
    /// Target host address
    pub host: String,
    /// SSH user
    pub user: String,
    /// SSH port
    pub port: u16,
    /// Connection info
    pub connection_info: ConnectionInfo,
    /// Variables from Terraform
    pub terraform_vars: HashMap<String, serde_json::Value>,
    /// Execution start time
    pub started_at: DateTime<Utc>,
    /// Triggers (if using null_resource)
    pub triggers: HashMap<String, String>,
}

/// Connection information for the provisioner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// Connection type (ssh, local, docker)
    pub connection_type: String,
    /// Private key path
    pub private_key: Option<String>,
    /// Connection timeout
    pub timeout: u64,
    /// Maximum retries
    pub retries: u32,
    /// Delay between retries
    pub retry_delay: u64,
}

/// Result of provisioner execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionerResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Execution start time
    pub started_at: DateTime<Utc>,
    /// Execution end time
    pub finished_at: DateTime<Utc>,
    /// Duration in seconds
    pub duration_secs: f64,
    /// Number of tasks executed
    pub tasks_executed: usize,
    /// Number of tasks changed
    pub tasks_changed: usize,
    /// Number of tasks failed
    pub tasks_failed: usize,
    /// Error message if failed
    pub error: Option<String>,
    /// Resource context
    pub resource_type: String,
    pub resource_name: String,
    pub host: String,
}

/// Error type for provisioner operations
#[derive(Debug, thiserror::Error)]
pub enum ProvisionerError {
    #[error("Connection failed to {host} after {retries} retries")]
    ConnectionFailed { host: String, retries: u32 },

    #[error("Playbook not found: {0}")]
    PlaybookNotFound(PathBuf),

    #[error("Invalid extra vars JSON: {0}")]
    InvalidExtraVars(String),

    #[error("Playbook execution failed: {0}")]
    ExecutionFailed(String),

    #[error("SSH key not found: {0}")]
    SshKeyNotFound(PathBuf),

    #[error("Connection timeout after {0}s")]
    ConnectionTimeout(u64),

    #[error("Host not reachable: {0}")]
    HostNotReachable(String),
}

impl ProvisionerArgs {
    /// Execute the provisioner command
    pub async fn execute(&self) -> Result<ProvisionerResult> {
        let started_at = Utc::now();
        let start_instant = Instant::now();

        // Validate playbook exists
        if !self.playbook.exists() {
            return Err(ProvisionerError::PlaybookNotFound(self.playbook.clone()).into());
        }

        // Validate SSH key if provided
        if let Some(ref key) = self.private_key {
            let expanded = expand_tilde(key);
            if !expanded.exists() {
                return Err(ProvisionerError::SshKeyNotFound(expanded).into());
            }
        }

        // Parse extra vars
        let terraform_vars = self.parse_extra_vars()?;

        // Build provisioner context
        let context = ProvisionerContext {
            resource_type: self.resource_type.clone(),
            resource_name: self.resource_name.clone(),
            host: self.host.clone(),
            user: self.user.clone(),
            port: self.port,
            connection_info: ConnectionInfo {
                connection_type: self.connection.clone(),
                private_key: self
                    .private_key
                    .as_ref()
                    .map(|p: &PathBuf| p.to_string_lossy().to_string()),
                timeout: self.timeout,
                retries: self.retries,
                retry_delay: self.retry_delay,
            },
            terraform_vars: terraform_vars.clone(),
            started_at,
            triggers: HashMap::new(),
        };

        self.log_info(&format!(
            "Provisioning {} ({}) at {}",
            context.resource_name, context.resource_type, context.host
        ));

        // Wait for host to become available
        if self.connection != "local" {
            self.wait_for_connection().await?;
        }

        // Execute the playbook
        let exec_result = self.run_playbook(&context, terraform_vars).await;

        let finished_at = Utc::now();
        let duration_secs = start_instant.elapsed().as_secs_f64();

        match exec_result {
            Ok((tasks_executed, tasks_changed, tasks_failed)) => {
                if tasks_failed > 0 {
                    let err = ProvisionerError::ExecutionFailed(format!(
                        "{} task(s) failed",
                        tasks_failed
                    ));
                    self.log_error(&format!("Provisioning failed: {}", err));
                    return Err(err.into());
                }

                let result = ProvisionerResult {
                    success: true,
                    started_at,
                    finished_at,
                    duration_secs,
                    tasks_executed,
                    tasks_changed,
                    tasks_failed,
                    error: None,
                    resource_type: self.resource_type.clone(),
                    resource_name: self.resource_name.clone(),
                    host: self.host.clone(),
                };

                self.log_info(&format!(
                    "Provisioning complete: {} tasks, {} changed in {:.1}s",
                    tasks_executed, tasks_changed, duration_secs
                ));

                Ok(result)
            }
            Err(e) => {
                let result = ProvisionerResult {
                    success: false,
                    started_at,
                    finished_at,
                    duration_secs,
                    tasks_executed: 0,
                    tasks_changed: 0,
                    tasks_failed: 1,
                    error: Some(e.to_string()),
                    resource_type: self.resource_type.clone(),
                    resource_name: self.resource_name.clone(),
                    host: self.host.clone(),
                };

                self.log_error(&format!("Provisioning failed: {}", e));

                Err(e)
            }
        }
    }

    /// Parse extra vars from JSON string
    fn parse_extra_vars(&self) -> Result<HashMap<String, serde_json::Value>> {
        match &self.extra_vars {
            Some(json_str) => serde_json::from_str(json_str)
                .map_err(|e| ProvisionerError::InvalidExtraVars(e.to_string()).into()),
            None => Ok(HashMap::new()),
        }
    }

    /// Wait for the host to become available with retries
    async fn wait_for_connection(&self) -> Result<()> {
        for attempt in 0..self.retries {
            if attempt > 0 {
                self.log_info(&format!(
                    "Retrying connection to {} (attempt {}/{})",
                    self.host,
                    attempt + 1,
                    self.retries
                ));
                tokio::time::sleep(Duration::from_secs(self.retry_delay)).await;
            }

            match self.check_connectivity().await {
                Ok(()) => {
                    self.log_debug(&format!("Connection established to {}", self.host));
                    return Ok(());
                }
                Err(e) => {
                    self.log_debug(&format!("Connection attempt {} failed: {}", attempt + 1, e));
                }
            }
        }

        Err(ProvisionerError::ConnectionFailed {
            host: self.host.clone(),
            retries: self.retries,
        }
        .into())
    }

    /// Check if the host is reachable via TCP
    async fn check_connectivity(&self) -> Result<()> {
        let addr = format!("{}:{}", self.host, self.port);
        let timeout = Duration::from_secs(self.timeout);

        // Use blocking TCP connect in spawn_blocking for better async compatibility
        let addr_clone = addr.clone();
        let result = tokio::task::spawn_blocking(move || {
            TcpStream::connect_timeout(&addr_clone.parse().unwrap(), timeout)
        })
        .await?;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                Err(ProvisionerError::HostNotReachable(format!("{}: {}", self.host, e)).into())
            }
        }
    }

    /// Run the playbook against the target host
    async fn run_playbook(
        &self,
        context: &ProvisionerContext,
        terraform_vars: HashMap<String, serde_json::Value>,
    ) -> Result<(usize, usize, usize)> {
        use rustible::connection::{ConnectionConfig, ConnectionFactory, HostConfig};
        use rustible::executor::playbook::Playbook;
        use rustible::executor::runtime::RuntimeContext;
        use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};
        use rustible::inventory::{ConnectionType, Group, Host, Inventory};

        // Read and parse playbook
        let playbook_path: &PathBuf = &self.playbook;
        let playbook_content: String = tokio::fs::read_to_string(playbook_path)
            .await
            .context("Failed to read playbook file")?;

        let playbook = Playbook::parse(&playbook_content, Some(self.playbook.clone()))
            .context("Failed to parse playbook")?;

        let host_name = context.resource_name.clone();
        let tf_group_name = format!("tf_{}", context.resource_type);

        let mut inventory = Inventory::new();
        let mut host = Host::new(&host_name);

        let resolved_host = if self.connection == "local" {
            "localhost".to_string()
        } else {
            context.host.clone()
        };
        host.ansible_host = Some(resolved_host);
        host.connection.connection = match self.connection.as_str() {
            "local" => ConnectionType::Local,
            "docker" => ConnectionType::Docker,
            "podman" => ConnectionType::Podman,
            "winrm" => ConnectionType::Winrm,
            _ => ConnectionType::Ssh,
        };
        host.connection.ssh.port = context.port;
        host.connection.ssh.user = Some(context.user.clone());
        if let Some(ref key) = self.private_key {
            host.connection.ssh.private_key_file = Some(key.to_string_lossy().to_string());
        }

        host.set_var(
            "terraform_resource_type",
            serde_yaml::Value::String(context.resource_type.clone()),
        );
        host.set_var(
            "terraform_resource_name",
            serde_yaml::Value::String(context.resource_name.clone()),
        );

        host.add_to_group("terraform");
        host.add_to_group(&tf_group_name);

        let mut terraform_group = Group::new("terraform");
        terraform_group.add_host(host_name.clone());
        inventory.add_group(terraform_group)?;

        let mut resource_group = Group::new(&tf_group_name);
        resource_group.add_host(host_name.clone());
        inventory.add_group(resource_group)?;

        inventory.add_host(host)?;
        let runtime = RuntimeContext::from_inventory(&inventory);

        // Build executor config
        let mut extra_vars = HashMap::new();
        for (key, value) in terraform_vars {
            extra_vars.insert(format!("terraform_{}", key), value);
        }

        let config = ExecutorConfig {
            check_mode: self.dry_run,
            diff_mode: self.show_diff,
            gather_facts: true,
            forks: 1, // Single host
            strategy: ExecutionStrategy::Linear,
            verbosity: self.verbose,
            extra_vars,
            r#become: self.r#become,
            become_user: self
                .become_user
                .clone()
                .unwrap_or_else(|| "root".to_string()),
            ..Default::default()
        };

        let mut conn_config = ConnectionConfig::default();
        conn_config.defaults.user = context.user.clone();

        let mut host_config = HostConfig::new()
            .hostname(context.host.clone())
            .port(context.port)
            .user(context.user.clone())
            .timeout(context.connection_info.timeout);
        host_config.retries = Some(context.connection_info.retries);
        host_config.retry_delay = Some(context.connection_info.retry_delay);

        if let Some(ref key) = context.connection_info.private_key {
            host_config.identity_file = Some(key.clone());
        }
        if self.connection != "ssh" {
            host_config.connection = Some(self.connection.clone());
        }
        conn_config.hosts.insert(host_name.clone(), host_config);

        // Create executor with runtime context
        let executor = Executor::with_runtime(config, runtime)
            .with_connection_factory(ConnectionFactory::new(conn_config));

        // Execute playbook
        self.log_debug(&format!(
            "Executing playbook: {} on {}",
            self.playbook.display(),
            context.host
        ));

        let results = executor.run_playbook(&playbook).await?;
        let summary = Executor::summarize_results(&results);

        let tasks_executed = summary.ok + summary.changed + summary.failed + summary.skipped;
        let tasks_changed = summary.changed;
        let tasks_failed = summary.failed + summary.unreachable;

        Ok((tasks_executed, tasks_changed, tasks_failed))
    }

    /// Log info message
    fn log_info(&self, msg: &str) {
        if self.verbose > 0 || std::env::var("RUSTIBLE_LOG").is_ok() {
            eprintln!("[INFO] {}", msg);
        }
    }

    /// Log debug message
    fn log_debug(&self, msg: &str) {
        if self.verbose > 1 {
            eprintln!("[DEBUG] {}", msg);
        }
    }

    /// Log error message
    fn log_error(&self, msg: &str) {
        eprintln!("[ERROR] {}", msg);
    }
}

/// Expand tilde (~) in path to user's home directory
fn expand_tilde(path: &PathBuf) -> PathBuf {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    path.clone()
}

/// Generate a Terraform module snippet for using Rustible as a provisioner
pub fn generate_terraform_module() -> String {
    r#"# Terraform module for Rustible provisioner
#
# Usage:
#   module "configure_server" {
#     source = "./modules/rustible-provisioner"
#     host          = aws_instance.web.public_ip
#     playbook      = "${path.module}/playbooks/configure.yml"
#     user          = "ubuntu"
#     resource_type = "aws_instance"
#     resource_name = "web"
#     extra_vars = {
#       instance_id = aws_instance.web.id
#     }
#   }

variable "host" {
  description = "Target host IP or hostname"
  type        = string
}

variable "playbook" {
  description = "Path to Rustible playbook"
  type        = string
}

variable "user" {
  description = "SSH user"
  type        = string
  default     = "root"
}

variable "private_key_path" {
  description = "Path to SSH private key"
  type        = string
  default     = "~/.ssh/id_rsa"
}

variable "extra_vars" {
  description = "Extra variables to pass to Rustible"
  type        = map(any)
  default     = {}
}

variable "resource_type" {
  description = "Terraform resource type"
  type        = string
}

variable "resource_name" {
  description = "Terraform resource name"
  type        = string
}

variable "timeout" {
  description = "Connection timeout in seconds"
  type        = number
  default     = 30
}

variable "retries" {
  description = "Maximum connection retries"
  type        = number
  default     = 5
}

variable "retry_delay" {
  description = "Delay between retries in seconds"
  type        = number
  default     = 15
}

resource "null_resource" "rustible_provisioner" {
  triggers = {
    playbook_hash = filemd5(var.playbook)
    host          = var.host
    extra_vars    = jsonencode(var.extra_vars)
  }

  provisioner "local-exec" {
    command = <<-EOT
      rustible provisioner \
        --playbook ${var.playbook} \
        --host ${var.host} \
        --user ${var.user} \
        --private-key ${var.private_key_path} \
        --resource-type ${var.resource_type} \
        --resource-name ${var.resource_name} \
        --timeout ${var.timeout} \
        --retries ${var.retries} \
        --retry-delay ${var.retry_delay} \
        --extra-vars '${jsonencode(var.extra_vars)}'
    EOT

    environment = {
      RUSTIBLE_LOG = "info"
    }
  }
}
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provisioner_args_defaults() {
        let args = ProvisionerArgs::try_parse_from([
            "provisioner",
            "--playbook",
            "site.yml",
            "--host",
            "192.168.1.100",
            "--resource-type",
            "aws_instance",
            "--resource-name",
            "web",
        ])
        .unwrap();

        assert_eq!(args.playbook, PathBuf::from("site.yml"));
        assert_eq!(args.host, "192.168.1.100");
        assert_eq!(args.user, "root");
        assert_eq!(args.port, 22);
        assert_eq!(args.timeout, 30);
        assert_eq!(args.retries, 3);
        assert_eq!(args.retry_delay, 10);
        assert!(!args.dry_run);
        assert!(!args.r#become);
    }

    #[test]
    fn test_provisioner_args_custom() {
        let args = ProvisionerArgs::try_parse_from([
            "provisioner",
            "--playbook",
            "configure.yml",
            "--host",
            "10.0.0.5",
            "--user",
            "ubuntu",
            "--port",
            "2222",
            "--resource-type",
            "azure_vm",
            "--resource-name",
            "backend",
            "--timeout",
            "60",
            "--retries",
            "5",
            "--retry-delay",
            "15",
            "--dry-run",
            "--become",
            "--become-user",
            "admin",
        ])
        .unwrap();

        assert_eq!(args.user, "ubuntu");
        assert_eq!(args.port, 2222);
        assert_eq!(args.timeout, 60);
        assert_eq!(args.retries, 5);
        assert_eq!(args.retry_delay, 15);
        assert!(args.dry_run);
        assert!(args.r#become);
        assert_eq!(args.become_user.as_deref(), Some("admin"));
    }

    #[test]
    fn test_parse_extra_vars_json() {
        let args = ProvisionerArgs::try_parse_from([
            "provisioner",
            "--playbook",
            "site.yml",
            "--host",
            "192.168.1.100",
            "--resource-type",
            "aws_instance",
            "--resource-name",
            "web",
            "-e",
            r#"{"instance_id": "i-12345", "region": "us-east-1"}"#,
        ])
        .unwrap();

        let vars = args.parse_extra_vars().unwrap();
        assert_eq!(vars.get("instance_id"), Some(&serde_json::json!("i-12345")));
        assert_eq!(vars.get("region"), Some(&serde_json::json!("us-east-1")));
    }

    #[test]
    fn test_parse_extra_vars_nested() {
        let args = ProvisionerArgs::try_parse_from([
            "provisioner",
            "--playbook",
            "site.yml",
            "--host",
            "192.168.1.100",
            "--resource-type",
            "aws_instance",
            "--resource-name",
            "web",
            "-e",
            r#"{"tags": {"env": "prod", "app": "web"}}"#,
        ])
        .unwrap();

        let vars = args.parse_extra_vars().unwrap();
        let tags = vars.get("tags").unwrap();
        assert_eq!(tags["env"], "prod");
        assert_eq!(tags["app"], "web");
    }

    #[test]
    fn test_parse_extra_vars_none() {
        let args = ProvisionerArgs::try_parse_from([
            "provisioner",
            "--playbook",
            "site.yml",
            "--host",
            "192.168.1.100",
            "--resource-type",
            "aws_instance",
            "--resource-name",
            "web",
        ])
        .unwrap();

        let vars = args.parse_extra_vars().unwrap();
        assert!(vars.is_empty());
    }

    #[test]
    fn test_expand_tilde() {
        let path = PathBuf::from("~/test/file.yml");
        let expanded = expand_tilde(&path);

        // Should expand if home dir exists
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded, home.join("test/file.yml"));
        }
    }

    #[test]
    fn test_expand_tilde_no_tilde() {
        let path = PathBuf::from("/absolute/path/file.yml");
        let expanded = expand_tilde(&path);
        assert_eq!(expanded, path);
    }

    #[test]
    fn test_provisioner_context_serialization() {
        let context = ProvisionerContext {
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            host: "192.168.1.100".to_string(),
            user: "ubuntu".to_string(),
            port: 22,
            connection_info: ConnectionInfo {
                connection_type: "ssh".to_string(),
                private_key: Some("~/.ssh/id_rsa".to_string()),
                timeout: 30,
                retries: 3,
                retry_delay: 10,
            },
            terraform_vars: {
                let mut vars = HashMap::new();
                vars.insert("instance_id".to_string(), serde_json::json!("i-12345"));
                vars
            },
            started_at: Utc::now(),
            triggers: HashMap::new(),
        };

        let json = serde_json::to_string(&context).unwrap();
        let deserialized: ProvisionerContext = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.resource_type, "aws_instance");
        assert_eq!(deserialized.resource_name, "web");
        assert_eq!(deserialized.host, "192.168.1.100");
    }

    #[test]
    fn test_provisioner_result_success() {
        let result = ProvisionerResult {
            success: true,
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_secs: 15.5,
            tasks_executed: 10,
            tasks_changed: 3,
            tasks_failed: 0,
            error: None,
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            host: "192.168.1.100".to_string(),
        };

        assert!(result.success);
        assert_eq!(result.tasks_executed, 10);
        assert_eq!(result.tasks_changed, 3);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_provisioner_result_failure() {
        let result = ProvisionerResult {
            success: false,
            started_at: Utc::now(),
            finished_at: Utc::now(),
            duration_secs: 5.0,
            tasks_executed: 3,
            tasks_changed: 1,
            tasks_failed: 1,
            error: Some("Connection refused".to_string()),
            resource_type: "aws_instance".to_string(),
            resource_name: "web".to_string(),
            host: "192.168.1.100".to_string(),
        };

        assert!(!result.success);
        assert_eq!(result.tasks_failed, 1);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_generate_terraform_module() {
        let module = generate_terraform_module();

        assert!(module.contains("variable \"host\""));
        assert!(module.contains("variable \"playbook\""));
        assert!(module.contains("rustible provisioner"));
        assert!(module.contains("null_resource"));
    }
}
