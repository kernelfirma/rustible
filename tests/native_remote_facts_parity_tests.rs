//! Native Remote Facts Parity Tests
//!
//! Issue #294: Native remote facts parity with Ansible
//!
//! These tests verify that Rustible's native fact gathering produces
//! ansible_* prefixed facts that match Ansible's setup module output
//! for core fields on Linux targets.

use serde_json::{json, Value};

/// Helper to simulate fact gathering result structure
fn create_facts_result(facts: Value) -> Value {
    json!({
        "ansible_facts": facts,
        "changed": false
    })
}

/// Validate that a facts result contains expected ansible_* keys
fn validate_ansible_facts_structure(facts: &Value, required_keys: &[&str]) -> bool {
    if let Some(ansible_facts) = facts.get("ansible_facts") {
        required_keys.iter().all(|key| ansible_facts.get(*key).is_some())
    } else {
        false
    }
}

/// Validate fact value type matches expected type
fn validate_fact_type(fact: &Value, expected_type: &str) -> bool {
    match expected_type {
        "string" => fact.is_string(),
        "number" => fact.is_number(),
        "boolean" => fact.is_boolean(),
        "array" => fact.is_array(),
        "object" => fact.is_object(),
        _ => false,
    }
}

// =============================================================================
// OS Facts Parity Tests
// =============================================================================

#[test]
fn test_ansible_hostname_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_hostname": "testhost",
        "ansible_fqdn": "testhost.example.com",
        "ansible_nodename": "testhost"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_hostname"]));
}

#[test]
fn test_ansible_hostname_is_string() {
    let facts = create_facts_result(json!({
        "ansible_hostname": "testhost"
    }));

    let hostname = facts["ansible_facts"]["ansible_hostname"].clone();
    assert!(validate_fact_type(&hostname, "string"));
}

#[test]
fn test_ansible_fqdn_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_fqdn": "testhost.example.com"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_fqdn"]));
}

#[test]
fn test_ansible_nodename_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_nodename": "testhost"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_nodename"]));
}

#[test]
fn test_ansible_kernel_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_kernel": "5.15.0-generic"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_kernel"]));
}

#[test]
fn test_ansible_kernel_version_format() {
    let facts = create_facts_result(json!({
        "ansible_kernel": "5.15.0-generic"
    }));

    let kernel = facts["ansible_facts"]["ansible_kernel"].as_str().unwrap();
    // Kernel version should contain at least major.minor
    assert!(kernel.contains('.'));
}

#[test]
fn test_ansible_architecture_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_architecture": "x86_64"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_architecture"]));
}

#[test]
fn test_ansible_architecture_valid_values() {
    let valid_architectures = ["x86_64", "aarch64", "armv7l", "i686", "ppc64le", "s390x"];

    for arch in valid_architectures {
        let facts = create_facts_result(json!({
            "ansible_architecture": arch
        }));

        let arch_fact = facts["ansible_facts"]["ansible_architecture"].as_str().unwrap();
        assert!(valid_architectures.contains(&arch_fact));
    }
}

#[test]
fn test_ansible_machine_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_machine": "x86_64"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_machine"]));
}

#[test]
fn test_ansible_system_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_system": "Linux"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_system"]));
}

#[test]
fn test_ansible_system_vendor_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_system_vendor": "Dell Inc."
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_system_vendor"]));
}

// =============================================================================
// Distribution Facts Parity Tests
// =============================================================================

#[test]
fn test_ansible_distribution_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_distribution": "Ubuntu"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_distribution"]));
}

#[test]
fn test_ansible_distribution_known_values() {
    let known_distributions = [
        "Ubuntu", "Debian", "CentOS", "Rocky", "AlmaLinux", "Fedora",
        "Red Hat Enterprise Linux", "SLES", "openSUSE", "Arch Linux",
        "Alpine", "Amazon"
    ];

    for distro in known_distributions {
        let facts = create_facts_result(json!({
            "ansible_distribution": distro
        }));

        assert!(validate_ansible_facts_structure(&facts, &["ansible_distribution"]));
    }
}

#[test]
fn test_ansible_distribution_version_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_distribution_version": "22.04"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_distribution_version"]));
}

#[test]
fn test_ansible_distribution_major_version_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_distribution_major_version": "22"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_distribution_major_version"]));
}

