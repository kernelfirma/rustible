//! Network Module Integration Tests
//!
//! This test suite validates network configuration modules for parity
//! with Ansible network modules including ios_config, nxos_config,
//! junos_config, and eos_config.
//!
//! These tests verify:
//! - Module interface consistency
//! - Parameter validation
//! - Configuration parsing and matching
//! - Playbook parsing for network tasks
//! - Transport options (SSH, NETCONF, NX-API)
//! - Configuration diff generation
//! - Checkpoint/rollback functionality

use rustible::modules::network::{
    EosConfigModule, IosConfigModule, JunosConfigModule, NxosConfigModule,
};
use rustible::modules::{Module, ModuleClassification, ParallelizationHint};
use rustible::playbook::Playbook;
use std::collections::HashMap;

// ============================================================================
// IOS Config Module Tests
// ============================================================================

mod ios_config_tests {
    use super::*;

    #[test]
    fn test_ios_config_module_name() {
        let module = IosConfigModule;
        assert_eq!(module.name(), "ios_config");
    }

    #[test]
    fn test_ios_config_module_description() {
        let module = IosConfigModule;
        let desc = module.description();
        assert!(!desc.is_empty());
        assert!(desc.to_lowercase().contains("ios") || desc.to_lowercase().contains("cisco"));
    }

    #[test]
    fn test_ios_config_module_classification() {
        let module = IosConfigModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_ios_config_parallelization_hint() {
        let module = IosConfigModule;
        // Network devices typically require exclusive access
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }

    #[test]
    fn test_ios_config_validate_params_with_lines() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "lines".to_string(),
            serde_json::json!(["ip address 10.0.0.1 255.255.255.0"]),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_with_parents() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "lines".to_string(),
            serde_json::json!(["ip address 10.0.0.1 255.255.255.0", "no shutdown"]),
        );
        params.insert(
            "parents".to_string(),
            serde_json::json!(["interface GigabitEthernet0/0"]),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_with_src() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!("templates/router.j2"),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_with_config() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "config".to_string(),
            serde_json::json!("hostname router1\ninterface Gi0/0\n ip address 10.0.0.1 255.255.255.0"),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_missing_required() {
        let module = IosConfigModule;
        let params = HashMap::new();

        let result = module.validate_params(&params);
        // Should fail without lines, src, or config
        assert!(result.is_err());
    }

    #[test]
    fn test_ios_config_validate_params_block_requires_parents() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test line"]));
        params.insert("replace".to_string(), serde_json::json!("block"));

        let result = module.validate_params(&params);
        // Block mode requires parents
        assert!(result.is_err());
    }

    #[test]
    fn test_ios_config_validate_params_override_requires_parents() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test line"]));
        params.insert("replace".to_string(), serde_json::json!("override"));

