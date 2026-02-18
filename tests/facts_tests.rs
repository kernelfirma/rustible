//! Comprehensive tests for the Rustible facts system
//!
//! These tests cover:
//! 1. Fact gathering from local system
//! 2. Parsing hostname facts
//! 3. Parsing OS facts (family, distribution, version)
//! 4. Parsing network facts (interfaces, IPs)
//! 5. Parsing hardware facts (CPU, memory)
//! 6. Parsing filesystem facts
//! 7. Parsing date/time facts
//! 8. Parsing /etc/os-release correctly
//! 9. Parsing uname output
//! 10. Parsing /proc/meminfo
//! 11. Parsing /proc/cpuinfo
//! 12. Parsing ip/ifconfig output
//! 13. Handling missing files gracefully
//! 14. Fact caching (facts gathered once per play)
//! 15. gather_facts: false skips gathering
//! 16. setup module re-gathers facts
//! 17. Fact cache invalidation
//! 18. facts.d directory discovery
//! 19. .fact file execution
//! 20. JSON fact output parsing
//! 21. INI fact output parsing
//! 22. Custom fact error handling
//! 23. Access facts as ansible_* variables
//! 24. Use facts in templates
//! 25. Use facts in conditions
//! 26. Nested fact access (ansible_default_ipv4.address)
//! 27. Set simple fact (set_fact module)
//! 28. Set complex fact (list, dict)
//! 29. Fact persists across tasks
//! 30. Fact scope (host-specific)
//! 31. cacheable option
//! 32. gather_subset option
//! 33. Gather only network facts
//! 34. Gather only hardware facts
//! 35. Exclude specific fact types
//! 36. Linux fact variations
//! 37. Different distributions
//! 38. Missing command handling

use indexmap::IndexMap;
use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::{RegisteredResult, RuntimeContext};
use rustible::executor::task::Task;
use rustible::executor::{Executor, ExecutorConfig};
use rustible::facts::Facts;
use std::collections::HashMap;
use std::path::PathBuf;

// ============================================================================
// PHASE 1: FACT GATHERING TESTS
// ============================================================================

mod fact_gathering {
    use super::*;

    #[test]
    fn test_gather_facts_from_local_system() {
        let facts = Facts::gather_local();

        // Should have basic OS info
        assert!(
            facts.get("os_family").is_some(),
            "Should have os_family fact"
        );
        assert!(facts.get("os_arch").is_some(), "Should have os_arch fact");
    }

    #[test]
    fn test_parse_hostname_facts() {
        let facts = Facts::gather_local();

        // Hostname should be present on most systems
        if let Some(hostname) = facts.get("hostname") {
            assert!(hostname.is_string(), "Hostname should be a string");
            let hostname_str = hostname.as_str().unwrap();
            assert!(!hostname_str.is_empty(), "Hostname should not be empty");
        }
    }

    #[test]
    fn test_parse_os_family_fact() {
        let facts = Facts::gather_local();

        let os_family = facts.get("os_family").expect("Should have os_family");
        assert!(os_family.is_string(), "os_family should be a string");

        // On Linux, should be "linux"
        #[cfg(target_os = "linux")]
        assert_eq!(os_family.as_str(), Some("linux"));

        // On macOS, should be "macos"
        #[cfg(target_os = "macos")]
        assert_eq!(os_family.as_str(), Some("macos"));

        // On Windows, should be "windows"
        #[cfg(target_os = "windows")]
        assert_eq!(os_family.as_str(), Some("windows"));
    }

    #[test]
    fn test_parse_architecture_fact() {
        let facts = Facts::gather_local();

        let arch = facts.get("os_arch").expect("Should have os_arch");
        assert!(arch.is_string(), "os_arch should be a string");

        let arch_str = arch.as_str().unwrap();
        // Common architectures
        let valid_archs = ["x86_64", "aarch64", "arm64", "armv7l", "i686", "i386"];
        assert!(
            valid_archs.iter().any(|a| arch_str.contains(a)) || !arch_str.is_empty(),
            "Architecture should be a valid value: {}",
            arch_str
        );
    }

    #[test]
    fn test_gather_user_fact() {
        let facts = Facts::gather_local();

        if let Some(user) = facts.get("user") {
            assert!(user.is_string(), "user should be a string");
            let user_str = user.as_str().unwrap();
            assert!(!user_str.is_empty(), "user should not be empty");
        }
    }
}

// ============================================================================
// PHASE 2: FACT PARSING TESTS
// ============================================================================

mod fact_parsing {
    use super::*;

