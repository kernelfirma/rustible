//! MySQL privilege management module
//!
//! This module provides granular control over MySQL user privileges,
//! separate from user management itself.
//!
//! # Parameters
//!
//! - `user` (required): MySQL username to manage privileges for
//! - `host`: Host from which the user can connect (default: localhost)
//! - `priv` (required): Privileges to manage (format: "db.table:PRIV1,PRIV2")
//! - `state`: `present` (default), `absent`, or `grant` (alias for present)
//! - `append_privs`: Append privileges instead of overwriting (default: false)
//! - `grant_option`: Add WITH GRANT OPTION (default: false)
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
//! - `mydb.mytable:SELECT,INSERT,UPDATE,DELETE` - Specific privileges on table
//! - `mydb.*:EXECUTE` - Execute stored procedures
//!
//! # Example
//!
//! ```yaml
//! # Grant all privileges on a database
//! - mysql_privs:
//!     user: myapp_user
//!     host: "%"
//!     priv: "myapp_db.*:ALL"
//!     state: present
//!
//! # Grant read-only access
//! - mysql_privs:
//!     user: readonly_user
//!     priv: "myapp_db.*:SELECT"
//!     state: present
//!
//! # Grant multiple privilege sets
//! - mysql_privs:
//!     user: app_user
//!     priv: "db1.*:SELECT,INSERT/db2.*:SELECT"
//!     state: present
//!
//! # Add privileges without revoking existing
//! - mysql_privs:
//!     user: developer
//!     priv: "staging_db.*:ALL"
//!     append_privs: true
//!
//! # Grant with GRANT OPTION
//! - mysql_privs:
//!     user: dba
//!     priv: "*.*:ALL"
//!     grant_option: true
//!
//! # Revoke privileges
//! - mysql_privs:
//!     user: old_user
//!     priv: "sensitive_db.*:ALL"
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

/// Desired state for privileges
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegeState {
    /// Grant the privileges
    Present,
    /// Revoke the privileges
    Absent,
}

impl PrivilegeState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "grant" => Ok(PrivilegeState::Present),
            "absent" | "revoke" => Ok(PrivilegeState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, grant, revoke",
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

        // Validate privilege names
        for priv_name in &privileges {
            Self::validate_privilege(priv_name)?;
        }

        Ok(PrivilegeSpec {
            database: db_table[0].to_string(),
            table: db_table[1].to_string(),
            privileges,
        })
    }

    /// Validate privilege name
    fn validate_privilege(priv_name: &str) -> ModuleResult<()> {
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

        if !VALID_PRIVILEGES.contains(&priv_name) {
            return Err(ModuleError::InvalidParameter(format!(
                "Unknown privilege: '{}'. Valid privileges include: SELECT, INSERT, UPDATE, DELETE, ALL, etc.",
                priv_name
            )));
        }

        Ok(())
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

/// Module for MySQL privilege management
pub struct MysqlPrivsModule;

impl MysqlPrivsModule {
    /// Validate username
    fn validate_username(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Username cannot be empty".to_string(),
            ));
        }

        if name.len() > 80 {
            return Err(ModuleError::InvalidParameter(
                "Username cannot exceed 80 characters".to_string(),
            ));
        }

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
                    .map_err(|_| ModuleError::ExecutionFailed("Thread panicked".into()))?
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
        grant_option: bool,
    ) -> ModuleResult<()> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        for priv_spec in privileges {
            let priv_str = priv_spec.privileges.join(", ");
            let on_clause = priv_spec.to_grant_on();

            let mut query = format!(
                "GRANT {} ON {} TO '{}'@'{}'",
                priv_str,
                on_clause,
                username.replace('\'', "''"),
                host.replace('\'', "''")
            );

            if grant_option {
                query.push_str(" WITH GRANT OPTION");
            }

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

    /// Revoke privileges from a user
    async fn revoke_privileges(
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
                "REVOKE {} ON {} FROM '{}'@'{}'",
                priv_str,
                on_clause,
                username.replace('\'', "''"),
                host.replace('\'', "''")
            );

            // Ignore errors for revoke - privilege might not exist
            let _ = pool.execute(&query).await;
        }

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

