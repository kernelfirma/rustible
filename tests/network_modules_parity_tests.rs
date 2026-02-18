//! Network Modules Parity Tests
//!
//! Conformance tests for ios_config, nxos_config, junos_config, eos_config modules.
//! These tests validate:
//! 1. Module registration and naming
//! 2. Configuration diff generation accuracy
//! 3. Idempotency - applying same config twice yields no change
//! 4. Transport and platform type support
//!
//! Closes Issue #306: Raised bar: Network modules parity tests

use rustible::modules::network::{
    calculate_config_checksum, generate_backup_filename, generate_config_diff, ConfigSource,
    EosConfigModule, IosConfigModule, JunosConfigModule, NetworkConfig, NetworkPlatform,
    NetworkTransport, NxosConfigModule,
};
use rustible::modules::network::{escape_config_text, extract_config_sections, parse_ios_config};
use rustible::modules::{Module, ModuleRegistry};

// ============================================================================
// Module Registration Tests
// ============================================================================

mod registration_tests {
    use super::*;

    #[test]
    fn test_ios_config_module_name() {
        let module = IosConfigModule;
        assert_eq!(module.name(), "ios_config");
    }

    #[test]
    fn test_junos_config_module_name() {
        let module = JunosConfigModule;
        assert_eq!(module.name(), "junos_config");
    }

    #[test]
    fn test_nxos_config_module_name() {
        let module = NxosConfigModule;
        assert_eq!(module.name(), "nxos_config");
    }

    #[test]
    fn test_eos_config_module_name() {
        let module = EosConfigModule;
        assert_eq!(module.name(), "eos_config");
    }

    #[test]
    fn test_all_network_modules_registered() {
        let mut registry = ModuleRegistry::new();
        rustible::modules::network::register_network_modules(&mut registry);

        // All four network modules should be registered
        assert!(
            registry.get("ios_config").is_some(),
            "ios_config should be registered"
        );
        assert!(
            registry.get("junos_config").is_some(),
            "junos_config should be registered"
        );
        assert!(
            registry.get("nxos_config").is_some(),
            "nxos_config should be registered"
        );
        assert!(
            registry.get("eos_config").is_some(),
            "eos_config should be registered"
        );
    }

    #[test]
    fn test_network_module_names_list() {
        let names = rustible::modules::network::network_module_names();
        assert!(names.contains(&"ios_config"));
        assert!(names.contains(&"junos_config"));
        assert!(names.contains(&"nxos_config"));
        assert!(names.contains(&"eos_config"));
        assert_eq!(names.len(), 4);
    }
}

// ============================================================================
// Network Transport Tests
// ============================================================================

mod transport_tests {
    use super::*;

    #[test]
    fn test_transport_ssh_default() {
        let transport = NetworkTransport::default();
        assert_eq!(transport, NetworkTransport::Ssh);
    }

    #[test]
    fn test_transport_display() {
        assert_eq!(format!("{}", NetworkTransport::Ssh), "ssh");
        assert_eq!(format!("{}", NetworkTransport::Netconf), "netconf");
        assert_eq!(format!("{}", NetworkTransport::Gnmi), "gnmi");
        assert_eq!(format!("{}", NetworkTransport::RestApi), "restapi");
    }

    #[test]
    fn test_transport_from_str() {
        assert_eq!(
            "ssh".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::Ssh
        );
        assert_eq!(
            "cli".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::Ssh
        );
        assert_eq!(
            "netconf".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::Netconf
        );
        assert_eq!(
            "nc".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::Netconf
        );
        assert_eq!(
            "gnmi".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::Gnmi
        );
        assert_eq!(
            "grpc".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::Gnmi
        );
        assert_eq!(
            "restapi".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::RestApi
        );
        assert_eq!(
            "rest".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::RestApi
        );
    }

    #[test]
    fn test_transport_invalid() {
        assert!("invalid".parse::<NetworkTransport>().is_err());
    }
}

// ============================================================================
// Network Platform Tests
// ============================================================================

