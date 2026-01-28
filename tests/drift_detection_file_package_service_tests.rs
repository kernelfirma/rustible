//! Drift Detection Tests for File/Package/Service Resources
//!
//! Tests for Issue #296: Raised bar - Drift detection for file/package/service
//!
//! This module tests drift detection functionality with actionable diffs,
//! ensuring drift results include expected vs actual details for:
//! - File resources (mode, owner, content, permissions)
//! - Package resources (version, state, source)
//! - Service resources (running, enabled, type)

use serde_json::json;

// Use the state manifest types for drift detection
use rustible::state::manifest::{
    DriftState, DriftSummary, HostManifest, ManifestStore, ResourceState,
};

// ============================================================================
// File Resource Drift Detection Tests
// ============================================================================

#[test]
fn test_file_drift_mode_change() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/nginx/nginx.conf",
        "file",
        json!({
            "mode": "0644",
            "owner": "root",
            "group": "root",
            "state": "present"
        }),
    );

    // Set actual state with different mode
    resource.set_actual_state(json!({
        "mode": "0755",
        "owner": "root",
        "group": "root",
        "state": "present"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);
    assert!(resource.drift_details.is_some());

    let details = resource.drift_details.as_ref().unwrap();
    assert!(!details.changed_fields.is_empty());

    // Find the mode field diff
    let mode_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "mode")
        .expect("Mode diff should be present");
    assert_eq!(mode_diff.expected, "0644");
    assert_eq!(mode_diff.actual, "0755");
}

#[test]
fn test_file_drift_owner_change() {
    let mut resource = ResourceState::new(
        "file",
        "/var/log/app.log",
        "file",
        json!({
            "owner": "www-data",
            "group": "www-data",
            "mode": "0640"
        }),
    );

    resource.set_actual_state(json!({
        "owner": "root",
        "group": "www-data",
        "mode": "0640"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let owner_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "owner")
        .expect("Owner diff should be present");
    assert_eq!(owner_diff.expected, "www-data");
    assert_eq!(owner_diff.actual, "root");
}

#[test]
fn test_file_drift_content_checksum() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/hosts",
        "file",
        json!({
            "checksum": "sha256:abcdef1234567890",
            "state": "present"
        }),
    );

    resource.set_actual_state(json!({
        "checksum": "sha256:different9876543210",
        "state": "present"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let checksum_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "checksum")
        .expect("Checksum diff should be present");
    assert_eq!(checksum_diff.expected, "sha256:abcdef1234567890");
    assert_eq!(checksum_diff.actual, "sha256:different9876543210");
}

#[test]
fn test_file_drift_missing_file() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/missing.conf",
        "file",
        json!({
            "state": "present",
            "content": "expected content"
        }),
    );

    resource.mark_missing();

    assert_eq!(resource.drift_status, DriftState::Missing);
    assert!(resource.actual_state.is_none());
}

#[test]
fn test_file_drift_extra_file() {
    let mut resource = ResourceState::new(
        "file",
        "/tmp/should-not-exist",
        "file",
        json!({
            "state": "absent"
        }),
    );
    resource.should_exist = false;

    // File exists when it shouldn't
    resource.set_actual_state(json!({
        "state": "present",
        "size": 1024
    }));

    assert_eq!(resource.drift_status, DriftState::Extra);
    assert!(resource.drift_details.is_some());
}

#[test]
fn test_file_drift_multiple_changes() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/app/config.yml",
        "file",
        json!({
            "mode": "0644",
            "owner": "app",
            "group": "app",
            "selinux_context": "system_u:object_r:etc_t:s0"
        }),
    );

    resource.set_actual_state(json!({
        "mode": "0777",
        "owner": "root",
        "group": "root",
        "selinux_context": "system_u:object_r:tmp_t:s0"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    // Should have multiple field diffs
    assert!(details.changed_fields.len() >= 3);

    // Verify we can identify each drift
    let fields: Vec<&str> = details.changed_fields.iter().map(|f| f.field.as_str()).collect();
    assert!(fields.contains(&"mode"));
    assert!(fields.contains(&"owner"));
    assert!(fields.contains(&"group"));
}

#[test]
fn test_file_drift_in_sync() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/passwd",
        "file",
        json!({
            "mode": "0644",
            "owner": "root",
            "group": "root"
        }),
    );

    resource.set_actual_state(json!({
        "mode": "0644",
        "owner": "root",
        "group": "root"
    }));

    assert_eq!(resource.drift_status, DriftState::InSync);
    assert!(resource.drift_details.is_none());
}

