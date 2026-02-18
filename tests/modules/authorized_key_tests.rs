//! Comprehensive unit tests for the Authorized Key module
//!
//! Tests cover:
//! - SSH key parsing (RSA, ED25519, ECDSA, security keys)
//! - Key options parsing and validation
//! - State management (present, absent)
//! - Add/remove key functionality
//! - Module metadata and parameter validation
//! - Edge cases and security

use rustible::modules::authorized_key::{
    parse_key_options, validate_ssh_key, AuthorizedKey, AuthorizedKeyModule, KeyState,
};
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;

// ============================================================================
// Test Constants - Sample SSH Keys
// ============================================================================

const TEST_RSA_KEY: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7test user@example.com";
const TEST_RSA_KEY_NO_COMMENT: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7test";
const TEST_ED25519_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAItest user@example.com";
const TEST_ECDSA_256_KEY: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY= test";
const TEST_ECDSA_384_KEY: &str = "ecdsa-sha2-nistp384 AAAAE2VjZHNhLXNoYTItbmlzdHAzODQ= test";
const TEST_ECDSA_521_KEY: &str = "ecdsa-sha2-nistp521 AAAAE2VjZHNhLXNoYTItbmlzdHA1MjE= test";
const TEST_SK_ED25519_KEY: &str =
    "sk-ssh-ed25519@openssh.com AAAAGnNrLXNzaC1lZDI1NTE5QG9wZW5zc2guY29t user";
const TEST_SK_ECDSA_KEY: &str =
    "sk-ecdsa-sha2-nistp256@openssh.com AAAAInNrLWVjZHNhLXNoYTItbmlzdHAyNTZAb3BlbnNzaC5jb20= user";

// ============================================================================
// SSH Key Parsing Tests
// ============================================================================

#[test]
fn test_parse_simple_rsa_key() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    assert_eq!(key.key_type, "ssh-rsa");
    assert_eq!(key.key_data, "AAAAB3NzaC1yc2EAAAADAQABAAABgQC7test");
    assert_eq!(key.comment, Some("user@example.com".to_string()));
    assert!(key.options.is_none());
}

#[test]
fn test_parse_rsa_key_without_comment() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY_NO_COMMENT).unwrap();
    assert_eq!(key.key_type, "ssh-rsa");
    assert_eq!(key.key_data, "AAAAB3NzaC1yc2EAAAADAQABAAABgQC7test");
    assert!(key.comment.is_none());
}

#[test]
fn test_parse_ed25519_key() {
    let key = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();
    assert_eq!(key.key_type, "ssh-ed25519");
    assert_eq!(key.key_data, "AAAAC3NzaC1lZDI1NTE5AAAAItest");
    assert_eq!(key.comment, Some("user@example.com".to_string()));
}

#[test]
fn test_parse_ecdsa_nistp256_key() {
    let key = AuthorizedKey::parse(TEST_ECDSA_256_KEY).unwrap();
    assert_eq!(key.key_type, "ecdsa-sha2-nistp256");
    assert_eq!(key.key_data, "AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY=");
}

#[test]
fn test_parse_ecdsa_nistp384_key() {
    let key = AuthorizedKey::parse(TEST_ECDSA_384_KEY).unwrap();
    assert_eq!(key.key_type, "ecdsa-sha2-nistp384");
}

#[test]
fn test_parse_ecdsa_nistp521_key() {
    let key = AuthorizedKey::parse(TEST_ECDSA_521_KEY).unwrap();
    assert_eq!(key.key_type, "ecdsa-sha2-nistp521");
}

#[test]
fn test_parse_sk_ed25519_key() {
    let key = AuthorizedKey::parse(TEST_SK_ED25519_KEY).unwrap();
    assert_eq!(key.key_type, "sk-ssh-ed25519@openssh.com");
}

#[test]
fn test_parse_sk_ecdsa_key() {
    let key = AuthorizedKey::parse(TEST_SK_ECDSA_KEY).unwrap();
    assert_eq!(key.key_type, "sk-ecdsa-sha2-nistp256@openssh.com");
}

// ============================================================================
// Key with Options Tests
// ============================================================================

#[test]
fn test_parse_key_with_no_pty_option() {
    let key_str = format!("no-pty {}", TEST_RSA_KEY);
    let key = AuthorizedKey::parse(&key_str).unwrap();
    assert_eq!(key.options, Some("no-pty".to_string()));
    assert_eq!(key.key_type, "ssh-rsa");
}