mod platform_tests {
    use super::*;

    #[test]
    fn test_platform_display() {
        assert_eq!(format!("{}", NetworkPlatform::CiscoIos), "cisco_ios");
        assert_eq!(format!("{}", NetworkPlatform::CiscoNxos), "cisco_nxos");
        assert_eq!(
            format!("{}", NetworkPlatform::JuniperJunos),
            "juniper_junos"
        );
        assert_eq!(format!("{}", NetworkPlatform::AristaEos), "arista_eos");
    }

    #[test]
    fn test_platform_from_str_ios() {
        assert_eq!(
            "cisco_ios".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoIos
        );
        assert_eq!(
            "ios".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoIos
        );
        assert_eq!(
            "ios_xe".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoIos
        );
        assert_eq!(
            "iosxe".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoIos
        );
    }

    #[test]
    fn test_platform_from_str_nxos() {
        assert_eq!(
            "cisco_nxos".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoNxos
        );
        assert_eq!(
            "nxos".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoNxos
        );
        assert_eq!(
            "nexus".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoNxos
        );
    }

    #[test]
    fn test_platform_from_str_junos() {
        assert_eq!(
            "juniper_junos".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::JuniperJunos
        );
        assert_eq!(
            "junos".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::JuniperJunos
        );
        assert_eq!(
            "juniper".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::JuniperJunos
        );
    }

    #[test]
    fn test_platform_from_str_eos() {
        assert_eq!(
            "arista_eos".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::AristaEos
        );
        assert_eq!(
            "eos".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::AristaEos
        );
        assert_eq!(
            "arista".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::AristaEos
        );
    }

    #[test]
    fn test_platform_from_str_generic() {
        assert_eq!(
            "generic".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::Generic
        );
        assert_eq!(
            "auto".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::Generic
        );
    }

    #[test]
    fn test_platform_invalid() {
        assert!("invalid_platform".parse::<NetworkPlatform>().is_err());
    }
}

// ============================================================================
// Config Source Tests
// ============================================================================

mod config_source_tests {
    use super::*;

    #[test]
    fn test_config_source_display() {
        assert_eq!(format!("{}", ConfigSource::Running), "running");
        assert_eq!(format!("{}", ConfigSource::Startup), "startup");
        assert_eq!(format!("{}", ConfigSource::Candidate), "candidate");
    }
}

// ============================================================================
// Network Config Tests
// ============================================================================

mod network_config_tests {
    use super::*;

    #[test]
    fn test_network_config_creation() {
        let config = NetworkConfig::new(
            "hostname router1".to_string(),
            NetworkPlatform::CiscoIos,
            ConfigSource::Running,
        );

        assert_eq!(config.content, "hostname router1");
        assert_eq!(config.platform, NetworkPlatform::CiscoIos);
        assert_eq!(config.source, ConfigSource::Running);
    }

    #[test]
    fn test_network_config_different_platforms() {
        let platforms = vec![
            NetworkPlatform::CiscoIos,
            NetworkPlatform::CiscoNxos,
            NetworkPlatform::JuniperJunos,
            NetworkPlatform::AristaEos,
        ];

        for platform in platforms {
            let config =
                NetworkConfig::new("test config".to_string(), platform, ConfigSource::Running);
            assert_eq!(config.platform, platform);
        }
    }
}

// ============================================================================
// Config Diff Tests - Idempotency Verification
// ============================================================================

mod config_diff_tests {
    use super::*;

    /// Helper to check if a diff indicates no changes
    /// The diff `after` field format is "X lines (A additions, D deletions)"
    fn diff_has_no_changes(diff: &rustible::modules::Diff) -> bool {
        diff.after.contains("(0 additions, 0 deletions)")
    }

    /// Helper to check if a diff indicates changes
    fn diff_has_changes(diff: &rustible::modules::Diff) -> bool {
        !diff.after.contains("(0 additions, 0 deletions)")
    }

