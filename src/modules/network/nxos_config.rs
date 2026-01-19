//! Cisco NX-OS Configuration Module
//!
//! This module manages configuration on Cisco NX-OS devices (Nexus switches).
//! It supports both NX-API (HTTP/HTTPS) and SSH transports with advanced features
//! like checkpoint/rollback and configuration replace.
//!
//! ## Parameters
//!
//! - `lines`: List of configuration lines to apply
//! - `parents`: List of parent configuration sections for hierarchical config
//! - `src`: Path to a configuration file to apply
//! - `replace`: Configuration replace mode (line, block, config)
//! - `backup`: Whether to backup the current running-config before changes
//! - `backup_options`: Options for backup file location and format
//! - `running_config`: The running-config to use for diff comparison
//! - `save_when`: When to save configuration (always, never, modified, changed)
//! - `diff_against`: What to diff against (startup, intended, running)
//! - `diff_ignore_lines`: Lines to ignore during diff comparison
//! - `match`: Match mode for config lines (line, strict, exact, none)
//! - `defaults`: Whether to include defaults in config output
//! - `transport`: Transport method (ssh, nxapi)
//!
//! ## Checkpoint/Rollback Parameters
//!
//! - `checkpoint`: Name of checkpoint to create
//! - `rollback_to`: Name of checkpoint to rollback to
//! - `checkpoint_file`: File path to save/load checkpoint
//!
//! ## NX-API Specific Parameters
//!
//! - `nxapi_host`: NX-API host address
//! - `nxapi_port`: NX-API port (default: 443 for HTTPS, 80 for HTTP)
//! - `nxapi_use_ssl`: Whether to use HTTPS (default: true)
//! - `nxapi_validate_certs`: Whether to validate SSL certificates (default: true)
//! - `nxapi_auth`: Authentication credentials (username/password)
//!
//! ## Examples
//!
//! ```yaml
//! # Apply VLAN configuration via SSH
//! - name: Configure VLANs
//!   nxos_config:
//!     lines:
//!       - vlan 100
//!       - name Production
//!     transport: ssh
//!
//! # Configure interface with parents
//! - name: Configure interface
//!   nxos_config:
//!     parents:
//!       - interface Ethernet1/1
//!     lines:
//!       - description Uplink to Core
//!       - switchport mode trunk
//!       - no shutdown
//!
//! # Create checkpoint and apply config
//! - name: Create checkpoint before changes
//!   nxos_config:
//!     checkpoint: pre_change_checkpoint
//!
//! # Rollback to checkpoint
//! - name: Rollback on failure
//!   nxos_config:
//!     rollback_to: pre_change_checkpoint
//!
//! # Config replace from file
//! - name: Replace entire configuration
//!   nxos_config:
//!     src: /path/to/new_config.txt
//!     replace: config
//!
//! # Use NX-API transport
//! - name: Configure via NX-API
//!   nxos_config:
//!     lines:
//!       - feature bgp
//!     transport: nxapi
//!     nxapi_host: 192.168.1.1
//!     nxapi_use_ssl: true
//! ```

use crate::connection::{CommandResult, Connection, ExecuteOptions};
use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

/// Default NX-API HTTPS port
const NXAPI_DEFAULT_HTTPS_PORT: u16 = 443;

/// Default NX-API HTTP port
const NXAPI_DEFAULT_HTTP_PORT: u16 = 80;

/// Default timeout for NX-API requests (seconds)
const NXAPI_DEFAULT_TIMEOUT: u64 = 30;

/// Maximum checkpoints NX-OS can store
const MAX_CHECKPOINTS: usize = 64;

// ============================================================================
// Transport Types
// ============================================================================

/// Transport method for connecting to NX-OS device
#[derive(Debug, Clone, PartialEq, Default)]
pub enum NxosTransport {
    /// SSH CLI-based transport (default)
    #[default]
    Ssh,
    /// NX-API HTTP/HTTPS REST transport
    NxApi,
}

impl NxosTransport {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "ssh" | "cli" => Ok(NxosTransport::Ssh),
            "nxapi" | "nx-api" | "http" | "https" => Ok(NxosTransport::NxApi),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid transport '{}'. Valid options: ssh, nxapi",
                s
            ))),
        }
    }
}

/// Configuration replace mode
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ReplaceMode {
    /// Replace individual lines (default)
    #[default]
    Line,
    /// Replace entire configuration block
    Block,
    /// Replace entire device configuration
    Config,
}

impl ReplaceMode {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "line" => Ok(ReplaceMode::Line),
            "block" => Ok(ReplaceMode::Block),
            "config" | "full" => Ok(ReplaceMode::Config),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid replace mode '{}'. Valid options: line, block, config",
                s
            ))),
        }
    }
}

/// Match mode for configuration comparison
#[derive(Debug, Clone, PartialEq, Default)]
pub enum MatchMode {
    /// Match line by line (default)
    #[default]
    Line,
    /// Strict matching - order matters
    Strict,
    /// Exact matching - must be identical
    Exact,
    /// No matching - always apply
    None,
}

impl MatchMode {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "line" => Ok(MatchMode::Line),
            "strict" => Ok(MatchMode::Strict),
            "exact" => Ok(MatchMode::Exact),
            "none" => Ok(MatchMode::None),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid match mode '{}'. Valid options: line, strict, exact, none",
                s
            ))),
        }
    }
}

