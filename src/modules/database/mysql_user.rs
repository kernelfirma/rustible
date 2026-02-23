//! MySQL user management module
//!
//! This module provides functionality for creating, modifying, and dropping MySQL users,
//! as well as managing their privileges.
//!
//! # Parameters
//!
//! - `name` (required): Name of the user to manage
//! - `host`: Host from which the user can connect (default: localhost)
//! - `password`: Password for the user (plaintext or hashed)
//! - `encrypted`: Whether the password is already hashed (default: false)
//! - `state`: `present` (default) or `absent`
//! - `priv`: Privileges to grant (format: "db.table:PRIV1,PRIV2" or "db.*:ALL")
//! - `append_privs`: Append privileges instead of replacing (default: false)
//! - `login_host`: MySQL server host (default: localhost)
//! - `login_port`: MySQL server port (default: 3306)
//! - `login_user`: MySQL username for authentication
//! - `login_password`: MySQL password for authentication
//!
//! # Privilege Format
//!
//! Privileges are specified as "db.table:PRIV1,PRIV2/db2.*:PRIV3"
//!
//! - `*.*:ALL` - All privileges on all databases
//! - `mydb.*:ALL` - All privileges on mydb
//! - `mydb.mytable:SELECT,INSERT` - SELECT and INSERT on specific table
//!
//! # Example
//!
//! ```yaml
//! # Create a user with all privileges on a database
//! - mysql_user:
//!     name: myapp_user
//!     host: "%"
//!     password: "{{ app_password }}"
//!     priv: "myapp_db.*:ALL"
//!     state: present
//!
//! # Create a read-only user
//! - mysql_user:
//!     name: readonly_user
//!     password: "{{ readonly_password }}"
//!     priv: "myapp_db.*:SELECT"
//!     state: present
//!
//! # Remove a user
//! - mysql_user:
//!     name: old_user
//!     state: absent
//! ```

use super::pool::global_pool_manager;
use super::{extract_connection_params, MysqlConnectionParams};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use sqlx::Row;
use tokio::runtime::Handle;

/// Desired state for a user
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Represents a privilege specification
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeSpec {
    pub database: String,
    pub table: String,
    pub privileges: Vec<String>,
}

impl PrivilegeSpec {
    /// Valid MySQL privilege names
    const VALID_PRIVILEGES: &[&str] = &[
        "ALL",
        "ALL PRIVILEGES",
        "ALTER",
        "ALTER ROUTINE",
        "CREATE",
        "CREATE ROUTINE",
        "CREATE TABLESPACE",
        "CREATE TEMPORARY TABLES",
        "CREATE USER",
        "CREATE VIEW",
        "DELETE",
        "DROP",
        "EVENT",
        "EXECUTE",
        "FILE",
        "GRANT OPTION",
        "INDEX",
        "INSERT",
        "LOCK TABLES",
        "PROCESS",
        "REFERENCES",
        "RELOAD",
        "REPLICATION CLIENT",
        "REPLICATION SLAVE",
        "SELECT",
        "SHOW DATABASES",
        "SHOW VIEW",
        "SHUTDOWN",
        "SUPER",
        "TRIGGER",
        "UPDATE",
        "USAGE",
    ];

