//! PostgreSQL User module - User/Role management
//!
//! This module manages PostgreSQL users (roles) including creation, deletion,
//! and privilege management.
//!
//! ## Parameters
//!
//! - `name`: User/role name (required)
//! - `state`: Desired state (present, absent) - default: present
//! - `password`: User password (plain text or md5 hash)
//! - `encrypted`: Whether password is already encrypted (default: false)
//! - `expires`: Account expiration timestamp (YYYY-MM-DD HH:MM:SS or infinity)
//! - `conn_limit`: Connection limit (-1 for unlimited)
//! - `role_attr_flags`: Role attributes (SUPERUSER, CREATEDB, CREATEROLE, LOGIN, etc.)
//! - `groups`: List of groups/roles to grant membership in
//! - `priv`: Privileges on database objects (format: db/table:privs)
//! - `db`: Database for privilege operations
//! - `fail_on_user`: Fail if user already exists (default: false)
//! - `no_password_changes`: Skip password changes if user exists (default: false)
//! - `login_host`: PostgreSQL server host (default: localhost)
//! - `login_port`: PostgreSQL server port (default: 5432)
//! - `login_user`: PostgreSQL login user (default: postgres)
//! - `login_password`: PostgreSQL login password
//! - `login_unix_socket`: Unix socket path for local connections
//! - `ssl_mode`: SSL mode (disable, allow, prefer, require, verify-ca, verify-full)

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::utils::shell_escape;
use std::collections::HashMap;
use std::sync::Arc;

use super::postgresql_db::{PgConnectionConfig, SslMode};

/// Desired state for a user
#[derive(Debug, Clone, PartialEq)]
pub enum UserState {
    Present,
    Absent,
}

impl UserState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(UserState::Present),
            "absent" => Ok(UserState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Role attribute flags
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RoleAttrFlags {
    pub superuser: Option<bool>,
    pub createdb: Option<bool>,
    pub createrole: Option<bool>,
    pub login: Option<bool>,
    pub replication: Option<bool>,
    pub bypassrls: Option<bool>,
    pub inherit: Option<bool>,
}

impl RoleAttrFlags {
    fn from_str(s: &str) -> ModuleResult<Self> {
        let mut flags = RoleAttrFlags::default();

        for part in s.split(',') {
            let part = part.trim().to_uppercase();
            match part.as_str() {
                "SUPERUSER" => flags.superuser = Some(true),
                "NOSUPERUSER" => flags.superuser = Some(false),
                "CREATEDB" => flags.createdb = Some(true),
                "NOCREATEDB" => flags.createdb = Some(false),
                "CREATEROLE" => flags.createrole = Some(true),
                "NOCREATEROLE" => flags.createrole = Some(false),
                "LOGIN" => flags.login = Some(true),
                "NOLOGIN" => flags.login = Some(false),
                "REPLICATION" => flags.replication = Some(true),
                "NOREPLICATION" => flags.replication = Some(false),
                "BYPASSRLS" => flags.bypassrls = Some(true),
                "NOBYPASSRLS" => flags.bypassrls = Some(false),
                "INHERIT" => flags.inherit = Some(true),
                "NOINHERIT" => flags.inherit = Some(false),
                "" => {}
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Unknown role attribute flag: '{}'",
                        part
                    )))
                }
            }
        }

        Ok(flags)
    }

    fn to_sql_options(&self) -> Vec<String> {
        let mut options = Vec::new();

        if let Some(v) = self.superuser {
            options.push(if v { "SUPERUSER" } else { "NOSUPERUSER" }.to_string());
        }
        if let Some(v) = self.createdb {
            options.push(if v { "CREATEDB" } else { "NOCREATEDB" }.to_string());
        }
        if let Some(v) = self.createrole {
            options.push(if v { "CREATEROLE" } else { "NOCREATEROLE" }.to_string());
        }
        if let Some(v) = self.login {
            options.push(if v { "LOGIN" } else { "NOLOGIN" }.to_string());
        }
        if let Some(v) = self.replication {
            options.push(if v { "REPLICATION" } else { "NOREPLICATION" }.to_string());
        }
        if let Some(v) = self.bypassrls {
            options.push(if v { "BYPASSRLS" } else { "NOBYPASSRLS" }.to_string());
        }
        if let Some(v) = self.inherit {
            options.push(if v { "INHERIT" } else { "NOINHERIT" }.to_string());
        }

        options
    }
}

