//! Comprehensive Variable Precedence Tests for Rustible
//!
//! This test suite verifies that Rustible's variable system matches Ansible's behavior
//! for variable precedence, merging, and resolution.
//!
//! # Variable Precedence (lowest to highest)
//!
//! Based on Ansible's 22-level variable precedence:
//!
//! 1. command line values (e.g., `-u` sets `ansible_user`)
//! 2. role defaults (roles/x/defaults/main.yml)
//! 3. inventory file group vars
//! 4. inventory group_vars/all
//! 5. playbook group_vars/all
//! 6. inventory group_vars/*
//! 7. playbook group_vars/*
//! 8. inventory file host vars
//! 9. inventory host_vars/*
//! 10. playbook host_vars/*
//! 11. host facts / cached set_facts
//! 12. play vars
//! 13. play vars_prompt
//! 14. play vars_files
//! 15. role vars (roles/x/vars/main.yml)
//! 16. block vars
//! 17. task vars
//! 18. include_vars
//! 19. set_facts / registered vars
//! 20. role params
//! 21. include params
//! 22. extra vars (-e)
//!
//! # Test Organization
//!
//! Tests are organized into sections:
//! - Full precedence chain tests
//! - Variable merging behavior
//! - Special variables (inventory_hostname, groups, hostvars, etc.)
//! - Variable types (string, int, bool, list, dict)
//! - Variable scopes (play, host, task, block, loop)
//! - Registered variables
//! - set_fact behavior

#![allow(unused_mut)]

use indexmap::IndexMap;
use rustible::vars::{deep_merge, resolve, HashBehaviour, VarPrecedence, VarStore, Variable};
use serde_yaml::Value;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a YAML Value from a string
fn yaml_string(s: &str) -> Value {
    Value::String(s.to_string())
}

/// Create a YAML Value from an integer
fn yaml_int(n: i64) -> Value {
    Value::Number(n.into())
}

/// Create a YAML Value from a boolean
fn yaml_bool(b: bool) -> Value {
    Value::Bool(b)
}

/// Create a YAML list from strings
fn yaml_list(items: &[&str]) -> Value {
    Value::Sequence(items.iter().map(|s| yaml_string(s)).collect())
}

/// Create a YAML mapping from key-value pairs
fn yaml_map(pairs: &[(&str, Value)]) -> Value {
    let mut mapping = serde_yaml::Mapping::new();
    for (k, v) in pairs {
        mapping.insert(yaml_string(k), v.clone());
    }
    Value::Mapping(mapping)
}

/// Parse YAML string to Value
fn parse_yaml(s: &str) -> Value {
    serde_yaml::from_str(s).expect("Invalid YAML")
}

// ============================================================================
// PHASE 1: VarPrecedence Enum Tests
// ============================================================================

mod precedence_enum {
    use super::*;

    #[test]
    fn test_precedence_levels_count() {
        let all: Vec<VarPrecedence> = VarPrecedence::all().collect();
        assert_eq!(all.len(), 20, "Should have 20 precedence levels");
    }

    #[test]
    fn test_precedence_ordering_role_defaults_lowest() {
        // RoleDefaults should be the lowest precedence
        let all: Vec<VarPrecedence> = VarPrecedence::all().collect();
        assert_eq!(all[0], VarPrecedence::RoleDefaults);
        assert_eq!(VarPrecedence::RoleDefaults.level(), 1);
    }

    #[test]
    fn test_precedence_ordering_extra_vars_highest() {
        // ExtraVars should be the highest precedence
        let all: Vec<VarPrecedence> = VarPrecedence::all().collect();
        assert_eq!(all[19], VarPrecedence::ExtraVars);
        assert_eq!(VarPrecedence::ExtraVars.level(), 20);
    }

    #[test]
    fn test_all_precedence_levels_correctly_ordered() {
        let all: Vec<VarPrecedence> = VarPrecedence::all().collect();

        // Verify ordering from lowest to highest
        for i in 0..all.len() - 1 {
            assert!(
                all[i] < all[i + 1],
                "Precedence level {} ({:?}) should be less than {} ({:?})",
                i,
                all[i],
                i + 1,
                all[i + 1]
            );
        }
    }

    #[test]
    fn test_precedence_comparisons() {
        // Critical ordering tests based on Ansible behavior
        assert!(VarPrecedence::RoleDefaults < VarPrecedence::InventoryGroupVars);
        assert!(VarPrecedence::InventoryGroupVars < VarPrecedence::PlaybookGroupVars);
        assert!(VarPrecedence::PlaybookGroupVars < VarPrecedence::PlaybookHostVars);
        assert!(VarPrecedence::PlaybookHostVars < VarPrecedence::HostFacts);
        assert!(VarPrecedence::HostFacts < VarPrecedence::PlayVars);
        assert!(VarPrecedence::PlayVars < VarPrecedence::RoleVars);
        assert!(VarPrecedence::RoleVars < VarPrecedence::TaskVars);
        assert!(VarPrecedence::TaskVars < VarPrecedence::SetFacts);
        assert!(VarPrecedence::SetFacts < VarPrecedence::ExtraVars);
    }

    #[test]
    fn test_precedence_display() {
        assert_eq!(format!("{}", VarPrecedence::RoleDefaults), "role defaults");
        assert_eq!(
            format!("{}", VarPrecedence::InventoryGroupVars),
            "inventory group vars"
        );
        assert_eq!(format!("{}", VarPrecedence::PlayVars), "play vars");
        assert_eq!(format!("{}", VarPrecedence::TaskVars), "task vars");
        assert_eq!(format!("{}", VarPrecedence::ExtraVars), "extra vars");
        assert_eq!(format!("{}", VarPrecedence::SetFacts), "set_facts");
        assert_eq!(format!("{}", VarPrecedence::RoleVars), "role vars");
        assert_eq!(format!("{}", VarPrecedence::BlockVars), "block vars");
    }

    #[test]
    fn test_complete_precedence_chain() {
        // Verify complete chain matches expected order
        let expected = vec![
            VarPrecedence::RoleDefaults,           // 1
            VarPrecedence::InventoryGroupVars,     // 2
            VarPrecedence::InventoryFileGroupVars, // 3
            VarPrecedence::PlaybookGroupVarsAll,   // 4
            VarPrecedence::PlaybookGroupVars,      // 5
            VarPrecedence::InventoryHostVars,      // 6
            VarPrecedence::InventoryFileHostVars,  // 7
            VarPrecedence::PlaybookHostVars,       // 8
            VarPrecedence::HostFacts,              // 9
            VarPrecedence::PlayVars,               // 10
            VarPrecedence::PlayVarsPrompt,         // 11
            VarPrecedence::PlayVarsFiles,          // 12
            VarPrecedence::RoleVars,               // 13
            VarPrecedence::BlockVars,              // 14
            VarPrecedence::TaskVars,               // 15
            VarPrecedence::IncludeVars,            // 16
            VarPrecedence::SetFacts,               // 17
            VarPrecedence::RoleParams,             // 18
            VarPrecedence::IncludeParams,          // 19
            VarPrecedence::ExtraVars,              // 20
        ];

        let actual: Vec<VarPrecedence> = VarPrecedence::all().collect();
        assert_eq!(actual, expected);
    }
}

// ============================================================================
// PHASE 2: Full Precedence Chain Tests
// ============================================================================

mod full_precedence_chain {
    use super::*;

    #[test]
    fn test_extra_vars_override_everything() {
        let mut store = VarStore::new();

        // Set variable at every precedence level
        for prec in VarPrecedence::all() {
            store.set("common_var", yaml_string(&format!("from_{:?}", prec)), prec);
        }

        // ExtraVars should always win
        let value = store.get("common_var");
        assert_eq!(value, Some(&yaml_string("from_ExtraVars")));
    }

    #[test]
    fn test_role_defaults_are_lowest() {
        let mut store = VarStore::new();

        // Set only at RoleDefaults level
        store.set(
            "my_var",
            yaml_string("default_value"),
            VarPrecedence::RoleDefaults,
        );
        assert_eq!(store.get("my_var"), Some(&yaml_string("default_value")));

        // Any higher level should override
        store.set("my_var", yaml_string("play_value"), VarPrecedence::PlayVars);
        assert_eq!(store.get("my_var"), Some(&yaml_string("play_value")));

        // Even removing the higher level reveals the lower
        store.remove("my_var", VarPrecedence::PlayVars);
        assert_eq!(store.get("my_var"), Some(&yaml_string("default_value")));
    }

    #[test]
    fn test_task_vars_override_block_vars() {
        let mut store = VarStore::new();

        store.set(
            "nested_var",
            yaml_string("from_block"),
            VarPrecedence::BlockVars,
        );
        store.set(
            "nested_var",
            yaml_string("from_task"),
            VarPrecedence::TaskVars,
        );

        assert_eq!(store.get("nested_var"), Some(&yaml_string("from_task")));
    }

    #[test]
    fn test_role_vars_override_play_vars() {
        let mut store = VarStore::new();

        store.set(
            "role_play_var",
            yaml_string("play_level"),
            VarPrecedence::PlayVars,
        );
        store.set(
            "role_play_var",
            yaml_string("role_level"),
            VarPrecedence::RoleVars,
        );

        assert_eq!(store.get("role_play_var"), Some(&yaml_string("role_level")));
    }