/// When to save configuration to startup-config
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SaveWhen {
    /// Always save after changes
    Always,
    /// Never save automatically
    #[default]
    Never,
    /// Save only if modified
    Modified,
    /// Save if changed
    Changed,
}

impl SaveWhen {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "always" => Ok(SaveWhen::Always),
            "never" => Ok(SaveWhen::Never),
            "modified" => Ok(SaveWhen::Modified),
            "changed" => Ok(SaveWhen::Changed),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid save_when '{}'. Valid options: always, never, modified, changed",
                s
            ))),
        }
    }
}

// ============================================================================
// NX-API Types
// ============================================================================

/// NX-API request format
#[derive(Debug, Serialize)]
struct NxApiRequest {
    ins_api: NxApiInsApi,
}

#[derive(Debug, Serialize)]
struct NxApiInsApi {
    version: String,
    #[serde(rename = "type")]
    req_type: String,
    chunk: String,
    sid: String,
    input: String,
    output_format: String,
}

/// NX-API response format
#[derive(Debug, Deserialize)]
struct NxApiResponse {
    ins_api: NxApiInsApiResponse,
}

#[derive(Debug, Deserialize)]
struct NxApiInsApiResponse {
    #[serde(rename = "type")]
    resp_type: String,
    version: String,
    sid: String,
    outputs: NxApiOutputs,
}

#[derive(Debug, Deserialize)]
struct NxApiOutputs {
    output: NxApiOutputWrapper,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NxApiOutputWrapper {
    Single(NxApiOutput),
    Multiple(Vec<NxApiOutput>),
}

#[derive(Debug, Deserialize)]
struct NxApiOutput {
    code: String,
    msg: String,
    #[serde(default)]
    body: serde_json::Value,
}

// ============================================================================
// Configuration Types
// ============================================================================

/// NX-OS configuration module options parsed from parameters
#[derive(Debug, Clone)]
struct NxosConfig {
    /// Configuration lines to apply
    lines: Option<Vec<String>>,
    /// Parent sections for hierarchical config
    parents: Option<Vec<String>>,
    /// Source file path for configuration
    src: Option<String>,
    /// Replace mode
    replace: ReplaceMode,
    /// Whether to backup running-config
    backup: bool,
    /// Backup options
    backup_options: Option<BackupOptions>,
    /// Pre-fetched running config for comparison
    running_config: Option<String>,
    /// When to save to startup-config
    save_when: SaveWhen,
    /// What to diff against
    diff_against: Option<String>,
    /// Lines to ignore during diff
    diff_ignore_lines: Vec<String>,
    /// Match mode
    match_mode: MatchMode,
    /// Include defaults in config output
    defaults: bool,
    /// Transport method
    transport: NxosTransport,
    /// Checkpoint name to create
    checkpoint: Option<String>,
    /// Checkpoint to rollback to
    rollback_to: Option<String>,
    /// Checkpoint file path
    checkpoint_file: Option<String>,
    /// NX-API host
    nxapi_host: Option<String>,
    /// NX-API port
    nxapi_port: Option<u16>,
    /// Use SSL for NX-API
    nxapi_use_ssl: bool,
    /// Validate SSL certificates
    nxapi_validate_certs: bool,
    /// NX-API username
    nxapi_username: Option<String>,
    /// NX-API password
    nxapi_password: Option<String>,
    /// Command timeout
    timeout: u64,
}

/// Backup file options
#[derive(Debug, Clone, Default)]
struct BackupOptions {
    /// Directory to store backup
    dir_path: Option<String>,
    /// Filename for backup
    filename: Option<String>,
}

impl NxosConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let transport = if let Some(t) = params.get_string("transport")? {
            NxosTransport::from_str(&t)?
        } else {
            NxosTransport::default()
        };

        let replace = if let Some(r) = params.get_string("replace")? {
            ReplaceMode::from_str(&r)?
        } else {
            ReplaceMode::default()
        };

        let save_when = if let Some(s) = params.get_string("save_when")? {
            SaveWhen::from_str(&s)?
        } else {
            SaveWhen::default()
        };

        let match_mode = if let Some(m) = params.get_string("match")? {
            MatchMode::from_str(&m)?
        } else {
            MatchMode::default()
        };

        let backup_options =
            if let Some(serde_json::Value::Object(opts)) = params.get("backup_options") {
                Some(BackupOptions {
                    dir_path: opts
                        .get("dir_path")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    filename: opts
                        .get("filename")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                })
            } else {
                None
            };

        let diff_ignore_lines = params
            .get_vec_string("diff_ignore_lines")?
            .unwrap_or_default();

        let timeout = params
            .get_i64("timeout")?
            .unwrap_or(NXAPI_DEFAULT_TIMEOUT as i64) as u64;

