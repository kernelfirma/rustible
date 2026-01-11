//! PostgreSQL Query module - Execute queries and scripts
//!
//! This module executes SQL queries or scripts against PostgreSQL databases.
//!
//! ## Parameters
//!
//! - `query`: SQL query to execute (mutually exclusive with path_to_script)
//! - `path_to_script`: Path to SQL script file to execute
//! - `db`: Target database name (required)
//! - `positional_args`: List of positional arguments for parameterized queries
//! - `named_args`: Dictionary of named arguments for parameterized queries
//! - `encoding`: Character encoding for the script file
//! - `autocommit`: Whether to use autocommit mode (default: false)
//! - `as_single_query`: Run script as a single query (default: false)
//! - `search_path`: Schema search path
//! - `login_host`: PostgreSQL server host (default: localhost)
//! - `login_port`: PostgreSQL server port (default: 5432)
//! - `login_user`: PostgreSQL login user (default: postgres)
//! - `login_password`: PostgreSQL login password
//! - `login_unix_socket`: Unix socket path for local connections
//! - `ssl_mode`: SSL mode (disable, allow, prefer, require, verify-ca, verify-full)

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt, ParallelizationHint,
};
use crate::utils::shell_escape;
use std::collections::HashMap;
use std::sync::Arc;

use super::postgresql_db::{PgConnectionConfig, SslMode};

/// Query configuration parsed from parameters
#[derive(Debug, Clone)]
struct QueryConfig {
    query: Option<String>,
    path_to_script: Option<String>,
    db: String,
    positional_args: Vec<String>,
    named_args: HashMap<String, String>,
    encoding: Option<String>,
    autocommit: bool,
    as_single_query: bool,
    search_path: Option<String>,
    conn: PgConnectionConfig,
}

impl QueryConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let query = params.get_string("query")?;
        let path_to_script = params.get_string("path_to_script")?;

        // Validate that exactly one of query or path_to_script is provided
        if query.is_none() && path_to_script.is_none() {
            return Err(ModuleError::MissingParameter(
                "Either 'query' or 'path_to_script' must be provided".to_string(),
            ));
        }
        if query.is_some() && path_to_script.is_some() {
            return Err(ModuleError::InvalidParameter(
                "'query' and 'path_to_script' are mutually exclusive".to_string(),
            ));
        }

        let positional_args = params.get_vec_string("positional_args")?.unwrap_or_default();

        // Parse named_args from JSON object
        let named_args = if let Some(serde_json::Value::Object(obj)) = params.get("named_args") {
            obj.iter()
                .filter_map(|(k, v)| {
                    if let serde_json::Value::String(s) = v {
                        Some((k.clone(), s.clone()))
                    } else {
                        Some((k.clone(), v.to_string()))
                    }
                })
                .collect()
        } else {
            HashMap::new()
        };

        // Parse connection config
        let ssl_mode = if let Some(mode) = params.get_string("ssl_mode")? {
            SslMode::from_str(&mode)?
        } else {
            SslMode::Prefer
        };

        let db = params.get_string_required("db")?;

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
            maintenance_db: db.clone(),
        };

        Ok(Self {
            query,
            path_to_script,
            db,
            positional_args,
            named_args,
            encoding: params.get_string("encoding")?,
            autocommit: params.get_bool_or("autocommit", false),
            as_single_query: params.get_bool_or("as_single_query", false),
            search_path: params.get_string("search_path")?,
            conn,
        })
    }

    /// Get the SQL to execute (either from query param or will read from file)
    fn get_sql(&self) -> Option<&str> {
        self.query.as_deref()
    }
}

/// Query execution result
#[derive(Debug, Clone)]
struct QueryResult {
    rowcount: i64,
    rows: Vec<Vec<String>>,
    columns: Vec<String>,
    status: String,
}

/// Module for PostgreSQL query execution
pub struct PostgresqlQueryModule;

impl PostgresqlQueryModule {
    /// Build execute options with privilege escalation and environment
    fn build_execute_options(context: &ModuleContext, env: HashMap<String, String>) -> ExecuteOptions {
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
            .map_err(|e| ModuleError::ExecutionFailed(format!("Connection execute failed: {}", e)))?;
        Ok((result.success, result.stdout, result.stderr))
    }

    /// Substitute positional arguments in query
    fn substitute_positional_args(query: &str, args: &[String]) -> String {
        let mut result = query.to_string();
        for (i, arg) in args.iter().enumerate() {
            let placeholder = format!("${}", i + 1);
            let escaped_arg = arg.replace('\'', "''");
            result = result.replace(&placeholder, &format!("'{}'", escaped_arg));
        }
        result
    }

