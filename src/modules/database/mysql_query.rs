//! MySQL query execution module
//!
//! This module provides functionality for executing arbitrary SQL queries
//! on MySQL databases.
//!
//! # Parameters
//!
//! - `query` (required): SQL query to execute (single query or list)
//! - `db`: Database to execute the query against
//! - `single_transaction`: Execute all queries in a single transaction (default: false)
//! - `login_host`: MySQL server host (default: localhost)
//! - `login_port`: MySQL server port (default: 3306)
//! - `login_user`: MySQL username for authentication
//! - `login_password`: MySQL password for authentication
//!
//! # Security
//!
//! This module executes arbitrary SQL and should be used with caution.
//! Always use parameterized queries when possible and validate input.
//!
//! # Example
//!
//! ```yaml
//! # Execute a simple query
//! - mysql_query:
//!     db: myapp
//!     query: "SELECT COUNT(*) FROM users"
//!   register: user_count
//!
//! # Execute multiple queries in a transaction
//! - mysql_query:
//!     db: myapp
//!     query:
//!       - "INSERT INTO audit_log (action) VALUES ('start')"
//!       - "UPDATE users SET last_seen = NOW()"
//!       - "INSERT INTO audit_log (action) VALUES ('end')"
//!     single_transaction: true
//!
//! # Create a table
//! - mysql_query:
//!     db: myapp
//!     query: |
//!       CREATE TABLE IF NOT EXISTS users (
//!         id INT PRIMARY KEY AUTO_INCREMENT,
//!         username VARCHAR(255) NOT NULL,
//!         created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
//!       )
//! ```

use super::pool::global_pool_manager;
use super::{extract_connection_params, MysqlConnectionParams};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use base64::Engine;
use sqlx::mysql::MySqlRow;
use sqlx::{Column, Row, TypeInfo};
use std::collections::HashMap;
use tokio::runtime::Handle;

/// Query execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    /// Execute as a regular query
    Execute,
    /// Execute and return results
    Fetch,
}

/// Result of a query execution
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Number of rows affected
    pub rows_affected: u64,
    /// Last insert ID (if applicable)
    pub last_insert_id: Option<u64>,
    /// Query results as rows of key-value pairs
    pub rows: Vec<HashMap<String, serde_json::Value>>,
    /// Column names in order
    pub columns: Vec<String>,
}

/// Module for MySQL query execution
pub struct MysqlQueryModule;

impl MysqlQueryModule {
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

    /// Convert a MySQL row to a HashMap of JSON values
    fn row_to_map(row: &MySqlRow) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();

