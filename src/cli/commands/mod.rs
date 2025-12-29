//! Subcommands module for Rustible CLI
//!
//! This module contains all the subcommand implementations.

pub mod check;
pub mod galaxy;
pub mod inventory;
pub mod provision;
pub mod run;
pub mod vault;

use crate::cli::output::OutputFormatter;
use crate::config::Config;
use anyhow::Result;
use rustible::connection::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Common context shared between commands
pub struct CommandContext {
    /// Configuration
    pub config: Config,
    /// Output formatter
    pub output: OutputFormatter,
    /// Inventory path
    pub inventory_path: Option<PathBuf>,
    /// Extra variables
    pub extra_vars: Vec<String>,
    /// Verbosity level
    #[allow(dead_code)]
    pub verbosity: u8,
    /// Check mode (dry-run)
    pub check_mode: bool,
    /// Diff mode
    #[allow(dead_code)]
    pub diff_mode: bool,
    /// Limit pattern
    pub limit: Option<String>,
    /// Number of parallel forks
    #[allow(dead_code)]
    pub forks: usize,
    /// Connection timeout
    #[allow(dead_code)]
    pub timeout: u64,
    /// Connection pool for reusing SSH connections
    pub connections: Arc<RwLock<HashMap<String, Arc<dyn Connection + Send + Sync>>>>,
}

impl CommandContext {
    /// Create a new command context from CLI arguments
    pub fn new(cli: &crate::cli::Cli, config: Config) -> Self {
        let output = OutputFormatter::new(!cli.no_color, cli.is_json(), cli.verbosity());

        Self {
            config,
            output,
            inventory_path: cli.inventory.clone(),
            extra_vars: cli.extra_vars.clone(),
            verbosity: cli.verbosity(),
            check_mode: cli.check_mode,
            diff_mode: cli.diff_mode,
            limit: cli.limit.clone(),
            forks: cli.forks,
            timeout: cli.timeout,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or create a connection for a host
    /// This pools connections to avoid creating new SSH sessions for every command
    pub async fn get_connection(
        &self,
        host: &str,
        ansible_host: &str,
        ansible_user: &str,
        ansible_port: u16,
        ansible_key: Option<&str>,
    ) -> Result<Arc<dyn Connection + Send + Sync>> {
        // Check if we already have a connection for this host
        {
            let connections = self.connections.read().await;
            if let Some(conn) = connections.get(host) {
                if conn.is_alive().await {
                    self.output
                        .debug(&format!("Reusing connection for {}", host));
                    return Ok(Arc::clone(conn));
                }
            }
        }

        self.output.debug(&format!(
            "Creating new SSH connection: {}@{}:{}",
            ansible_user, ansible_host, ansible_port
        ));

        // Build host config for SSH connection
        let mut host_config = rustible::connection::HostConfig::default();
        host_config.hostname = Some(ansible_host.to_string());
        host_config.port = Some(ansible_port);
        host_config.user = Some(ansible_user.to_string());
        if let Some(key_path) = ansible_key {
            // Expand ~ to home directory
            let expanded_path = if key_path.starts_with("~/") {
                if let Some(home) = dirs::home_dir() {
                    home.join(&key_path[2..]).to_string_lossy().to_string()
                } else {
                    key_path.to_string()
                }
            } else {
                key_path.to_string()
            };
            host_config.identity_file = Some(expanded_path);
        }

        // Create SSH connection - prefer russh (pure Rust) when available
        let conn_config = rustible::connection::ConnectionConfig::default();
        #[cfg(feature = "russh")]
        let conn: Arc<dyn Connection + Send + Sync> = {
            let conn = rustible::connection::russh::RusshConnection::connect(
                ansible_host,
                ansible_port,
                ansible_user,
                Some(host_config),
                &conn_config,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to {}: {}", host, e))?;
            Arc::new(conn)
        };
        #[cfg(all(feature = "ssh2-backend", not(feature = "russh")))]
        let conn: Arc<dyn Connection + Send + Sync> = {
            let conn = rustible::connection::ssh::SshConnection::connect(
                ansible_host,
                ansible_port,
                ansible_user,
                Some(host_config),
                &conn_config,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to {}: {}", host, e))?;
            Arc::new(conn)
        };
        #[cfg(not(any(feature = "russh", feature = "ssh2-backend")))]
        let conn: Arc<dyn Connection + Send + Sync> = {
            return Err(anyhow::anyhow!(
                "No SSH backend available. Enable 'russh' or 'ssh2-backend' feature."
            ));
        };

        // Cache the connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(host.to_string(), Arc::clone(&conn));
        }

        Ok(conn)
    }

    /// Close all cached connections
    pub async fn close_connections(&self) {
        let connections: Vec<_> = {
            let mut pool = self.connections.write().await;
            pool.drain().map(|(_, v)| v).collect()
        };

        for conn in connections {
            let _ = conn.close().await;
        }
    }

    /// Get the effective inventory path
    pub fn inventory(&self) -> Option<&PathBuf> {
        self.inventory_path
            .as_ref()
            .or(self.config.defaults.inventory.as_ref())
    }

    /// Parse extra variables into a HashMap
    pub fn parse_extra_vars(&self) -> Result<std::collections::HashMap<String, serde_yaml::Value>> {
        use std::collections::HashMap;

        let mut vars = HashMap::new();

        for var in &self.extra_vars {
            if let Some(file_path) = var.strip_prefix('@') {
                // Load from file
                let content = std::fs::read_to_string(file_path)?;
                let file_vars: HashMap<String, serde_yaml::Value> = serde_yaml::from_str(&content)?;
                vars.extend(file_vars);
            } else if let Some((key, value)) = var.split_once('=') {
                // Parse key=value
                let parsed_value: serde_yaml::Value = serde_yaml::from_str(value)
                    .unwrap_or_else(|_| serde_yaml::Value::String(value.to_string()));
                vars.insert(key.to_string(), parsed_value);
            }
        }

        Ok(vars)
    }
}

/// Trait for runnable commands
#[async_trait::async_trait]
#[allow(dead_code)]
pub trait Runnable {
    /// Execute the command
    async fn run(&self, ctx: &mut CommandContext) -> Result<i32>;
}
