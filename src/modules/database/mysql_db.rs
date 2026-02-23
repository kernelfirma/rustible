//! MySQL database management module
//!
//! This module provides functionality for creating and dropping MySQL databases.
//!
//! # Parameters
//!
//! - `name` (required): Name of the database to manage
//! - `state`: `present` (default) or `absent`
//! - `encoding`: Character encoding for the database (e.g., utf8mb4)
//! - `collation`: Collation for the database (e.g., utf8mb4_unicode_ci)
//! - `login_host`: MySQL server host (default: localhost)
//! - `login_port`: MySQL server port (default: 3306)
//! - `login_user`: MySQL username (default: root)
//! - `login_password`: MySQL password
//! - `login_unix_socket`: Unix socket path for local connections
//!
//! # Example
//!
//! ```yaml
//! # Create a database
//! - mysql_db:
//!     name: myapp
//!     state: present
//!     encoding: utf8mb4
//!     collation: utf8mb4_unicode_ci
//!     login_user: root
//!     login_password: "{{ mysql_root_password }}"
//!
//! # Drop a database
//! - mysql_db:
//!     name: old_database
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

/// Desired state for a database
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseState {
    Present,
    Absent,
}

impl DatabaseState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(DatabaseState::Present),
            "absent" => Ok(DatabaseState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Information about a database
#[derive(Debug, Clone)]
pub struct DatabaseInfo {
    pub name: String,
    pub encoding: String,
    pub collation: String,
}

/// Module for MySQL database management
pub struct MysqlDbModule;

impl MysqlDbModule {
    /// Validate a MySQL identifier (encoding, collation, etc.) to prevent SQL injection.
    /// Only allows alphanumeric characters, underscores, and hyphens.
    fn validate_mysql_identifier(value: &str, param_name: &str) -> ModuleResult<()> {
        if value.is_empty() {
            return Err(ModuleError::InvalidParameter(format!(
                "{} cannot be empty",
                param_name
            )));
        }
        if value.len() > 64 {
            return Err(ModuleError::InvalidParameter(format!(
                "{} cannot exceed 64 characters",
                param_name
            )));
        }
        for c in value.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
                return Err(ModuleError::InvalidParameter(format!(
                    "{} contains invalid character: '{}'. Only alphanumeric, underscore, and hyphen are allowed",
                    param_name, c
                )));
            }
        }
        Ok(())
    }

    /// Validate database name to prevent SQL injection
    fn validate_db_name(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Database name cannot be empty".to_string(),
            ));
        }

        // MySQL database names must be <= 64 characters
        if name.len() > 64 {
            return Err(ModuleError::InvalidParameter(
                "Database name cannot exceed 64 characters".to_string(),
            ));
        }

        // Database names should only contain alphanumeric, underscore, and dollar sign
        // (and not start with a number)
        let first_char = name.chars().next().unwrap();
        if first_char.is_ascii_digit() {
            return Err(ModuleError::InvalidParameter(
                "Database name cannot start with a number".to_string(),
            ));
        }

        for c in name.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '$' {
                return Err(ModuleError::InvalidParameter(format!(
                    "Database name contains invalid character: '{}'",
                    c
                )));
            }
        }

        // Reject MySQL reserved words
        let reserved = ["mysql", "information_schema", "performance_schema", "sys"];
        if reserved.contains(&name.to_lowercase().as_str()) {
            return Err(ModuleError::InvalidParameter(format!(
                "Cannot manage system database: {}",
                name
            )));
        }

        Ok(())
    }

    /// Execute database operations using async runtime
    fn execute_async<F, T>(f: F) -> ModuleResult<T>
    where
        F: std::future::Future<Output = ModuleResult<T>> + Send,
        T: Send,
    {
        // Try to use existing runtime, or create a new one
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

    /// Check if database exists
    async fn database_exists(
        conn_params: &MysqlConnectionParams,
        db_name: &str,
    ) -> ModuleResult<bool> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = format!(
            "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = '{}'",
            db_name.replace('\'', "''")
        );

        let result = pool
            .fetch_optional(&query)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        Ok(result.is_some())
    }

    /// Get database info
    async fn get_database_info(
        conn_params: &MysqlConnectionParams,
        db_name: &str,
    ) -> ModuleResult<Option<DatabaseInfo>> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = format!(
            "SELECT SCHEMA_NAME, DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME \
             FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = '{}'",
            db_name.replace('\'', "''")
        );

        let row = pool
            .fetch_optional(&query)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        Ok(row.map(|r| DatabaseInfo {
            name: r.get::<String, _>("SCHEMA_NAME"),
            encoding: r.get::<String, _>("DEFAULT_CHARACTER_SET_NAME"),
            collation: r.get::<String, _>("DEFAULT_COLLATION_NAME"),
        }))
    }

    /// Create a database
    async fn create_database(
        conn_params: &MysqlConnectionParams,
        db_name: &str,
        encoding: Option<&str>,
        collation: Option<&str>,
    ) -> ModuleResult<()> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let mut query = format!("CREATE DATABASE `{}`", db_name);

        if let Some(enc) = encoding {
            Self::validate_mysql_identifier(enc, "encoding")?;
            query.push_str(&format!(" CHARACTER SET {}", enc));
        }

        if let Some(coll) = collation {
            Self::validate_mysql_identifier(coll, "collation")?;
            query.push_str(&format!(" COLLATE {}", coll));
        }

        pool.execute(&query).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create database: {}", e))
        })?;

        Ok(())
    }

    /// Modify database encoding/collation if needed
    async fn modify_database(
        conn_params: &MysqlConnectionParams,
        db_name: &str,
        current: &DatabaseInfo,
        encoding: Option<&str>,
        collation: Option<&str>,
    ) -> ModuleResult<bool> {
        let needs_encoding_change = encoding.map(|e| e != current.encoding).unwrap_or(false);
        let needs_collation_change = collation.map(|c| c != current.collation).unwrap_or(false);

        if !needs_encoding_change && !needs_collation_change {
            return Ok(false);
        }

        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let mut query = format!("ALTER DATABASE `{}`", db_name);

        if let Some(enc) = encoding {
            if enc != current.encoding {
                Self::validate_mysql_identifier(enc, "encoding")?;
                query.push_str(&format!(" CHARACTER SET {}", enc));
            }
        }

        if let Some(coll) = collation {
            if coll != current.collation {
                Self::validate_mysql_identifier(coll, "collation")?;
                query.push_str(&format!(" COLLATE {}", coll));
            }
        }

        pool.execute(&query).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to modify database: {}", e))
        })?;

        Ok(true)
    }

    /// Drop a database
    async fn drop_database(conn_params: &MysqlConnectionParams, db_name: &str) -> ModuleResult<()> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(None))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        let query = format!("DROP DATABASE `{}`", db_name);

        pool.execute(&query)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to drop database: {}", e)))?;

        Ok(())
    }
}

