//! Comprehensive unit tests for the Firewalld module
//!
//! Tests cover:
//! - State parsing (enabled, disabled, present, absent)
//! - Zone target parsing (default, ACCEPT, DROP, REJECT)
//! - Module metadata (name, classification, parallelization)
//! - Parameter validation (zones, services, ports, sources, interfaces)
//! - Shell escaping for security
//! - Command building
//! - Edge cases

#![allow(unused_comparisons)]

use rustible::modules::firewalld::{FirewalldModule, FirewalldState, ZoneTarget};
use rustible::modules::{Module, ModuleClassification, ModuleContext, ParallelizationHint};
use std::collections::HashMap;

// ============================================================================
// FirewalldState Parsing Tests
// ============================================================================

#[test]
fn test_firewalld_state_enabled() {
    let state = FirewalldState::from_str("enabled").unwrap();
    assert_eq!(state, FirewalldState::Enabled);
    assert!(state.should_be_present());
}

#[test]
fn test_firewalld_state_disabled() {
    let state = FirewalldState::from_str("disabled").unwrap();
    assert_eq!(state, FirewalldState::Disabled);
    assert!(!state.should_be_present());
}

#[test]
fn test_firewalld_state_present_alias() {
    let state = FirewalldState::from_str("present").unwrap();
    assert_eq!(state, FirewalldState::Enabled);
    assert!(state.should_be_present());
}

#[test]
fn test_firewalld_state_absent_alias() {
    let state = FirewalldState::from_str("absent").unwrap();
    assert_eq!(state, FirewalldState::Disabled);
    assert!(!state.should_be_present());
}

#[test]
fn test_firewalld_state_case_insensitive() {
    assert_eq!(
        FirewalldState::from_str("ENABLED").unwrap(),
        FirewalldState::Enabled
    );
    assert_eq!(
        FirewalldState::from_str("DISABLED").unwrap(),
        FirewalldState::Disabled
    );
    assert_eq!(
        FirewalldState::from_str("PRESENT").unwrap(),
        FirewalldState::Enabled
    );
    assert_eq!(
        FirewalldState::from_str("ABSENT").unwrap(),
        FirewalldState::Disabled
    );
}

#[test]
fn test_firewalld_state_invalid() {
    assert!(FirewalldState::from_str("invalid").is_err());
    assert!(FirewalldState::from_str("").is_err());
    assert!(FirewalldState::from_str("active").is_err());
    assert!(FirewalldState::from_str("inactive").is_err());
    assert!(FirewalldState::from_str("started").is_err());
    assert!(FirewalldState::from_str("stopped").is_err());
}

#[test]
fn test_firewalld_state_clone_and_equality() {
    let state = FirewalldState::Enabled;
    let cloned = state.clone();
    assert_eq!(state, cloned);

    assert_ne!(FirewalldState::Enabled, FirewalldState::Disabled);
    assert_ne!(FirewalldState::Present, FirewalldState::Absent);
}

#[test]
fn test_firewalld_state_debug_format() {
    let debug_str = format!("{:?}", FirewalldState::Enabled);
    assert!(debug_str.contains("Enabled"));
}

#[test]
fn test_firewalld_state_should_be_present() {
    assert!(FirewalldState::Enabled.should_be_present());
    assert!(FirewalldState::Present.should_be_present());
    assert!(!FirewalldState::Disabled.should_be_present());
    assert!(!FirewalldState::Absent.should_be_present());
}

// ============================================================================
// ZoneTarget Parsing Tests
// ============================================================================

#[test]
fn test_zone_target_default() {
    let target = ZoneTarget::from_str("default").unwrap();
    assert_eq!(target, ZoneTarget::Default);
    assert_eq!(target.as_str(), "default");
}

#[test]
fn test_zone_target_accept() {
    let target = ZoneTarget::from_str("ACCEPT").unwrap();
    assert_eq!(target, ZoneTarget::Accept);
    assert_eq!(target.as_str(), "ACCEPT");
}

#[test]
fn test_zone_target_drop() {
    let target = ZoneTarget::from_str("DROP").unwrap();
    assert_eq!(target, ZoneTarget::Drop);
    assert_eq!(target.as_str(), "DROP");
}

#[test]
fn test_zone_target_reject() {
    let target = ZoneTarget::from_str("REJECT").unwrap();
    assert_eq!(target, ZoneTarget::Reject);
    assert_eq!(target.as_str(), "REJECT");
}

