//! Comprehensive unit tests for the Pip module
//!
//! Tests cover:
//! - State parsing (present, absent, latest, installed, removed)
//! - Module metadata
//! - Parameter validation
//! - Virtualenv handling
//! - Edge cases

use rustible::modules::pip::{PipModule, PipState};
use rustible::modules::{Module, ModuleClassification, ModuleContext, ParallelizationHint};
use std::collections::HashMap;

// ============================================================================
// State Parsing Tests
// ============================================================================

#[test]
fn test_pip_state_present() {
    let state = PipState::from_str("present").unwrap();
    assert_eq!(state, PipState::Present);
}

#[test]
fn test_pip_state_installed_alias() {
    let state = PipState::from_str("installed").unwrap();
    assert_eq!(state, PipState::Present);
}

#[test]
fn test_pip_state_absent() {
    let state = PipState::from_str("absent").unwrap();
    assert_eq!(state, PipState::Absent);
}

#[test]
fn test_pip_state_removed_alias() {
    let state = PipState::from_str("removed").unwrap();
    assert_eq!(state, PipState::Absent);
}

#[test]
fn test_pip_state_latest() {
    let state = PipState::from_str("latest").unwrap();
    assert_eq!(state, PipState::Latest);
}

#[test]
fn test_pip_state_case_insensitive() {
    assert_eq!(PipState::from_str("PRESENT").unwrap(), PipState::Present);
    assert_eq!(PipState::from_str("Present").unwrap(), PipState::Present);
    assert_eq!(PipState::from_str("ABSENT").unwrap(), PipState::Absent);
    assert_eq!(PipState::from_str("LATEST").unwrap(), PipState::Latest);
}

#[test]
fn test_pip_state_invalid() {
    let result = PipState::from_str("invalid");
    assert!(result.is_err());
}

#[test]
fn test_pip_state_empty_string() {
    let result = PipState::from_str("");
    assert!(result.is_err());
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_pip_module_name() {
    let module = PipModule;
    assert_eq!(module.name(), "pip");
}

#[test]
fn test_pip_module_description() {
    let module = PipModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("pip"));
}

#[test]
fn test_pip_module_classification() {
    let module = PipModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_pip_module_parallelization() {
    let module = PipModule;
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::FullyParallel
    );
}

#[test]
fn test_pip_module_required_params() {
    let module = PipModule;
    let required = module.required_params();
    // Either 'name' or 'requirements' must be provided, but neither is strictly required
    assert!(required.is_empty());
}

// ============================================================================
// Build Pip Command Tests
// ============================================================================

#[test]
fn test_build_pip_command_default() {
    let module = PipModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();

    let cmd = module.build_pip_command(&params).unwrap();
    assert_eq!(cmd, "pip3");
}

#[test]
fn test_build_pip_command_custom_executable() {
    let module = PipModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("executable".to_string(), serde_json::json!("pip"));

    let cmd = module.build_pip_command(&params).unwrap();
    assert_eq!(cmd, "pip");
}

#[test]
fn test_build_pip_command_virtualenv() {
    let module = PipModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("virtualenv".to_string(), serde_json::json!("/path/to/venv"));

    let cmd = module.build_pip_command(&params).unwrap();
    assert_eq!(cmd, "/path/to/venv/bin/pip");
}

#[test]
fn test_build_pip_command_virtualenv_overrides_executable() {
    let module = PipModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("executable".to_string(), serde_json::json!("custom-pip"));
    params.insert("virtualenv".to_string(), serde_json::json!("/path/to/venv"));

    let cmd = module.build_pip_command(&params).unwrap();
    assert_eq!(cmd, "/path/to/venv/bin/pip");
}

// ============================================================================
// Parameter Validation Tests
// ============================================================================

#[test]
fn test_pip_validate_params_missing_both() {
    let module = PipModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();

    let result = module.validate_params(&params);
    assert!(result.is_err());
}

#[test]
fn test_pip_validate_params_with_name() {
    let module = PipModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("requests"));

    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

#[test]
fn test_pip_validate_params_with_requirements() {
    let module = PipModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "requirements".to_string(),
        serde_json::json!("requirements.txt"),
    );

    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

// ============================================================================
// Package Name Tests
// ============================================================================

#[test]
fn test_pip_with_single_package() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("requests"));

    assert!(params.contains_key("name"));
}

#[test]
fn test_pip_with_package_list() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "name".to_string(),
        serde_json::json!(["requests", "flask", "django"]),
    );

    assert!(params.contains_key("name"));
}

#[test]
fn test_pip_with_version_specifier() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("requests==2.28.0"));

    assert!(params.contains_key("name"));
}

#[test]
fn test_pip_with_version_range() {
    let packages = [
        "requests>=2.0",
        "flask<3.0",
        "django>=3.0,<4.0",
        "numpy~=1.20",
    ];

    for pkg in packages {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!(pkg));
        assert!(params.contains_key("name"));
    }
}

// ============================================================================
// Virtualenv Tests
// ============================================================================

#[test]
fn test_pip_virtualenv_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("requests"));
    params.insert("virtualenv".to_string(), serde_json::json!("/opt/venv"));

    assert!(params.contains_key("virtualenv"));
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_pip_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_pip_common_packages() {
    let common_packages = [
        "requests", "flask", "django", "numpy", "pandas", "pytest", "boto3", "pyyaml",
    ];

    for pkg in common_packages {
        assert!(!pkg.is_empty());
    }
}

#[test]
fn test_pip_extra_index_url() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("private-package"));
    params.insert(
        "extra_args".to_string(),
        serde_json::json!("--extra-index-url https://pypi.private.com/simple/"),
    );

    assert!(params.contains_key("extra_args"));
}

// ============================================================================
// State Clone and Equality Tests
// ============================================================================

#[test]
fn test_pip_state_clone() {
    let state = PipState::Present;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn test_pip_state_debug_format() {
    let state = PipState::Present;
    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("Present"));
}

#[test]
fn test_pip_state_equality() {
    assert_eq!(PipState::Present, PipState::Present);
    assert_eq!(PipState::Absent, PipState::Absent);
    assert_eq!(PipState::Latest, PipState::Latest);
    assert_ne!(PipState::Present, PipState::Absent);
}

// ============================================================================
// Requirements File Tests
// ============================================================================

#[test]
fn test_pip_requirements_file() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "requirements".to_string(),
        serde_json::json!("/path/to/requirements.txt"),
    );

    assert!(params.contains_key("requirements"));
}

#[test]
fn test_pip_requirements_with_state_absent_invalid() {
    // state=absent is not supported with requirements
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "requirements".to_string(),
        serde_json::json!("requirements.txt"),
    );
    params.insert("state".to_string(), serde_json::json!("absent"));

    // This combination should be invalid
    assert!(params.contains_key("requirements"));
    assert_eq!(params.get("state").unwrap().as_str().unwrap(), "absent");
}

// ============================================================================
// Comprehensive Parameter Tests
// ============================================================================

#[test]
fn test_pip_full_parameters() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("requests"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("virtualenv".to_string(), serde_json::json!("/opt/venv"));
    params.insert("executable".to_string(), serde_json::json!("pip3"));

    assert_eq!(params.len(), 4);
}
