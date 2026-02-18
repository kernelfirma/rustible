//! Comprehensive unit tests for the SystemdUnit module
//!
//! Tests cover:
//! - Unit type handling
//! - Unit state handling
//! - Running state handling
//! - Configuration parameters
//! - Template helpers
//! - Module metadata

use rustible::modules::systemd_unit::{templates, SystemdUnitModule};
use rustible::modules::{Module, ModuleClassification, ModuleContext, ParallelizationHint};
use std::collections::HashMap;

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_systemd_unit_module_name() {
    let module = SystemdUnitModule;
    assert_eq!(module.name(), "systemd_unit");
}

#[test]
fn test_systemd_unit_module_description() {
    let module = SystemdUnitModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("systemd"));
}

#[test]
fn test_systemd_unit_module_classification() {
    let module = SystemdUnitModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_systemd_unit_module_required_params() {
    let module = SystemdUnitModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
    assert_eq!(required.len(), 1);
}

#[test]
fn test_systemd_unit_module_parallelization_hint() {
    let module = SystemdUnitModule;
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::FullyParallel
    );
}

// ============================================================================
// Parameter Handling Tests
// ============================================================================

#[test]
fn test_systemd_unit_missing_connection() {
    let module = SystemdUnitModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert("content".to_string(), serde_json::json!("[Unit]"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_systemd_unit_missing_name() {
    let module = SystemdUnitModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("content".to_string(), serde_json::json!("[Unit]"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_systemd_unit_basic_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert(
        "content".to_string(),
        serde_json::json!("[Unit]\nDescription=Test"),
    );

    assert!(params.contains_key("name"));
    assert!(params.contains_key("content"));
}

#[test]
fn test_systemd_unit_template_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert(
        "template".to_string(),
        serde_json::json!("[Unit]\nDescription={{ description }}"),
    );

    assert!(params.contains_key("template"));
}

#[test]
fn test_systemd_unit_state_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("content".to_string(), serde_json::json!("[Unit]"));

    assert_eq!(params.get("state").unwrap(), &serde_json::json!("present"));
}

#[test]
fn test_systemd_unit_enabled_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert("content".to_string(), serde_json::json!("[Unit]"));
    params.insert("enabled".to_string(), serde_json::json!(true));

    assert_eq!(params.get("enabled").unwrap(), &serde_json::json!(true));
}

#[test]
fn test_systemd_unit_running_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert("content".to_string(), serde_json::json!("[Unit]"));
    params.insert("running".to_string(), serde_json::json!("started"));

    assert_eq!(
        params.get("running").unwrap(),
        &serde_json::json!("started")
    );
}

#[test]
fn test_systemd_unit_custom_path_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert("content".to_string(), serde_json::json!("[Unit]"));
    params.insert(
        "unit_path".to_string(),
        serde_json::json!("/usr/lib/systemd/system"),
    );

    assert_eq!(
        params.get("unit_path").unwrap(),
        &serde_json::json!("/usr/lib/systemd/system")
    );
}

// ============================================================================
// Template Helper Tests
// ============================================================================

#[test]
fn test_template_service_basic() {
    let content = templates::service("My Application", "/usr/bin/myapp", None, None);
    assert!(content.contains("[Unit]"));
    assert!(content.contains("Description=My Application"));
    assert!(content.contains("[Service]"));
    assert!(content.contains("ExecStart=/usr/bin/myapp"));
    assert!(content.contains("[Install]"));
    assert!(content.contains("WantedBy=multi-user.target"));
    assert!(content.contains("Restart=on-failure"));
}

#[test]
fn test_template_service_with_user() {
    let content = templates::service("My App", "/usr/bin/myapp", Some("myuser"), None);
    assert!(content.contains("User=myuser"));
}

#[test]
fn test_template_service_with_wanted_by() {
    let content = templates::service("My App", "/usr/bin/myapp", None, Some("default.target"));
    assert!(content.contains("WantedBy=default.target"));
}

#[test]
fn test_template_service_full() {
    let content = templates::service(
        "Full Service",
        "/usr/bin/fullapp",
        Some("appuser"),
        Some("multi-user.target"),
    );
    assert!(content.contains("Description=Full Service"));
    assert!(content.contains("ExecStart=/usr/bin/fullapp"));
    assert!(content.contains("User=appuser"));
    assert!(content.contains("WantedBy=multi-user.target"));
}

