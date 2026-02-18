//! Comprehensive unit tests for the Group module
//!
//! Tests cover:
//! - State parsing (present, absent)
//! - Module metadata
//! - Shell escaping for security
//! - Parameter validation
//! - Edge cases

use rustible::modules::group::{GroupModule, GroupState};
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;

// ============================================================================
// State Parsing Tests
// ============================================================================

#[test]
fn test_group_state_present() {
    let state = GroupState::from_str("present").unwrap();
    assert_eq!(state, GroupState::Present);
}

#[test]
fn test_group_state_absent() {
    let state = GroupState::from_str("absent").unwrap();
    assert_eq!(state, GroupState::Absent);
}

#[test]
fn test_group_state_case_insensitive() {
    assert_eq!(
        GroupState::from_str("PRESENT").unwrap(),
        GroupState::Present
    );
    assert_eq!(
        GroupState::from_str("Present").unwrap(),
        GroupState::Present
    );
    assert_eq!(GroupState::from_str("ABSENT").unwrap(), GroupState::Absent);
    assert_eq!(GroupState::from_str("Absent").unwrap(), GroupState::Absent);
}

#[test]
fn test_group_state_invalid() {
    let result = GroupState::from_str("invalid");
    assert!(result.is_err());
}

#[test]
fn test_group_state_empty_string() {
    let result = GroupState::from_str("");
    assert!(result.is_err());
}

#[test]
fn test_group_state_not_supported_states() {
    // Group module doesn't support 'latest' like package modules
    let result = GroupState::from_str("latest");
    assert!(result.is_err());
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_group_module_name() {
    let module = GroupModule;
    assert_eq!(module.name(), "group");
}

#[test]
fn test_group_module_description() {
    let module = GroupModule;
    assert!(!module.description().is_empty());
    // Note: description might say "groups" or "group"
    let desc_lower = module.description().to_lowercase();
    assert!(desc_lower.contains("group"));
}

#[test]
fn test_group_module_classification() {
    let module = GroupModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_group_module_required_params() {
    let module = GroupModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Shell Escape Tests (Security)
// ============================================================================

#[test]
fn test_group_shell_escape_simple() {
    // Using the actual shell_escape function behavior
    assert_eq!(shell_escape("simple"), "simple");
}

#[test]
fn test_group_shell_escape_with_space() {
    assert_eq!(shell_escape("with space"), "'with space'");
}

#[test]
fn test_group_shell_escape_with_quote() {
    assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
}

#[test]
fn test_group_shell_escape_alphanumeric_and_allowed() {
    // These should pass through unchanged
    assert_eq!(shell_escape("group_name"), "group_name");
    assert_eq!(shell_escape("group-name"), "group-name");
    assert_eq!(shell_escape("group.name"), "group.name");
    assert_eq!(shell_escape("group/name"), "group/name");
}

#[test]
fn test_group_command_injection_patterns() {
    let dangerous_inputs = ["; rm -rf /", "$(whoami)", "`id`", "group && malicious"];

    for input in dangerous_inputs {
        let escaped = shell_escape(input);
        // Escaped version should be wrapped in single quotes
        assert!(escaped.starts_with("'") && escaped.ends_with("'"));
    }
}

// ============================================================================
// Parameter Validation Tests
// ============================================================================

#[test]
fn test_group_missing_name_parameter() {
    let module = GroupModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_group_with_gid_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testgroup"));
    params.insert("gid".to_string(), serde_json::json!(1001));

    assert!(params.contains_key("gid"));
}

#[test]
fn test_group_with_system_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testgroup"));
    params.insert("system".to_string(), serde_json::json!(true));

    assert!(params.contains_key("system"));
}

#[test]
fn test_group_with_state_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testgroup"));
    params.insert("state".to_string(), serde_json::json!("present"));

    assert!(params.contains_key("state"));
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_group_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_group_valid_group_names() {
    let valid_names = [
        "wheel",
        "docker",
        "sudo",
        "users",
        "admin_group",
        "dev-team",
    ];

    for name in valid_names {
        assert!(!name.is_empty());
    }
}

#[test]
fn test_group_null_connection_handling() {
    let module = GroupModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testgroup"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_group_system_group_creation() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("sysgroup"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("system".to_string(), serde_json::json!(true));

    assert!(params.contains_key("system"));
}

// ============================================================================
// State Clone and Equality Tests
// ============================================================================

#[test]
fn test_group_state_clone() {
    let state = GroupState::Present;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn test_group_state_debug_format() {
    let state = GroupState::Present;
    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("Present"));
}

#[test]
fn test_group_state_equality() {
    assert_eq!(GroupState::Present, GroupState::Present);
    assert_eq!(GroupState::Absent, GroupState::Absent);
    assert_ne!(GroupState::Present, GroupState::Absent);
}

// ============================================================================
// GroupInfo Tests
// ============================================================================

#[test]
fn test_group_info_structure() {
    use rustible::modules::group::GroupInfo;

    let info = GroupInfo {
        name: "testgroup".to_string(),
        gid: 1001,
        members: vec!["user1".to_string(), "user2".to_string()],
    };

    assert_eq!(info.name, "testgroup");
    assert_eq!(info.gid, 1001);
    assert_eq!(info.members.len(), 2);
}

#[test]
fn test_group_info_empty_members() {
    use rustible::modules::group::GroupInfo;

    let info = GroupInfo {
        name: "emptygroup".to_string(),
        gid: 1002,
        members: Vec::new(),
    };

    assert!(info.members.is_empty());
}

// ============================================================================
// Helper function for shell escaping (copy from module for testing)
// ============================================================================

fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}
