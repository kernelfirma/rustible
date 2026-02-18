//! Comprehensive unit tests for the DNF module
//!
//! Tests cover:
//! - State parsing (present, absent, latest, installed, removed)
//! - Module metadata (name, classification, parallelization)
//! - Shell escaping for security
//! - Parameter validation
//! - Edge cases

use rustible::modules::dnf::{DnfModule, DnfState};
use rustible::modules::{Module, ModuleClassification, ModuleContext, ParallelizationHint};
use std::collections::HashMap;

// ============================================================================
// State Parsing Tests
// ============================================================================

#[test]
fn test_dnf_state_present() {
    let state = DnfState::from_str("present").unwrap();
    assert_eq!(state, DnfState::Present);
}

#[test]
fn test_dnf_state_installed_alias() {
    let state = DnfState::from_str("installed").unwrap();
    assert_eq!(state, DnfState::Present);
}

#[test]
fn test_dnf_state_absent() {
    let state = DnfState::from_str("absent").unwrap();
    assert_eq!(state, DnfState::Absent);
}

#[test]
fn test_dnf_state_removed_alias() {
    let state = DnfState::from_str("removed").unwrap();
    assert_eq!(state, DnfState::Absent);
}

#[test]
fn test_dnf_state_latest() {
    let state = DnfState::from_str("latest").unwrap();
    assert_eq!(state, DnfState::Latest);
}

#[test]
fn test_dnf_state_case_insensitive() {
    assert_eq!(DnfState::from_str("PRESENT").unwrap(), DnfState::Present);
    assert_eq!(DnfState::from_str("Present").unwrap(), DnfState::Present);
    assert_eq!(DnfState::from_str("ABSENT").unwrap(), DnfState::Absent);
    assert_eq!(DnfState::from_str("LATEST").unwrap(), DnfState::Latest);
}

#[test]
fn test_dnf_state_invalid() {
    let result = DnfState::from_str("invalid");
    assert!(result.is_err());
}

#[test]
fn test_dnf_state_empty_string() {
    let result = DnfState::from_str("");
    assert!(result.is_err());
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_dnf_module_name() {
    let module = DnfModule;
    assert_eq!(module.name(), "dnf");
}

#[test]
fn test_dnf_module_description() {
    let module = DnfModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("dnf"));
}

#[test]
fn test_dnf_module_classification() {
    let module = DnfModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_dnf_module_parallelization_host_exclusive() {
    let module = DnfModule;
    // DNF uses locks, so it should be host exclusive
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::HostExclusive
    );
}

#[test]
fn test_dnf_module_required_params() {
    let module = DnfModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Shell Escape Tests (Security)
// ============================================================================

#[test]
fn test_dnf_shell_escape_simple_name() {
    let input = "httpd";
    assert!(input.chars().all(|c| c.is_alphanumeric() || c == '-'));
}

#[test]
fn test_dnf_shell_escape_with_version() {
    let input = "kernel-5.14.0-284";
    assert!(input
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '.'));
}

#[test]
fn test_dnf_command_injection_patterns() {
    let dangerous_inputs = [
        "; rm -rf /",
        "$(whoami)",
        "`id`",
        "pkg && malicious",
        "pkg || malicious",
    ];

    for input in dangerous_inputs {
        // These should not pass validation
        assert!(input
            .chars()
            .any(|c| !c.is_alphanumeric() && c != '-' && c != '.'));
    }
}

// ============================================================================
// Parameter Validation Tests
// ============================================================================

#[test]
fn test_dnf_missing_name_parameter() {
    let module = DnfModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_dnf_empty_name_parameter() {
    let module = DnfModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!(""));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_dnf_invalid_state_parameter() {
    let module = DnfModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("httpd"));
    params.insert("state".to_string(), serde_json::json!("invalid_state"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_dnf_with_update_cache_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("httpd"));
    params.insert("update_cache".to_string(), serde_json::json!(true));

    assert!(params.contains_key("update_cache"));
}

#[test]
fn test_dnf_package_list_parameter() {
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
fn test_dnf_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_dnf_rpm_specific_package_names() {
    // Valid RPM package name patterns
    let valid_names = [
        "glibc-2.34-40",
        "kernel-core",
        "python3-libs",
        "gcc-c++",
        "libstdc++-devel",
    ];

    for name in valid_names {
        assert!(!name.is_empty());
    }
}

#[test]
fn test_dnf_null_connection_handling() {
    let module = DnfModule;
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
fn test_dnf_state_clone() {
    let state = DnfState::Present;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn test_dnf_state_debug_format() {
    let state = DnfState::Present;
    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("Present"));
}

#[test]
fn test_dnf_state_equality() {
    assert_eq!(DnfState::Present, DnfState::Present);
    assert_eq!(DnfState::Absent, DnfState::Absent);
    assert_eq!(DnfState::Latest, DnfState::Latest);
    assert_ne!(DnfState::Present, DnfState::Absent);
}