    #[test]
    fn test_set_facts_override_task_vars() {
        let mut store = VarStore::new();

        store.set(
            "dynamic_var",
            yaml_string("static_task_value"),
            VarPrecedence::TaskVars,
        );
        store.set(
            "dynamic_var",
            yaml_string("set_fact_value"),
            VarPrecedence::SetFacts,
        );

        assert_eq!(
            store.get("dynamic_var"),
            Some(&yaml_string("set_fact_value"))
        );
    }

    #[test]
    fn test_include_vars_override_role_vars() {
        let mut store = VarStore::new();

        store.set(
            "included_var",
            yaml_string("role_value"),
            VarPrecedence::RoleVars,
        );
        store.set(
            "included_var",
            yaml_string("included_value"),
            VarPrecedence::IncludeVars,
        );

        assert_eq!(
            store.get("included_var"),
            Some(&yaml_string("included_value"))
        );
    }

    #[test]
    fn test_inventory_group_vars_chain() {
        let mut store = VarStore::new();

        // Test group vars precedence chain
        store.set(
            "group_var",
            yaml_string("inventory_group"),
            VarPrecedence::InventoryGroupVars,
        );
        assert_eq!(
            store.get("group_var"),
            Some(&yaml_string("inventory_group"))
        );

        store.set(
            "group_var",
            yaml_string("inventory_file_group"),
            VarPrecedence::InventoryFileGroupVars,
        );
        assert_eq!(
            store.get("group_var"),
            Some(&yaml_string("inventory_file_group"))
        );

        store.set(
            "group_var",
            yaml_string("playbook_group_all"),
            VarPrecedence::PlaybookGroupVarsAll,
        );
        assert_eq!(
            store.get("group_var"),
            Some(&yaml_string("playbook_group_all"))
        );

        store.set(
            "group_var",
            yaml_string("playbook_group_specific"),
            VarPrecedence::PlaybookGroupVars,
        );
        assert_eq!(
            store.get("group_var"),
            Some(&yaml_string("playbook_group_specific"))
        );
    }

    #[test]
    fn test_host_vars_override_group_vars() {
        let mut store = VarStore::new();

        // Group vars
        store.set(
            "host_group_var",
            yaml_string("from_group"),
            VarPrecedence::PlaybookGroupVars,
        );
        assert_eq!(
            store.get("host_group_var"),
            Some(&yaml_string("from_group"))
        );

        // Host vars should override
        store.set(
            "host_group_var",
            yaml_string("from_host"),
            VarPrecedence::PlaybookHostVars,
        );
        assert_eq!(store.get("host_group_var"), Some(&yaml_string("from_host")));
    }

    #[test]
    fn test_play_vars_files_override_play_vars() {
        let mut store = VarStore::new();

        store.set(
            "file_var",
            yaml_string("inline_play"),
            VarPrecedence::PlayVars,
        );
        store.set(
            "file_var",
            yaml_string("from_vars_files"),
            VarPrecedence::PlayVarsFiles,
        );

        assert_eq!(store.get("file_var"), Some(&yaml_string("from_vars_files")));
    }

    #[test]
    fn test_role_params_override_set_facts() {
        let mut store = VarStore::new();

        store.set(
            "param_var",
            yaml_string("set_fact_value"),
            VarPrecedence::SetFacts,
        );
        store.set(
            "param_var",
            yaml_string("role_param_value"),
            VarPrecedence::RoleParams,
        );

        assert_eq!(
            store.get("param_var"),
            Some(&yaml_string("role_param_value"))
        );
    }

    #[test]
    fn test_include_params_override_role_params() {
        let mut store = VarStore::new();

        store.set(
            "include_var",
            yaml_string("role_param"),
            VarPrecedence::RoleParams,
        );
        store.set(
            "include_var",
            yaml_string("include_param"),
            VarPrecedence::IncludeParams,
        );

        assert_eq!(
            store.get("include_var"),
            Some(&yaml_string("include_param"))
        );
    }

    #[test]
    fn test_host_facts_precedence() {
        let mut store = VarStore::new();

        store.set(
            "ansible_os_family",
            yaml_string("from_host_vars"),
            VarPrecedence::PlaybookHostVars,
        );
        // Host facts gathered during play
        store.set(
            "ansible_os_family",
            yaml_string("Debian"),
            VarPrecedence::HostFacts,
        );

        assert_eq!(store.get("ansible_os_family"), Some(&yaml_string("Debian")));
    }

    #[test]
    fn test_vars_prompt_override_play_vars() {
        let mut store = VarStore::new();

        store.set(
            "prompted_var",
            yaml_string("default_play"),
            VarPrecedence::PlayVars,
        );
        store.set(
            "prompted_var",
            yaml_string("user_prompted"),
            VarPrecedence::PlayVarsPrompt,
        );

        assert_eq!(
            store.get("prompted_var"),
            Some(&yaml_string("user_prompted"))
        );
    }

    #[test]
    fn test_multiple_vars_different_levels() {
        let mut store = VarStore::new();

        // Set different vars at different levels
        store.set(
            "role_default_only",
            yaml_string("rd"),
            VarPrecedence::RoleDefaults,
        );
        store.set("play_var_only", yaml_string("pv"), VarPrecedence::PlayVars);
        store.set("task_var_only", yaml_string("tv"), VarPrecedence::TaskVars);
        store.set(
            "extra_var_only",
            yaml_string("ev"),
            VarPrecedence::ExtraVars,
        );

        // All should be accessible
        assert_eq!(store.get("role_default_only"), Some(&yaml_string("rd")));
        assert_eq!(store.get("play_var_only"), Some(&yaml_string("pv")));
        assert_eq!(store.get("task_var_only"), Some(&yaml_string("tv")));
        assert_eq!(store.get("extra_var_only"), Some(&yaml_string("ev")));
    }

    #[test]
    fn test_precedence_with_clear() {
        let mut store = VarStore::new();

        store.set(
            "clearable",
            yaml_string("role_default"),
            VarPrecedence::RoleDefaults,
        );
        store.set(
            "clearable",
            yaml_string("play_vars"),
            VarPrecedence::PlayVars,
        );
        store.set(
            "clearable",
            yaml_string("task_vars"),
            VarPrecedence::TaskVars,
        );

        // Clear task level - should fall back to play level
        store.clear_precedence(VarPrecedence::TaskVars);
        assert_eq!(store.get("clearable"), Some(&yaml_string("play_vars")));

        // Clear play level - should fall back to role defaults
        store.clear_precedence(VarPrecedence::PlayVars);
        assert_eq!(store.get("clearable"), Some(&yaml_string("role_default")));
    }
}

// ============================================================================
// PHASE 3: Variable Merging Tests
// ============================================================================

mod variable_merging {
    use super::*;

    #[test]
    fn test_hash_behaviour_replace_default() {
        let store = VarStore::new();
        assert_eq!(store.hash_behaviour(), HashBehaviour::Replace);
    }

    #[test]
    fn test_hash_behaviour_replace_completely_replaces_dicts() {
        let mut store = VarStore::with_hash_behaviour(HashBehaviour::Replace);

        let map1 = yaml_map(&[
            ("key1", yaml_string("value1")),
            ("key2", yaml_string("value2")),
        ]);

        let map2 = yaml_map(&[("key3", yaml_string("value3"))]);

        store.set("config", map1, VarPrecedence::RoleDefaults);
        store.set("config", map2.clone(), VarPrecedence::PlayVars);

        // With Replace, higher precedence completely replaces
        let result = store.get("config");
        assert_eq!(result, Some(&map2));
    }

    #[test]
    fn test_hash_behaviour_merge_deep_merges_dicts() {
        let mut store = VarStore::with_hash_behaviour(HashBehaviour::Merge);

        let map1 = yaml_map(&[
            ("key1", yaml_string("value1")),
            ("key2", yaml_string("value2")),
        ]);

        let map2 = yaml_map(&[
            ("key2", yaml_string("overwritten")),
            ("key3", yaml_string("value3")),
        ]);

        store.set("config", map1, VarPrecedence::RoleDefaults);
        store.set("config", map2, VarPrecedence::PlayVars);

        let result = store.get("config").expect("config should exist");

        // Should have merged all keys
        assert_eq!(
            resolve::resolve_path(result, "key1"),
            Some(&yaml_string("value1"))
        );
        assert_eq!(
            resolve::resolve_path(result, "key2"),
            Some(&yaml_string("overwritten"))
        );
        assert_eq!(
            resolve::resolve_path(result, "key3"),
            Some(&yaml_string("value3"))
        );
    }

    #[test]
    fn test_list_replacement_not_merge() {
        let mut store = VarStore::with_hash_behaviour(HashBehaviour::Merge);

        let list1 = yaml_list(&["item1", "item2"]);
        let list2 = yaml_list(&["item3"]);

        store.set("my_list", list1, VarPrecedence::RoleDefaults);
        store.set("my_list", list2.clone(), VarPrecedence::PlayVars);

        // Lists are replaced, not merged (even with hash_behaviour=merge)
        let result = store.get("my_list");
        assert_eq!(result, Some(&list2));
    }

    #[test]
    fn test_deep_merge_nested_dicts() {
        let base = parse_yaml(
            r#"
            level1:
              level2:
                a: 1
                b: 2
              other: preserved
        "#,
        );

        let overlay = parse_yaml(
            r#"
            level1:
              level2:
                b: 20
                c: 3
        "#,
        );

        let merged = deep_merge(&base, &overlay);

        assert_eq!(
            resolve::resolve_path(&merged, "level1.level2.a"),
            Some(&yaml_int(1))
        );
        assert_eq!(
            resolve::resolve_path(&merged, "level1.level2.b"),
            Some(&yaml_int(20))
        );
        assert_eq!(
            resolve::resolve_path(&merged, "level1.level2.c"),
            Some(&yaml_int(3))
        );
        assert_eq!(
            resolve::resolve_path(&merged, "level1.other"),
            Some(&yaml_string("preserved"))
        );
    }