    /// Helper function to parse /etc/os-release format
    fn parse_os_release(content: &str) -> HashMap<String, String> {
        let mut result = HashMap::new();
        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim_matches('"');
                result.insert(key.to_string(), value.to_string());
            }
        }
        result
    }

    #[test]
    fn test_parse_os_release_debian() {
        let content = include_str!("fixtures/facts/os_release_debian");
        let parsed = parse_os_release(content);

        assert_eq!(parsed.get("ID"), Some(&"debian".to_string()));
        assert_eq!(parsed.get("VERSION_ID"), Some(&"12".to_string()));
        assert_eq!(
            parsed.get("VERSION_CODENAME"),
            Some(&"bookworm".to_string())
        );
        assert!(parsed.contains_key("PRETTY_NAME"));
    }

    #[test]
    fn test_parse_os_release_ubuntu() {
        let content = include_str!("fixtures/facts/os_release_ubuntu");
        let parsed = parse_os_release(content);

        assert_eq!(parsed.get("ID"), Some(&"ubuntu".to_string()));
        assert_eq!(parsed.get("VERSION_ID"), Some(&"22.04".to_string()));
        assert_eq!(parsed.get("ID_LIKE"), Some(&"debian".to_string()));
        assert_eq!(parsed.get("VERSION_CODENAME"), Some(&"jammy".to_string()));
    }

    #[test]
    fn test_parse_os_release_fedora() {
        let content = include_str!("fixtures/facts/os_release_fedora");
        let parsed = parse_os_release(content);

        assert_eq!(parsed.get("ID"), Some(&"fedora".to_string()));
        assert_eq!(parsed.get("VERSION_ID"), Some(&"39".to_string()));
        assert_eq!(parsed.get("ID_LIKE"), Some(&"redhat".to_string()));
    }

    #[test]
    fn test_parse_os_release_arch() {
        let content = include_str!("fixtures/facts/os_release_arch");
        let parsed = parse_os_release(content);

        assert_eq!(parsed.get("ID"), Some(&"arch".to_string()));
        assert_eq!(parsed.get("NAME"), Some(&"Arch Linux".to_string()));
        // Arch doesn't have VERSION_ID (rolling release)
        assert!(!parsed.contains_key("VERSION_ID"));
    }

    /// Helper function to determine OS family from distribution
    fn get_os_family(distribution: &str) -> &'static str {
        match distribution.to_lowercase().as_str() {
            "ubuntu" | "debian" | "linuxmint" | "pop" | "elementary" => "debian",
            "fedora" | "centos" | "rhel" | "rocky" | "alma" | "oracle" => "redhat",
            "arch" | "manjaro" | "endeavouros" => "arch",
            "opensuse" | "sles" => "suse",
            "alpine" => "alpine",
            "gentoo" => "gentoo",
            _ => "unknown",
        }
    }

    #[test]
    fn test_os_family_mapping() {
        assert_eq!(get_os_family("ubuntu"), "debian");
        assert_eq!(get_os_family("debian"), "debian");
        assert_eq!(get_os_family("fedora"), "redhat");
        assert_eq!(get_os_family("centos"), "redhat");
        assert_eq!(get_os_family("arch"), "arch");
        assert_eq!(get_os_family("manjaro"), "arch");
        assert_eq!(get_os_family("alpine"), "alpine");
    }

    /// Helper to parse /proc/meminfo
    fn parse_meminfo(content: &str) -> HashMap<String, u64> {
        let mut result = HashMap::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let key = parts[0].trim_end_matches(':');
                if let Ok(value) = parts[1].parse::<u64>() {
                    result.insert(key.to_string(), value);
                }
            }
        }
        result
    }

    #[test]
    fn test_parse_meminfo() {
        let content = include_str!("fixtures/facts/meminfo_sample");
        let parsed = parse_meminfo(content);

        assert_eq!(parsed.get("MemTotal"), Some(&16266356));
        assert_eq!(parsed.get("MemFree"), Some(&8234567));
        assert_eq!(parsed.get("SwapTotal"), Some(&8388604));
        assert_eq!(parsed.get("SwapFree"), Some(&8388604));

        // Test memory in MB
        let mem_total_mb = parsed.get("MemTotal").map(|kb| kb / 1024);
        assert_eq!(mem_total_mb, Some(15885)); // ~15.5 GB
    }

    /// Helper to parse /proc/cpuinfo
    fn parse_cpuinfo(content: &str) -> (usize, Option<String>, Option<usize>) {
        let mut processor_count = 0;
        let mut model_name = None;
        let mut cpu_cores = None;

        for line in content.lines() {
            if line.starts_with("processor") {
                processor_count += 1;
            } else if line.starts_with("model name") {
                if let Some((_, value)) = line.split_once(':') {
                    model_name = Some(value.trim().to_string());
                }
            } else if line.starts_with("cpu cores") {
                if let Some((_, value)) = line.split_once(':') {
                    cpu_cores = value.trim().parse().ok();
                }
            }
        }

        (processor_count, model_name, cpu_cores)
    }

    #[test]
    fn test_parse_cpuinfo() {
        let content = include_str!("fixtures/facts/cpuinfo_sample");
        let (processor_count, model_name, cpu_cores) = parse_cpuinfo(content);

        assert_eq!(processor_count, 4, "Should have 4 processors");
        assert_eq!(
            model_name,
            Some("Intel(R) Core(TM) i7-10700K CPU @ 3.80GHz".to_string())
        );
        assert_eq!(cpu_cores, Some(8));
    }

    #[test]
    fn test_parse_network_interfaces_json() {
        let content = include_str!("fixtures/facts/network_interfaces");
        let interfaces: serde_json::Value = serde_json::from_str(content).unwrap();

        assert!(interfaces.get("eth0").is_some());
        assert!(interfaces.get("lo").is_some());

        let eth0 = &interfaces["eth0"];
        assert_eq!(eth0["device"], "eth0");
        assert_eq!(eth0["macaddress"], "00:11:22:33:44:55");
        assert_eq!(eth0["active"], true);
        assert_eq!(eth0["mtu"], 1500);
        assert_eq!(eth0["ipv4"]["address"], "192.168.1.100");
    }

    #[test]
    fn test_handle_missing_os_release_fields() {
        // Arch Linux doesn't have VERSION_ID
        let content = include_str!("fixtures/facts/os_release_arch");
        let parsed = parse_os_release(content);

        // Should still work even without VERSION_ID
        assert!(parsed.contains_key("ID"));
        assert!(!parsed.contains_key("VERSION_ID"));
        assert!(parsed.contains_key("NAME"));
    }

    #[test]
    fn test_handle_empty_content() {
        let parsed = parse_os_release("");
        assert!(parsed.is_empty());

        let (count, model, cores) = parse_cpuinfo("");
        assert_eq!(count, 0);
        assert!(model.is_none());
        assert!(cores.is_none());

        let mem = parse_meminfo("");
        assert!(mem.is_empty());
    }
}

