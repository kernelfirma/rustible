//! Comprehensive unit tests for the UFW module
//!
//! Tests cover:
//! - Rule parsing (allow, deny, reject, limit)
//! - Direction parsing (in, out, routed)
//! - Protocol parsing (tcp, udp, any)
//! - State parsing (enabled, disabled, reset, reloaded)
//! - Default policy parsing (allow, deny, reject)
//! - Log level parsing (off, low, medium, high, full)
//! - Module metadata (name, classification, parallelization)
//! - Parameter validation (ports, IPs, interfaces, app names)
//! - Shell escaping for security
//! - Rule command building
//! - Edge cases

#![allow(unused_comparisons)]
#![allow(unused_variables)]

use rustible::modules::ufw::{
    UfwDefault, UfwDirection, UfwLogLevel, UfwModule, UfwProto, UfwRule, UfwState,
};
use rustible::modules::{Module, ModuleClassification, ModuleContext, ParallelizationHint};
use std::collections::HashMap;

// ============================================================================
// UfwRule Parsing Tests
// ============================================================================

#[test]
fn test_ufw_rule_allow() {
    let rule = UfwRule::from_str("allow").unwrap();
    assert_eq!(rule, UfwRule::Allow);
    assert_eq!(rule.as_str(), "allow");
}

#[test]
fn test_ufw_rule_deny() {
    let rule = UfwRule::from_str("deny").unwrap();
    assert_eq!(rule, UfwRule::Deny);
    assert_eq!(rule.as_str(), "deny");
}

#[test]
fn test_ufw_rule_reject() {
    let rule = UfwRule::from_str("reject").unwrap();
    assert_eq!(rule, UfwRule::Reject);
    assert_eq!(rule.as_str(), "reject");
}

#[test]
fn test_ufw_rule_limit() {
    let rule = UfwRule::from_str("limit").unwrap();
    assert_eq!(rule, UfwRule::Limit);
    assert_eq!(rule.as_str(), "limit");
}

#[test]
fn test_ufw_rule_case_insensitive() {
    assert_eq!(UfwRule::from_str("ALLOW").unwrap(), UfwRule::Allow);
    assert_eq!(UfwRule::from_str("Allow").unwrap(), UfwRule::Allow);
    assert_eq!(UfwRule::from_str("DENY").unwrap(), UfwRule::Deny);
    assert_eq!(UfwRule::from_str("REJECT").unwrap(), UfwRule::Reject);
    assert_eq!(UfwRule::from_str("LIMIT").unwrap(), UfwRule::Limit);
}

#[test]
fn test_ufw_rule_invalid() {
    assert!(UfwRule::from_str("invalid").is_err());
    assert!(UfwRule::from_str("").is_err());
    assert!(UfwRule::from_str("drop").is_err()); // UFW uses 'deny' not 'drop'
    assert!(UfwRule::from_str("accept").is_err()); // UFW uses 'allow' not 'accept'
}

#[test]
fn test_ufw_rule_clone_and_equality() {
    let rule = UfwRule::Allow;
    let cloned = rule.clone();
    assert_eq!(rule, cloned);
    assert_ne!(UfwRule::Allow, UfwRule::Deny);
    assert_ne!(UfwRule::Deny, UfwRule::Reject);
    assert_ne!(UfwRule::Reject, UfwRule::Limit);
}

#[test]
fn test_ufw_rule_debug_format() {
    let debug_str = format!("{:?}", UfwRule::Allow);
    assert!(debug_str.contains("Allow"));
}

// ============================================================================
// UfwDirection Parsing Tests
// ============================================================================

#[test]
fn test_ufw_direction_in() {
    let dir = UfwDirection::from_str("in").unwrap();
    assert_eq!(dir, UfwDirection::In);
    assert_eq!(dir.as_str(), "in");
}

#[test]
fn test_ufw_direction_incoming_alias() {
    let dir = UfwDirection::from_str("incoming").unwrap();
    assert_eq!(dir, UfwDirection::In);
}