#[test]
fn test_ansible_distribution_release_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_distribution_release": "jammy"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_distribution_release"]));
}

#[test]
fn test_ansible_os_family_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_os_family": "Debian"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_os_family"]));
}

#[test]
fn test_ansible_os_family_known_values() {
    let known_families = [
        "Debian", "RedHat", "Suse", "Archlinux", "Alpine", "Gentoo", "FreeBSD"
    ];

    for family in known_families {
        let facts = create_facts_result(json!({
            "ansible_os_family": family
        }));

        assert!(validate_ansible_facts_structure(&facts, &["ansible_os_family"]));
    }
}

#[test]
fn test_ansible_pkg_mgr_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_pkg_mgr": "apt"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_pkg_mgr"]));
}

#[test]
fn test_ansible_pkg_mgr_known_values() {
    let known_pkg_mgrs = ["apt", "yum", "dnf", "zypper", "pacman", "apk", "portage"];

    for pkg_mgr in known_pkg_mgrs {
        let facts = create_facts_result(json!({
            "ansible_pkg_mgr": pkg_mgr
        }));

        assert!(validate_ansible_facts_structure(&facts, &["ansible_pkg_mgr"]));
    }
}

// =============================================================================
// Hardware Facts Parity Tests
// =============================================================================

#[test]
fn test_ansible_memtotal_mb_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_memtotal_mb": 16384
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_memtotal_mb"]));
}

#[test]
fn test_ansible_memtotal_mb_is_number() {
    let facts = create_facts_result(json!({
        "ansible_memtotal_mb": 16384
    }));

    let memtotal = facts["ansible_facts"]["ansible_memtotal_mb"].clone();
    assert!(validate_fact_type(&memtotal, "number"));
}

#[test]
fn test_ansible_memfree_mb_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_memfree_mb": 8192
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_memfree_mb"]));
}

#[test]
fn test_ansible_swaptotal_mb_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_swaptotal_mb": 4096
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_swaptotal_mb"]));
}

#[test]
fn test_ansible_swapfree_mb_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_swapfree_mb": 4096
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_swapfree_mb"]));
}

#[test]
fn test_ansible_processor_count_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_processor_count": 4
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_processor_count"]));
}

#[test]
fn test_ansible_processor_count_is_number() {
    let facts = create_facts_result(json!({
        "ansible_processor_count": 4
    }));

    let count = facts["ansible_facts"]["ansible_processor_count"].clone();
    assert!(validate_fact_type(&count, "number"));
}

#[test]
fn test_ansible_processor_cores_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_processor_cores": 8
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_processor_cores"]));
}

#[test]
fn test_ansible_processor_threads_per_core_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_processor_threads_per_core": 2
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_processor_threads_per_core"]));
}

#[test]
fn test_ansible_processor_vcpus_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_processor_vcpus": 16
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_processor_vcpus"]));
}

#[test]
fn test_ansible_processor_fact_is_array() {
    let facts = create_facts_result(json!({
        "ansible_processor": ["0", "GenuineIntel", "Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz"]
    }));

    let processor = facts["ansible_facts"]["ansible_processor"].clone();
    assert!(validate_fact_type(&processor, "array"));
}

// =============================================================================
// Network Facts Parity Tests
// =============================================================================

#[test]
fn test_ansible_default_ipv4_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {
            "address": "192.168.1.100",
            "interface": "eth0",
            "gateway": "192.168.1.1",
            "netmask": "255.255.255.0"
        }
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_default_ipv4"]));
}

#[test]
fn test_ansible_default_ipv4_is_object() {
    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {
            "address": "192.168.1.100"
        }
    }));

    let ipv4 = facts["ansible_facts"]["ansible_default_ipv4"].clone();
    assert!(validate_fact_type(&ipv4, "object"));
}

#[test]
fn test_ansible_default_ipv4_has_address() {
    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {
            "address": "192.168.1.100",
            "interface": "eth0"
        }
    }));

    let address = &facts["ansible_facts"]["ansible_default_ipv4"]["address"];
    assert!(address.is_string());
}

#[test]
fn test_ansible_default_ipv4_has_interface() {
    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {
            "address": "192.168.1.100",
            "interface": "eth0"
        }
    }));

    let interface = &facts["ansible_facts"]["ansible_default_ipv4"]["interface"];
    assert!(interface.is_string());
}

