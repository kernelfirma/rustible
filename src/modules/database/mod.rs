//! Database modules for Rustible
//!
//! This module provides database management capabilities including:
//!
//! ## PostgreSQL Modules (always available)
//! - `postgresql_db`: PostgreSQL database management (create, drop, backup, restore)
//! - `postgresql_user`: PostgreSQL user/role management
//! - `postgresql_query`: Execute PostgreSQL queries and scripts
//!
//! PostgreSQL modules use CLI tools (psql, pg_dump, pg_restore) via SSH and do not
//! require any database driver dependencies.
//!
//! ## MySQL Modules (requires `database` feature)
//! - `mysql_db`: MySQL database management (create, drop, modify encoding/collation)
//! - `mysql_user`: MySQL user/role management with privilege grants
//! - `mysql_query`: Execute MySQL queries and scripts
//!
//! MySQL modules require the `sqlx` crate and are only available when the
//! `database` feature is enabled.
//!
//! ## Connection Pooling (requires `database` feature)
//!
//! When the `database` feature is enabled, MySQL operations utilize connection
//! pooling for efficient database connection management. The pool module provides:
//! - Configurable pool sizes (min/max connections)
//! - Connection timeout and idle timeout handling
//! - Connection health checking
//! - Global pool manager for connection reuse
//!
//! # Example Usage
//!
//! ```yaml
//! # PostgreSQL database operations (always available)
//! - postgresql_db:
//!     name: myapp_db
//!     state: present
//!     encoding: UTF8
//!     login_user: postgres
//!
//! # MySQL database operations (requires `database` feature)
//! - mysql_db:
//!     name: myapp_db
//!     state: present
//!     encoding: utf8mb4
//!     collation: utf8mb4_unicode_ci
//!     login_user: root
//!     login_password: "{{ mysql_root_password }}"
//! ```

// PostgreSQL modules - always available (use CLI tools, no sqlx needed)
pub mod postgresql_db;
pub mod postgresql_query;
pub mod postgresql_user;

// Re-export PostgreSQL modules
pub use postgresql_db::PostgresqlDbModule;
pub use postgresql_query::PostgresqlQueryModule;
pub use postgresql_user::PostgresqlUserModule;

// MySQL modules - require sqlx (database feature)
#[cfg(feature = "database")]
pub mod mysql_db;
#[cfg(feature = "database")]
pub mod mysql_query;
#[cfg(feature = "database")]
pub mod mysql_user;
#[cfg(feature = "database")]
pub mod pool;

// Re-export MySQL modules when available
#[cfg(feature = "database")]
pub use mysql_db::MysqlDbModule;
#[cfg(feature = "database")]
pub use mysql_query::MysqlQueryModule;
#[cfg(feature = "database")]
pub use mysql_user::MysqlUserModule;

// Re-export pool types when available
#[cfg(feature = "database")]
pub use pool::{global_pool_manager, DatabasePool, PoolConfig, PoolManager, PoolStats};

/// Common MySQL connection parameters used across all MySQL modules
/// Only available when the `database` feature is enabled.
#[cfg(feature = "database")]
#[derive(Debug, Clone)]
pub struct MysqlConnectionParams {
    /// MySQL host (default: localhost)
    pub host: String,
    /// MySQL port (default: 3306)
    pub port: u16,
    /// MySQL user for authentication
    pub user: String,
    /// MySQL password for authentication
    pub password: Option<String>,
    /// MySQL socket path (for local connections)
    pub socket: Option<String>,
    /// SSL mode (disabled, preferred, required)
    pub ssl_mode: MysqlSslMode,
    /// Path to SSL CA certificate
    pub ssl_ca: Option<String>,
    /// Path to SSL client certificate
    pub ssl_cert: Option<String>,
    /// Path to SSL client key
    pub ssl_key: Option<String>,
    /// Connection timeout in seconds
    pub connect_timeout: u64,
}

#[cfg(feature = "database")]
impl Default for MysqlConnectionParams {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: None,
            socket: None,
            ssl_mode: MysqlSslMode::Preferred,
            ssl_ca: None,
            ssl_cert: None,
            ssl_key: None,
            connect_timeout: 30,
        }
    }
}

#[cfg(feature = "database")]
impl MysqlConnectionParams {
    /// Build a connection URL for sqlx
    pub fn to_connection_url(&self, database: Option<&str>) -> String {
        let auth = if let Some(ref password) = self.password {
            format!("{}:{}", self.user, urlencoding::encode(password))
        } else {
            self.user.clone()
        };

        let db_part = database.map(|d| format!("/{}", d)).unwrap_or_default();

        // Build query parameters
        let mut params = Vec::new();

        match self.ssl_mode {
            MysqlSslMode::Disabled => params.push("ssl-mode=disabled".to_string()),
            MysqlSslMode::Preferred => params.push("ssl-mode=preferred".to_string()),
            MysqlSslMode::Required => params.push("ssl-mode=required".to_string()),
        }

        if let Some(ref ca) = self.ssl_ca {
            params.push(format!("ssl-ca={}", urlencoding::encode(ca)));
        }
        if let Some(ref cert) = self.ssl_cert {
            params.push(format!("ssl-cert={}", urlencoding::encode(cert)));
        }
        if let Some(ref key) = self.ssl_key {
            params.push(format!("ssl-key={}", urlencoding::encode(key)));
        }

        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };

