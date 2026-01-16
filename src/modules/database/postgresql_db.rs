//! PostgreSQL Database module - Database management
//!
//! This module manages PostgreSQL databases including creation, deletion,
//! backup (pg_dump), and restore (pg_restore).
//!
//! ## Parameters
//!
//! - `name`: Database name (required)
//! - `state`: Desired state (present, absent, dump, restore) - default: present
//! - `owner`: Database owner
//! - `encoding`: Database encoding (default: UTF8)
//! - `lc_collate`: Collation order (LC_COLLATE)
//! - `lc_ctype`: Character classification (LC_CTYPE)
//! - `template`: Template database (default: template0)
//! - `tablespace`: Default tablespace
//! - `conn_limit`: Connection limit (-1 for unlimited)
//! - `login_host`: PostgreSQL server host (default: localhost)
//! - `login_port`: PostgreSQL server port (default: 5432)
//! - `login_user`: PostgreSQL login user (default: postgres)
//! - `login_password`: PostgreSQL login password
//! - `login_unix_socket`: Unix socket path for local connections
//! - `ssl_mode`: SSL mode (disable, allow, prefer, require, verify-ca, verify-full)
//! - `ca_cert`: Path to CA certificate for SSL
//! - `target`: Target path for dump/restore operations
//! - `target_opts`: Additional options for pg_dump/pg_restore
//! - `dump_extra_args`: Extra arguments for pg_dump
//! - `maintenance_db`: Database to connect for admin operations (default: postgres)
//! - `force`: Force drop database even with active connections

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::utils::shell_escape;
use std::collections::HashMap;
use std::sync::Arc;

/// Desired state for a database
#[derive(Debug, Clone, PartialEq)]
pub enum DbState {
    /// Database should exist
    Present,
    /// Database should not exist
    Absent,
    /// Create a database dump
    Dump,
    /// Restore database from dump
    Restore,
}

impl DbState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(DbState::Present),
            "absent" => Ok(DbState::Absent),
            "dump" => Ok(DbState::Dump),
            "restore" => Ok(DbState::Restore),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, dump, restore",
                s
            ))),
        }
    }
}

/// SSL mode for PostgreSQL connections
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SslMode {
    Disable,
    Allow,
    #[default]
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "disable" => Ok(SslMode::Disable),
            "allow" => Ok(SslMode::Allow),
            "prefer" => Ok(SslMode::Prefer),
            "require" => Ok(SslMode::Require),
            "verify-ca" | "verify_ca" => Ok(SslMode::VerifyCa),
            "verify-full" | "verify_full" => Ok(SslMode::VerifyFull),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid ssl_mode '{}'. Valid modes: disable, allow, prefer, require, verify-ca, verify-full",
                s
            ))),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            SslMode::Disable => "disable",
            SslMode::Allow => "allow",
            SslMode::Prefer => "prefer",
            SslMode::Require => "require",
            SslMode::VerifyCa => "verify-ca",
            SslMode::VerifyFull => "verify-full",
        }
    }
}

/// PostgreSQL connection configuration
#[derive(Debug, Clone)]
pub struct PgConnectionConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub unix_socket: Option<String>,
    pub ssl_mode: SslMode,
    pub ca_cert: Option<String>,
    pub maintenance_db: String,
}

impl Default for PgConnectionConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            user: "postgres".to_string(),
            password: None,
            unix_socket: None,
            ssl_mode: SslMode::Prefer,
            ca_cert: None,
            maintenance_db: "postgres".to_string(),
        }
    }
}

