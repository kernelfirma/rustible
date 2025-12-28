//! Arista EOS Configuration Module
//!
//! This module provides configuration management for Arista EOS devices using
//! the eAPI (JSON-RPC over HTTP/HTTPS) transport. It implements functionality
//! equivalent to Ansible's `arista.eos.eos_config` module.
//!
//! ## Features
//!
//! - **eAPI Transport**: Native JSON-RPC over HTTP/HTTPS communication
//! - **Configuration Sessions**: Atomic configuration changes with commit/abort
//! - **Replace/Merge Modes**: Support for both configuration merge and replace
//! - **Diff Output**: Show configuration differences before and after changes
//! - **Backup Support**: Automatic configuration backup before changes
//! - **Check Mode**: Preview changes without applying them
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
//! - `diff_against`: What to diff against (startup, intended, running, session)
//! - `diff_ignore_lines`: Lines to ignore during diff comparison
//! - `match`: Match mode for config lines (line, strict, exact, none)
//! - `defaults`: Whether to include defaults in config output
//! - `transport`: Transport method (eapi, ssh)
//!
//! ## Session Parameters
//!
//! - `session`: Name of configuration session to use
//! - `commit`: Whether to commit session changes
//! - `abort`: Whether to abort an existing session
//! - `session_timeout`: Timeout for session in seconds
//!
//! ## eAPI Specific Parameters
//!
//! - `eapi_host`: eAPI host address
//! - `eapi_port`: eAPI port (default: 443 for HTTPS, 80 for HTTP)
//! - `eapi_use_ssl`: Whether to use HTTPS (default: true)
//! - `eapi_validate_certs`: Whether to validate SSL certificates (default: true)
//! - `eapi_username`: eAPI username
//! - `eapi_password`: eAPI password
//!
//! ## Examples
//!
//! ```yaml
//! # Apply VLAN configuration via eAPI
//! - name: Configure VLANs
//!   eos_config:
//!     lines:
//!       - vlan 100
//!       - name Production
//!     transport: eapi
//!     eapi_host: 192.168.1.1
//!
//! # Configure interface with parents
//! - name: Configure interface
//!   eos_config:
//!     parents:
//!       - interface Ethernet1
//!     lines:
//!       - description Uplink to Core
//!       - switchport mode trunk
//!       - no shutdown
//!
//! # Use configuration session for atomic changes
//! - name: Configure with session
//!   eos_config:
//!     lines:
//!       - router bgp 65001
//!       - neighbor 10.0.0.1 remote-as 65002
//!     session: my_session
//!     commit: true
//!
//! # Replace entire configuration
//! - name: Replace configuration
//!   eos_config:
//!     src: /path/to/new_config.txt
//!     replace: config
//!
//! # Show session diff before commit
//! - name: Preview changes
//!   eos_config:
//!     lines:
//!       - hostname new-switch
//!     session: preview_session
//!     diff_against: session
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

/// Default eAPI HTTPS port
const EAPI_DEFAULT_HTTPS_PORT: u16 = 443;

/// Default eAPI HTTP port
const EAPI_DEFAULT_HTTP_PORT: u16 = 80;

/// Default timeout for eAPI requests (seconds)
const EAPI_DEFAULT_TIMEOUT: u64 = 30;

/// Default session timeout in seconds
const DEFAULT_SESSION_TIMEOUT: u64 = 300;

// ============================================================================
// Transport Types
// ============================================================================

/// Transport method for connecting to EOS device
#[derive(Debug, Clone, PartialEq, Default)]
pub enum EosTransport {
    /// eAPI HTTP/HTTPS REST transport (default for EOS)
    #[default]
    Eapi,
    /// SSH CLI-based transport
    Ssh,
}

impl EosTransport {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "eapi" | "http" | "https" | "api" => Ok(EosTransport::Eapi),
            "ssh" | "cli" => Ok(EosTransport::Ssh),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid transport '{}'. Valid options: eapi, ssh",
                s
            ))),
        }
    }
}

/// Configuration replace mode
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ReplaceMode {
    /// Merge individual lines (default)
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
            "line" | "merge" => Ok(ReplaceMode::Line),
            "block" => Ok(ReplaceMode::Block),
            "config" | "full" | "replace" => Ok(ReplaceMode::Config),
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

/// What to diff against
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DiffAgainst {
    /// Diff against running configuration (default)
    #[default]
    Running,
    /// Diff against startup configuration
    Startup,
    /// Diff against intended configuration
    Intended,
    /// Diff against session configuration (before commit)
    Session,
}

impl DiffAgainst {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "running" => Ok(DiffAgainst::Running),
            "startup" => Ok(DiffAgainst::Startup),
            "intended" => Ok(DiffAgainst::Intended),
            "session" => Ok(DiffAgainst::Session),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid diff_against '{}'. Valid options: running, startup, intended, session",
                s
            ))),
        }
    }
}

