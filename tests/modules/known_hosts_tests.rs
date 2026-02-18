//! Comprehensive unit tests for the Known Hosts module
//!
//! Tests cover:
//! - Known hosts entry parsing
//! - Key type conversion and validation
//! - State management (present, absent)
//! - Hostname formatting and matching
//! - Hashed hostname support
//! - Module metadata and parameter validation
//! - File operations

use rustible::modules::known_hosts::{
    KeyType, KnownHostsEntry, KnownHostsFile, KnownHostsModule, KnownHostsState,
};
use rustible::modules::{Module, ModuleClassification, ModuleContext, ParallelizationHint};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Test Constants - Sample Known Hosts Entries
// ============================================================================

const TEST_GITHUB_ED25519: &str =
    "github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
const TEST_RSA_ENTRY: &str = "example.com ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQtest";
const TEST_ECDSA_ENTRY: &str =
    "test.example.com ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY=";
const TEST_WITH_PORT: &str = "[example.com]:2222 ssh-rsa AAAAB3NzaC1yc2EAAAAtest";
const TEST_WITH_MARKER: &str =
    "@cert-authority *.example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAItest";
const TEST_REVOKED: &str = "@revoked badhost.com ssh-rsa AAAAB3NzaC1yc2EAAAAtest";
const TEST_HASHED: &str =
    "|1|F3GJvMX9f3ByPm4MQq5R7S7E/wY=|hXGJd+SqtTeGJ8jELEmYvNF0J24= ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test";
const TEST_MULTIPLE_HOSTS: &str =
    "github.com,192.30.255.113 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAItest";

// ============================================================================
// KnownHostsEntry Parsing Tests
// ============================================================================

#[test]
fn test_parse_simple_entry() {
    let entry = KnownHostsEntry::parse(TEST_GITHUB_ED25519, Some(0)).unwrap();

    assert_eq!(entry.hostnames, "github.com");
    assert_eq!(entry.key_type, "ssh-ed25519");
    assert!(!entry.is_hashed);
    assert!(entry.marker.is_none());
    assert_eq!(entry.line_number, Some(0));
}

#[test]
fn test_parse_rsa_entry() {
    let entry = KnownHostsEntry::parse(TEST_RSA_ENTRY, None).unwrap();

    assert_eq!(entry.hostnames, "example.com");
    assert_eq!(entry.key_type, "ssh-rsa");
    assert!(entry.line_number.is_none());
}

#[test]
fn test_parse_ecdsa_entry() {
    let entry = KnownHostsEntry::parse(TEST_ECDSA_ENTRY, None).unwrap();

    assert_eq!(entry.hostnames, "test.example.com");
    assert_eq!(entry.key_type, "ecdsa-sha2-nistp256");
}

#[test]
fn test_parse_entry_with_port() {
    let entry = KnownHostsEntry::parse(TEST_WITH_PORT, None).unwrap();

    assert_eq!(entry.hostnames, "[example.com]:2222");
    assert_eq!(entry.key_type, "ssh-rsa");
}

#[test]
fn test_parse_entry_with_cert_authority_marker() {
    let entry = KnownHostsEntry::parse(TEST_WITH_MARKER, None).unwrap();

    assert_eq!(entry.marker, Some("@cert-authority".to_string()));
    assert_eq!(entry.hostnames, "*.example.com");
    assert_eq!(entry.key_type, "ssh-ed25519");
}

#[test]
fn test_parse_entry_with_revoked_marker() {
    let entry = KnownHostsEntry::parse(TEST_REVOKED, None).unwrap();

    assert_eq!(entry.marker, Some("@revoked".to_string()));
    assert_eq!(entry.hostnames, "badhost.com");
}

#[test]
fn test_parse_hashed_entry() {
    let entry = KnownHostsEntry::parse(TEST_HASHED, None).unwrap();

    assert!(entry.is_hashed);
    assert!(entry.hostnames.starts_with("|1|"));
    assert_eq!(entry.key_type, "ssh-ed25519");
}