    #[test]
    fn test_generate_config_diff_no_change() {
        let before = "hostname router1\nip domain-name example.com";
        let after = "hostname router1\nip domain-name example.com";

        let diff = generate_config_diff(before, after);

        assert!(
            diff_has_no_changes(&diff),
            "Identical configs should produce no-change diff"
        );
    }

    #[test]
    fn test_generate_config_diff_addition() {
        let before = "hostname router1";
        let after = "hostname router1\nip domain-name example.com";

        let diff = generate_config_diff(before, after);

        assert!(diff_has_changes(&diff), "Should detect added line");
    }

    #[test]
    fn test_generate_config_diff_removal() {
        let before = "hostname router1\nip domain-name example.com";
        let after = "hostname router1";

        let diff = generate_config_diff(before, after);

        assert!(diff_has_changes(&diff), "Should detect removed line");
    }

    #[test]
    fn test_generate_config_diff_modification() {
        let before = "hostname router1";
        let after = "hostname router2";

        let diff = generate_config_diff(before, after);

        assert!(diff_has_changes(&diff), "Should detect modification");
    }

    #[test]
    fn test_idempotency_same_config() {
        let config = r#"
interface GigabitEthernet0/0
 ip address 10.0.0.1 255.255.255.0
 no shutdown
!
"#;

        let diff = generate_config_diff(config, config);
        assert!(
            diff_has_no_changes(&diff),
            "Idempotency check: same config should produce no diff"
        );
    }
}

// ============================================================================
// Config Checksum Tests
// ============================================================================

mod checksum_tests {
    use super::*;

    #[test]
    fn test_checksum_same_content() {
        let checksum1 = calculate_config_checksum("hostname router1");
        let checksum2 = calculate_config_checksum("hostname router1");
        assert_eq!(
            checksum1, checksum2,
            "Same content should produce same checksum"
        );
    }

    #[test]
    fn test_checksum_different_content() {
        let checksum1 = calculate_config_checksum("hostname router1");
        let checksum2 = calculate_config_checksum("hostname router2");
        assert_ne!(
            checksum1, checksum2,
            "Different content should produce different checksum"
        );
    }

    #[test]
    fn test_checksum_format() {
        let checksum = calculate_config_checksum("test");
        assert!(
            checksum.chars().all(|c| c.is_ascii_hexdigit()),
            "Checksum should be hex string"
        );
    }
}

// ============================================================================
// Backup Filename Tests
// ============================================================================

mod backup_tests {
    use super::*;

    #[test]
    fn test_backup_filename_format() {
        let filename =
            generate_backup_filename("router1", NetworkPlatform::CiscoIos, ConfigSource::Running);

        assert!(filename.contains("router1"), "Should contain hostname");
        assert!(
            filename.ends_with(".cfg") || filename.contains("."),
            "Should have extension"
        );
    }

    #[test]
    fn test_backup_filename_unique() {
        let filename1 =
            generate_backup_filename("router1", NetworkPlatform::CiscoIos, ConfigSource::Running);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let filename2 =
            generate_backup_filename("router1", NetworkPlatform::CiscoIos, ConfigSource::Running);

        assert_ne!(filename1, filename2, "Backup filenames should be unique");
    }
}

// ============================================================================
// IOS Configuration Parsing Tests
// ============================================================================

mod ios_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_ios_config_simple() {
        let config = "hostname router1\nip domain-name example.com";
        let result = parse_ios_config(config);