// ============================================================================
// eAPI Types
// ============================================================================

/// eAPI JSON-RPC request format
#[derive(Debug, Serialize)]
struct EapiRequest {
    jsonrpc: String,
    method: String,
    params: EapiParams,
    id: String,
}

#[derive(Debug, Serialize)]
struct EapiParams {
    version: u32,
    cmds: Vec<EapiCommand>,
    format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "autoComplete")]
    auto_complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "expandAliases")]
    expand_aliases: Option<bool>,
}

/// eAPI command - can be a simple string or an object for session commands
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum EapiCommand {
    Simple(String),
    Complex {
        cmd: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<String>,
    },
}

impl From<String> for EapiCommand {
    fn from(s: String) -> Self {
        EapiCommand::Simple(s)
    }
}

impl From<&str> for EapiCommand {
    fn from(s: &str) -> Self {
        EapiCommand::Simple(s.to_string())
    }
}

/// eAPI JSON-RPC response format
#[derive(Debug, Deserialize)]
struct EapiResponse {
    jsonrpc: String,
    id: String,
    #[serde(default)]
    result: Option<Vec<EapiResult>>,
    #[serde(default)]
    error: Option<EapiError>,
}

#[derive(Debug, Deserialize)]
struct EapiResult {
    #[serde(default)]
    output: String,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct EapiError {
    code: i32,
    message: String,
    #[serde(default)]
    data: Option<Vec<EapiErrorData>>,
}

#[derive(Debug, Deserialize)]
struct EapiErrorData {
    #[serde(default)]
    errors: Vec<String>,
}

// ============================================================================
// Configuration Types
// ============================================================================

/// EOS configuration module options parsed from parameters
#[derive(Debug, Clone)]
struct EosConfig {
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
    diff_against: DiffAgainst,
    /// Lines to ignore during diff
    diff_ignore_lines: Vec<String>,
    /// Match mode
    match_mode: MatchMode,
    /// Include defaults in config output
    defaults: bool,
    /// Transport method
    transport: EosTransport,
    /// Configuration session name
    session: Option<String>,
    /// Whether to commit session
    commit: bool,
    /// Whether to abort existing session
    abort: bool,
    /// Session timeout in seconds
    session_timeout: u64,
    /// Intended configuration for diff comparison
    intended_config: Option<String>,
    /// eAPI host
    eapi_host: Option<String>,
    /// eAPI port
    eapi_port: Option<u16>,
    /// Use SSL for eAPI
    eapi_use_ssl: bool,
    /// Validate SSL certificates
    eapi_validate_certs: bool,
    /// eAPI username
    eapi_username: Option<String>,
    /// eAPI password
    eapi_password: Option<String>,
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

impl EosConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let transport = if let Some(t) = params.get_string("transport")? {
            EosTransport::from_str(&t)?
        } else {
            EosTransport::default()
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

        let diff_against = if let Some(d) = params.get_string("diff_against")? {
            DiffAgainst::from_str(&d)?
        } else {
            DiffAgainst::default()
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
            .unwrap_or(EAPI_DEFAULT_TIMEOUT as i64) as u64;

        let session_timeout = params
            .get_i64("session_timeout")?
            .unwrap_or(DEFAULT_SESSION_TIMEOUT as i64) as u64;

        Ok(Self {
            lines: params.get_vec_string("lines")?,
            parents: params.get_vec_string("parents")?,
            src: params.get_string("src")?,
            replace,
            backup: params.get_bool_or("backup", false),
            backup_options,
            running_config: params.get_string("running_config")?,
            save_when,
            diff_against,
            diff_ignore_lines,
            match_mode,
            defaults: params.get_bool_or("defaults", false),
            transport,
            session: params.get_string("session")?,
            commit: params.get_bool_or("commit", true),
            abort: params.get_bool_or("abort", false),
            session_timeout,
            intended_config: params.get_string("intended_config")?,
            eapi_host: params.get_string("eapi_host")?,
            eapi_port: params.get_u32("eapi_port")?.map(|p| p as u16),
            eapi_use_ssl: params.get_bool_or("eapi_use_ssl", true),
            eapi_validate_certs: params.get_bool_or("eapi_validate_certs", true),
            eapi_username: params.get_string("eapi_username")?,
            eapi_password: params.get_string("eapi_password")?,
            timeout,
        })
    }

    /// Check if this is a session abort only operation
    fn is_abort_only(&self) -> bool {
        self.abort && self.session.is_some() && self.lines.is_none() && self.src.is_none()
    }

    /// Get the effective eAPI port
    fn effective_eapi_port(&self) -> u16 {
        self.eapi_port.unwrap_or_else(|| {
            if self.eapi_use_ssl {
                EAPI_DEFAULT_HTTPS_PORT
            } else {
                EAPI_DEFAULT_HTTP_PORT
            }
        })
    }
}

// ============================================================================
// EOS Config Module Implementation
// ============================================================================

/// Arista EOS configuration management module
///
/// Manages configuration on Arista EOS devices using the eAPI (JSON-RPC)
/// interface. Supports configuration sessions for atomic changes, replace
/// and merge modes, and comprehensive diff output.
pub struct EosConfigModule;

impl EosConfigModule {
    /// Build execute options with privilege escalation if needed
    fn build_execute_options(context: &ModuleContext) -> Option<ExecuteOptions> {
        if context.r#become {
            Some(ExecuteOptions {
                escalate: true,
                escalate_user: context.become_user.clone(),
                escalate_method: context.become_method.clone(),
                ..Default::default()
            })
        } else {
            None
        }
    }