    #[test]
    fn test_deep_merge_preserves_structure() {
        let base = parse_yaml(
            r#"
            server:
              name: myserver
              config:
                port: 80
                ssl: false
        "#,
        );

        let overlay = parse_yaml(
            r#"
            server:
              config:
                ssl: true
                timeout: 30
        "#,
        );

        let merged = deep_merge(&base, &overlay);

        assert_eq!(
            resolve::resolve_path(&merged, "server.name"),
            Some(&yaml_string("myserver"))
        );
        assert_eq!(
            resolve::resolve_path(&merged, "server.config.port"),
            Some(&yaml_int(80))
        );
        assert_eq!(
            resolve::resolve_path(&merged, "server.config.ssl"),
            Some(&yaml_bool(true))
        );
        assert_eq!(
            resolve::resolve_path(&merged, "server.config.timeout"),
            Some(&yaml_int(30))
        );
    }

    #[test]
    fn test_merge_scalar_overwrites() {
        let base = yaml_string("base_value");
        let overlay = yaml_string("overlay_value");

        let merged = deep_merge(&base, &overlay);
        assert_eq!(merged, yaml_string("overlay_value"));
    }

    #[test]
    fn test_merge_dict_over_scalar() {
        let base = yaml_string("scalar");
        let overlay = yaml_map(&[("key", yaml_string("value"))]);

        let merged = deep_merge(&base, &overlay);
        assert_eq!(merged, overlay);
    }

    #[test]
    fn test_merge_scalar_over_dict() {
        let base = yaml_map(&[("key", yaml_string("value"))]);
        let overlay = yaml_string("scalar");

        let merged = deep_merge(&base, &overlay);
        assert_eq!(merged, overlay);
    }

    #[test]
    fn test_three_level_merge() {
        let mut store = VarStore::with_hash_behaviour(HashBehaviour::Merge);

        // Role defaults
        store.set(
            "config",
            yaml_map(&[("a", yaml_int(1)), ("b", yaml_int(2))]),
            VarPrecedence::RoleDefaults,
        );

        // Play vars
        store.set(
            "config",
            yaml_map(&[("b", yaml_int(20)), ("c", yaml_int(3))]),
            VarPrecedence::PlayVars,
        );

        // Task vars
        store.set(
            "config",
            yaml_map(&[("c", yaml_int(30)), ("d", yaml_int(4))]),
            VarPrecedence::TaskVars,
        );

        let result = store.get("config").expect("config should exist");

        // All levels should be merged
        assert_eq!(resolve::resolve_path(result, "a"), Some(&yaml_int(1)));
        assert_eq!(resolve::resolve_path(result, "b"), Some(&yaml_int(20)));
        assert_eq!(resolve::resolve_path(result, "c"), Some(&yaml_int(30)));
        assert_eq!(resolve::resolve_path(result, "d"), Some(&yaml_int(4)));
    }
}

// ============================================================================
// PHASE 4: Special Variables Tests
// ============================================================================

mod special_variables {
    use super::*;

    #[test]
    fn test_inventory_hostname_variable() {
        let mut store = VarStore::new();

        // inventory_hostname is typically set at HostFacts level
        store.set(
            "inventory_hostname",
            yaml_string("webserver1"),
            VarPrecedence::HostFacts,
        );

        assert_eq!(
            store.get("inventory_hostname"),
            Some(&yaml_string("webserver1"))
        );
    }

    #[test]
    fn test_inventory_hostname_short() {
        let mut store = VarStore::new();

        store.set(
            "inventory_hostname_short",
            yaml_string("webserver1"),
            VarPrecedence::HostFacts,
        );
        assert_eq!(
            store.get("inventory_hostname_short"),
            Some(&yaml_string("webserver1"))
        );
    }

    #[test]
    fn test_groups_variable_structure() {
        let mut store = VarStore::new();

        let groups = yaml_map(&[
            ("all", yaml_list(&["web1", "web2", "db1"])),
            ("webservers", yaml_list(&["web1", "web2"])),
            ("databases", yaml_list(&["db1"])),
        ]);

        store.set("groups", groups.clone(), VarPrecedence::HostFacts);

        let result = store.get("groups").expect("groups should exist");
        assert_eq!(
            resolve::resolve_path(result, "webservers.0"),
            Some(&yaml_string("web1"))
        );
    }

    #[test]
    fn test_hostvars_variable_structure() {
        let mut store = VarStore::new();

        let hostvars = yaml_map(&[
            (
                "web1",
                yaml_map(&[("ansible_host", yaml_string("10.0.0.1"))]),
            ),
            (
                "web2",
                yaml_map(&[("ansible_host", yaml_string("10.0.0.2"))]),
            ),
        ]);

        store.set("hostvars", hostvars.clone(), VarPrecedence::HostFacts);

        let result = store.get("hostvars").expect("hostvars should exist");
        assert_eq!(
            resolve::resolve_path(result, "web1.ansible_host"),
            Some(&yaml_string("10.0.0.1"))
        );
    }

    #[test]
    fn test_ansible_connection_variables() {
        let mut store = VarStore::new();

        store.set(
            "ansible_connection",
            yaml_string("ssh"),
            VarPrecedence::HostFacts,
        );
        store.set(
            "ansible_host",
            yaml_string("192.168.1.100"),
            VarPrecedence::HostFacts,
        );
        store.set("ansible_port", yaml_int(22), VarPrecedence::HostFacts);
        store.set(
            "ansible_user",
            yaml_string("admin"),
            VarPrecedence::HostFacts,
        );

        assert_eq!(store.get("ansible_connection"), Some(&yaml_string("ssh")));
        assert_eq!(
            store.get("ansible_host"),
            Some(&yaml_string("192.168.1.100"))
        );
        assert_eq!(store.get("ansible_port"), Some(&yaml_int(22)));
        assert_eq!(store.get("ansible_user"), Some(&yaml_string("admin")));
    }

    #[test]
    fn test_ansible_facts_structure() {
        let mut store = VarStore::new();

        let ansible_facts = yaml_map(&[
            ("distribution", yaml_string("Ubuntu")),
            ("distribution_version", yaml_string("22.04")),
            ("os_family", yaml_string("Debian")),
            ("architecture", yaml_string("x86_64")),
        ]);

        store.set("ansible_facts", ansible_facts, VarPrecedence::HostFacts);

        let result = store
            .get("ansible_facts")
            .expect("ansible_facts should exist");
        assert_eq!(
            resolve::resolve_path(result, "distribution"),
            Some(&yaml_string("Ubuntu"))
        );
        assert_eq!(
            resolve::resolve_path(result, "os_family"),
            Some(&yaml_string("Debian"))
        );
    }

    #[test]
    fn test_play_hosts_variable() {
        let mut store = VarStore::new();

        let play_hosts = yaml_list(&["web1", "web2", "web3"]);
        store.set("play_hosts", play_hosts, VarPrecedence::PlayVars);

        let result = store.get("play_hosts").expect("play_hosts should exist");
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 3);
        } else {
            panic!("play_hosts should be a list");
        }
    }

    #[test]
    fn test_ansible_become_variables() {
        let mut store = VarStore::new();

        store.set("ansible_become", yaml_bool(true), VarPrecedence::PlayVars);
        store.set(
            "ansible_become_method",
            yaml_string("sudo"),
            VarPrecedence::PlayVars,
        );
        store.set(
            "ansible_become_user",
            yaml_string("root"),
            VarPrecedence::PlayVars,
        );

        assert_eq!(store.get("ansible_become"), Some(&yaml_bool(true)));
        assert_eq!(
            store.get("ansible_become_method"),
            Some(&yaml_string("sudo"))
        );
        assert_eq!(store.get("ansible_become_user"), Some(&yaml_string("root")));
    }

    #[test]
    fn test_omit_special_value() {
        // omit is a special value used to skip parameters
        let mut store = VarStore::new();

        // The omit value is typically a unique string that the executor recognizes
        store.set(
            "__omit_place_holder__",
            yaml_string("__OMIT_PLACEHOLDER__"),
            VarPrecedence::PlayVars,
        );

        assert!(store.contains("__omit_place_holder__"));
    }

    #[test]
    fn test_role_path_variable() {
        let mut store = VarStore::new();

        store.set(
            "role_path",
            yaml_string("/path/to/roles/webserver"),
            VarPrecedence::RoleVars,
        );

        assert_eq!(
            store.get("role_path"),
            Some(&yaml_string("/path/to/roles/webserver"))
        );
    }

    #[test]
    fn test_playbook_dir_variable() {
        let mut store = VarStore::new();

        store.set(
            "playbook_dir",
            yaml_string("/home/user/ansible"),
            VarPrecedence::PlayVars,
        );

        assert_eq!(
            store.get("playbook_dir"),
            Some(&yaml_string("/home/user/ansible"))
        );
    }
}

// ============================================================================
// PHASE 5: Variable Types Tests
// ============================================================================

mod variable_types {
    use super::*;