#[test]
fn test_parse_entry_with_multiple_hosts() {
    let entry = KnownHostsEntry::parse(TEST_MULTIPLE_HOSTS, None).unwrap();

    assert_eq!(entry.hostnames, "github.com,192.30.255.113");
}

#[test]
fn test_parse_empty_line() {
    let result = KnownHostsEntry::parse("", None);
    assert!(result.is_none());
}

#[test]
fn test_parse_whitespace_line() {
    let result = KnownHostsEntry::parse("   ", None);
    assert!(result.is_none());
}

#[test]
fn test_parse_comment_line() {
    let result = KnownHostsEntry::parse("# This is a comment", None);
    assert!(result.is_none());
}

#[test]
fn test_parse_comment_with_leading_whitespace() {
    let result = KnownHostsEntry::parse("  # Indented comment", None);
    assert!(result.is_none());
}

#[test]
fn test_parse_invalid_entry() {
    let result = KnownHostsEntry::parse("not a valid entry", None);
    assert!(result.is_none());
}

// ============================================================================
// Entry to_line Tests
// ============================================================================

#[test]
fn test_entry_to_line_simple() {
    let entry = KnownHostsEntry {
        marker: None,
        hostnames: "example.com".to_string(),
        key_type: "ssh-ed25519".to_string(),
        key: "AAAAC3NzaC1lZDI1NTE5testkey".to_string(),
        comment: None,
        is_hashed: false,
        line_number: None,
    };

    let line = entry.to_line();
    assert_eq!(line, "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5testkey");
}

#[test]
fn test_entry_to_line_with_marker() {
    let entry = KnownHostsEntry {
        marker: Some("@cert-authority".to_string()),
        hostnames: "*.example.com".to_string(),
        key_type: "ssh-rsa".to_string(),
        key: "AAAAB3NzaC1yc2EAAAAtest".to_string(),
        comment: None,
        is_hashed: false,
        line_number: None,
    };

    let line = entry.to_line();
    assert!(line.starts_with("@cert-authority"));
    assert!(line.contains("*.example.com"));
}

#[test]
fn test_entry_to_line_with_comment() {
    let entry = KnownHostsEntry {
        marker: None,
        hostnames: "example.com".to_string(),
        key_type: "ssh-rsa".to_string(),
        key: "AAAAB3NzaC1yc2EAAAAtest".to_string(),
        comment: Some("Added by script".to_string()),
        is_hashed: false,
        line_number: None,
    };

    let line = entry.to_line();
    assert!(line.ends_with("Added by script"));
}

// ============================================================================
// Hostname Matching Tests
// ============================================================================

#[test]
fn test_matches_hostname_exact() {
    let entry = KnownHostsEntry::parse(TEST_GITHUB_ED25519, None).unwrap();

    assert!(entry.matches_hostname("github.com", None));
    assert!(!entry.matches_hostname("gitlab.com", None));
}

#[test]
fn test_matches_hostname_in_list() {
    let entry = KnownHostsEntry::parse(TEST_MULTIPLE_HOSTS, None).unwrap();

    assert!(entry.matches_hostname("github.com", None));
    assert!(entry.matches_hostname("192.30.255.113", None));
    assert!(!entry.matches_hostname("other.com", None));
}

#[test]
fn test_matches_hostname_with_port() {
    let entry = KnownHostsEntry::parse(TEST_WITH_PORT, None).unwrap();

    // Should match with explicit port
    assert!(entry.matches_hostname("example.com", Some(2222)));
    // Should not match without port or with default port
    assert!(!entry.matches_hostname("example.com", None));
    assert!(!entry.matches_hostname("example.com", Some(22)));
}

#[test]
fn test_matches_hostname_default_port() {
    let entry = KnownHostsEntry::parse(TEST_RSA_ENTRY, None).unwrap();

    // Default port (22) should match entry without port
    assert!(entry.matches_hostname("example.com", None));
    assert!(entry.matches_hostname("example.com", Some(22)));
}

// ============================================================================
// Key Type Matching Tests
// ============================================================================