    /// Validate a MySQL identifier (database or table name) to prevent backtick injection.
    /// Allows alphanumeric, underscore, dollar, hyphen, and wildcard (*).
    fn validate_identifier(name: &str, param_name: &str) -> ModuleResult<()> {
        if name == "*" {
            return Ok(());
        }
        for c in name.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '$' && c != '-' {
                return Err(ModuleError::InvalidParameter(format!(
                    "{} contains invalid character: '{}'. Only alphanumeric, underscore, dollar, and hyphen are allowed",
                    param_name, c
                )));
            }
        }
        Ok(())
    }

    /// Escape backticks within a MySQL identifier (double them)
    fn escape_backtick(name: &str) -> String {
        name.replace('`', "``")
    }

    /// Parse privilege string like "db.table:PRIV1,PRIV2"
    fn parse(spec: &str) -> ModuleResult<Self> {
        let parts: Vec<&str> = spec.split(':').collect();
        if parts.len() != 2 {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid privilege format: '{}'. Expected 'db.table:PRIV1,PRIV2'",
                spec
            )));
        }

        let db_table: Vec<&str> = parts[0].split('.').collect();
        if db_table.len() != 2 {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid database.table format: '{}'. Expected 'db.table'",
                parts[0]
            )));
        }

        // Validate database and table identifiers
        Self::validate_identifier(db_table[0], "database name")?;
        Self::validate_identifier(db_table[1], "table name")?;

        let privileges: Vec<String> = parts[1]
            .split(',')
            .map(|p| p.trim().to_uppercase())
            .filter(|p| !p.is_empty())
            .collect();

        if privileges.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "At least one privilege must be specified".to_string(),
            ));
        }

        // Validate privilege names against allowlist
        for priv_name in &privileges {
            if !Self::VALID_PRIVILEGES.contains(&priv_name.as_str()) {
                return Err(ModuleError::InvalidParameter(format!(
                    "Unknown privilege: '{}'. Valid privileges include: SELECT, INSERT, UPDATE, DELETE, ALL, etc.",
                    priv_name
                )));
            }
        }

        Ok(PrivilegeSpec {
            database: db_table[0].to_string(),
            table: db_table[1].to_string(),
            privileges,
        })
    }

    /// Parse multiple privilege specs separated by /
    fn parse_all(specs: &str) -> ModuleResult<Vec<Self>> {
        if specs.trim().is_empty() {
            return Ok(Vec::new());
        }

        specs
            .split('/')
            .filter(|s| !s.trim().is_empty())
            .map(|s| Self::parse(s.trim()))
            .collect()
    }

    /// Convert to GRANT statement format
    fn to_grant_on(&self) -> String {
        let db = if self.database == "*" {
            "*".to_string()
        } else {
            format!("`{}`", Self::escape_backtick(&self.database))
        };

        let table = if self.table == "*" {
            "*".to_string()
        } else {
            format!("`{}`", Self::escape_backtick(&self.table))
        };

        format!("{}.{}", db, table)
    }
}

/// Information about a MySQL user
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub name: String,
    pub host: String,
    pub privileges: Vec<PrivilegeSpec>,
}

/// Module for MySQL user management
pub struct MysqlUserModule;