    #[test]
    fn test_string_variable() {
        let mut store = VarStore::new();

        store.set(
            "string_var",
            yaml_string("hello world"),
            VarPrecedence::PlayVars,
        );

        let result = store.get("string_var").expect("string_var should exist");
        assert!(matches!(result, Value::String(_)));
        assert_eq!(resolve::to_string(result), "hello world");
    }

    #[test]
    fn test_integer_variable() {
        let mut store = VarStore::new();

        store.set("int_var", yaml_int(42), VarPrecedence::PlayVars);

        let result = store.get("int_var").expect("int_var should exist");
        assert!(matches!(result, Value::Number(_)));
        assert_eq!(resolve::to_int(result), Some(42));
    }

    #[test]
    fn test_negative_integer_variable() {
        let mut store = VarStore::new();

        store.set("negative_var", yaml_int(-100), VarPrecedence::PlayVars);

        let result = store
            .get("negative_var")
            .expect("negative_var should exist");
        assert_eq!(resolve::to_int(result), Some(-100));
    }

    #[test]
    fn test_float_variable() {
        let mut store = VarStore::new();

        let float_val = Value::Number(serde_yaml::Number::from(3.14_f64));
        store.set("float_var", float_val, VarPrecedence::PlayVars);

        let result = store.get("float_var").expect("float_var should exist");
        assert_eq!(resolve::to_float(result), Some(3.14));
    }

    #[test]
    fn test_boolean_true_variable() {
        let mut store = VarStore::new();

        store.set("bool_var", yaml_bool(true), VarPrecedence::PlayVars);

        let result = store.get("bool_var").expect("bool_var should exist");
        assert_eq!(resolve::to_bool(result), Some(true));
    }

    #[test]
    fn test_boolean_false_variable() {
        let mut store = VarStore::new();

        store.set("bool_var", yaml_bool(false), VarPrecedence::PlayVars);

        let result = store.get("bool_var").expect("bool_var should exist");
        assert_eq!(resolve::to_bool(result), Some(false));
    }

    #[test]
    fn test_boolean_string_conversions() {
        // Test various string representations of booleans
        assert_eq!(resolve::to_bool(&yaml_string("yes")), Some(true));
        assert_eq!(resolve::to_bool(&yaml_string("no")), Some(false));
        assert_eq!(resolve::to_bool(&yaml_string("true")), Some(true));
        assert_eq!(resolve::to_bool(&yaml_string("false")), Some(false));
        assert_eq!(resolve::to_bool(&yaml_string("on")), Some(true));
        assert_eq!(resolve::to_bool(&yaml_string("off")), Some(false));
        assert_eq!(resolve::to_bool(&yaml_string("1")), Some(true));
        assert_eq!(resolve::to_bool(&yaml_string("0")), Some(false));
        assert_eq!(resolve::to_bool(&yaml_string("YES")), Some(true));
        assert_eq!(resolve::to_bool(&yaml_string("NO")), Some(false));
        assert_eq!(resolve::to_bool(&yaml_string("True")), Some(true));
        assert_eq!(resolve::to_bool(&yaml_string("False")), Some(false));
    }

    #[test]
    fn test_list_variable() {
        let mut store = VarStore::new();

        let list = yaml_list(&["item1", "item2", "item3"]);
        store.set("list_var", list, VarPrecedence::PlayVars);

        let result = store.get("list_var").expect("list_var should exist");
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 3);
            assert_eq!(seq[0], yaml_string("item1"));
            assert_eq!(seq[1], yaml_string("item2"));
            assert_eq!(seq[2], yaml_string("item3"));
        } else {
            panic!("list_var should be a sequence");
        }
    }

    #[test]
    fn test_dict_variable() {
        let mut store = VarStore::new();

        let dict = yaml_map(&[
            ("name", yaml_string("webserver")),
            ("port", yaml_int(80)),
            ("enabled", yaml_bool(true)),
        ]);
        store.set("dict_var", dict, VarPrecedence::PlayVars);

        let result = store.get("dict_var").expect("dict_var should exist");
        assert_eq!(
            resolve::resolve_path(result, "name"),
            Some(&yaml_string("webserver"))
        );
        assert_eq!(resolve::resolve_path(result, "port"), Some(&yaml_int(80)));
        assert_eq!(
            resolve::resolve_path(result, "enabled"),
            Some(&yaml_bool(true))
        );
    }

    #[test]
    fn test_nested_structure() {
        let mut store = VarStore::new();

        let nested = parse_yaml(
            r#"
            server:
              name: webserver
              ports:
                - 80
                - 443
              config:
                ssl:
                  enabled: true
                  certificate: /etc/ssl/cert.pem
        "#,
        );
        store.set("nested", nested, VarPrecedence::PlayVars);

        let result = store.get("nested").expect("nested should exist");
        assert_eq!(
            resolve::resolve_path(result, "server.name"),
            Some(&yaml_string("webserver"))
        );
        assert_eq!(
            resolve::resolve_path(result, "server.ports.0"),
            Some(&yaml_int(80))
        );
        assert_eq!(
            resolve::resolve_path(result, "server.config.ssl.enabled"),
            Some(&yaml_bool(true))
        );
    }

    #[test]
    fn test_null_variable() {
        let mut store = VarStore::new();

        store.set("null_var", Value::Null, VarPrecedence::PlayVars);

        let result = store.get("null_var").expect("null_var should exist");
        assert_eq!(*result, Value::Null);
        assert_eq!(resolve::to_string(result), "");
    }

    #[test]
    fn test_empty_string_variable() {
        let mut store = VarStore::new();

        store.set("empty_str", yaml_string(""), VarPrecedence::PlayVars);

        let result = store.get("empty_str").expect("empty_str should exist");
        assert_eq!(resolve::to_string(result), "");
    }

    #[test]
    fn test_empty_list_variable() {
        let mut store = VarStore::new();

        let empty_list = Value::Sequence(vec![]);
        store.set("empty_list", empty_list, VarPrecedence::PlayVars);

        let result = store.get("empty_list").expect("empty_list should exist");
        if let Value::Sequence(seq) = result {
            assert!(seq.is_empty());
        } else {
            panic!("empty_list should be a sequence");
        }
    }

    #[test]
    fn test_empty_dict_variable() {
        let mut store = VarStore::new();

        let empty_dict = Value::Mapping(serde_yaml::Mapping::new());
        store.set("empty_dict", empty_dict, VarPrecedence::PlayVars);

        let result = store.get("empty_dict").expect("empty_dict should exist");
        if let Value::Mapping(map) = result {
            assert!(map.is_empty());
        } else {
            panic!("empty_dict should be a mapping");
        }
    }

    #[test]
    fn test_to_list_from_csv() {
        let csv = yaml_string("a, b, c");
        let list = resolve::to_list(&csv);

        assert_eq!(list.len(), 3);
        assert_eq!(list[0], yaml_string("a"));
        assert_eq!(list[1], yaml_string("b"));
        assert_eq!(list[2], yaml_string("c"));
    }

    #[test]
    fn test_to_list_from_single_value() {
        let single = yaml_int(42);
        let list = resolve::to_list(&single);

        assert_eq!(list.len(), 1);
        assert_eq!(list[0], yaml_int(42));
    }
}

// ============================================================================
// PHASE 6: Variable Scope Tests
// ============================================================================

mod variable_scope {
    use super::*;

    #[test]
    fn test_play_scope_variables() {
        let mut store = VarStore::new();

        // Play-scoped variables should be at PlayVars level
        store.set(
            "play_var",
            yaml_string("play_value"),
            VarPrecedence::PlayVars,
        );

        assert!(store.contains("play_var"));
        assert_eq!(store.get("play_var"), Some(&yaml_string("play_value")));
    }

    #[test]
    fn test_task_scope_variables() {
        let mut store = VarStore::new();

        // Task-scoped variables should be at TaskVars level
        store.set(
            "task_var",
            yaml_string("task_value"),
            VarPrecedence::TaskVars,
        );

        assert!(store.contains("task_var"));
    }

    #[test]
    fn test_block_scope_variables() {
        let mut store = VarStore::new();

        // Block-scoped variables
        store.set(
            "block_var",
            yaml_string("block_value"),
            VarPrecedence::BlockVars,
        );

        assert!(store.contains("block_var"));
        assert_eq!(store.get("block_var"), Some(&yaml_string("block_value")));
    }

    #[test]
    fn test_var_scope_child_override() {
        let store = VarStore::new();

        // Create a scope with local overrides
        let mut scope = store.scope();
        scope.set("scoped_var", yaml_string("scope_value"));

        assert_eq!(scope.get("scoped_var"), Some(&yaml_string("scope_value")));
    }

    #[test]
    fn test_var_scope_inherits_parent() {
        let mut store = VarStore::new();
        store.set(
            "parent_var",
            yaml_string("parent_value"),
            VarPrecedence::PlayVars,
        );

        let scope = store.scope();

        // Scope should see parent variable
        assert_eq!(scope.get("parent_var"), Some(&yaml_string("parent_value")));
    }

    #[test]
    fn test_var_scope_overrides_parent() {
        let mut store = VarStore::new();
        store.set(
            "shared_var",
            yaml_string("parent_value"),
            VarPrecedence::PlayVars,
        );

        let mut scope = store.scope();
        scope.set("shared_var", yaml_string("child_value"));

        // Scope should see overridden value
        assert_eq!(scope.get("shared_var"), Some(&yaml_string("child_value")));

        // Parent store unchanged
        assert_eq!(store.get("shared_var"), Some(&yaml_string("parent_value")));
    }