#[test]
fn test_parse_key_with_command_option() {
    let key_str = format!(r#"command="/bin/date" {}"#, TEST_ED25519_KEY);
    let key = AuthorizedKey::parse(&key_str).unwrap();
    assert_eq!(key.options, Some(r#"command="/bin/date""#.to_string()));
    assert_eq!(key.key_type, "ssh-ed25519");
}

#[test]
fn test_parse_key_with_from_option() {
    let key_str = format!(r#"from="192.168.1.0/24" {}"#, TEST_RSA_KEY);
    let key = AuthorizedKey::parse(&key_str).unwrap();
    assert_eq!(key.options, Some(r#"from="192.168.1.0/24""#.to_string()));
}

#[test]
fn test_parse_key_with_multiple_options() {
    let key_str = format!(
        r#"command="/bin/date",no-pty,no-agent-forwarding {}"#,
        TEST_RSA_KEY
    );
    let key = AuthorizedKey::parse(&key_str).unwrap();
    assert_eq!(
        key.options,
        Some(r#"command="/bin/date",no-pty,no-agent-forwarding"#.to_string())
    );
}

#[test]
fn test_parse_key_with_environment_option() {
    let key_str = format!(r#"environment="PATH=/usr/bin" {}"#, TEST_RSA_KEY);
    let key = AuthorizedKey::parse(&key_str).unwrap();
    assert!(key.options.is_some());
    assert!(key.options.unwrap().contains("environment"));
}

#[test]
fn test_parse_key_with_complex_from_option() {
    let key_str = format!(
        r#"from="10.0.0.0/8,!10.0.0.1,*.example.com" {}"#,
        TEST_ED25519_KEY
    );
    let key = AuthorizedKey::parse(&key_str).unwrap();
    assert!(key.options.is_some());
}

// ============================================================================
// Key to_line and Formatting Tests
// ============================================================================

#[test]
fn test_key_to_line_without_options() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let line = key.to_line();
    assert!(line.contains("ssh-rsa"));
    assert!(line.contains("AAAAB3NzaC1yc2EAAAADAQABAAABgQC7test"));
    assert!(line.contains("user@example.com"));
}

#[test]
fn test_key_to_line_with_options() {
    let key = AuthorizedKey {
        options: Some("no-pty,no-x11-forwarding".to_string()),
        key_type: "ssh-rsa".to_string(),
        key_data: "AAAAB3NzaC1yc2EAAAADAQABtest".to_string(),
        comment: Some("test@host".to_string()),
    };
    let line = key.to_line();
    assert!(line.starts_with("no-pty,no-x11-forwarding"));
    assert!(line.contains("ssh-rsa"));
    assert!(line.contains("test@host"));
}

#[test]
fn test_key_to_line_without_comment() {
    let key = AuthorizedKey {
        options: None,
        key_type: "ssh-ed25519".to_string(),
        key_data: "AAAAC3NzaC1lZDI1NTE5test".to_string(),
        comment: None,
    };
    let line = key.to_line();
    assert_eq!(line, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test");
}

// ============================================================================
// same_key Comparison Tests
// ============================================================================

#[test]
fn test_same_key_identical() {
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    assert!(key1.same_key(&key2));
}

#[test]
fn test_same_key_different_comment() {
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 =
        AuthorizedKey::parse(&TEST_RSA_KEY.replace("user@example.com", "other@host")).unwrap();
    assert!(key1.same_key(&key2));
}

#[test]
fn test_same_key_different_options() {
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 = AuthorizedKey::parse(&format!("no-pty {}", TEST_RSA_KEY)).unwrap();
    assert!(key1.same_key(&key2));
}

#[test]
fn test_different_keys() {
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();
    assert!(!key1.same_key(&key2));
}

#[test]
fn test_different_key_types_same_data_length() {
    let rsa_key = AuthorizedKey::parse("ssh-rsa AAAA user").unwrap();
    let ed_key = AuthorizedKey::parse("ssh-ed25519 AAAA user").unwrap();
    assert!(!rsa_key.same_key(&ed_key));
}

// ============================================================================
// with_options and with_comment Tests
// ============================================================================

#[test]
fn test_with_options() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key_with_opts = key.with_options(Some("no-pty".to_string()));
    assert_eq!(key_with_opts.options, Some("no-pty".to_string()));
}

#[test]
fn test_with_options_replace_existing() {
    let key = AuthorizedKey::parse(&format!("no-pty {}", TEST_RSA_KEY)).unwrap();
    let key_with_opts = key.with_options(Some("no-x11-forwarding".to_string()));
    assert_eq!(key_with_opts.options, Some("no-x11-forwarding".to_string()));
}

#[test]
fn test_with_options_remove() {
    let key = AuthorizedKey::parse(&format!("no-pty {}", TEST_RSA_KEY)).unwrap();
    let key_without_opts = key.with_options(None);
    assert!(key_without_opts.options.is_none());
}

#[test]
fn test_with_comment() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key_with_comment = key.with_comment(Some("new@comment".to_string()));
    assert_eq!(key_with_comment.comment, Some("new@comment".to_string()));
}

#[test]
fn test_with_comment_remove() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key_without_comment = key.with_comment(None);
    assert!(key_without_comment.comment.is_none());
}

// ============================================================================
// validate_ssh_key Tests
// ============================================================================

#[test]
fn test_validate_ssh_key_rsa() {
    assert!(validate_ssh_key(TEST_RSA_KEY).is_ok());
}

#[test]
fn test_validate_ssh_key_ed25519() {
    assert!(validate_ssh_key(TEST_ED25519_KEY).is_ok());
}

#[test]
fn test_validate_ssh_key_ecdsa() {
    assert!(validate_ssh_key(TEST_ECDSA_256_KEY).is_ok());
    assert!(validate_ssh_key(TEST_ECDSA_384_KEY).is_ok());
    assert!(validate_ssh_key(TEST_ECDSA_521_KEY).is_ok());
}

#[test]
fn test_validate_ssh_key_security_keys() {
    assert!(validate_ssh_key(TEST_SK_ED25519_KEY).is_ok());
    assert!(validate_ssh_key(TEST_SK_ECDSA_KEY).is_ok());
}

#[test]
fn test_validate_ssh_key_empty() {
    assert!(validate_ssh_key("").is_err());
}

#[test]
fn test_validate_ssh_key_whitespace_only() {
    assert!(validate_ssh_key("   ").is_err());
}

#[test]
fn test_validate_ssh_key_invalid_format() {
    assert!(validate_ssh_key("not a valid key").is_err());
    assert!(validate_ssh_key("invalid-type AAAA user").is_err());
    assert!(validate_ssh_key("ssh-rsa").is_err()); // No key data
}

#[test]
fn test_validate_ssh_key_with_leading_trailing_whitespace() {
    let key_with_space = format!("  {}  ", TEST_RSA_KEY);
    assert!(validate_ssh_key(&key_with_space).is_ok());
}

// ============================================================================
// parse_key_options Tests
// ============================================================================

#[test]
fn test_parse_key_options_simple() {
    assert_eq!(parse_key_options("no-pty").unwrap(), "no-pty");
}

#[test]
fn test_parse_key_options_multiple() {
    let opts = "no-pty,no-agent-forwarding,no-x11-forwarding";
    assert_eq!(parse_key_options(opts).unwrap(), opts);
}

#[test]
fn test_parse_key_options_with_command() {
    let opts = r#"command="/bin/date""#;
    assert_eq!(parse_key_options(opts).unwrap(), opts);
}

#[test]
fn test_parse_key_options_with_from() {
    let opts = r#"from="10.0.0.0/8,192.168.1.0/24""#;
    assert_eq!(parse_key_options(opts).unwrap(), opts);
}

#[test]
fn test_parse_key_options_empty() {
    assert_eq!(parse_key_options("").unwrap(), "");
}

#[test]
fn test_parse_key_options_whitespace() {
    assert_eq!(parse_key_options("  ").unwrap(), "");
}

#[test]
fn test_parse_key_options_with_newlines() {
    assert!(parse_key_options("option\nwith\nnewlines").is_err());
}

#[test]
fn test_parse_key_options_with_carriage_return() {
    assert!(parse_key_options("option\rwith\rcr").is_err());
}

#[test]
fn test_parse_key_options_unbalanced_quotes() {
    assert!(parse_key_options(r#"command="/bin/date"#).is_err());
    assert!(parse_key_options(r#"from="10.0.0.0/8"#).is_err());
}

#[test]
fn test_parse_key_options_balanced_quotes() {
    assert!(parse_key_options(r#"command="/bin/date""#).is_ok());
    assert!(parse_key_options(r#"from="10.0.0.0/8",command="/bin/ls""#).is_ok());
}

// ============================================================================
// KeyState Tests
// ============================================================================

#[test]
fn test_key_state_from_str_present() {
    assert_eq!(KeyState::from_str("present").unwrap(), KeyState::Present);
}

#[test]
fn test_key_state_from_str_absent() {
    assert_eq!(KeyState::from_str("absent").unwrap(), KeyState::Absent);
}

#[test]
fn test_key_state_case_insensitive() {
    assert_eq!(KeyState::from_str("PRESENT").unwrap(), KeyState::Present);
    assert_eq!(KeyState::from_str("Present").unwrap(), KeyState::Present);
    assert_eq!(KeyState::from_str("ABSENT").unwrap(), KeyState::Absent);
    assert_eq!(KeyState::from_str("Absent").unwrap(), KeyState::Absent);
}

#[test]
fn test_key_state_invalid() {
    assert!(KeyState::from_str("invalid").is_err());
    assert!(KeyState::from_str("latest").is_err());
    assert!(KeyState::from_str("").is_err());
}

#[test]
fn test_key_state_display() {
    assert_eq!(format!("{}", KeyState::Present), "present");
    assert_eq!(format!("{}", KeyState::Absent), "absent");
}

#[test]
fn test_key_state_clone() {
    let state = KeyState::Present;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn test_key_state_debug() {
    let debug_str = format!("{:?}", KeyState::Present);
    assert!(debug_str.contains("Present"));
}

// ============================================================================
// Add/Remove Key Functionality Tests
// ============================================================================

#[test]
fn test_add_key_to_empty_list() {
    let mut keys = Vec::new();
    let new_key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();

    let changed = add_key_helper(&mut keys, &new_key);

    assert!(changed);
    assert_eq!(keys.len(), 1);
}

#[test]
fn test_add_duplicate_key() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let mut keys = vec![key.clone()];

    let changed = add_key_helper(&mut keys, &key);

    assert!(!changed);
    assert_eq!(keys.len(), 1);
}

#[test]
fn test_add_key_with_different_comment() {
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 =
        AuthorizedKey::parse(&TEST_RSA_KEY.replace("user@example.com", "other@host")).unwrap();
    let mut keys = vec![key1];

    let changed = add_key_helper(&mut keys, &key2);

    // Should update comment
    assert!(changed);
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].comment, Some("other@host".to_string()));
}

#[test]
fn test_add_key_update_options() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let mut keys = vec![key.clone()];

    let new_key = key.with_options(Some("no-pty".to_string()));
    let changed = add_key_helper(&mut keys, &new_key);

    assert!(changed);
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].options, Some("no-pty".to_string()));
}

#[test]
fn test_add_multiple_different_keys() {
    let mut keys = Vec::new();
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();

    add_key_helper(&mut keys, &key1);
    add_key_helper(&mut keys, &key2);

    assert_eq!(keys.len(), 2);
}

#[test]
fn test_remove_key_exists() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let mut keys = vec![key.clone()];

    let changed = remove_key_helper(&mut keys, &key);

    assert!(changed);
    assert!(keys.is_empty());
}

#[test]
fn test_remove_key_not_found() {
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();
    let mut keys = vec![key1];

    let changed = remove_key_helper(&mut keys, &key2);

    assert!(!changed);
    assert_eq!(keys.len(), 1);
}

#[test]
fn test_remove_key_from_multiple() {
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();
    let mut keys = vec![key1.clone(), key2];

    let changed = remove_key_helper(&mut keys, &key1);

    assert!(changed);
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].key_type, "ssh-ed25519");
}

#[test]
fn test_remove_key_matches_by_data_not_options() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key_with_opts = AuthorizedKey::parse(&format!("no-pty {}", TEST_RSA_KEY)).unwrap();
    let mut keys = vec![key_with_opts];

    let changed = remove_key_helper(&mut keys, &key);

    assert!(changed);
    assert!(keys.is_empty());
}

// Helper functions that mirror the module's internal logic
fn add_key_helper(existing_keys: &mut Vec<AuthorizedKey>, new_key: &AuthorizedKey) -> bool {
    for key in existing_keys.iter_mut() {
        if key.same_key(new_key) {
            if key.options != new_key.options || key.comment != new_key.comment {
                key.options = new_key.options.clone();
                key.comment = new_key.comment.clone();
                return true;
            }
            return false;
        }
    }
    existing_keys.push(new_key.clone());
    true
}

fn remove_key_helper(
    existing_keys: &mut Vec<AuthorizedKey>,
    key_to_remove: &AuthorizedKey,
) -> bool {
    let original_len = existing_keys.len();
    existing_keys.retain(|k| !k.same_key(key_to_remove));
    existing_keys.len() != original_len
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_module_name() {
    let module = AuthorizedKeyModule;
    assert_eq!(module.name(), "authorized_key");
}

#[test]
fn test_module_description() {
    let module = AuthorizedKeyModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("authorized"));
}

#[test]
fn test_module_classification() {
    let module = AuthorizedKeyModule;
    assert_eq!(
        module.classification(),
        ModuleClassification::NativeTransport
    );
}

#[test]
fn test_module_required_params() {
    let module = AuthorizedKeyModule;
    let required = module.required_params();
    assert!(required.contains(&"user"));
    assert!(required.contains(&"key"));
}

// ============================================================================
// Parameter Validation Tests
// ============================================================================

#[test]
fn test_validate_params_valid() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("testuser"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

    assert!(module.validate_params(&params).is_ok());
}

#[test]
fn test_validate_params_missing_user() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

    assert!(module.validate_params(&params).is_err());
}

#[test]
fn test_validate_params_missing_key() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("testuser"));

    assert!(module.validate_params(&params).is_err());
}

#[test]
fn test_validate_params_empty_user() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!(""));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

    assert!(module.validate_params(&params).is_err());
}