/// User configuration parsed from parameters
#[derive(Debug, Clone)]
struct UserConfig {
    name: String,
    state: UserState,
    password: Option<String>,
    encrypted: bool,
    expires: Option<String>,
    conn_limit: Option<i32>,
    role_attr_flags: RoleAttrFlags,
    groups: Vec<String>,
    db: Option<String>,
    fail_on_user: bool,
    no_password_changes: bool,
    conn: PgConnectionConfig,
}

impl UserConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let state = if let Some(s) = params.get_string("state")? {
            UserState::from_str(&s)?
        } else {
            UserState::Present
        };

        let role_attr_flags = if let Some(flags) = params.get_string("role_attr_flags")? {
            RoleAttrFlags::from_str(&flags)?
        } else {
            RoleAttrFlags::default()
        };

        let groups = params.get_vec_string("groups")?.unwrap_or_default();

        let conn_limit = params.get_i64("conn_limit")?.map(|limit| limit as i32);

        // Parse connection config
        let ssl_mode = if let Some(mode) = params.get_string("ssl_mode")? {
            SslMode::from_str(&mode)?
        } else {
            SslMode::Prefer
        };

        let conn = PgConnectionConfig {
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
                .get_string("db")?
                .unwrap_or_else(|| "postgres".to_string()),
        };

        Ok(Self {
            name: params.get_string_required("name")?,
            state,
            password: params.get_string("password")?,
            encrypted: params.get_bool_or("encrypted", false),
            expires: params.get_string("expires")?,
            conn_limit,
            role_attr_flags,
            groups,
            db: params.get_string("db")?,
            fail_on_user: params.get_bool_or("fail_on_user", false),
            no_password_changes: params.get_bool_or("no_password_changes", false),
            conn,
        })
    }
}

/// Current user/role information from database
#[derive(Debug, Clone)]
struct RoleInfo {
    name: String,
    superuser: bool,
    createdb: bool,
    createrole: bool,
    login: bool,
    replication: bool,
    bypassrls: bool,
    inherit: bool,
    conn_limit: i32,
    expires: Option<String>,
    member_of: Vec<String>,
}

/// Module for PostgreSQL user/role management
pub struct PostgresqlUserModule;

impl PostgresqlUserModule {
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
            if let Some(ref password) = context.become_password {
                options.escalate_password = Some(password.clone());
            }
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

    /// Check if role exists
    async fn role_exists(
        connection: &dyn Connection,
        config: &UserConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let query = format!(
            "SELECT 1 FROM pg_roles WHERE rolname = '{}'",
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

    /// Get role information
    async fn get_role_info(
        connection: &dyn Connection,
        config: &UserConfig,
        context: &ModuleContext,
    ) -> ModuleResult<Option<RoleInfo>> {
        let query = format!(
            "SELECT r.rolname, r.rolsuper, r.rolcreatedb, r.rolcreaterole, r.rolcanlogin, \
             r.rolreplication, r.rolbypassrls, r.rolinherit, r.rolconnlimit, \
             r.rolvaliduntil::text \
             FROM pg_roles r WHERE r.rolname = '{}'",
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
        if parts.len() < 10 {
            return Ok(None);
        }

        // Get group memberships
        let groups_query = format!(
            "SELECT r.rolname FROM pg_roles r \
             JOIN pg_auth_members m ON r.oid = m.roleid \
             JOIN pg_roles u ON u.oid = m.member \
             WHERE u.rolname = '{}'",
            config.name.replace('\'', "''")
        );
        let groups_cmd = format!(
            "psql {} -tAc \"{}\"",
            config.conn.build_psql_args(&config.conn.maintenance_db),
            groups_query
        );

        let (_, groups_stdout, _) = Self::execute_command(
            connection,
            &groups_cmd,
            context,
            config.conn.build_env_vars(),
        )
        .await?;

        let member_of: Vec<String> = groups_stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(Some(RoleInfo {
            name: parts[0].to_string(),
            superuser: parts[1] == "t",
            createdb: parts[2] == "t",
            createrole: parts[3] == "t",
            login: parts[4] == "t",
            replication: parts[5] == "t",
            bypassrls: parts[6] == "t",
            inherit: parts[7] == "t",
            conn_limit: parts[8].parse().unwrap_or(-1),
            expires: if parts[9].is_empty() || parts[9] == "infinity" {
                None
            } else {
                Some(parts[9].to_string())
            },
            member_of,
        }))
    }

    /// Create a role
    async fn create_role(
        connection: &dyn Connection,
        config: &UserConfig,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let mut options = config.role_attr_flags.to_sql_options();

        // Add password
        if let Some(ref password) = config.password {
            options.push(format!("PASSWORD '{}'", password.replace('\'', "''")));
        }

        // Add connection limit
        if let Some(limit) = config.conn_limit {
            options.push(format!("CONNECTION LIMIT {}", limit));
        }

        // Add expiration
        if let Some(ref expires) = config.expires {
            options.push(format!("VALID UNTIL '{}'", expires.replace('\'', "''")));
        }

        let create_sql = format!(
            "CREATE ROLE {} {}",
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

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to create role '{}': {}",
                config.name, stderr
            )));
        }

        // Grant group memberships
        for group in &config.groups {
            Self::grant_role(connection, config, group, context).await?;
        }

        Ok(())
    }