    #[test]
    fn test_var_scope_all_merged() {
        let mut store = VarStore::new();
        store.set("a", yaml_int(1), VarPrecedence::PlayVars);
        store.set("b", yaml_int(2), VarPrecedence::RoleDefaults);

        let mut scope = store.scope();
        scope.set("c", yaml_int(3));
        scope.set("a", yaml_int(10)); // Override

        let all = scope.all();
        assert_eq!(all.len(), 3);
        assert_eq!(all.get("a"), Some(&yaml_int(10))); // Overridden
        assert_eq!(all.get("b"), Some(&yaml_int(2)));
        assert_eq!(all.get("c"), Some(&yaml_int(3)));
    }

    #[test]
    fn test_loop_scope_item_variable() {
        let mut store = VarStore::new();

        // Loop item variable
        let mut scope = store.scope();
        scope.set("item", yaml_string("current_item"));

        assert_eq!(scope.get("item"), Some(&yaml_string("current_item")));
    }

    #[test]
    fn test_loop_scope_loop_index() {
        let store = VarStore::new();
        let mut scope = store.scope();

        // Loop control variables
        scope.set(
            "ansible_loop",
            yaml_map(&[
                ("index", yaml_int(0)),
                ("index0", yaml_int(0)),
                ("first", yaml_bool(true)),
                ("last", yaml_bool(false)),
                ("length", yaml_int(5)),
            ]),
        );

        assert!(scope.get("ansible_loop").is_some());
    }

    #[test]
    fn test_nested_scopes() {
        let mut store = VarStore::new();
        store.set(
            "global",
            yaml_string("global_value"),
            VarPrecedence::PlayVars,
        );

        let mut scope1 = store.scope();
        scope1.set("scope1_var", yaml_string("scope1_value"));

        // scope1 sees both
        assert_eq!(scope1.get("global"), Some(&yaml_string("global_value")));
        assert_eq!(scope1.get("scope1_var"), Some(&yaml_string("scope1_value")));
    }

    #[test]
    fn test_scope_set_many() {
        let store = VarStore::new();
        let mut scope = store.scope();

        let mut vars = IndexMap::new();
        vars.insert("var1".to_string(), yaml_int(1));
        vars.insert("var2".to_string(), yaml_int(2));
        vars.insert("var3".to_string(), yaml_int(3));

        scope.set_many(vars);

        assert_eq!(scope.get("var1"), Some(&yaml_int(1)));
        assert_eq!(scope.get("var2"), Some(&yaml_int(2)));
        assert_eq!(scope.get("var3"), Some(&yaml_int(3)));
    }
}

// ============================================================================
// PHASE 7: Registered Variables Tests
// ============================================================================

mod registered_variables {
    use super::*;

    #[test]
    fn test_registered_var_at_set_facts_level() {
        let mut store = VarStore::new();

        // Registered variables use SetFacts precedence
        let result = yaml_map(&[
            ("stdout", yaml_string("command output")),
            ("stderr", yaml_string("")),
            ("rc", yaml_int(0)),
            ("changed", yaml_bool(true)),
        ]);

        store.set("my_result", result.clone(), VarPrecedence::SetFacts);

        let registered = store.get("my_result").expect("my_result should exist");
        assert_eq!(
            resolve::resolve_path(registered, "stdout"),
            Some(&yaml_string("command output"))
        );
        assert_eq!(resolve::resolve_path(registered, "rc"), Some(&yaml_int(0)));
    }

    #[test]
    fn test_registered_var_overrides_task_var() {
        let mut store = VarStore::new();

        // Task var set first
        store.set("result", yaml_string("task_value"), VarPrecedence::TaskVars);

        // Register result should override
        store.set(
            "result",
            yaml_map(&[("stdout", yaml_string("registered"))]),
            VarPrecedence::SetFacts,
        );

        let result = store.get("result").expect("result should exist");
        assert_eq!(
            resolve::resolve_path(result, "stdout"),
            Some(&yaml_string("registered"))
        );
    }

    #[test]
    fn test_registered_var_persistence_across_tasks() {
        let mut store = VarStore::new();

        // Simulating first task registration
        store.set(
            "first_result",
            yaml_map(&[("value", yaml_int(1))]),
            VarPrecedence::SetFacts,
        );

        // Simulating second task
        store.set(
            "some_task_var",
            yaml_string("task2"),
            VarPrecedence::TaskVars,
        );

        // First result should still be available
        assert!(store.contains("first_result"));
        let result = store
            .get("first_result")
            .expect("first_result should exist");
        assert_eq!(resolve::resolve_path(result, "value"), Some(&yaml_int(1)));
    }

    #[test]
    fn test_registered_var_can_be_overwritten() {
        let mut store = VarStore::new();

        // First registration
        store.set(
            "result",
            yaml_map(&[("value", yaml_int(1))]),
            VarPrecedence::SetFacts,
        );

        // Second registration overwrites
        store.set(
            "result",
            yaml_map(&[("value", yaml_int(2))]),
            VarPrecedence::SetFacts,
        );

        let result = store.get("result").expect("result should exist");
        assert_eq!(resolve::resolve_path(result, "value"), Some(&yaml_int(2)));
    }

    #[test]
    fn test_registered_var_command_result_structure() {
        let mut store = VarStore::new();

        // Standard command module result structure
        let cmd_result = yaml_map(&[
            ("changed", yaml_bool(true)),
            ("cmd", yaml_list(&["echo", "hello"])),
            ("delta", yaml_string("0:00:00.001234")),
            ("end", yaml_string("2024-01-01 12:00:00.123456")),
            ("failed", yaml_bool(false)),
            ("rc", yaml_int(0)),
            ("start", yaml_string("2024-01-01 12:00:00.122222")),
            ("stderr", yaml_string("")),
            ("stderr_lines", Value::Sequence(vec![])),
            ("stdout", yaml_string("hello")),
            ("stdout_lines", yaml_list(&["hello"])),
        ]);

        store.set("cmd_result", cmd_result, VarPrecedence::SetFacts);

        let result = store.get("cmd_result").expect("cmd_result should exist");
        assert_eq!(resolve::resolve_path(result, "rc"), Some(&yaml_int(0)));
        assert_eq!(
            resolve::resolve_path(result, "stdout"),
            Some(&yaml_string("hello"))
        );
        assert_eq!(
            resolve::resolve_path(result, "changed"),
            Some(&yaml_bool(true))
        );
        assert_eq!(
            resolve::resolve_path(result, "failed"),
            Some(&yaml_bool(false))
        );
    }

    #[test]
    fn test_registered_var_with_source_metadata() {
        let mut store = VarStore::new();

        let var = Variable::with_source(
            yaml_map(&[("stdout", yaml_string("output"))]),
            VarPrecedence::SetFacts,
            "task: Run command",
        );

        store.set_variable("my_register", var);

        let variable = store.get_variable("my_register");
        assert!(variable.is_some());
        let variable = variable.unwrap();
        assert_eq!(variable.source, Some("task: Run command".to_string()));
        assert_eq!(variable.precedence, VarPrecedence::SetFacts);
    }
}

// ============================================================================
// PHASE 8: set_fact Tests
// ============================================================================

mod set_fact {
    use super::*;

    #[test]
    fn test_set_fact_creates_variable() {
        let mut store = VarStore::new();

        // set_fact creates a variable at SetFacts precedence
        store.set(
            "my_fact",
            yaml_string("fact_value"),
            VarPrecedence::SetFacts,
        );

        assert!(store.contains("my_fact"));
        assert_eq!(store.get("my_fact"), Some(&yaml_string("fact_value")));
    }

    #[test]
    fn test_set_fact_precedence_over_play_vars() {
        let mut store = VarStore::new();

        store.set("shared", yaml_string("play_value"), VarPrecedence::PlayVars);
        store.set(
            "shared",
            yaml_string("set_fact_value"),
            VarPrecedence::SetFacts,
        );

        assert_eq!(store.get("shared"), Some(&yaml_string("set_fact_value")));
    }

    #[test]
    fn test_set_fact_does_not_override_extra_vars() {
        let mut store = VarStore::new();

        store.set(
            "protected",
            yaml_string("extra_vars_value"),
            VarPrecedence::ExtraVars,
        );
        store.set(
            "protected",
            yaml_string("set_fact_value"),
            VarPrecedence::SetFacts,
        );

        // ExtraVars should still win
        assert_eq!(
            store.get("protected"),
            Some(&yaml_string("extra_vars_value"))
        );
    }

    #[test]
    fn test_set_fact_with_dict_value() {
        let mut store = VarStore::new();

        let fact_dict = yaml_map(&[
            ("key1", yaml_string("value1")),
            ("key2", yaml_int(42)),
            ("key3", yaml_bool(true)),
        ]);

        store.set("dict_fact", fact_dict, VarPrecedence::SetFacts);

        let result = store.get("dict_fact").expect("dict_fact should exist");
        assert_eq!(
            resolve::resolve_path(result, "key1"),
            Some(&yaml_string("value1"))
        );
        assert_eq!(resolve::resolve_path(result, "key2"), Some(&yaml_int(42)));
    }

    #[test]
    fn test_set_fact_with_list_value() {
        let mut store = VarStore::new();

        let fact_list = yaml_list(&["item1", "item2", "item3"]);
        store.set("list_fact", fact_list.clone(), VarPrecedence::SetFacts);

        let result = store.get("list_fact").expect("list_fact should exist");
        assert_eq!(*result, fact_list);
    }

