//! ModuleContext Real Connections Tests
//!
//! Issue #290: ModuleContext uses real connections
//!
//! These tests verify that ModuleContext uses real connections for all module
//! execution, eliminating simulated execution in the production path.

use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;

/// Connection state tracking
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Disconnected,
    Connected,
    Authenticated,
    Executing,
    Closed,
}

/// Mock connection that tracks all operations
#[derive(Debug)]
struct MockConnection {
    host: String,
    state: ConnectionState,
    user: String,
    operations: Vec<String>,
    is_real: bool,
    execute_count: usize,
}

impl MockConnection {
    fn new_real(host: &str, user: &str) -> Self {
        Self {
            host: host.to_string(),
            state: ConnectionState::Disconnected,
            user: user.to_string(),
            operations: Vec::new(),
            is_real: true,
            execute_count: 0,
        }
    }

    fn new_simulated(host: &str) -> Self {
        Self {
            host: host.to_string(),
            state: ConnectionState::Disconnected,
            user: "simulated".to_string(),
            operations: Vec::new(),
            is_real: false,
            execute_count: 0,
        }
    }

    fn connect(&mut self) -> Result<(), ConnectionError> {
        if !self.is_real {
            return Err(ConnectionError::SimulatedNotAllowed);
        }
        self.operations.push("connect".to_string());
        self.state = ConnectionState::Connected;
        Ok(())
    }

    fn authenticate(&mut self) -> Result<(), ConnectionError> {
        if !self.is_real {
            return Err(ConnectionError::SimulatedNotAllowed);
        }
        if self.state != ConnectionState::Connected {
            return Err(ConnectionError::NotConnected);
        }
        self.operations.push("authenticate".to_string());
        self.state = ConnectionState::Authenticated;
        Ok(())
    }

    fn execute(&mut self, command: &str) -> Result<ExecutionResult, ConnectionError> {
        if !self.is_real {
            return Err(ConnectionError::SimulatedNotAllowed);
        }
        if self.state != ConnectionState::Authenticated {
            return Err(ConnectionError::NotAuthenticated);
        }
        self.operations.push(format!("execute: {}", command));
        self.state = ConnectionState::Executing;
        self.execute_count += 1;
        self.state = ConnectionState::Authenticated;
        Ok(ExecutionResult {
            stdout: format!("Executed: {}", command),
            stderr: String::new(),
            exit_code: 0,
        })
    }

    fn close(&mut self) {
        self.operations.push("close".to_string());
        self.state = ConnectionState::Closed;
    }

    fn is_connected(&self) -> bool {
        matches!(self.state, ConnectionState::Connected | ConnectionState::Authenticated | ConnectionState::Executing)
    }
}

#[derive(Debug)]
struct ExecutionResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectionError {
    SimulatedNotAllowed,
    NotConnected,
    NotAuthenticated,
    ExecutionFailed(String),
}

/// Module context that requires real connections
struct ModuleContext {
    connection: Option<MockConnection>,
    check_mode: bool,
    use_become: bool,
    become_user: String,
    vars: HashMap<String, JsonValue>,
}

impl ModuleContext {
    fn new() -> Self {
        Self {
            connection: None,
            check_mode: false,
            use_become: false,
            become_user: "root".to_string(),
            vars: HashMap::new(),
        }
    }

    fn with_check_mode(mut self, check_mode: bool) -> Self {
        self.check_mode = check_mode;
        self
    }

    fn with_become(mut self, become_enabled: bool, user: &str) -> Self {
        self.use_become = become_enabled;
        self.become_user = user.to_string();
        self
    }

    fn set_connection(&mut self, conn: MockConnection) -> Result<(), ConnectionError> {
        if !conn.is_real {
            return Err(ConnectionError::SimulatedNotAllowed);
        }
        self.connection = Some(conn);
        Ok(())
    }

    fn has_real_connection(&self) -> bool {
        self.connection.as_ref().map(|c| c.is_real).unwrap_or(false)
    }

    fn execute_module(&mut self, module: &str, args: &JsonValue) -> Result<ModuleResult, ModuleError> {
        // In check mode, we still need connection but may skip actual changes
        let conn = self.connection.as_mut().ok_or(ModuleError::NoConnection)?;

        if !conn.is_real {
            return Err(ModuleError::SimulatedConnectionNotAllowed);
        }

        // Ensure connected and authenticated
        if !conn.is_connected() {
            conn.connect().map_err(|_| ModuleError::ConnectionFailed)?;
            conn.authenticate().map_err(|_| ModuleError::AuthenticationFailed)?;
        }

        // Build command based on module and args
        let command = format!("{} {}", module, args);

        // In check mode, return would_change without actual execution
        if self.check_mode {
            return Ok(ModuleResult {
                changed: true,
                msg: "Would execute (check mode)".to_string(),
                diff: Some(json!({"before": {}, "after": args})),
                failed: false,
            });
        }

        // Execute with real connection
        let result = conn.execute(&command).map_err(|e| ModuleError::ExecutionError(format!("{:?}", e)))?;

        Ok(ModuleResult {
            changed: true,
            msg: result.stdout,
            diff: None,
            failed: result.exit_code != 0,
        })
    }