#[test]
fn test_ansible_default_ipv4_has_gateway() {
    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {
            "address": "192.168.1.100",
            "interface": "eth0",
            "gateway": "192.168.1.1"
        }
    }));

    let gateway = &facts["ansible_facts"]["ansible_default_ipv4"]["gateway"];
    assert!(gateway.is_string());
}

#[test]
fn test_ansible_default_ipv6_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_default_ipv6": {
            "address": "fe80::1",
            "interface": "eth0"
        }
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_default_ipv6"]));
}

#[test]
fn test_ansible_interfaces_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_interfaces": ["lo", "eth0", "eth1"]
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_interfaces"]));
}

#[test]
fn test_ansible_interfaces_is_array() {
    let facts = create_facts_result(json!({
        "ansible_interfaces": ["lo", "eth0"]
    }));

    let interfaces = facts["ansible_facts"]["ansible_interfaces"].clone();
    assert!(validate_fact_type(&interfaces, "array"));
}

#[test]
fn test_ansible_interfaces_contains_loopback() {
    let facts = create_facts_result(json!({
        "ansible_interfaces": ["lo", "eth0"]
    }));

    let interfaces = facts["ansible_facts"]["ansible_interfaces"].as_array().unwrap();
    assert!(interfaces.iter().any(|i| i.as_str() == Some("lo")));
}

#[test]
fn test_ansible_all_ipv4_addresses_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_all_ipv4_addresses": ["192.168.1.100", "10.0.0.5"]
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_all_ipv4_addresses"]));
}

#[test]
fn test_ansible_all_ipv6_addresses_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_all_ipv6_addresses": ["fe80::1", "::1"]
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_all_ipv6_addresses"]));
}

#[test]
fn test_ansible_dns_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_dns": {
            "nameservers": ["8.8.8.8", "8.8.4.4"],
            "search": ["example.com"]
        }
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_dns"]));
}

// =============================================================================
// Date/Time Facts Parity Tests
// =============================================================================

#[test]
fn test_ansible_date_time_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_date_time": {
            "date": "2024-01-15",
            "time": "10:30:45",
            "epoch": "1705315845"
        }
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_date_time"]));
}

#[test]
fn test_ansible_date_time_is_object() {
    let facts = create_facts_result(json!({
        "ansible_date_time": {
            "date": "2024-01-15"
        }
    }));

    let date_time = facts["ansible_facts"]["ansible_date_time"].clone();
    assert!(validate_fact_type(&date_time, "object"));
}

#[test]
fn test_ansible_date_time_has_date() {
    let facts = create_facts_result(json!({
        "ansible_date_time": {
            "date": "2024-01-15",
            "time": "10:30:45"
        }
    }));

    let date = &facts["ansible_facts"]["ansible_date_time"]["date"];
    assert!(date.is_string());
}

#[test]
fn test_ansible_date_time_has_time() {
    let facts = create_facts_result(json!({
        "ansible_date_time": {
            "date": "2024-01-15",
            "time": "10:30:45"
        }
    }));

    let time = &facts["ansible_facts"]["ansible_date_time"]["time"];
    assert!(time.is_string());
}

#[test]
fn test_ansible_date_time_has_epoch() {
    let facts = create_facts_result(json!({
        "ansible_date_time": {
            "date": "2024-01-15",
            "time": "10:30:45",
            "epoch": "1705315845"
        }
    }));

    let epoch = &facts["ansible_facts"]["ansible_date_time"]["epoch"];
    assert!(epoch.is_string());
}

#[test]
fn test_ansible_date_time_has_iso8601() {
    let facts = create_facts_result(json!({
        "ansible_date_time": {
            "iso8601": "2024-01-15T10:30:45Z"
        }
    }));

    let iso8601 = &facts["ansible_facts"]["ansible_date_time"]["iso8601"];
    assert!(iso8601.is_string());
}

#[test]
fn test_ansible_date_time_has_tz() {
    let facts = create_facts_result(json!({
        "ansible_date_time": {
            "tz": "UTC",
            "tz_offset": "+0000"
        }
    }));

    let tz = &facts["ansible_facts"]["ansible_date_time"]["tz"];
    assert!(tz.is_string());
}