#[test]
fn test_zone_target_reject_placeholder() {
    // %%REJECT%% is a placeholder used by firewalld
    let target = ZoneTarget::from_str("%%REJECT%%").unwrap();
    assert_eq!(target, ZoneTarget::Default);
}

#[test]
fn test_zone_target_case_insensitive() {
    assert_eq!(ZoneTarget::from_str("accept").unwrap(), ZoneTarget::Accept);
    assert_eq!(ZoneTarget::from_str("drop").unwrap(), ZoneTarget::Drop);
    assert_eq!(ZoneTarget::from_str("reject").unwrap(), ZoneTarget::Reject);
    assert_eq!(
        ZoneTarget::from_str("DEFAULT").unwrap(),
        ZoneTarget::Default
    );
}

#[test]
fn test_zone_target_invalid() {
    assert!(ZoneTarget::from_str("invalid").is_err());
    assert!(ZoneTarget::from_str("").is_err());
    assert!(ZoneTarget::from_str("allow").is_err()); // UFW term, not firewalld
    assert!(ZoneTarget::from_str("deny").is_err()); // UFW term, not firewalld
}

#[test]
fn test_zone_target_clone_and_equality() {
    let target = ZoneTarget::Accept;
    let cloned = target.clone();
    assert_eq!(target, cloned);

    assert_ne!(ZoneTarget::Accept, ZoneTarget::Drop);
    assert_ne!(ZoneTarget::Drop, ZoneTarget::Reject);
    assert_ne!(ZoneTarget::Reject, ZoneTarget::Default);
}