        for column in row.columns() {
            let name = column.name().to_string();
            let type_info = column.type_info();
            let type_name = type_info.name();

            let value: serde_json::Value = match type_name {
                "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" => row
                    .try_get::<i64, _>(name.as_str())
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null),
                "INT UNSIGNED" | "BIGINT UNSIGNED" | "SMALLINT UNSIGNED" | "TINYINT UNSIGNED" => {
                    row.try_get::<u64, _>(name.as_str())
                        .map(serde_json::Value::from)
                        .unwrap_or(serde_json::Value::Null)
                }
                "FLOAT" | "DOUBLE" | "DECIMAL" => row
                    .try_get::<f64, _>(name.as_str())
                    .map(|v| serde_json::json!(v))
                    .unwrap_or(serde_json::Value::Null),
                "BOOLEAN" | "BOOL" => row
                    .try_get::<bool, _>(name.as_str())
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null),
                "VARCHAR" | "CHAR" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" | "TINYTEXT" => row
                    .try_get::<String, _>(name.as_str())
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null),
                "DATETIME" | "TIMESTAMP" | "DATE" | "TIME" => row
                    .try_get::<String, _>(name.as_str())
                    .map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null),
                "BLOB" | "MEDIUMBLOB" | "LONGBLOB" | "TINYBLOB" | "BINARY" | "VARBINARY" => {
                    // For binary data, encode as base64
                    row.try_get::<Vec<u8>, _>(name.as_str())
                        .map(|v| {
                            serde_json::json!(base64::engine::general_purpose::STANDARD.encode(&v))
                        })
                        .unwrap_or(serde_json::Value::Null)
                }
                "JSON" => row
                    .try_get::<serde_json::Value, _>(name.as_str())
                    .unwrap_or(serde_json::Value::Null),
                _ => {
                    // Fallback: try as string
                    row.try_get::<String, _>(name.as_str())
                        .map(serde_json::Value::from)
                        .unwrap_or(serde_json::Value::Null)
                }
            };

            map.insert(name, value);
        }

        map
    }

    /// Execute a single query and return results
    async fn execute_query(
        conn_params: &MysqlConnectionParams,
        database: Option<&str>,
        query: &str,
    ) -> ModuleResult<QueryResult> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(database))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        // Determine if this is a SELECT query
        let is_select = query.trim().to_uppercase().starts_with("SELECT")
            || query.trim().to_uppercase().starts_with("SHOW")
            || query.trim().to_uppercase().starts_with("DESCRIBE")
            || query.trim().to_uppercase().starts_with("EXPLAIN");

        if is_select {
            let rows = pool
                .fetch_all(query)
                .await
                .map_err(|e| ModuleError::ExecutionFailed(format!("Query failed: {}", e)))?;

            let columns: Vec<String> = if !rows.is_empty() {
                rows[0]
                    .columns()
                    .iter()
                    .map(|c| c.name().to_string())
                    .collect()
            } else {
                Vec::new()
            };

            let result_rows: Vec<HashMap<String, serde_json::Value>> =
                rows.iter().map(Self::row_to_map).collect();

            Ok(QueryResult {
                rows_affected: result_rows.len() as u64,
                last_insert_id: None,
                rows: result_rows,
                columns,
            })
        } else {
            let result = pool
                .execute(query)
                .await
                .map_err(|e| ModuleError::ExecutionFailed(format!("Query failed: {}", e)))?;

            Ok(QueryResult {
                rows_affected: result,
                last_insert_id: None,
                rows: Vec::new(),
                columns: Vec::new(),
            })
        }
    }

    /// Execute multiple queries in a transaction
    async fn execute_queries_transaction(
        conn_params: &MysqlConnectionParams,
        database: Option<&str>,
        queries: &[String],
    ) -> ModuleResult<Vec<QueryResult>> {
        let pool = global_pool_manager()
            .get_or_create(&conn_params.to_connection_url(database))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

        // Start transaction
        pool.execute("START TRANSACTION").await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to start transaction: {}", e))
        })?;

        let mut results = Vec::new();

        for query in queries {
            match Self::execute_single_in_pool(pool.as_ref(), query).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    // Rollback on error
                    let _ = pool.execute("ROLLBACK").await;
                    return Err(e);
                }
            }
        }

        // Commit transaction
        pool.execute("COMMIT").await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(results)
    }

    /// Execute a single query using an existing pool
    async fn execute_single_in_pool(
        pool: &super::pool::DatabasePool,
        query: &str,
    ) -> ModuleResult<QueryResult> {
        let is_select = query.trim().to_uppercase().starts_with("SELECT")
            || query.trim().to_uppercase().starts_with("SHOW")
            || query.trim().to_uppercase().starts_with("DESCRIBE")
            || query.trim().to_uppercase().starts_with("EXPLAIN");

        if is_select {
            let rows = pool
                .fetch_all(query)
                .await
                .map_err(|e| ModuleError::ExecutionFailed(format!("Query failed: {}", e)))?;

            let columns: Vec<String> = if !rows.is_empty() {
                rows[0]
                    .columns()
                    .iter()
                    .map(|c| c.name().to_string())
                    .collect()
            } else {
                Vec::new()
            };

            let result_rows: Vec<HashMap<String, serde_json::Value>> =
                rows.iter().map(Self::row_to_map).collect();

            Ok(QueryResult {
                rows_affected: result_rows.len() as u64,
                last_insert_id: None,
                rows: result_rows,
                columns,
            })
        } else {
            let affected = pool
                .execute(query)
                .await
                .map_err(|e| ModuleError::ExecutionFailed(format!("Query failed: {}", e)))?;

            Ok(QueryResult {
                rows_affected: affected,
                last_insert_id: None,
                rows: Vec::new(),
                columns: Vec::new(),
            })
        }
    }

    /// Parse query parameter (can be string or array)
    fn parse_queries(params: &ModuleParams) -> ModuleResult<Vec<String>> {
        match params.get("query") {
            Some(serde_json::Value::String(s)) => Ok(vec![s.clone()]),
            Some(serde_json::Value::Array(arr)) => {
                let mut queries = Vec::new();
                for item in arr {
                    match item {
                        serde_json::Value::String(s) => queries.push(s.clone()),
                        _ => {
                            return Err(ModuleError::InvalidParameter(
                                "Query array items must be strings".to_string(),
                            ))
                        }
                    }
                }
                Ok(queries)
            }
            Some(_) => Err(ModuleError::InvalidParameter(
                "Query must be a string or array of strings".to_string(),
            )),
            None => Err(ModuleError::MissingParameter("query".to_string())),
        }
    }
}