    /// Substitute named arguments in query
    fn substitute_named_args(query: &str, args: &HashMap<String, String>) -> String {
        let mut result = query.to_string();
        for (name, value) in args {
            // Support both %(name)s and :name syntax
            let placeholder1 = format!("%({})", name);
            let placeholder2 = format!(":{}", name);
            let escaped_value = value.replace('\'', "''");
            let replacement = format!("'{}'", escaped_value);
            result = result.replace(&placeholder1, &replacement);
            result = result.replace(&placeholder2, &replacement);
        }
        result
    }

    /// Check if query is a data modification statement
    fn is_modification_query(query: &str) -> bool {
        let query_upper = query.trim().to_uppercase();
        query_upper.starts_with("INSERT")
            || query_upper.starts_with("UPDATE")
            || query_upper.starts_with("DELETE")
            || query_upper.starts_with("CREATE")
            || query_upper.starts_with("ALTER")
            || query_upper.starts_with("DROP")
            || query_upper.starts_with("TRUNCATE")
            || query_upper.starts_with("GRANT")
            || query_upper.starts_with("REVOKE")
    }

    /// Execute a SQL query
    async fn execute_query(
        connection: &dyn Connection,
        config: &QueryConfig,
        sql: &str,
        context: &ModuleContext,
    ) -> ModuleResult<QueryResult> {
        // Substitute arguments
        let mut processed_sql = Self::substitute_positional_args(sql, &config.positional_args);
        processed_sql = Self::substitute_named_args(&processed_sql, &config.named_args);

        // Build psql command
        let mut psql_opts = Vec::new();

        // Connection options
        psql_opts.push(config.conn.build_psql_args(&config.db));

        // Output formatting for machine parsing
        psql_opts.push("-t".to_string()); // Tuples only
        psql_opts.push("-A".to_string()); // Unaligned
        psql_opts.push("-F '|'".to_string()); // Field separator

        // Set search path if specified
        if let Some(ref search_path) = config.search_path {
            let set_path = format!("SET search_path TO {};", search_path);
            processed_sql = format!("{} {}", set_path, processed_sql);
        }

        // Build full command
        let cmd = format!(
            "psql {} -c \"{}\"",
            psql_opts.join(" "),
            processed_sql.replace('"', "\\\"")
        );

        let (success, stdout, stderr) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Query failed: {}",
                if stderr.is_empty() { &stdout } else { &stderr }
            )));
        }

        // Parse results
        let lines: Vec<&str> = stdout.lines().collect();
        let rows: Vec<Vec<String>> = lines
            .iter()
            .filter(|l| !l.is_empty())
            .map(|line| line.split('|').map(|s| s.to_string()).collect())
            .collect();

        let rowcount = rows.len() as i64;

        Ok(QueryResult {
            rowcount,
            rows,
            columns: Vec::new(), // psql -t doesn't give column names easily
            status: if success { "SUCCESS" } else { "FAILED" }.to_string(),
        })
    }

    /// Execute a SQL script file
    async fn execute_script(
        connection: &dyn Connection,
        config: &QueryConfig,
        context: &ModuleContext,
    ) -> ModuleResult<QueryResult> {
        let script_path = config.path_to_script.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("path_to_script is required".to_string())
        })?;

        // Build psql command
        let mut psql_opts = Vec::new();

        // Connection options
        psql_opts.push(config.conn.build_psql_args(&config.db));

        // Set search path if specified
        let mut preamble = String::new();
        if let Some(ref search_path) = config.search_path {
            preamble = format!("SET search_path TO {};", search_path);
        }

        // Execute options
        if config.as_single_query {
            psql_opts.push("-1".to_string()); // Single transaction
        }

        // Build full command
        let cmd = if preamble.is_empty() {
            format!(
                "psql {} -f {}",
                psql_opts.join(" "),
                shell_escape(script_path)
            )
        } else {
            format!(
                "psql {} -c \"{}\" -f {}",
                psql_opts.join(" "),
                preamble,
                shell_escape(script_path)
            )
        };

        let (success, stdout, stderr) =
            Self::execute_command(connection, &cmd, context, config.conn.build_env_vars()).await?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Script execution failed: {}",
                if stderr.is_empty() { &stdout } else { &stderr }
            )));
        }

        Ok(QueryResult {
            rowcount: 0, // Can't easily count affected rows from script
            rows: Vec::new(),
            columns: Vec::new(),
            status: "SUCCESS".to_string(),
        })
    }

    /// Execute the module with async connection
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let config = QueryConfig::from_params(params)?;

        // Determine if this is a modification query
        let is_modification = if let Some(sql) = config.get_sql() {
            Self::is_modification_query(sql)
        } else {
            // Scripts are assumed to modify data
            true
        };

        // In check mode, don't execute modification queries
        if context.check_mode && is_modification {
            if let Some(sql) = config.get_sql() {
                return Ok(ModuleOutput::changed(format!(
                    "Would execute query on database '{}'",
                    config.db
                ))
                .with_data("query", serde_json::json!(sql)));
            } else {
                return Ok(ModuleOutput::changed(format!(
                    "Would execute script '{}' on database '{}'",
                    config.path_to_script.as_deref().unwrap_or(""),
                    config.db
                )));
            }
        }

        // Execute query or script
        let result = if let Some(sql) = config.get_sql() {
            Self::execute_query(connection.as_ref(), &config, sql, context).await?
        } else {
            Self::execute_script(connection.as_ref(), &config, context).await?
        };

        // Build output
        let mut output = if is_modification {
            ModuleOutput::changed(format!(
                "Query executed successfully, {} rows affected",
                result.rowcount
            ))
        } else {
            ModuleOutput::ok(format!("Query returned {} rows", result.rowcount))
        };

        output = output
            .with_data("rowcount", serde_json::json!(result.rowcount))
            .with_data("status", serde_json::json!(result.status));

        // Include results for SELECT queries (limit to prevent huge outputs)
        if !result.rows.is_empty() && result.rows.len() <= 1000 {
            output = output.with_data("rows", serde_json::json!(result.rows));
        }

        if !result.columns.is_empty() {
            output = output.with_data("columns", serde_json::json!(result.columns));
        }

        Ok(output)
    }
}