// ============================================================================
// Package Resource Drift Detection Tests
// ============================================================================

#[test]
fn test_package_drift_version_change() {
    let mut resource = ResourceState::new(
        "package",
        "nginx",
        "apt",
        json!({
            "name": "nginx",
            "version": "1.18.0-1ubuntu1",
            "state": "present"
        }),
    );

    resource.set_actual_state(json!({
        "name": "nginx",
        "version": "1.14.0-0ubuntu1",
        "state": "present"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let version_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "version")
        .expect("Version diff should be present");
    assert_eq!(version_diff.expected, "1.18.0-1ubuntu1");
    assert_eq!(version_diff.actual, "1.14.0-0ubuntu1");
}

#[test]
fn test_package_drift_not_installed() {
    let mut resource = ResourceState::new(
        "package",
        "docker-ce",
        "apt",
        json!({
            "name": "docker-ce",
            "state": "present"
        }),
    );

    resource.mark_missing();

    assert_eq!(resource.drift_status, DriftState::Missing);
}

#[test]
fn test_package_drift_should_be_absent() {
    let mut resource = ResourceState::new(
        "package",
        "telnet",
        "apt",
        json!({
            "name": "telnet",
            "state": "absent"
        }),
    );
    resource.should_exist = false;

    resource.set_actual_state(json!({
        "name": "telnet",
        "version": "0.17-41ubuntu1",
        "state": "present"
    }));

    assert_eq!(resource.drift_status, DriftState::Extra);
}

#[test]
fn test_package_drift_multiple_packages() {
    let mut manifest = HostManifest::new("webserver1");

    // Package 1: in sync
    let mut pkg1 = ResourceState::new(
        "package",
        "curl",
        "apt",
        json!({
            "name": "curl",
            "state": "present"
        }),
    );
    pkg1.set_actual_state(json!({
        "name": "curl",
        "state": "present"
    }));
    manifest.record_resource(pkg1);

    // Package 2: version drift
    let mut pkg2 = ResourceState::new(
        "package",
        "nodejs",
        "apt",
        json!({
            "name": "nodejs",
            "version": "18.x",
            "state": "present"
        }),
    );
    pkg2.set_actual_state(json!({
        "name": "nodejs",
        "version": "16.x",
        "state": "present"
    }));
    manifest.record_resource(pkg2);

    // Package 3: missing
    let mut pkg3 = ResourceState::new(
        "package",
        "redis-server",
        "apt",
        json!({
            "name": "redis-server",
            "state": "present"
        }),
    );
    pkg3.mark_missing();
    manifest.record_resource(pkg3);

    manifest.update_drift_status();

    assert!(manifest.drift_detected);
    assert_eq!(manifest.drift_count, 1); // Only "Drifted" status counts, not "Missing"

    let summary = manifest.drift_summary();
    assert_eq!(summary.total, 3);
    assert_eq!(summary.in_sync, 1);
    assert_eq!(summary.drifted, 1);
    assert_eq!(summary.missing, 1);
}

#[test]
fn test_package_drift_with_source_change() {
    let mut resource = ResourceState::new(
        "package",
        "nginx",
        "apt",
        json!({
            "name": "nginx",
            "source": "ppa:nginx/stable",
            "state": "present"
        }),
    );

    resource.set_actual_state(json!({
        "name": "nginx",
        "source": "ubuntu-main",
        "state": "present"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let source_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "source")
        .expect("Source diff should be present");
    assert_eq!(source_diff.expected, "ppa:nginx/stable");
    assert_eq!(source_diff.actual, "ubuntu-main");
}

// ============================================================================
// Service Resource Drift Detection Tests
// ============================================================================

#[test]
fn test_service_drift_not_running() {
    let mut resource = ResourceState::new(
        "service",
        "nginx",
        "systemd",
        json!({
            "name": "nginx",
            "state": "started",
            "enabled": true
        }),
    );

    resource.set_actual_state(json!({
        "name": "nginx",
        "state": "stopped",
        "enabled": true
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let state_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "state")
        .expect("State diff should be present");
    assert_eq!(state_diff.expected, "started");
    assert_eq!(state_diff.actual, "stopped");
}

#[test]
fn test_service_drift_not_enabled() {
    let mut resource = ResourceState::new(
        "service",
        "docker",
        "systemd",
        json!({
            "name": "docker",
            "state": "started",
            "enabled": true
        }),
    );

    resource.set_actual_state(json!({
        "name": "docker",
        "state": "started",
        "enabled": false
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let enabled_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "enabled")
        .expect("Enabled diff should be present");
    assert_eq!(enabled_diff.expected, "true");
    assert_eq!(enabled_diff.actual, "false");
}

#[test]
fn test_service_drift_should_be_stopped() {
    let mut resource = ResourceState::new(
        "service",
        "cups",
        "systemd",
        json!({
            "name": "cups",
            "state": "stopped",
            "enabled": false
        }),
    );

    resource.set_actual_state(json!({
        "name": "cups",
        "state": "started",
        "enabled": true
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    assert!(details.changed_fields.len() >= 2);
}

#[test]
fn test_service_drift_missing_service() {
    let mut resource = ResourceState::new(
        "service",
        "custom-app",
        "systemd",
        json!({
            "name": "custom-app",
            "state": "started"
        }),
    );

    resource.mark_missing();

    assert_eq!(resource.drift_status, DriftState::Missing);
}

#[test]
fn test_service_drift_in_sync() {
    let mut resource = ResourceState::new(
        "service",
        "sshd",
        "systemd",
        json!({
            "name": "sshd",
            "state": "started",
            "enabled": true
        }),
    );

    resource.set_actual_state(json!({
        "name": "sshd",
        "state": "started",
        "enabled": true
    }));

    assert_eq!(resource.drift_status, DriftState::InSync);
    assert!(resource.drift_details.is_none());
}

#[test]
fn test_service_drift_with_extra_properties() {
    let mut resource = ResourceState::new(
        "service",
        "nginx",
        "systemd",
        json!({
            "name": "nginx",
            "state": "started",
            "enabled": true,
            "restart_policy": "always"
        }),
    );

    resource.set_actual_state(json!({
        "name": "nginx",
        "state": "started",
        "enabled": true,
        "restart_policy": "on-failure"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let policy_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "restart_policy")
        .expect("Restart policy diff should be present");
    assert_eq!(policy_diff.expected, "always");
    assert_eq!(policy_diff.actual, "on-failure");
}

// ============================================================================
// HostManifest Drift Detection Tests
// ============================================================================

#[test]
fn test_manifest_aggregate_drift_summary() {
    let mut manifest = HostManifest::new("server1");

    // Add various resources with different states
    for i in 0..5 {
        let mut resource = ResourceState::new(
            "file",
            format!("/etc/config{}.conf", i),
            "file",
            json!({"mode": "0644"}),
        );
        resource.set_actual_state(json!({"mode": "0644"}));
        manifest.record_resource(resource);
    }

    for i in 0..3 {
        let mut resource = ResourceState::new(
            "package",
            format!("package{}", i),
            "apt",
            json!({"version": "1.0"}),
        );
        resource.set_actual_state(json!({"version": "0.9"}));
        manifest.record_resource(resource);
    }

    for i in 0..2 {
        let mut resource = ResourceState::new(
            "service",
            format!("service{}", i),
            "systemd",
            json!({"state": "started"}),
        );
        resource.mark_missing();
        manifest.record_resource(resource);
    }

    let summary = manifest.drift_summary();
    assert_eq!(summary.total, 10);
    assert_eq!(summary.in_sync, 5);
    assert_eq!(summary.drifted, 3);
    assert_eq!(summary.missing, 2);
}

#[test]
fn test_manifest_drifted_resources_list() {
    let mut manifest = HostManifest::new("webserver");

    let mut in_sync = ResourceState::new("file", "/etc/good.conf", "file", json!({}));
    in_sync.drift_status = DriftState::InSync;
    manifest.record_resource(in_sync);

    let mut drifted = ResourceState::new("file", "/etc/bad.conf", "file", json!({}));
    drifted.drift_status = DriftState::Drifted;
    manifest.record_resource(drifted);

    let mut missing = ResourceState::new("package", "missing-pkg", "apt", json!({}));
    missing.drift_status = DriftState::Missing;
    manifest.record_resource(missing);

    let drifted_list = manifest.drifted_resources();
    assert_eq!(drifted_list.len(), 1);
    assert_eq!(drifted_list[0].resource_id, "/etc/bad.conf");
}

#[test]
fn test_manifest_resources_by_type() {
    let mut manifest = HostManifest::new("server");

    manifest.record_resource(ResourceState::new("file", "/etc/a", "file", json!({})));
    manifest.record_resource(ResourceState::new("file", "/etc/b", "file", json!({})));
    manifest.record_resource(ResourceState::new("package", "nginx", "apt", json!({})));
    manifest.record_resource(ResourceState::new("service", "nginx", "systemd", json!({})));

    let files = manifest.resources_by_type("file");
    assert_eq!(files.len(), 2);

    let packages = manifest.resources_by_type("package");
    assert_eq!(packages.len(), 1);

    let services = manifest.resources_by_type("service");
    assert_eq!(services.len(), 1);
}

// ============================================================================
// Field Diff Detail Tests
// ============================================================================

#[test]
fn test_field_diff_nested_objects() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/complex.conf",
        "file",
        json!({
            "permissions": {
                "owner": "root",
                "group": "admin",
                "mode": "0644"
            }
        }),
    );

    resource.set_actual_state(json!({
        "permissions": {
            "owner": "nobody",
            "group": "admin",
            "mode": "0644"
        }
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    // Should have nested path
    let owner_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "permissions.owner")
        .expect("Nested owner diff should be present");
    assert_eq!(owner_diff.expected, "root");
    assert_eq!(owner_diff.actual, "nobody");
}

#[test]
fn test_field_diff_array_changes() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/list.conf",
        "file",
        json!({
            "allowed_users": ["alice", "bob", "charlie"]
        }),
    );

    resource.set_actual_state(json!({
        "allowed_users": ["alice", "bob"]
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    // Should detect array length difference
    let len_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field.contains("length"))
        .expect("Array length diff should be present");
    assert_eq!(len_diff.expected, "3");
    assert_eq!(len_diff.actual, "2");
}

#[test]
fn test_field_diff_missing_field_in_actual() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/config.conf",
        "file",
        json!({
            "mode": "0644",
            "selinux_context": "system_u:object_r:etc_t:s0"
        }),
    );

    resource.set_actual_state(json!({
        "mode": "0644"
        // selinux_context is missing
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let selinux_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "selinux_context")
        .expect("Missing field diff should be present");
    assert_eq!(selinux_diff.actual, "<missing>");
}

#[test]
fn test_field_diff_extra_field_in_actual() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/config.conf",
        "file",
        json!({
            "mode": "0644"
        }),
    );

    resource.set_actual_state(json!({
        "mode": "0644",
        "extra_field": "unexpected"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let extra_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "extra_field")
        .expect("Extra field diff should be present");
    assert_eq!(extra_diff.expected, "<not expected>");
}

// ============================================================================
// DriftSummary Tests
// ============================================================================

#[test]
fn test_drift_summary_has_drift() {
    let no_drift = DriftSummary {
        total: 10,
        in_sync: 10,
        drifted: 0,
        missing: 0,
        extra: 0,
        unknown: 0,
    };
    assert!(!no_drift.has_drift());

    let with_drift = DriftSummary {
        total: 10,
        in_sync: 8,
        drifted: 2,
        missing: 0,
        extra: 0,
        unknown: 0,
    };
    assert!(with_drift.has_drift());

    let with_missing = DriftSummary {
        total: 10,
        in_sync: 9,
        drifted: 0,
        missing: 1,
        extra: 0,
        unknown: 0,
    };
    assert!(with_missing.has_drift());

    let with_extra = DriftSummary {
        total: 10,
        in_sync: 9,
        drifted: 0,
        missing: 0,
        extra: 1,
        unknown: 0,
    };
    assert!(with_extra.has_drift());
}

#[test]
fn test_drift_summary_sync_percentage() {
    let summary = DriftSummary {
        total: 100,
        in_sync: 75,
        drifted: 15,
        missing: 5,
        extra: 5,
        unknown: 0,
    };
    assert_eq!(summary.sync_percentage(), 75.0);

    let empty = DriftSummary::default();
    assert_eq!(empty.sync_percentage(), 100.0); // Edge case: empty is considered 100% in sync
}

// ============================================================================
// ManifestStore Tests
// ============================================================================

#[test]
fn test_manifest_store_aggregate_drift() {
    let dir = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(dir.path());

    // Create manifests for multiple hosts
    let mut manifest1 = HostManifest::new("host1");
    let mut r1 = ResourceState::new("file", "/etc/a", "file", json!({}));
    r1.drift_status = DriftState::InSync;
    manifest1.record_resource(r1);
    store.save(&manifest1).unwrap();

    let mut manifest2 = HostManifest::new("host2");
    let mut r2 = ResourceState::new("file", "/etc/b", "file", json!({}));
    r2.drift_status = DriftState::Drifted;
    manifest2.record_resource(r2);
    let mut r3 = ResourceState::new("package", "nginx", "apt", json!({}));
    r3.drift_status = DriftState::Missing;
    manifest2.record_resource(r3);
    store.save(&manifest2).unwrap();

    let aggregate = store.aggregate_drift_summary().unwrap();
    assert_eq!(aggregate.total, 3);
    assert_eq!(aggregate.in_sync, 1);
    assert_eq!(aggregate.drifted, 1);
    assert_eq!(aggregate.missing, 1);
}

#[test]
fn test_manifest_persistence_preserves_drift_details() {
    let dir = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(dir.path());

    let mut manifest = HostManifest::new("testhost");
    let mut resource = ResourceState::new(
        "file",
        "/etc/test.conf",
        "file",
        json!({"mode": "0644"}),
    );
    resource.set_actual_state(json!({"mode": "0755"}));
    manifest.record_resource(resource);
    store.save(&manifest).unwrap();

    // Load and verify drift details are preserved
    let loaded = store.load("testhost").unwrap();
    let loaded_resource = loaded.get_resource("file", "/etc/test.conf").unwrap();
    assert_eq!(loaded_resource.drift_status, DriftState::Drifted);
    assert!(loaded_resource.drift_details.is_some());

    let details = loaded_resource.drift_details.as_ref().unwrap();
    let mode_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "mode")
        .expect("Mode diff should be preserved");
    assert_eq!(mode_diff.expected, "0644");
    assert_eq!(mode_diff.actual, "0755");
}

// ============================================================================
// ResourceState Builder Pattern Tests
// ============================================================================

#[test]
fn test_resource_state_with_display_name() {
    let resource = ResourceState::new("file", "/etc/nginx/nginx.conf", "file", json!({}))
        .with_display_name("Nginx main configuration");

    assert_eq!(
        resource.display_name,
        Some("Nginx main configuration".to_string())
    );
}

#[test]
fn test_resource_state_with_task_name() {
    let resource = ResourceState::new("package", "nginx", "apt", json!({}))
        .with_task_name("Install Nginx web server");

    assert_eq!(
        resource.task_name,
        Some("Install Nginx web server".to_string())
    );
}

#[test]
fn test_resource_state_with_tags() {
    let resource = ResourceState::new("service", "nginx", "systemd", json!({}))
        .with_tags(vec!["webserver".to_string(), "production".to_string()]);

    assert_eq!(resource.tags.len(), 2);
    assert!(resource.tags.contains(&"webserver".to_string()));
    assert!(resource.tags.contains(&"production".to_string()));
}

// ============================================================================
// DriftState Display Tests
// ============================================================================

#[test]
fn test_drift_state_display() {
    assert_eq!(DriftState::InSync.to_string(), "in-sync");
    assert_eq!(DriftState::Drifted.to_string(), "drifted");
    assert_eq!(DriftState::Missing.to_string(), "missing");
    assert_eq!(DriftState::Extra.to_string(), "extra");
    assert_eq!(DriftState::Unknown.to_string(), "unknown");
}

// ============================================================================
// Edge Cases and Complex Scenarios
// ============================================================================

#[test]
fn test_drift_null_values() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/test.conf",
        "file",
        json!({
            "content": null,
            "mode": "0644"
        }),
    );

    resource.set_actual_state(json!({
        "content": "some content",
        "mode": "0644"
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);
}

#[test]
fn test_drift_numeric_type_differences() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/test.conf",
        "file",
        json!({
            "size": 1024
        }),
    );

    resource.set_actual_state(json!({
        "size": 2048
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let size_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "size")
        .expect("Size diff should be present");
    assert_eq!(size_diff.expected, "1024");
    assert_eq!(size_diff.actual, "2048");
}

#[test]
fn test_drift_boolean_values() {
    let mut resource = ResourceState::new(
        "service",
        "nginx",
        "systemd",
        json!({
            "enabled": true
        }),
    );

    resource.set_actual_state(json!({
        "enabled": false
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let enabled_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "enabled")
        .expect("Enabled diff should be present");
    assert_eq!(enabled_diff.expected, "true");
    assert_eq!(enabled_diff.actual, "false");
}

#[test]
fn test_drift_deep_nesting() {
    let mut resource = ResourceState::new(
        "file",
        "/etc/complex.yml",
        "file",
        json!({
            "config": {
                "level1": {
                    "level2": {
                        "level3": {
                            "value": "expected"
                        }
                    }
                }
            }
        }),
    );

    resource.set_actual_state(json!({
        "config": {
            "level1": {
                "level2": {
                    "level3": {
                        "value": "actual"
                    }
                }
            }
        }
    }));

    assert_eq!(resource.drift_status, DriftState::Drifted);

    let details = resource.drift_details.as_ref().unwrap();
    let deep_diff = details
        .changed_fields
        .iter()
        .find(|f| f.field == "config.level1.level2.level3.value")
        .expect("Deep nested diff should be present");
    assert_eq!(deep_diff.expected, "expected");
    assert_eq!(deep_diff.actual, "actual");
}

#[test]
fn test_manifest_update_drift_status() {
    let mut manifest = HostManifest::new("server");

    let mut r1 = ResourceState::new("file", "/etc/a", "file", json!({}));
    r1.drift_status = DriftState::Drifted;
    manifest.record_resource(r1);

    let mut r2 = ResourceState::new("file", "/etc/b", "file", json!({}));
    r2.drift_status = DriftState::Drifted;
    manifest.record_resource(r2);

    assert!(!manifest.drift_detected); // Not updated yet

    manifest.update_drift_status();

    assert!(manifest.drift_detected);
    assert_eq!(manifest.drift_count, 2);
    assert!(manifest.last_drift_check.is_some());
}

#[test]
fn test_host_manifest_with_playbook() {
    let manifest = HostManifest::with_playbook("server1", "site.yml");
    assert_eq!(manifest.hostname, "server1");
    assert_eq!(manifest.source_playbook, Some("site.yml".to_string()));
}

#[test]
fn test_resource_key_generation() {
    let key = HostManifest::resource_key("file", "/etc/nginx/nginx.conf");
    assert_eq!(key, "file::/etc/nginx/nginx.conf");
}

#[test]
fn test_manifest_remove_resource() {
    let mut manifest = HostManifest::new("server");
    manifest.record_resource(ResourceState::new("file", "/etc/test", "file", json!({})));

    assert!(manifest.get_resource("file", "/etc/test").is_some());

    let removed = manifest.remove_resource("file", "/etc/test");
    assert!(removed.is_some());
    assert!(manifest.get_resource("file", "/etc/test").is_none());
}

#[test]
fn test_manifest_host_facts() {
    let mut manifest = HostManifest::new("server");

    manifest.set_fact("os_family", json!("Debian"));
    manifest.set_fact("ansible_distribution", json!("Ubuntu"));
    manifest.set_fact("ansible_distribution_version", json!("22.04"));

    assert_eq!(manifest.get_fact("os_family"), Some(&json!("Debian")));
    assert_eq!(manifest.get_fact("nonexistent"), None);
}