#[test]
fn test_matches_key_type_ed25519() {
    let entry = KnownHostsEntry::parse(TEST_GITHUB_ED25519, None).unwrap();

    assert!(entry.matches_key_type(&KeyType::Ed25519));
    assert!(!entry.matches_key_type(&KeyType::Rsa));
}

#[test]
fn test_matches_key_type_rsa() {
    let entry = KnownHostsEntry::parse(TEST_RSA_ENTRY, None).unwrap();

    assert!(entry.matches_key_type(&KeyType::Rsa));
    assert!(!entry.matches_key_type(&KeyType::Ed25519));
}

#[test]
fn test_matches_key_type_ecdsa() {
    let entry = KnownHostsEntry::parse(TEST_ECDSA_ENTRY, None).unwrap();

    assert!(entry.matches_key_type(&KeyType::EcdsaNistp256));
}

// ============================================================================
// KeyType Tests
// ============================================================================

#[test]
fn test_key_type_from_str_rsa() {
    assert_eq!(KeyType::from_str("rsa").unwrap(), KeyType::Rsa);
    assert_eq!(KeyType::from_str("ssh-rsa").unwrap(), KeyType::Rsa);
}

#[test]
fn test_key_type_from_str_ed25519() {
    assert_eq!(KeyType::from_str("ed25519").unwrap(), KeyType::Ed25519);
    assert_eq!(KeyType::from_str("ssh-ed25519").unwrap(), KeyType::Ed25519);
}

#[test]
fn test_key_type_from_str_dss() {
    assert_eq!(KeyType::from_str("dss").unwrap(), KeyType::Dss);
    assert_eq!(KeyType::from_str("ssh-dss").unwrap(), KeyType::Dss);
    assert_eq!(KeyType::from_str("dsa").unwrap(), KeyType::Dss);
}

#[test]
fn test_key_type_from_str_ecdsa() {
    assert_eq!(KeyType::from_str("ecdsa").unwrap(), KeyType::EcdsaNistp256);
    assert_eq!(
        KeyType::from_str("ecdsa-sha2-nistp256").unwrap(),
        KeyType::EcdsaNistp256
    );
    assert_eq!(
        KeyType::from_str("ecdsa-sha2-nistp384").unwrap(),
        KeyType::EcdsaNistp384
    );
    assert_eq!(
        KeyType::from_str("ecdsa-sha2-nistp521").unwrap(),
        KeyType::EcdsaNistp521
    );
}

#[test]
fn test_key_type_from_str_security_keys() {
    assert_eq!(
        KeyType::from_str("sk-ssh-ed25519").unwrap(),
        KeyType::SkEd25519
    );
    assert_eq!(KeyType::from_str("sk-ed25519").unwrap(), KeyType::SkEd25519);
    assert_eq!(
        KeyType::from_str("sk-ssh-ed25519@openssh.com").unwrap(),
        KeyType::SkEd25519
    );
    assert_eq!(
        KeyType::from_str("sk-ecdsa-sha2-nistp256").unwrap(),
        KeyType::SkEcdsa
    );
    assert_eq!(KeyType::from_str("sk-ecdsa").unwrap(), KeyType::SkEcdsa);
}

#[test]
fn test_key_type_from_str_invalid() {
    assert!(KeyType::from_str("invalid").is_err());
    assert!(KeyType::from_str("").is_err());
    assert!(KeyType::from_str("aes-256").is_err());
}

#[test]
fn test_key_type_as_openssh_str() {
    assert_eq!(KeyType::Rsa.as_openssh_str(), "ssh-rsa");
    assert_eq!(KeyType::Ed25519.as_openssh_str(), "ssh-ed25519");
    assert_eq!(KeyType::Dss.as_openssh_str(), "ssh-dss");
    assert_eq!(
        KeyType::EcdsaNistp256.as_openssh_str(),
        "ecdsa-sha2-nistp256"
    );
    assert_eq!(
        KeyType::EcdsaNistp384.as_openssh_str(),
        "ecdsa-sha2-nistp384"
    );
    assert_eq!(
        KeyType::EcdsaNistp521.as_openssh_str(),
        "ecdsa-sha2-nistp521"
    );
    assert_eq!(
        KeyType::SkEd25519.as_openssh_str(),
        "sk-ssh-ed25519@openssh.com"
    );
    assert_eq!(
        KeyType::SkEcdsa.as_openssh_str(),
        "sk-ecdsa-sha2-nistp256@openssh.com"
    );
}