    /// Update role attributes
    async fn update_role(
        connection: &dyn Connection,
        config: &UserConfig,
        current: &RoleInfo,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let mut changed = false;
        let mut alterations = Vec::new();

        // Check role attributes
        if let Some(v) = config.role_attr_flags.superuser {
            if current.superuser != v {
                alterations.push(if v { "SUPERUSER" } else { "NOSUPERUSER" }.to_string());
            }
        }
        if let Some(v) = config.role_attr_flags.createdb {
            if current.createdb != v {
                alterations.push(if v { "CREATEDB" } else { "NOCREATEDB" }.to_string());
            }
        }
        if let Some(v) = config.role_attr_flags.createrole {
            if current.createrole != v {
                alterations.push(if v { "CREATEROLE" } else { "NOCREATEROLE" }.to_string());
            }
        }
        if let Some(v) = config.role_attr_flags.login {
            if current.login != v {
                alterations.push(if v { "LOGIN" } else { "NOLOGIN" }.to_string());
            }
        }
        if let Some(v) = config.role_attr_flags.replication {
            if current.replication != v {
                alterations.push(if v { "REPLICATION" } else { "NOREPLICATION" }.to_string());
            }
        }
        if let Some(v) = config.role_attr_flags.bypassrls {
            if current.bypassrls != v {
                alterations.push(if v { "BYPASSRLS" } else { "NOBYPASSRLS" }.to_string());
            }
        }
        if let Some(v) = config.role_attr_flags.inherit {
            if current.inherit != v {
                alterations.push(if v { "INHERIT" } else { "NOINHERIT" }.to_string());
            }
        }

        // Check connection limit
        if let Some(limit) = config.conn_limit {
            if current.conn_limit != limit {
                alterations.push(format!("CONNECTION LIMIT {}", limit));
            }
        }

        // Check expiration
        if let Some(ref expires) = config.expires {
            let current_expires = current.expires.as_deref().unwrap_or("");
            if current_expires != expires {
                alterations.push(format!("VALID UNTIL '{}'", expires.replace('\'', "''")));
            }
        }

        // Apply attribute changes
        if !alterations.is_empty() {
            let alter_sql = format!(
                "ALTER ROLE {} {}",
                shell_escape(&config.name),
                alterations.join(" ")
            );
            let cmd = format!(
                "psql {} -c \"{}\"",
                config.conn.build_psql_args(&config.conn.maintenance_db),
                alter_sql
            );

            let (success, _, stderr) =
                Self::execute_command(connection, &cmd, context, config.conn.build_env_vars())
                    .await?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to alter role '{}': {}",
                    config.name, stderr
                )));
            }
            changed = true;
        }

        // Handle password changes
        if !config.no_password_changes {
            if let Some(ref password) = config.password {
                // Always set password since we can't easily compare encrypted passwords
                let pwd_sql = format!(
                    "ALTER ROLE {} PASSWORD '{}'",
                    shell_escape(&config.name),
                    password.replace('\'', "''")
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.conn.maintenance_db),
                    pwd_sql
                );

                let (success, _, stderr) =
                    Self::execute_command(connection, &cmd, context, config.conn.build_env_vars())
                        .await?;

                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to set password for role '{}': {}",
                        config.name, stderr
                    )));
                }
                changed = true;
            }
        }

        // Handle group memberships
        for group in &config.groups {
            if !current.member_of.contains(group) {
                Self::grant_role(connection, config, group, context).await?;
                changed = true;
            }
        }

        Ok(changed)
    }

    /// Grant a role membership
    async fn grant_role(
        connection: &dyn Connection,
        config: &UserConfig,
        group: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let grant_sql = format!(
            "GRANT {} TO {}",
            shell_escape(group),
            shell_escape(&config.name)
        );
        let cmd = format!(
            "psql {} -c \"{}\"",
            config.conn.build_psql_args(&config.conn.maintenance_db),
            grant_sql
        );

        let (success, _, stderr) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to grant role '{}' to '{}': {}",
                group, config.name, stderr
            )))
        }
    }

    /// Drop a role
    async fn drop_role(
        connection: &dyn Connection,
        config: &UserConfig,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let drop_sql = format!("DROP ROLE IF EXISTS {}", shell_escape(&config.name));
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
                "Failed to drop role '{}': {}",
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
        let config = UserConfig::from_params(params)?;

        match config.state {
            UserState::Present => {
                let exists = Self::role_exists(connection.as_ref(), &config, context).await?;

                if exists {
                    if config.fail_on_user {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Role '{}' already exists and fail_on_user is set",
                            config.name
                        )));
                    }

                    // Update existing role
                    let current = Self::get_role_info(connection.as_ref(), &config, context)
                        .await?
                        .ok_or_else(|| {
                            ModuleError::ExecutionFailed(format!(
                                "Role '{}' exists but could not get info",
                                config.name
                            ))
                        })?;

                    if context.check_mode {
                        return Ok(ModuleOutput::ok(format!("Role '{}' exists", config.name)));
                    }

                    let changed =
                        Self::update_role(connection.as_ref(), &config, &current, context).await?;

                    if changed {
                        Ok(
                            ModuleOutput::changed(format!("Updated role '{}'", config.name))
                                .with_data("name", serde_json::json!(config.name)),
                        )
                    } else {
                        Ok(
                            ModuleOutput::ok(format!("Role '{}' is in desired state", config.name))
                                .with_data("name", serde_json::json!(config.name)),
                        )
                    }
                } else {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create role '{}'",
                            config.name
                        )));
                    }

                    Self::create_role(connection.as_ref(), &config, context).await?;
                    Ok(
                        ModuleOutput::changed(format!("Created role '{}'", config.name))
                            .with_data("name", serde_json::json!(config.name)),
                    )
                }
            }

            UserState::Absent => {
                let exists = Self::role_exists(connection.as_ref(), &config, context).await?;

                if exists {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would drop role '{}'",
                            config.name
                        )));
                    }

                    Self::drop_role(connection.as_ref(), &config, context).await?;
                    Ok(ModuleOutput::changed(format!(
                        "Dropped role '{}'",
                        config.name
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Role '{}' already absent",
                        config.name
                    )))
                }
            }
        }
    }
}

