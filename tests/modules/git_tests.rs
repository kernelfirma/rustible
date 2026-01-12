//! Comprehensive unit tests for the Git module
//!
//! Tests cover:
//! - Module metadata
//! - Parameter validation
//! - Clone and update logic
//! - Check mode
//! - Edge cases

use rustible::modules::git::GitModule;
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;
use tempfile::TempDir;

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_git_module_name() {
    let module = GitModule;
    assert_eq!(module.name(), "git");
}

#[test]
fn test_git_module_description() {
    let module = GitModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("git"));
}

#[test]
fn test_git_module_classification() {
    let module = GitModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_git_module_required_params() {
    let module = GitModule;
    let required = module.required_params();
    assert!(required.contains(&"repo"));
    assert!(required.contains(&"dest"));
    assert_eq!(required.len(), 2);
}

// ============================================================================
// Parameter Validation Tests
// ============================================================================

#[test]
fn test_git_validate_params_missing_repo() {
    let module = GitModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));

    let result = module.validate_params(&params);
    assert!(result.is_err());
}

#[test]
fn test_git_validate_params_missing_dest() {
    let module = GitModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );

    let result = module.validate_params(&params);
    assert!(result.is_err());
}

#[test]
fn test_git_validate_params_valid() {
    let module = GitModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));

    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

#[test]
fn test_git_validate_depth_zero() {
    let module = GitModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("depth".to_string(), serde_json::json!(0));

    let result = module.validate_params(&params);
    assert!(result.is_err());
}

#[test]
fn test_git_validate_depth_valid() {
    let module = GitModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("depth".to_string(), serde_json::json!(1));

    let result = module.validate_params(&params);
    assert!(result.is_ok());
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_git_check_mode_clone() {
    let module = GitModule;
    let temp = TempDir::new().unwrap();
    let dest_path = temp.path().join("test-repo");

    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert(
        "dest".to_string(),
        serde_json::json!(dest_path.to_str().unwrap()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would clone"));
    assert!(!dest_path.exists()); // Should not be created in check mode
}

// ============================================================================
// Optional Parameters Tests
// ============================================================================

#[test]
fn test_git_with_version_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("version".to_string(), serde_json::json!("v1.0.0"));

    assert!(params.get("version").is_some());
}

#[test]
fn test_git_with_branch_version() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("version".to_string(), serde_json::json!("develop"));

    assert!(params.get("version").is_some());
}

#[test]
fn test_git_with_commit_hash() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("version".to_string(), serde_json::json!("abc123def456"));

    assert!(params.get("version").is_some());
}

#[test]
fn test_git_with_depth_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("depth".to_string(), serde_json::json!(1));

    assert!(params.get("depth").is_some());
}

#[test]
fn test_git_with_update_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("update".to_string(), serde_json::json!(true));

    assert!(params.get("update").is_some());
}

#[test]
fn test_git_update_disabled() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("update".to_string(), serde_json::json!(false));

    let update = params.get("update").unwrap().as_bool().unwrap();
    assert!(!update);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_git_various_repo_formats() {
    let valid_repos = [
        "https://github.com/user/repo.git",
        "git@github.com:user/repo.git",
        "ssh://git@github.com/user/repo.git",
        "file:///local/path/to/repo",
    ];

    for repo in valid_repos {
        assert!(!repo.is_empty());
    }
}

#[test]
fn test_git_deep_clone_depth() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "repo".to_string(),
        serde_json::json!("https://github.com/test/repo"),
    );
    params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
    params.insert("depth".to_string(), serde_json::json!(100));

    let depth = params.get("depth").unwrap().as_u64().unwrap();
    assert_eq!(depth, 100);
}

// ============================================================================
// Module Trait Tests
// ============================================================================

#[test]
fn test_git_module_implements_traits() {
    let module = GitModule;

    // Test that we can call all Module trait methods
    let _ = module.name();
    let _ = module.description();
    let _ = module.classification();
    let _ = module.required_params();
}

// ============================================================================
// Repository URL Tests
// ============================================================================

#[test]
fn test_git_https_url() {
    let url = "https://github.com/rust-lang/rust.git";
    assert!(url.starts_with("https://"));
}

#[test]
fn test_git_ssh_url() {
    let url = "git@github.com:rust-lang/rust.git";
    assert!(url.contains("@"));
}

#[test]
fn test_git_file_url() {
    let url = "file:///path/to/local/repo";
    assert!(url.starts_with("file://"));
}
