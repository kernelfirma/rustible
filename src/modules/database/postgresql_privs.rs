//! PostgreSQL Privileges module - Privilege management
//!
//! This module manages PostgreSQL privileges on database objects including
//! databases, schemas, tables, sequences, functions, and types.
//!
//! ## Parameters
//!
//! - `role`: Role/user to grant/revoke privileges for (required)
//! - `database`: Target database (required for database-level privs)
//! - `state`: Desired state (present, absent) - default: present
//! - `type`: Object type (database, schema, table, sequence, function, type, default_privs)
//! - `objs`: Object names to manage privileges on (comma-separated or ALL)
//! - `schema`: Schema containing the objects (default: public)
//! - `privs`: Privileges to grant/revoke (comma-separated or ALL)
//! - `grant_option`: Grant WITH GRANT OPTION (default: false)
//! - `target_roles`: For default_privs, the roles whose defaults are being altered
//! - `login_host`: PostgreSQL server host (default: localhost)
//! - `login_port`: PostgreSQL server port (default: 5432)
//! - `login_user`: PostgreSQL login user (default: postgres)
//! - `login_password`: PostgreSQL login password
//! - `login_unix_socket`: Unix socket path for local connections
//! - `ssl_mode`: SSL mode (disable, allow, prefer, require, verify-ca, verify-full)
//!
//! ## Object Types and Valid Privileges
//!
//! - `database`: CREATE, CONNECT, TEMP, TEMPORARY, ALL
//! - `schema`: CREATE, USAGE, ALL
//! - `table`: SELECT, INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, ALL
//! - `sequence`: USAGE, SELECT, UPDATE, ALL
//! - `function`: EXECUTE, ALL
//! - `type`: USAGE, ALL
//! - `default_privs`: Set default privileges for future objects
//!
//! ## Examples
//!
//! ```yaml
//! # Grant SELECT on all tables in a schema
//! - postgresql_privs:
//!     role: myapp
//!     database: mydb
//!     type: table
//!     objs: ALL_IN_SCHEMA
//!     schema: public
//!     privs: SELECT
//!
//! # Grant all privileges on a specific table
//! - postgresql_privs:
//!     role: admin
//!     database: mydb
//!     type: table
//!     objs: users,orders
//!     privs: ALL
//!     grant_option: true
//!
//! # Revoke database access
//! - postgresql_privs:
//!     role: readonly
//!     database: mydb
//!     type: database
//!     privs: CONNECT
//!     state: absent
//! ```

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::utils::shell_escape;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::postgresql_db::{PgConnectionConfig, SslMode};

/// Privilege state
#[derive(Debug, Clone, PartialEq)]
pub enum PrivState {
    /// Privileges should be granted
    Present,
    /// Privileges should be revoked
    Absent,
}

impl PrivState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "grant" => Ok(PrivState::Present),
            "absent" | "revoke" => Ok(PrivState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Object type for privilege management
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectType {
    Database,
    Schema,
    Table,
    Sequence,
    Function,
    Type,
    DefaultPrivs,
}

impl ObjectType {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "database" | "db" => Ok(ObjectType::Database),
            "schema" => Ok(ObjectType::Schema),
            "table" => Ok(ObjectType::Table),
            "sequence" => Ok(ObjectType::Sequence),
            "function" | "procedure" => Ok(ObjectType::Function),
            "type" => Ok(ObjectType::Type),
            "default_privs" | "default" => Ok(ObjectType::DefaultPrivs),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid object type '{}'. Valid types: database, schema, table, sequence, function, type, default_privs",
                s
            ))),
        }
    }

    fn valid_privileges(&self) -> &[&str] {
        match self {
            ObjectType::Database => &["CREATE", "CONNECT", "TEMP", "TEMPORARY", "ALL"],
            ObjectType::Schema => &["CREATE", "USAGE", "ALL"],
            ObjectType::Table => &[
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
                "ALL",
            ],
            ObjectType::Sequence => &["USAGE", "SELECT", "UPDATE", "ALL"],
            ObjectType::Function => &["EXECUTE", "ALL"],
            ObjectType::Type => &["USAGE", "ALL"],
            ObjectType::DefaultPrivs => &[
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
                "USAGE",
                "EXECUTE",
                "ALL",
            ],
        }
    }

    fn as_sql_keyword(&self) -> &str {
        match self {
            ObjectType::Database => "DATABASE",
            ObjectType::Schema => "SCHEMA",
            ObjectType::Table => "TABLE",
            ObjectType::Sequence => "SEQUENCE",
            ObjectType::Function => "FUNCTION",
            ObjectType::Type => "TYPE",
            ObjectType::DefaultPrivs => "TABLES", // Used in ALTER DEFAULT PRIVILEGES
        }
    }
}