        Ok(Self {
            lines: params.get_vec_string("lines")?,
            parents: params.get_vec_string("parents")?,
            src: params.get_string("src")?,
            replace,
            backup: params.get_bool_or("backup", false),
            backup_options,
            running_config: params.get_string("running_config")?,
            save_when,
            diff_against: params.get_string("diff_against")?,
            diff_ignore_lines,
            match_mode,
            defaults: params.get_bool_or("defaults", false),
            transport,
            checkpoint: params.get_string("checkpoint")?,
            rollback_to: params.get_string("rollback_to")?,
            checkpoint_file: params.get_string("checkpoint_file")?,
            nxapi_host: params.get_string("nxapi_host")?,
            nxapi_port: params.get_u32("nxapi_port")?.map(|p| p as u16),
            nxapi_use_ssl: params.get_bool_or("nxapi_use_ssl", true),
            nxapi_validate_certs: params.get_bool_or("nxapi_validate_certs", true),
            nxapi_username: params.get_string("nxapi_username")?,
            nxapi_password: params.get_string("nxapi_password")?,
            timeout,
        })
    }

    /// Check if this is a checkpoint operation only
    fn is_checkpoint_only(&self) -> bool {
        (self.checkpoint.is_some() || self.rollback_to.is_some())
            && self.lines.is_none()
            && self.src.is_none()
    }

    /// Get the effective NX-API port
    fn effective_nxapi_port(&self) -> u16 {
        self.nxapi_port.unwrap_or({
            if self.nxapi_use_ssl {
                NXAPI_DEFAULT_HTTPS_PORT
            } else {
                NXAPI_DEFAULT_HTTP_PORT
            }
        })
    }
}

// ============================================================================
// NX-OS Config Module Implementation
// ============================================================================

/// Cisco NX-OS configuration management module
pub struct NxosConfigModule;