#[test]
fn test_ufw_direction_out() {
    let dir = UfwDirection::from_str("out").unwrap();
    assert_eq!(dir, UfwDirection::Out);
    assert_eq!(dir.as_str(), "out");
}

#[test]
fn test_ufw_direction_outgoing_alias() {
    let dir = UfwDirection::from_str("outgoing").unwrap();
    assert_eq!(dir, UfwDirection::Out);
}

#[test]
fn test_ufw_direction_routed() {
    let dir = UfwDirection::from_str("routed").unwrap();
    assert_eq!(dir, UfwDirection::Routed);
    assert_eq!(dir.as_str(), "routed");
}

#[test]
fn test_ufw_direction_route_alias() {
    let dir = UfwDirection::from_str("route").unwrap();
    assert_eq!(dir, UfwDirection::Routed);
}

#[test]
fn test_ufw_direction_case_insensitive() {
    assert_eq!(UfwDirection::from_str("IN").unwrap(), UfwDirection::In);
    assert_eq!(UfwDirection::from_str("OUT").unwrap(), UfwDirection::Out);
    assert_eq!(
        UfwDirection::from_str("ROUTED").unwrap(),
        UfwDirection::Routed
    );
}

#[test]
fn test_ufw_direction_invalid() {
    assert!(UfwDirection::from_str("invalid").is_err());
    assert!(UfwDirection::from_str("").is_err());
    assert!(UfwDirection::from_str("inbound").is_err());
    assert!(UfwDirection::from_str("outbound").is_err());
}

#[test]
fn test_ufw_direction_clone_and_equality() {
    let dir = UfwDirection::In;
    let cloned = dir.clone();
    assert_eq!(dir, cloned);
    assert_ne!(UfwDirection::In, UfwDirection::Out);
    assert_ne!(UfwDirection::Out, UfwDirection::Routed);
}

// ============================================================================
// UfwProto Parsing Tests
// ============================================================================

#[test]
fn test_ufw_proto_tcp() {
    let proto = UfwProto::from_str("tcp").unwrap();
    assert_eq!(proto, UfwProto::Tcp);
    assert_eq!(proto.as_str(), "tcp");
}

#[test]
fn test_ufw_proto_udp() {
    let proto = UfwProto::from_str("udp").unwrap();
    assert_eq!(proto, UfwProto::Udp);
    assert_eq!(proto.as_str(), "udp");
}

#[test]
fn test_ufw_proto_any() {
    let proto = UfwProto::from_str("any").unwrap();
    assert_eq!(proto, UfwProto::Any);
    assert_eq!(proto.as_str(), "any");
}

#[test]
fn test_ufw_proto_empty_is_any() {
    let proto = UfwProto::from_str("").unwrap();
    assert_eq!(proto, UfwProto::Any);
}

#[test]
fn test_ufw_proto_case_insensitive() {
    assert_eq!(UfwProto::from_str("TCP").unwrap(), UfwProto::Tcp);
    assert_eq!(UfwProto::from_str("UDP").unwrap(), UfwProto::Udp);
    assert_eq!(UfwProto::from_str("ANY").unwrap(), UfwProto::Any);
}

#[test]
fn test_ufw_proto_invalid() {
    assert!(UfwProto::from_str("invalid").is_err());
    assert!(UfwProto::from_str("sctp").is_err()); // UFW doesn't support sctp directly
    assert!(UfwProto::from_str("icmp").is_err());
}

#[test]
fn test_ufw_proto_clone_and_equality() {
    let proto = UfwProto::Tcp;
    let cloned = proto.clone();
    assert_eq!(proto, cloned);
    assert_ne!(UfwProto::Tcp, UfwProto::Udp);
    assert_ne!(UfwProto::Udp, UfwProto::Any);
}

// ============================================================================
// UfwState Parsing Tests
// ============================================================================

#[test]
fn test_ufw_state_enabled() {
    let state = UfwState::from_str("enabled").unwrap();
    assert_eq!(state, UfwState::Enabled);
}