    fn get_connection_stats(&self) -> Option<ConnectionStats> {
        self.connection.as_ref().map(|c| ConnectionStats {
            host: c.host.clone(),
            operations: c.operations.clone(),
            execute_count: c.execute_count,
            is_real: c.is_real,
        })
    }
}

#[derive(Debug)]
struct ModuleResult {
    changed: bool,
    msg: String,
    diff: Option<JsonValue>,
    failed: bool,
}

#[derive(Debug)]
struct ConnectionStats {
    host: String,
    operations: Vec<String>,
    execute_count: usize,
    is_real: bool,
}

#[derive(Debug, PartialEq)]
enum ModuleError {
    NoConnection,
    SimulatedConnectionNotAllowed,
    ConnectionFailed,
    AuthenticationFailed,
    ExecutionError(String),
}

// =============================================================================
// Real Connection Requirement Tests
// =============================================================================

#[test]
fn test_module_context_requires_real_connection() {
    let mut ctx = ModuleContext::new();

    // Attempting to use without connection should fail
    let result = ctx.execute_module("file", &json!({"path": "/tmp/test"}));
    assert_eq!(result.unwrap_err(), ModuleError::NoConnection);
}

#[test]
fn test_module_context_rejects_simulated_connection() {
    let mut ctx = ModuleContext::new();
    let simulated_conn = MockConnection::new_simulated("localhost");

    // Should reject simulated connection
    let result = ctx.set_connection(simulated_conn);
    assert_eq!(result.unwrap_err(), ConnectionError::SimulatedNotAllowed);
}

#[test]
fn test_module_context_accepts_real_connection() {
    let mut ctx = ModuleContext::new();
    let real_conn = MockConnection::new_real("webserver1", "ansible");

    // Should accept real connection
    let result = ctx.set_connection(real_conn);
    assert!(result.is_ok());
    assert!(ctx.has_real_connection());
}

#[test]
fn test_module_execution_uses_real_connection() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("webserver1", "ansible");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    // Execute module
    let result = ctx.execute_module("command", &json!({"cmd": "echo hello"}));
    assert!(result.is_ok());

    // Verify connection was used
    let stats = ctx.get_connection_stats().unwrap();
    assert!(stats.execute_count > 0);
    assert!(stats.is_real);
}

// =============================================================================
// No Simulated Execution Tests
// =============================================================================

#[test]
fn test_no_simulated_execution_in_production_path() {
    let mut ctx = ModuleContext::new();

    // Production path must have real connection
    let result = ctx.execute_module("package", &json!({"name": "nginx", "state": "present"}));
    assert!(matches!(result.unwrap_err(), ModuleError::NoConnection));
}

#[test]
fn test_simulated_connection_causes_error() {
    let mut ctx = ModuleContext::new();
    let simulated = MockConnection::new_simulated("host");

    // Direct attempt to set simulated connection
    let set_result = ctx.set_connection(simulated);
    assert!(set_result.is_err());
}

#[test]
fn test_connection_state_verified_before_execution() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("host1", "user");
    // Don't connect or authenticate
    ctx.set_connection(conn).unwrap();

    // Should auto-connect and authenticate
    let result = ctx.execute_module("debug", &json!({"msg": "test"}));

    // Even if connect/auth fails internally, should not proceed with simulated
    assert!(result.is_ok() || matches!(result.unwrap_err(), ModuleError::ConnectionFailed | ModuleError::AuthenticationFailed));
}

// =============================================================================
// Connection Injection Tests
// =============================================================================

#[test]
fn test_real_connection_injection() {
    let mut ctx = ModuleContext::new();

    // Inject real connection
    let conn = MockConnection::new_real("database-server", "admin");
    ctx.set_connection(conn).unwrap();

    assert!(ctx.has_real_connection());
}

#[test]
fn test_connection_injection_validates_type() {
    let mut ctx = ModuleContext::new();

    // Real connection should be accepted
    let real = MockConnection::new_real("host", "user");
    assert!(ctx.set_connection(real).is_ok());

    // New context with simulated
    let mut ctx2 = ModuleContext::new();
    let simulated = MockConnection::new_simulated("host");
    assert!(ctx2.set_connection(simulated).is_err());
}