/// Privilege configuration parsed from parameters
#[derive(Debug, Clone)]
struct PrivConfig {
    role: String,
    database: String,
    state: PrivState,
    obj_type: ObjectType,
    objs: Vec<String>,
    schema: String,
    privs: Vec<String>,
    grant_option: bool,
    target_roles: Vec<String>,
    conn: PgConnectionConfig,
}

impl PrivConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let state = if let Some(s) = params.get_string("state")? {
            PrivState::from_str(&s)?
        } else {
            PrivState::Present
        };

        let obj_type = if let Some(t) = params.get_string("type")? {
            ObjectType::from_str(&t)?
        } else {
            ObjectType::Table // Default to table privileges
        };

        // Parse object names
        let objs = if let Some(objs_str) = params.get_string("objs")? {
            objs_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else if obj_type == ObjectType::Database {
            // For database privileges, use the database name
            vec![]
        } else {
            Vec::new()
        };

        // Parse privileges
        let privs_str = params.get_string("privs")?.unwrap_or_default();
        let privs: Vec<String> = privs_str
            .split(',')
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .collect();

        // Validate privileges against object type
        let valid = obj_type.valid_privileges();
        for privilege in &privs {
            if !valid.contains(&privilege.as_ref()) && privilege != "ALL" && privilege != "ALL PRIVILEGES" {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid privilege '{}' for object type '{:?}'. Valid privileges: {:?}",
                    privilege, obj_type, valid
                )));
            }
        }

        // Parse target roles for default privileges
        let target_roles = params.get_vec_string("target_roles")?.unwrap_or_default();

        // Parse connection config
        let ssl_mode = if let Some(mode) = params.get_string("ssl_mode")? {
            SslMode::from_str(&mode)?
        } else {
            SslMode::Prefer
        };

        let database = params.get_string_required("database")?;

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
            maintenance_db: database.clone(),
        };

        Ok(Self {
            role: params.get_string_required("role")?,
            database,
            state,
            obj_type,
            objs,
            schema: params
                .get_string("schema")?
                .unwrap_or_else(|| "public".to_string()),
            privs,
            grant_option: params.get_bool_or("grant_option", false),
            target_roles,
            conn,
        })
    }
}

/// Module for PostgreSQL privilege management
pub struct PostgresqlPrivsModule;