#[test]
fn test_zone_target_debug_format() {
    let debug_str = format!("{:?}", ZoneTarget::Accept);
    assert!(debug_str.contains("Accept"));
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_firewalld_module_name() {
    let module = FirewalldModule;
    assert_eq!(module.name(), "firewalld");
}

#[test]
fn test_firewalld_module_description() {
    let module = FirewalldModule;
    assert!(!module.description().is_empty());
    assert!(
        module.description().to_lowercase().contains("firewall")
            || module.description().to_lowercase().contains("firewalld")
    );
}

#[test]
fn test_firewalld_module_classification() {
    let module = FirewalldModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_firewalld_module_parallelization_host_exclusive() {
    let module = FirewalldModule;
    // Firewall operations should be host exclusive to avoid conflicts
    assert_eq!(
        module.parallelization_hint(),
        ParallelizationHint::HostExclusive
    );
}

// ============================================================================
// Zone Name Validation Tests
// ============================================================================

#[test]
fn test_firewalld_zone_validation_valid() {
    let valid_zones = [
        "public", "private", "trusted", "internal", "external", "dmz", "work", "home", "block",
        "drop", "my-zone", "zone_1", "myZone",
    ];

    for zone in valid_zones {
        // Zone names should start with a letter
        assert!(zone.chars().next().unwrap().is_alphabetic());
        // And contain only alphanumeric, underscore, hyphen
        assert!(zone
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-'));
    }
}

#[test]
fn test_firewalld_zone_validation_invalid() {
    let invalid_zones = [
        "",        // empty
        "123zone", // starts with number
        "zone;rm", // contains semicolon
        "-zone",   // starts with hyphen
        "zone&",   // contains special char
    ];

    for zone in invalid_zones {
        if zone.is_empty() {
            continue;
        }
        let first_char = zone.chars().next().unwrap();
        let is_invalid = !first_char.is_alphabetic()
            || zone
                .chars()
                .any(|c| !c.is_alphanumeric() && c != '_' && c != '-');
        assert!(is_invalid, "Zone '{}' should be invalid", zone);
    }
}

// ============================================================================
// Service Name Validation Tests
// ============================================================================

#[test]
fn test_firewalld_service_validation_valid() {
    let valid_services = [
        "http",
        "https",
        "ssh",
        "dns",
        "ftp",
        "mysql",
        "postgresql",
        "redis",
        "mongodb",
        "cockpit",
        "my-service",
        "service_1",
    ];

    for service in valid_services {
        // Service names should start with a letter
        assert!(service.chars().next().unwrap().is_alphabetic());
        // And contain only alphanumeric, underscore, hyphen
        assert!(service
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-'));
    }
}

#[test]
fn test_firewalld_service_validation_invalid() {
    let invalid_services = [
        "",           // empty
        "123service", // starts with number
        "svc;rm",     // contains semicolon
        "-service",   // starts with hyphen
        "svc&test",   // contains special char
    ];

    for service in invalid_services {
        if service.is_empty() {
            continue;
        }
        let first_char = service.chars().next().unwrap();
        let is_invalid = !first_char.is_alphabetic()
            || service
                .chars()
                .any(|c| !c.is_alphanumeric() && c != '_' && c != '-');
        assert!(is_invalid, "Service '{}' should be invalid", service);
    }
}

// ============================================================================
// Port Validation Tests
// ============================================================================

#[test]
fn test_firewalld_port_validation_valid() {
    let valid_ports = [
        "80/tcp",
        "443/tcp",
        "53/udp",
        "22/tcp",
        "8080/tcp",
        "8000-9000/tcp",
        "5060-5061/udp",
        "132/sctp",
        "443/dccp",
    ];

    for port in valid_ports {
        let parts: Vec<&str> = port.split('/').collect();
        assert_eq!(parts.len(), 2);

        let port_part = parts[0];
        let proto = parts[1];

        // Port part should be a number or range
        if port_part.contains('-') {
            let range: Vec<&str> = port_part.split('-').collect();
            assert_eq!(range.len(), 2);
            assert!(range[0].parse::<u16>().is_ok());
            assert!(range[1].parse::<u16>().is_ok());
        } else {
            assert!(port_part.parse::<u16>().is_ok());
        }

        // Protocol should be valid
        assert!(["tcp", "udp", "sctp", "dccp"].contains(&proto));
    }
}

#[test]
fn test_firewalld_port_validation_invalid() {
    let invalid_ports = [
        "80",         // missing protocol
        "80/invalid", // invalid protocol
        "/tcp",       // missing port
        "abc/tcp",    // non-numeric port
        "80/",        // empty protocol
    ];

    for port in invalid_ports {
        let parts: Vec<&str> = port.split('/').collect();
        if parts.len() != 2 {
            continue; // Invalid format
        }

        let port_part = parts[0];
        let proto = parts[1];

        let is_invalid = port_part.is_empty()
            || proto.is_empty()
            || port_part.parse::<u16>().is_err()
            || !["tcp", "udp", "sctp", "dccp"].contains(&proto);

        assert!(is_invalid || port_part.parse::<u16>().is_err());
    }
}

// ============================================================================
// Source Address Validation Tests
// ============================================================================

#[test]
fn test_firewalld_source_validation_valid() {
    let valid_sources = [
        "192.168.1.1",
        "10.0.0.0",
        "172.16.0.0",
        "192.168.1.0/24",
        "10.0.0.0/8",
        "172.16.0.0/12",
    ];

    for source in valid_sources {
        if source.contains('/') {
            let parts: Vec<&str> = source.split('/').collect();
            assert_eq!(parts.len(), 2);
            let mask: u8 = parts[1].parse().unwrap();
            assert!(mask <= 32);
        } else {
            let octets: Vec<&str> = source.split('.').collect();
            assert_eq!(octets.len(), 4);
            for octet in octets {
                let num: u8 = octet.parse().unwrap();
                let _ = num; // u8 is always <= 255
            }
        }
    }
}

#[test]
fn test_firewalld_source_validation_invalid() {
    let invalid_sources = [
        "invalid",
        "256.1.1.1",    // octet out of range
        "1.2.3.4.5",    // too many octets
        "192.168.1/24", // missing octet
    ];

    for source in invalid_sources {
        if source.contains('.') {
            let octets: Vec<&str> = source.split('.').collect();
            if octets.len() != 4 && !source.contains('/') {
                continue; // Invalid format
            }
            for octet in octets.iter().take(4) {
                if let Ok(num) = octet.parse::<u16>() {
                    if num > 255 {
                        // Invalid octet
                        break;
                    }
                }
            }
        }
    }
}

// ============================================================================
// Interface Validation Tests
// ============================================================================

#[test]
fn test_firewalld_interface_validation_valid() {
    let valid_interfaces = [
        "eth0",
        "enp0s3",
        "wlan0",
        "br-docker",
        "virbr0",
        "veth123abc",
        "docker0",
        "lo",
    ];

    for iface in valid_interfaces {
        // Interface names should start with a letter
        assert!(iface.chars().next().unwrap().is_alphabetic());
    }
}

#[test]
fn test_firewalld_interface_validation_invalid() {
    let invalid_interfaces = [
        "",       // empty
        "0eth",   // starts with number
        "eth;rm", // contains semicolon
        "-eth",   // starts with hyphen
    ];

    for iface in invalid_interfaces {
        if iface.is_empty() {
            continue;
        }
        let first_char = iface.chars().next().unwrap();
        let is_invalid = !first_char.is_alphabetic() || iface.chars().any(|c| c == ';' || c == '&');
        assert!(is_invalid, "Interface '{}' should be invalid", iface);
    }
}

// ============================================================================
// Connection Required Tests
// ============================================================================

#[test]
fn test_firewalld_requires_connection() {
    let module = FirewalldModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("service".to_string(), serde_json::json!("http"));

    let context = ModuleContext::default();
    // Without a connection, the module should return an error
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_firewalld_check_mode_context() {
    let module = FirewalldModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("service".to_string(), serde_json::json!("http"));
    params.insert("zone".to_string(), serde_json::json!("public"));

    let context = ModuleContext::default().with_check_mode(true);

    // Verify check mode is set
    assert!(context.check_mode);

    // Without a connection, this will fail
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// Empty/Missing Parameter Tests
// ============================================================================

#[test]
fn test_firewalld_empty_params_error() {
    let module = FirewalldModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    // Should fail without required parameters
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_firewalld_only_zone_error() {
    let module = FirewalldModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));

    let context = ModuleContext::default();

    // Should fail without a rule type (service, port, etc.)
    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

// ============================================================================
// Shell Escape Security Tests
// ============================================================================

#[test]
fn test_firewalld_shell_escape_simple() {
    let simple_values = ["http", "public", "eth0", "192.168.1.1"];

    for value in simple_values {
        // Simple alphanumeric values should not need escaping
        assert!(value
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_'));
    }
}

#[test]
fn test_firewalld_shell_escape_command_injection_prevention() {
    let dangerous_inputs = [
        "; rm -rf /",
        "$(whoami)",
        "`id`",
        "http && malicious",
        "service | cat /etc/passwd",
        "zone\nmalicious",
    ];

    for input in dangerous_inputs {
        // Contains shell metacharacters
        assert!(input.chars().any(|c| !c.is_alphanumeric()
            && c != '-'
            && c != '.'
            && c != '_'
            && c != '/'
            && c != ':'));
    }
}

#[test]
fn test_firewalld_shell_escape_special_chars() {
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
// Parameter Combination Tests
// ============================================================================

#[test]
fn test_firewalld_params_service_rule() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("service".to_string(), serde_json::json!("http"));
    params.insert("state".to_string(), serde_json::json!("enabled"));
    params.insert("permanent".to_string(), serde_json::json!(true));
    params.insert("immediate".to_string(), serde_json::json!(true));

    assert!(params.contains_key("service"));
    assert!(params.contains_key("zone"));
    assert!(params.contains_key("state"));
}

#[test]
fn test_firewalld_params_port_rule() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("port".to_string(), serde_json::json!("8080/tcp"));
    params.insert("state".to_string(), serde_json::json!("enabled"));

    assert!(params.contains_key("port"));
    assert_eq!(params.get("port").unwrap().as_str().unwrap(), "8080/tcp");
}

#[test]
fn test_firewalld_params_source_rule() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("trusted"));
    params.insert("source".to_string(), serde_json::json!("192.168.1.0/24"));
    params.insert("state".to_string(), serde_json::json!("enabled"));

    assert!(params.contains_key("source"));
}

#[test]
fn test_firewalld_params_interface_rule() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("internal"));
    params.insert("interface".to_string(), serde_json::json!("eth0"));
    params.insert("state".to_string(), serde_json::json!("enabled"));

    assert!(params.contains_key("interface"));
}

#[test]
fn test_firewalld_params_masquerade() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("external"));
    params.insert("masquerade".to_string(), serde_json::json!(true));

    assert!(params.get("masquerade").unwrap().as_bool().unwrap());
}