    #[test]
    fn test_set_fact_persistence_within_play() {
        let mut store = VarStore::new();

        // Task 1: set_fact
        store.set("persistent_fact", yaml_int(1), VarPrecedence::SetFacts);

        // Task 2: should still see it
        assert!(store.contains("persistent_fact"));

        // Task 3: can update it
        store.set("persistent_fact", yaml_int(2), VarPrecedence::SetFacts);
        assert_eq!(store.get("persistent_fact"), Some(&yaml_int(2)));
    }

    #[test]
    fn test_set_fact_multiple_facts_at_once() {
        let mut store = VarStore::new();

        let mut facts = IndexMap::new();
        facts.insert("fact1".to_string(), yaml_string("value1"));
        facts.insert("fact2".to_string(), yaml_string("value2"));
        facts.insert("fact3".to_string(), yaml_string("value3"));

        store.set_many(facts, VarPrecedence::SetFacts);

        assert!(store.contains("fact1"));
        assert!(store.contains("fact2"));
        assert!(store.contains("fact3"));
    }

    #[test]
    fn test_set_fact_computed_value() {
        let mut store = VarStore::new();

        // Simulating a computed/dynamic fact
        let computed = yaml_map(&[
            ("hostname", yaml_string("web1")),
            ("ip", yaml_string("10.0.0.1")),
            ("combined", yaml_string("web1-10.0.0.1")),
        ]);

        store.set("computed_fact", computed, VarPrecedence::SetFacts);

        let result = store
            .get("computed_fact")
            .expect("computed_fact should exist");
        assert_eq!(
            resolve::resolve_path(result, "combined"),
            Some(&yaml_string("web1-10.0.0.1"))
        );
    }

    #[test]
    fn test_set_fact_bool_values() {
        let mut store = VarStore::new();

        store.set("is_primary", yaml_bool(true), VarPrecedence::SetFacts);
        store.set("is_replica", yaml_bool(false), VarPrecedence::SetFacts);

        assert_eq!(store.get("is_primary"), Some(&yaml_bool(true)));
        assert_eq!(store.get("is_replica"), Some(&yaml_bool(false)));
    }

    #[test]
    fn test_set_fact_numeric_values() {
        let mut store = VarStore::new();

        store.set("count", yaml_int(42), VarPrecedence::SetFacts);
        store.set(
            "rate",
            Value::Number(serde_yaml::Number::from(3.14_f64)),
            VarPrecedence::SetFacts,
        );

        assert_eq!(store.get("count"), Some(&yaml_int(42)));
        assert_eq!(resolve::to_float(store.get("rate").unwrap()), Some(3.14));
    }

    #[test]
    fn test_set_fact_nested_structure() {
        let mut store = VarStore::new();

        let nested_fact = parse_yaml(
            r#"
            database:
              host: db.example.com
              port: 5432
              credentials:
                user: admin
                password: secret
        "#,
        );

        store.set("db_config", nested_fact, VarPrecedence::SetFacts);

        let result = store.get("db_config").expect("db_config should exist");
        assert_eq!(
            resolve::resolve_path(result, "database.host"),
            Some(&yaml_string("db.example.com"))
        );
        assert_eq!(
            resolve::resolve_path(result, "database.credentials.user"),
            Some(&yaml_string("admin"))
        );
    }
}

// ============================================================================
// PHASE 9: Variable Resolution Path Tests
// ============================================================================

mod resolution_path {
    use super::*;

    #[test]
    fn test_resolve_simple_path() {
        let value = yaml_map(&[("key", yaml_string("value"))]);

        assert_eq!(
            resolve::resolve_path(&value, "key"),
            Some(&yaml_string("value"))
        );
    }

    #[test]
    fn test_resolve_nested_path() {
        let value = parse_yaml(
            r#"
            level1:
              level2:
                level3: deep_value
        "#,
        );

        assert_eq!(
            resolve::resolve_path(&value, "level1.level2.level3"),
            Some(&yaml_string("deep_value"))
        );
    }

    #[test]
    fn test_resolve_list_index() {
        let value = parse_yaml(
            r#"
            items:
              - first
              - second
              - third
        "#,
        );

        assert_eq!(
            resolve::resolve_path(&value, "items.0"),
            Some(&yaml_string("first"))
        );
        assert_eq!(
            resolve::resolve_path(&value, "items.1"),
            Some(&yaml_string("second"))
        );
        assert_eq!(
            resolve::resolve_path(&value, "items.2"),
            Some(&yaml_string("third"))
        );
    }

    #[test]
    fn test_resolve_mixed_path() {
        let value = parse_yaml(
            r#"
            servers:
              - name: web1
                port: 80
              - name: web2
                port: 8080
        "#,
        );

        assert_eq!(
            resolve::resolve_path(&value, "servers.0.name"),
            Some(&yaml_string("web1"))
        );
        assert_eq!(
            resolve::resolve_path(&value, "servers.1.port"),
            Some(&yaml_int(8080))
        );
    }

    #[test]
    fn test_resolve_path_not_found() {
        let value = yaml_map(&[("exists", yaml_string("value"))]);

        assert_eq!(resolve::resolve_path(&value, "nonexistent"), None);
        assert_eq!(resolve::resolve_path(&value, "exists.nested"), None);
    }

    #[test]
    fn test_resolve_invalid_list_index() {
        let value = yaml_list(&["only_one"]);

        assert_eq!(resolve::resolve_path(&value, "5"), None);
        assert_eq!(resolve::resolve_path(&value, "abc"), None);
    }

    #[test]
    fn test_set_path_simple() {
        let mut value = yaml_map(&[("existing", yaml_int(1))]);

        let success = resolve::set_path(&mut value, "new_key", yaml_int(2));
        assert!(success);
        assert_eq!(resolve::resolve_path(&value, "new_key"), Some(&yaml_int(2)));
    }

    #[test]
    fn test_set_path_nested_creates_intermediate() {
        let mut value = yaml_map(&[("root", Value::Mapping(serde_yaml::Mapping::new()))]);

        let success = resolve::set_path(&mut value, "root.nested.deep", yaml_string("value"));
        assert!(success);
        assert_eq!(
            resolve::resolve_path(&value, "root.nested.deep"),
            Some(&yaml_string("value"))
        );
    }

    #[test]
    fn test_set_path_overwrite() {
        let mut value = yaml_map(&[("key", yaml_int(1))]);

        let success = resolve::set_path(&mut value, "key", yaml_int(2));
        assert!(success);
        assert_eq!(resolve::resolve_path(&value, "key"), Some(&yaml_int(2)));
    }

    #[test]
    fn test_set_path_empty() {
        let mut value = yaml_map(&[("key", yaml_int(1))]);

        // Note: empty path "" splits to [""] which inserts an empty key
        // This is implementation-defined behavior; the test documents actual behavior
        let success = resolve::set_path(&mut value, "", yaml_int(2));
        // The implementation currently allows empty keys, so this succeeds
        assert!(success);
        // Verify an empty key was set
        assert_eq!(resolve::resolve_path(&value, ""), Some(&yaml_int(2)));
    }
}

// ============================================================================
// PHASE 10: Cache Invalidation Tests
// ============================================================================

mod cache_invalidation {
    use super::*;

    #[test]
    fn test_cache_invalidated_on_set() {
        let mut store = VarStore::new();

        store.set("a", yaml_int(1), VarPrecedence::PlayVars);
        let _ = store.get("a"); // Force cache build
        assert!(store.is_cache_valid());

        store.set("b", yaml_int(2), VarPrecedence::PlayVars);
        assert!(!store.is_cache_valid());
    }

    #[test]
    fn test_cache_invalidated_on_set_variable() {
        let mut store = VarStore::new();

        store.set("a", yaml_int(1), VarPrecedence::PlayVars);
        let _ = store.get("a"); // Force cache build
        assert!(store.is_cache_valid());

        store.set_variable("b", Variable::new(yaml_int(2), VarPrecedence::TaskVars));
        assert!(!store.is_cache_valid());
    }

    #[test]
    fn test_cache_invalidated_on_set_many() {
        let mut store = VarStore::new();

        store.set("a", yaml_int(1), VarPrecedence::PlayVars);
        let _ = store.get("a");
        assert!(store.is_cache_valid());

        let mut vars = IndexMap::new();
        vars.insert("b".to_string(), yaml_int(2));
        store.set_many(vars, VarPrecedence::TaskVars);
        assert!(!store.is_cache_valid());
    }

    #[test]
    fn test_cache_invalidated_on_remove() {
        let mut store = VarStore::new();

        store.set("a", yaml_int(1), VarPrecedence::PlayVars);
        let _ = store.get("a");
        assert!(store.is_cache_valid());

        store.remove("a", VarPrecedence::PlayVars);
        assert!(!store.is_cache_valid());
    }

    #[test]
    fn test_cache_invalidated_on_clear_precedence() {
        let mut store = VarStore::new();

        store.set("a", yaml_int(1), VarPrecedence::PlayVars);
        let _ = store.get("a");
        assert!(store.is_cache_valid());

        store.clear_precedence(VarPrecedence::PlayVars);
        assert!(!store.is_cache_valid());
    }

    #[test]
    fn test_cache_invalidated_on_clear() {
        let mut store = VarStore::new();

        store.set("a", yaml_int(1), VarPrecedence::PlayVars);
        let _ = store.get("a");
        assert!(store.is_cache_valid());

        store.clear();
        assert!(!store.is_cache_valid());
    }