#[test]
fn test_validate_params_empty_key() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("testuser"));
    params.insert("key".to_string(), serde_json::json!(""));

    assert!(module.validate_params(&params).is_err());
}

#[test]
fn test_validate_params_invalid_user_injection() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("user; rm -rf /"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

    assert!(module.validate_params(&params).is_err());
}

#[test]
fn test_validate_params_invalid_user_command_substitution() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("$(whoami)"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

    assert!(module.validate_params(&params).is_err());
}

#[test]
fn test_validate_params_invalid_user_backticks() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("`id`"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

    assert!(module.validate_params(&params).is_err());
}

#[test]
fn test_validate_params_valid_user_with_underscore() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("test_user"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

    assert!(module.validate_params(&params).is_ok());
}

#[test]
fn test_validate_params_valid_user_with_hyphen() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("test-user"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

    assert!(module.validate_params(&params).is_ok());
}

#[test]
fn test_validate_params_invalid_state() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("testuser"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));
    params.insert("state".to_string(), serde_json::json!("invalid"));

    assert!(module.validate_params(&params).is_err());
}

#[test]
fn test_validate_params_valid_state_present() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("testuser"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));
    params.insert("state".to_string(), serde_json::json!("present"));

    assert!(module.validate_params(&params).is_ok());
}