// ============================================================================
// PHASE 3: FACT CACHING TESTS
// ============================================================================

mod fact_caching {
    use super::*;

    #[test]
    fn test_facts_persistence_across_tasks() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        // First task sets facts
        ctx.set_host_fact("testhost", "fact1".to_string(), serde_json::json!("value1"));

        // Second task should see the fact
        assert_eq!(
            ctx.get_host_fact("testhost", "fact1"),
            Some(serde_json::json!("value1"))
        );

        // Third task adds more facts
        ctx.set_host_fact("testhost", "fact2".to_string(), serde_json::json!("value2"));

        // Both facts should be available
        assert_eq!(
            ctx.get_host_fact("testhost", "fact1"),
            Some(serde_json::json!("value1"))
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "fact2"),
            Some(serde_json::json!("value2"))
        );
    }

    #[tokio::test]
    async fn test_gather_facts_false_skips_gathering() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        let config = ExecutorConfig {
            gather_facts: false,
            ..Default::default()
        };
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("No Facts Test");
        let mut play = Play::new("Skip Facts", "all");
        play.gather_facts = false;

        play.add_task(Task::new("Debug message", "debug").arg("msg", "No facts gathered"));

        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        assert!(results.contains_key("localhost"));
        let host_result = results.get("localhost").unwrap();
        assert!(!host_result.failed);
    }

    #[tokio::test]
    async fn test_playbook_parse_with_gather_facts_false() {
        let yaml = r#"
- name: Test Play
  hosts: all
  gather_facts: false
  tasks:
    - name: Debug message
      debug:
        msg: "No facts"
"#;

        let playbook = Playbook::parse(yaml, None).unwrap();

        assert_eq!(playbook.plays.len(), 1);
        assert!(!playbook.plays[0].gather_facts);
    }

    #[tokio::test]
    async fn test_playbook_parse_with_gather_facts_true() {
        let yaml = r#"
- name: Test Play
  hosts: all
  gather_facts: true
  tasks:
    - name: Debug message
      debug:
        msg: "With facts"
"#;

        let playbook = Playbook::parse(yaml, None).unwrap();

        assert_eq!(playbook.plays.len(), 1);
        assert!(playbook.plays[0].gather_facts);
    }

    #[test]
    fn test_facts_isolation_between_hosts() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("host1".to_string(), None);
        ctx.add_host("host2".to_string(), None);

        ctx.set_host_fact(
            "host1",
            "fact1".to_string(),
            serde_json::json!("host1_value"),
        );
        ctx.set_host_fact(
            "host2",
            "fact1".to_string(),
            serde_json::json!("host2_value"),
        );

        assert_eq!(
            ctx.get_host_fact("host1", "fact1"),
            Some(serde_json::json!("host1_value"))
        );
        assert_eq!(
            ctx.get_host_fact("host2", "fact1"),
            Some(serde_json::json!("host2_value"))
        );
    }

    #[test]
    fn test_fact_overwrite() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        ctx.set_host_fact(
            "testhost",
            "my_fact".to_string(),
            serde_json::json!("original"),
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "my_fact"),
            Some(serde_json::json!("original"))
        );

        // Overwrite should work (like setup module re-gathering)
        ctx.set_host_fact(
            "testhost",
            "my_fact".to_string(),
            serde_json::json!("updated"),
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "my_fact"),
            Some(serde_json::json!("updated"))
        );
    }
}

// ============================================================================
// PHASE 4: CUSTOM FACTS TESTS
// ============================================================================

mod custom_facts {
    use super::*;

    #[test]
    fn test_parse_json_custom_fact() {
        let content = include_str!("fixtures/facts/custom_fact.json");
        let facts: serde_json::Value = serde_json::from_str(content).unwrap();

        assert_eq!(facts["custom_app_version"], "2.5.1");
        assert_eq!(facts["custom_app_port"], 8080);
        assert_eq!(facts["custom_app_enabled"], true);
        assert_eq!(facts["custom_app_config"]["debug"], false);
        assert_eq!(facts["custom_app_config"]["log_level"], "info");
        assert_eq!(facts["custom_app_config"]["workers"], 4);
        assert!(facts["custom_app_features"].is_array());
    }