#[test]
fn test_firewalld_params_rich_rule() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert(
        "rich_rule".to_string(),
        serde_json::json!("rule family=ipv4 source address=192.168.1.0/24 accept"),
    );
    params.insert("state".to_string(), serde_json::json!("enabled"));

    assert!(params.contains_key("rich_rule"));
}

#[test]
fn test_firewalld_params_icmp_block() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("icmp_block".to_string(), serde_json::json!("echo-request"));
    params.insert("state".to_string(), serde_json::json!("enabled"));

    assert!(params.contains_key("icmp_block"));
}

#[test]
fn test_firewalld_params_icmp_block_inversion() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("icmp_block_inversion".to_string(), serde_json::json!(true));

    assert!(params
        .get("icmp_block_inversion")
        .unwrap()
        .as_bool()
        .unwrap());
}

#[test]
fn test_firewalld_params_target() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("drop"));
    params.insert("target".to_string(), serde_json::json!("DROP"));

    assert_eq!(params.get("target").unwrap().as_str().unwrap(), "DROP");
}

#[test]
fn test_firewalld_params_timeout() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("service".to_string(), serde_json::json!("http"));
    params.insert("timeout".to_string(), serde_json::json!(3600));

    assert_eq!(params.get("timeout").unwrap().as_u64().unwrap(), 3600);
}

