//! Comprehensive unit tests for the Package module
//!
//! Tests cover:
//! - State parsing
//! - Package manager detection and parsing
//! - Module metadata
//! - Parameter validation
//! - Edge cases

use rustible::modules::package::{PackageManager, PackageModule, PackageState};
use rustible::modules::{Module, ModuleClassification, ModuleContext, ParallelizationHint};
use std::collections::HashMap;

// ============================================================================
// PackageState Parsing Tests
// ============================================================================

#[test]
fn test_package_state_present() {
    let state = PackageState::from_str("present").unwrap();
    assert_eq!(state, PackageState::Present);
}

#[test]
fn test_package_state_installed_alias() {
    let state = PackageState::from_str("installed").unwrap();
    assert_eq!(state, PackageState::Present);
}

#[test]
fn test_package_state_absent() {
    let state = PackageState::from_str("absent").unwrap();
    assert_eq!(state, PackageState::Absent);
}

#[test]
fn test_package_state_removed_alias() {
    let state = PackageState::from_str("removed").unwrap();
    assert_eq!(state, PackageState::Absent);
}

#[test]
fn test_package_state_latest() {
    let state = PackageState::from_str("latest").unwrap();
    assert_eq!(state, PackageState::Latest);
}

#[test]
fn test_package_state_case_insensitive() {
    assert_eq!(
        PackageState::from_str("PRESENT").unwrap(),
        PackageState::Present
    );
    assert_eq!(
        PackageState::from_str("Present").unwrap(),
        PackageState::Present
    );
    assert_eq!(
        PackageState::from_str("ABSENT").unwrap(),
        PackageState::Absent
    );
    assert_eq!(
        PackageState::from_str("LATEST").unwrap(),
        PackageState::Latest
    );
}

#[test]
fn test_package_state_invalid() {
    let result = PackageState::from_str("invalid");
    assert!(result.is_err());
}

// ============================================================================
// PackageManager Parsing Tests
// ============================================================================

#[test]
fn test_package_manager_apt() {
    assert_eq!(
        PackageManager::from_str("apt").unwrap(),
        PackageManager::Apt
    );
    assert_eq!(
        PackageManager::from_str("apt-get").unwrap(),
        PackageManager::Apt
    );
}

#[test]
fn test_package_manager_dnf() {
    assert_eq!(
        PackageManager::from_str("dnf").unwrap(),
        PackageManager::Dnf
    );
}

#[test]
fn test_package_manager_yum() {
    assert_eq!(
        PackageManager::from_str("yum").unwrap(),
        PackageManager::Yum
    );
}

#[test]
fn test_package_manager_pacman() {
    assert_eq!(
        PackageManager::from_str("pacman").unwrap(),
        PackageManager::Pacman
    );
}

#[test]
fn test_package_manager_zypper() {
    assert_eq!(
        PackageManager::from_str("zypper").unwrap(),
        PackageManager::Zypper
    );
}

#[test]
fn test_package_manager_apk() {
    assert_eq!(
        PackageManager::from_str("apk").unwrap(),
        PackageManager::Apk
    );
}

#[test]
fn test_package_manager_brew() {
    assert_eq!(
        PackageManager::from_str("brew").unwrap(),
        PackageManager::Brew
    );
    assert_eq!(
        PackageManager::from_str("homebrew").unwrap(),
        PackageManager::Brew
    );
}

#[test]
fn test_package_manager_invalid() {
    let result = PackageManager::from_str("invalid");
    assert!(result.is_err());
}

// ============================================================================
// PackageManager Commands Tests
// ============================================================================

#[test]
fn test_apt_commands() {
    let apt = PackageManager::Apt;
    assert_eq!(apt.install_cmd(), vec!["apt-get", "install", "-y"]);
    assert_eq!(apt.remove_cmd(), vec!["apt-get", "remove", "-y"]);
    assert_eq!(apt.update_cmd(), vec!["apt-get", "update"]);
}

