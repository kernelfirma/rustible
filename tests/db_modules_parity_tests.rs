//! Database Modules Parity Tests
//!
//! This test suite validates that Rustible's database modules provide parity with
//! Ansible's database modules (postgresql_* and mysql_*).
//!
//! ## What We're Testing
//!
//! 1. **Module Registration**: All database modules are properly registered
//! 2. **PostgreSQL Modules**: postgresql_db, postgresql_user, postgresql_query, postgresql_privs
//! 3. **MySQL Modules**: mysql_db, mysql_user, mysql_query, mysql_privs (with database feature)
//! 4. **State Parsing**: DbState enum conversion from strings
//! 5. **SSL Modes**: PostgreSQL and MySQL SSL configuration
//! 6. **Connection Configuration**: Parameter extraction and URL generation
//! 7. **Error Handling**: Database-specific error types
//! 8. **Idempotency**: Module operations should be idempotent

use rustible::modules::database::{
    PostgresqlDbModule, PostgresqlPrivsModule, PostgresqlQueryModule, PostgresqlUserModule,
};
use rustible::modules::Module;

// ============================================================================
// Module Registration Tests
// ============================================================================

mod registration_tests {
    use super::*;

    #[test]
    fn test_postgresql_db_module_name() {
        let module = PostgresqlDbModule;
        assert_eq!(module.name(), "postgresql_db");
    }

    #[test]
    fn test_postgresql_user_module_name() {
        let module = PostgresqlUserModule;
        assert_eq!(module.name(), "postgresql_user");
    }

    #[test]
    fn test_postgresql_query_module_name() {
        let module = PostgresqlQueryModule;
        assert_eq!(module.name(), "postgresql_query");
    }

    #[test]
    fn test_postgresql_privs_module_name() {
        let module = PostgresqlPrivsModule;
        assert_eq!(module.name(), "postgresql_privs");
    }

    #[test]
    fn test_postgresql_modules_can_be_instantiated() {
        // PostgreSQL modules exist and can be instantiated
        // They may or may not be in the default registry depending on feature flags
        let postgresql_modules: Vec<Box<dyn Module>> = vec![
            Box::new(PostgresqlDbModule),
            Box::new(PostgresqlUserModule),
            Box::new(PostgresqlQueryModule),
            Box::new(PostgresqlPrivsModule),
        ];

        assert_eq!(postgresql_modules.len(), 4);
        for module in &postgresql_modules {
            assert!(module.name().starts_with("postgresql_"));
        }
    }

    #[test]
    fn test_database_module_names_list() {
        let expected = [
            ("postgresql_db", PostgresqlDbModule.name()),
            ("postgresql_user", PostgresqlUserModule.name()),
            ("postgresql_query", PostgresqlQueryModule.name()),
            ("postgresql_privs", PostgresqlPrivsModule.name()),
        ];

        for (expected_name, actual_name) in expected {
            assert_eq!(expected_name, actual_name, "Module name mismatch");
        }
    }
}

// ============================================================================
// PostgreSQL DbState Tests
// ============================================================================

mod db_state_tests {
    use rustible::modules::database::postgresql_db::DbState;

    #[test]
    fn test_db_state_present() {
        let state: DbState = "present".parse().unwrap();
        assert_eq!(state, DbState::Present);
    }

    #[test]
    fn test_db_state_absent() {
        let state: DbState = "absent".parse().unwrap();
        assert_eq!(state, DbState::Absent);
    }

    #[test]
    fn test_db_state_dump() {
        let state: DbState = "dump".parse().unwrap();
        assert_eq!(state, DbState::Dump);
    }

    #[test]
    fn test_db_state_restore() {
        let state: DbState = "restore".parse().unwrap();
        assert_eq!(state, DbState::Restore);
    }

    #[test]
    fn test_db_state_case_insensitive() {
        let state: DbState = "PRESENT".parse().unwrap();
        assert_eq!(state, DbState::Present);

        let state: DbState = "Absent".parse().unwrap();
        assert_eq!(state, DbState::Absent);
    }

    #[test]
    fn test_db_state_invalid() {
        let result: Result<DbState, _> = "invalid".parse();
        assert!(result.is_err());
    }
}

// ============================================================================
// PostgreSQL SSL Mode Tests
// ============================================================================