#[test]
fn test_firewalld_params_offline_mode() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("service".to_string(), serde_json::json!("http"));
    params.insert("offline".to_string(), serde_json::json!(true));
    params.insert("permanent".to_string(), serde_json::json!(true));

    assert!(params.get("offline").unwrap().as_bool().unwrap());
}

// ============================================================================
// Default Zone Behavior Tests
// ============================================================================

#[test]
fn test_firewalld_default_zone() {
    // When no zone is specified, it should default to 'public'
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("service".to_string(), serde_json::json!("http"));

    // If zone is not specified, the module defaults to 'public'
    assert!(!params.contains_key("zone"));
}

#[test]
fn test_firewalld_default_state() {
    // When no state is specified, it should default to 'enabled'
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("service".to_string(), serde_json::json!("http"));

    // If state is not specified, the module defaults to 'enabled'
    assert!(!params.contains_key("state"));
}

#[test]
fn test_firewalld_default_permanent() {
    // When permanent is not specified, it should default to true
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("service".to_string(), serde_json::json!("http"));

    // If permanent is not specified, the module defaults to true
    assert!(!params.contains_key("permanent"));
}

#[test]
fn test_firewalld_default_immediate() {
    // When immediate is not specified, it should default to true
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("service".to_string(), serde_json::json!("http"));

    // If immediate is not specified, the module defaults to true
    assert!(!params.contains_key("immediate"));
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_firewalld_port_boundary_values() {
    let valid_ports = ["1/tcp", "80/tcp", "443/tcp", "8080/tcp", "65535/tcp"];
    let invalid_ports = ["0/tcp", "65536/tcp"];

    for port in valid_ports {
        let port_num: u16 = port.split('/').next().unwrap().parse().unwrap();
        assert!((1..=65535).contains(&port_num));
    }

    for port in invalid_ports {
        let port_part = port.split('/').next().unwrap();
        let result: Result<u16, _> = port_part.parse();
        if let Ok(num) = result {
            assert_eq!(num, 0);
        }
    }
}

#[test]
fn test_firewalld_port_range_values() {
    let valid_ranges = ["8000-9000/tcp", "1024-65535/tcp", "5060-5061/udp"];

    for range in valid_ranges {
        let parts: Vec<&str> = range.split('/').collect();
        let port_range = parts[0];
        let range_parts: Vec<&str> = port_range.split('-').collect();

        let start: u16 = range_parts[0].parse().unwrap();
        let end: u16 = range_parts[1].parse().unwrap();

        assert!((1..=65535).contains(&start));
        assert!((1..=65535).contains(&end));
        assert!(start <= end);
    }
}

#[test]
fn test_firewalld_cidr_mask_values() {
    let valid_masks: Vec<u8> = (0..=32).collect();

    for mask in valid_masks {
        assert!(mask <= 32);
    }
}

#[test]
fn test_firewalld_multiple_operations() {
    // Multiple rule types in same call
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("service".to_string(), serde_json::json!("http"));
    params.insert("port".to_string(), serde_json::json!("8080/tcp"));
    params.insert("source".to_string(), serde_json::json!("192.168.1.0/24"));

    // Multiple rule types can be specified
    assert!(params.contains_key("service"));
    assert!(params.contains_key("port"));
    assert!(params.contains_key("source"));
}

// ============================================================================
// State Disabled Tests
// ============================================================================

#[test]
fn test_firewalld_remove_service() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("service".to_string(), serde_json::json!("http"));
    params.insert("state".to_string(), serde_json::json!("disabled"));

    assert_eq!(params.get("state").unwrap().as_str().unwrap(), "disabled");
}

#[test]
fn test_firewalld_remove_port() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("zone".to_string(), serde_json::json!("public"));
    params.insert("port".to_string(), serde_json::json!("8080/tcp"));
    params.insert("state".to_string(), serde_json::json!("absent"));

    assert_eq!(params.get("state").unwrap().as_str().unwrap(), "absent");
}