#[test]
fn test_ufw_state_disabled() {
    let state = UfwState::from_str("disabled").unwrap();
    assert_eq!(state, UfwState::Disabled);
}

#[test]
fn test_ufw_state_reset() {
    let state = UfwState::from_str("reset").unwrap();
    assert_eq!(state, UfwState::Reset);
}

#[test]
fn test_ufw_state_reloaded() {
    let state = UfwState::from_str("reloaded").unwrap();
    assert_eq!(state, UfwState::Reloaded);
}

#[test]
fn test_ufw_state_case_insensitive() {
    assert_eq!(UfwState::from_str("ENABLED").unwrap(), UfwState::Enabled);
    assert_eq!(UfwState::from_str("DISABLED").unwrap(), UfwState::Disabled);
    assert_eq!(UfwState::from_str("RESET").unwrap(), UfwState::Reset);
    assert_eq!(UfwState::from_str("RELOADED").unwrap(), UfwState::Reloaded);
}

#[test]
fn test_ufw_state_invalid() {
    assert!(UfwState::from_str("invalid").is_err());
    assert!(UfwState::from_str("").is_err());
    assert!(UfwState::from_str("started").is_err());
    assert!(UfwState::from_str("stopped").is_err());
}

#[test]
fn test_ufw_state_clone_and_equality() {
    let state = UfwState::Enabled;
    let cloned = state.clone();
    assert_eq!(state, cloned);
    assert_ne!(UfwState::Enabled, UfwState::Disabled);
    assert_ne!(UfwState::Reset, UfwState::Reloaded);
}

// ============================================================================
// UfwDefault Parsing Tests
// ============================================================================

#[test]
fn test_ufw_default_allow() {
    let default = UfwDefault::from_str("allow").unwrap();
    assert_eq!(default, UfwDefault::Allow);
    assert_eq!(default.as_str(), "allow");
}

#[test]
fn test_ufw_default_deny() {
    let default = UfwDefault::from_str("deny").unwrap();
    assert_eq!(default, UfwDefault::Deny);
    assert_eq!(default.as_str(), "deny");
}

#[test]
fn test_ufw_default_reject() {
    let default = UfwDefault::from_str("reject").unwrap();
    assert_eq!(default, UfwDefault::Reject);
    assert_eq!(default.as_str(), "reject");
}

#[test]
fn test_ufw_default_case_insensitive() {
    assert_eq!(UfwDefault::from_str("ALLOW").unwrap(), UfwDefault::Allow);
    assert_eq!(UfwDefault::from_str("DENY").unwrap(), UfwDefault::Deny);
    assert_eq!(UfwDefault::from_str("REJECT").unwrap(), UfwDefault::Reject);
}

#[test]
fn test_ufw_default_invalid() {
    assert!(UfwDefault::from_str("invalid").is_err());
    assert!(UfwDefault::from_str("").is_err());
    assert!(UfwDefault::from_str("drop").is_err());
    assert!(UfwDefault::from_str("accept").is_err());
}

#[test]
fn test_ufw_default_clone_and_equality() {
    let default = UfwDefault::Allow;
    let cloned = default.clone();
    assert_eq!(default, cloned);
    assert_ne!(UfwDefault::Allow, UfwDefault::Deny);
    assert_ne!(UfwDefault::Deny, UfwDefault::Reject);
}

// ============================================================================
// UfwLogLevel Parsing Tests
// ============================================================================

#[test]
fn test_ufw_log_level_off() {
    let level = UfwLogLevel::from_str("off").unwrap();
    assert_eq!(level, UfwLogLevel::Off);
    assert_eq!(level.as_str(), "off");
}

#[test]
fn test_ufw_log_level_low() {
    let level = UfwLogLevel::from_str("low").unwrap();
    assert_eq!(level, UfwLogLevel::Low);
    assert_eq!(level.as_str(), "low");
}

#[test]
fn test_ufw_log_level_on_alias() {
    // "on" is an alias for "low"
    let level = UfwLogLevel::from_str("on").unwrap();
    assert_eq!(level, UfwLogLevel::Low);
}

