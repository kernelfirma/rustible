//! ModuleContext Real Connections Tests
//!
//! Issue #290: ModuleContext uses real connections
//!
//! These tests exercise the production ModuleContext and connection
//! implementations to ensure module execution uses real connections.

use std::collections::HashMap;
use std::sync::Arc;

use rustible::connection::local::LocalConnection;
use rustible::modules::command::CommandModule;
use rustible::modules::{Module, ModuleContextBuilder, ModuleParams};
use serde_json::json;

#[test]
fn test_command_module_with_local_connection() {
    let connection = Arc::new(LocalConnection::new());
    let context = ModuleContextBuilder::new()
        .connection(connection)
        .build()
        .expect("valid module context");

    let mut params: ModuleParams = HashMap::new();
    params.insert("cmd".to_string(), json!("echo hello"));
    params.insert("shell_type".to_string(), json!("posix"));

    let module = CommandModule;
    let result = module
        .execute(&params, &context)
        .expect("command execution");

    assert!(result.changed);
    assert_eq!(result.rc, Some(0));
    assert!(result.stdout.unwrap_or_default().contains("hello"));
}

#[test]
fn test_command_module_check_mode_with_connection() {
    let connection = Arc::new(LocalConnection::new());
    let context = ModuleContextBuilder::new()
        .connection(connection)
        .check_mode(true)
        .build()
        .expect("valid module context");

    let mut params: ModuleParams = HashMap::new();
    params.insert("cmd".to_string(), json!("echo hello"));
    params.insert("shell_type".to_string(), json!("posix"));

    let module = CommandModule;
    let result = module
        .execute(&params, &context)
        .expect("command execution");

    assert!(result.changed);
    assert_eq!(result.rc, Some(0));
    assert_eq!(result.stdout.unwrap_or_default(), "");
}