impl PgConnectionConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let ssl_mode = if let Some(mode) = params.get_string("ssl_mode")? {
            SslMode::from_str(&mode)?
        } else {
            SslMode::Prefer
        };

        Ok(Self {
            host: params
                .get_string("login_host")?
                .unwrap_or_else(|| "localhost".to_string()),
            port: params.get_u32("login_port")?.unwrap_or(5432) as u16,
            user: params
                .get_string("login_user")?
                .unwrap_or_else(|| "postgres".to_string()),
            password: params.get_string("login_password")?,
            unix_socket: params.get_string("login_unix_socket")?,
            ssl_mode,
            ca_cert: params.get_string("ca_cert")?,
            maintenance_db: params
                .get_string("maintenance_db")?
                .unwrap_or_else(|| "postgres".to_string()),
        })
    }

    /// Build environment variables for psql/pg_dump commands
    pub fn build_env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("PGHOST".to_string(), self.host.clone());
        env.insert("PGPORT".to_string(), self.port.to_string());
        env.insert("PGUSER".to_string(), self.user.clone());

        if let Some(ref password) = self.password {
            env.insert("PGPASSWORD".to_string(), password.clone());
        }

        if let Some(ref socket) = self.unix_socket {
            env.insert("PGHOST".to_string(), socket.clone());
        }

        env.insert("PGSSLMODE".to_string(), self.ssl_mode.as_str().to_string());

        if let Some(ref ca_cert) = self.ca_cert {
            env.insert("PGSSLROOTCERT".to_string(), ca_cert.clone());
        }

        env
    }

    /// Build psql connection string arguments
    pub fn build_psql_args(&self, database: &str) -> String {
        let mut args = Vec::new();

        if self.unix_socket.is_some() {
            args.push(format!(
                "-h {}",
                shell_escape(self.unix_socket.as_ref().unwrap())
            ));
        } else {
            args.push(format!("-h {}", shell_escape(&self.host)));
        }

        args.push(format!("-p {}", self.port));
        args.push(format!("-U {}", shell_escape(&self.user)));
        args.push(format!("-d {}", shell_escape(database)));

        args.join(" ")
    }
}

/// Database configuration parsed from parameters
#[derive(Debug, Clone)]
struct DbConfig {
    name: String,
    state: DbState,
    owner: Option<String>,
    encoding: String,
    lc_collate: Option<String>,
    lc_ctype: Option<String>,
    template: String,
    tablespace: Option<String>,
    conn_limit: Option<i32>,
    target: Option<String>,
    target_opts: Option<String>,
    dump_extra_args: Option<String>,
    force: bool,
    conn: PgConnectionConfig,
}

impl DbConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let state = if let Some(s) = params.get_string("state")? {
            DbState::from_str(&s)?
        } else {
            DbState::Present
        };

        let conn_limit = if let Some(limit) = params.get_i64("conn_limit")? {
            Some(limit as i32)
        } else {
            None
        };

        Ok(Self {
            name: params.get_string_required("name")?,
            state,
            owner: params.get_string("owner")?,
            encoding: params
                .get_string("encoding")?
                .unwrap_or_else(|| "UTF8".to_string()),
            lc_collate: params.get_string("lc_collate")?,
            lc_ctype: params.get_string("lc_ctype")?,
            template: params
                .get_string("template")?
                .unwrap_or_else(|| "template0".to_string()),
            tablespace: params.get_string("tablespace")?,
            conn_limit,
            target: params.get_string("target")?,
            target_opts: params.get_string("target_opts")?,
            dump_extra_args: params.get_string("dump_extra_args")?,
            force: params.get_bool_or("force", false),
            conn: PgConnectionConfig::from_params(params)?,
        })
    }
}

/// Module for PostgreSQL database management
pub struct PostgresqlDbModule;

impl PostgresqlDbModule {
    /// Build execute options with privilege escalation and environment
    fn build_execute_options(
        context: &ModuleContext,
        env: HashMap<String, String>,
    ) -> ExecuteOptions {
        let mut options = ExecuteOptions::new();

        for (key, value) in env {
            options = options.with_env(&key, &value);
        }

        if context.r#become {
            options.escalate = true;
            options.escalate_user = context.become_user.clone();
            options.escalate_method = context.become_method.clone();
        }