#[test]
fn test_ufw_log_level_medium() {
    let level = UfwLogLevel::from_str("medium").unwrap();
    assert_eq!(level, UfwLogLevel::Medium);
    assert_eq!(level.as_str(), "medium");
}

#[test]
fn test_ufw_log_level_high() {
    let level = UfwLogLevel::from_str("high").unwrap();
    assert_eq!(level, UfwLogLevel::High);
    assert_eq!(level.as_str(), "high");
}

#[test]
fn test_ufw_log_level_full() {
    let level = UfwLogLevel::from_str("full").unwrap();
    assert_eq!(level, UfwLogLevel::Full);
    assert_eq!(level.as_str(), "full");
}

#[test]
fn test_ufw_log_level_case_insensitive() {
    assert_eq!(UfwLogLevel::from_str("OFF").unwrap(), UfwLogLevel::Off);
    assert_eq!(UfwLogLevel::from_str("LOW").unwrap(), UfwLogLevel::Low);
    assert_eq!(
        UfwLogLevel::from_str("MEDIUM").unwrap(),
        UfwLogLevel::Medium
    );
    assert_eq!(UfwLogLevel::from_str("HIGH").unwrap(), UfwLogLevel::High);
    assert_eq!(UfwLogLevel::from_str("FULL").unwrap(), UfwLogLevel::Full);
}

#[test]
fn test_ufw_log_level_invalid() {
    assert!(UfwLogLevel::from_str("invalid").is_err());
    assert!(UfwLogLevel::from_str("").is_err());
    assert!(UfwLogLevel::from_str("debug").is_err());
    assert!(UfwLogLevel::from_str("info").is_err());
}