impl NxosConfigModule {
    /// Build execute options with privilege escalation if needed
    fn build_execute_options(context: &ModuleContext) -> Option<ExecuteOptions> {
        if context.r#become {
            Some(ExecuteOptions {
                escalate: true,
                escalate_user: context.become_user.clone(),
                escalate_method: context.become_method.clone(),
                escalate_password: context.become_password.clone(),
                ..Default::default()
            })
        } else {
            None
        }
    }

    /// Execute a command via SSH connection
    async fn execute_ssh_command(
        connection: &dyn Connection,
        command: &str,
        context: &ModuleContext,
    ) -> ModuleResult<CommandResult> {
        let options = Self::build_execute_options(context);
        connection
            .execute(command, options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("SSH command failed: {}", e)))
    }

    /// Execute commands via NX-API
    async fn execute_nxapi_commands(
        config: &NxosConfig,
        commands: &[String],
        context: &ModuleContext,
    ) -> ModuleResult<Vec<NxApiOutput>> {
        let host = config.nxapi_host.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("nxapi_host is required for NX-API transport".to_string())
        })?;

        let username = config
            .nxapi_username
            .clone()
            .or_else(|| {
                context
                    .vars
                    .get("ansible_user")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .ok_or_else(|| {
                ModuleError::MissingParameter(
                    "nxapi_username is required for NX-API transport".to_string(),
                )
            })?;

        let password = config
            .nxapi_password
            .clone()
            .or_else(|| {
                context
                    .vars
                    .get("ansible_password")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .ok_or_else(|| {
                ModuleError::MissingParameter(
                    "nxapi_password is required for NX-API transport".to_string(),
                )
            })?;

        let port = config.effective_nxapi_port();
        let scheme = if config.nxapi_use_ssl {
            "https"
        } else {
            "http"
        };
        let url = format!("{}://{}:{}/ins", scheme, host, port);

        // Build client with appropriate SSL settings
        let client = if config.nxapi_use_ssl && !config.nxapi_validate_certs {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(config.timeout))
                .build()
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to create HTTP client: {}", e))
                })?
        } else {
            Client::builder()
                .timeout(Duration::from_secs(config.timeout))
                .build()
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to create HTTP client: {}", e))
                })?
        };

        // Join commands with semicolons for batch execution
        let command_str = commands.join(" ; ");

        let request = NxApiRequest {
            ins_api: NxApiInsApi {
                version: "1.0".to_string(),
                req_type: "cli_conf".to_string(),
                chunk: "0".to_string(),
                sid: "1".to_string(),
                input: command_str,
                output_format: "json".to_string(),
            },
        };

        let response = client
            .post(&url)
            .basic_auth(&username, Some(&password))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("NX-API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ModuleError::ExecutionFailed(format!(
                "NX-API returned error status {}: {}",
                status, body
            )));
        }

        let api_response: NxApiResponse = response.json().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to parse NX-API response: {}", e))
        })?;

        let outputs = match api_response.ins_api.outputs.output {
            NxApiOutputWrapper::Single(out) => vec![out],
            NxApiOutputWrapper::Multiple(outs) => outs,
        };

        // Check for errors in outputs
        for output in &outputs {
            if output.code != "200" {
                return Err(ModuleError::ExecutionFailed(format!(
                    "NX-API command failed with code {}: {}",
                    output.code, output.msg
                )));
            }
        }

        Ok(outputs)
    }

    /// Get running configuration via SSH
    async fn get_running_config_ssh(
        connection: &dyn Connection,
        config: &NxosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let cmd = if config.defaults {
            "show running-config all"
        } else {
            "show running-config"
        };

        let result = Self::execute_ssh_command(connection, cmd, context).await?;
        if result.success {
            Ok(result.stdout)
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to get running-config: {}",
                result.stderr
            )))
        }
    }

    /// Get running configuration via NX-API
    async fn get_running_config_nxapi(
        config: &NxosConfig,
        _context: &ModuleContext,
    ) -> ModuleResult<String> {
        let cmd = if config.defaults {
            "show running-config all"
        } else {
            "show running-config"
        };

        // For show commands, we need cli_show type
        let host = config
            .nxapi_host
            .as_ref()
            .ok_or_else(|| ModuleError::MissingParameter("nxapi_host is required".to_string()))?;

        let username = config.nxapi_username.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("nxapi_username is required".to_string())
        })?;

        let password = config.nxapi_password.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("nxapi_password is required".to_string())
        })?;

        let port = config.effective_nxapi_port();
        let scheme = if config.nxapi_use_ssl {
            "https"
        } else {
            "http"
        };
        let url = format!("{}://{}:{}/ins", scheme, host, port);

        let client = if config.nxapi_use_ssl && !config.nxapi_validate_certs {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(config.timeout))
                .build()
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to create HTTP client: {}", e))
                })?
        } else {
            Client::builder()
                .timeout(Duration::from_secs(config.timeout))
                .build()
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to create HTTP client: {}", e))
                })?
        };

        let request = NxApiRequest {
            ins_api: NxApiInsApi {
                version: "1.0".to_string(),
                req_type: "cli_show_ascii".to_string(),
                chunk: "0".to_string(),
                sid: "1".to_string(),
                input: cmd.to_string(),
                output_format: "json".to_string(),
            },
        };

        let response = client
            .post(&url)
            .basic_auth(username, Some(password))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("NX-API request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "NX-API returned status {}",
                response.status()
            )));
        }

        let api_response: NxApiResponse = response.json().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to parse response: {}", e))
        })?;

        let output = match api_response.ins_api.outputs.output {
            NxApiOutputWrapper::Single(out) => out,
            NxApiOutputWrapper::Multiple(outs) => outs
                .into_iter()
                .next()
                .ok_or_else(|| ModuleError::ExecutionFailed("Empty NX-API response".to_string()))?,
        };

        // Extract config from body
        if let Some(config_str) = output.body.as_str() {
            Ok(config_str.to_string())
        } else {
            Ok(output.body.to_string())
        }
    }

    /// Create a checkpoint via SSH
    async fn create_checkpoint_ssh(
        connection: &dyn Connection,
        checkpoint_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Validate checkpoint name (alphanumeric and underscores only)
        if !checkpoint_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid checkpoint name '{}': must contain only alphanumeric characters, underscores, and hyphens",
                checkpoint_name
            )));
        }

        let cmd = format!("checkpoint {}", checkpoint_name);
        let result = Self::execute_ssh_command(connection, &cmd, context).await?;

        if result.success || result.stdout.contains("done") {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to create checkpoint '{}': {}",
                checkpoint_name, result.stderr
            )))
        }
    }

    /// Create a checkpoint via NX-API
    async fn create_checkpoint_nxapi(
        config: &NxosConfig,
        checkpoint_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Validate checkpoint name
        if !checkpoint_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid checkpoint name '{}': must contain only alphanumeric characters, underscores, and hyphens",
                checkpoint_name
            )));
        }

        let cmd = format!("checkpoint {}", checkpoint_name);
        Self::execute_nxapi_commands(config, &[cmd], context).await?;
        Ok(())
    }

    /// Rollback to checkpoint via SSH
    async fn rollback_checkpoint_ssh(
        connection: &dyn Connection,
        checkpoint_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Validate checkpoint name
        if !checkpoint_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid checkpoint name '{}': must contain only alphanumeric characters, underscores, and hyphens",
                checkpoint_name
            )));
        }

        let cmd = format!("rollback running-config checkpoint {}", checkpoint_name);
        let result = Self::execute_ssh_command(connection, &cmd, context).await?;

        if result.success || result.stdout.contains("Rollback Done") {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to rollback to checkpoint '{}': {}",
                checkpoint_name, result.stderr
            )))
        }
    }

    /// Rollback to checkpoint via NX-API
    async fn rollback_checkpoint_nxapi(
        config: &NxosConfig,
        checkpoint_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Validate checkpoint name
        if !checkpoint_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid checkpoint name '{}': must contain only alphanumeric characters, underscores, and hyphens",
                checkpoint_name
            )));
        }

        let cmd = format!("rollback running-config checkpoint {}", checkpoint_name);
        Self::execute_nxapi_commands(config, &[cmd], context).await?;
        Ok(())
    }

    /// List checkpoints via SSH
    async fn list_checkpoints_ssh(
        connection: &dyn Connection,
        context: &ModuleContext,
    ) -> ModuleResult<Vec<String>> {
        let result =
            Self::execute_ssh_command(connection, "show checkpoint summary", context).await?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to list checkpoints: {}",
                result.stderr
            )));
        }

        // Parse checkpoint names from output
        let checkpoints: Vec<String> = result
            .stdout
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if !trimmed.is_empty()
                    && !trimmed.starts_with("----")
                    && !trimmed.starts_with("Name")
                    && !trimmed.starts_with("Total")
                {
                    // Extract checkpoint name (first column)
                    trimmed.split_whitespace().next().map(String::from)
                } else {
                    None
                }
            })
            .collect();

        Ok(checkpoints)
    }

    /// Delete checkpoint via SSH
    async fn delete_checkpoint_ssh(
        connection: &dyn Connection,
        checkpoint_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Validate checkpoint name
        if !checkpoint_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid checkpoint name '{}'",
                checkpoint_name
            )));
        }

        let cmd = format!("no checkpoint {}", checkpoint_name);
        let result = Self::execute_ssh_command(connection, &cmd, context).await?;

        if result.success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to delete checkpoint '{}': {}",
                checkpoint_name, result.stderr
            )))
        }
    }

    /// Apply configuration lines via SSH
    async fn apply_config_ssh(
        connection: &dyn Connection,
        config: &NxosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, Vec<String>)> {
        let mut commands = Vec::new();
        #[allow(unused_assignments)]
        let mut changed = false;

        // Build command list with parents and lines
        if let Some(ref parents) = config.parents {
            for parent in parents {
                commands.push(parent.clone());
            }
        }

        if let Some(ref lines) = config.lines {
            commands.extend(lines.clone());
        }

        // Read from source file if specified
        if let Some(ref src) = config.src {
            let source_contents = std::fs::read_to_string(src).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read source file '{}': {}", src, e))
            })?;

            for line in source_contents.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('!') {
                    commands.push(trimmed.to_string());
                }
            }
        }

        if commands.is_empty() {
            return Ok((false, Vec::new()));
        }

        // Enter configuration mode and apply commands
        #[allow(unused_assignments)]
        let mut applied_commands = Vec::new();

        // Use configure terminal mode
        let conf_cmd = format!("configure terminal ; {} ; end", commands.join(" ; "));

        let result = Self::execute_ssh_command(connection, &conf_cmd, context).await?;

        if result.success {
            changed = true;
            applied_commands = commands;
        } else {
            // Check if error is acceptable (some warnings are ok)
            if result.stderr.contains("Invalid command")
                || result.stderr.contains("Error")
                || result.stderr.contains("FAILED")
            {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Configuration failed: {}",
                    result.stderr
                )));
            }
            // Warnings are acceptable
            changed = true;
            applied_commands = commands;
        }

        Ok((changed, applied_commands))
    }

    /// Apply configuration via NX-API
    async fn apply_config_nxapi(
        config: &NxosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, Vec<String>)> {
        let mut commands = Vec::new();

        // Build command list with parents and lines
        if let Some(ref parents) = config.parents {
            for parent in parents {
                commands.push(parent.clone());
            }
        }

        if let Some(ref lines) = config.lines {
            commands.extend(lines.clone());
        }

        // Read from source file if specified
        if let Some(ref src) = config.src {
            let source_contents = std::fs::read_to_string(src).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read source file '{}': {}", src, e))
            })?;

            for line in source_contents.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('!') {
                    commands.push(trimmed.to_string());
                }
            }
        }

        if commands.is_empty() {
            return Ok((false, Vec::new()));
        }

        // Execute commands via NX-API
        Self::execute_nxapi_commands(config, &commands, context).await?;

        Ok((true, commands))
    }

    /// Replace configuration via SSH
    async fn replace_config_ssh(
        connection: &dyn Connection,
        config: &NxosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let src = config.src.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter(
                "src parameter is required for config replace".to_string(),
            )
        })?;

        // For config replace, we need to copy the file and use rollback
        // This requires the file to be accessible from the device

        // Read the config file content
        let source_contents = std::fs::read_to_string(src).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to read source file '{}': {}", src, e))
        })?;

        // Create a temporary checkpoint before replace
        let temp_checkpoint = format!("rustible_replace_{}", chrono::Utc::now().timestamp());
        Self::create_checkpoint_ssh(connection, &temp_checkpoint, context).await?;

        // Apply the new configuration
        let commands: Vec<String> = source_contents
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('!') {
                    Some(trimmed.to_string())
                } else {
                    None
                }
            })
            .collect();

        if commands.is_empty() {
            return Ok(false);
        }

        // Clear existing config sections and apply new
        let conf_cmd = format!("configure terminal ; {} ; end", commands.join(" ; "));

        let result = Self::execute_ssh_command(connection, &conf_cmd, context).await?;

        if !result.success && (result.stderr.contains("Error") || result.stderr.contains("FAILED"))
        {
            // Rollback on failure
            let _ = Self::rollback_checkpoint_ssh(connection, &temp_checkpoint, context).await;
            return Err(ModuleError::ExecutionFailed(format!(
                "Config replace failed: {}",
                result.stderr
            )));
        }

        // Clean up temporary checkpoint
        let _ = Self::delete_checkpoint_ssh(connection, &temp_checkpoint, context).await;

        Ok(true)
    }

    /// Save running-config to startup-config via SSH
    async fn save_config_ssh(
        connection: &dyn Connection,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let result =
            Self::execute_ssh_command(connection, "copy running-config startup-config", context)
                .await?;

        if result.success
            || result.stdout.contains("[OK]")
            || result.stdout.contains("Copy complete")
        {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to save configuration: {}",
                result.stderr
            )))
        }
    }

    /// Save configuration via NX-API
    async fn save_config_nxapi(config: &NxosConfig, context: &ModuleContext) -> ModuleResult<()> {
        Self::execute_nxapi_commands(
            config,
            &["copy running-config startup-config".to_string()],
            context,
        )
        .await?;
        Ok(())
    }

    /// Backup running-config
    async fn backup_config(
        connection: &dyn Connection,
        config: &NxosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        if !config.backup {
            return Ok(None);
        }

        let running_config = match config.transport {
            NxosTransport::Ssh => Self::get_running_config_ssh(connection, config, context).await?,
            NxosTransport::NxApi => Self::get_running_config_nxapi(config, context).await?,
        };

        // Determine backup path
        let backup_dir = config
            .backup_options
            .as_ref()
            .and_then(|o| o.dir_path.clone())
            .unwrap_or_else(|| ".".to_string());

        let backup_filename = config
            .backup_options
            .as_ref()
            .and_then(|o| o.filename.clone())
            .unwrap_or_else(|| {
                format!(
                    "nxos_backup_{}.cfg",
                    chrono::Utc::now().format("%Y%m%d_%H%M%S")
                )
            });

        let backup_path = format!("{}/{}", backup_dir, backup_filename);

        // Create backup directory if it doesn't exist
        if let Some(parent) = std::path::Path::new(&backup_path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to create backup directory: {}", e))
            })?;
        }

        // Write backup file
        std::fs::write(&backup_path, &running_config).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to write backup file: {}", e))
        })?;

        Ok(Some(backup_path))
    }

    /// Execute the module with async connection (SSH)
    async fn execute_async_ssh(
        &self,
        config: &NxosConfig,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut messages = Vec::new();
        let mut data = HashMap::new();

        // Backup current configuration if requested
        if let Some(backup_path) = Self::backup_config(connection.as_ref(), config, context).await?
        {
            messages.push(format!("Backup saved to {}", backup_path));
            data.insert("backup_path".to_string(), serde_json::json!(backup_path));
        }

        // Handle checkpoint creation
        if let Some(ref checkpoint_name) = config.checkpoint {
            if context.check_mode {
                messages.push(format!("Would create checkpoint '{}'", checkpoint_name));
                changed = true;
            } else {
                Self::create_checkpoint_ssh(connection.as_ref(), checkpoint_name, context).await?;
                messages.push(format!("Created checkpoint '{}'", checkpoint_name));
                changed = true;
            }
        }

        // Handle rollback
        if let Some(ref rollback_to) = config.rollback_to {
            if context.check_mode {
                messages.push(format!("Would rollback to checkpoint '{}'", rollback_to));
                changed = true;
            } else {
                Self::rollback_checkpoint_ssh(connection.as_ref(), rollback_to, context).await?;
                messages.push(format!("Rolled back to checkpoint '{}'", rollback_to));
                changed = true;
            }
        }

        // Apply configuration if lines/src provided
        if config.lines.is_some() || config.src.is_some() {
            if context.check_mode {
                if let Some(ref lines) = config.lines {
                    messages.push(format!("Would apply {} configuration lines", lines.len()));
                }
                if let Some(ref src) = config.src {
                    messages.push(format!("Would apply configuration from {}", src));
                }
                changed = true;
            } else {
                // Handle replace mode
                if config.replace == ReplaceMode::Config {
                    let replaced =
                        Self::replace_config_ssh(connection.as_ref(), config, context).await?;
                    if replaced {
                        messages.push("Configuration replaced successfully".to_string());
                        changed = true;
                    }
                } else {
                    let (config_changed, applied) =
                        Self::apply_config_ssh(connection.as_ref(), config, context).await?;
                    if config_changed {
                        messages.push(format!("Applied {} configuration commands", applied.len()));
                        data.insert("commands".to_string(), serde_json::json!(applied));
                        changed = true;
                    }
                }
            }
        }

        // Save configuration if needed
        let should_save = match config.save_when {
            SaveWhen::Always => true,
            SaveWhen::Changed => changed,
            SaveWhen::Modified => changed,
            SaveWhen::Never => false,
        };

        if should_save && !context.check_mode {
            Self::save_config_ssh(connection.as_ref(), context).await?;
            messages.push("Configuration saved to startup-config".to_string());
        }

        // Build output
        let msg = if messages.is_empty() {
            "No changes required".to_string()
        } else {
            messages.join(". ")
        };

        let mut output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        for (key, value) in data {
            output = output.with_data(key, value);
        }

        Ok(output)
    }

    /// Execute the module with NX-API transport
    async fn execute_async_nxapi(
        &self,
        config: &NxosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut messages = Vec::new();
        let mut data = HashMap::new();

        // Handle checkpoint creation
        if let Some(ref checkpoint_name) = config.checkpoint {
            if context.check_mode {
                messages.push(format!("Would create checkpoint '{}'", checkpoint_name));
                changed = true;
            } else {
                Self::create_checkpoint_nxapi(config, checkpoint_name, context).await?;
                messages.push(format!("Created checkpoint '{}'", checkpoint_name));
                changed = true;
            }
        }

        // Handle rollback
        if let Some(ref rollback_to) = config.rollback_to {
            if context.check_mode {
                messages.push(format!("Would rollback to checkpoint '{}'", rollback_to));
                changed = true;
            } else {
                Self::rollback_checkpoint_nxapi(config, rollback_to, context).await?;
                messages.push(format!("Rolled back to checkpoint '{}'", rollback_to));
                changed = true;
            }
        }

        // Apply configuration if lines/src provided
        if config.lines.is_some() || config.src.is_some() {
            if context.check_mode {
                if let Some(ref lines) = config.lines {
                    messages.push(format!("Would apply {} configuration lines", lines.len()));
                }
                if let Some(ref src) = config.src {
                    messages.push(format!("Would apply configuration from {}", src));
                }
                changed = true;
            } else {
                let (config_changed, applied) = Self::apply_config_nxapi(config, context).await?;
                if config_changed {
                    messages.push(format!("Applied {} configuration commands", applied.len()));
                    data.insert("commands".to_string(), serde_json::json!(applied));
                    changed = true;
                }
            }
        }

        // Save configuration if needed
        let should_save = match config.save_when {
            SaveWhen::Always => true,
            SaveWhen::Changed => changed,
            SaveWhen::Modified => changed,
            SaveWhen::Never => false,
        };

        if should_save && !context.check_mode {
            Self::save_config_nxapi(config, context).await?;
            messages.push("Configuration saved to startup-config".to_string());
        }

        // Build output
        let msg = if messages.is_empty() {
            "No changes required".to_string()
        } else {
            messages.join(". ")
        };

        let mut output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        for (key, value) in data {
            output = output.with_data(key, value);
        }

        Ok(output)
    }

    /// Execute the module
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = NxosConfig::from_params(params)?;

        match config.transport {
            NxosTransport::Ssh => {
                let connection = context.connection.clone().ok_or_else(|| {
                    ModuleError::ExecutionFailed(
                        "No SSH connection available for NX-OS module".to_string(),
                    )
                })?;
                self.execute_async_ssh(&config, context, connection).await
            }
            NxosTransport::NxApi => self.execute_async_nxapi(&config, context).await,
        }
    }

    /// Generate diff for configuration changes
    async fn diff_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<Option<Diff>> {
        let config = NxosConfig::from_params(params)?;

        // Get current running config
        let current_config = match config.transport {
            NxosTransport::Ssh => {
                if let Some(ref connection) = context.connection {
                    Self::get_running_config_ssh(connection.as_ref(), &config, context).await?
                } else {
                    return Ok(None);
                }
            }
            NxosTransport::NxApi => Self::get_running_config_nxapi(&config, context).await?,
        };

        // Build proposed config lines
        let mut proposed_lines = Vec::new();

        if let Some(ref parents) = config.parents {
            proposed_lines.extend(parents.clone());
        }

        if let Some(ref lines) = config.lines {
            proposed_lines.extend(lines.clone());
        }

        if let Some(ref src) = config.src {
            let source_contents = std::fs::read_to_string(src).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read source file: {}", e))
            })?;
            for line in source_contents.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('!') {
                    proposed_lines.push(trimmed.to_string());
                }
            }
        }

        if proposed_lines.is_empty() {
            return Ok(None);
        }

        // Filter out ignored lines
        let filter_ignored = |text: &str| -> String {
            text.lines()
                .filter(|line| {
                    !config
                        .diff_ignore_lines
                        .iter()
                        .any(|pattern| line.contains(pattern))
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let filtered_current = filter_ignored(&current_config);
        let proposed = proposed_lines.join("\n");

        Ok(Some(
            Diff::new(filtered_current, proposed.clone())
                .with_details(format!("Proposed configuration changes:\n{}", proposed)),
        ))
    }
}

impl Module for NxosConfigModule {
    fn name(&self) -> &'static str {
        "nxos_config"
    }

    fn description(&self) -> &'static str {
        "Manage Cisco NX-OS configuration (Nexus switches)"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Network device configuration should be rate-limited
        ParallelizationHint::RateLimited {
            requests_per_second: 5,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        // No strictly required params - can be checkpoint-only or config-only
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let config = NxosConfig::from_params(params)?;

        // Must have either config changes or checkpoint operation
        let has_config = config.lines.is_some() || config.src.is_some();
        let has_checkpoint = config.checkpoint.is_some() || config.rollback_to.is_some();

        if !has_config && !has_checkpoint {
            return Err(ModuleError::InvalidParameter(
                "Must provide either configuration (lines/src) or checkpoint operation (checkpoint/rollback_to)".to_string(),
            ));
        }

        // NX-API requires host
        if config.transport == NxosTransport::NxApi && config.nxapi_host.is_none() {
            return Err(ModuleError::MissingParameter(
                "nxapi_host is required when using NX-API transport".to_string(),
            ));
        }

        // Config replace requires src
        if config.replace == ReplaceMode::Config && config.src.is_none() {
            return Err(ModuleError::MissingParameter(
                "src parameter is required for config replace mode".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Use tokio runtime to execute async code
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_from_str() {
        assert_eq!(NxosTransport::from_str("ssh").unwrap(), NxosTransport::Ssh);
        assert_eq!(NxosTransport::from_str("cli").unwrap(), NxosTransport::Ssh);
        assert_eq!(
            NxosTransport::from_str("nxapi").unwrap(),
            NxosTransport::NxApi
        );
        assert_eq!(
            NxosTransport::from_str("nx-api").unwrap(),
            NxosTransport::NxApi
        );
        assert!(NxosTransport::from_str("invalid").is_err());
    }

    #[test]
    fn test_replace_mode_from_str() {
        assert_eq!(ReplaceMode::from_str("line").unwrap(), ReplaceMode::Line);
        assert_eq!(ReplaceMode::from_str("block").unwrap(), ReplaceMode::Block);
        assert_eq!(
            ReplaceMode::from_str("config").unwrap(),
            ReplaceMode::Config
        );
        assert_eq!(ReplaceMode::from_str("full").unwrap(), ReplaceMode::Config);
        assert!(ReplaceMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_match_mode_from_str() {
        assert_eq!(MatchMode::from_str("line").unwrap(), MatchMode::Line);
        assert_eq!(MatchMode::from_str("strict").unwrap(), MatchMode::Strict);
        assert_eq!(MatchMode::from_str("exact").unwrap(), MatchMode::Exact);
        assert_eq!(MatchMode::from_str("none").unwrap(), MatchMode::None);
        assert!(MatchMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_save_when_from_str() {
        assert_eq!(SaveWhen::from_str("always").unwrap(), SaveWhen::Always);
        assert_eq!(SaveWhen::from_str("never").unwrap(), SaveWhen::Never);
        assert_eq!(SaveWhen::from_str("modified").unwrap(), SaveWhen::Modified);
        assert_eq!(SaveWhen::from_str("changed").unwrap(), SaveWhen::Changed);
        assert!(SaveWhen::from_str("invalid").is_err());
    }

    #[test]
    fn test_nxos_config_module_metadata() {
        let module = NxosConfigModule;
        assert_eq!(module.name(), "nxos_config");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert!(matches!(
            module.parallelization_hint(),
            ParallelizationHint::RateLimited { .. }
        ));
    }

    #[test]
    fn test_config_from_params_basic() {
        let mut params = ModuleParams::new();
        params.insert(
            "lines".to_string(),
            serde_json::json!(["vlan 100", "name Production"]),
        );

        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.lines.as_ref().unwrap().len(), 2);
        assert_eq!(config.transport, NxosTransport::Ssh);
        assert_eq!(config.replace, ReplaceMode::Line);
        assert_eq!(config.save_when, SaveWhen::Never);
        assert!(!config.backup);
    }

    #[test]
    fn test_config_from_params_with_parents() {
        let mut params = ModuleParams::new();
        params.insert(
            "parents".to_string(),
            serde_json::json!(["interface Ethernet1/1"]),
        );
        params.insert(
            "lines".to_string(),
            serde_json::json!(["description Test", "no shutdown"]),
        );

        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.parents.as_ref().unwrap().len(), 1);
        assert_eq!(config.lines.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_config_from_params_nxapi() {
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["feature bgp"]));
        params.insert("transport".to_string(), serde_json::json!("nxapi"));
        params.insert("nxapi_host".to_string(), serde_json::json!("192.168.1.1"));
        params.insert("nxapi_port".to_string(), serde_json::json!(8443));
        params.insert("nxapi_use_ssl".to_string(), serde_json::json!(true));
        params.insert("nxapi_validate_certs".to_string(), serde_json::json!(false));

        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.transport, NxosTransport::NxApi);
        assert_eq!(config.nxapi_host.as_ref().unwrap(), "192.168.1.1");
        assert_eq!(config.nxapi_port, Some(8443));
        assert!(config.nxapi_use_ssl);
        assert!(!config.nxapi_validate_certs);
    }

    #[test]
    fn test_config_from_params_checkpoint() {
        let mut params = ModuleParams::new();
        params.insert(
            "checkpoint".to_string(),
            serde_json::json!("before_changes"),
        );

        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.checkpoint.as_ref().unwrap(), "before_changes");
        assert!(config.is_checkpoint_only());
    }

    #[test]
    fn test_config_from_params_rollback() {
        let mut params = ModuleParams::new();
        params.insert(
            "rollback_to".to_string(),
            serde_json::json!("before_changes"),
        );

        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.rollback_to.as_ref().unwrap(), "before_changes");
        assert!(config.is_checkpoint_only());
    }

    #[test]
    fn test_config_from_params_backup() {
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("backup".to_string(), serde_json::json!(true));
        params.insert(
            "backup_options".to_string(),
            serde_json::json!({
                "dir_path": "/backups",
                "filename": "mybackup.cfg"
            }),
        );

        let config = NxosConfig::from_params(&params).unwrap();
        assert!(config.backup);
        assert!(config.backup_options.is_some());
        let opts = config.backup_options.as_ref().unwrap();
        assert_eq!(opts.dir_path.as_ref().unwrap(), "/backups");
        assert_eq!(opts.filename.as_ref().unwrap(), "mybackup.cfg");
    }

    #[test]
    fn test_config_from_params_replace() {
        let mut params = ModuleParams::new();
        params.insert("src".to_string(), serde_json::json!("/path/to/config.txt"));
        params.insert("replace".to_string(), serde_json::json!("config"));

        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.replace, ReplaceMode::Config);
        assert_eq!(config.src.as_ref().unwrap(), "/path/to/config.txt");
    }

    #[test]
    fn test_effective_nxapi_port() {
        let mut params = ModuleParams::new();
        params.insert("checkpoint".to_string(), serde_json::json!("test"));
        params.insert("transport".to_string(), serde_json::json!("nxapi"));

        // Default HTTPS port
        params.insert("nxapi_use_ssl".to_string(), serde_json::json!(true));
        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.effective_nxapi_port(), 443);

        // Default HTTP port
        params.insert("nxapi_use_ssl".to_string(), serde_json::json!(false));
        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.effective_nxapi_port(), 80);

        // Custom port
        params.insert("nxapi_port".to_string(), serde_json::json!(8080));
        let config = NxosConfig::from_params(&params).unwrap();
        assert_eq!(config.effective_nxapi_port(), 8080);
    }

    #[test]
    fn test_validate_params_requires_action() {
        let module = NxosConfigModule;
        let params = ModuleParams::new();

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_nxapi_requires_host() {
        let module = NxosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("nxapi"));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_replace_requires_src() {
        let module = NxosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("replace".to_string(), serde_json::json!("config"));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_valid_config() {
        let module = NxosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_params_valid_checkpoint() {
        let module = NxosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("checkpoint".to_string(), serde_json::json!("test"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }
}