impl PostgresqlPrivsModule {
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
        config: &PrivConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let query = format!(
            "SELECT 1 FROM pg_roles WHERE rolname = '{}'",
            config.role.replace('\'', "''")
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

    /// Get objects in a schema (tables, sequences, functions)
    async fn get_schema_objects(
        connection: &dyn Connection,
        config: &PrivConfig,
        context: &ModuleContext,
    ) -> ModuleResult<Vec<String>> {
        let query = match config.obj_type {
            ObjectType::Table => format!(
                "SELECT tablename FROM pg_tables WHERE schemaname = '{}'",
                config.schema.replace('\'', "''")
            ),
            ObjectType::Sequence => format!(
                "SELECT sequencename FROM pg_sequences WHERE schemaname = '{}'",
                config.schema.replace('\'', "''")
            ),
            ObjectType::Function => format!(
                "SELECT p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')' \
                 FROM pg_proc p JOIN pg_namespace n ON p.pronamespace = n.oid \
                 WHERE n.nspname = '{}'",
                config.schema.replace('\'', "''")
            ),
            ObjectType::Type => format!(
                "SELECT t.typname FROM pg_type t \
                 JOIN pg_namespace n ON t.typnamespace = n.oid \
                 WHERE n.nspname = '{}' AND t.typtype IN ('c', 'e', 'd')",
                config.schema.replace('\'', "''")
            ),
            _ => return Ok(Vec::new()),
        };

        let cmd = format!(
            "psql {} -tAc \"{}\"",
            config.conn.build_psql_args(&config.database),
            query
        );

        let (success, stdout, _) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if !success {
            return Ok(Vec::new());
        }

        Ok(stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }

    /// Get current privileges for a role on objects
    async fn get_current_privileges(
        connection: &dyn Connection,
        config: &PrivConfig,
        context: &ModuleContext,
    ) -> ModuleResult<HashSet<String>> {
        let mut current_privs = HashSet::new();

        match config.obj_type {
            ObjectType::Database => {
                // Check database privileges
                let query = format!(
                    "SELECT privilege_type FROM information_schema.role_table_grants \
                     WHERE grantee = '{}' AND table_catalog = '{}' \
                     UNION \
                     SELECT CASE \
                       WHEN has_database_privilege('{}', '{}', 'CREATE') THEN 'CREATE' \
                       ELSE NULL END \
                     UNION \
                     SELECT CASE \
                       WHEN has_database_privilege('{}', '{}', 'CONNECT') THEN 'CONNECT' \
                       ELSE NULL END \
                     UNION \
                     SELECT CASE \
                       WHEN has_database_privilege('{}', '{}', 'TEMP') THEN 'TEMP' \
                       ELSE NULL END",
                    config.role.replace('\'', "''"),
                    config.database.replace('\'', "''"),
                    config.role.replace('\'', "''"),
                    config.database.replace('\'', "''"),
                    config.role.replace('\'', "''"),
                    config.database.replace('\'', "''"),
                    config.role.replace('\'', "''"),
                    config.database.replace('\'', "''"),
                );

                let cmd = format!(
                    "psql {} -tAc \"{}\"",
                    config.conn.build_psql_args(&config.database),
                    query
                );

                let (success, stdout, _) = Self::execute_command(
                    connection,
                    &cmd,
                    context,
                    config.conn.build_env_vars(),
                )
                .await?;

                if success {
                    for line in stdout.lines() {
                        let priv_name = line.trim().to_uppercase();
                        if !priv_name.is_empty() {
                            current_privs.insert(priv_name);
                        }
                    }
                }
            }
            ObjectType::Schema => {
                // Check schema privileges
                for privilege in &["CREATE", "USAGE"] {
                    let query = format!(
                        "SELECT has_schema_privilege('{}', '{}', '{}')",
                        config.role.replace('\'', "''"),
                        config.schema.replace('\'', "''"),
                        privilege
                    );

                    let cmd = format!(
                        "psql {} -tAc \"{}\"",
                        config.conn.build_psql_args(&config.database),
                        query
                    );

                    let (success, stdout, _) = Self::execute_command(
                        connection,
                        &cmd,
                        context,
                        config.conn.build_env_vars(),
                    )
                    .await?;

                    if success && stdout.trim() == "t" {
                        current_privs.insert(privilege.to_string());
                    }
                }
            }
            ObjectType::Table => {
                // Check table privileges
                for obj in &config.objs {
                    if obj.to_uppercase() == "ALL_IN_SCHEMA" {
                        continue;
                    }

                    let query = format!(
                        "SELECT privilege_type FROM information_schema.role_table_grants \
                         WHERE grantee = '{}' AND table_schema = '{}' AND table_name = '{}'",
                        config.role.replace('\'', "''"),
                        config.schema.replace('\'', "''"),
                        obj.replace('\'', "''")
                    );

                    let cmd = format!(
                        "psql {} -tAc \"{}\"",
                        config.conn.build_psql_args(&config.database),
                        query
                    );

                    let (success, stdout, _) = Self::execute_command(
                        connection,
                        &cmd,
                        context,
                        config.conn.build_env_vars(),
                    )
                    .await?;

                    if success {
                        for line in stdout.lines() {
                            let priv_name = line.trim().to_uppercase();
                            if !priv_name.is_empty() {
                                current_privs.insert(priv_name);
                            }
                        }
                    }
                }
            }
            _ => {
                // For other types, we'll just proceed with grant/revoke
            }
        }

        Ok(current_privs)
    }

    /// Grant privileges
    async fn grant_privileges(
        connection: &dyn Connection,
        config: &PrivConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let mut changed = false;
        let privs_str = config.privs.join(", ");
        let role = shell_escape(&config.role);

        match config.obj_type {
            ObjectType::Database => {
                let sql = format!(
                    "GRANT {} ON DATABASE {} TO {}{}",
                    privs_str,
                    shell_escape(&config.database),
                    role,
                    if config.grant_option {
                        " WITH GRANT OPTION"
                    } else {
                        ""
                    }
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.database),
                    sql
                );

                let (success, _, stderr) = Self::execute_command(
                    connection,
                    &cmd,
                    context,
                    config.conn.build_env_vars(),
                )
                .await?;

                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to grant database privileges: {}",
                        stderr
                    )));
                }
                changed = true;
            }

            ObjectType::Schema => {
                let sql = format!(
                    "GRANT {} ON SCHEMA {} TO {}{}",
                    privs_str,
                    shell_escape(&config.schema),
                    role,
                    if config.grant_option {
                        " WITH GRANT OPTION"
                    } else {
                        ""
                    }
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.database),
                    sql
                );

                let (success, _, stderr) = Self::execute_command(
                    connection,
                    &cmd,
                    context,
                    config.conn.build_env_vars(),
                )
                .await?;

                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to grant schema privileges: {}",
                        stderr
                    )));
                }
                changed = true;
            }

            ObjectType::Table | ObjectType::Sequence | ObjectType::Type => {
                let objs = if config.objs.is_empty()
                    || config
                        .objs
                        .iter()
                        .any(|o| o.to_uppercase() == "ALL_IN_SCHEMA")
                {
                    // Get all objects in schema
                    Self::get_schema_objects(connection, config, context).await?
                } else {
                    config.objs.clone()
                };

                for obj in objs {
                    let qualified_obj = format!("{}.{}", config.schema, obj);
                    let sql = format!(
                        "GRANT {} ON {} {} TO {}{}",
                        privs_str,
                        config.obj_type.as_sql_keyword(),
                        shell_escape(&qualified_obj),
                        role,
                        if config.grant_option {
                            " WITH GRANT OPTION"
                        } else {
                            ""
                        }
                    );
                    let cmd = format!(
                        "psql {} -c \"{}\"",
                        config.conn.build_psql_args(&config.database),
                        sql
                    );

                    let (success, _, stderr) = Self::execute_command(
                        connection,
                        &cmd,
                        context,
                        config.conn.build_env_vars(),
                    )
                    .await?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to grant {} privileges on '{}': {}",
                            config.obj_type.as_sql_keyword(),
                            obj,
                            stderr
                        )));
                    }
                    changed = true;
                }
            }

            ObjectType::Function => {
                let objs = if config.objs.is_empty()
                    || config
                        .objs
                        .iter()
                        .any(|o| o.to_uppercase() == "ALL_IN_SCHEMA")
                {
                    Self::get_schema_objects(connection, config, context).await?
                } else {
                    config.objs.clone()
                };

                for obj in objs {
                    let qualified_obj = format!("{}.{}", config.schema, obj);
                    let sql = format!(
                        "GRANT {} ON FUNCTION {} TO {}{}",
                        privs_str,
                        shell_escape(&qualified_obj),
                        role,
                        if config.grant_option {
                            " WITH GRANT OPTION"
                        } else {
                            ""
                        }
                    );
                    let cmd = format!(
                        "psql {} -c \"{}\"",
                        config.conn.build_psql_args(&config.database),
                        sql
                    );

                    let (success, _, stderr) = Self::execute_command(
                        connection,
                        &cmd,
                        context,
                        config.conn.build_env_vars(),
                    )
                    .await?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to grant function privileges on '{}': {}",
                            obj, stderr
                        )));
                    }
                    changed = true;
                }
            }

            ObjectType::DefaultPrivs => {
                // ALTER DEFAULT PRIVILEGES
                let target_role = if config.target_roles.is_empty() {
                    config.conn.user.clone()
                } else {
                    config.target_roles.join(", ")
                };

                let obj_keyword = if config.objs.is_empty() {
                    "TABLES"
                } else {
                    match config.objs[0].to_uppercase().as_str() {
                        "TABLES" | "TABLE" => "TABLES",
                        "SEQUENCES" | "SEQUENCE" => "SEQUENCES",
                        "FUNCTIONS" | "FUNCTION" => "FUNCTIONS",
                        "TYPES" | "TYPE" => "TYPES",
                        "SCHEMAS" | "SCHEMA" => "SCHEMAS",
                        _ => "TABLES",
                    }
                };

                let sql = format!(
                    "ALTER DEFAULT PRIVILEGES FOR ROLE {} IN SCHEMA {} GRANT {} ON {} TO {}{}",
                    shell_escape(&target_role),
                    shell_escape(&config.schema),
                    privs_str,
                    obj_keyword,
                    role,
                    if config.grant_option {
                        " WITH GRANT OPTION"
                    } else {
                        ""
                    }
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.database),
                    sql
                );

                let (success, _, stderr) = Self::execute_command(
                    connection,
                    &cmd,
                    context,
                    config.conn.build_env_vars(),
                )
                .await?;

                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to set default privileges: {}",
                        stderr
                    )));
                }
                changed = true;
            }
        }

        Ok(changed)
    }

    /// Revoke privileges
    async fn revoke_privileges(
        connection: &dyn Connection,
        config: &PrivConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let mut changed = false;
        let privs_str = config.privs.join(", ");
        let role = shell_escape(&config.role);

        match config.obj_type {
            ObjectType::Database => {
                let sql = format!(
                    "REVOKE {} ON DATABASE {} FROM {}",
                    privs_str,
                    shell_escape(&config.database),
                    role
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.database),
                    sql
                );

                let (success, _, stderr) = Self::execute_command(
                    connection,
                    &cmd,
                    context,
                    config.conn.build_env_vars(),
                )
                .await?;

                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to revoke database privileges: {}",
                        stderr
                    )));
                }
                changed = true;
            }

            ObjectType::Schema => {
                let sql = format!(
                    "REVOKE {} ON SCHEMA {} FROM {}",
                    privs_str,
                    shell_escape(&config.schema),
                    role
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.database),
                    sql
                );

                let (success, _, stderr) = Self::execute_command(
                    connection,
                    &cmd,
                    context,
                    config.conn.build_env_vars(),
                )
                .await?;

                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to revoke schema privileges: {}",
                        stderr
                    )));
                }
                changed = true;
            }

            ObjectType::Table | ObjectType::Sequence | ObjectType::Type => {
                let objs = if config.objs.is_empty()
                    || config
                        .objs
                        .iter()
                        .any(|o| o.to_uppercase() == "ALL_IN_SCHEMA")
                {
                    Self::get_schema_objects(connection, config, context).await?
                } else {
                    config.objs.clone()
                };

                for obj in objs {
                    let qualified_obj = format!("{}.{}", config.schema, obj);
                    let sql = format!(
                        "REVOKE {} ON {} {} FROM {}",
                        privs_str,
                        config.obj_type.as_sql_keyword(),
                        shell_escape(&qualified_obj),
                        role
                    );
                    let cmd = format!(
                        "psql {} -c \"{}\"",
                        config.conn.build_psql_args(&config.database),
                        sql
                    );

                    let (success, _, stderr) = Self::execute_command(
                        connection,
                        &cmd,
                        context,
                        config.conn.build_env_vars(),
                    )
                    .await?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to revoke {} privileges on '{}': {}",
                            config.obj_type.as_sql_keyword(),
                            obj,
                            stderr
                        )));
                    }
                    changed = true;
                }
            }

            ObjectType::Function => {
                let objs = if config.objs.is_empty()
                    || config
                        .objs
                        .iter()
                        .any(|o| o.to_uppercase() == "ALL_IN_SCHEMA")
                {
                    Self::get_schema_objects(connection, config, context).await?
                } else {
                    config.objs.clone()
                };

                for obj in objs {
                    let qualified_obj = format!("{}.{}", config.schema, obj);
                    let sql = format!(
                        "REVOKE {} ON FUNCTION {} FROM {}",
                        privs_str,
                        shell_escape(&qualified_obj),
                        role
                    );
                    let cmd = format!(
                        "psql {} -c \"{}\"",
                        config.conn.build_psql_args(&config.database),
                        sql
                    );

                    let (success, _, stderr) = Self::execute_command(
                        connection,
                        &cmd,
                        context,
                        config.conn.build_env_vars(),
                    )
                    .await?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to revoke function privileges on '{}': {}",
                            obj, stderr
                        )));
                    }
                    changed = true;
                }
            }

            ObjectType::DefaultPrivs => {
                let target_role = if config.target_roles.is_empty() {
                    config.conn.user.clone()
                } else {
                    config.target_roles.join(", ")
                };

                let obj_keyword = if config.objs.is_empty() {
                    "TABLES"
                } else {
                    match config.objs[0].to_uppercase().as_str() {
                        "TABLES" | "TABLE" => "TABLES",
                        "SEQUENCES" | "SEQUENCE" => "SEQUENCES",
                        "FUNCTIONS" | "FUNCTION" => "FUNCTIONS",
                        "TYPES" | "TYPE" => "TYPES",
                        "SCHEMAS" | "SCHEMA" => "SCHEMAS",
                        _ => "TABLES",
                    }
                };

                let sql = format!(
                    "ALTER DEFAULT PRIVILEGES FOR ROLE {} IN SCHEMA {} REVOKE {} ON {} FROM {}",
                    shell_escape(&target_role),
                    shell_escape(&config.schema),
                    privs_str,
                    obj_keyword,
                    role
                );
                let cmd = format!(
                    "psql {} -c \"{}\"",
                    config.conn.build_psql_args(&config.database),
                    sql
                );

                let (success, _, stderr) = Self::execute_command(
                    connection,
                    &cmd,
                    context,
                    config.conn.build_env_vars(),
                )
                .await?;

                if !success {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to revoke default privileges: {}",
                        stderr
                    )));
                }
                changed = true;
            }
        }

        Ok(changed)
    }

    /// Execute the module with async connection
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let config = PrivConfig::from_params(params)?;

        // Validate role exists
        if !Self::role_exists(connection.as_ref(), &config, context).await? {
            return Err(ModuleError::ExecutionFailed(format!(
                "Role '{}' does not exist",
                config.role
            )));
        }

        // Validate privileges were specified
        if config.privs.is_empty() {
            return Err(ModuleError::MissingParameter(
                "No privileges specified (privs parameter is required)".to_string(),
            ));
        }

        match config.state {
            PrivState::Present => {
                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would grant {} privileges to '{}'",
                        config.privs.join(", "),
                        config.role
                    )));
                }

                let changed =
                    Self::grant_privileges(connection.as_ref(), &config, context).await?;

                if changed {
                    Ok(ModuleOutput::changed(format!(
                        "Granted {} privileges to '{}'",
                        config.privs.join(", "),
                        config.role
                    ))
                    .with_data("role", serde_json::json!(config.role))
                    .with_data("privileges", serde_json::json!(config.privs))
                    .with_data("object_type", serde_json::json!(format!("{:?}", config.obj_type))))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Role '{}' already has requested privileges",
                        config.role
                    )))
                }
            }

            PrivState::Absent => {
                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would revoke {} privileges from '{}'",
                        config.privs.join(", "),
                        config.role
                    )));
                }

                let changed =
                    Self::revoke_privileges(connection.as_ref(), &config, context).await?;

                if changed {
                    Ok(ModuleOutput::changed(format!(
                        "Revoked {} privileges from '{}'",
                        config.privs.join(", "),
                        config.role
                    ))
                    .with_data("role", serde_json::json!(config.role))
                    .with_data("privileges", serde_json::json!(config.privs))
                    .with_data("object_type", serde_json::json!(format!("{:?}", config.obj_type))))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Role '{}' did not have specified privileges",
                        config.role
                    )))
                }
            }
        }
    }
}