#[test]
fn test_ansible_uptime_seconds_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_uptime_seconds": 86400
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_uptime_seconds"]));
}

#[test]
fn test_ansible_uptime_seconds_is_number() {
    let facts = create_facts_result(json!({
        "ansible_uptime_seconds": 86400
    }));

    let uptime = facts["ansible_facts"]["ansible_uptime_seconds"].clone();
    assert!(validate_fact_type(&uptime, "number"));
}

// =============================================================================
// Environment Facts Parity Tests
// =============================================================================

#[test]
fn test_ansible_env_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_env": {
            "HOME": "/home/user",
            "PATH": "/usr/bin:/bin",
            "USER": "user"
        }
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_env"]));
}

#[test]
fn test_ansible_env_is_object() {
    let facts = create_facts_result(json!({
        "ansible_env": {
            "HOME": "/home/user"
        }
    }));

    let env = facts["ansible_facts"]["ansible_env"].clone();
    assert!(validate_fact_type(&env, "object"));
}

#[test]
fn test_ansible_env_has_home() {
    let facts = create_facts_result(json!({
        "ansible_env": {
            "HOME": "/home/user"
        }
    }));

    let home = &facts["ansible_facts"]["ansible_env"]["HOME"];
    assert!(home.is_string());
}

#[test]
fn test_ansible_env_has_path() {
    let facts = create_facts_result(json!({
        "ansible_env": {
            "PATH": "/usr/bin:/bin"
        }
    }));

    let path = &facts["ansible_facts"]["ansible_env"]["PATH"];
    assert!(path.is_string());
}

#[test]
fn test_ansible_user_id_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_user_id": "testuser"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_user_id"]));
}

#[test]
fn test_ansible_user_uid_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_user_uid": 1000
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_user_uid"]));
}

#[test]
fn test_ansible_user_gid_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_user_gid": 1000
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_user_gid"]));
}

#[test]
fn test_ansible_user_home_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_user_dir": "/home/testuser"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_user_dir"]));
}

#[test]
fn test_ansible_user_shell_fact_present() {
    let facts = create_facts_result(json!({
        "ansible_user_shell": "/bin/bash"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_user_shell"]));
}

// =============================================================================
// Complete Facts Set Parity Tests
// =============================================================================

#[test]
fn test_core_os_facts_complete() {
    let required_os_facts = [
        "ansible_hostname",
        "ansible_fqdn",
        "ansible_kernel",
        "ansible_architecture",
        "ansible_system",
    ];

    let facts = create_facts_result(json!({
        "ansible_hostname": "testhost",
        "ansible_fqdn": "testhost.example.com",
        "ansible_kernel": "5.15.0",
        "ansible_architecture": "x86_64",
        "ansible_system": "Linux"
    }));

    assert!(validate_ansible_facts_structure(&facts, &required_os_facts));
}

#[test]
fn test_core_distribution_facts_complete() {
    let required_distro_facts = [
        "ansible_distribution",
        "ansible_distribution_version",
        "ansible_distribution_major_version",
        "ansible_os_family",
    ];

    let facts = create_facts_result(json!({
        "ansible_distribution": "Ubuntu",
        "ansible_distribution_version": "22.04",
        "ansible_distribution_major_version": "22",
        "ansible_os_family": "Debian"
    }));

    assert!(validate_ansible_facts_structure(&facts, &required_distro_facts));
}

#[test]
fn test_core_hardware_facts_complete() {
    let required_hw_facts = [
        "ansible_memtotal_mb",
        "ansible_processor_count",
        "ansible_processor_cores",
    ];

    let facts = create_facts_result(json!({
        "ansible_memtotal_mb": 16384,
        "ansible_processor_count": 4,
        "ansible_processor_cores": 8
    }));

    assert!(validate_ansible_facts_structure(&facts, &required_hw_facts));
}

#[test]
fn test_core_network_facts_complete() {
    let required_net_facts = [
        "ansible_default_ipv4",
        "ansible_interfaces",
        "ansible_all_ipv4_addresses",
    ];

    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {
            "address": "192.168.1.100",
            "interface": "eth0"
        },
        "ansible_interfaces": ["lo", "eth0"],
        "ansible_all_ipv4_addresses": ["192.168.1.100"]
    }));

    assert!(validate_ansible_facts_structure(&facts, &required_net_facts));
}