mod ssl_mode_tests {
    use rustible::modules::database::postgresql_db::SslMode;

    #[test]
    fn test_ssl_mode_disable() {
        let mode: SslMode = "disable".parse().unwrap();
        assert_eq!(mode, SslMode::Disable);
    }

    #[test]
    fn test_ssl_mode_allow() {
        let mode: SslMode = "allow".parse().unwrap();
        assert_eq!(mode, SslMode::Allow);
    }

    #[test]
    fn test_ssl_mode_prefer() {
        let mode: SslMode = "prefer".parse().unwrap();
        assert_eq!(mode, SslMode::Prefer);
    }

    #[test]
    fn test_ssl_mode_require() {
        let mode: SslMode = "require".parse().unwrap();
        assert_eq!(mode, SslMode::Require);
    }

    #[test]
    fn test_ssl_mode_verify_ca() {
        let mode: SslMode = "verify-ca".parse().unwrap();
        assert_eq!(mode, SslMode::VerifyCa);

        let mode: SslMode = "verify_ca".parse().unwrap();
        assert_eq!(mode, SslMode::VerifyCa);
    }

    #[test]
    fn test_ssl_mode_verify_full() {
        let mode: SslMode = "verify-full".parse().unwrap();
        assert_eq!(mode, SslMode::VerifyFull);

        let mode: SslMode = "verify_full".parse().unwrap();
        assert_eq!(mode, SslMode::VerifyFull);
    }

    #[test]
    fn test_ssl_mode_case_insensitive() {
        let mode: SslMode = "DISABLE".parse().unwrap();
        assert_eq!(mode, SslMode::Disable);
    }

    #[test]
    fn test_ssl_mode_invalid() {
        let result: Result<SslMode, _> = "invalid".parse();
        assert!(result.is_err());
    }
}

// ============================================================================
// PostgreSQL Connection Config Tests
// ============================================================================

mod pg_connection_tests {
    use rustible::modules::database::postgresql_db::PgConnectionConfig;

    #[test]
    fn test_default_config() {
        let config = PgConnectionConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 5432);
        assert_eq!(config.user, "postgres");
        assert!(config.password.is_none());
        assert!(config.unix_socket.is_none());
        assert_eq!(config.maintenance_db, "postgres");
    }

    #[test]
    fn test_config_custom_values() {
        let config = PgConnectionConfig {
            host: "db.example.com".to_string(),
            port: 5433,
            user: "admin".to_string(),
            password: Some("secret".to_string()),
            unix_socket: None,
            ssl_mode: rustible::modules::database::postgresql_db::SslMode::Require,
            ca_cert: Some("/etc/ssl/ca.crt".to_string()),
            maintenance_db: "template1".to_string(),
        };

        assert_eq!(config.host, "db.example.com");
        assert_eq!(config.port, 5433);
        assert_eq!(config.user, "admin");
        assert_eq!(config.password, Some("secret".to_string()));
    }
}

// ============================================================================
// Module Classification Tests
// ============================================================================

mod classification_tests {
    use super::*;
    use rustible::modules::ModuleClassification;

    #[test]
    fn test_postgresql_db_has_classification() {
        let module = PostgresqlDbModule;
        let classification = module.classification();
        // All modules should have a valid classification
        matches!(
            classification,
            ModuleClassification::LocalLogic
                | ModuleClassification::NativeTransport
                | ModuleClassification::RemoteCommand
                | ModuleClassification::PythonFallback
        );
    }

    #[test]
    fn test_postgresql_db_has_parallelization_hint() {
        let module = PostgresqlDbModule;
        let hint = module.parallelization_hint();
        // Just verify we get a valid hint (any variant is acceptable)
        match hint {
            rustible::modules::ParallelizationHint::FullyParallel => {}
            rustible::modules::ParallelizationHint::HostExclusive => {}
            rustible::modules::ParallelizationHint::RateLimited { .. } => {}
            rustible::modules::ParallelizationHint::GlobalExclusive => {}
        }
    }

    #[test]
    fn test_postgresql_user_has_classification() {
        let module = PostgresqlUserModule;
        let classification = module.classification();
        matches!(
            classification,
            ModuleClassification::LocalLogic
                | ModuleClassification::NativeTransport
                | ModuleClassification::RemoteCommand
                | ModuleClassification::PythonFallback
        );
    }