        options
    }

    /// Execute a command via connection
    async fn execute_command(
        connection: &dyn Connection,
        command: &str,
        context: &ModuleContext,
        env: HashMap<String, String>,
    ) -> ModuleResult<(bool, String, String)> {
        let options = Self::build_execute_options(context, env);
        let result = connection
            .execute(command, Some(options))
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Connection execute failed: {}", e))
            })?;
        Ok((result.success, result.stdout, result.stderr))
    }

    /// Check if database exists
    async fn database_exists(
        connection: &dyn Connection,
        config: &DbConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let query = format!(
            "SELECT 1 FROM pg_database WHERE datname = '{}'",
            config.name.replace('\'', "''")
        );
        let cmd = format!(
            "psql {} -tAc \"{}\"",
            config.conn.build_psql_args(&config.conn.maintenance_db),
            query
        );

        let (success, stdout, _) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        Ok(success && stdout.trim() == "1")
    }

    /// Get database info
    async fn get_database_info(
        connection: &dyn Connection,
        config: &DbConfig,
        context: &ModuleContext,
    ) -> ModuleResult<Option<HashMap<String, String>>> {
        let query = format!(
            "SELECT d.datname, pg_catalog.pg_get_userbyid(d.datdba) as owner, \
             pg_catalog.pg_encoding_to_char(d.encoding) as encoding, \
             d.datcollate as lc_collate, d.datctype as lc_ctype, \
             d.datconnlimit as conn_limit, t.spcname as tablespace \
             FROM pg_catalog.pg_database d \
             LEFT JOIN pg_catalog.pg_tablespace t ON d.dattablespace = t.oid \
             WHERE d.datname = '{}'",
            config.name.replace('\'', "''")
        );

        let cmd = format!(
            "psql {} -tAF '|' -c \"{}\"",
            config.conn.build_psql_args(&config.conn.maintenance_db),
            query
        );

        let (success, stdout, _) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if !success || stdout.trim().is_empty() {
            return Ok(None);
        }

        let parts: Vec<&str> = stdout.trim().split('|').collect();
        if parts.len() < 7 {
            return Ok(None);
        }

        let mut info = HashMap::new();
        info.insert("name".to_string(), parts[0].to_string());
        info.insert("owner".to_string(), parts[1].to_string());
        info.insert("encoding".to_string(), parts[2].to_string());
        info.insert("lc_collate".to_string(), parts[3].to_string());
        info.insert("lc_ctype".to_string(), parts[4].to_string());
        info.insert("conn_limit".to_string(), parts[5].to_string());
        info.insert("tablespace".to_string(), parts[6].to_string());

        Ok(Some(info))
    }

    /// Create a database
    async fn create_database(
        connection: &dyn Connection,
        config: &DbConfig,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let mut options = Vec::new();

        if let Some(ref owner) = config.owner {
            options.push(format!("OWNER {}", shell_escape(owner)));
        }

        options.push(format!("ENCODING '{}'", config.encoding));
        options.push(format!("TEMPLATE {}", shell_escape(&config.template)));

        if let Some(ref lc_collate) = config.lc_collate {
            options.push(format!("LC_COLLATE '{}'", lc_collate));
        }

        if let Some(ref lc_ctype) = config.lc_ctype {
            options.push(format!("LC_CTYPE '{}'", lc_ctype));
        }

        if let Some(ref tablespace) = config.tablespace {
            options.push(format!("TABLESPACE {}", shell_escape(tablespace)));
        }

        if let Some(conn_limit) = config.conn_limit {
            options.push(format!("CONNECTION LIMIT {}", conn_limit));
        }

        let create_sql = format!(
            "CREATE DATABASE {} {}",
            shell_escape(&config.name),
            options.join(" ")
        );

        let cmd = format!(
            "psql {} -c \"{}\"",
            config.conn.build_psql_args(&config.conn.maintenance_db),
            create_sql
        );

        let (success, _, stderr) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to create database '{}': {}",
                config.name, stderr
            )))
        }
    }

    /// Update database properties
    async fn update_database(
        connection: &dyn Connection,
        config: &DbConfig,
        current: &HashMap<String, String>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let mut changed = false;

        // Check owner
        if let Some(ref desired_owner) = config.owner {
            if current.get("owner").map(|s| s.as_str()) != Some(desired_owner.as_str()) {
                let sql = format!(
                    "ALTER DATABASE {} OWNER TO {}",
                    shell_escape(&config.name),
                    shell_escape(desired_owner)
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.conn.maintenance_db),
                    sql
                );
                let (success, _, stderr) =
                    Self::execute_command(connection, &cmd, context, config.conn.build_env_vars())
                        .await?;
                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to change database owner: {}",
                        stderr
                    )));
                }
                changed = true;
            }
        }

        // Check connection limit
        if let Some(desired_limit) = config.conn_limit {
            let current_limit: i32 = current
                .get("conn_limit")
                .and_then(|s| s.parse().ok())
                .unwrap_or(-1);
            if current_limit != desired_limit {
                let sql = format!(
                    "ALTER DATABASE {} CONNECTION LIMIT {}",
                    shell_escape(&config.name),
                    desired_limit
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.conn.maintenance_db),
                    sql
                );
                let (success, _, stderr) =
                    Self::execute_command(connection, &cmd, context, config.conn.build_env_vars())
                        .await?;
                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to change connection limit: {}",
                        stderr
                    )));
                }
                changed = true;
            }
        }

        // Check tablespace
        if let Some(ref desired_tablespace) = config.tablespace {
            if current.get("tablespace").map(|s| s.as_str()) != Some(desired_tablespace.as_str()) {
                let sql = format!(
                    "ALTER DATABASE {} SET TABLESPACE {}",
                    shell_escape(&config.name),
                    shell_escape(desired_tablespace)
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.conn.maintenance_db),
                    sql
                );
                let (success, _, stderr) =
                    Self::execute_command(connection, &cmd, context, config.conn.build_env_vars())
                        .await?;
                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to change tablespace: {}",
                        stderr
                    )));
                }
                changed = true;
            }
        }

        Ok(changed)
    }

    /// Drop a database
    async fn drop_database(
        connection: &dyn Connection,
        config: &DbConfig,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // If force is set, terminate existing connections first
        if config.force {
            let terminate_sql = format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}' AND pid <> pg_backend_pid()",
                config.name.replace('\'', "''")
            );
            let cmd = format!(
                "psql {} -c \"{}\"",
                config.conn.build_psql_args(&config.conn.maintenance_db),
                terminate_sql
            );
            // Ignore errors from terminate - connections might not exist
            let _ = Self::execute_command(connection, &cmd, context, config.conn.build_env_vars())
                .await;
        }

        let drop_sql = format!("DROP DATABASE IF EXISTS {}", shell_escape(&config.name));
        let cmd = format!(
            "psql {} -c \"{}\"",
            config.conn.build_psql_args(&config.conn.maintenance_db),
            drop_sql
        );

        let (success, _, stderr) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to drop database '{}': {}",
                config.name, stderr
            )))
        }
    }

    /// Dump database using pg_dump
    async fn dump_database(
        connection: &dyn Connection,
        config: &DbConfig,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let target = config.target.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("target is required for dump operation".to_string())
        })?;

        let mut cmd_parts = vec!["pg_dump".to_string()];

        // Connection options
        if config.conn.unix_socket.is_some() {
            cmd_parts.push(format!(
                "-h {}",
                shell_escape(config.conn.unix_socket.as_ref().unwrap())
            ));
        } else {
            cmd_parts.push(format!("-h {}", shell_escape(&config.conn.host)));
        }
        cmd_parts.push(format!("-p {}", config.conn.port));
        cmd_parts.push(format!("-U {}", shell_escape(&config.conn.user)));

        // Output format based on file extension or explicit options
        if target.ends_with(".sql") {
            cmd_parts.push("-Fp".to_string()); // Plain SQL format
        } else if target.ends_with(".dump") || target.ends_with(".backup") {
            cmd_parts.push("-Fc".to_string()); // Custom format (compressed)
        } else if target.ends_with(".tar") {
            cmd_parts.push("-Ft".to_string()); // Tar format
        } else if target.ends_with('/') || std::path::Path::new(target).is_dir() {
            cmd_parts.push("-Fd".to_string()); // Directory format
        } else {
            cmd_parts.push("-Fc".to_string()); // Default to custom format
        }

        // Extra arguments
        if let Some(ref extra_args) = config.dump_extra_args {
            cmd_parts.push(extra_args.clone());
        }

        // Target options
        if let Some(ref opts) = config.target_opts {
            cmd_parts.push(opts.clone());
        }

        cmd_parts.push(format!("-f {}", shell_escape(target)));
        cmd_parts.push(shell_escape(&config.name).into_owned());

        let cmd = cmd_parts.join(" ");
        let (success, _, stderr) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if success {
            Ok(target.clone())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to dump database '{}': {}",
                config.name, stderr
            )))
        }
    }

    /// Restore database using pg_restore or psql
    async fn restore_database(
        connection: &dyn Connection,
        config: &DbConfig,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let target = config.target.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("target is required for restore operation".to_string())
        })?;

        // Determine restore method based on file type
        let is_plain_sql = target.ends_with(".sql");

        let cmd = if is_plain_sql {
            // Use psql for plain SQL files
            format!(
                "psql {} -f {}",
                config.conn.build_psql_args(&config.name),
                shell_escape(target)
            )
        } else {
            // Use pg_restore for custom/tar/directory formats
            let mut cmd_parts = vec!["pg_restore".to_string()];

            // Connection options
            if config.conn.unix_socket.is_some() {
                cmd_parts.push(format!(
                    "-h {}",
                    shell_escape(config.conn.unix_socket.as_ref().unwrap())
                ));
            } else {
                cmd_parts.push(format!("-h {}", shell_escape(&config.conn.host)));
            }
            cmd_parts.push(format!("-p {}", config.conn.port));
            cmd_parts.push(format!("-U {}", shell_escape(&config.conn.user)));
            cmd_parts.push(format!("-d {}", shell_escape(&config.name)));

            // Target options
            if let Some(ref opts) = config.target_opts {
                cmd_parts.push(opts.clone());
            }

            cmd_parts.push(shell_escape(target).into_owned());

            cmd_parts.join(" ")
        };

        let (success, _, stderr) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to restore database '{}': {}",
                config.name, stderr
            )))
        }
    }

    /// Execute the module with async connection
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let config = DbConfig::from_params(params)?;

        // In check mode, if diff mode is requested, we can calculate the diff
        let diff = if context.check_mode && context.diff_mode {
            let exists = Self::database_exists(connection.as_ref(), &config, context).await?;
            let before = if exists {
                let info = Self::get_database_info(connection.as_ref(), &config, context).await?;
                if let Some(info) = info {
                    format!(
                        "database: {}\nowner: {}\nencoding: {}\nstate: present",
                        info.get("name").unwrap_or(&String::new()),
                        info.get("owner").unwrap_or(&String::new()),
                        info.get("encoding").unwrap_or(&String::new())
                    )
                } else {
                    format!("database: {}\nstate: present", config.name)
                }
            } else {
                format!("database: {}\nstate: absent", config.name)
            };

            let after = match config.state {
                DbState::Present => format!(
                    "database: {}\nowner: {}\nencoding: {}\nstate: present",
                    config.name,
                    config.owner.as_deref().unwrap_or("(default)"),
                    config.encoding
                ),
                DbState::Absent => format!("database: {}\nstate: absent", config.name),
                DbState::Dump => format!(
                    "database: {}\nstate: dump\ntarget: {}",
                    config.name,
                    config.target.as_deref().unwrap_or("(unspecified)")
                ),
                DbState::Restore => format!(
                    "database: {}\nstate: restore\nsource: {}",
                    config.name,
                    config.target.as_deref().unwrap_or("(unspecified)")
                ),
            };

            if before == after {
                None
            } else {
                Some(Diff::new(before, after))
            }
        } else {
            None
        };

        match config.state {
            DbState::Present => {
                let exists = Self::database_exists(connection.as_ref(), &config, context).await?;

                if !exists {
                    if context.check_mode {
                        let mut output = ModuleOutput::changed(format!(
                            "Would create database '{}'",
                            config.name
                        ));
                        if let Some(d) = diff {
                            output = output.with_diff(d);
                        }
                        return Ok(output);
                    }

                    Self::create_database(connection.as_ref(), &config, context).await?;
                    Ok(
                        ModuleOutput::changed(format!("Created database '{}'", config.name))
                            .with_data("name", serde_json::json!(config.name))
                            .with_data("owner", serde_json::json!(config.owner))
                            .with_data("encoding", serde_json::json!(config.encoding)),
                    )
                } else {
                    // Database exists, check if updates needed
                    let current =
                        Self::get_database_info(connection.as_ref(), &config, context).await?;

                    if let Some(current_info) = current {
                        if context.check_mode {
                            // In check mode we need to know if it WOULD change
                            // Re-using the diff logic logic or update logic to determine change status
                            // For simplicity, we assume diff implies change if we have one, or check fields

                            // Check for differences using the update logic (dry run essentially)
                            // But update_database actually executes commands.
                            // We should inspect the fields manually or trust the diff.
                            // Let's check fields to be precise about 'changed' status.

                            let mut needs_change = false;
                            if let Some(ref desired_owner) = config.owner {
                                if current_info.get("owner").map(|s| s.as_str())
                                    != Some(desired_owner.as_str())
                                {
                                    needs_change = true;
                                }
                            }
                            if let Some(desired_limit) = config.conn_limit {
                                let current_limit: i32 = current_info
                                    .get("conn_limit")
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(-1);
                                if current_limit != desired_limit {
                                    needs_change = true;
                                }
                            }
                            if let Some(ref desired_tablespace) = config.tablespace {
                                if current_info.get("tablespace").map(|s| s.as_str())
                                    != Some(desired_tablespace.as_str())
                                {
                                    needs_change = true;
                                }
                            }

                            if needs_change {
                                let mut output = ModuleOutput::changed(format!(
                                    "Would update database '{}'",
                                    config.name
                                ));
                                if let Some(d) = diff {
                                    output = output.with_diff(d);
                                }
                                return Ok(output);
                            } else {
                                return Ok(ModuleOutput::ok(format!(
                                    "Database '{}' exists",
                                    config.name
                                )));
                            }
                        }

                        let changed = Self::update_database(
                            connection.as_ref(),
                            &config,
                            &current_info,
                            context,
                        )
                        .await?;

                        if changed {
                            Ok(
                                ModuleOutput::changed(format!(
                                    "Updated database '{}'",
                                    config.name
                                ))
                                .with_data("name", serde_json::json!(config.name)),
                            )
                        } else {
                            Ok(ModuleOutput::ok(format!(
                                "Database '{}' is in desired state",
                                config.name
                            ))
                            .with_data("name", serde_json::json!(config.name)))
                        }
                    } else {
                        Ok(ModuleOutput::ok(format!(
                            "Database '{}' exists",
                            config.name
                        )))
                    }
                }
            }

            DbState::Absent => {
                let exists = Self::database_exists(connection.as_ref(), &config, context).await?;

                if exists {
                    if context.check_mode {
                        let mut output =
                            ModuleOutput::changed(format!("Would drop database '{}'", config.name));
                        if let Some(d) = diff {
                            output = output.with_diff(d);
                        }
                        return Ok(output);
                    }

                    Self::drop_database(connection.as_ref(), &config, context).await?;
                    Ok(ModuleOutput::changed(format!(
                        "Dropped database '{}'",
                        config.name
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Database '{}' already absent",
                        config.name
                    )))
                }
            }

            DbState::Dump => {
                let exists = Self::database_exists(connection.as_ref(), &config, context).await?;

                if !exists {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Database '{}' does not exist, cannot dump",
                        config.name
                    )));
                }

                if context.check_mode {
                    let mut output = ModuleOutput::changed(format!(
                        "Would dump database '{}' to '{}'",
                        config.name,
                        config.target.as_deref().unwrap_or("(unspecified)")
                    ));
                    if let Some(d) = diff {
                        output = output.with_diff(d);
                    }
                    return Ok(output);
                }

                let target = Self::dump_database(connection.as_ref(), &config, context).await?;
                Ok(ModuleOutput::changed(format!(
                    "Dumped database '{}' to '{}'",
                    config.name, target
                ))
                .with_data("name", serde_json::json!(config.name))
                .with_data("target", serde_json::json!(target)))
            }

            DbState::Restore => {
                // Ensure database exists before restore
                let exists = Self::database_exists(connection.as_ref(), &config, context).await?;

                if !exists {
                    // Create the database first
                    if context.check_mode {
                        let mut output = ModuleOutput::changed(format!(
                            "Would create database '{}' and restore from '{}'",
                            config.name,
                            config.target.as_deref().unwrap_or("(unspecified)")
                        ));
                        if let Some(d) = diff {
                            output = output.with_diff(d);
                        }
                        return Ok(output);
                    }

                    Self::create_database(connection.as_ref(), &config, context).await?;
                }

                if context.check_mode {
                    let mut output = ModuleOutput::changed(format!(
                        "Would restore database '{}' from '{}'",
                        config.name,
                        config.target.as_deref().unwrap_or("(unspecified)")
                    ));
                    if let Some(d) = diff {
                        output = output.with_diff(d);
                    }
                    return Ok(output);
                }

                Self::restore_database(connection.as_ref(), &config, context).await?;
                Ok(ModuleOutput::changed(format!(
                    "Restored database '{}' from '{}'",
                    config.name,
                    config.target.as_deref().unwrap_or("")
                ))
                .with_data("name", serde_json::json!(config.name))
                .with_data("target", serde_json::json!(config.target)))
            }
        }
    }
}