impl Module for MysqlPrivsModule {
    fn name(&self) -> &'static str {
        "mysql_privs"
    }

    fn description(&self) -> &'static str {
        "Manage MySQL user privileges"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        &["user", "priv"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let name = params.get_string_required("user")?;
        Self::validate_username(&name)?;

        if let Some(host) = params.get_string("host")? {
            Self::validate_host(&host)?;
        }

        if let Some(state) = params.get_string("state")? {
            PrivilegeState::from_str(&state)?;
        }

        // Validate privilege format
        let priv_str = params.get_string_required("priv")?;
        PrivilegeSpec::parse_all(&priv_str)?;

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let username = params.get_string_required("user")?;
        let host = params
            .get_string("host")?
            .unwrap_or_else(|| "localhost".to_string());
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = PrivilegeState::from_str(&state_str)?;

        let priv_str = params.get_string_required("priv")?;
        let privileges = PrivilegeSpec::parse_all(&priv_str)?;
        let append_privs = params.get_bool_or("append_privs", false);
        let grant_option = params.get_bool_or("grant_option", false);

        let conn_params = extract_connection_params(params)?;

        Self::execute_async(async {
            // Verify user exists
            let exists = Self::user_exists(&conn_params, &username, &host).await?;
            if !exists {
                return Err(ModuleError::ExecutionFailed(format!(
                    "User '{}'@'{}' does not exist",
                    username, host
                )));
            }

            // Get current privileges for comparison
            let current_grants = Self::get_privileges(&conn_params, &username, &host).await?;

            match state {
                PrivilegeState::Absent => {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would revoke privileges from '{}'@'{}'",
                            username, host
                        )));
                    }

                    Self::revoke_privileges(&conn_params, &username, &host, &privileges).await?;

                    let new_grants = Self::get_privileges(&conn_params, &username, &host).await?;

                    Ok(ModuleOutput::changed(format!(
                        "Revoked privileges from '{}'@'{}'",
                        username, host
                    ))
                    .with_data("grants", serde_json::json!(new_grants)))
                }

                PrivilegeState::Present => {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would grant privileges to '{}'@'{}'",
                            username, host
                        )));
                    }

                    // If not appending, revoke existing privileges first
                    if !append_privs {
                        Self::revoke_all_privileges(&conn_params, &username, &host).await?;
                    }

                    Self::grant_privileges(
                        &conn_params,
                        &username,
                        &host,
                        &privileges,
                        grant_option,
                    )
                    .await?;

                    let new_grants = Self::get_privileges(&conn_params, &username, &host).await?;

                    // Check if grants actually changed
                    let changed = new_grants != current_grants;

                    let msg = if changed {
                        format!("Updated privileges for '{}'@'{}'", username, host)
                    } else {
                        format!("Privileges for '{}'@'{}' unchanged", username, host)
                    };

                    let output = if changed {
                        ModuleOutput::changed(msg)
                    } else {
                        ModuleOutput::ok(msg)
                    };

                    Ok(output
                        .with_data("user", serde_json::json!(username))
                        .with_data("host", serde_json::json!(host))
                        .with_data("grants", serde_json::json!(new_grants)))
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
    fn test_privilege_state_from_str() {
        assert_eq!(
            PrivilegeState::from_str("present").unwrap(),
            PrivilegeState::Present
        );
        assert_eq!(
            PrivilegeState::from_str("grant").unwrap(),
            PrivilegeState::Present
        );
        assert_eq!(
            PrivilegeState::from_str("absent").unwrap(),
            PrivilegeState::Absent
        );
        assert_eq!(
            PrivilegeState::from_str("revoke").unwrap(),
            PrivilegeState::Absent
        );
        assert!(PrivilegeState::from_str("invalid").is_err());
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
        assert_eq!(specs[0].privileges, vec!["SELECT"]);
        assert_eq!(specs[1].database, "db2");
        assert_eq!(specs[1].privileges, vec!["ALL"]);
    }

    #[test]
    fn test_privilege_spec_invalid() {
        assert!(PrivilegeSpec::parse("invalid").is_err());
        assert!(PrivilegeSpec::parse("db:").is_err());
        assert!(PrivilegeSpec::parse(":PRIV").is_err());
        assert!(PrivilegeSpec::parse("db.table:INVALID_PRIV").is_err());
    }

    #[test]
    fn test_privilege_spec_to_grant_on() {
        let spec = PrivilegeSpec {
            database: "mydb".to_string(),
            table: "mytable".to_string(),
            privileges: vec!["SELECT".to_string()],
        };
        assert_eq!(spec.to_grant_on(), "`mydb`.`mytable`");

        let spec = PrivilegeSpec {
            database: "*".to_string(),
            table: "*".to_string(),
            privileges: vec!["ALL".to_string()],
        };
        assert_eq!(spec.to_grant_on(), "*.*");

        let spec = PrivilegeSpec {
            database: "mydb".to_string(),
            table: "*".to_string(),
            privileges: vec!["SELECT".to_string()],
        };
        assert_eq!(spec.to_grant_on(), "`mydb`.*");
    }

    #[test]
    fn test_validate_username() {
        assert!(MysqlPrivsModule::validate_username("myuser").is_ok());
        assert!(MysqlPrivsModule::validate_username("my_user").is_ok());
        assert!(MysqlPrivsModule::validate_username("").is_err());
        assert!(MysqlPrivsModule::validate_username("user;drop").is_err());
    }

    #[test]
    fn test_validate_host() {
        assert!(MysqlPrivsModule::validate_host("localhost").is_ok());
        assert!(MysqlPrivsModule::validate_host("%").is_ok());
        assert!(MysqlPrivsModule::validate_host("192.168.1.1").is_ok());
        assert!(MysqlPrivsModule::validate_host("").is_err());
    }

    #[test]
    fn test_module_name() {
        let module = MysqlPrivsModule;
        assert_eq!(module.name(), "mysql_privs");
    }

    #[test]
    fn test_module_required_params() {
        let module = MysqlPrivsModule;
        assert_eq!(module.required_params(), &["user", "priv"]);
    }

    #[test]
    fn test_module_classification() {
        let module = MysqlPrivsModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }
}
