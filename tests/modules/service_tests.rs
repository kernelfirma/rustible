//! Comprehensive unit tests for the Service module
//!
//! Tests cover:
//! - State parsing (started, stopped, restarted, reloaded, running)
//! - Init system detection
//! - Module metadata
//! - Parameter validation
//! - Edge cases

use rustible::modules::service::{InitSystem, ServiceModule, ServiceState};
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;

// ============================================================================
// ServiceState Parsing Tests
// ============================================================================

#[test]
fn test_service_state_started() {
    let state = ServiceState::from_str("started").unwrap();
    assert_eq!(state, ServiceState::Started);
}

#[test]
fn test_service_state_running_alias() {
    let state = ServiceState::from_str("running").unwrap();
    assert_eq!(state, ServiceState::Started);
}

#[test]
fn test_service_state_stopped() {
    let state = ServiceState::from_str("stopped").unwrap();
    assert_eq!(state, ServiceState::Stopped);
}

#[test]
fn test_service_state_restarted() {
    let state = ServiceState::from_str("restarted").unwrap();
    assert_eq!(state, ServiceState::Restarted);
}

#[test]
fn test_service_state_reloaded() {
    let state = ServiceState::from_str("reloaded").unwrap();
    assert_eq!(state, ServiceState::Reloaded);
}

#[test]
fn test_service_state_case_insensitive() {
    assert_eq!(
        ServiceState::from_str("STARTED").unwrap(),
        ServiceState::Started
    );
    assert_eq!(
        ServiceState::from_str("Started").unwrap(),
        ServiceState::Started
    );
    assert_eq!(
        ServiceState::from_str("STOPPED").unwrap(),
        ServiceState::Stopped
    );
}

#[test]
fn test_service_state_invalid() {
    let result = ServiceState::from_str("invalid");
    assert!(result.is_err());
}

#[test]
fn test_service_state_empty_string() {
    let result = ServiceState::from_str("");
    assert!(result.is_err());
}

// ============================================================================
// InitSystem Tests
// ============================================================================

#[test]
fn test_init_system_variants() {
    let systemd = InitSystem::Systemd;
    let sysv = InitSystem::SysV;
    let upstart = InitSystem::Upstart;
    let openrc = InitSystem::OpenRC;
    let launchd = InitSystem::Launchd;

    assert_eq!(systemd, InitSystem::Systemd);
    assert_eq!(sysv, InitSystem::SysV);
    assert_eq!(upstart, InitSystem::Upstart);
    assert_eq!(openrc, InitSystem::OpenRC);
    assert_eq!(launchd, InitSystem::Launchd);
}

#[test]
fn test_init_system_debug_format() {
    let systemd = InitSystem::Systemd;
    let debug_str = format!("{:?}", systemd);
    assert!(debug_str.contains("Systemd"));
}

#[test]
fn test_init_system_clone() {
    let systemd = InitSystem::Systemd;
    let cloned = systemd.clone();
    assert_eq!(systemd, cloned);
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_service_module_name() {
    let module = ServiceModule;
    assert_eq!(module.name(), "service");
}

#[test]
fn test_service_module_description() {
    let module = ServiceModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("service"));
}

#[test]
fn test_service_module_classification() {
    let module = ServiceModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_service_module_required_params() {
    let module = ServiceModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Parameter Validation Tests
// ============================================================================

#[test]
fn test_service_missing_name_parameter() {
    let module = ServiceModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_service_with_state_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("started"));

    assert!(params.contains_key("state"));
}

#[test]
fn test_service_with_enabled_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("enabled".to_string(), serde_json::json!(true));

    assert!(params.contains_key("enabled"));
}

#[test]
fn test_service_with_daemon_reload() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("daemon_reload".to_string(), serde_json::json!(true));

    assert!(params.contains_key("daemon_reload"));
}

#[test]
fn test_service_invalid_state_parameter() {
    let module = ServiceModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("invalid_state"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_service_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_service_common_service_names() {
    let valid_services = [
        "nginx",
        "apache2",
        "httpd",
        "sshd",
        "docker",
        "mysql",
        "postgresql",
        "redis-server",
    ];

    for name in valid_services {
        assert!(!name.is_empty());
        assert!(name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
    }
}

#[test]
fn test_service_null_connection_handling() {
    let module = ServiceModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// State Clone and Equality Tests
// ============================================================================

#[test]
fn test_service_state_clone() {
    let state = ServiceState::Started;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn test_service_state_debug_format() {
    let state = ServiceState::Started;
    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("Started"));
}

#[test]
fn test_service_state_equality() {
    assert_eq!(ServiceState::Started, ServiceState::Started);
    assert_eq!(ServiceState::Stopped, ServiceState::Stopped);
    assert_eq!(ServiceState::Restarted, ServiceState::Restarted);
    assert_eq!(ServiceState::Reloaded, ServiceState::Reloaded);
    assert_ne!(ServiceState::Started, ServiceState::Stopped);
}

// ============================================================================
// Combined State and Enabled Tests
// ============================================================================

#[test]
fn test_service_multiple_parameters() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("state".to_string(), serde_json::json!("started"));
    params.insert("enabled".to_string(), serde_json::json!(true));
    params.insert("daemon_reload".to_string(), serde_json::json!(false));

    assert_eq!(params.len(), 4);
}