#[test]
fn test_validate_params_valid_state_absent() {
    let module = AuthorizedKeyModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("user".to_string(), serde_json::json!("testuser"));
    params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));
    params.insert("state".to_string(), serde_json::json!("absent"));

    assert!(module.validate_params(&params).is_ok());
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_parse_key_with_long_comment() {
    let long_comment = "a".repeat(500);
    let key_str = format!("ssh-rsa AAAAB3NzaC1yc2EAAAADAQABtest {}", long_comment);
    let key = AuthorizedKey::parse(&key_str).unwrap();
    assert_eq!(key.comment, Some(long_comment));
}

#[test]
fn test_parse_key_with_special_comment_chars() {
    let key_str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABtest user+tag@example.com";
    let key = AuthorizedKey::parse(key_str).unwrap();
    assert_eq!(key.comment, Some("user+tag@example.com".to_string()));
}

#[test]
fn test_parse_key_with_unicode_comment() {
    let key_str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABtest usuario@ejemplo.com";
    let key = AuthorizedKey::parse(key_str).unwrap();
    assert!(key.comment.is_some());
}

#[test]
fn test_parse_keys_from_file_format() {
    let lines = ["# This is a comment".to_string(),
        "".to_string(),
        TEST_RSA_KEY.to_string(),
        "# Another comment".to_string(),
        TEST_ED25519_KEY.to_string()];

    let keys: Vec<_> = lines
        .iter()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
        .filter_map(|line| AuthorizedKey::parse(line).ok())
        .collect();

    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].key_type, "ssh-rsa");
    assert_eq!(keys[1].key_type, "ssh-ed25519");
}

// ============================================================================
// Clone and Equality Tests
// ============================================================================

#[test]
fn test_authorized_key_clone() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let cloned = key.clone();
    assert_eq!(key.key_type, cloned.key_type);
    assert_eq!(key.key_data, cloned.key_data);
    assert_eq!(key.comment, cloned.comment);
    assert_eq!(key.options, cloned.options);
}

#[test]
fn test_authorized_key_debug() {
    let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let debug_str = format!("{:?}", key);
    assert!(debug_str.contains("ssh-rsa"));
}

#[test]
fn test_authorized_key_partial_eq() {
    let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key2 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
    let key3 = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();

    assert_eq!(key1, key2);
    assert_ne!(key1, key3);
}