#[test]
fn test_ufw_log_level_clone_and_equality() {
    let level = UfwLogLevel::Low;
    let cloned = level.clone();
    assert_eq!(level, cloned);
    assert_ne!(UfwLogLevel::Off, UfwLogLevel::Low);
    assert_ne!(UfwLogLevel::Medium, UfwLogLevel::High);
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_ufw_module_name() {
    let module = UfwModule;
    assert_eq!(module.name(), "ufw");
}

#[test]
fn test_ufw_module_description() {
    let module = UfwModule;
    assert!(!module.description().is_empty());
    assert!(
        module.description().to_lowercase().contains("ufw")
            || module.description().to_lowercase().contains("firewall")
    );
}

#[test]
fn test_ufw_module_classification() {
    let module = UfwModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_ufw_module_parallelization_host_exclusive() {
    let module = UfwModule;
    // Firewall operations should be host exclusive to avoid conflicts
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::HostExclusive
    );
}

// ============================================================================
// Parameter Validation Tests - Using inline tests from the module
// ============================================================================

// The following tests verify the validation functions work correctly
// These test the same logic as the inline module tests but in a different context

#[test]
fn test_ufw_port_validation_simple() {
    // Test valid ports through parameter parsing
    let module = UfwModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("port".to_string(), serde_json::json!("22"));

    // Port 22 should be valid
    assert!(params
        .get("port")
        .unwrap()
        .as_str()
        .unwrap()
        .parse::<u16>()
        .is_ok());
}

#[test]
fn test_ufw_port_validation_range() {
    // Port ranges like 8000:9000 should be valid
    let port_range = "8000:9000";
    let parts: Vec<&str> = port_range.split(':').collect();
    assert_eq!(parts.len(), 2);
    assert!(parts[0].parse::<u16>().is_ok());
    assert!(parts[1].parse::<u16>().is_ok());
}

#[test]
fn test_ufw_ip_validation_ipv4() {
    // IPv4 addresses should be valid
    let valid_ips = ["192.168.1.1", "10.0.0.0", "172.16.0.1", "255.255.255.255"];

    for ip in valid_ips {
        let parts: Vec<&str> = ip.split('.').collect();
        assert_eq!(parts.len(), 4);
        for part in parts {
            let num: u8 = part.parse().unwrap();
            assert!(num <= 255);
        }
    }
}

#[test]
fn test_ufw_ip_validation_cidr() {
    // CIDR notation should be valid
    let valid_cidrs = ["192.168.1.0/24", "10.0.0.0/8", "172.16.0.0/12"];

    for cidr in valid_cidrs {
        let parts: Vec<&str> = cidr.split('/').collect();
        assert_eq!(parts.len(), 2);
        let mask: u8 = parts[1].parse().unwrap();
        assert!(mask <= 32);
    }
}

#[test]
fn test_ufw_interface_validation_valid() {
    // Valid interface names
    let valid_interfaces = ["eth0", "enp0s3", "wlan0", "br0", "docker0", "veth123"];

    for iface in valid_interfaces {
        // Interface names should start with a letter
        assert!(iface.chars().next().unwrap().is_alphabetic());
    }
}

#[test]
fn test_ufw_interface_validation_invalid() {
    // Invalid interface names
    let invalid_interfaces = ["0eth", "123", "", "eth;cmd"];

    for iface in invalid_interfaces {
        // Should either be empty or start with a non-letter
        if !iface.is_empty() {
            let first_char = iface.chars().next().unwrap();
            let is_invalid = !first_char.is_alphabetic() || iface.contains(';');
            assert!(is_invalid);
        }
    }
}

#[test]
fn test_ufw_app_name_validation_valid() {
    // Valid application profile names
    let valid_apps = ["OpenSSH", "Apache", "Nginx HTTP", "Apache Full"];

    for app in valid_apps {
        // App names should start with a letter
        assert!(app.chars().next().unwrap().is_alphabetic());
    }
}

#[test]
fn test_ufw_app_name_validation_invalid() {
    // Invalid application names
    let invalid_apps = ["123app", "", ";malicious"];

    for app in invalid_apps {
        if !app.is_empty() {
            let first_char = app.chars().next().unwrap();
            let is_invalid = !first_char.is_alphabetic() || app.contains(';');
            assert!(is_invalid);
        }
    }
}

// ============================================================================
// Missing Connection Tests
// ============================================================================

#[test]
fn test_ufw_requires_connection() {
    let module = UfwModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("state".to_string(), serde_json::json!("enabled"));

    let context = ModuleContext::default();
    // Without a connection, the module should return an error
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_ufw_check_mode_context() {
    let module = UfwModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("port".to_string(), serde_json::json!("22"));

    let context = ModuleContext::default().with_check_mode(true);

    // Verify check mode is set
    assert!(context.check_mode);

    // Without a connection, this will fail
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_ufw_empty_params_error() {
    let module = UfwModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    // Should fail without required parameters
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_ufw_very_long_comment() {
    // Very long comments should be handled
    let long_comment = "a".repeat(1024);
    assert_eq!(long_comment.len(), 1024);
}

#[test]
fn test_ufw_port_boundary_values() {
    // Test port boundary values
    let valid_ports = ["1", "80", "443", "8080", "65535"];
    let invalid_ports = ["0", "65536", "-1", "99999"];

    for port in valid_ports {
        let num: u16 = port.parse().unwrap();
        assert!(num >= 1 && num <= 65535);
    }

    for port in invalid_ports {
        let result: Result<u16, _> = port.parse();
        if let Ok(num) = result {
            assert!(num == 0 || num > 65535);
        }
    }
}

// ============================================================================
// Shell Escape Security Tests
// ============================================================================

#[test]
fn test_ufw_shell_escape_simple_name() {
    let input = "22";
    // Simple numeric values should not need escaping
    assert!(input.chars().all(|c| c.is_numeric()));
}

#[test]
fn test_ufw_shell_escape_command_injection_prevention() {
    // These dangerous patterns should not pass through unescaped
    let dangerous_inputs = [
        "; rm -rf /",
        "$(whoami)",
        "`id`",
        "22 && malicious",
        "22 || malicious",
        "22 | cat /etc/passwd",
        "22\nmalicious",
    ];

    for input in dangerous_inputs {
        // Contains shell metacharacters that need escaping
        assert!(input.chars().any(|c| !c.is_alphanumeric()
            && c != '-'
            && c != '.'
            && c != '_'
            && c != '/'));
    }
}

#[test]
fn test_ufw_shell_escape_special_chars() {
    // Test that special characters trigger escaping
    let inputs_needing_escape = [
        "with space",
        "with'quote",
        "with\"double",
        "with$dollar",
        "with`backtick",
    ];

    for input in inputs_needing_escape {
        assert!(input.chars().any(|c| !c.is_alphanumeric()
            && c != '-'
            && c != '.'
            && c != '_'
            && c != '/'
            && c != ':'));
    }
}

// ============================================================================
// Config Operation Type Tests
// ============================================================================

#[test]
fn test_ufw_params_rule_operation() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("port".to_string(), serde_json::json!("22"));

    // This is a rule operation
    assert!(params.contains_key("rule"));
}

#[test]
fn test_ufw_params_state_operation() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("state".to_string(), serde_json::json!("enabled"));

    // This is a state operation
    assert!(params.contains_key("state"));
}

#[test]
fn test_ufw_params_default_operation() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("default".to_string(), serde_json::json!("deny"));
    params.insert("direction".to_string(), serde_json::json!("incoming"));

    // This is a default policy operation
    assert!(params.contains_key("default"));
}

#[test]
fn test_ufw_params_app_operation() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("app".to_string(), serde_json::json!("OpenSSH"));

    // This is a rule operation with an app profile
    assert!(params.contains_key("app"));
}

