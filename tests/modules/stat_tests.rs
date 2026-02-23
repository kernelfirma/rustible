//! Comprehensive unit tests for the Stat module
//!
//! Tests cover:
//! - File stat operations
//! - Directory stat
//! - Symlink handling
//! - Checksum calculation
//! - Check mode
//! - Edge cases

use rustible::modules::stat::StatModule;
use rustible::modules::{Module, ModuleClassification, ModuleContext, ModuleStatus};
use std::collections::HashMap;
use tempfile::TempDir;

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_stat_module_name() {
    let module = StatModule;
    assert_eq!(module.name(), "stat");
}

#[test]
fn test_stat_module_description() {
    let module = StatModule;
    assert!(!module.description().is_empty());
}

#[test]
fn test_stat_module_classification() {
    let module = StatModule;
    assert_eq!(
        module.classification(),
        ModuleClassification::RemoteCommand
    );
}

#[test]
fn test_stat_module_required_params() {
    let module = StatModule;
    let required = module.required_params();
    assert!(required.contains(&"path"));
}

// ============================================================================
// File Stat Tests
// ============================================================================

#[test]
fn test_stat_existing_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    std::fs::write(&path, "test content").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert_eq!(result.status, ModuleStatus::Ok);
    assert!(result.data.contains_key("stat"));

    let stat = &result.data["stat"];
    assert_eq!(stat["exists"], true);
    assert_eq!(stat["isreg"], true);
    assert_eq!(stat["isdir"], false);
    assert_eq!(stat["size"], 12); // "test content" is 12 bytes
}

#[test]
fn test_stat_nonexistent_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("nonexistent");

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert_eq!(result.status, ModuleStatus::Ok);
    assert!(result.data.contains_key("stat"));

    let stat = &result.data["stat"];
    assert_eq!(stat["exists"], false);
}

// ============================================================================
// Directory Stat Tests
// ============================================================================

#[test]
fn test_stat_directory() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testdir");
    std::fs::create_dir(&path).unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert_eq!(result.status, ModuleStatus::Ok);

    let stat = &result.data["stat"];
    assert_eq!(stat["exists"], true);
    assert_eq!(stat["isdir"], true);
    assert_eq!(stat["isreg"], false);
}

// ============================================================================
// Symlink Tests
// ============================================================================

#[test]
#[cfg(unix)]
fn test_stat_symlink() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");

    std::fs::write(&target, "content").unwrap();
    symlink(&target, &link).unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(link.to_str().unwrap()),
    );
    params.insert("follow".to_string(), serde_json::json!(false));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert_eq!(stat["exists"], true);
    assert_eq!(stat["islnk"], true);
}

#[test]
#[cfg(unix)]
fn test_stat_symlink_follow() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");

    std::fs::write(&target, "content").unwrap();
    symlink(&target, &link).unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(link.to_str().unwrap()),
    );
    params.insert("follow".to_string(), serde_json::json!(true));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert_eq!(stat["exists"], true);
    // When following, it should show as regular file, not symlink
    assert_eq!(stat["isreg"], true);
}

// ============================================================================
// Checksum Tests
// ============================================================================

#[test]
fn test_stat_with_checksum_sha1() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    std::fs::write(&path, "test content").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("checksum".to_string(), serde_json::json!(true));
    params.insert("checksum_algorithm".to_string(), serde_json::json!("sha1"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert!(stat.get("checksum").is_some());
}

#[test]
fn test_stat_with_checksum_sha256() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    std::fs::write(&path, "test content").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("checksum".to_string(), serde_json::json!(true));
    params.insert(
        "checksum_algorithm".to_string(),
        serde_json::json!("sha256"),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert!(stat.get("checksum").is_some());
}

#[test]
fn test_stat_with_checksum_md5() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    std::fs::write(&path, "test content").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    params.insert("checksum".to_string(), serde_json::json!(true));
    params.insert("checksum_algorithm".to_string(), serde_json::json!("md5"));

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert!(stat.get("checksum").is_some());
}

#[test]
fn test_stat_checksum_not_requested() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    std::fs::write(&path, "test content").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );
    // checksum not requested (default is false)

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    // Checksum should not be present
    assert!(stat.get("checksum").is_none());
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_stat_check_mode() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    std::fs::write(&path, "test content").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.check(&params, &context).unwrap();

    // Stat is read-only, so it should work the same in check mode
    assert_eq!(result.status, ModuleStatus::Ok);
    let stat = &result.data["stat"];
    assert_eq!(stat["exists"], true);
}

// ============================================================================
// File Attributes Tests
// ============================================================================

#[test]
#[cfg(unix)]
fn test_stat_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    std::fs::write(&path, "test").unwrap();

    // Set specific permissions
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert!(stat.get("mode").is_some());
}

#[test]
#[cfg(unix)]
fn test_stat_uid_gid() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("testfile");
    std::fs::write(&path, "test").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert!(stat.get("uid").is_some());
    assert!(stat.get("gid").is_some());
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_stat_missing_path_parameter() {
    let module = StatModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_stat_empty_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("empty");
    std::fs::write(&path, "").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert_eq!(stat["exists"], true);
    assert_eq!(stat["size"], 0);
}

#[test]
fn test_stat_special_characters_in_path() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("file with spaces");
    std::fs::write(&path, "test").unwrap();

    let module = StatModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    let stat = &result.data["stat"];
    assert_eq!(stat["exists"], true);
}