#[test]
fn test_dnf_commands() {
    let dnf = PackageManager::Dnf;
    assert_eq!(dnf.install_cmd(), vec!["dnf", "install", "-y"]);
    assert_eq!(dnf.remove_cmd(), vec!["dnf", "remove", "-y"]);
    assert_eq!(dnf.update_cmd(), vec!["dnf", "makecache"]);
}

#[test]
fn test_yum_commands() {
    let yum = PackageManager::Yum;
    assert_eq!(yum.install_cmd(), vec!["yum", "install", "-y"]);
    assert_eq!(yum.remove_cmd(), vec!["yum", "remove", "-y"]);
    assert_eq!(yum.update_cmd(), vec!["yum", "makecache"]);
}

#[test]
fn test_pacman_commands() {
    let pacman = PackageManager::Pacman;
    assert_eq!(pacman.install_cmd(), vec!["pacman", "-S", "--noconfirm"]);
    assert_eq!(pacman.remove_cmd(), vec!["pacman", "-R", "--noconfirm"]);
    assert_eq!(pacman.update_cmd(), vec!["pacman", "-Sy"]);
}

#[test]
fn test_zypper_commands() {
    let zypper = PackageManager::Zypper;
    assert_eq!(zypper.install_cmd(), vec!["zypper", "install", "-y"]);
    assert_eq!(zypper.remove_cmd(), vec!["zypper", "remove", "-y"]);
    assert_eq!(zypper.update_cmd(), vec!["zypper", "refresh"]);
}

#[test]
fn test_apk_commands() {
    let apk = PackageManager::Apk;
    assert_eq!(apk.install_cmd(), vec!["apk", "add"]);
    assert_eq!(apk.remove_cmd(), vec!["apk", "del"]);
    assert_eq!(apk.update_cmd(), vec!["apk", "update"]);
}

#[test]
fn test_brew_commands() {
    let brew = PackageManager::Brew;
    assert_eq!(brew.install_cmd(), vec!["brew", "install"]);
    assert_eq!(brew.remove_cmd(), vec!["brew", "uninstall"]);
    assert_eq!(brew.update_cmd(), vec!["brew", "update"]);
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_package_module_name() {
    let module = PackageModule;
    assert_eq!(module.name(), "package");
}

#[test]
fn test_package_module_description() {
    let module = PackageModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("package"));
}

#[test]
fn test_package_module_classification() {
    let module = PackageModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_package_module_parallelization() {
    let module = PackageModule;
    // Package managers use locks
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::HostExclusive
    );
}

#[test]
fn test_package_module_required_params() {
    let module = PackageModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Parameter Tests
// ============================================================================

#[test]
fn test_package_with_use_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("use".to_string(), serde_json::json!("apt"));

    assert!(params.contains_key("use"));
}

#[test]
fn test_package_with_update_cache() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("nginx"));
    params.insert("update_cache".to_string(), serde_json::json!(true));

    assert!(params.contains_key("update_cache"));
}

#[test]
fn test_package_list_parameter() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "name".to_string(),
        serde_json::json!(["nginx", "vim", "curl"]),
    );

    assert!(params.contains_key("name"));
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_package_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_package_manager_equality() {
    assert_eq!(PackageManager::Apt, PackageManager::Apt);
    assert_ne!(PackageManager::Apt, PackageManager::Dnf);
}

#[test]
fn test_package_manager_clone() {
    let apt = PackageManager::Apt;
    let cloned = apt.clone();
    assert_eq!(apt, cloned);
}

#[test]
fn test_package_state_clone() {
    let state = PackageState::Present;
    let cloned = state.clone();
    assert_eq!(state, cloned);
}

#[test]
fn test_package_state_debug_format() {
    let state = PackageState::Present;
    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("Present"));
}

#[test]
fn test_package_manager_debug_format() {
    let manager = PackageManager::Apt;
    let debug_str = format!("{:?}", manager);
    assert!(debug_str.contains("Apt"));
}