impl Module for PostgresqlPrivsModule {
    fn name(&self) -> &'static str {
        "postgresql_privs"
    }

    fn description(&self) -> &'static str {
        "Manage PostgreSQL privileges on database objects"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        &["role", "database"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.clone().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available for postgresql_privs module execution".to_string(),
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
    fn test_priv_state_from_str() {
        assert_eq!(PrivState::from_str("present").unwrap(), PrivState::Present);
        assert_eq!(PrivState::from_str("grant").unwrap(), PrivState::Present);
        assert_eq!(PrivState::from_str("absent").unwrap(), PrivState::Absent);
        assert_eq!(PrivState::from_str("revoke").unwrap(), PrivState::Absent);
        assert!(PrivState::from_str("invalid").is_err());
    }

    #[test]
    fn test_object_type_from_str() {
        assert_eq!(
            ObjectType::from_str("database").unwrap(),
            ObjectType::Database
        );
        assert_eq!(ObjectType::from_str("db").unwrap(), ObjectType::Database);
        assert_eq!(ObjectType::from_str("schema").unwrap(), ObjectType::Schema);
        assert_eq!(ObjectType::from_str("table").unwrap(), ObjectType::Table);
        assert_eq!(
            ObjectType::from_str("sequence").unwrap(),
            ObjectType::Sequence
        );
        assert_eq!(
            ObjectType::from_str("function").unwrap(),
            ObjectType::Function
        );
        assert_eq!(ObjectType::from_str("type").unwrap(), ObjectType::Type);
        assert_eq!(
            ObjectType::from_str("default_privs").unwrap(),
            ObjectType::DefaultPrivs
        );
        assert!(ObjectType::from_str("invalid").is_err());
    }

    #[test]
    fn test_object_type_valid_privileges() {
        let db_privs = ObjectType::Database.valid_privileges();
        assert!(db_privs.contains(&"CREATE"));
        assert!(db_privs.contains(&"CONNECT"));
        assert!(db_privs.contains(&"TEMP"));

        let table_privs = ObjectType::Table.valid_privileges();
        assert!(table_privs.contains(&"SELECT"));
        assert!(table_privs.contains(&"INSERT"));
        assert!(table_privs.contains(&"UPDATE"));
        assert!(table_privs.contains(&"DELETE"));
        assert!(table_privs.contains(&"ALL"));

        let schema_privs = ObjectType::Schema.valid_privileges();
        assert!(schema_privs.contains(&"CREATE"));
        assert!(schema_privs.contains(&"USAGE"));

        let seq_privs = ObjectType::Sequence.valid_privileges();
        assert!(seq_privs.contains(&"USAGE"));
        assert!(seq_privs.contains(&"SELECT"));
        assert!(seq_privs.contains(&"UPDATE"));

        let func_privs = ObjectType::Function.valid_privileges();
        assert!(func_privs.contains(&"EXECUTE"));
    }

    #[test]
    fn test_object_type_as_sql_keyword() {
        assert_eq!(ObjectType::Database.as_sql_keyword(), "DATABASE");
        assert_eq!(ObjectType::Schema.as_sql_keyword(), "SCHEMA");
        assert_eq!(ObjectType::Table.as_sql_keyword(), "TABLE");
        assert_eq!(ObjectType::Sequence.as_sql_keyword(), "SEQUENCE");
        assert_eq!(ObjectType::Function.as_sql_keyword(), "FUNCTION");
        assert_eq!(ObjectType::Type.as_sql_keyword(), "TYPE");
    }

    #[test]
    fn test_module_metadata() {
        let module = PostgresqlPrivsModule;
        assert_eq!(module.name(), "postgresql_privs");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["role", "database"]);
    }

    #[test]
    fn test_privilege_validation() {
        // Valid table privilege
        let valid = ObjectType::Table.valid_privileges();
        assert!(valid.contains(&"SELECT"));

        // Valid database privilege
        let valid = ObjectType::Database.valid_privileges();
        assert!(valid.contains(&"CONNECT"));
    }

    #[test]
    fn test_all_object_types_have_all_privilege() {
        // ALL privilege should be valid for all object types
        for obj_type in [
            ObjectType::Database,
            ObjectType::Schema,
            ObjectType::Table,
            ObjectType::Sequence,
            ObjectType::Function,
            ObjectType::Type,
            ObjectType::DefaultPrivs,
        ] {
            let valid = obj_type.valid_privileges();
            assert!(
                valid.contains(&"ALL"),
                "{:?} should have ALL privilege",
                obj_type
            );
        }
    }
}