impl Module for PostgresqlDbModule {
    fn name(&self) -> &'static str {
        "postgresql_db"
    }

    fn description(&self) -> &'static str {
        "Manage PostgreSQL databases including backup and restore"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Database operations should be serialized per host to avoid conflicts
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.clone().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available for postgresql_db module execution".to_string(),
            )
        })?;

        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;
        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context, connection)))
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_state_from_str() {
        assert_eq!(DbState::from_str("present").unwrap(), DbState::Present);
        assert_eq!(DbState::from_str("absent").unwrap(), DbState::Absent);
        assert_eq!(DbState::from_str("dump").unwrap(), DbState::Dump);
        assert_eq!(DbState::from_str("restore").unwrap(), DbState::Restore);
        assert!(DbState::from_str("invalid").is_err());
    }

    #[test]
    fn test_ssl_mode_from_str() {
        assert_eq!(SslMode::from_str("disable").unwrap(), SslMode::Disable);
        assert_eq!(SslMode::from_str("require").unwrap(), SslMode::Require);
        assert_eq!(SslMode::from_str("verify-ca").unwrap(), SslMode::VerifyCa);
        assert_eq!(
            SslMode::from_str("verify_full").unwrap(),
            SslMode::VerifyFull
        );
        assert!(SslMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_pg_connection_config_default() {
        let config = PgConnectionConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 5432);
        assert_eq!(config.user, "postgres");
        assert_eq!(config.maintenance_db, "postgres");
    }

    #[test]
    fn test_pg_connection_config_env_vars() {
        let mut config = PgConnectionConfig::default();
        config.password = Some("secret".to_string());
        config.ssl_mode = SslMode::Require;

        let env = config.build_env_vars();
        assert_eq!(env.get("PGHOST").unwrap(), "localhost");
        assert_eq!(env.get("PGPORT").unwrap(), "5432");
        assert_eq!(env.get("PGUSER").unwrap(), "postgres");
        assert_eq!(env.get("PGPASSWORD").unwrap(), "secret");
        assert_eq!(env.get("PGSSLMODE").unwrap(), "require");
    }

    #[test]
    fn test_module_metadata() {
        let module = PostgresqlDbModule;
        assert_eq!(module.name(), "postgresql_db");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
    }
}