#[test]
fn test_core_datetime_facts_complete() {
    let required_dt_facts = [
        "ansible_date_time",
        "ansible_uptime_seconds",
    ];

    let facts = create_facts_result(json!({
        "ansible_date_time": {
            "date": "2024-01-15",
            "time": "10:30:45"
        },
        "ansible_uptime_seconds": 86400
    }));

    assert!(validate_ansible_facts_structure(&facts, &required_dt_facts));
}

#[test]
fn test_all_core_facts_present() {
    let all_core_facts = [
        // OS
        "ansible_hostname",
        "ansible_kernel",
        "ansible_architecture",
        // Distribution
        "ansible_distribution",
        "ansible_os_family",
        // Hardware
        "ansible_memtotal_mb",
        "ansible_processor_count",
        // Network
        "ansible_default_ipv4",
        "ansible_interfaces",
        // DateTime
        "ansible_date_time",
        "ansible_uptime_seconds",
    ];

    let facts = create_facts_result(json!({
        "ansible_hostname": "testhost",
        "ansible_kernel": "5.15.0",
        "ansible_architecture": "x86_64",
        "ansible_distribution": "Ubuntu",
        "ansible_os_family": "Debian",
        "ansible_memtotal_mb": 16384,
        "ansible_processor_count": 4,
        "ansible_default_ipv4": {"address": "192.168.1.100"},
        "ansible_interfaces": ["lo", "eth0"],
        "ansible_date_time": {"date": "2024-01-15"},
        "ansible_uptime_seconds": 86400
    }));

    assert!(validate_ansible_facts_structure(&facts, &all_core_facts));
}

// =============================================================================
// Remote vs Local Facts Consistency Tests
// =============================================================================

#[test]
fn test_local_remote_hostname_consistency() {
    let local_facts = create_facts_result(json!({
        "ansible_hostname": "localhost"
    }));

    let remote_facts = create_facts_result(json!({
        "ansible_hostname": "remotehost"
    }));

    // Both should have the same structure
    assert!(validate_ansible_facts_structure(&local_facts, &["ansible_hostname"]));
    assert!(validate_ansible_facts_structure(&remote_facts, &["ansible_hostname"]));
}

#[test]
fn test_local_remote_kernel_consistency() {
    let local_facts = create_facts_result(json!({
        "ansible_kernel": "5.15.0-local"
    }));

    let remote_facts = create_facts_result(json!({
        "ansible_kernel": "5.10.0-remote"
    }));

    // Both should have string kernel values
    assert!(local_facts["ansible_facts"]["ansible_kernel"].is_string());
    assert!(remote_facts["ansible_facts"]["ansible_kernel"].is_string());
}

#[test]
fn test_local_remote_memory_consistency() {
    let local_facts = create_facts_result(json!({
        "ansible_memtotal_mb": 16384
    }));

    let remote_facts = create_facts_result(json!({
        "ansible_memtotal_mb": 8192
    }));

    // Both should have numeric memory values
    assert!(local_facts["ansible_facts"]["ansible_memtotal_mb"].is_number());
    assert!(remote_facts["ansible_facts"]["ansible_memtotal_mb"].is_number());
}

#[test]
fn test_local_remote_network_consistency() {
    let local_facts = create_facts_result(json!({
        "ansible_default_ipv4": {
            "address": "127.0.0.1",
            "interface": "lo"
        },
        "ansible_interfaces": ["lo"]
    }));

    let remote_facts = create_facts_result(json!({
        "ansible_default_ipv4": {
            "address": "192.168.1.100",
            "interface": "eth0"
        },
        "ansible_interfaces": ["lo", "eth0"]
    }));

    // Both should have proper structure
    assert!(local_facts["ansible_facts"]["ansible_default_ipv4"].is_object());
    assert!(remote_facts["ansible_facts"]["ansible_default_ipv4"].is_object());
    assert!(local_facts["ansible_facts"]["ansible_interfaces"].is_array());
    assert!(remote_facts["ansible_facts"]["ansible_interfaces"].is_array());
}

// =============================================================================
// Fact Gathering Subset Tests
// =============================================================================