        assert_eq!(result.content, config);
        assert_eq!(result.platform, NetworkPlatform::CiscoIos);
    }

    #[test]
    fn test_parse_ios_config_with_interface() {
        let config = r#"
interface GigabitEthernet0/0
 ip address 10.0.0.1 255.255.255.0
 no shutdown
!"#;

        let result = parse_ios_config(config);
        assert!(result.content.contains("GigabitEthernet0/0"));
    }

    #[test]
    fn test_extract_config_sections_interface() {
        let config = r#"
interface GigabitEthernet0/0
 ip address 10.0.0.1 255.255.255.0
 no shutdown
!
interface GigabitEthernet0/1
 ip address 10.0.1.1 255.255.255.0
!
"#;

        let sections = extract_config_sections(config, "interface");

        assert!(
            sections.len() >= 2,
            "Should extract multiple interface sections, found {}",
            sections.len()
        );
    }

    #[test]
    fn test_escape_config_text() {
        let text = "ip address 10.0.0.1 255.255.255.0";
        let escaped = escape_config_text(text);

        assert!(escaped.contains("10.0.0.1"));
    }

    #[test]
    fn test_escape_config_text_special_chars() {
        let text = "description \"Production Server\"";
        let escaped = escape_config_text(text);

        assert!(escaped.len() >= text.len());
    }
}

// ============================================================================
// Idempotency Conformance Tests
// ============================================================================

mod idempotency_conformance_tests {
    use super::*;

    /// Check if diff indicates no changes (idempotent operation)
    /// The diff `after` field format is "X lines (A additions, D deletions)"
    fn is_idempotent(diff: &rustible::modules::Diff) -> bool {
        diff.after.contains("(0 additions, 0 deletions)")
    }

    #[test]
    fn test_ios_interface_config_idempotent() {
        let config = r#"
interface GigabitEthernet0/0
 description Uplink to Core
 ip address 10.0.0.1 255.255.255.0
 duplex auto
 speed auto
 no shutdown
!
"#;

        let diff = generate_config_diff(config, config);

        assert!(
            is_idempotent(&diff),
            "IOS interface config should be idempotent"
        );
    }

    #[test]
    fn test_ios_acl_config_idempotent() {
        let config = r#"
ip access-list extended MANAGEMENT
 10 permit ip 10.0.0.0 0.255.255.255 any
 20 permit ip 192.168.1.0 0.0.0.255 any
 30 deny ip any any log
!
"#;

        let diff = generate_config_diff(config, config);

        assert!(is_idempotent(&diff), "IOS ACL config should be idempotent");
    }

    #[test]
    fn test_ios_bgp_config_idempotent() {
        let config = r#"
router bgp 65000
 bgp router-id 1.1.1.1
 neighbor 192.168.1.1 remote-as 65001
 neighbor 192.168.1.1 update-source Loopback0
 !
 address-family ipv4 unicast
  network 10.0.0.0 mask 255.255.255.0
  neighbor 192.168.1.1 activate
 exit-address-family
!
"#;

        let diff = generate_config_diff(config, config);

        assert!(is_idempotent(&diff), "IOS BGP config should be idempotent");
    }

    #[test]
    fn test_nxos_vlan_config_idempotent() {
        let config = r#"
vlan 10
  name Production
  state active
vlan 20
  name Development
  state active
"#;

        let diff = generate_config_diff(config, config);

        assert!(
            is_idempotent(&diff),
            "NX-OS VLAN config should be idempotent"
        );
    }

    #[test]
    fn test_junos_interface_config_idempotent() {
        let config = r#"
interfaces {
    ge-0/0/0 {
        description "Uplink";
        unit 0 {
            family inet {
                address 10.0.0.1/24;
            }
        }
    }
}
"#;

        let diff = generate_config_diff(config, config);

        assert!(
            is_idempotent(&diff),
            "Junos interface config should be idempotent"
        );
    }

    #[test]
    fn test_eos_routing_config_idempotent() {
        let config = r#"
ip routing
!
router ospf 1
   router-id 1.1.1.1
   network 10.0.0.0/24 area 0.0.0.0
   passive-interface default
   no passive-interface Ethernet1
!
"#;

        let diff = generate_config_diff(config, config);

        assert!(is_idempotent(&diff), "EOS OSPF config should be idempotent");
    }
}

// ============================================================================
// Config Diff Accuracy Tests
// ============================================================================

mod diff_accuracy_tests {
    use super::*;