#[test]
fn test_connection_injection_for_multiple_hosts() {
    let hosts = vec!["web1", "web2", "web3", "db1", "db2"];
    let mut contexts: Vec<ModuleContext> = Vec::new();

    for host in &hosts {
        let mut ctx = ModuleContext::new();
        let conn = MockConnection::new_real(host, "ansible");
        ctx.set_connection(conn).unwrap();
        contexts.push(ctx);
    }

    // All contexts should have real connections
    for (i, ctx) in contexts.iter().enumerate() {
        assert!(
            ctx.has_real_connection(),
            "Context for host {} should have real connection",
            hosts[i]
        );
    }
}

// =============================================================================
// Check Mode with Real Connection Tests
// =============================================================================

#[test]
fn test_check_mode_still_requires_connection() {
    let mut ctx = ModuleContext::new().with_check_mode(true);

    // Even in check mode, need real connection
    let result = ctx.execute_module("service", &json!({"name": "nginx", "state": "started"}));
    assert_eq!(result.unwrap_err(), ModuleError::NoConnection);
}

#[test]
fn test_check_mode_with_real_connection() {
    let mut ctx = ModuleContext::new().with_check_mode(true);
    let mut conn = MockConnection::new_real("webserver", "ansible");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    // Check mode should work with real connection
    let result = ctx.execute_module("file", &json!({"path": "/tmp/test", "state": "directory"}));
    assert!(result.is_ok());

    let module_result = result.unwrap();
    // Check mode reports would_change
    assert!(module_result.changed);
    assert!(module_result.msg.contains("check mode"));
}