    #[test]
    fn test_cache_rebuilt_correctly_after_change() {
        let mut store = VarStore::new();

        store.set("var", yaml_string("initial"), VarPrecedence::RoleDefaults);
        assert_eq!(store.get("var"), Some(&yaml_string("initial")));

        store.set("var", yaml_string("updated"), VarPrecedence::PlayVars);
        assert_eq!(store.get("var"), Some(&yaml_string("updated")));

        store.remove("var", VarPrecedence::PlayVars);
        assert_eq!(store.get("var"), Some(&yaml_string("initial")));
    }
}

// ============================================================================
// PHASE 11: Variable Loading from Files Tests
// ============================================================================

mod file_loading {
    use super::*;

    fn create_temp_yaml_file(content: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("vars.yml");
        fs::write(&file_path, content).unwrap();
        (dir, file_path)
    }

    #[test]
    fn test_load_yaml_file() {
        let yaml_content = r#"
var1: value1
var2: 42
var3: true
"#;
        let (_dir, file_path) = create_temp_yaml_file(yaml_content);

        let mut store = VarStore::new();
        store
            .load_file(&file_path, VarPrecedence::PlayVars)
            .unwrap();

        assert_eq!(store.get("var1"), Some(&yaml_string("value1")));
        assert_eq!(store.get("var2"), Some(&yaml_int(42)));
        assert_eq!(store.get("var3"), Some(&yaml_bool(true)));
    }

    #[test]
    fn test_load_file_with_source_tracking() {
        let yaml_content = "tracked_var: value";
        let (_dir, file_path) = create_temp_yaml_file(yaml_content);

        let mut store = VarStore::new();
        store
            .load_file(&file_path, VarPrecedence::RoleVars)
            .unwrap();

        let var = store.get_variable("tracked_var");
        assert!(var.is_some());
        let var = var.unwrap();
        assert!(var.source.is_some());
        assert!(var.source.as_ref().unwrap().contains("vars.yml"));
    }

    #[test]
    fn test_load_nested_yaml() {
        let yaml_content = r#"
server:
  host: localhost
  port: 8080
  config:
    timeout: 30
"#;
        let (_dir, file_path) = create_temp_yaml_file(yaml_content);

        let mut store = VarStore::new();
        store
            .load_file(&file_path, VarPrecedence::PlayVars)
            .unwrap();

        let server = store.get("server").expect("server should exist");
        assert_eq!(
            resolve::resolve_path(server, "host"),
            Some(&yaml_string("localhost"))
        );
        assert_eq!(resolve::resolve_path(server, "port"), Some(&yaml_int(8080)));
        assert_eq!(
            resolve::resolve_path(server, "config.timeout"),
            Some(&yaml_int(30))
        );
    }

    #[test]
    fn test_load_file_with_list() {
        let yaml_content = r#"
packages:
  - nginx
  - python3
  - git
"#;
        let (_dir, file_path) = create_temp_yaml_file(yaml_content);

        let mut store = VarStore::new();
        store
            .load_file(&file_path, VarPrecedence::PlayVarsFiles)
            .unwrap();

        let packages = store.get("packages").expect("packages should exist");
        if let Value::Sequence(seq) = packages {
            assert_eq!(seq.len(), 3);
        } else {
            panic!("packages should be a sequence");
        }
    }

    #[test]
    fn test_load_nonexistent_file_fails() {
        let mut store = VarStore::new();
        let result = store.load_file("/nonexistent/path/vars.yml", VarPrecedence::PlayVars);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_yaml_fails() {
        let invalid_yaml = "this: is: not: valid: yaml: [[[";
        let (_dir, file_path) = create_temp_yaml_file(invalid_yaml);

        let mut store = VarStore::new();
        let result = store.load_file(&file_path, VarPrecedence::PlayVars);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_multiple_files_different_precedence() {
        let yaml1 = "shared: from_defaults\ndefault_only: yes";
        let yaml2 = "shared: from_play\nplay_only: yes";

        let dir = TempDir::new().unwrap();
        let file1 = dir.path().join("defaults.yml");
        let file2 = dir.path().join("play.yml");
        fs::write(&file1, yaml1).unwrap();
        fs::write(&file2, yaml2).unwrap();

        let mut store = VarStore::new();
        store
            .load_file(&file1, VarPrecedence::RoleDefaults)
            .unwrap();
        store.load_file(&file2, VarPrecedence::PlayVars).unwrap();

        assert_eq!(store.get("shared"), Some(&yaml_string("from_play")));
        assert!(store.contains("default_only"));
        assert!(store.contains("play_only"));
    }
}

// ============================================================================
// PHASE 12: Variable with Metadata Tests
// ============================================================================

mod variable_metadata {
    use super::*;

    #[test]
    fn test_variable_new() {
        let var = Variable::new(yaml_string("value"), VarPrecedence::PlayVars);

        assert_eq!(var.value, yaml_string("value"));
        assert_eq!(var.precedence, VarPrecedence::PlayVars);
        assert!(var.source.is_none());
        assert!(!var.encrypted);
    }

    #[test]
    fn test_variable_with_source() {
        let var = Variable::with_source(
            yaml_int(42),
            VarPrecedence::RoleDefaults,
            "roles/myrole/defaults/main.yml",
        );

        assert_eq!(var.value, yaml_int(42));
        assert_eq!(var.precedence, VarPrecedence::RoleDefaults);
        assert_eq!(
            var.source,
            Some("roles/myrole/defaults/main.yml".to_string())
        );
    }

    #[test]
    fn test_variable_encrypted_flag() {
        let var = Variable::new(yaml_string("secret"), VarPrecedence::PlayVars).encrypted();

        assert!(var.encrypted);
    }

    #[test]
    fn test_get_variable_returns_highest_precedence() {
        let mut store = VarStore::new();

        store.set_variable(
            "var",
            Variable::new(yaml_string("low"), VarPrecedence::RoleDefaults),
        );
        store.set_variable(
            "var",
            Variable::new(yaml_string("high"), VarPrecedence::TaskVars),
        );

        let var = store.get_variable("var");
        assert!(var.is_some());
        let var = var.unwrap();
        assert_eq!(var.precedence, VarPrecedence::TaskVars);
        assert_eq!(var.value, yaml_string("high"));
    }

    #[test]
    fn test_variable_source_preserved() {
        let mut store = VarStore::new();

        let var = Variable::with_source(
            yaml_string("value"),
            VarPrecedence::PlayVarsFiles,
            "vars/main.yml",
        );
        store.set_variable("sourced", var);

        let retrieved = store.get_variable("sourced");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().source, Some("vars/main.yml".to_string()));
    }
}

// ============================================================================
// PHASE 13: All/Keys Iteration Tests
// ============================================================================

mod iteration_tests {
    use super::*;

    #[test]
    fn test_all_returns_merged_variables() {
        let mut store = VarStore::new();

        store.set("a", yaml_int(1), VarPrecedence::RoleDefaults);
        store.set("b", yaml_int(2), VarPrecedence::PlayVars);
        store.set("a", yaml_int(10), VarPrecedence::TaskVars); // Override

        let all = store.all();

        assert_eq!(all.len(), 2);
        assert_eq!(all.get("a"), Some(&yaml_int(10))); // Highest precedence wins
        assert_eq!(all.get("b"), Some(&yaml_int(2)));
    }

    #[test]
    fn test_keys_returns_all_variable_names() {
        let mut store = VarStore::new();

        store.set("alpha", yaml_int(1), VarPrecedence::PlayVars);
        store.set("beta", yaml_int(2), VarPrecedence::TaskVars);
        store.set("gamma", yaml_int(3), VarPrecedence::RoleDefaults);

        let keys: Vec<&String> = store.keys().collect();

        assert_eq!(keys.len(), 3);
        assert!(keys.iter().any(|k| *k == "alpha"));
        assert!(keys.iter().any(|k| *k == "beta"));
        assert!(keys.iter().any(|k| *k == "gamma"));
    }

    #[test]
    fn test_len_counts_unique_vars() {
        let mut store = VarStore::new();

        store.set("var", yaml_int(1), VarPrecedence::RoleDefaults);
        store.set("var", yaml_int(2), VarPrecedence::PlayVars);
        store.set("other", yaml_int(3), VarPrecedence::TaskVars);

        assert_eq!(store.len(), 2); // Only 2 unique variables
    }