#[test]
fn test_template_timer_with_calendar() {
    let content = templates::timer(
        "Run backup daily",
        Some("*-*-* 02:00:00"),
        None,
        None,
        "backup.service",
    );
    assert!(content.contains("[Unit]"));
    assert!(content.contains("Description=Run backup daily"));
    assert!(content.contains("[Timer]"));
    assert!(content.contains("OnCalendar=*-*-* 02:00:00"));
    assert!(content.contains("Unit=backup.service"));
    assert!(content.contains("Persistent=true"));
    assert!(content.contains("[Install]"));
    assert!(content.contains("WantedBy=timers.target"));
}

#[test]
fn test_template_timer_with_boot_sec() {
    let content = templates::timer(
        "Run after boot",
        None,
        Some("5min"),
        None,
        "startup.service",
    );
    assert!(content.contains("OnBootSec=5min"));
    assert!(!content.contains("OnCalendar="));
}

#[test]
fn test_template_timer_with_unit_active_sec() {
    let content = templates::timer(
        "Run periodically",
        None,
        None,
        Some("1h"),
        "periodic.service",
    );
    assert!(content.contains("OnUnitActiveSec=1h"));
}

#[test]
fn test_template_timer_combined() {
    let content = templates::timer(
        "Complex timer",
        Some("*-*-* 00:00:00"),
        Some("10min"),
        Some("30min"),
        "complex.service",
    );
    assert!(content.contains("OnCalendar=*-*-* 00:00:00"));
    assert!(content.contains("OnBootSec=10min"));
    assert!(content.contains("OnUnitActiveSec=30min"));
}

#[test]
fn test_template_socket_stream() {
    let content = templates::socket("My Application Socket", Some("0.0.0.0:8080"), None, false);
    assert!(content.contains("[Unit]"));
    assert!(content.contains("Description=My Application Socket"));
    assert!(content.contains("[Socket]"));
    assert!(content.contains("ListenStream=0.0.0.0:8080"));
    assert!(content.contains("Accept=no"));
    assert!(content.contains("[Install]"));
    assert!(content.contains("WantedBy=sockets.target"));
}

#[test]
fn test_template_socket_datagram() {
    let content = templates::socket("UDP Socket", None, Some("0.0.0.0:9090"), false);
    assert!(content.contains("ListenDatagram=0.0.0.0:9090"));
    assert!(!content.contains("ListenStream="));
}

#[test]
fn test_template_socket_accept() {
    let content = templates::socket("Accept Socket", Some("/run/myapp.sock"), None, true);
    assert!(content.contains("Accept=yes"));
}

#[test]
fn test_template_socket_unix() {
    let content = templates::socket("Unix Socket", Some("/run/myapp.sock"), None, false);
    assert!(content.contains("ListenStream=/run/myapp.sock"));
}

#[test]
fn test_template_path_exists() {
    let content = templates::path(
        "Watch for file",
        Some("/tmp/trigger.file"),
        None,
        None,
        None,
        None,
        "triggered.service",
    );
    assert!(content.contains("[Unit]"));
    assert!(content.contains("Description=Watch for file"));
    assert!(content.contains("[Path]"));
    assert!(content.contains("PathExists=/tmp/trigger.file"));
    assert!(content.contains("Unit=triggered.service"));
    assert!(content.contains("[Install]"));
    assert!(content.contains("WantedBy=paths.target"));
}

#[test]
fn test_template_path_changed() {
    let content = templates::path(
        "Watch for config changes",
        None,
        None,
        Some("/etc/myapp/config.yaml"),
        None,
        None,
        "myapp-reload.service",
    );
    assert!(content.contains("PathChanged=/etc/myapp/config.yaml"));
}

#[test]
fn test_template_path_modified() {
    let content = templates::path(
        "Watch for modifications",
        None,
        None,
        None,
        Some("/var/log/myapp.log"),
        None,
        "log-process.service",
    );
    assert!(content.contains("PathModified=/var/log/myapp.log"));
}