impl MysqlUserModule {
    /// Validate username
    fn validate_username(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Username cannot be empty".to_string(),
            ));
        }

        // MySQL usernames are limited to 32 characters (80 in MySQL 8.0+)
        if name.len() > 80 {
            return Err(ModuleError::InvalidParameter(
                "Username cannot exceed 80 characters".to_string(),
            ));
        }

        // Validate characters
        for c in name.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '.' {
                return Err(ModuleError::InvalidParameter(format!(
                    "Username contains invalid character: '{}'",
                    c
                )));
            }
        }

        Ok(())
    }

    /// Validate host pattern
    fn validate_host(host: &str) -> ModuleResult<()> {
        if host.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Host cannot be empty".to_string(),
            ));
        }

        if host.len() > 255 {
            return Err(ModuleError::InvalidParameter(
                "Host cannot exceed 255 characters".to_string(),
            ));
        }

        // Allow common patterns: %, localhost, IP addresses, hostnames
        for c in host.chars() {
            if !c.is_ascii_alphanumeric()
                && c != '_'
                && c != '-'
                && c != '.'
                && c != '%'
                && c != ':'
            {
                return Err(ModuleError::InvalidParameter(format!(
                    "Host contains invalid character: '{}'",
                    c
                )));
            }
        }

        Ok(())
    }

    /// Execute async operations
    fn execute_async<F, T>(f: F) -> ModuleResult<T>
    where
        F: std::future::Future<Output = ModuleResult<T>> + Send,
        T: Send,
    {
        if let Ok(handle) = Handle::try_current() {
            std::thread::scope(|s| {
                s.spawn(|| handle.block_on(f))
                    .join()
                    .expect("Thread panicked")
            })
        } else {
            let rt = tokio::runtime::Runtime::new().map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to create runtime: {}", e))
            })?;
            rt.block_on(f)
        }
    }

    /// Check if user exists
    async fn user_exists(
        conn_params: &MysqlConnectionParams,
        username: &str,
        host: &str,
    ) -> ModuleResult<bool> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = format!(
            "SELECT User FROM mysql.user WHERE User = '{}' AND Host = '{}'",
            username.replace('\'', "''"),
            host.replace('\'', "''")
        );

        let result = pool
            .fetch_optional(&query)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        Ok(result.is_some())
    }

    /// Create a user
    async fn create_user(
        conn_params: &MysqlConnectionParams,
        username: &str,
        host: &str,
        password: Option<&str>,
        encrypted: bool,
    ) -> ModuleResult<()> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = if let Some(pwd) = password {
            if encrypted {
                format!(
                    "CREATE USER '{}'@'{}' IDENTIFIED WITH mysql_native_password AS '{}'",
                    username.replace('\'', "''"),
                    host.replace('\'', "''"),
                    pwd.replace('\'', "''")
                )
            } else {
                format!(
                    "CREATE USER '{}'@'{}' IDENTIFIED BY '{}'",
                    username.replace('\'', "''"),
                    host.replace('\'', "''"),
                    pwd.replace('\'', "''")
                )
            }
        } else {
            format!(
                "CREATE USER '{}'@'{}'",
                username.replace('\'', "''"),
                host.replace('\'', "''")
            )
        };

        pool.execute(&query)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to create user: {}", e)))?;

        Ok(())
    }

    /// Update user password
    async fn update_password(
        conn_params: &MysqlConnectionParams,
        username: &str,
        host: &str,
        password: &str,
        encrypted: bool,
    ) -> ModuleResult<()> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = if encrypted {
            format!(
                "ALTER USER '{}'@'{}' IDENTIFIED WITH mysql_native_password AS '{}'",
                username.replace('\'', "''"),
                host.replace('\'', "''"),
                password.replace('\'', "''")
            )
        } else {
            format!(
                "ALTER USER '{}'@'{}' IDENTIFIED BY '{}'",
                username.replace('\'', "''"),
                host.replace('\'', "''"),
                password.replace('\'', "''")
            )
        };

        pool.execute(&query).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to update password: {}", e))
        })?;

        Ok(())
    }

    /// Drop a user
    async fn drop_user(
        conn_params: &MysqlConnectionParams,
        username: &str,
        host: &str,
    ) -> ModuleResult<()> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = format!(
            "DROP USER '{}'@'{}'",
            username.replace('\'', "''"),
            host.replace('\'', "''")
        );

        pool.execute(&query)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to drop user: {}", e)))?;

        Ok(())
    }

    /// Get current privileges for a user
    async fn get_privileges(
        conn_params: &MysqlConnectionParams,
        username: &str,
        host: &str,
    ) -> ModuleResult<Vec<String>> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = format!(
            "SHOW GRANTS FOR '{}'@'{}'",
            username.replace('\'', "''"),
            host.replace('\'', "''")
        );

        let rows = pool
            .fetch_all(&query)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let grants: Vec<String> = rows
            .iter()
            .filter_map(|row| row.try_get::<String, _>(0).ok())
            .collect();

        Ok(grants)
    }

    /// Grant privileges to a user
    async fn grant_privileges(
        conn_params: &MysqlConnectionParams,
        username: &str,
        host: &str,
        privileges: &[PrivilegeSpec],
    ) -> ModuleResult<()> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        for priv_spec in privileges {
            let priv_str = priv_spec.privileges.join(", ");
            let on_clause = priv_spec.to_grant_on();

            let query = format!(
                "GRANT {} ON {} TO '{}'@'{}'",
                priv_str,
                on_clause,
                username.replace('\'', "''"),
                host.replace('\'', "''")
            );

            pool.execute(&query).await.map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to grant privileges: {}", e))
            })?;
        }

        // Flush privileges to ensure they take effect
        pool.execute("FLUSH PRIVILEGES").await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to flush privileges: {}", e))
        })?;

        Ok(())
    }

    /// Revoke all privileges from a user
    async fn revoke_all_privileges(
        conn_params: &MysqlConnectionParams,
        username: &str,
        host: &str,
    ) -> ModuleResult<()> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = format!(
            "REVOKE ALL PRIVILEGES, GRANT OPTION FROM '{}'@'{}'",
            username.replace('\'', "''"),
            host.replace('\'', "''")
        );

        pool.execute(&query).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to revoke privileges: {}", e))
        })?;

        pool.execute("FLUSH PRIVILEGES").await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to flush privileges: {}", e))
        })?;

        Ok(())
    }
}