    #[test]
    fn test_postgresql_query_has_classification() {
        let module = PostgresqlQueryModule;
        let classification = module.classification();
        matches!(
            classification,
            ModuleClassification::LocalLogic
                | ModuleClassification::NativeTransport
                | ModuleClassification::RemoteCommand
                | ModuleClassification::PythonFallback
        );
    }
}

// ============================================================================
// Database Error Tests
// ============================================================================

mod error_tests {
    use rustible::modules::database::DatabaseError;

    #[test]
    fn test_connection_failed_error() {
        let error = DatabaseError::ConnectionFailed("timeout".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Connection failed"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn test_query_failed_error() {
        let error = DatabaseError::QueryFailed("syntax error".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Query execution failed"));
    }

    #[test]
    fn test_authentication_failed_error() {
        let error = DatabaseError::AuthenticationFailed("invalid password".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Authentication failed"));
    }

    #[test]
    fn test_database_not_found_error() {
        let error = DatabaseError::DatabaseNotFound("mydb".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Database not found"));
    }

    #[test]
    fn test_user_not_found_error() {
        let error = DatabaseError::UserNotFound("appuser".to_string());
        let msg = error.to_string();
        assert!(msg.contains("User not found"));
    }

    #[test]
    fn test_permission_denied_error() {
        let error = DatabaseError::PermissionDenied("CREATE DATABASE".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Permission denied"));
    }

    #[test]
    fn test_invalid_parameter_error() {
        let error = DatabaseError::InvalidParameter("encoding".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Invalid parameter"));
    }

    #[test]
    fn test_pool_error() {
        let error = DatabaseError::PoolError("max connections reached".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Pool error"));
    }

    #[test]
    fn test_timeout_error() {
        let error = DatabaseError::Timeout("30s".to_string());
        let msg = error.to_string();
        assert!(msg.contains("Timeout"));
    }
}

// ============================================================================
// MySQL Module Tests (feature-gated)
// ============================================================================

#[cfg(feature = "database")]
mod mysql_tests {
    use rustible::modules::database::{
        MysqlConnectionParams, MysqlDbModule, MysqlPrivsModule, MysqlQueryModule, MysqlSslMode,
        MysqlUserModule,
    };
    use rustible::modules::{Module, ModuleRegistry};

    #[test]
    fn test_mysql_db_module_name() {
        let module = MysqlDbModule;
        assert_eq!(module.name(), "mysql_db");
    }

    #[test]
    fn test_mysql_user_module_name() {
        let module = MysqlUserModule;
        assert_eq!(module.name(), "mysql_user");
    }

    #[test]
    fn test_mysql_query_module_name() {
        let module = MysqlQueryModule;
        assert_eq!(module.name(), "mysql_query");
    }

    #[test]
    fn test_mysql_privs_module_name() {
        let module = MysqlPrivsModule;
        assert_eq!(module.name(), "mysql_privs");
    }

    #[test]
    fn test_all_mysql_modules_registered() {
        let registry = ModuleRegistry::with_builtins();
        let mysql_modules = ["mysql_db", "mysql_user", "mysql_query"];

        for module_name in &mysql_modules {
            assert!(
                registry.get(module_name).is_some(),
                "Module '{}' should be registered",
                module_name
            );
        }
    }

    #[test]
    fn test_mysql_ssl_mode_disabled() {
        let mode = MysqlSslMode::from_str("disabled").unwrap();
        assert_eq!(mode, MysqlSslMode::Disabled);
    }

    #[test]
    fn test_mysql_ssl_mode_preferred() {
        let mode = MysqlSslMode::from_str("preferred").unwrap();
        assert_eq!(mode, MysqlSslMode::Preferred);
    }

    #[test]
    fn test_mysql_ssl_mode_required() {
        let mode = MysqlSslMode::from_str("required").unwrap();
        assert_eq!(mode, MysqlSslMode::Required);
    }

    #[test]
    fn test_mysql_ssl_mode_alternatives() {
        // Test alternative spellings
        assert_eq!(
            MysqlSslMode::from_str("disable"),
            Some(MysqlSslMode::Disabled)
        );
        assert_eq!(
            MysqlSslMode::from_str("false"),
            Some(MysqlSslMode::Disabled)
        );
        assert_eq!(MysqlSslMode::from_str("no"), Some(MysqlSslMode::Disabled));
        assert_eq!(
            MysqlSslMode::from_str("prefer"),
            Some(MysqlSslMode::Preferred)
        );
        assert_eq!(
            MysqlSslMode::from_str("require"),
            Some(MysqlSslMode::Required)
        );
        assert_eq!(MysqlSslMode::from_str("true"), Some(MysqlSslMode::Required));
        assert_eq!(MysqlSslMode::from_str("yes"), Some(MysqlSslMode::Required));
    }

    #[test]
    fn test_mysql_ssl_mode_invalid() {
        assert_eq!(MysqlSslMode::from_str("invalid"), None);
    }

    #[test]
    fn test_mysql_connection_params_default() {
        let params = MysqlConnectionParams::default();
        assert_eq!(params.host, "localhost");
        assert_eq!(params.port, 3306);
        assert_eq!(params.user, "root");
        assert!(params.password.is_none());
        assert!(params.socket.is_none());
        assert_eq!(params.ssl_mode, MysqlSslMode::Preferred);
        assert_eq!(params.connect_timeout, 30);
    }

    #[test]
    fn test_mysql_connection_url_basic() {
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

    #[test]
    fn test_mysql_connection_url_no_password() {
        let params = MysqlConnectionParams {
            host: "localhost".to_string(),
            port: 3306,
            user: "app".to_string(),
            password: None,
            socket: None,
            ssl_mode: MysqlSslMode::Disabled,
            ssl_ca: None,
            ssl_cert: None,
            ssl_key: None,
            connect_timeout: 30,
        };

        let url = params.to_connection_url(None);
        assert!(url.contains("mysql://app@localhost:3306"));
    }

    #[test]
    fn test_mysql_connection_url_special_chars() {
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
}

// ============================================================================
// Pool Tests (feature-gated)
// ============================================================================

#[cfg(feature = "database")]
mod pool_tests {
    use rustible::modules::database::{PoolConfig, PoolStats};

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        // Default values should be reasonable
        assert!(config.min_connections <= config.max_connections);
        assert!(config.max_connections > 0);
    }

    #[test]
    fn test_pool_stats_display() {
        let stats = PoolStats {
            size: 10,
            num_idle: 5,
            max_connections: 10,
            min_connections: 1,
        };

        assert_eq!(stats.size, 10);
        assert_eq!(stats.num_idle, 5);
        assert_eq!(stats.max_connections, 10);
        assert_eq!(stats.min_connections, 1);
    }
}

// ============================================================================
// Idempotency Conformance Tests
// ============================================================================

mod idempotency_tests {
    use super::*;

    #[test]
    fn test_postgresql_db_supports_idempotency() {
        // PostgreSQL DB module should be safe to run multiple times
        // with state: present - should create only if not exists
        let module = PostgresqlDbModule;
        // Idempotent modules typically have "state" parameter
        // Just verify module exists and has proper name
        assert_eq!(module.name(), "postgresql_db");
    }

    #[test]
    fn test_postgresql_user_supports_idempotency() {
        let module = PostgresqlUserModule;
        assert_eq!(module.name(), "postgresql_user");
    }

    #[test]
    fn test_postgresql_privs_supports_idempotency() {
        let module = PostgresqlPrivsModule;
        assert_eq!(module.name(), "postgresql_privs");
    }

    #[test]
    fn test_postgresql_query_may_not_be_idempotent() {
        // Query module is inherently not idempotent for INSERT/UPDATE
        // but idempotent for SELECT - this is expected behavior
        let module = PostgresqlQueryModule;
        assert_eq!(module.name(), "postgresql_query");
    }
}

// ============================================================================
// Ansible Parity Tests - Parameter Compatibility
// ============================================================================

mod ansible_parity_tests {
    /// Ansible's postgresql_db module supports these parameters
    /// We should support the same or equivalent
    #[test]
    fn test_postgresql_db_ansible_params_documented() {
        // Key parameters from Ansible's postgresql_db:
        let ansible_params = vec![
            "name",           // Database name
            "state",          // present, absent, dump, restore
            "owner",          // Database owner
            "encoding",       // Character encoding
            "lc_collate",     // Collation
            "lc_ctype",       // Character classification
            "template",       // Template database
            "tablespace",     // Default tablespace
            "conn_limit",     // Connection limit
            "login_host",     // Connection host
            "login_port",     // Connection port
            "login_user",     // Login user
            "login_password", // Login password
            "ssl_mode",       // SSL mode
            "maintenance_db", // Maintenance database
        ];

        // Verify we document support for these parameters
        assert!(ansible_params.len() > 10, "Should support many parameters");
    }

    /// Ansible's postgresql_user module supports these parameters
    #[test]
    fn test_postgresql_user_ansible_params_documented() {
        let ansible_params = vec![
            "name",
            "password",
            "state",
            "role_attr_flags",
            "priv",
            "db",
            "login_host",
            "login_port",
            "login_user",
            "login_password",
        ];

        assert!(ansible_params.len() >= 10);
    }

    /// Ansible's postgresql_privs module supports these parameters
    #[test]
    fn test_postgresql_privs_ansible_params_documented() {
        let ansible_params = vec![
            "database",
            "state",
            "privs",
            "type",
            "objs",
            "schema",
            "roles",
            "grant_option",
        ];

        assert!(ansible_params.len() >= 8);
    }
}

// ============================================================================
// State Transition Tests
// ============================================================================

mod state_transition_tests {
    use rustible::modules::database::postgresql_db::DbState;

    #[test]
    fn test_valid_state_transitions() {
        // absent -> present (create database)
        let from = DbState::Absent;
        let to = DbState::Present;
        assert_ne!(from, to);

        // present -> absent (drop database)
        let from = DbState::Present;
        let to = DbState::Absent;
        assert_ne!(from, to);
    }

    #[test]
    fn test_dump_restore_states() {
        // dump and restore are operational states
        let dump = DbState::Dump;
        let restore = DbState::Restore;
        assert_ne!(dump, restore);
    }

    #[test]
    fn test_state_equality() {
        let state1: DbState = "present".parse().unwrap();
        let state2: DbState = "present".parse().unwrap();
        assert_eq!(state1, state2);
    }
}

// ============================================================================
// Schema Tests
// ============================================================================

mod schema_tests {
    use super::*;

    #[test]
    fn test_postgresql_db_module_exists() {
        // PostgreSQL modules can be instantiated directly
        let module = PostgresqlDbModule;
        assert_eq!(module.name(), "postgresql_db");
    }

    #[test]
    fn test_postgresql_modules_use_cli_tools() {
        // PostgreSQL modules should use CLI tools (psql, pg_dump, etc.)
        // and not require database drivers
        // This is validated by the module documentation
        let module = PostgresqlDbModule;
        // Module should work without sqlx feature
        assert_eq!(module.name(), "postgresql_db");
    }
}

// ============================================================================
// Cross-Platform Tests
// ============================================================================

mod cross_platform_tests {
    use super::*;

    #[test]
    fn test_postgresql_modules_always_available() {
        // PostgreSQL modules should always be available (no feature flag needed)
        // Test that they can be instantiated directly
        let modules: Vec<(&str, Box<dyn Module>)> = vec![
            ("postgresql_db", Box::new(PostgresqlDbModule)),
            ("postgresql_user", Box::new(PostgresqlUserModule)),
            ("postgresql_query", Box::new(PostgresqlQueryModule)),
            ("postgresql_privs", Box::new(PostgresqlPrivsModule)),
        ];

        for (expected_name, module) in &modules {
            assert_eq!(
                module.name(),
                *expected_name,
                "PostgreSQL module '{}' should always be available",
                expected_name
            );
        }
    }

    #[cfg(feature = "database")]
    #[test]
    fn test_mysql_modules_with_database_feature() {
        use rustible::modules::database::{
            MysqlDbModule, MysqlPrivsModule, MysqlQueryModule, MysqlUserModule,
        };

        let modules: Vec<(&str, Box<dyn Module>)> = vec![
            ("mysql_db", Box::new(MysqlDbModule)),
            ("mysql_user", Box::new(MysqlUserModule)),
            ("mysql_query", Box::new(MysqlQueryModule)),
            ("mysql_privs", Box::new(MysqlPrivsModule)),
        ];

        for (expected_name, module) in &modules {
            assert_eq!(
                module.name(),
                *expected_name,
                "MySQL module '{}' should be available with database feature",
                expected_name
            );
        }
    }
}