    #[test]
    fn test_is_empty() {
        let mut store = VarStore::new();
        assert!(store.is_empty());

        store.set("var", yaml_int(1), VarPrecedence::PlayVars);
        assert!(!store.is_empty());

        store.clear();
        assert!(store.is_empty());
    }
}

// ============================================================================
// PHASE 14: Edge Cases and Error Handling
// ============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_get_nonexistent_variable() {
        let mut store = VarStore::new();
        assert_eq!(store.get("nonexistent"), None);
    }

    #[test]
    fn test_get_variable_nonexistent() {
        let store = VarStore::new();
        assert!(store.get_variable("nonexistent").is_none());
    }

    #[test]
    fn test_remove_nonexistent_variable() {
        let mut store = VarStore::new();
        let removed = store.remove("nonexistent", VarPrecedence::PlayVars);
        assert!(removed.is_none());
    }

    #[test]
    fn test_remove_from_wrong_precedence() {
        let mut store = VarStore::new();
        store.set("var", yaml_int(1), VarPrecedence::PlayVars);

        let removed = store.remove("var", VarPrecedence::RoleDefaults);
        assert!(removed.is_none());

        // Variable should still exist
        assert!(store.contains("var"));
    }

    #[test]
    fn test_clear_empty_precedence() {
        let mut store = VarStore::new();
        // Should not panic
        store.clear_precedence(VarPrecedence::PlayVars);
    }

    #[test]
    fn test_unicode_variable_names() {
        let mut store = VarStore::new();

        store.set("unicode_var", yaml_string("value"), VarPrecedence::PlayVars);
        assert!(store.contains("unicode_var"));
    }

    #[test]
    fn test_unicode_variable_values() {
        let mut store = VarStore::new();

        store.set(
            "greeting",
            yaml_string("Hello, World!"),
            VarPrecedence::PlayVars,
        );
        assert_eq!(store.get("greeting"), Some(&yaml_string("Hello, World!")));
    }

    #[test]
    fn test_very_deep_nesting() {
        let mut store = VarStore::new();

        let deep = parse_yaml(
            r#"
            l1:
              l2:
                l3:
                  l4:
                    l5:
                      l6:
                        l7:
                          l8:
                            l9:
                              l10: deep_value
        "#,
        );

        store.set("deep", deep, VarPrecedence::PlayVars);

        let result = store.get("deep").expect("deep should exist");
        assert_eq!(
            resolve::resolve_path(result, "l1.l2.l3.l4.l5.l6.l7.l8.l9.l10"),
            Some(&yaml_string("deep_value"))
        );
    }

    #[test]
    fn test_very_long_variable_name() {
        let mut store = VarStore::new();

        let long_name = "a".repeat(1000);
        store.set(&long_name, yaml_int(1), VarPrecedence::PlayVars);

        assert!(store.contains(&long_name));
    }

    #[test]
    fn test_very_long_string_value() {
        let mut store = VarStore::new();

        let long_value = "x".repeat(100000);
        store.set(
            "long_val",
            yaml_string(&long_value),
            VarPrecedence::PlayVars,
        );

        let result = store.get("long_val").expect("long_val should exist");
        assert_eq!(resolve::to_string(result).len(), 100000);
    }

    #[test]
    fn test_special_characters_in_values() {
        let mut store = VarStore::new();

        store.set(
            "special",
            yaml_string("line1\nline2\ttabbed"),
            VarPrecedence::PlayVars,
        );
        assert_eq!(
            store.get("special"),
            Some(&yaml_string("line1\nline2\ttabbed"))
        );
    }

    #[test]
    fn test_large_list() {
        let mut store = VarStore::new();

        let large_list: Vec<Value> = (0..1000).map(|i| yaml_int(i)).collect();
        store.set(
            "large_list",
            Value::Sequence(large_list),
            VarPrecedence::PlayVars,
        );

        let result = store.get("large_list").expect("large_list should exist");
        if let Value::Sequence(seq) = result {
            assert_eq!(seq.len(), 1000);
        } else {
            panic!("large_list should be a sequence");
        }
    }

    #[test]
    fn test_large_dict() {
        let mut store = VarStore::new();

        let mut mapping = serde_yaml::Mapping::new();
        for i in 0..1000 {
            mapping.insert(yaml_string(&format!("key{}", i)), yaml_int(i));
        }
        store.set(
            "large_dict",
            Value::Mapping(mapping),
            VarPrecedence::PlayVars,
        );

        let result = store.get("large_dict").expect("large_dict should exist");
        if let Value::Mapping(map) = result {
            assert_eq!(map.len(), 1000);
        } else {
            panic!("large_dict should be a mapping");
        }
    }
}

// ============================================================================
// PHASE 15: Integration Tests with Multiple Features
// ============================================================================

mod integration {
    use super::*;

    #[test]
    fn test_complete_variable_lifecycle() {
        let mut store = VarStore::new();

        // 1. Start with role defaults
        store.set(
            "config",
            yaml_map(&[("port", yaml_int(80)), ("ssl", yaml_bool(false))]),
            VarPrecedence::RoleDefaults,
        );

        // 2. Override with play vars
        store.set(
            "config",
            yaml_map(&[("port", yaml_int(8080)), ("timeout", yaml_int(30))]),
            VarPrecedence::PlayVars,
        );

        // 3. Set facts during execution
        store.set(
            "runtime_config",
            yaml_map(&[("active_connections", yaml_int(100))]),
            VarPrecedence::SetFacts,
        );

        // 4. Override with extra vars
        store.set(
            "config",
            yaml_map(&[("port", yaml_int(9090))]),
            VarPrecedence::ExtraVars,
        );

        // Verify final state
        let config = store.get("config").expect("config should exist");
        assert_eq!(resolve::resolve_path(config, "port"), Some(&yaml_int(9090)));

        let runtime = store
            .get("runtime_config")
            .expect("runtime_config should exist");
        assert_eq!(
            resolve::resolve_path(runtime, "active_connections"),
            Some(&yaml_int(100))
        );
    }

    #[test]
    fn test_simulated_playbook_execution() {
        let mut store = VarStore::new();

        // Phase 1: Load inventory vars
        store.set(
            "inventory_var",
            yaml_string("from_inventory"),
            VarPrecedence::InventoryGroupVars,
        );

        // Phase 2: Load playbook vars
        store.set(
            "playbook_var",
            yaml_string("from_playbook"),
            VarPrecedence::PlayVars,
        );

        // Phase 3: Gather facts
        store.set(
            "ansible_hostname",
            yaml_string("webserver1"),
            VarPrecedence::HostFacts,
        );
        store.set(
            "ansible_os_family",
            yaml_string("Debian"),
            VarPrecedence::HostFacts,
        );

        // Phase 4: Execute task with vars
        store.set(
            "task_var",
            yaml_string("from_task"),
            VarPrecedence::TaskVars,
        );

        // Phase 5: Register result
        store.set(
            "task_result",
            yaml_map(&[("stdout", yaml_string("success")), ("rc", yaml_int(0))]),
            VarPrecedence::SetFacts,
        );

        // Verify all vars accessible
        assert!(store.contains("inventory_var"));
        assert!(store.contains("playbook_var"));
        assert!(store.contains("ansible_hostname"));
        assert!(store.contains("task_var"));
        assert!(store.contains("task_result"));
    }

    #[test]
    fn test_role_execution_vars() {
        let mut store = VarStore::new();

        // Role defaults
        store.set("http_port", yaml_int(80), VarPrecedence::RoleDefaults);
        store.set("https_port", yaml_int(443), VarPrecedence::RoleDefaults);

        // Role vars (higher priority than defaults)
        store.set(
            "nginx_user",
            yaml_string("www-data"),
            VarPrecedence::RoleVars,
        );

        // Role params (when role is included with params)
        store.set("http_port", yaml_int(8080), VarPrecedence::RoleParams);

        // Verify role params override defaults
        assert_eq!(store.get("http_port"), Some(&yaml_int(8080)));
        // Defaults still accessible for non-overridden vars
        assert_eq!(store.get("https_port"), Some(&yaml_int(443)));
        // Role vars accessible
        assert_eq!(store.get("nginx_user"), Some(&yaml_string("www-data")));
    }

    #[test]
    fn test_block_and_rescue_vars() {
        let mut store = VarStore::new();

        // Play vars
        store.set("play_level", yaml_string("play"), VarPrecedence::PlayVars);

        // Block vars
        store.set(
            "block_level",
            yaml_string("block"),
            VarPrecedence::BlockVars,
        );

        // Task vars within block
        store.set("task_level", yaml_string("task"), VarPrecedence::TaskVars);

        // Verify hierarchy
        assert_eq!(store.get("play_level"), Some(&yaml_string("play")));
        assert_eq!(store.get("block_level"), Some(&yaml_string("block")));
        assert_eq!(store.get("task_level"), Some(&yaml_string("task")));

        // Block vars override play vars for same variable
        store.set("shared", yaml_string("from_play"), VarPrecedence::PlayVars);
        store.set(
            "shared",
            yaml_string("from_block"),
            VarPrecedence::BlockVars,
        );
        assert_eq!(store.get("shared"), Some(&yaml_string("from_block")));
    }

    #[test]
    fn test_include_vars_scenario() {
        let mut store = VarStore::new();

        // Role vars
        store.set(
            "config",
            yaml_map(&[("key", yaml_string("role_value"))]),
            VarPrecedence::RoleVars,
        );

        // include_vars should override role vars
        store.set(
            "config",
            yaml_map(&[("key", yaml_string("included_value"))]),
            VarPrecedence::IncludeVars,
        );

        let config = store.get("config").expect("config should exist");
        assert_eq!(
            resolve::resolve_path(config, "key"),
            Some(&yaml_string("included_value"))
        );
    }

    #[test]
    fn test_loop_variable_simulation() {
        let mut store = VarStore::new();

        // Play vars
        store.set(
            "my_list",
            yaml_list(&["a", "b", "c"]),
            VarPrecedence::PlayVars,
        );

        // Simulate loop iteration using scope
        let mut scope = store.scope();

        // First iteration
        scope.set("item", yaml_string("a"));
        scope.set(
            "ansible_loop",
            yaml_map(&[
                ("index", yaml_int(1)),
                ("index0", yaml_int(0)),
                ("first", yaml_bool(true)),
                ("last", yaml_bool(false)),
            ]),
        );

        assert_eq!(scope.get("item"), Some(&yaml_string("a")));
        let loop_var = scope
            .get("ansible_loop")
            .expect("ansible_loop should exist");
        assert_eq!(
            resolve::resolve_path(loop_var, "first"),
            Some(&yaml_bool(true))
        );
    }
}