#[test]
fn test_key_type_as_ssh_keyscan_type() {
    assert_eq!(KeyType::Rsa.as_ssh_keyscan_type(), "rsa");
    assert_eq!(KeyType::Ed25519.as_ssh_keyscan_type(), "ed25519");
    assert_eq!(KeyType::Dss.as_ssh_keyscan_type(), "dsa");
    assert_eq!(KeyType::EcdsaNistp256.as_ssh_keyscan_type(), "ecdsa");
    assert_eq!(KeyType::EcdsaNistp384.as_ssh_keyscan_type(), "ecdsa");
    assert_eq!(KeyType::EcdsaNistp521.as_ssh_keyscan_type(), "ecdsa");
}

#[test]
fn test_key_type_from_openssh_str() {
    assert_eq!(KeyType::from_openssh_str("ssh-rsa"), Some(KeyType::Rsa));
    assert_eq!(
        KeyType::from_openssh_str("ssh-ed25519"),
        Some(KeyType::Ed25519)
    );
    assert_eq!(KeyType::from_openssh_str("ssh-dss"), Some(KeyType::Dss));
    assert_eq!(
        KeyType::from_openssh_str("ecdsa-sha2-nistp256"),
        Some(KeyType::EcdsaNistp256)
    );
    assert_eq!(KeyType::from_openssh_str("invalid"), None);
}

#[test]
fn test_key_type_clone() {
    let kt = KeyType::Ed25519;
    let cloned = kt;
    assert_eq!(kt, cloned);
}

#[test]
fn test_key_type_debug() {
    let debug_str = format!("{:?}", KeyType::Ed25519);
    assert!(debug_str.contains("Ed25519"));
}

#[test]
fn test_key_type_copy() {
    let kt1 = KeyType::Rsa;
    let kt2 = kt1; // Copy
    assert_eq!(kt1, kt2);
}

// ============================================================================
// KnownHostsState Tests
// ============================================================================

#[test]
fn test_state_from_str_present() {
    assert_eq!(
        KnownHostsState::from_str("present").unwrap(),
        KnownHostsState::Present
    );
}

#[test]
fn test_state_from_str_absent() {
    assert_eq!(
        KnownHostsState::from_str("absent").unwrap(),
        KnownHostsState::Absent
    );
}

#[test]
fn test_state_case_insensitive() {
    assert_eq!(
        KnownHostsState::from_str("PRESENT").unwrap(),
        KnownHostsState::Present
    );
    assert_eq!(
        KnownHostsState::from_str("Present").unwrap(),
        KnownHostsState::Present
    );
    assert_eq!(
        KnownHostsState::from_str("ABSENT").unwrap(),
        KnownHostsState::Absent
    );
}

#[test]
fn test_state_invalid() {
    assert!(KnownHostsState::from_str("invalid").is_err());
    assert!(KnownHostsState::from_str("latest").is_err());
    assert!(KnownHostsState::from_str("").is_err());
}

#[test]
fn test_state_clone() {
    let state = KnownHostsState::Present;
    let cloned = state;
    assert_eq!(state, cloned);
}

#[test]
fn test_state_copy() {
    let s1 = KnownHostsState::Present;
    let s2 = s1; // Copy
    assert_eq!(s1, s2);
}

#[test]
fn test_state_debug() {
    let debug_str = format!("{:?}", KnownHostsState::Present);
    assert!(debug_str.contains("Present"));
}

// ============================================================================
// KnownHostsFile Tests
// ============================================================================

#[test]
fn test_load_nonexistent_file() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("nonexistent_known_hosts");

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert!(known_hosts.entries.is_empty());
}

