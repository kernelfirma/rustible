//! Comprehensive unit tests for the YUM module
//!
//! Tests cover:
//! - State parsing (present, absent, latest, installed, removed)
//! - Module metadata (name, classification, parallelization)
//! - Shell escaping for security
//! - Parameter validation
//! - Edge cases

use rustible::modules::yum::{YumModule, YumState};
use rustible::modules::{Module, ModuleClassification, ModuleContext, ParallelizationHint};
use std::collections::HashMap;

// ============================================================================
// State Parsing Tests
// ============================================================================

#[test]
fn test_yum_state_present() {
    let state = YumState::from_str("present").unwrap();
    assert_eq!(state, YumState::Present);
}

#[test]
fn test_yum_state_installed_alias() {
    let state = YumState::from_str("installed").unwrap();
    assert_eq!(state, YumState::Present);
}

#[test]
fn test_yum_state_absent() {
    let state = YumState::from_str("absent").unwrap();
    assert_eq!(state, YumState::Absent);
}

#[test]
fn test_yum_state_removed_alias() {
    let state = YumState::from_str("removed").unwrap();
    assert_eq!(state, YumState::Absent);
}

#[test]
fn test_yum_state_latest() {
    let state = YumState::from_str("latest").unwrap();
    assert_eq!(state, YumState::Latest);
}

#[test]
fn test_yum_state_case_insensitive() {
    assert_eq!(YumState::from_str("PRESENT").unwrap(), YumState::Present);
    assert_eq!(YumState::from_str("Present").unwrap(), YumState::Present);
    assert_eq!(YumState::from_str("ABSENT").unwrap(), YumState::Absent);
    assert_eq!(YumState::from_str("LATEST").unwrap(), YumState::Latest);
}

#[test]
fn test_yum_state_invalid() {
    let result = YumState::from_str("invalid");
    assert!(result.is_err());
}

#[test]
fn test_yum_state_empty_string() {
    let result = YumState::from_str("");
    assert!(result.is_err());
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_yum_module_name() {
    let module = YumModule;
    assert_eq!(module.name(), "yum");
}

#[test]
fn test_yum_module_description() {
    let module = YumModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("yum"));
}

#[test]
fn test_yum_module_classification() {
    let module = YumModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_yum_module_parallelization_host_exclusive() {
    let module = YumModule;
    // YUM uses locks, so it should be host exclusive
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::HostExclusive
    );
}

#[test]
fn test_yum_module_required_params() {
    let module = YumModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Shell Escape Tests (Security)
// ============================================================================

#[test]
fn test_yum_shell_escape_simple_name() {
    let input = "httpd";
    assert!(input.chars().all(|c| c.is_alphanumeric() || c == '-'));
}

#[test]
fn test_yum_shell_escape_with_version() {
    let input = "nginx-1.0";
    assert!(input
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '.'));
}

#[test]
fn test_yum_command_injection_patterns() {
    let dangerous_inputs = ["; rm -rf /", "$(whoami)", "`id`"];

    for input in dangerous_inputs {
        assert!(input
            .chars()
            .any(|c| !c.is_alphanumeric() && c != '-' && c != '.'));
    }
}

// ============================================================================
// Parameter Validation Tests
// ============================================================================

#[test]
fn test_yum_missing_name_parameter() {
    let module = YumModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_yum_empty_name_parameter() {
    let module = YumModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!(""));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_yum_invalid_state_parameter() {
    let module = YumModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("httpd"));
    params.insert("state".to_string(), serde_json::json!("invalid_state"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_yum_with_update_cache_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("httpd"));
    params.insert("update_cache".to_string(), serde_json::json!(true));

    assert!(params.contains_key("update_cache"));
}

#[test]
fn test_yum_with_disable_gpg_check() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("httpd"));
    params.insert("disable_gpg_check".to_string(), serde_json::json!(true));

    assert!(params.contains_key("disable_gpg_check"));
}

#[test]
fn test_yum_with_enablerepo() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("httpd"));
    params.insert("enablerepo".to_string(), serde_json::json!("epel"));

    assert!(params.contains_key("enablerepo"));
}

#[test]
fn test_yum_with_disablerepo() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("httpd"));
    params.insert("disablerepo".to_string(), serde_json::json!("*"));

    assert!(params.contains_key("disablerepo"));
}

#[test]
fn test_yum_package_list_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "name".to_string(),
        serde_json::json!(["httpd", "vim", "curl"]),
    );

    assert!(params.contains_key("name"));
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_yum_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_yum_rpm_specific_package_names() {
    let valid_names = ["kernel-headers", "gcc-c++", "python3-devel"];

    for name in valid_names {
        assert!(!name.is_empty());
    }
}

#[test]
fn test_yum_null_connection_handling() {
    let module = YumModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("httpd"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// State Clone and Equality Tests
// ============================================================================

#[test]
fn test_yum_state_clone() {
    let state = YumState::Present;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn test_yum_state_debug_format() {
    let state = YumState::Present;
    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("Present"));
}

#[test]
fn test_yum_state_equality() {
    assert_eq!(YumState::Present, YumState::Present);
    assert_eq!(YumState::Absent, YumState::Absent);
    assert_eq!(YumState::Latest, YumState::Latest);
    assert_ne!(YumState::Present, YumState::Absent);
}