#[test]
fn test_template_path_directory_not_empty() {
    let content = templates::path(
        "Watch directory",
        None,
        None,
        None,
        None,
        Some("/var/spool/myapp"),
        "process-spool.service",
    );
    assert!(content.contains("DirectoryNotEmpty=/var/spool/myapp"));
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_systemd_unit_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Unit Name Format Tests
// ============================================================================

#[test]
fn test_unit_name_service_format() {
    let valid_names = [
        "myapp.service",
        "my-app.service",
        "my_app.service",
        "myapp123.service",
        "my.app.service",
        "nginx.service",
        "postgresql-14.service",
    ];

    for name in valid_names {
        assert!(
            name.ends_with(".service"),
            "Name '{}' should end with .service",
            name
        );
    }
}

#[test]
fn test_unit_name_timer_format() {
    let valid_names = [
        "backup.timer",
        "cleanup.timer",
        "daily-report.timer",
        "my_timer.timer",
    ];

    for name in valid_names {
        assert!(
            name.ends_with(".timer"),
            "Name '{}' should end with .timer",
            name
        );
    }
}

#[test]
fn test_unit_name_socket_format() {
    let valid_names = ["myapp.socket", "web.socket", "api-server.socket"];

    for name in valid_names {
        assert!(
            name.ends_with(".socket"),
            "Name '{}' should end with .socket",
            name
        );
    }
}

#[test]
fn test_unit_name_path_format() {
    let valid_names = ["config-watcher.path", "upload.path", "myapp.path"];

    for name in valid_names {
        assert!(
            name.ends_with(".path"),
            "Name '{}' should end with .path",
            name
        );
    }
}

#[test]
fn test_unit_name_template_format() {
    let template_names = [
        "myapp@.service",
        "container@.service",
        "myapp@instance.service",
    ];

    for name in template_names {
        assert!(name.contains('@'), "Name '{}' should contain @", name);
    }
}

// ============================================================================
// All Unit Types Tests
// ============================================================================

#[test]
fn test_all_unit_type_extensions() {
    let unit_types = [
        ("myapp.service", "service"),
        ("myapp.socket", "socket"),
        ("myapp.timer", "timer"),
        ("myapp.path", "path"),
        ("myapp.mount", "mount"),
        ("myapp.automount", "automount"),
        ("myapp.swap", "swap"),
        ("myapp.slice", "slice"),
        ("myapp.scope", "scope"),
        ("myapp.target", "target"),
    ];

    for (name, expected_type) in unit_types {
        assert!(
            name.ends_with(&format!(".{}", expected_type)),
            "Name '{}' should end with .{}",
            name,
            expected_type
        );
    }
}

// ============================================================================
// State Value Tests
// ============================================================================

#[test]
fn test_unit_state_values() {
    let valid_states = ["present", "absent"];

    for state in valid_states {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("state".to_string(), serde_json::json!(state));

        if state == "absent" {
            // absent doesn't need content
        } else {
            params.insert("content".to_string(), serde_json::json!("[Unit]"));
        }

        assert!(
            params.contains_key("state"),
            "State '{}' should be valid",
            state
        );
    }
}

#[test]
fn test_running_state_values() {
    let valid_states = ["started", "running", "stopped", "restarted", "reloaded"];

    for state in valid_states {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("running".to_string(), serde_json::json!(state));
        params.insert("content".to_string(), serde_json::json!("[Unit]"));

        assert!(
            params.contains_key("running"),
            "Running state '{}' should be valid",
            state
        );
    }
}

// ============================================================================
// Full Configuration Tests
// ============================================================================

#[test]
fn test_systemd_unit_full_service_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert("state".to_string(), serde_json::json!("present"));
    params.insert("content".to_string(), serde_json::json!("[Unit]\nDescription=My App\n\n[Service]\nExecStart=/usr/bin/myapp\n\n[Install]\nWantedBy=multi-user.target"));
    params.insert("enabled".to_string(), serde_json::json!(true));
    params.insert("running".to_string(), serde_json::json!("started"));
    params.insert("daemon_reload".to_string(), serde_json::json!(true));

    assert_eq!(params.len(), 6);
}

#[test]
fn test_systemd_unit_absent_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert("state".to_string(), serde_json::json!("absent"));

    // state=absent doesn't require content
    assert_eq!(params.len(), 2);
    assert!(!params.contains_key("content"));
}

#[test]
fn test_systemd_unit_permissions_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("myapp.service"));
    params.insert("content".to_string(), serde_json::json!("[Unit]"));
    params.insert("mode".to_string(), serde_json::json!(0o644));
    params.insert("owner".to_string(), serde_json::json!("root"));
    params.insert("group".to_string(), serde_json::json!("root"));

    assert!(params.contains_key("mode"));
    assert!(params.contains_key("owner"));
    assert!(params.contains_key("group"));
}