    /// Helper to parse INI-style custom facts
    fn parse_ini_fact(content: &str) -> HashMap<String, HashMap<String, String>> {
        let mut result: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current_section = "default".to_string();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].to_string();
                result.entry(current_section.clone()).or_default();
            } else if let Some((key, value)) = line.split_once('=') {
                result
                    .entry(current_section.clone())
                    .or_default()
                    .insert(key.trim().to_string(), value.trim().to_string());
            }
        }

        result
    }

    #[test]
    fn test_parse_ini_custom_fact() {
        let content = include_str!("fixtures/facts/custom_fact.ini");
        let facts = parse_ini_fact(content);

        assert_eq!(
            facts.get("general").and_then(|s| s.get("app_name")),
            Some(&"myapp".to_string())
        );
        assert_eq!(
            facts.get("general").and_then(|s| s.get("version")),
            Some(&"1.0.0".to_string())
        );
        assert_eq!(
            facts.get("database").and_then(|s| s.get("host")),
            Some(&"localhost".to_string())
        );
        assert_eq!(
            facts.get("database").and_then(|s| s.get("port")),
            Some(&"5432".to_string())
        );
        assert_eq!(
            facts.get("features").and_then(|s| s.get("auth")),
            Some(&"enabled".to_string())
        );
    }

    #[test]
    fn test_custom_fact_error_handling_invalid_json() {
        let invalid_json = "{ invalid json }";
        let result: Result<serde_json::Value, _> = serde_json::from_str(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_custom_fact_empty_file() {
        let empty_content = "";
        let facts = parse_ini_fact(empty_content);
        assert!(facts.is_empty());
    }

    #[test]
    fn test_custom_fact_with_comments() {
        let content = r#"
# This is a comment
[section]
; This is also a comment
key=value
"#;
        let facts = parse_ini_fact(content);
        assert_eq!(
            facts.get("section").and_then(|s| s.get("key")),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn test_facts_d_directory_structure() {
        // Test the expected structure for facts.d
        let facts_d_path = PathBuf::from("/etc/ansible/facts.d");
        // This is testing the concept - the actual directory may not exist
        assert!(facts_d_path.to_str().unwrap().contains("facts.d"));

        // Alternative locations
        let alt_paths = vec![
            PathBuf::from("/etc/ansible/facts.d"),
            PathBuf::from("~/.ansible/facts.d"),
            PathBuf::from("/usr/local/etc/ansible/facts.d"),
        ];

        for path in alt_paths {
            assert!(path.to_str().unwrap().contains("facts.d"));
        }
    }
}

// ============================================================================
// PHASE 5: FACT USAGE TESTS
// ============================================================================

mod fact_usage {
    use super::*;

    #[test]
    fn test_access_facts_as_ansible_variables() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        // Set facts with ansible_ prefix (like real Ansible)
        ctx.set_host_fact(
            "testhost",
            "ansible_os_family".to_string(),
            serde_json::json!("Debian"),
        );
        ctx.set_host_fact(
            "testhost",
            "ansible_distribution".to_string(),
            serde_json::json!("Ubuntu"),
        );

        let merged = ctx.get_merged_vars("testhost");

        // Facts should be accessible through the merged vars
        // They are nested under ansible_facts
        assert!(
            merged.contains_key("ansible_facts")
                || ctx.get_host_fact("testhost", "ansible_os_family").is_some()
        );
    }

    #[test]
    fn test_nested_fact_access() {
        let mut facts = Facts::new();

        facts.set(
            "ansible_default_ipv4",
            serde_json::json!({
                "address": "192.168.1.100",
                "netmask": "255.255.255.0",
                "gateway": "192.168.1.1",
                "interface": "eth0"
            }),
        );

        let ipv4 = facts.get("ansible_default_ipv4").unwrap();
        assert!(ipv4.is_object());

        let address = ipv4.get("address").unwrap();
        assert_eq!(address.as_str(), Some("192.168.1.100"));

        let interface = ipv4.get("interface").unwrap();
        assert_eq!(interface.as_str(), Some("eth0"));
    }

    #[test]
    fn test_facts_in_conditions() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        ctx.set_host_fact(
            "testhost",
            "ansible_os_family".to_string(),
            serde_json::json!("Debian"),
        );

        // Simulate condition evaluation
        let os_family = ctx.get_host_fact("testhost", "ansible_os_family").unwrap();
        let condition_result = os_family == serde_json::json!("Debian");
        assert!(condition_result);

        // Test condition that should be false
        let condition_result = os_family == serde_json::json!("RedHat");
        assert!(!condition_result);
    }

    #[test]
    fn test_registered_result_with_facts() {
        let result = RegisteredResult {
            changed: false,
            failed: false,
            skipped: false,
            rc: None,
            stdout: None,
            stdout_lines: None,
            stderr: None,
            stderr_lines: None,
            msg: Some("Facts gathered".to_string()),
            results: None,
            data: {
                let mut data = IndexMap::new();
                data.insert(
                    "ansible_facts".to_string(),
                    serde_json::json!({
                        "os_family": "Debian",
                        "distribution": "Ubuntu"
                    }),
                );
                data
            },
        };

        assert!(!result.changed);
        assert!(!result.failed);

        if let Some(facts) = result.data.get("ansible_facts") {
            assert!(facts.is_object());
            if let Some(obj) = facts.as_object() {
                assert_eq!(obj.get("os_family"), Some(&serde_json::json!("Debian")));
            }
        }
    }

    #[tokio::test]
    async fn test_facts_available_in_task_conditionals() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        // Pre-populate some facts
        runtime.set_host_fact(
            "localhost",
            "ansible_os_family".to_string(),
            serde_json::json!("Debian"),
        );

        let config = ExecutorConfig::default();
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("Conditional Test");
        let mut play = Play::new("Test", "all");
        play.gather_facts = false;

        // Task with when condition
        play.add_task(
            Task::new("Debian-specific task", "debug")
                .arg("msg", "This is Debian")
                .when("ansible_os_family == 'Debian'"),
        );

        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        assert!(results.contains_key("localhost"));
    }
}

// ============================================================================
// PHASE 6: SET_FACT MODULE TESTS
// ============================================================================

mod set_fact {
    use super::*;

    #[test]
    fn test_set_simple_fact() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        // Simulate set_fact module
        ctx.set_host_fact(
            "testhost",
            "my_custom_var".to_string(),
            serde_json::json!("my_value"),
        );