impl Module for MysqlQueryModule {
    fn name(&self) -> &'static str {
        "mysql_query"
    }

    fn description(&self) -> &'static str {
        "Execute MySQL queries"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Query operations should be serialized per host to avoid transaction conflicts
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        &["query"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate query format
        Self::parse_queries(params)?;
        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let queries = Self::parse_queries(params)?;
        let database = params.get_string("db")?;
        let single_transaction = params.get_bool_or("single_transaction", false);

        let conn_params = extract_connection_params(params)?;

        if queries.is_empty() {
            return Ok(ModuleOutput::ok("No queries to execute"));
        }

        if context.check_mode {
            let msg = if queries.len() == 1 {
                format!("Would execute query: {}", &queries[0])
            } else {
                format!("Would execute {} queries", queries.len())
            };
            return Ok(ModuleOutput::changed(msg));
        }

        Self::execute_async(async {
            let results = if single_transaction && queries.len() > 1 {
                Self::execute_queries_transaction(&conn_params, database.as_deref(), &queries)
                    .await?
            } else {
                let mut results = Vec::new();
                for query in &queries {
                    let result =
                        Self::execute_query(&conn_params, database.as_deref(), query).await?;
                    results.push(result);
                }
                results
            };

            let total_affected: u64 = results.iter().map(|r| r.rows_affected).sum();
            let total_rows: usize = results.iter().map(|r| r.rows.len()).sum();

            // Prepare output
            let query_results: Vec<serde_json::Value> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "rows_affected": r.rows_affected,
                        "row_count": r.rows.len(),
                        "columns": r.columns,
                        "rows": r.rows
                    })
                })
                .collect();

            let msg = if queries.len() == 1 {
                if results[0].rows.is_empty() {
                    format!("Query executed, {} rows affected", total_affected)
                } else {
                    format!("Query returned {} rows", total_rows)
                }
            } else {
                format!(
                    "Executed {} queries, {} total rows affected",
                    queries.len(),
                    total_affected
                )
            };

            let mut output = if total_affected > 0 || total_rows > 0 {
                ModuleOutput::changed(msg)
            } else {
                ModuleOutput::ok(msg)
            };

            output = output
                .with_data("query_result", serde_json::json!(query_results))
                .with_data("rowcount", serde_json::json!(total_rows))
                .with_data("rows_affected", serde_json::json!(total_affected));

            // For single SELECT query, also add rows directly for convenience
            if queries.len() == 1 && !results[0].rows.is_empty() {
                output = output.with_data("rows", serde_json::json!(results[0].rows));
            }

            Ok(output)
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
    fn test_parse_queries_string() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("query".to_string(), serde_json::json!("SELECT 1"));

        let queries = MysqlQueryModule::parse_queries(&params).unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0], "SELECT 1");
    }

    #[test]
    fn test_parse_queries_array() {
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "query".to_string(),
            serde_json::json!(["SELECT 1", "SELECT 2"]),
        );

        let queries = MysqlQueryModule::parse_queries(&params).unwrap();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0], "SELECT 1");
        assert_eq!(queries[1], "SELECT 2");
    }

    #[test]
    fn test_parse_queries_missing() {
        let params: ModuleParams = HashMap::new();
        assert!(MysqlQueryModule::parse_queries(&params).is_err());
    }

    #[test]
    fn test_module_name() {
        let module = MysqlQueryModule;
        assert_eq!(module.name(), "mysql_query");
    }

    #[test]
    fn test_module_classification() {
        let module = MysqlQueryModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_required_params() {
        let module = MysqlQueryModule;
        assert_eq!(module.required_params(), &["query"]);
    }
}