impl Module for MysqlDbModule {
    fn name(&self) -> &'static str {
        "mysql_db"
    }

    fn description(&self) -> &'static str {
        "Create or drop MySQL databases"
    }

    fn classification(&self) -> ModuleClassification {
        // This runs on the control node, connecting to MySQL directly
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Database create/drop operations should be serialized per host to avoid conflicts
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let name = params.get_string_required("name")?;
        Self::validate_db_name(&name)?;

        if let Some(state) = params.get_string("state")? {
            DatabaseState::from_str(&state)?;
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let db_name = params.get_string_required("name")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = DatabaseState::from_str(&state_str)?;

        let encoding = params.get_string("encoding")?;
        let collation = params.get_string("collation")?;

        let conn_params = extract_connection_params(params)?;

        Self::execute_async(async {
            let exists = Self::database_exists(&conn_params, &db_name).await?;

            match state {
                DatabaseState::Absent => {
                    if !exists {
                        return Ok(ModuleOutput::ok(format!(
                            "Database '{}' already absent",
                            db_name
                        )));
                    }

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would drop database '{}'",
                            db_name
                        )));
                    }

                    Self::drop_database(&conn_params, &db_name).await?;

                    Ok(ModuleOutput::changed(format!(
                        "Dropped database '{}'",
                        db_name
                    )))
                }

                DatabaseState::Present => {
                    if !exists {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would create database '{}'",
                                db_name
                            )));
                        }

                        Self::create_database(
                            &conn_params,
                            &db_name,
                            encoding.as_deref(),
                            collation.as_deref(),
                        )
                        .await?;

                        let mut output =
                            ModuleOutput::changed(format!("Created database '{}'", db_name));

                        // Add database info to output
                        if let Some(info) = Self::get_database_info(&conn_params, &db_name).await? {
                            output = output
                                .with_data("encoding", serde_json::json!(info.encoding))
                                .with_data("collation", serde_json::json!(info.collation));
                        }

                        return Ok(output);
                    }

                    // Database exists, check if modifications are needed
                    let current = Self::get_database_info(&conn_params, &db_name)
                        .await?
                        .ok_or_else(|| {
                            ModuleError::ExecutionFailed(format!(
                                "Database '{}' exists but cannot read info",
                                db_name
                            ))
                        })?;

                    let needs_change = encoding
                        .as_ref()
                        .map(|e| e != &current.encoding)
                        .unwrap_or(false)
                        || collation
                            .as_ref()
                            .map(|c| c != &current.collation)
                            .unwrap_or(false);

                    if !needs_change {
                        return Ok(ModuleOutput::ok(format!(
                            "Database '{}' already exists with correct settings",
                            db_name
                        ))
                        .with_data("encoding", serde_json::json!(current.encoding))
                        .with_data("collation", serde_json::json!(current.collation)));
                    }

                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would modify database '{}' settings",
                            db_name
                        )));
                    }

                    Self::modify_database(
                        &conn_params,
                        &db_name,
                        &current,
                        encoding.as_deref(),
                        collation.as_deref(),
                    )
                    .await?;

                    let updated = Self::get_database_info(&conn_params, &db_name)
                        .await?
                        .unwrap_or(current);

                    Ok(
                        ModuleOutput::changed(format!("Modified database '{}' settings", db_name))
                            .with_data("encoding", serde_json::json!(updated.encoding))
                            .with_data("collation", serde_json::json!(updated.collation)),
                    )
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
    fn test_validate_db_name_valid() {
        assert!(MysqlDbModule::validate_db_name("myapp").is_ok());
        assert!(MysqlDbModule::validate_db_name("my_app").is_ok());
        assert!(MysqlDbModule::validate_db_name("MyApp123").is_ok());
        assert!(MysqlDbModule::validate_db_name("app$data").is_ok());
    }

    #[test]
    fn test_validate_db_name_invalid() {
        // Empty name
        assert!(MysqlDbModule::validate_db_name("").is_err());

        // Starts with number
        assert!(MysqlDbModule::validate_db_name("123app").is_err());

        // Invalid characters
        assert!(MysqlDbModule::validate_db_name("my-app").is_err());
        assert!(MysqlDbModule::validate_db_name("my app").is_err());
        assert!(MysqlDbModule::validate_db_name("my;app").is_err());

        // System databases
        assert!(MysqlDbModule::validate_db_name("mysql").is_err());
        assert!(MysqlDbModule::validate_db_name("information_schema").is_err());
    }

    #[test]
    fn test_database_state_from_str() {
        assert_eq!(
            DatabaseState::from_str("present").unwrap(),
            DatabaseState::Present
        );
        assert_eq!(
            DatabaseState::from_str("absent").unwrap(),
            DatabaseState::Absent
        );
        assert!(DatabaseState::from_str("invalid").is_err());
    }

    #[test]
    fn test_module_name() {
        let module = MysqlDbModule;
        assert_eq!(module.name(), "mysql_db");
    }

    #[test]
    fn test_module_classification() {
        let module = MysqlDbModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_required_params() {
        let module = MysqlDbModule;
        assert_eq!(module.required_params(), &["name"]);
    }
}