    /// Build eAPI client with appropriate SSL settings
    fn build_eapi_client(config: &EosConfig) -> ModuleResult<Client> {
        let builder = Client::builder().timeout(Duration::from_secs(config.timeout));

        let client = if config.eapi_use_ssl && !config.eapi_validate_certs {
            builder.danger_accept_invalid_certs(true).build()
        } else {
            builder.build()
        };

        client.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create HTTP client: {}", e))
        })
    }

    /// Build eAPI URL
    fn build_eapi_url(config: &EosConfig) -> ModuleResult<String> {
        let host = config.eapi_host.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("eapi_host is required for eAPI transport".to_string())
        })?;

        let port = config.effective_eapi_port();
        let scheme = if config.eapi_use_ssl { "https" } else { "http" };

        Ok(format!("{}://{}:{}/command-api", scheme, host, port))
    }

    /// Get eAPI credentials from config or context
    fn get_eapi_credentials(
        config: &EosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<(String, String)> {
        let username = config
            .eapi_username
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
                    "eapi_username is required for eAPI transport".to_string(),
                )
            })?;

        let password = config
            .eapi_password
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
                    "eapi_password is required for eAPI transport".to_string(),
                )
            })?;

        Ok((username, password))
    }

    /// Execute commands via eAPI
    async fn execute_eapi_commands(
        config: &EosConfig,
        commands: &[EapiCommand],
        context: &ModuleContext,
    ) -> ModuleResult<Vec<EapiResult>> {
        let client = Self::build_eapi_client(config)?;
        let url = Self::build_eapi_url(config)?;
        let (username, password) = Self::get_eapi_credentials(config, context)?;

        let request = EapiRequest {
            jsonrpc: "2.0".to_string(),
            method: "runCmds".to_string(),
            params: EapiParams {
                version: 1,
                cmds: commands.to_vec(),
                format: "text".to_string(),
                timestamps: None,
                auto_complete: None,
                expand_aliases: None,
            },
            id: uuid::Uuid::new_v4().to_string(),
        };

        let response = client
            .post(&url)
            .basic_auth(&username, Some(&password))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("eAPI request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ModuleError::ExecutionFailed(format!(
                "eAPI returned error status {}: {}",
                status, body
            )));
        }

        let eapi_response: EapiResponse = response.json().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to parse eAPI response: {}", e))
        })?;

        if let Some(error) = eapi_response.error {
            let error_details = error
                .data
                .map(|d| {
                    d.iter()
                        .flat_map(|ed| ed.errors.iter())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();

            return Err(ModuleError::ExecutionFailed(format!(
                "eAPI error {}: {} {}",
                error.code, error.message, error_details
            )));
        }

        eapi_response
            .result
            .ok_or_else(|| ModuleError::ExecutionFailed("eAPI returned no result".to_string()))
    }

    /// Execute a single eAPI command and return output
    async fn execute_eapi_single(
        config: &EosConfig,
        command: &str,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let commands = vec![EapiCommand::Simple(command.to_string())];
        let results = Self::execute_eapi_commands(config, &commands, context).await?;

        results
            .into_iter()
            .next()
            .map(|r| r.output)
            .ok_or_else(|| ModuleError::ExecutionFailed("No output from eAPI".to_string()))
    }

    /// Get running configuration via eAPI
    async fn get_running_config_eapi(
        config: &EosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let cmd = if config.defaults {
            "show running-config all"
        } else {
            "show running-config"
        };

        Self::execute_eapi_single(config, cmd, context).await
    }

    /// Get startup configuration via eAPI
    async fn get_startup_config_eapi(
        config: &EosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        Self::execute_eapi_single(config, "show startup-config", context).await
    }

    /// Get session configuration diff via eAPI
    async fn get_session_diff_eapi(
        config: &EosConfig,
        session_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        // Validate session name
        if !session_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid session name '{}': must contain only alphanumeric characters, underscores, and hyphens",
                session_name
            )));
        }

        let cmd = format!("show session-config named {} diffs", session_name);
        Self::execute_eapi_single(config, &cmd, context).await
    }

    /// Create or enter a configuration session via eAPI
    async fn enter_session_eapi(
        config: &EosConfig,
        session_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Validate session name
        if !session_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid session name '{}': must contain only alphanumeric characters, underscores, and hyphens",
                session_name
            )));
        }

        let cmd = format!("configure session {}", session_name);
        let commands = vec![EapiCommand::Simple(cmd)];
        Self::execute_eapi_commands(config, &commands, context).await?;
        Ok(())
    }

    /// Commit a configuration session via eAPI
    async fn commit_session_eapi(
        config: &EosConfig,
        session_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Validate session name
        if !session_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid session name '{}'",
                session_name
            )));
        }

        // First enter the session, then commit
        let commands = vec![
            EapiCommand::Simple(format!("configure session {}", session_name)),
            EapiCommand::Simple("commit".to_string()),
        ];
        Self::execute_eapi_commands(config, &commands, context).await?;
        Ok(())
    }

    /// Abort a configuration session via eAPI
    async fn abort_session_eapi(
        config: &EosConfig,
        session_name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Validate session name
        if !session_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid session name '{}'",
                session_name
            )));
        }

        // Enter session and abort
        let commands = vec![
            EapiCommand::Simple(format!("configure session {}", session_name)),
            EapiCommand::Simple("abort".to_string()),
        ];
        Self::execute_eapi_commands(config, &commands, context).await?;
        Ok(())
    }

    /// Apply configuration lines via eAPI (with optional session)
    async fn apply_config_eapi(
        config: &EosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, Vec<String>, Option<String>)> {
        let mut commands: Vec<EapiCommand> = Vec::new();
        let mut config_commands = Vec::new();

        // Build command list with parents and lines
        if let Some(ref parents) = config.parents {
            for parent in parents {
                config_commands.push(parent.clone());
            }
        }

        if let Some(ref lines) = config.lines {
            config_commands.extend(lines.clone());
        }

        // Read from source file if specified
        if let Some(ref src) = config.src {
            let content = std::fs::read_to_string(src).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read source file '{}': {}", src, e))
            })?;

            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('!') && !trimmed.starts_with('#') {
                    config_commands.push(trimmed.to_string());
                }
            }
        }

        if config_commands.is_empty() {
            return Ok((false, Vec::new(), None));
        }

        // Enter configuration mode (or session)
        if let Some(ref session_name) = config.session {
            // Validate session name
            if !session_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid session name '{}'",
                    session_name
                )));
            }
            commands.push(EapiCommand::Simple(format!(
                "configure session {}",
                session_name
            )));
        } else {
            commands.push(EapiCommand::Simple("configure".to_string()));
        }

        // Add configuration commands
        for cmd in &config_commands {
            commands.push(EapiCommand::Simple(cmd.clone()));
        }

        // Handle session commit or just exit
        if let Some(ref _session_name) = config.session {
            if config.commit {
                commands.push(EapiCommand::Simple("commit".to_string()));
            }
            // Session will be left open if not committed
        } else {
            commands.push(EapiCommand::Simple("end".to_string()));
        }

        // Execute commands
        Self::execute_eapi_commands(config, &commands, context).await?;

        // Get session diff if using session and not committing yet
        let session_diff = if config.session.is_some() && !config.commit {
            if let Some(ref session_name) = config.session {
                Some(
                    Self::get_session_diff_eapi(config, session_name, context)
                        .await
                        .unwrap_or_default(),
                )
            } else {
                None
            }
        } else {
            None
        };

        Ok((true, config_commands, session_diff))
    }

    /// Replace configuration via eAPI
    async fn replace_config_eapi(
        config: &EosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let src = config.src.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter(
                "src parameter is required for config replace".to_string(),
            )
        })?;

        // Read the config file content
        let content = std::fs::read_to_string(src).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to read source file '{}': {}", src, e))
        })?;

        // Build commands
        let mut commands: Vec<EapiCommand> = Vec::new();

        // Use session for replace if specified, otherwise use configure replace
        if let Some(ref session_name) = config.session {
            // Validate session name
            if !session_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid session name '{}'",
                    session_name
                )));
            }
            commands.push(EapiCommand::Simple(format!(
                "configure session {}",
                session_name
            )));
        } else {
            commands.push(EapiCommand::Simple(
                "configure replace terminal:".to_string(),
            ));
        }

        // Add configuration lines
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                commands.push(EapiCommand::Simple(trimmed.to_string()));
            }
        }

        // End configuration or commit session
        if let Some(ref _session_name) = config.session {
            if config.commit {
                commands.push(EapiCommand::Simple("commit".to_string()));
            }
        } else {
            commands.push(EapiCommand::Simple("EOF".to_string()));
        }

        // Execute commands
        Self::execute_eapi_commands(config, &commands, context).await?;

        Ok(true)
    }

    /// Save running-config to startup-config via eAPI
    async fn save_config_eapi(config: &EosConfig, context: &ModuleContext) -> ModuleResult<()> {
        Self::execute_eapi_single(config, "write memory", context).await?;
        Ok(())
    }

    /// Backup running-config
    async fn backup_config(
        config: &EosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        if !config.backup {
            return Ok(None);
        }

        let running_config = Self::get_running_config_eapi(config, context).await?;

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
                    "eos_backup_{}.cfg",
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

    /// Get running configuration via SSH
    async fn get_running_config_ssh(
        connection: &dyn Connection,
        config: &EosConfig,
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

    /// Apply configuration via SSH
    async fn apply_config_ssh(
        connection: &dyn Connection,
        config: &EosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, Vec<String>)> {
        let mut config_commands = Vec::new();

        // Build command list with parents and lines
        if let Some(ref parents) = config.parents {
            for parent in parents {
                config_commands.push(parent.clone());
            }
        }

        if let Some(ref lines) = config.lines {
            config_commands.extend(lines.clone());
        }

        // Read from source file if specified
        if let Some(ref src) = config.src {
            let content = std::fs::read_to_string(src).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read source file '{}': {}", src, e))
            })?;

            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('!') && !trimmed.starts_with('#') {
                    config_commands.push(trimmed.to_string());
                }
            }
        }

        if config_commands.is_empty() {
            return Ok((false, Vec::new()));
        }

        // Build full command string
        let mut full_cmd = String::new();

        if let Some(ref session_name) = config.session {
            // Validate session name
            if !session_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid session name '{}'",
                    session_name
                )));
            }
            full_cmd.push_str(&format!("configure session {}\n", session_name));
        } else {
            full_cmd.push_str("configure\n");
        }

        for cmd in &config_commands {
            full_cmd.push_str(cmd);
            full_cmd.push('\n');
        }

        if config.session.is_some() && config.commit {
            full_cmd.push_str("commit\n");
        } else if config.session.is_none() {
            full_cmd.push_str("end\n");
        }

        let result = Self::execute_ssh_command(connection, &full_cmd, context).await?;

        if result.success {
            Ok((true, config_commands))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Configuration failed: {}",
                result.stderr
            )))
        }
    }

    /// Save configuration via SSH
    async fn save_config_ssh(
        connection: &dyn Connection,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let result = Self::execute_ssh_command(connection, "write memory", context).await?;

        if result.success || result.stdout.contains("Copy completed") {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to save configuration: {}",
                result.stderr
            )))
        }
    }

    /// Execute the module with eAPI transport
    async fn execute_async_eapi(
        &self,
        config: &EosConfig,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut messages = Vec::new();
        let mut data = HashMap::new();
        let mut diff_output: Option<Diff> = None;

        // Get running config before changes for diff
        let before_config = if config.diff_against == DiffAgainst::Running
            || config.diff_against == DiffAgainst::Startup
        {
            Some(match config.diff_against {
                DiffAgainst::Running => Self::get_running_config_eapi(config, context).await?,
                DiffAgainst::Startup => Self::get_startup_config_eapi(config, context).await?,
                _ => String::new(),
            })
        } else {
            None
        };

        // Backup current configuration if requested
        if let Some(backup_path) = Self::backup_config(config, context).await? {
            messages.push(format!("Backup saved to {}", backup_path));
            data.insert("backup_path".to_string(), serde_json::json!(backup_path));
        }

        // Handle session abort
        if config.abort {
            if let Some(ref session_name) = config.session {
                if context.check_mode {
                    messages.push(format!("Would abort session '{}'", session_name));
                    changed = true;
                } else {
                    Self::abort_session_eapi(config, session_name, context).await?;
                    messages.push(format!("Aborted session '{}'", session_name));
                    changed = true;
                }

                if config.is_abort_only() {
                    let msg = messages.join(". ");
                    return Ok(if changed {
                        ModuleOutput::changed(msg)
                    } else {
                        ModuleOutput::ok(msg)
                    });
                }
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
                if let Some(ref session) = config.session {
                    messages.push(format!("Would use session '{}'", session));
                    if config.commit {
                        messages.push("Would commit session".to_string());
                    }
                }
                changed = true;
            } else {
                // Handle replace mode
                if config.replace == ReplaceMode::Config {
                    let replaced = Self::replace_config_eapi(config, context).await?;
                    if replaced {
                        messages.push("Configuration replaced successfully".to_string());
                        changed = true;
                    }
                } else {
                    let (config_changed, applied, session_diff) =
                        Self::apply_config_eapi(config, context).await?;

                    if config_changed {
                        messages.push(format!("Applied {} configuration commands", applied.len()));
                        data.insert("commands".to_string(), serde_json::json!(applied));
                        changed = true;

                        // Include session diff if available
                        if let Some(diff_text) = session_diff {
                            if !diff_text.is_empty() {
                                data.insert(
                                    "session_diff".to_string(),
                                    serde_json::json!(diff_text),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Generate diff output
        if changed && !context.check_mode {
            if let Some(before) = before_config {
                let after = Self::get_running_config_eapi(config, context).await?;
                let diff_details = generate_unified_diff(&before, &after);

                if !diff_details.is_empty() {
                    diff_output = Some(
                        Diff::new(
                            format!("{} lines", before.lines().count()),
                            format!("{} lines", after.lines().count()),
                        )
                        .with_details(diff_details),
                    );
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
            Self::save_config_eapi(config, context).await?;
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

        if let Some(diff) = diff_output {
            output = output.with_diff(diff);
        }

        for (key, value) in data {
            output = output.with_data(key, value);
        }

        Ok(output)
    }

    /// Execute the module with SSH transport
    async fn execute_async_ssh(
        &self,
        config: &EosConfig,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut messages = Vec::new();
        let mut data = HashMap::new();
        let mut diff_output: Option<Diff> = None;

        // Get running config before changes for diff
        let before_config =
            Self::get_running_config_ssh(connection.as_ref(), config, context).await?;

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
                let (config_changed, applied) =
                    Self::apply_config_ssh(connection.as_ref(), config, context).await?;

                if config_changed {
                    messages.push(format!("Applied {} configuration commands", applied.len()));
                    data.insert("commands".to_string(), serde_json::json!(applied));
                    changed = true;
                }
            }
        }

        // Generate diff output
        if changed && !context.check_mode {
            let after = Self::get_running_config_ssh(connection.as_ref(), config, context).await?;
            let diff_details = generate_unified_diff(&before_config, &after);

            if !diff_details.is_empty() {
                diff_output = Some(
                    Diff::new(
                        format!("{} lines", before_config.lines().count()),
                        format!("{} lines", after.lines().count()),
                    )
                    .with_details(diff_details),
                );
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

        if let Some(diff) = diff_output {
            output = output.with_diff(diff);
        }

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
        let config = EosConfig::from_params(params)?;

        match config.transport {
            EosTransport::Eapi => self.execute_async_eapi(&config, context).await,
            EosTransport::Ssh => {
                let connection = context.connection.clone().ok_or_else(|| {
                    ModuleError::ExecutionFailed(
                        "No SSH connection available for EOS module".to_string(),
                    )
                })?;
                self.execute_async_ssh(&config, context, connection).await
            }
        }
    }

    /// Generate diff for configuration changes
    async fn diff_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<Option<Diff>> {
        let config = EosConfig::from_params(params)?;

        // Get current running config based on transport
        let current_config = match config.transport {
            EosTransport::Eapi => Self::get_running_config_eapi(&config, context).await?,
            EosTransport::Ssh => {
                if let Some(ref connection) = context.connection {
                    Self::get_running_config_ssh(connection.as_ref(), &config, context).await?
                } else {
                    return Ok(None);
                }
            }
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
            let content = std::fs::read_to_string(src).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read source file: {}", e))
            })?;
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('!') && !trimmed.starts_with('#') {
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

/// Generate a unified diff between two configurations
fn generate_unified_diff(before: &str, after: &str) -> String {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(before, after);
    let mut output = String::new();

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        output.push_str(&format!("{}{}", sign, change));
    }

    output
}

impl Module for EosConfigModule {
    fn name(&self) -> &'static str {
        "eos_config"
    }

    fn description(&self) -> &'static str {
        "Manage Arista EOS configuration with eAPI support, sessions, and diff output"
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
        // No strictly required params - can be session operation or config
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let config = EosConfig::from_params(params)?;

        // Must have either config changes or session operation
        let has_config = config.lines.is_some() || config.src.is_some();
        let has_session_op = config.session.is_some() && (config.commit || config.abort);

        if !has_config && !has_session_op {
            return Err(ModuleError::InvalidParameter(
                "Must provide either configuration (lines/src) or session operation (session with commit/abort)".to_string(),
            ));
        }

        // eAPI requires host
        if config.transport == EosTransport::Eapi && config.eapi_host.is_none() {
            return Err(ModuleError::MissingParameter(
                "eapi_host is required when using eAPI transport".to_string(),
            ));
        }

        // Config replace requires src
        if config.replace == ReplaceMode::Config && config.src.is_none() {
            return Err(ModuleError::MissingParameter(
                "src parameter is required for config replace mode".to_string(),
            ));
        }

        // Diff against intended requires intended_config
        if config.diff_against == DiffAgainst::Intended && config.intended_config.is_none() {
            return Err(ModuleError::MissingParameter(
                "intended_config is required when diff_against is 'intended'".to_string(),
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

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        self.execute(params, &check_context)
    }

    fn diff(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let handle = match tokio::runtime::Handle::try_current() {
            Ok(h) => h,
            Err(_) => return Ok(None),
        };

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.diff_async(&params, &context)))
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
        assert_eq!(EosTransport::from_str("eapi").unwrap(), EosTransport::Eapi);
        assert_eq!(EosTransport::from_str("api").unwrap(), EosTransport::Eapi);
        assert_eq!(EosTransport::from_str("https").unwrap(), EosTransport::Eapi);
        assert_eq!(EosTransport::from_str("ssh").unwrap(), EosTransport::Ssh);
        assert_eq!(EosTransport::from_str("cli").unwrap(), EosTransport::Ssh);
        assert!(EosTransport::from_str("invalid").is_err());
    }

    #[test]
    fn test_replace_mode_from_str() {
        assert_eq!(ReplaceMode::from_str("line").unwrap(), ReplaceMode::Line);
        assert_eq!(ReplaceMode::from_str("merge").unwrap(), ReplaceMode::Line);
        assert_eq!(ReplaceMode::from_str("block").unwrap(), ReplaceMode::Block);
        assert_eq!(
            ReplaceMode::from_str("config").unwrap(),
            ReplaceMode::Config
        );
        assert_eq!(
            ReplaceMode::from_str("replace").unwrap(),
            ReplaceMode::Config
        );
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
    fn test_diff_against_from_str() {
        assert_eq!(
            DiffAgainst::from_str("running").unwrap(),
            DiffAgainst::Running
        );
        assert_eq!(
            DiffAgainst::from_str("startup").unwrap(),
            DiffAgainst::Startup
        );
        assert_eq!(
            DiffAgainst::from_str("intended").unwrap(),
            DiffAgainst::Intended
        );
        assert_eq!(
            DiffAgainst::from_str("session").unwrap(),
            DiffAgainst::Session
        );
        assert!(DiffAgainst::from_str("invalid").is_err());
    }

    #[test]
    fn test_eos_config_module_metadata() {
        let module = EosConfigModule;
        assert_eq!(module.name(), "eos_config");
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
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let config = EosConfig::from_params(&params).unwrap();
        assert_eq!(config.lines.as_ref().unwrap().len(), 2);
        assert_eq!(config.transport, EosTransport::Eapi);
        assert_eq!(config.replace, ReplaceMode::Line);
        assert_eq!(config.save_when, SaveWhen::Never);
        assert!(!config.backup);
        assert!(config.commit); // Default is true
    }

    #[test]
    fn test_config_from_params_with_parents() {
        let mut params = ModuleParams::new();
        params.insert(
            "parents".to_string(),
            serde_json::json!(["interface Ethernet1"]),
        );
        params.insert(
            "lines".to_string(),
            serde_json::json!(["description Test", "no shutdown"]),
        );
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let config = EosConfig::from_params(&params).unwrap();
        assert_eq!(config.parents.as_ref().unwrap().len(), 1);
        assert_eq!(config.lines.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_config_from_params_session() {
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("session".to_string(), serde_json::json!("my_session"));
        params.insert("commit".to_string(), serde_json::json!(false));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let config = EosConfig::from_params(&params).unwrap();
        assert_eq!(config.session.as_ref().unwrap(), "my_session");
        assert!(!config.commit);
    }

    #[test]
    fn test_config_from_params_eapi() {
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["feature bgp"]));
        params.insert("transport".to_string(), serde_json::json!("eapi"));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));
        params.insert("eapi_port".to_string(), serde_json::json!(8443));
        params.insert("eapi_use_ssl".to_string(), serde_json::json!(true));
        params.insert("eapi_validate_certs".to_string(), serde_json::json!(false));

        let config = EosConfig::from_params(&params).unwrap();
        assert_eq!(config.transport, EosTransport::Eapi);
        assert_eq!(config.eapi_host.as_ref().unwrap(), "192.168.1.1");
        assert_eq!(config.eapi_port, Some(8443));
        assert!(config.eapi_use_ssl);
        assert!(!config.eapi_validate_certs);
    }

    #[test]
    fn test_config_from_params_backup() {
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("backup".to_string(), serde_json::json!(true));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));
        params.insert(
            "backup_options".to_string(),
            serde_json::json!({
                "dir_path": "/backups",
                "filename": "mybackup.cfg"
            }),
        );

        let config = EosConfig::from_params(&params).unwrap();
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
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let config = EosConfig::from_params(&params).unwrap();
        assert_eq!(config.replace, ReplaceMode::Config);
        assert_eq!(config.src.as_ref().unwrap(), "/path/to/config.txt");
    }

    #[test]
    fn test_effective_eapi_port() {
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        // Default HTTPS port
        params.insert("eapi_use_ssl".to_string(), serde_json::json!(true));
        let config = EosConfig::from_params(&params).unwrap();
        assert_eq!(config.effective_eapi_port(), 443);

        // Default HTTP port
        params.insert("eapi_use_ssl".to_string(), serde_json::json!(false));
        let config = EosConfig::from_params(&params).unwrap();
        assert_eq!(config.effective_eapi_port(), 80);

        // Custom port
        params.insert("eapi_port".to_string(), serde_json::json!(8080));
        let config = EosConfig::from_params(&params).unwrap();
        assert_eq!(config.effective_eapi_port(), 8080);
    }

    #[test]
    fn test_validate_params_requires_action() {
        let module = EosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_eapi_requires_host() {
        let module = EosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("eapi"));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_replace_requires_src() {
        let module = EosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("replace".to_string(), serde_json::json!("config"));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_valid_config() {
        let module = EosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_params_valid_session() {
        let module = EosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("session".to_string(), serde_json::json!("test"));
        params.insert("abort".to_string(), serde_json::json!(true));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_unified_diff() {
        let before = "line1\nline2\nline3\n";
        let after = "line1\nline2_modified\nline3\nline4\n";

        let diff = generate_unified_diff(before, after);
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+line2_modified"));
        assert!(diff.contains("+line4"));
    }

    #[test]
    fn test_is_abort_only() {
        let mut params = ModuleParams::new();
        params.insert("session".to_string(), serde_json::json!("test"));
        params.insert("abort".to_string(), serde_json::json!(true));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let config = EosConfig::from_params(&params).unwrap();
        assert!(config.is_abort_only());

        // With lines, should not be abort only
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        let config = EosConfig::from_params(&params).unwrap();
        assert!(!config.is_abort_only());
    }

    #[test]
    fn test_config_diff_against_intended_requires_config() {
        let module = EosConfigModule;
        let mut params = ModuleParams::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("diff_against".to_string(), serde_json::json!("intended"));
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let result = module.validate_params(&params);
        assert!(result.is_err());

        // With intended_config, should pass
        params.insert(
            "intended_config".to_string(),
            serde_json::json!("vlan 100\nname Test"),
        );
        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }
}