impl Module for PostgresqlUserModule {
    fn name(&self) -> &'static str {
        "postgresql_user"
    }

    fn description(&self) -> &'static str {
        "Manage PostgreSQL users/roles"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
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
                "No connection available for postgresql_user module execution".to_string(),
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
    fn test_user_state_from_str() {
        assert_eq!(UserState::from_str("present").unwrap(), UserState::Present);
        assert_eq!(UserState::from_str("absent").unwrap(), UserState::Absent);
        assert!(UserState::from_str("invalid").is_err());
    }

    #[test]
    fn test_role_attr_flags_from_str() {
        let flags = RoleAttrFlags::from_str("SUPERUSER,CREATEDB,LOGIN").unwrap();
        assert_eq!(flags.superuser, Some(true));
        assert_eq!(flags.createdb, Some(true));
        assert_eq!(flags.login, Some(true));
        assert_eq!(flags.createrole, None);
    }

    #[test]
    fn test_role_attr_flags_negative() {
        let flags = RoleAttrFlags::from_str("NOSUPERUSER,NOCREATEDB").unwrap();
        assert_eq!(flags.superuser, Some(false));
        assert_eq!(flags.createdb, Some(false));
    }

    #[test]
    fn test_role_attr_flags_to_sql() {
        let mut flags = RoleAttrFlags::default();
        flags.superuser = Some(true);
        flags.login = Some(true);

        let sql = flags.to_sql_options();
        assert!(sql.contains(&"SUPERUSER".to_string()));
        assert!(sql.contains(&"LOGIN".to_string()));
    }

    #[test]
    fn test_module_metadata() {
        let module = PostgresqlUserModule;
        assert_eq!(module.name(), "postgresql_user");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }
}