        if let Some(ref socket) = self.socket {
            // Unix socket connection
            format!(
                "mysql://{}@localhost{}{}?socket={}",
                auth,
                db_part,
                if query.is_empty() { "?" } else { "&" },
                urlencoding::encode(socket)
            )
        } else {
            // TCP connection
            format!(
                "mysql://{}@{}:{}{}{}",
                auth, self.host, self.port, db_part, query
            )
        }
    }
}

/// SSL connection mode for MySQL
/// Only available when the `database` feature is enabled.
#[cfg(feature = "database")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MysqlSslMode {
    /// SSL is disabled
    Disabled,
    /// SSL is preferred but not required (default)
    #[default]
    Preferred,
    /// SSL is required
    Required,
}

#[cfg(feature = "database")]
impl MysqlSslMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "disabled" | "disable" | "false" | "no" => Some(MysqlSslMode::Disabled),
            "preferred" | "prefer" => Some(MysqlSslMode::Preferred),
            "required" | "require" | "true" | "yes" => Some(MysqlSslMode::Required),
            _ => None,
        }
    }
}

/// Common error types for database operations
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Query execution failed: {0}")]
    QueryFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Database not found: {0}")]
    DatabaseNotFound(String),

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Pool error: {0}")]
    PoolError(String),

    #[error("Timeout: {0}")]
    Timeout(String),
}

/// Helper function to extract MySQL connection parameters from module params
/// Only available when the `database` feature is enabled.
#[cfg(feature = "database")]
pub fn extract_connection_params(
    params: &super::ModuleParams,
) -> Result<MysqlConnectionParams, super::ModuleError> {
    use super::ParamExt;

    let mut conn = MysqlConnectionParams::default();

    if let Some(host) = params.get_string("login_host")? {
        conn.host = host;
    }

    if let Some(port) = params.get_u32("login_port")? {
        conn.port = port as u16;
    }

    if let Some(user) = params.get_string("login_user")? {
        conn.user = user;
    }

    conn.password = params.get_string("login_password")?;
    conn.socket = params.get_string("login_unix_socket")?;

    if let Some(ssl_mode_str) = params.get_string("ssl_mode")? {
        conn.ssl_mode = MysqlSslMode::from_str(&ssl_mode_str).unwrap_or(MysqlSslMode::Preferred);
    }

    conn.ssl_ca = params.get_string("ssl_ca")?;
    conn.ssl_cert = params.get_string("ssl_cert")?;
    conn.ssl_key = params.get_string("ssl_key")?;

    if let Some(timeout) = params.get_u32("connect_timeout")? {
        conn.connect_timeout = timeout as u64;
    }

    Ok(conn)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "database")]
    use super::*;

    #[cfg(feature = "database")]
    #[test]
    fn test_connection_url_basic() {
        let params = MysqlConnectionParams {
            host: "localhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: Some("secret".to_string()),
            socket: None,
            ssl_mode: MysqlSslMode::Preferred,
            ssl_ca: None,
            ssl_cert: None,
            ssl_key: None,
            connect_timeout: 30,
        };

        let url = params.to_connection_url(Some("testdb"));
        assert!(url.starts_with("mysql://root:secret@localhost:3306/testdb"));
    }

    #[cfg(feature = "database")]
    #[test]
    fn test_connection_url_with_special_chars() {
        let params = MysqlConnectionParams {
            host: "localhost".to_string(),
            port: 3306,
            user: "admin".to_string(),
            password: Some("p@ss!word#123".to_string()),
            socket: None,
            ssl_mode: MysqlSslMode::Disabled,
            ssl_ca: None,
            ssl_cert: None,
            ssl_key: None,
            connect_timeout: 30,
        };

        let url = params.to_connection_url(None);
        // Password should be URL encoded
        assert!(url.contains("p%40ss%21word%23123"));
    }

    #[cfg(feature = "database")]
    #[test]
    fn test_ssl_mode_parsing() {
        assert_eq!(
            MysqlSslMode::from_str("disabled"),
            Some(MysqlSslMode::Disabled)
        );
        assert_eq!(
            MysqlSslMode::from_str("DISABLED"),
            Some(MysqlSslMode::Disabled)
        );
        assert_eq!(
            MysqlSslMode::from_str("preferred"),
            Some(MysqlSslMode::Preferred)
        );
        assert_eq!(
            MysqlSslMode::from_str("required"),
            Some(MysqlSslMode::Required)
        );
        assert_eq!(MysqlSslMode::from_str("invalid"), None);
    }
}