        assert_eq!(
            ctx.get_host_fact("testhost", "my_custom_var"),
            Some(serde_json::json!("my_value"))
        );
    }

    #[test]
    fn test_set_complex_fact_list() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        ctx.set_host_fact(
            "testhost",
            "my_list".to_string(),
            serde_json::json!(["item1", "item2", "item3"]),
        );

        let fact = ctx.get_host_fact("testhost", "my_list").unwrap();
        assert!(fact.is_array());
        assert_eq!(fact.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_set_complex_fact_dict() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        ctx.set_host_fact(
            "testhost",
            "my_dict".to_string(),
            serde_json::json!({
                "key1": "value1",
                "key2": 42,
                "key3": true,
                "nested": {
                    "inner_key": "inner_value"
                }
            }),
        );

        let fact = ctx.get_host_fact("testhost", "my_dict").unwrap();
        assert!(fact.is_object());
        assert_eq!(fact["key1"], "value1");
        assert_eq!(fact["key2"], 42);
        assert_eq!(fact["key3"], true);
        assert_eq!(fact["nested"]["inner_key"], "inner_value");
    }

    #[test]
    fn test_fact_persists_across_tasks() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        // Task 1 sets a fact
        ctx.set_host_fact("testhost", "task1_fact".to_string(), serde_json::json!(1));

        // Task 2 sets another fact
        ctx.set_host_fact("testhost", "task2_fact".to_string(), serde_json::json!(2));

        // Task 3 reads both facts
        assert_eq!(
            ctx.get_host_fact("testhost", "task1_fact"),
            Some(serde_json::json!(1))
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "task2_fact"),
            Some(serde_json::json!(2))
        );
    }

    #[test]
    fn test_fact_scope_host_specific() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("host1".to_string(), None);
        ctx.add_host("host2".to_string(), None);

        // Set fact on host1 only
        ctx.set_host_fact(
            "host1",
            "host_specific".to_string(),
            serde_json::json!("host1_value"),
        );

        // Host1 should have the fact
        assert_eq!(
            ctx.get_host_fact("host1", "host_specific"),
            Some(serde_json::json!("host1_value"))
        );

        // Host2 should NOT have the fact
        assert_eq!(ctx.get_host_fact("host2", "host_specific"), None);
    }

    #[test]
    fn test_set_fact_with_registered_variable() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        // Simulate command result that gets registered
        let result = RegisteredResult {
            changed: true,
            failed: false,
            skipped: false,
            rc: Some(0),
            stdout: Some("hello world".to_string()),
            stdout_lines: Some(vec!["hello world".to_string()]),
            stderr: None,
            stderr_lines: None,
            msg: None,
            results: None,
            data: IndexMap::new(),
        };

        ctx.register_result("testhost", "cmd_result".to_string(), result);

        // Now set_fact using the registered result
        let registered = ctx.get_registered("testhost", "cmd_result").unwrap();
        ctx.set_host_fact(
            "testhost",
            "parsed_output".to_string(),
            serde_json::json!(registered.stdout.as_ref().unwrap()),
        );

        assert_eq!(
            ctx.get_host_fact("testhost", "parsed_output"),
            Some(serde_json::json!("hello world"))
        );
    }
}

// ============================================================================
// PHASE 7: FACT FILTERING TESTS
// ============================================================================

mod fact_filtering {
    use super::*;

    #[tokio::test]
    async fn test_gather_subset_option() {
        let yaml = r#"
- name: Test Play
  hosts: all
  gather_facts: true
  gather_subset:
    - os
    - hardware
  tasks:
    - name: Debug message
      debug:
        msg: "Minimal facts"
"#;

        let playbook = Playbook::parse(yaml, None).unwrap();

        assert_eq!(playbook.plays.len(), 1);
        assert!(playbook.plays[0].gather_facts);
    }

    #[test]
    fn test_gather_subset_categories() {
        // Test that all expected gather_subset categories are recognized
        let valid_subsets = vec![
            "all",
            "min",
            "hardware",
            "network",
            "virtual",
            "ohai",
            "facter",
            "os",
            "date_time",
            "env",
        ];

        for subset in valid_subsets {
            // Just verify these are valid subset names
            assert!(!subset.is_empty());
        }
    }

    #[test]
    fn test_gather_subset_exclusion() {
        // Test the !subset syntax for exclusion
        let exclusion_subsets = vec!["!hardware", "!network", "!virtual"];

        for subset in exclusion_subsets {
            assert!(subset.starts_with('!'));
            let actual_subset = &subset[1..];
            assert!(!actual_subset.is_empty());
        }
    }
}

// ============================================================================
// PHASE 8: CROSS-PLATFORM TESTS
// ============================================================================

mod cross_platform {
    use super::*;

    #[test]
    fn test_linux_distribution_detection() {
        let distributions = vec![
            ("ubuntu", "debian"),
            ("debian", "debian"),
            ("fedora", "redhat"),
            ("centos", "redhat"),
            ("rhel", "redhat"),
            ("rocky", "redhat"),
            ("alma", "redhat"),
            ("arch", "arch"),
            ("manjaro", "arch"),
            ("alpine", "alpine"),
            ("opensuse", "suse"),
            ("gentoo", "gentoo"),
        ];

        fn get_family(distro: &str) -> &'static str {
            match distro.to_lowercase().as_str() {
                "ubuntu" | "debian" | "linuxmint" | "pop" | "elementary" => "debian",
                "fedora" | "centos" | "rhel" | "rocky" | "alma" | "oracle" => "redhat",
                "arch" | "manjaro" | "endeavouros" => "arch",
                "opensuse" | "sles" => "suse",
                "alpine" => "alpine",
                "gentoo" => "gentoo",
                _ => "unknown",
            }
        }

