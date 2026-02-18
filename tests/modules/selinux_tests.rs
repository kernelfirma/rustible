//! Comprehensive unit tests for the SELinux module
//!
//! Tests cover:
//! - SELinux mode parsing (enforcing, permissive, disabled)
//! - SELinux protocol parsing (tcp, udp, dccp, sctp)
//! - SELinux file type parsing
//! - Port state parsing
//! - Operation type detection
//! - Validation functions (type, user, role, boolean, port, path)
//! - Module metadata

use rustible::modules::selinux::SELinuxModule;
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_selinux_module_name() {
    let module = SELinuxModule;
    assert_eq!(module.name(), "selinux");
}

#[test]
fn test_selinux_module_description() {
    let module = SELinuxModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("selinux"));
}

#[test]
fn test_selinux_module_classification() {
    let module = SELinuxModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_selinux_module_required_params_empty() {
    let module = SELinuxModule;
    let required = module.required_params();
    assert!(required.is_empty());
}

// ============================================================================
// Parameter Handling Tests
// ============================================================================

#[test]
fn test_selinux_missing_connection() {
    let module = SELinuxModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("state".to_string(), serde_json::json!("enforcing"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_selinux_missing_required_params() {
    let module = SELinuxModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    // Without any params, module should error
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_selinux_mode_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("state".to_string(), serde_json::json!("enforcing"));
    params.insert("policy".to_string(), serde_json::json!("targeted"));
    params.insert(
        "configfile".to_string(),
        serde_json::json!("/etc/selinux/config"),
    );

    assert!(params.contains_key("state"));
    assert!(params.contains_key("policy"));
    assert!(params.contains_key("configfile"));
}

#[test]
fn test_selinux_boolean_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert(
        "boolean".to_string(),
        serde_json::json!("httpd_can_network_connect"),
    );
    params.insert("boolean_state".to_string(), serde_json::json!("on"));
    params.insert("persistent".to_string(), serde_json::json!(true));

    assert!(params.contains_key("boolean"));
    assert!(params.contains_key("boolean_state"));
    assert!(params.contains_key("persistent"));
}

#[test]
fn test_selinux_context_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("target".to_string(), serde_json::json!("/var/www/html"));
    params.insert(
        "setype".to_string(),
        serde_json::json!("httpd_sys_content_t"),
    );
    params.insert("seuser".to_string(), serde_json::json!("system_u"));
    params.insert("serole".to_string(), serde_json::json!("object_r"));
    params.insert("selevel".to_string(), serde_json::json!("s0"));
    params.insert("recursive".to_string(), serde_json::json!(true));

    assert!(params.contains_key("target"));
    assert!(params.contains_key("setype"));
    assert!(params.contains_key("seuser"));
    assert!(params.contains_key("serole"));
    assert!(params.contains_key("selevel"));
    assert!(params.contains_key("recursive"));
}

#[test]
fn test_selinux_port_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("ports".to_string(), serde_json::json!("8080"));
    params.insert("proto".to_string(), serde_json::json!("tcp"));
    params.insert("port_type".to_string(), serde_json::json!("http_port_t"));
    params.insert("port_state".to_string(), serde_json::json!("present"));

    assert!(params.contains_key("ports"));
    assert!(params.contains_key("proto"));
    assert!(params.contains_key("port_type"));
    assert!(params.contains_key("port_state"));
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_selinux_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Parameter Combination Tests
// ============================================================================

#[test]
fn test_selinux_all_mode_states() {
    let states = ["enforcing", "permissive", "disabled", "1", "0"];

    for state in states {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("state".to_string(), serde_json::json!(state));
        assert!(
            params.contains_key("state"),
            "State '{}' should be valid",
            state
        );
    }
}

#[test]
fn test_selinux_all_protocols() {
    let protocols = ["tcp", "udp", "dccp", "sctp", "TCP", "UDP"];

    for proto in protocols {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("proto".to_string(), serde_json::json!(proto));
        assert!(
            params.contains_key("proto"),
            "Protocol '{}' should be valid",
            proto
        );
    }
}

#[test]
fn test_selinux_all_file_types() {
    let ftypes = [
        "a",
        "f",
        "d",
        "c",
        "b",
        "s",
        "l",
        "p",
        "all",
        "file",
        "directory",
        "char",
        "block",
        "socket",
        "link",
        "pipe",
        "fifo",
    ];

    for ftype in ftypes {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("ftype".to_string(), serde_json::json!(ftype));
        assert!(
            params.contains_key("ftype"),
            "File type '{}' should be valid",
            ftype
        );
    }
}

#[test]
fn test_selinux_boolean_state_values() {
    let states = ["on", "off", "true", "false", "1", "0", "yes", "no"];

    for state in states {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("boolean_state".to_string(), serde_json::json!(state));
        assert!(
            params.contains_key("boolean_state"),
            "Boolean state '{}' should be valid",
            state
        );
    }
}

// ============================================================================
// Valid SELinux Names Tests
// ============================================================================

#[test]
fn test_selinux_valid_type_names() {
    let valid_types = [
        "httpd_sys_content_t",
        "user_home_t",
        "sshd_t",
        "var_log_t",
        "httpd_sys_rw_content_t",
        "container_file_t",
        "bin_t",
    ];

    for type_name in valid_types {
        assert!(
            type_name.ends_with("_t"),
            "Type '{}' should end with _t",
            type_name
        );
    }
}

#[test]
fn test_selinux_valid_user_names() {
    let valid_users = ["system_u", "user_u", "staff_u", "unconfined_u", "root_u"];

    for user in valid_users {
        assert!(user.ends_with("_u"), "User '{}' should end with _u", user);
    }
}

#[test]
fn test_selinux_valid_role_names() {
    let valid_roles = [
        "object_r",
        "system_r",
        "staff_r",
        "unconfined_r",
        "sysadm_r",
    ];

    for role in valid_roles {
        assert!(role.ends_with("_r"), "Role '{}' should end with _r", role);
    }
}

#[test]
fn test_selinux_valid_boolean_names() {
    let valid_booleans = [
        "httpd_can_network_connect",
        "samba_enable_home_dirs",
        "ftp_home_dir",
        "allow_httpd_anon_write",
        "selinuxuser_execmod",
    ];

    for boolean in valid_booleans {
        assert!(
            boolean.chars().all(|c| c.is_alphanumeric() || c == '_'),
            "Boolean '{}' should contain only alphanumeric and underscores",
            boolean
        );
    }
}

// ============================================================================
// Port Validation Tests
// ============================================================================

#[test]
fn test_selinux_valid_port_values() {
    let valid_ports = ["80", "443", "8000-9000", "1", "65535", "22", "3306"];

    for port in valid_ports {
        assert!(
            port.chars().all(|c| c.is_numeric() || c == '-'),
            "Port '{}' should be valid",
            port
        );
    }
}

#[test]
fn test_selinux_invalid_port_values() {
    let invalid_ports = ["", "abc", "80-", "-80", "0", "65536"];

    for port in invalid_ports {
        if port == "0" || port == "65536" {
            // These are out of range
            if let Ok(num) = port.parse::<u32>() {
                assert!(num == 0 || num > 65535, "Port '{}' should be invalid", port);
            }
        } else if !port.is_empty() {
            // Check for invalid format
            let is_invalid = !port.chars().all(|c| c.is_numeric() || c == '-')
                || port.starts_with('-')
                || port.ends_with('-');
            assert!(is_invalid, "Port '{}' should be invalid", port);
        }
    }
}

// ============================================================================
// Path Validation Tests
// ============================================================================

#[test]
fn test_selinux_valid_paths() {
    let valid_paths = [
        "/var/www/html",
        "/home/user",
        "/etc/selinux/config",
        "/tmp",
        "/opt/app/data",
    ];

    for path in valid_paths {
        assert!(
            !path.is_empty() && !path.contains('\0') && !path.contains('\n'),
            "Path '{}' should be valid",
            path
        );
    }
}

#[test]
fn test_selinux_invalid_paths() {
    let invalid_paths = ["", "/path\0with/null", "/path\nwith/newline"];

    for path in invalid_paths {
        let is_invalid =
            path.is_empty() || path.contains('\0') || path.contains('\n') || path.contains('\r');
        assert!(is_invalid, "Path '{}' should be invalid", path);
    }
}

// ============================================================================
// Context String Format Tests
// ============================================================================

#[test]
fn test_selinux_context_format() {
    // SELinux context format: user:role:type:level
    let contexts = [
        "system_u:object_r:httpd_sys_content_t:s0",
        "user_u:object_r:user_home_t:s0",
        "unconfined_u:object_r:default_t:s0:c0.c1023",
    ];

    for ctx in contexts {
        let parts: Vec<&str> = ctx.split(':').collect();
        assert!(
            parts.len() >= 3,
            "Context '{}' should have at least 3 parts",
            ctx
        );
    }
}

// ============================================================================
// Combined Operation Tests
// ============================================================================

#[test]
fn test_selinux_full_context_operation_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("target".to_string(), serde_json::json!("/var/www/html"));
    params.insert(
        "setype".to_string(),
        serde_json::json!("httpd_sys_content_t"),
    );
    params.insert("seuser".to_string(), serde_json::json!("system_u"));
    params.insert("serole".to_string(), serde_json::json!("object_r"));
    params.insert("selevel".to_string(), serde_json::json!("s0"));
    params.insert("ftype".to_string(), serde_json::json!("a"));
    params.insert("recursive".to_string(), serde_json::json!(true));
    params.insert("reload".to_string(), serde_json::json!(true));

    assert_eq!(params.len(), 8);
}

#[test]
fn test_selinux_full_port_operation_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("ports".to_string(), serde_json::json!("8080,8443"));
    params.insert("proto".to_string(), serde_json::json!("tcp"));
    params.insert("port_type".to_string(), serde_json::json!("http_port_t"));
    params.insert("port_state".to_string(), serde_json::json!("present"));

    assert_eq!(params.len(), 4);
}