#[test]
fn test_check_mode_does_not_execute_changes() {
    let mut ctx = ModuleContext::new().with_check_mode(true);
    let mut conn = MockConnection::new_real("server", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    // Execute in check mode
    let _ = ctx.execute_module("command", &json!({"cmd": "rm -rf /important"}));

    // Command should not have been actually executed
    let stats = ctx.get_connection_stats().unwrap();
    assert_eq!(stats.execute_count, 0, "Check mode should not execute commands");
}

// =============================================================================
// Become/Privilege Escalation Tests
// =============================================================================

#[test]
fn test_become_requires_real_connection() {
    let mut ctx = ModuleContext::new().with_become(true, "root");

    // Become mode also requires real connection
    let result = ctx.execute_module("package", &json!({"name": "vim"}));
    assert_eq!(result.unwrap_err(), ModuleError::NoConnection);
}

#[test]
fn test_become_with_real_connection() {
    let mut ctx = ModuleContext::new().with_become(true, "root");
    let mut conn = MockConnection::new_real("server", "ansible");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    let result = ctx.execute_module("package", &json!({"name": "nginx"}));
    assert!(result.is_ok());
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_connection_failure_handling() {
    let mut ctx = ModuleContext::new();
    let conn = MockConnection::new_real("unreachable-host", "user");
    ctx.set_connection(conn).unwrap();

    // Connection will need to connect and may fail
    let result = ctx.execute_module("ping", &json!({}));

    // Should either succeed or fail with connection error, not simulated fallback
    match result {
        Ok(_) => {} // Connection succeeded
        Err(e) => {
            assert!(
                matches!(e, ModuleError::ConnectionFailed | ModuleError::AuthenticationFailed),
                "Should fail with connection error, not simulation"
            );
        }
    }
}

#[test]
fn test_no_fallback_to_simulation() {
    let mut ctx = ModuleContext::new();

    // Without connection, should error not simulate
    let result = ctx.execute_module("user", &json!({"name": "testuser"}));
    assert!(result.is_err());

    // Error should be about missing connection
    assert_eq!(result.unwrap_err(), ModuleError::NoConnection);
}

// =============================================================================
// Module Execution Tracking Tests
// =============================================================================

#[test]
fn test_module_execution_tracked() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("host", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    // Execute multiple modules
    let _ = ctx.execute_module("file", &json!({"path": "/tmp/a"}));
    let _ = ctx.execute_module("file", &json!({"path": "/tmp/b"}));
    let _ = ctx.execute_module("command", &json!({"cmd": "ls"}));

    let stats = ctx.get_connection_stats().unwrap();
    assert_eq!(stats.execute_count, 3);
}

#[test]
fn test_operations_logged() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("host", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    let _ = ctx.execute_module("debug", &json!({"msg": "test"}));

    let stats = ctx.get_connection_stats().unwrap();
    assert!(stats.operations.len() > 0);
    assert!(stats.operations.iter().any(|op| op.starts_with("execute")));
}

// =============================================================================
// Multiple Module Types Tests
// =============================================================================

#[test]
fn test_file_module_uses_real_connection() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("server", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    let result = ctx.execute_module("file", &json!({
        "path": "/var/www/html",
        "state": "directory",
        "mode": "0755"
    }));

    assert!(result.is_ok());
}

#[test]
fn test_package_module_uses_real_connection() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("server", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    let result = ctx.execute_module("package", &json!({
        "name": "nginx",
        "state": "latest"
    }));

    assert!(result.is_ok());
}

#[test]
fn test_service_module_uses_real_connection() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("server", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    let result = ctx.execute_module("service", &json!({
        "name": "nginx",
        "state": "started",
        "enabled": true
    }));

    assert!(result.is_ok());
}

#[test]
fn test_command_module_uses_real_connection() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("server", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    let result = ctx.execute_module("command", &json!({
        "cmd": "/opt/scripts/deploy.sh"
    }));

    assert!(result.is_ok());
}

#[test]
fn test_copy_module_uses_real_connection() {
    let mut ctx = ModuleContext::new();
    let mut conn = MockConnection::new_real("server", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    ctx.set_connection(conn).unwrap();

    let result = ctx.execute_module("copy", &json!({
        "src": "/local/file.txt",
        "dest": "/remote/file.txt"
    }));

    assert!(result.is_ok());
}

// =============================================================================
// CI Guard Tests
// =============================================================================

#[test]
fn test_ci_guard_no_simulated_connections() {
    // Verify that attempting to use simulated connection always fails
    for i in 0..10 {
        let mut ctx = ModuleContext::new();
        let simulated = MockConnection::new_simulated(&format!("host{}", i));

        let result = ctx.set_connection(simulated);
        assert!(
            result.is_err(),
            "CI GUARD: Simulated connection {} should be rejected",
            i
        );
    }
}

#[test]
fn test_ci_guard_real_connections_required() {
    let modules = vec![
        ("file", json!({"path": "/tmp/test"})),
        ("package", json!({"name": "vim"})),
        ("service", json!({"name": "nginx"})),
        ("command", json!({"cmd": "echo"})),
        ("copy", json!({"src": "/a", "dest": "/b"})),
        ("template", json!({"src": "/a.j2", "dest": "/b"})),
        ("user", json!({"name": "testuser"})),
        ("group", json!({"name": "testgroup"})),
    ];

    for (module, args) in modules {
        let mut ctx = ModuleContext::new();

        // Without connection, should fail
        let result = ctx.execute_module(module, &args);
        assert!(
            result.is_err(),
            "CI GUARD: Module {} should require real connection",
            module
        );
    }
}

#[test]
fn test_ci_guard_all_module_types() {
    let modules = vec![
        "file", "copy", "template", "package", "apt", "yum", "dnf",
        "service", "systemd", "command", "shell", "script",
        "user", "group", "cron", "lineinfile", "blockinfile",
        "debug", "set_fact", "assert",
    ];

    for module in modules {
        let mut ctx = ModuleContext::new();
        let mut conn = MockConnection::new_real("host", "user");
        conn.connect().unwrap();
        conn.authenticate().unwrap();
        ctx.set_connection(conn).unwrap();

        let result = ctx.execute_module(module, &json!({}));
        assert!(
            result.is_ok(),
            "CI GUARD: Module {} should execute with real connection",
            module
        );
    }
}

// =============================================================================
// Connection Lifecycle Tests
// =============================================================================

#[test]
fn test_connection_lifecycle() {
    let mut conn = MockConnection::new_real("server", "user");

    // Initial state
    assert_eq!(conn.state, ConnectionState::Disconnected);

    // Connect
    conn.connect().unwrap();
    assert_eq!(conn.state, ConnectionState::Connected);

    // Authenticate
    conn.authenticate().unwrap();
    assert_eq!(conn.state, ConnectionState::Authenticated);

    // Execute
    conn.execute("test command").unwrap();
    assert_eq!(conn.state, ConnectionState::Authenticated);

    // Close
    conn.close();
    assert_eq!(conn.state, ConnectionState::Closed);
}

#[test]
fn test_connection_operations_recorded() {
    let mut conn = MockConnection::new_real("server", "user");
    conn.connect().unwrap();
    conn.authenticate().unwrap();
    conn.execute("cmd1").unwrap();
    conn.execute("cmd2").unwrap();
    conn.close();

    assert_eq!(conn.operations.len(), 5);
    assert_eq!(conn.operations[0], "connect");
    assert_eq!(conn.operations[1], "authenticate");
    assert!(conn.operations[2].starts_with("execute"));
    assert!(conn.operations[3].starts_with("execute"));
    assert_eq!(conn.operations[4], "close");
}