impl Module for PostgresqlQueryModule {
    fn name(&self) -> &'static str {
        "postgresql_query"
    }

    fn description(&self) -> &'static str {
        "Execute PostgreSQL queries and scripts"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Queries can run in parallel unless they modify data
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        &["db"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let has_query = params.get("query").is_some();
        let has_script = params.get("path_to_script").is_some();

        if !has_query && !has_script {
            return Err(ModuleError::MissingParameter(
                "Either 'query' or 'path_to_script' must be provided".to_string(),
            ));
        }

        if has_query && has_script {
            return Err(ModuleError::InvalidParameter(
                "'query' and 'path_to_script' are mutually exclusive".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.clone().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available for postgresql_query module execution".to_string(),
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
    fn test_positional_arg_substitution() {
        let query = "SELECT * FROM users WHERE id = $1 AND name = $2";
        let args = vec!["42".to_string(), "John".to_string()];
        let result = PostgresqlQueryModule::substitute_positional_args(query, &args);
        assert_eq!(
            result,
            "SELECT * FROM users WHERE id = '42' AND name = 'John'"
        );
    }

    #[test]
    fn test_named_arg_substitution() {
        let query = "SELECT * FROM users WHERE id = :id AND name = %(name)";
        let mut args = HashMap::new();
        args.insert("id".to_string(), "42".to_string());
        args.insert("name".to_string(), "John".to_string());
        let result = PostgresqlQueryModule::substitute_named_args(query, &args);
        assert!(result.contains("'42'"));
        assert!(result.contains("'John'"));
    }

    #[test]
    fn test_sql_injection_prevention() {
        let query = "SELECT * FROM users WHERE name = $1";
        let args = vec!["'; DROP TABLE users; --".to_string()];
        let result = PostgresqlQueryModule::substitute_positional_args(query, &args);
        assert!(result.contains("''; DROP TABLE users; --'")); // Escaped properly
    }

    #[test]
    fn test_is_modification_query() {
        assert!(PostgresqlQueryModule::is_modification_query("INSERT INTO users VALUES (1)"));
        assert!(PostgresqlQueryModule::is_modification_query("UPDATE users SET name = 'foo'"));
        assert!(PostgresqlQueryModule::is_modification_query("DELETE FROM users"));
        assert!(PostgresqlQueryModule::is_modification_query("CREATE TABLE foo (id INT)"));
        assert!(PostgresqlQueryModule::is_modification_query("DROP TABLE foo"));
        assert!(!PostgresqlQueryModule::is_modification_query("SELECT * FROM users"));
        assert!(!PostgresqlQueryModule::is_modification_query("   SELECT * FROM users"));
    }

    #[test]
    fn test_module_metadata() {
        let module = PostgresqlQueryModule;
        assert_eq!(module.name(), "postgresql_query");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["db"]);
    }

    #[test]
    fn test_validate_params_no_query_or_script() {
        let module = PostgresqlQueryModule;
        let mut params = HashMap::new();
        params.insert("db".to_string(), serde_json::json!("mydb"));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_params_both_query_and_script() {
        let module = PostgresqlQueryModule;
        let mut params = HashMap::new();
        params.insert("db".to_string(), serde_json::json!("mydb"));
        params.insert("query".to_string(), serde_json::json!("SELECT 1"));
        params.insert("path_to_script".to_string(), serde_json::json!("/tmp/script.sql"));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }
}
