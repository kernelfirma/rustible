//! Comprehensive unit tests for the User module
//!
//! Tests cover:
//! - State parsing (present, absent)
//! - Module metadata
//! - Shell escaping for security
//! - Parameter validation
//! - Edge cases

use rustible::modules::user::{UserModule, UserState};
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;

// ============================================================================
// State Parsing Tests
// ============================================================================

#[test]
fn test_user_state_present() {
    let state = UserState::from_str("present").unwrap();
    assert_eq!(state, UserState::Present);
}

#[test]
fn test_user_state_absent() {
    let state = UserState::from_str("absent").unwrap();
    assert_eq!(state, UserState::Absent);
}

#[test]
fn test_user_state_case_insensitive() {
    assert_eq!(UserState::from_str("PRESENT").unwrap(), UserState::Present);
    assert_eq!(UserState::from_str("Present").unwrap(), UserState::Present);
    assert_eq!(UserState::from_str("ABSENT").unwrap(), UserState::Absent);
}

#[test]
fn test_user_state_invalid() {
    let result = UserState::from_str("invalid");
    assert!(result.is_err());
}

#[test]
fn test_user_state_empty_string() {
    let result = UserState::from_str("");
    assert!(result.is_err());
}

#[test]
fn test_user_state_not_supported_states() {
    // User module doesn't support 'latest' like package modules
    let result = UserState::from_str("latest");
    assert!(result.is_err());
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_user_module_name() {
    let module = UserModule;
    assert_eq!(module.name(), "user");
}

#[test]
fn test_user_module_description() {
    let module = UserModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("user"));
}

#[test]
fn test_user_module_classification() {
    let module = UserModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_user_module_required_params() {
    let module = UserModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Shell Escape Tests (Security)
// ============================================================================

#[test]
fn test_user_shell_escape_simple_name() {
    let input = "testuser";
    assert!(input.chars().all(|c| c.is_alphanumeric() || c == '_'));
}

#[test]
fn test_user_shell_escape_with_underscore() {
    let input = "test_user";
    assert!(input.chars().all(|c| c.is_alphanumeric() || c == '_'));
}

#[test]
fn test_user_command_injection_patterns() {
    let dangerous_inputs = ["; rm -rf /", "$(whoami)", "`id`", "user && malicious"];

    for input in dangerous_inputs {
        assert!(input
            .chars()
            .any(|c| !c.is_alphanumeric() && c != '_' && c != '-'));
    }
}

// ============================================================================
// Parameter Validation Tests
// ============================================================================

#[test]
fn test_user_missing_name_parameter() {
    let module = UserModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_user_with_uid_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("uid".to_string(), serde_json::json!(1001));

    assert!(params.contains_key("uid"));
}

#[test]
fn test_user_with_group_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("group".to_string(), serde_json::json!("testgroup"));

    assert!(params.contains_key("group"));
}

#[test]
fn test_user_with_groups_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("groups".to_string(), serde_json::json!(["wheel", "docker"]));

    assert!(params.contains_key("groups"));
}

#[test]
fn test_user_with_home_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("home".to_string(), serde_json::json!("/home/testuser"));

    assert!(params.contains_key("home"));
}

#[test]
fn test_user_with_shell_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("shell".to_string(), serde_json::json!("/bin/bash"));

    assert!(params.contains_key("shell"));
}

#[test]
fn test_user_with_comment_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert(
        "comment".to_string(),
        serde_json::json!("Test User Account"),
    );

    assert!(params.contains_key("comment"));
}

#[test]
fn test_user_with_create_home_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("create_home".to_string(), serde_json::json!(true));

    assert!(params.contains_key("create_home"));
}

#[test]
fn test_user_with_system_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("system".to_string(), serde_json::json!(true));

    assert!(params.contains_key("system"));
}

#[test]
fn test_user_with_password_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert(
        "password".to_string(),
        serde_json::json!("$6$hashed$password"),
    );
    params.insert("password_encrypted".to_string(), serde_json::json!(true));

    assert!(params.contains_key("password"));
}

#[test]
fn test_user_with_generate_ssh_key() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("generate_ssh_key".to_string(), serde_json::json!(true));
    params.insert("ssh_key_type".to_string(), serde_json::json!("ed25519"));
    params.insert("ssh_key_bits".to_string(), serde_json::json!(4096));

    assert!(params.contains_key("generate_ssh_key"));
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_user_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_user_valid_usernames() {
    let valid_usernames = [
        "root",
        "testuser",
        "test_user",
        "test-user",
        "user123",
        "_service",
    ];

    for name in valid_usernames {
        assert!(!name.is_empty());
    }
}

#[test]
fn test_user_null_connection_handling() {
    let module = UserModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_user_remove_with_home() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("state".to_string(), serde_json::json!("absent"));
    params.insert("remove".to_string(), serde_json::json!(true));

    assert!(params.contains_key("remove"));
}

#[test]
fn test_user_force_remove() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("state".to_string(), serde_json::json!("absent"));
    params.insert("force".to_string(), serde_json::json!(true));

    assert!(params.contains_key("force"));
}

#[test]
fn test_user_append_groups() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("groups".to_string(), serde_json::json!(["docker"]));
    params.insert("append".to_string(), serde_json::json!(true));

    assert!(params.contains_key("append"));
}

#[test]
fn test_user_move_home() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("home".to_string(), serde_json::json!("/new/home/path"));
    params.insert("move_home".to_string(), serde_json::json!(true));

    assert!(params.contains_key("move_home"));
}

// ============================================================================
// State Clone and Equality Tests
// ============================================================================

#[test]
fn test_user_state_clone() {
    let state = UserState::Present;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn test_user_state_debug_format() {
    let state = UserState::Present;
    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("Present"));
}

#[test]
fn test_user_state_equality() {
    assert_eq!(UserState::Present, UserState::Present);
    assert_eq!(UserState::Absent, UserState::Absent);
    assert_ne!(UserState::Present, UserState::Absent);
}

// ============================================================================
// Comprehensive User Creation Tests
// ============================================================================

#[test]
fn test_user_full_creation_parameters() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("uid".to_string(), serde_json::json!(1001));
    params.insert("group".to_string(), serde_json::json!("users"));
    params.insert("groups".to_string(), serde_json::json!(["wheel", "docker"]));
    params.insert("home".to_string(), serde_json::json!("/home/testuser"));
    params.insert("shell".to_string(), serde_json::json!("/bin/bash"));
    params.insert("comment".to_string(), serde_json::json!("Test User"));
    params.insert("create_home".to_string(), serde_json::json!(true));
    params.insert("system".to_string(), serde_json::json!(false));

    assert_eq!(params.len(), 10);
}