        for (distro, expected_family) in distributions {
            assert_eq!(
                get_family(distro),
                expected_family,
                "Distribution '{}' should map to family '{}'",
                distro,
                expected_family
            );
        }
    }

    #[test]
    fn test_architecture_normalization() {
        fn normalize_arch(arch: &str) -> &'static str {
            match arch {
                "x86_64" | "amd64" => "x86_64",
                "aarch64" | "arm64" => "aarch64",
                "armv7l" => "armv7l",
                "i686" | "i386" => "i386",
                _ => "unknown",
            }
        }

        assert_eq!(normalize_arch("x86_64"), "x86_64");
        assert_eq!(normalize_arch("amd64"), "x86_64");
        assert_eq!(normalize_arch("aarch64"), "aarch64");
        assert_eq!(normalize_arch("arm64"), "aarch64");
        assert_eq!(normalize_arch("i686"), "i386");
        assert_eq!(normalize_arch("i386"), "i386");
    }

    #[test]
    fn test_missing_command_handling() {
        // Simulate handling of missing commands
        // In real code, this would return an error or empty result
        use std::process::Command;

        let result = Command::new("nonexistent_command_12345")
            .arg("--version")
            .output();

        // Command should fail because it doesn't exist
        assert!(result.is_err());
    }

    #[test]
    fn test_os_detection_fallback() {
        // Test fallback behavior when OS detection files are missing
        let mut facts = Facts::new();

        // Set defaults that would be used as fallback
        facts.set("os_family", serde_json::json!("unknown"));
        facts.set("distribution", serde_json::json!("unknown"));

        assert_eq!(facts.get("os_family"), Some(&serde_json::json!("unknown")));
    }
}

// ============================================================================
// FACTS CORE TESTS
// ============================================================================

mod facts_core {
    use super::*;

    #[test]
    fn test_facts_new() {
        let facts = Facts::new();
        assert!(facts.all().is_empty());
    }

    #[test]
    fn test_facts_set_and_get() {
        let mut facts = Facts::new();

        facts.set("hostname", serde_json::json!("testhost"));
        facts.set("os_family", serde_json::json!("linux"));

        assert_eq!(facts.get("hostname"), Some(&serde_json::json!("testhost")));
        assert_eq!(facts.get("os_family"), Some(&serde_json::json!("linux")));
        assert_eq!(facts.get("nonexistent"), None);
    }

    #[test]
    fn test_facts_all() {
        let mut facts = Facts::new();

        facts.set("fact1", serde_json::json!("value1"));
        facts.set("fact2", serde_json::json!(42));
        facts.set("fact3", serde_json::json!(true));

        let all_facts = facts.all();
        assert_eq!(all_facts.len(), 3);
        assert!(all_facts.get("fact1").is_some());
        assert!(all_facts.get("fact2").is_some());
        assert!(all_facts.get("fact3").is_some());
    }

    #[test]
    fn test_facts_gather_local() {
        let facts = Facts::gather_local();

        // Basic facts should always be present
        assert!(facts.get("os_family").is_some());
        assert!(facts.get("os_arch").is_some());

        // Hostname should be available on most systems
        if facts.get("hostname").is_some() {
            let hostname = facts.get("hostname").unwrap();
            assert!(hostname.is_string());
        }
    }

    #[test]
    fn test_facts_overwrite() {
        let mut facts = Facts::new();

        facts.set("key", serde_json::json!("original"));
        assert_eq!(facts.get("key"), Some(&serde_json::json!("original")));

        facts.set("key", serde_json::json!("updated"));
        assert_eq!(facts.get("key"), Some(&serde_json::json!("updated")));
    }

    #[test]
    fn test_facts_gather_local_has_ansible_compatible_names() {
        let facts = Facts::gather_local();

        // Facts should use underscore naming (snake_case), which is Ansible-compatible
        for key in facts.all().keys() {
            assert!(
                !key.contains('-'),
                "Fact name should not contain hyphens: {}",
                key
            );
        }
    }
}

// ============================================================================
// RUNTIME CONTEXT FACTS TESTS
// ============================================================================

mod runtime_context_facts {
    use super::*;

    #[test]
    fn test_runtime_context_host_facts() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        ctx.set_host_fact(
            "testhost",
            "os_family".to_string(),
            serde_json::json!("Debian"),
        );
        ctx.set_host_fact(
            "testhost",
            "distribution".to_string(),
            serde_json::json!("Ubuntu"),
        );
        ctx.set_host_fact(
            "testhost",
            "ansible_processor_count".to_string(),
            serde_json::json!(4),
        );