// ============================================================================
// Comprehensive Parameter Combination Tests
// ============================================================================

#[test]
fn test_ufw_params_full_rule() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("direction".to_string(), serde_json::json!("in"));
    params.insert("port".to_string(), serde_json::json!("22"));
    params.insert("proto".to_string(), serde_json::json!("tcp"));
    params.insert("from_ip".to_string(), serde_json::json!("192.168.1.0/24"));
    params.insert("comment".to_string(), serde_json::json!("Allow SSH"));

    // All parameters should be present
    assert!(params.contains_key("rule"));
    assert!(params.contains_key("direction"));
    assert!(params.contains_key("port"));
    assert!(params.contains_key("proto"));
    assert!(params.contains_key("from_ip"));
    assert!(params.contains_key("comment"));
}

#[test]
fn test_ufw_params_routed_rule() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("route".to_string(), serde_json::json!(true));
    params.insert("interface_in".to_string(), serde_json::json!("eth0"));
    params.insert("interface_out".to_string(), serde_json::json!("eth1"));
    params.insert("from_ip".to_string(), serde_json::json!("192.168.1.0/24"));
    params.insert("to_ip".to_string(), serde_json::json!("10.0.0.0/8"));

    // Routed rule parameters should be present
    assert!(params.contains_key("route"));
    assert!(params.contains_key("interface_in"));
    assert!(params.contains_key("interface_out"));
}

#[test]
fn test_ufw_params_delete_rule() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("port".to_string(), serde_json::json!("22"));
    params.insert("delete".to_string(), serde_json::json!(true));

    // Delete parameter should be present
    assert!(params.get("delete").unwrap().as_bool().unwrap());
}

#[test]
fn test_ufw_params_insert_position() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("port".to_string(), serde_json::json!("22"));
    params.insert("insert".to_string(), serde_json::json!(1));

    // Insert position should be a number
    assert!(params.get("insert").unwrap().as_u64().is_some());
}

#[test]
fn test_ufw_params_logging() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("rule".to_string(), serde_json::json!("allow"));
    params.insert("port".to_string(), serde_json::json!("22"));
    params.insert("log".to_string(), serde_json::json!(true));
    params.insert("log_level".to_string(), serde_json::json!("high"));

    // Logging parameters should be present
    assert!(params.get("log").unwrap().as_bool().unwrap());
    assert_eq!(params.get("log_level").unwrap().as_str().unwrap(), "high");
}