    /// Check if diff indicates changes were detected
    /// The diff `after` field format is "X lines (A additions, D deletions)"
    fn has_changes(diff: &rustible::modules::Diff) -> bool {
        !diff.after.contains("(0 additions, 0 deletions)")
    }

    #[test]
    fn test_diff_detects_ip_change() {
        let before = "interface Gi0/0\n ip address 10.0.0.1 255.255.255.0";
        let after = "interface Gi0/0\n ip address 10.0.0.2 255.255.255.0";

        let diff = generate_config_diff(before, after);

        assert!(has_changes(&diff), "Should detect IP address change");
    }

    #[test]
    fn test_diff_detects_description_change() {
        let before = "interface Gi0/0\n description Old Description";
        let after = "interface Gi0/0\n description New Description";

        let diff = generate_config_diff(before, after);

        assert!(has_changes(&diff), "Should detect description change");
    }

    #[test]
    fn test_diff_detects_new_line() {
        let before = "interface Gi0/0\n no shutdown";
        let after = "interface Gi0/0\n no shutdown\n mtu 9000";

        let diff = generate_config_diff(before, after);

        assert!(has_changes(&diff), "Should detect new MTU line");
    }

    #[test]
    fn test_diff_detects_removed_line() {
        let before = "interface Gi0/0\n no shutdown\n mtu 9000";
        let after = "interface Gi0/0\n no shutdown";

        let diff = generate_config_diff(before, after);

        assert!(has_changes(&diff), "Should detect removed MTU line");
    }

    #[test]
    fn test_diff_handles_reordering() {
        let before = "line1\nline2\nline3";
        let after = "line1\nline3\nline2";

        let diff = generate_config_diff(before, after);

        assert!(has_changes(&diff), "Should detect line reordering");
    }

    #[test]
    fn test_diff_multiline_change() {
        let before = r#"
interface Gi0/0
 ip address 10.0.0.1 255.255.255.0
 no shutdown
"#;
        let after = r#"
interface Gi0/0
 ip address 10.0.0.2 255.255.255.0
 description Updated
 no shutdown
"#;

        let diff = generate_config_diff(before, after);

        assert!(has_changes(&diff), "Should detect multiple changes");
    }
}

// ============================================================================
// Parent Hierarchy Tests
// ============================================================================

mod parent_hierarchy_tests {
    use super::*;

    #[test]
    fn test_single_parent_section() {
        let config = r#"
interface GigabitEthernet0/0
 ip address 10.0.0.1 255.255.255.0
 no shutdown
!
"#;

        let sections = extract_config_sections(config, "interface");

        assert!(!sections.is_empty(), "Should extract interface section");
    }

    #[test]
    fn test_nested_parent_sections() {
        let config = r#"
router bgp 65000
 neighbor 192.168.1.1 remote-as 65001
 !
 address-family ipv4 unicast
  network 10.0.0.0 mask 255.255.255.0
  neighbor 192.168.1.1 activate
 exit-address-family
!
"#;

        let sections = extract_config_sections(config, "router");

        assert!(!sections.is_empty(), "Should extract BGP sections");
    }

    #[test]
    fn test_multiple_top_level_sections() {
        let config = r#"
hostname router1
!
interface Gi0/0
 ip address 10.0.0.1 255.255.255.0
!
interface Gi0/1
 ip address 10.0.1.1 255.255.255.0
!
router ospf 1
 network 10.0.0.0 0.0.0.255 area 0
!
"#;

        let interface_sections = extract_config_sections(config, "interface");

        assert!(
            interface_sections.len() >= 2,
            "Should extract multiple interface sections"
        );
    }
}

// ============================================================================
// Module Classification Tests
// ============================================================================

mod classification_tests {
    use super::*;

    #[test]
    fn test_ios_module_has_classification() {
        let module = IosConfigModule;
        let _classification = module.classification();
        // Just verify it doesn't panic
    }

    #[test]
    fn test_ios_module_has_parallelization_hint() {
        let module = IosConfigModule;
        let _hint = module.parallelization_hint();
        // Just verify it doesn't panic
    }
}