#[test]
fn test_load_empty_file() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(&path, "").unwrap();

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert!(known_hosts.entries.is_empty());
}

#[test]
fn test_load_file_with_entries() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(
        &path,
        format!("{}\n{}\n", TEST_GITHUB_ED25519, TEST_RSA_ENTRY),
    )
    .unwrap();

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert_eq!(known_hosts.entries.len(), 2);
}

#[test]
fn test_load_file_with_comments() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    let content = format!(
        "# GitHub's SSH key\n{}\n\n# Another entry\n{}\n",
        TEST_GITHUB_ED25519, TEST_RSA_ENTRY
    );
    fs::write(&path, content).unwrap();

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert_eq!(known_hosts.entries.len(), 2);
}

#[test]
fn test_save_file() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();
    known_hosts.add_entry(KnownHostsEntry {
        marker: None,
        hostnames: "example.com".to_string(),
        key_type: "ssh-ed25519".to_string(),
        key: "AAAAC3NzaC1lZDI1NTE5test".to_string(),
        comment: None,
        is_hashed: false,
        line_number: None,
    });

    known_hosts.save().unwrap();

    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("example.com"));
    assert!(content.contains("ssh-ed25519"));
}

#[test]
fn test_save_creates_parent_directory() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("subdir").join("known_hosts");

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();
    known_hosts.add_entry(KnownHostsEntry {
        marker: None,
        hostnames: "example.com".to_string(),
        key_type: "ssh-ed25519".to_string(),
        key: "AAAAC3NzaC1lZDI1NTE5test".to_string(),
        comment: None,
        is_hashed: false,
        line_number: None,
    });

    known_hosts.save().unwrap();
    assert!(path.exists());
}

#[test]
fn test_find_entries_by_hostname() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(
        &path,
        format!("{}\n{}\n", TEST_GITHUB_ED25519, TEST_RSA_ENTRY),
    )
    .unwrap();

    let known_hosts = KnownHostsFile::load(&path).unwrap();

    let entries = known_hosts.find_entries("github.com", None, None);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key_type, "ssh-ed25519");
}

#[test]
fn test_find_entries_by_hostname_and_key_type() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(
        &path,
        "example.com ssh-rsa AAAAB3NzaC1yc2EAAAAtest1\nexample.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test2\n",
    )
    .unwrap();

    let known_hosts = KnownHostsFile::load(&path).unwrap();

    let entries = known_hosts.find_entries("example.com", None, Some(&KeyType::Ed25519));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key_type, "ssh-ed25519");
}

#[test]
fn test_find_entries_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(&path, TEST_GITHUB_ED25519).unwrap();

    let known_hosts = KnownHostsFile::load(&path).unwrap();

    let entries = known_hosts.find_entries("notfound.com", None, None);
    assert!(entries.is_empty());
}

#[test]
fn test_add_entry() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();
    assert!(known_hosts.entries.is_empty());

    known_hosts.add_entry(KnownHostsEntry {
        marker: None,
        hostnames: "test.com".to_string(),
        key_type: "ssh-rsa".to_string(),
        key: "testkey".to_string(),
        comment: None,
        is_hashed: false,
        line_number: None,
    });

    assert_eq!(known_hosts.entries.len(), 1);
}

#[test]
fn test_remove_entries() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(
        &path,
        format!("{}\n{}\n", TEST_GITHUB_ED25519, TEST_RSA_ENTRY),
    )
    .unwrap();

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();
    assert_eq!(known_hosts.entries.len(), 2);

    let removed = known_hosts.remove_entries("github.com", None, None);
    assert_eq!(removed, 1);
    assert_eq!(known_hosts.entries.len(), 1);
}

#[test]
fn test_remove_entries_with_key_type() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(
        &path,
        "example.com ssh-rsa AAAAB3NzaC1yc2EAAAAtest\nexample.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test\n",
    )
    .unwrap();

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();
    assert_eq!(known_hosts.entries.len(), 2);

    let removed = known_hosts.remove_entries("example.com", None, Some(&KeyType::Rsa));
    assert_eq!(removed, 1);
    assert_eq!(known_hosts.entries.len(), 1);
    assert_eq!(known_hosts.entries[0].key_type, "ssh-ed25519");
}