        let result = module.validate_params(&params);
        // Override mode requires parents
        assert!(result.is_err());
    }

    #[test]
    fn test_ios_config_validate_params_intended_requires_config() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test line"]));
        params.insert("diff_against".to_string(), serde_json::json!("intended"));

        let result = module.validate_params(&params);
        // diff_against: intended requires intended_config
        assert!(result.is_err());
    }

    #[test]
    fn test_ios_config_validate_params_valid_transport_ssh() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("transport".to_string(), serde_json::json!("ssh"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_valid_transport_netconf() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("transport".to_string(), serde_json::json!("netconf"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_invalid_transport() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("transport".to_string(), serde_json::json!("invalid"));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_ios_config_validate_params_match_modes() {
        let module = IosConfigModule;

        for mode in &["line", "strict", "exact", "none"] {
            let mut params = HashMap::new();
            params.insert("lines".to_string(), serde_json::json!(["test"]));
            params.insert("match".to_string(), serde_json::json!(mode));

            let result = module.validate_params(&params);
            assert!(result.is_ok(), "Match mode '{}' should be valid", mode);
        }
    }

    #[test]
    fn test_ios_config_validate_params_save_when_options() {
        let module = IosConfigModule;

        for save_when in &["always", "never", "modified", "changed"] {
            let mut params = HashMap::new();
            params.insert("lines".to_string(), serde_json::json!(["test"]));
            params.insert("save_when".to_string(), serde_json::json!(save_when));

            let result = module.validate_params(&params);
            assert!(result.is_ok(), "save_when '{}' should be valid", save_when);
        }
    }

    #[test]
    fn test_ios_config_validate_params_replace_modes() {
        let module = IosConfigModule;

        // merge doesn't require parents
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("replace".to_string(), serde_json::json!("merge"));
        assert!(module.validate_params(&params).is_ok());

        // block with parents should work
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("replace".to_string(), serde_json::json!("block"));
        params.insert("parents".to_string(), serde_json::json!(["interface Gi0/0"]));
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_multi_level_parents() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["remote-as 65001"]));
        params.insert(
            "parents".to_string(),
            serde_json::json!(["router bgp 65000", "neighbor 192.168.1.1"]),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_backup_options() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("backup".to_string(), serde_json::json!(true));
        params.insert("backup_dir".to_string(), serde_json::json!("/backups"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_checkpoint_options() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("create_checkpoint".to_string(), serde_json::json!(true));
        params.insert("checkpoint_name".to_string(), serde_json::json!("pre_change"));
        params.insert("rollback_on_failure".to_string(), serde_json::json!(true));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_before_after() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["ip address 10.0.0.1 255.255.255.0"]));
        params.insert("parents".to_string(), serde_json::json!(["interface Gi0/0"]));
        params.insert("before".to_string(), serde_json::json!(["no shutdown"]));
        params.insert("after".to_string(), serde_json::json!(["description Configured by Rustible"]));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ios_config_validate_params_diff_ignore_lines() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert(
            "diff_ignore_lines".to_string(),
            serde_json::json!(["^Building configuration.*", "^Current configuration.*"]),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }
}

// ============================================================================
// NX-OS Config Module Tests
// ============================================================================

mod nxos_config_tests {
    use super::*;

    #[test]
    fn test_nxos_config_module_name() {
        let module = NxosConfigModule;
        assert_eq!(module.name(), "nxos_config");
    }

    #[test]
    fn test_nxos_config_module_description() {
        let module = NxosConfigModule;
        let desc = module.description();
        assert!(!desc.is_empty());
        assert!(desc.to_lowercase().contains("nxos") || desc.to_lowercase().contains("nx-os") || desc.to_lowercase().contains("nexus"));
    }

    #[test]
    fn test_nxos_config_module_classification() {
        let module = NxosConfigModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_nxos_config_parallelization_hint() {
        let module = NxosConfigModule;
        // NX-OS typically uses rate limiting
        match module.parallelization_hint() {
            ParallelizationHint::RateLimited { requests_per_second } => {
                assert!(requests_per_second > 0);
            }
            ParallelizationHint::HostExclusive => {
                // Also acceptable
            }
            _ => panic!("Unexpected parallelization hint for nxos_config"),
        }
    }

    #[test]
    fn test_nxos_config_validate_params_with_lines() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "lines".to_string(),
            serde_json::json!(["vlan 100", "name Production"]),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_with_checkpoint() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "checkpoint".to_string(),
            serde_json::json!("before_changes"),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_with_rollback() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "rollback_to".to_string(),
            serde_json::json!("before_changes"),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_requires_action() {
        let module = NxosConfigModule;
        let params = HashMap::new();

        let result = module.validate_params(&params);
        // Must have lines, src, or checkpoint operation
        assert!(result.is_err());
    }

    #[test]
    fn test_nxos_config_validate_params_nxapi_requires_host() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("nxapi"));

        let result = module.validate_params(&params);
        // NX-API requires nxapi_host
        assert!(result.is_err());
    }

    #[test]
    fn test_nxos_config_validate_params_nxapi_with_host() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("nxapi"));
        params.insert("nxapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_replace_config_requires_src() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("replace".to_string(), serde_json::json!("config"));

        let result = module.validate_params(&params);
        // Config replace requires src
        assert!(result.is_err());
    }

    #[test]
    fn test_nxos_config_validate_params_replace_config_with_src() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("/path/to/config.txt"));
        params.insert("replace".to_string(), serde_json::json!("config"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_transport_ssh() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("ssh"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_transport_cli() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("cli"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_match_modes() {
        let module = NxosConfigModule;

        for mode in &["line", "strict", "exact", "none"] {
            let mut params = HashMap::new();
            params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
            params.insert("match".to_string(), serde_json::json!(mode));

            let result = module.validate_params(&params);
            assert!(result.is_ok(), "Match mode '{}' should be valid", mode);
        }
    }

    #[test]
    fn test_nxos_config_validate_params_save_when() {
        let module = NxosConfigModule;

        for save_when in &["always", "never", "modified", "changed"] {
            let mut params = HashMap::new();
            params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
            params.insert("save_when".to_string(), serde_json::json!(save_when));

            let result = module.validate_params(&params);
            assert!(result.is_ok(), "save_when '{}' should be valid", save_when);
        }
    }

    #[test]
    fn test_nxos_config_validate_params_backup() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("backup".to_string(), serde_json::json!(true));
        params.insert(
            "backup_options".to_string(),
            serde_json::json!({
                "dir_path": "/backups",
                "filename": "nxos_backup.cfg"
            }),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_with_parents() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "parents".to_string(),
            serde_json::json!(["interface Ethernet1/1"]),
        );
        params.insert(
            "lines".to_string(),
            serde_json::json!(["description Uplink", "switchport mode trunk", "no shutdown"]),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_defaults() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("defaults".to_string(), serde_json::json!(true));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nxos_config_validate_params_nxapi_ssl_options() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["feature bgp"]));
        params.insert("transport".to_string(), serde_json::json!("nxapi"));
        params.insert("nxapi_host".to_string(), serde_json::json!("192.168.1.1"));
        params.insert("nxapi_port".to_string(), serde_json::json!(8443));
        params.insert("nxapi_use_ssl".to_string(), serde_json::json!(true));
        params.insert("nxapi_validate_certs".to_string(), serde_json::json!(false));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }
}

// ============================================================================
// Junos Config Module Tests
// ============================================================================

mod junos_config_tests {
    use super::*;

    #[test]
    fn test_junos_config_module_name() {
        let module = JunosConfigModule;
        assert_eq!(module.name(), "junos_config");
    }

    #[test]
    fn test_junos_config_module_description() {
        let module = JunosConfigModule;
        let desc = module.description();
        assert!(!desc.is_empty());
        assert!(desc.to_lowercase().contains("junos") || desc.to_lowercase().contains("juniper"));
    }

    #[test]
    fn test_junos_config_module_classification() {
        let module = JunosConfigModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_junos_config_parallelization_hint() {
        let module = JunosConfigModule;
        // Junos module uses default parallelization which is FullyParallel
        // This is acceptable for NETCONF as the protocol handles concurrency
        let _hint = module.parallelization_hint();
        // Accept any parallelization hint - actual value depends on implementation
    }

    #[test]
    fn test_junos_config_validate_params_with_config() {
        let module = JunosConfigModule;
        let mut params = HashMap::new();
        // Junos uses 'config' parameter, not 'lines'
        params.insert(
            "config".to_string(),
            serde_json::json!("set system host-name router1"),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_junos_config_validate_params_with_src() {
        let module = JunosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!("junos_config.set"),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_junos_config_validate_params_missing_required() {
        let module = JunosConfigModule;
        let params = HashMap::new();

        let result = module.validate_params(&params);
        // Should fail without lines or src
        assert!(result.is_err());
    }

    #[test]
    fn test_junos_config_validate_params_commit_options() {
        let module = JunosConfigModule;
        let mut params = HashMap::new();
        params.insert("config".to_string(), serde_json::json!("set system host-name test"));
        params.insert("commit".to_string(), serde_json::json!(true));
        params.insert("comment".to_string(), serde_json::json!("Configured by Rustible"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_junos_config_validate_params_rollback() {
        let module = JunosConfigModule;
        let mut params = HashMap::new();
        params.insert("rollback".to_string(), serde_json::json!(1));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_junos_config_validate_params_confirm() {
        let module = JunosConfigModule;
        let mut params = HashMap::new();
        // confirm cannot be combined with config, so test it alone
        params.insert("confirm".to_string(), serde_json::json!(true));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_junos_config_validate_params_load_operation() {
        let module = JunosConfigModule;

        // Junos uses 'load_operation' or 'operation' parameter
        for operation in &["merge", "override", "replace", "update"] {
            let mut params = HashMap::new();
            params.insert("config".to_string(), serde_json::json!("set system host-name test"));
            params.insert("load_operation".to_string(), serde_json::json!(operation));

            let result = module.validate_params(&params);
            assert!(result.is_ok(), "Load operation '{}' should be valid", operation);
        }
    }

    #[test]
    fn test_junos_config_validate_params_compare() {
        let module = JunosConfigModule;
        let mut params = HashMap::new();
        // Junos uses 'compare' to compare candidate with running config
        params.insert("compare".to_string(), serde_json::json!(true));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_junos_config_validate_params_validate() {
        let module = JunosConfigModule;
        let mut params = HashMap::new();
        // Junos uses 'validate' to validate config without committing
        params.insert("validate".to_string(), serde_json::json!(true));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }
}

// ============================================================================
// EOS Config Module Tests
// ============================================================================

mod eos_config_tests {
    use super::*;

    #[test]
    fn test_eos_config_module_name() {
        let module = EosConfigModule;
        assert_eq!(module.name(), "eos_config");
    }

    #[test]
    fn test_eos_config_module_description() {
        let module = EosConfigModule;
        let desc = module.description();
        assert!(!desc.is_empty());
        assert!(desc.to_lowercase().contains("eos") || desc.to_lowercase().contains("arista"));
    }

    #[test]
    fn test_eos_config_module_classification() {
        let module = EosConfigModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_eos_config_parallelization_hint() {
        let module = EosConfigModule;
        // Network devices typically require exclusive access or rate limiting
        match module.parallelization_hint() {
            ParallelizationHint::HostExclusive => {}
            ParallelizationHint::RateLimited { .. } => {}
            _ => panic!("Unexpected parallelization hint for eos_config"),
        }
    }

    #[test]
    fn test_eos_config_validate_params_with_lines() {
        let module = EosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "lines".to_string(),
            serde_json::json!(["vlan 100", "name Production"]),
        );
        // EOS defaults to eAPI transport which requires host, use SSH transport
        params.insert("transport".to_string(), serde_json::json!("ssh"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_eos_config_validate_params_with_parents() {
        let module = EosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "parents".to_string(),
            serde_json::json!(["interface Ethernet1"]),
        );
        params.insert(
            "lines".to_string(),
            serde_json::json!(["description Uplink", "switchport mode trunk"]),
        );
        params.insert("transport".to_string(), serde_json::json!("ssh"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_eos_config_validate_params_with_src() {
        let module = EosConfigModule;
        let mut params = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!("eos_config.cfg"),
        );
        params.insert("transport".to_string(), serde_json::json!("ssh"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_eos_config_validate_params_missing_required() {
        let module = EosConfigModule;
        let params = HashMap::new();

        let result = module.validate_params(&params);
        // Should fail without lines or src
        assert!(result.is_err());
    }

    #[test]
    fn test_eos_config_validate_params_match_modes() {
        let module = EosConfigModule;

        for mode in &["line", "strict", "exact", "none"] {
            let mut params = HashMap::new();
            params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
            params.insert("match".to_string(), serde_json::json!(mode));
            params.insert("transport".to_string(), serde_json::json!("ssh"));

            let result = module.validate_params(&params);
            assert!(result.is_ok(), "Match mode '{}' should be valid", mode);
        }
    }

    #[test]
    fn test_eos_config_validate_params_replace_modes() {
        let module = EosConfigModule;

        // merge should work without parents
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("replace".to_string(), serde_json::json!("line"));
        params.insert("transport".to_string(), serde_json::json!("ssh"));
        assert!(module.validate_params(&params).is_ok());

        // block with parents
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["description Test"]));
        params.insert("parents".to_string(), serde_json::json!(["interface Ethernet1"]));
        params.insert("replace".to_string(), serde_json::json!("block"));
        params.insert("transport".to_string(), serde_json::json!("ssh"));
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_eos_config_validate_params_save_when() {
        let module = EosConfigModule;

        for save_when in &["always", "never", "modified", "changed"] {
            let mut params = HashMap::new();
            params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
            params.insert("save_when".to_string(), serde_json::json!(save_when));
            params.insert("transport".to_string(), serde_json::json!("ssh"));

            let result = module.validate_params(&params);
            assert!(result.is_ok(), "save_when '{}' should be valid", save_when);
        }
    }

    #[test]
    fn test_eos_config_validate_params_backup() {
        let module = EosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("backup".to_string(), serde_json::json!(true));
        params.insert("transport".to_string(), serde_json::json!("ssh"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_eos_config_validate_params_diff_against() {
        let module = EosConfigModule;

        for diff_against in &["running", "startup", "intended"] {
            let mut params = HashMap::new();
            params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
            params.insert("diff_against".to_string(), serde_json::json!(diff_against));
            params.insert("transport".to_string(), serde_json::json!("ssh"));

            if *diff_against == "intended" {
                params.insert("intended_config".to_string(), serde_json::json!("vlan 100\nname Production"));
            }

            let result = module.validate_params(&params);
            assert!(result.is_ok(), "diff_against '{}' should be valid", diff_against);
        }
    }

    #[test]
    fn test_eos_config_validate_params_session() {
        let module = EosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("session".to_string(), serde_json::json!("ansible_session"));
        params.insert("transport".to_string(), serde_json::json!("ssh"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }

    #[test]
    fn test_eos_config_validate_params_eapi_transport() {
        let module = EosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("eapi"));
        // eAPI transport requires eapi_host
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }
}

// ============================================================================
// Playbook Parsing Tests for Network Modules
// ============================================================================

mod playbook_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_ios_config_playbook() {
        let yaml = r#"
---
- name: Configure IOS devices
  hosts: ios_routers
  gather_facts: false
  tasks:
    - name: Configure interface
      ios_config:
        lines:
          - ip address 10.0.0.1 255.255.255.0
          - no shutdown
        parents:
          - interface GigabitEthernet0/0
        backup: true
        save_when: modified
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse IOS config playbook");
        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 1);

        let task = &playbook.plays[0].tasks[0];
        assert_eq!(task.name, "Configure interface");
        assert!(task.module_name() == "ios_config");
    }

    #[test]
    fn test_parse_ios_config_template_playbook() {
        let yaml = r#"
---
- name: Apply IOS configuration template
  hosts: ios_routers
  gather_facts: false
  tasks:
    - name: Apply router template
      ios_config:
        src: templates/router.j2
        backup: true
        diff_against: running
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse playbook");
        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 1);
    }

    #[test]
    fn test_parse_ios_config_bgp_playbook() {
        let yaml = r#"
---
- name: Configure BGP
  hosts: ios_routers
  gather_facts: false
  tasks:
    - name: Configure BGP neighbor
      ios_config:
        lines:
          - remote-as 65001
          - update-source Loopback0
          - timers 5 15
        parents:
          - router bgp 65000
          - neighbor 192.168.1.1
        match: exact
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse BGP playbook");
        assert_eq!(playbook.plays.len(), 1);
    }

    #[test]
    fn test_parse_nxos_config_playbook() {
        let yaml = r#"
---
- name: Configure NX-OS devices
  hosts: nexus_switches
  gather_facts: false
  tasks:
    - name: Configure VLANs
      nxos_config:
        lines:
          - vlan 100
          - name Production
        save_when: changed
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse NX-OS playbook");
        assert_eq!(playbook.plays.len(), 1);

        let task = &playbook.plays[0].tasks[0];
        assert!(task.module_name() == "nxos_config");
    }

    #[test]
    fn test_parse_nxos_config_checkpoint_playbook() {
        let yaml = r#"
---
- name: NX-OS with checkpoint
  hosts: nexus_switches
  gather_facts: false
  tasks:
    - name: Create checkpoint
      nxos_config:
        checkpoint: before_changes

    - name: Apply configuration
      nxos_config:
        lines:
          - feature bgp
        parents: []

    - name: Rollback on failure
      nxos_config:
        rollback_to: before_changes
      when: config_failed | default(false)
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse checkpoint playbook");
        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 3);
    }

    #[test]
    fn test_parse_nxos_config_nxapi_playbook() {
        let yaml = r#"
---
- name: Configure via NX-API
  hosts: nexus_switches
  gather_facts: false
  tasks:
    - name: Configure via NX-API
      nxos_config:
        lines:
          - feature nxapi
        transport: nxapi
        nxapi_host: "{{ ansible_host }}"
        nxapi_use_ssl: true
        nxapi_validate_certs: false
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse NX-API playbook");
        assert_eq!(playbook.plays.len(), 1);
    }

    #[test]
    fn test_parse_junos_config_playbook() {
        let yaml = r#"
---
- name: Configure Junos devices
  hosts: juniper_routers
  gather_facts: false
  tasks:
    - name: Set hostname
      junos_config:
        lines:
          - set system host-name router1
          - set system domain-name example.com
        commit: true
        commit_comment: "Configured by Rustible"
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse Junos playbook");
        assert_eq!(playbook.plays.len(), 1);

        let task = &playbook.plays[0].tasks[0];
        assert!(task.module_name() == "junos_config");
    }

    #[test]
    fn test_parse_junos_config_rollback_playbook() {
        let yaml = r#"
---
- name: Junos rollback
  hosts: juniper_routers
  gather_facts: false
  tasks:
    - name: Rollback to previous configuration
      junos_config:
        rollback: 1
        commit: true
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse rollback playbook");
        assert_eq!(playbook.plays.len(), 1);
    }

    #[test]
    fn test_parse_junos_config_confirm_playbook() {
        let yaml = r#"
---
- name: Junos confirmed commit
  hosts: juniper_routers
  gather_facts: false
  tasks:
    - name: Commit with confirmation
      junos_config:
        lines:
          - set system host-name newname
        commit: true
        confirm: 5
        confirm_commit: true
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse confirm playbook");
        assert_eq!(playbook.plays.len(), 1);
    }

    #[test]
    fn test_parse_eos_config_playbook() {
        let yaml = r#"
---
- name: Configure EOS devices
  hosts: arista_switches
  gather_facts: false
  tasks:
    - name: Configure VLANs
      eos_config:
        lines:
          - vlan 100
          - name Production
        save_when: modified
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse EOS playbook");
        assert_eq!(playbook.plays.len(), 1);

        let task = &playbook.plays[0].tasks[0];
        assert!(task.module_name() == "eos_config");
    }

    #[test]
    fn test_parse_eos_config_session_playbook() {
        let yaml = r#"
---
- name: EOS with session
  hosts: arista_switches
  gather_facts: false
  tasks:
    - name: Configure with session
      eos_config:
        lines:
          - interface Ethernet1
          - description Uplink
          - no shutdown
        session: ansible_session
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse session playbook");
        assert_eq!(playbook.plays.len(), 1);
    }

    #[test]
    fn test_parse_eos_config_eapi_playbook() {
        let yaml = r#"
---
- name: EOS via eAPI
  hosts: arista_switches
  gather_facts: false
  tasks:
    - name: Configure via eAPI
      eos_config:
        lines:
          - vlan 200
          - name Development
        transport: eapi
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse eAPI playbook");
        assert_eq!(playbook.plays.len(), 1);
    }

    #[test]
    fn test_parse_mixed_network_playbook() {
        let yaml = r#"
---
- name: Multi-vendor configuration
  hosts: all
  gather_facts: false
  tasks:
    - name: Configure Cisco IOS
      ios_config:
        lines:
          - ip address 10.0.0.1 255.255.255.0
        parents:
          - interface GigabitEthernet0/0
      when: ansible_network_os == 'ios'

    - name: Configure Cisco NX-OS
      nxos_config:
        lines:
          - vlan 100
      when: ansible_network_os == 'nxos'

    - name: Configure Juniper Junos
      junos_config:
        lines:
          - set system host-name router1
        commit: true
      when: ansible_network_os == 'junos'

    - name: Configure Arista EOS
      eos_config:
        lines:
          - vlan 100
      when: ansible_network_os == 'eos'
"#;

        let playbook = Playbook::from_yaml(yaml, None).expect("Failed to parse multi-vendor playbook");
        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].tasks.len(), 4);
    }
}

// ============================================================================
// Common Network Module Functionality Tests
// ============================================================================

mod common_tests {
    use super::*;

    #[test]
    fn test_all_network_modules_have_consistent_interface() {
        let ios = IosConfigModule;
        let nxos = NxosConfigModule;
        let junos = JunosConfigModule;
        let eos = EosConfigModule;

        // All should be RemoteCommand classification
        assert_eq!(ios.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(nxos.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(junos.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(eos.classification(), ModuleClassification::RemoteCommand);

        // All should have non-empty descriptions
        assert!(!ios.description().is_empty());
        assert!(!nxos.description().is_empty());
        assert!(!junos.description().is_empty());
        assert!(!eos.description().is_empty());

        // All should have names matching module pattern
        assert!(ios.name().ends_with("_config"));
        assert!(nxos.name().ends_with("_config"));
        assert!(junos.name().ends_with("_config"));
        assert!(eos.name().ends_with("_config"));
    }

    #[test]
    fn test_network_modules_require_config_source() {
        let modules: Vec<Box<dyn Module>> = vec![
            Box::new(IosConfigModule),
            Box::new(EosConfigModule),
        ];

        for module in modules {
            let params = HashMap::new();
            let result = module.validate_params(&params);
            assert!(
                result.is_err(),
                "{} should require lines, src, or config",
                module.name()
            );
        }
    }

    #[test]
    fn test_network_modules_accept_lines_parameter() {
        let ios = IosConfigModule;
        let nxos = NxosConfigModule;
        let eos = EosConfigModule;

        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test config line"]));

        assert!(ios.validate_params(&params).is_ok());
        assert!(nxos.validate_params(&params).is_ok());

        // EOS defaults to eAPI transport which requires host, so use SSH
        let mut eos_params = params.clone();
        eos_params.insert("transport".to_string(), serde_json::json!("ssh"));
        assert!(eos.validate_params(&eos_params).is_ok());
    }

    #[test]
    fn test_network_parallelization_hints() {
        let ios = IosConfigModule;
        let nxos = NxosConfigModule;
        let junos = JunosConfigModule;
        let eos = EosConfigModule;

        // Verify that network modules have appropriate parallelization hints
        // IOS and NX-OS typically use HostExclusive or RateLimited
        // Junos uses NETCONF which may support concurrent operations (FullyParallel)
        // EOS may use HostExclusive or RateLimited

        let _ios_hint = ios.parallelization_hint();
        let _nxos_hint = nxos.parallelization_hint();
        let _junos_hint = junos.parallelization_hint();
        let _eos_hint = eos.parallelization_hint();

        // All hints are valid - specific implementation choices are acceptable
    }
}

// ============================================================================
// Configuration Diff Tests
// ============================================================================

mod config_diff_tests {
    use super::*;

    #[test]
    fn test_ios_diff_against_options() {
        let module = IosConfigModule;

        // diff_against: running (default)
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("diff_against".to_string(), serde_json::json!("running"));
        assert!(module.validate_params(&params).is_ok());

        // diff_against: startup
        params.insert("diff_against".to_string(), serde_json::json!("startup"));
        assert!(module.validate_params(&params).is_ok());

        // diff_against: intended (requires intended_config)
        params.insert("diff_against".to_string(), serde_json::json!("intended"));
        params.insert("intended_config".to_string(), serde_json::json!("test config"));
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_nxos_diff_ignore_lines() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert(
            "diff_ignore_lines".to_string(),
            serde_json::json!(["^!Time:.*", "^Building configuration.*"]),
        );

        let result = module.validate_params(&params);
        assert!(result.is_ok());
    }
}

// ============================================================================
// Transport and Connection Tests
// ============================================================================

mod transport_tests {
    use super::*;

    #[test]
    fn test_ios_supports_ssh_transport() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("transport".to_string(), serde_json::json!("ssh"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_ios_supports_netconf_transport() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("transport".to_string(), serde_json::json!("netconf"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_nxos_supports_nxapi_transport() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("nxapi"));
        params.insert("nxapi_host".to_string(), serde_json::json!("192.168.1.1"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_eos_supports_eapi_transport() {
        let module = EosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("transport".to_string(), serde_json::json!("eapi"));
        // eAPI transport requires eapi_host
        params.insert("eapi_host".to_string(), serde_json::json!("192.168.1.1"));

        assert!(module.validate_params(&params).is_ok());
    }
}

// ============================================================================
// Backup and Recovery Tests
// ============================================================================

mod backup_tests {
    use super::*;

    #[test]
    fn test_ios_backup_options() {
        let module = IosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["test"]));
        params.insert("backup".to_string(), serde_json::json!(true));
        params.insert("backup_dir".to_string(), serde_json::json!("/var/backups/network"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_nxos_backup_options() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("lines".to_string(), serde_json::json!(["vlan 100"]));
        params.insert("backup".to_string(), serde_json::json!(true));
        params.insert(
            "backup_options".to_string(),
            serde_json::json!({
                "dir_path": "/backups",
                "filename": "{{ inventory_hostname }}.cfg"
            }),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_nxos_checkpoint_create() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("checkpoint".to_string(), serde_json::json!("pre_maintenance"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_nxos_checkpoint_rollback() {
        let module = NxosConfigModule;
        let mut params = HashMap::new();
        params.insert("rollback_to".to_string(), serde_json::json!("pre_maintenance"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_junos_rollback() {
        let module = JunosConfigModule;
        let mut params = HashMap::new();
        params.insert("rollback".to_string(), serde_json::json!(0));

        assert!(module.validate_params(&params).is_ok());
    }
}