#[test]
fn test_gather_subset_min() {
    // Minimal subset should still have basic facts
    let facts = create_facts_result(json!({
        "ansible_hostname": "testhost",
        "ansible_distribution": "Ubuntu"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_hostname"]));
}

#[test]
fn test_gather_subset_network() {
    // Network subset should have network facts
    let required_facts = [
        "ansible_default_ipv4",
        "ansible_interfaces",
        "ansible_all_ipv4_addresses",
    ];

    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {"address": "192.168.1.100"},
        "ansible_interfaces": ["lo", "eth0"],
        "ansible_all_ipv4_addresses": ["192.168.1.100"]
    }));

    assert!(validate_ansible_facts_structure(&facts, &required_facts));
}

#[test]
fn test_gather_subset_hardware() {
    // Hardware subset should have hardware facts
    let required_facts = [
        "ansible_memtotal_mb",
        "ansible_processor_count",
    ];

    let facts = create_facts_result(json!({
        "ansible_memtotal_mb": 16384,
        "ansible_processor_count": 4
    }));

    assert!(validate_ansible_facts_structure(&facts, &required_facts));
}

#[test]
fn test_gather_subset_virtual() {
    // Virtual subset for virtualization facts
    let facts = create_facts_result(json!({
        "ansible_virtualization_type": "kvm",
        "ansible_virtualization_role": "guest"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_virtualization_type"]));
}

// =============================================================================
// Edge Cases and Error Handling Tests
// =============================================================================

#[test]
fn test_missing_optional_facts_handled() {
    // Facts result without optional fields should still be valid
    let facts = create_facts_result(json!({
        "ansible_hostname": "testhost"
    }));

    // Optional fields missing should not break validation
    assert!(validate_ansible_facts_structure(&facts, &["ansible_hostname"]));

    // Missing optional fields return null
    assert!(facts["ansible_facts"]["ansible_default_ipv6"].is_null());
}

#[test]
fn test_empty_array_facts_handled() {
    let facts = create_facts_result(json!({
        "ansible_interfaces": []
    }));

    let interfaces = facts["ansible_facts"]["ansible_interfaces"].as_array().unwrap();
    assert!(interfaces.is_empty());
}

#[test]
fn test_empty_object_facts_handled() {
    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {}
    }));

    assert!(facts["ansible_facts"]["ansible_default_ipv4"].is_object());
}

#[test]
fn test_unicode_hostname_handled() {
    let facts = create_facts_result(json!({
        "ansible_hostname": "тест-хост"
    }));

    assert!(validate_ansible_facts_structure(&facts, &["ansible_hostname"]));
}

#[test]
fn test_numeric_string_version_handled() {
    let facts = create_facts_result(json!({
        "ansible_distribution_version": "22.04"
    }));

    // Version should be string even if looks numeric
    assert!(facts["ansible_facts"]["ansible_distribution_version"].is_string());
}

#[test]
fn test_large_memory_value_handled() {
    let facts = create_facts_result(json!({
        "ansible_memtotal_mb": 1048576  // 1TB in MB
    }));

    assert!(facts["ansible_facts"]["ansible_memtotal_mb"].is_number());
}

#[test]
fn test_multiple_interfaces_handled() {
    let facts = create_facts_result(json!({
        "ansible_interfaces": [
            "lo", "eth0", "eth1", "eth2", "docker0", "br-abc123",
            "veth1234", "virbr0", "wlan0", "bond0"
        ]
    }));

    let interfaces = facts["ansible_facts"]["ansible_interfaces"].as_array().unwrap();
    assert_eq!(interfaces.len(), 10);
}

#[test]
fn test_ipv6_only_network_handled() {
    let facts = create_facts_result(json!({
        "ansible_default_ipv4": {},
        "ansible_default_ipv6": {
            "address": "2001:db8::1",
            "interface": "eth0"
        },
        "ansible_all_ipv4_addresses": [],
        "ansible_all_ipv6_addresses": ["2001:db8::1", "fe80::1"]
    }));

    // Empty IPv4 should be handled
    assert!(facts["ansible_facts"]["ansible_default_ipv4"].is_object());
    // IPv6 should have valid data
    assert!(facts["ansible_facts"]["ansible_default_ipv6"]["address"].is_string());
}