impl Module for MysqlUserModule {
    fn name(&self) -> &'static str {
        "mysql_user"
    }

    fn description(&self) -> &'static str {
        "Manage MySQL users and their privileges"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // User modifications on same MySQL server can cause race conditions
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let name = params.get_string_required("name")?;
        Self::validate_username(&name)?;

        if let Some(host) = params.get_string("host")? {
            Self::validate_host(&host)?;
        }

        if let Some(state) = params.get_string("state")? {
            UserState::from_str(&state)?;
        }

        // Validate privilege format if provided
        if let Some(priv_str) = params.get_string("priv")? {
            PrivilegeSpec::parse_all(&priv_str)?;
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let username = params.get_string_required("name")?;
        let host = params
            .get_string("host")?
            .unwrap_or_else(|| "localhost".to_string());
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = UserState::from_str(&state_str)?;

        let password = params.get_string("password")?;
        let encrypted = params.get_bool_or("encrypted", false);
        let priv_str = params.get_string("priv")?;
        let append_privs = params.get_bool_or("append_privs", false);
        let update_password = params
            .get_string("update_password")?
            .unwrap_or_else(|| "always".to_string());

        let conn_params = extract_connection_params(params)?;

        Self::execute_async(async {
            let exists = Self::user_exists(&conn_params, &username, &host).await?;

            match state {
                UserState::Absent => {
                    if !exists {
                        return Ok(ModuleOutput::ok(format!(
                            "User '{}'@'{}' already absent",
                            username, host
                        )));
                    }

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would drop user '{}'@'{}'",
                            username, host
                        )));
                    }

                    Self::drop_user(&conn_params, &username, &host).await?;

                    Ok(ModuleOutput::changed(format!(
                        "Dropped user '{}'@'{}'",
                        username, host
                    )))
                }

                UserState::Present => {
                    let mut changed = false;
                    let mut messages = Vec::new();

                    if !exists {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would create user '{}'@'{}'",
                                username, host
                            )));
                        }

                        Self::create_user(
                            &conn_params,
                            &username,
                            &host,
                            password.as_deref(),
                            encrypted,
                        )
                        .await?;

                        changed = true;
                        messages.push(format!("Created user '{}'@'{}'", username, host));
                    } else if password.is_some() && update_password == "always" {
                        if context.check_mode {
                            messages.push("Would update password".to_string());
                            changed = true;
                        } else {
                            Self::update_password(
                                &conn_params,
                                &username,
                                &host,
                                password.as_ref().unwrap(),
                                encrypted,
                            )
                            .await?;
                            messages.push("Updated password".to_string());
                            changed = true;
                        }
                    }

                    // Handle privileges
                    if let Some(ref priv_str) = priv_str {
                        let privileges = PrivilegeSpec::parse_all(priv_str)?;

                        if context.check_mode {
                            if !privileges.is_empty() {
                                messages.push("Would update privileges".to_string());
                                changed = true;
                            }
                        } else {
                            if !append_privs && !privileges.is_empty() {
                                // Revoke existing privileges first
                                Self::revoke_all_privileges(&conn_params, &username, &host).await?;
                            }

                            if !privileges.is_empty() {
                                Self::grant_privileges(&conn_params, &username, &host, &privileges)
                                    .await?;
                                messages.push("Updated privileges".to_string());
                                changed = true;
                            }
                        }
                    }

                    // Get current privileges for output
                    let current_grants = if !context.check_mode {
                        Self::get_privileges(&conn_params, &username, &host).await?
                    } else {
                        Vec::new()
                    };

                    let msg = if messages.is_empty() {
                        format!("User '{}'@'{}' is in desired state", username, host)
                    } else {
                        messages.join(". ")
                    };

                    let mut output = if changed {
                        ModuleOutput::changed(msg)
                    } else {
                        ModuleOutput::ok(msg)
                    };

                    output = output
                        .with_data("user", serde_json::json!(username))
                        .with_data("host", serde_json::json!(host))
                        .with_data("grants", serde_json::json!(current_grants));

                    Ok(output)
                }
            }
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
    fn test_validate_username_valid() {
        assert!(MysqlUserModule::validate_username("myuser").is_ok());
        assert!(MysqlUserModule::validate_username("my_user").is_ok());
        assert!(MysqlUserModule::validate_username("user123").is_ok());
        assert!(MysqlUserModule::validate_username("my.user").is_ok());
    }

    #[test]
    fn test_validate_username_invalid() {
        assert!(MysqlUserModule::validate_username("").is_err());
        assert!(MysqlUserModule::validate_username("user name").is_err());
        assert!(MysqlUserModule::validate_username("user;drop").is_err());
    }

    #[test]
    fn test_validate_host_valid() {
        assert!(MysqlUserModule::validate_host("localhost").is_ok());
        assert!(MysqlUserModule::validate_host("%").is_ok());
        assert!(MysqlUserModule::validate_host("192.168.1.1").is_ok());
        assert!(MysqlUserModule::validate_host("%.example.com").is_ok());
    }

    #[test]
    fn test_privilege_spec_parse() {
        let spec = PrivilegeSpec::parse("mydb.mytable:SELECT,INSERT").unwrap();
        assert_eq!(spec.database, "mydb");
        assert_eq!(spec.table, "mytable");
        assert_eq!(spec.privileges, vec!["SELECT", "INSERT"]);

        let spec = PrivilegeSpec::parse("*.*:ALL").unwrap();
        assert_eq!(spec.database, "*");
        assert_eq!(spec.table, "*");
        assert_eq!(spec.privileges, vec!["ALL"]);
    }

    #[test]
    fn test_privilege_spec_parse_multiple() {
        let specs = PrivilegeSpec::parse_all("db1.*:SELECT/db2.*:ALL").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].database, "db1");
        assert_eq!(specs[1].database, "db2");
    }

    #[test]
    fn test_privilege_spec_invalid() {
        assert!(PrivilegeSpec::parse("invalid").is_err());
        assert!(PrivilegeSpec::parse("db:").is_err());
        assert!(PrivilegeSpec::parse(":PRIV").is_err());
    }

    #[test]
    fn test_user_state_from_str() {
        assert_eq!(UserState::from_str("present").unwrap(), UserState::Present);
        assert_eq!(UserState::from_str("absent").unwrap(), UserState::Absent);
        assert!(UserState::from_str("invalid").is_err());
    }

    #[test]
    fn test_module_name() {
        let module = MysqlUserModule;
        assert_eq!(module.name(), "mysql_user");
    }
}