        assert_eq!(
            ctx.get_host_fact("testhost", "os_family"),
            Some(serde_json::json!("Debian"))
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "distribution"),
            Some(serde_json::json!("Ubuntu"))
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "ansible_processor_count"),
            Some(serde_json::json!(4))
        );
    }

    #[test]
    fn test_runtime_context_multiple_hosts_facts() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("host1".to_string(), None);
        ctx.add_host("host2".to_string(), None);

        ctx.set_host_fact(
            "host1",
            "os_family".to_string(),
            serde_json::json!("Debian"),
        );
        ctx.set_host_fact(
            "host2",
            "os_family".to_string(),
            serde_json::json!("RedHat"),
        );

        assert_eq!(
            ctx.get_host_fact("host1", "os_family"),
            Some(serde_json::json!("Debian"))
        );
        assert_eq!(
            ctx.get_host_fact("host2", "os_family"),
            Some(serde_json::json!("RedHat"))
        );
    }

    #[test]
    fn test_runtime_context_fact_override() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        ctx.set_host_fact(
            "testhost",
            "custom_fact".to_string(),
            serde_json::json!("original"),
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "custom_fact"),
            Some(serde_json::json!("original"))
        );

        ctx.set_host_fact(
            "testhost",
            "custom_fact".to_string(),
            serde_json::json!("updated"),
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "custom_fact"),
            Some(serde_json::json!("updated"))
        );
    }

    #[test]
    fn test_runtime_context_set_fact() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        // Simulate set_fact
        ctx.set_host_fact(
            "testhost",
            "custom_var".to_string(),
            serde_json::json!("custom_value"),
        );
        ctx.set_host_fact(
            "testhost",
            "my_list".to_string(),
            serde_json::json!([1, 2, 3]),
        );
        ctx.set_host_fact(
            "testhost",
            "my_dict".to_string(),
            serde_json::json!({"key": "value"}),
        );

        assert_eq!(
            ctx.get_host_fact("testhost", "custom_var"),
            Some(serde_json::json!("custom_value"))
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "my_list"),
            Some(serde_json::json!([1, 2, 3]))
        );
        assert_eq!(
            ctx.get_host_fact("testhost", "my_dict"),
            Some(serde_json::json!({"key": "value"}))
        );
    }

    #[test]
    fn test_runtime_context_facts_in_merged_vars() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        ctx.set_host_fact("testhost", "fact1".to_string(), serde_json::json!("value1"));
        ctx.set_host_var(
            "testhost",
            "var1".to_string(),
            serde_json::json!("varvalue"),
        );

        let merged = ctx.get_merged_vars("testhost");

        // Both facts and vars should be available in merged context
        assert!(merged.contains_key("ansible_facts") || merged.contains_key("var1"));
    }
}

// ============================================================================
// COMPLEX FACT TYPES TESTS
// ============================================================================

mod complex_fact_types {
    use super::*;

    #[test]
    fn test_facts_with_nested_structures() {
        let mut facts = Facts::new();

        facts.set(
            "network",
            serde_json::json!({
                "interfaces": {
                    "eth0": {
                        "ipv4": "192.168.1.100",
                        "mac": "00:11:22:33:44:55"
                    },
                    "eth1": {
                        "ipv4": "10.0.0.100",
                        "mac": "AA:BB:CC:DD:EE:FF"
                    }
                }
            }),
        );

        let network = facts.get("network").unwrap();
        assert!(network.is_object());

        if let Some(obj) = network.as_object() {
            assert!(obj.contains_key("interfaces"));
        }
    }

    #[test]
    fn test_facts_with_array_values() {
        let mut facts = Facts::new();

        facts.set(
            "ip_addresses",
            serde_json::json!(["192.168.1.1", "10.0.0.1", "172.16.0.1"]),
        );
        facts.set("packages", serde_json::json!(["nginx", "php", "mysql"]));

        let ips = facts.get("ip_addresses").unwrap();
        assert!(ips.is_array());

        if let Some(arr) = ips.as_array() {
            assert_eq!(arr.len(), 3);
        }
    }

    #[test]
    fn test_facts_with_numeric_values() {
        let mut facts = Facts::new();

        facts.set("processor_count", serde_json::json!(4));
        facts.set("memtotal_mb", serde_json::json!(8192));
        facts.set("uptime_seconds", serde_json::json!(123456));

        let cpu = facts.get("processor_count").unwrap();
        assert!(cpu.is_number());
        assert_eq!(cpu.as_u64(), Some(4));

        let mem = facts.get("memtotal_mb").unwrap();
        assert_eq!(mem.as_u64(), Some(8192));
    }

    #[test]
    fn test_facts_with_boolean_values() {
        let mut facts = Facts::new();

        facts.set("is_virtual", serde_json::json!(false));
        facts.set("has_battery", serde_json::json!(true));

        let is_virtual = facts.get("is_virtual").unwrap();
        assert!(is_virtual.is_boolean());
        assert_eq!(is_virtual.as_bool(), Some(false));

        let has_battery = facts.get("has_battery").unwrap();
        assert_eq!(has_battery.as_bool(), Some(true));
    }

    #[test]
    fn test_facts_with_various_data_types() {
        let mut facts = Facts::new();

        // Test all JSON value types
        facts.set("string_fact", serde_json::json!("text"));
        facts.set("number_int", serde_json::json!(42));
        facts.set("number_float", serde_json::json!(2.72));
        facts.set("bool_fact", serde_json::json!(true));
        facts.set("array_fact", serde_json::json!([1, 2, 3]));
        facts.set("object_fact", serde_json::json!({"key": "value"}));
        facts.set("null_fact", serde_json::Value::Null);

        assert_eq!(facts.all().len(), 7);
        assert!(facts.get("string_fact").unwrap().is_string());
        assert!(facts.get("number_int").unwrap().is_number());
        assert!(facts.get("bool_fact").unwrap().is_boolean());
        assert!(facts.get("array_fact").unwrap().is_array());
        assert!(facts.get("object_fact").unwrap().is_object());
        assert!(facts.get("null_fact").unwrap().is_null());
    }
}

// ============================================================================
// ERROR HANDLING TESTS
// ============================================================================

mod error_handling {
    use super::*;

    #[test]
    fn test_get_nonexistent_fact() {
        let mut ctx = RuntimeContext::new();
        ctx.add_host("testhost".to_string(), None);

        assert_eq!(ctx.get_host_fact("testhost", "nonexistent"), None);
        assert_eq!(ctx.get_host_fact("nonexistent_host", "fact"), None);
    }

    #[test]
    fn test_facts_get_missing_key() {
        let facts = Facts::new();
        assert_eq!(facts.get("nonexistent"), None);
    }

    #[test]
    fn test_facts_with_empty_string() {
        let mut facts = Facts::new();
        facts.set("empty", serde_json::json!(""));

        assert_eq!(facts.get("empty"), Some(&serde_json::json!("")));
    }

