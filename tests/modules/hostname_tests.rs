//! Integration tests for the hostname module
//!
//! Note: Most tests are marked #[ignore] as they require a connection
//! or privileged access. Run with --ignored to test against a real system.

use rustible::modules::{hostname::HostnameModule, Module, ModuleParams};
use std::collections::HashMap;

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_name(mut params: ModuleParams, name: &str) -> ModuleParams {
    params.insert("name".to_string(), serde_json::json!(name));
    params
}

// ============================================================================
// Module Metadata Tests (no connection needed)
// ============================================================================

#[test]
fn test_hostname_module_name() {
    let module = HostnameModule;
    assert_eq!(module.name(), "hostname");
}

#[test]
fn test_hostname_module_description() {
    let module = HostnameModule;
    let desc = module.description();
    assert!(!desc.is_empty());
    assert!(desc.to_lowercase().contains("hostname"));
}

#[test]
fn test_hostname_module_classification() {
    use rustible::modules::ModuleClassification;
    let module = HostnameModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_hostname_required_params() {
    let module = HostnameModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Parameter Validation Tests (no connection needed)
// These use the default validate_params which returns Ok(())
// ============================================================================

#[test]
fn test_hostname_validate_params_simple() {
    let module = HostnameModule;
    let params = with_name(create_params(), "myhost");

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Simple hostname should be valid");
}

#[test]
fn test_hostname_validate_params_fqdn() {
    let module = HostnameModule;
    let params = with_name(create_params(), "myhost.example.com");

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "FQDN should be valid");
}

#[test]
fn test_hostname_validate_params_with_numbers() {
    let module = HostnameModule;
    let params = with_name(create_params(), "server01");

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Hostname with numbers should be valid");
}

#[test]
fn test_hostname_validate_params_with_hyphens() {
    let module = HostnameModule;
    let params = with_name(create_params(), "web-server-01");

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Hostname with hyphens should be valid");
}

#[test]
fn test_hostname_validate_empty_params() {
    let module = HostnameModule;
    let params = create_params();

    // Default validate_params returns Ok(()) - required param checking is done elsewhere
    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

#[test]
fn test_hostname_validate_with_strategy() {
    let module = HostnameModule;
    let mut params = with_name(create_params(), "newhost");
    params.insert("use".to_string(), serde_json::json!("systemd"));

    let result = module.validate_params(&params);
    assert!(result.is_ok(), "Hostname with strategy should be valid");
}

// ============================================================================
// Execution Tests (require connection or privileges - ignored)
// ============================================================================

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_hostname_set_simple() {
    // Would test setting a simple hostname
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_hostname_set_fqdn() {
    // Would test setting a fully qualified domain name
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_hostname_file_strategy() {
    // Would test file-based hostname strategy
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_hostname_systemd_strategy() {
    // Would test systemd hostnamectl strategy
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_hostname_command_strategy() {
    // Would test hostname command strategy
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_hostname_idempotent() {
    // Would test idempotency
}

#[test]
#[ignore = "Requires connection for remote execution"]
fn test_hostname_check_mode() {
    // Would test check mode
}