#[test]
fn test_remove_entries_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(&path, TEST_GITHUB_ED25519).unwrap();

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();

    let removed = known_hosts.remove_entries("notfound.com", None, None);
    assert_eq!(removed, 0);
    assert_eq!(known_hosts.entries.len(), 1);
}

#[test]
fn test_update_or_add_new_entry() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();

    let entry = KnownHostsEntry {
        marker: None,
        hostnames: "example.com".to_string(),
        key_type: "ssh-ed25519".to_string(),
        key: "newkey".to_string(),
        comment: None,
        is_hashed: false,
        line_number: None,
    };

    let changed = known_hosts.update_or_add("example.com", None, entry);
    assert!(changed);
    assert_eq!(known_hosts.entries.len(), 1);
}

#[test]
fn test_update_or_add_update_existing() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(&path, "example.com ssh-ed25519 oldkey\n").unwrap();

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();

    let entry = KnownHostsEntry {
        marker: None,
        hostnames: "example.com".to_string(),
        key_type: "ssh-ed25519".to_string(),
        key: "newkey".to_string(),
        comment: None,
        is_hashed: false,
        line_number: None,
    };

    let changed = known_hosts.update_or_add("example.com", None, entry);
    assert!(changed);
    assert_eq!(known_hosts.entries.len(), 1);
    assert_eq!(known_hosts.entries[0].key, "newkey");
}

#[test]
fn test_update_or_add_no_change_needed() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(&path, "example.com ssh-ed25519 samekey\n").unwrap();

    let mut known_hosts = KnownHostsFile::load(&path).unwrap();

    let entry = KnownHostsEntry {
        marker: None,
        hostnames: "example.com".to_string(),
        key_type: "ssh-ed25519".to_string(),
        key: "samekey".to_string(),
        comment: None,
        is_hashed: false,
        line_number: None,
    };

    let changed = known_hosts.update_or_add("example.com", None, entry);
    assert!(!changed);
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_module_name() {
    let module = KnownHostsModule;
    assert_eq!(module.name(), "known_hosts");
}

#[test]
fn test_module_description() {
    let module = KnownHostsModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("known_hosts"));
}

#[test]
fn test_module_classification() {
    let module = KnownHostsModule;
    assert_eq!(module.classification(), ModuleClassification::LocalLogic);
}

#[test]
fn test_module_parallelization_hint() {
    let module = KnownHostsModule;
    match module.parallelization_hint() {
        ParallelizationHint::RateLimited {
            requests_per_second,
        } => {
            assert!(requests_per_second > 0);
        }
        _ => panic!("Expected RateLimited hint"),
    }
}

#[test]
fn test_module_required_params() {
    let module = KnownHostsModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
}

// ============================================================================
// Module Execution Tests
// ============================================================================

#[test]
fn test_module_absent_removes_entry() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(&path, "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test\n").unwrap();

    let module = KnownHostsModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("example.com"));
    params.insert("state".to_string(), serde_json::json!("absent"));
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert!(known_hosts.entries.is_empty());
}

#[test]
fn test_module_absent_no_change_when_not_present() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(&path, "other.com ssh-ed25519 testkey\n").unwrap();

    let module = KnownHostsModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("example.com"));
    params.insert("state".to_string(), serde_json::json!("absent"));
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

#[test]
fn test_module_present_with_key_data() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");

    let module = KnownHostsModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("example.com"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("key_type".to_string(), serde_json::json!("ed25519"));
    params.insert(
        "key_data".to_string(),
        serde_json::json!("AAAAC3NzaC1lZDI1NTE5testkey"),
    );
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert_eq!(known_hosts.entries.len(), 1);
    assert_eq!(known_hosts.entries[0].hostnames, "example.com");
}