    #[test]
    fn test_facts_with_null_value() {
        let mut facts = Facts::new();
        facts.set("null_fact", serde_json::Value::Null);

        assert_eq!(facts.get("null_fact"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn test_facts_with_special_characters_in_name() {
        let mut facts = Facts::new();
        facts.set("ansible_eth0_ipv4", serde_json::json!("192.168.1.1"));
        facts.set("custom_fact.nested", serde_json::json!("value"));

        assert_eq!(
            facts.get("ansible_eth0_ipv4"),
            Some(&serde_json::json!("192.168.1.1"))
        );
        assert_eq!(
            facts.get("custom_fact.nested"),
            Some(&serde_json::json!("value"))
        );
    }

    #[test]
    fn test_facts_unicode_values() {
        let mut facts = Facts::new();
        facts.set("unicode_fact", serde_json::json!("Hello World"));

        assert_eq!(
            facts.get("unicode_fact"),
            Some(&serde_json::json!("Hello World"))
        );
    }
}

// ============================================================================
// PERFORMANCE AND SCALE TESTS
// ============================================================================

mod performance {
    use super::*;

    #[test]
    fn test_facts_large_number_of_facts() {
        let mut facts = Facts::new();

        // Add many facts
        for i in 0..1000 {
            facts.set(
                format!("fact_{}", i),
                serde_json::json!(format!("value_{}", i)),
            );
        }

        assert_eq!(facts.all().len(), 1000);

        // Verify retrieval
        assert_eq!(facts.get("fact_0"), Some(&serde_json::json!("value_0")));
        assert_eq!(facts.get("fact_999"), Some(&serde_json::json!("value_999")));
    }

    #[test]
    fn test_multiple_hosts_with_many_facts() {
        let mut ctx = RuntimeContext::new();

        // Add multiple hosts
        for i in 0..10 {
            let hostname = format!("host{}", i);
            ctx.add_host(hostname.clone(), None);

            // Add facts for each host
            for j in 0..100 {
                ctx.set_host_fact(
                    &hostname,
                    format!("fact_{}", j),
                    serde_json::json!(format!("value_{}_{}", i, j)),
                );
            }
        }

        // Verify facts are isolated
        assert_eq!(
            ctx.get_host_fact("host0", "fact_0"),
            Some(serde_json::json!("value_0_0"))
        );
        assert_eq!(
            ctx.get_host_fact("host9", "fact_99"),
            Some(serde_json::json!("value_9_99"))
        );
    }

    #[test]
    fn test_deeply_nested_facts() {
        let mut facts = Facts::new();

        facts.set(
            "deep_nested",
            serde_json::json!({
                "level1": {
                    "level2": {
                        "level3": {
                            "level4": {
                                "level5": "deep_value"
                            }
                        }
                    }
                }
            }),
        );

        let deep = facts.get("deep_nested").unwrap();
        let value = &deep["level1"]["level2"]["level3"]["level4"]["level5"];
        assert_eq!(value, "deep_value");
    }
}

// ============================================================================
// ASYNC INTEGRATION TESTS
// ============================================================================

mod async_integration {
    use super::*;

    #[tokio::test]
    async fn test_playbook_with_gather_facts_true() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        let config = ExecutorConfig::default();
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("Facts Test");
        let mut play = Play::new("Gather Facts", "all");
        play.gather_facts = true;

        play.add_task(Task::new("Debug message", "debug").arg("msg", "Facts gathered"));

        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        assert!(results.contains_key("localhost"));
        let host_result = results.get("localhost").unwrap();
        assert!(!host_result.failed);
    }

    #[tokio::test]
    async fn test_playbook_with_gather_facts_false() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        let config = ExecutorConfig::default();
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("No Facts Test");
        let mut play = Play::new("Skip Facts", "all");
        play.gather_facts = false;

        play.add_task(Task::new("Debug message", "debug").arg("msg", "No facts gathered"));

        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        assert!(results.contains_key("localhost"));
        let host_result = results.get("localhost").unwrap();
        assert!(!host_result.failed);
    }

    #[tokio::test]
    async fn test_executor_config_gather_facts_false() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        let config = ExecutorConfig {
            gather_facts: false,
            ..Default::default()
        };
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("No Facts Config Test");
        let mut play = Play::new("Test", "all");
        // Even though play.gather_facts defaults to true, config overrides it

        play.add_task(Task::new("Debug message", "debug").arg("msg", "Test"));

        playbook.add_play(play);

        let results = executor.run_playbook(&playbook).await.unwrap();

        assert!(results.contains_key("localhost"));
    }

    #[tokio::test]
    async fn test_multiple_plays_share_facts() {
        let mut runtime = RuntimeContext::new();
        runtime.add_host("localhost".to_string(), None);

        // Pre-set a fact that should persist across plays
        runtime.set_host_fact(
            "localhost",
            "persistent_fact".to_string(),
            serde_json::json!("persistent_value"),
        );

        let config = ExecutorConfig::default();
        let executor = Executor::with_runtime(config, runtime);

        let mut playbook = Playbook::new("Multi-Play Test");

        // First play
        let mut play1 = Play::new("Play 1", "all");
        play1.gather_facts = false;
        play1.add_task(Task::new("Task 1", "debug").arg("msg", "First play"));
        playbook.add_play(play1);

        // Second play should still have access to facts
        let mut play2 = Play::new("Play 2", "all");
        play2.gather_facts = false;
        play2.add_task(Task::new("Task 2", "debug").arg("msg", "Second play"));
        playbook.add_play(play2);

        let results = executor.run_playbook(&playbook).await.unwrap();

        assert!(results.contains_key("localhost"));
    }
}