#[test]
fn test_module_present_no_change_when_key_matches() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(
        &path,
        "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5testkey\n",
    )
    .unwrap();

    let module = KnownHostsModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("example.com"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("key_type".to_string(), serde_json::json!("ed25519"));
    params.insert(
        "key_data".to_string(),
        serde_json::json!("AAAAC3NzaC1lZDI1NTE5testkey"),
    );
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(!result.changed);
}

#[test]
fn test_module_check_mode_absent() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    fs::write(&path, "example.com ssh-ed25519 testkey\n").unwrap();

    let module = KnownHostsModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("example.com"));
    params.insert("state".to_string(), serde_json::json!("absent"));
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would remove"));

    // Verify entry was NOT removed
    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert_eq!(known_hosts.entries.len(), 1);
}

#[test]
fn test_module_check_mode_present() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");

    let module = KnownHostsModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("example.com"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("key_type".to_string(), serde_json::json!("ed25519"));
    params.insert("key_data".to_string(), serde_json::json!("testkey"));
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default().with_check_mode(true);
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(result.msg.contains("Would add"));

    // Verify entry was NOT added
    assert!(!path.exists());
}

#[test]
fn test_module_with_port() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");

    let module = KnownHostsModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("example.com"));
    params.insert("port".to_string(), serde_json::json!(2222));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("key_type".to_string(), serde_json::json!("ed25519"));
    params.insert("key_data".to_string(), serde_json::json!("testkey"));
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert_eq!(known_hosts.entries[0].hostnames, "[example.com]:2222");
}

#[test]
fn test_module_with_hash_host() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");

    let module = KnownHostsModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("example.com"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("key_type".to_string(), serde_json::json!("ed25519"));
    params.insert("key_data".to_string(), serde_json::json!("testkey"));
    params.insert("hash_host".to_string(), serde_json::json!(true));
    params.insert(
        "path".to_string(),
        serde_json::json!(path.to_str().unwrap()),
    );

    let context = ModuleContext::default();
    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    assert!(known_hosts.entries[0].is_hashed);
    assert!(known_hosts.entries[0].hostnames.starts_with("|1|"));
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_entry_with_long_key() {
    let long_key = "A".repeat(1000);
    let line = format!("example.com ssh-rsa {}", long_key);
    let entry = KnownHostsEntry::parse(&line, None).unwrap();
    assert_eq!(entry.key.len(), 1000);
}

#[test]
fn test_entry_with_ipv6_address() {
    let line = "2001:db8::1 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test";
    let entry = KnownHostsEntry::parse(line, None).unwrap();
    assert_eq!(entry.hostnames, "2001:db8::1");
}

#[test]
fn test_entry_with_wildcard() {
    let line = "*.example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test";
    let entry = KnownHostsEntry::parse(line, None).unwrap();
    assert_eq!(entry.hostnames, "*.example.com");
}

#[test]
fn test_load_file_with_invalid_entries() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("known_hosts");
    let content = format!(
        "{}\ninvalid line here\n{}\n",
        TEST_GITHUB_ED25519, TEST_RSA_ENTRY
    );
    fs::write(&path, content).unwrap();

    let known_hosts = KnownHostsFile::load(&path).unwrap();
    // Invalid lines should be silently ignored
    assert_eq!(known_hosts.entries.len(), 2);
}

#[test]
fn test_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Clone Tests
// ============================================================================

#[test]
fn test_entry_clone() {
    let entry = KnownHostsEntry::parse(TEST_GITHUB_ED25519, Some(5)).unwrap();
    let cloned = entry.clone();

    assert_eq!(entry.hostnames, cloned.hostnames);
    assert_eq!(entry.key_type, cloned.key_type);
    assert_eq!(entry.key, cloned.key);
    assert_eq!(entry.line_number, cloned.line_number);
}

#[test]
fn test_entry_debug() {
    let entry = KnownHostsEntry::parse(TEST_GITHUB_ED25519, None).unwrap();
    let debug_str = format!("{:?}", entry);
    assert!(debug_str.contains("github.com"));
    assert!(debug_str.contains("ssh-ed25519"));
}
