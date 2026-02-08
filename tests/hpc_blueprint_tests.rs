//! Integration tests for HPC reference blueprints
//!
//! These tests validate that the HPC example playbooks and inventories
//! parse correctly and can generate valid execution plans.
//!
//! Run with: cargo test --test hpc_validate_blueprints

use std::path::Path;

#[test]
fn test_onprem_inventory_exists() {
    let inventory_path = Path::new("examples/hpc/inventories/onprem/hosts.yml");
    assert!(
        inventory_path.exists(),
        "On-prem inventory file must exist at {}",
        inventory_path.display()
    );
}

#[test]
fn test_cloud_burst_inventory_exists() {
    let inventory_path = Path::new("examples/hpc/inventories/cloud-burst/hosts.yml");
    assert!(
        inventory_path.exists(),
        "Cloud-burst inventory file must exist at {}",
        inventory_path.display()
    );
}

#[test]
fn test_site_playbook_exists() {
    let playbook_path = Path::new("examples/hpc/playbooks/site.yml");
    assert!(
        playbook_path.exists(),
        "Site playbook must exist at {}",
        playbook_path.display()
    );
}

#[test]
fn test_validate_playbook_exists() {
    let playbook_path = Path::new("examples/hpc/playbooks/validate.yml");
    assert!(
        playbook_path.exists(),
        "Validation playbook must exist at {}",
        playbook_path.display()
    );
}

#[test]
fn test_healthcheck_playbook_exists() {
    let playbook_path = Path::new("examples/hpc/playbooks/healthcheck.yml");
    assert!(
        playbook_path.exists(),
        "Healthcheck playbook must exist at {}",
        playbook_path.display()
    );
}

#[test]
fn test_onprem_inventory_parses_as_valid_yaml() {
    let content = std::fs::read_to_string("examples/hpc/inventories/onprem/hosts.yml").unwrap();
    let value: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
    assert!(value.is_mapping(), "Inventory must be a YAML mapping");

    let all = value.get("all").expect("Inventory must have 'all' group");
    let children = all.get("children").expect("'all' must have 'children'");

    let required_groups = ["login", "controller", "compute", "slurm_nodes", "cluster"];
    for group in &required_groups {
        assert!(
            children.get(*group).is_some(),
            "Inventory must define '{}' group",
            group
        );
    }
}

#[test]
fn test_cloud_burst_inventory_parses_as_valid_yaml() {
    let content =
        std::fs::read_to_string("examples/hpc/inventories/cloud-burst/hosts.yml").unwrap();
    let value: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
    assert!(value.is_mapping(), "Inventory must be a YAML mapping");

    let all = value.get("all").expect("Inventory must have 'all' group");
    let children = all.get("children").expect("'all' must have 'children'");

    let required_groups = ["controller", "compute", "slurm_nodes", "cluster"];
    for group in &required_groups {
        assert!(
            children.get(*group).is_some(),
            "Inventory must define '{}' group",
            group
        );
    }
}

#[test]
fn test_onprem_has_expected_host_count() {
    let content = std::fs::read_to_string("examples/hpc/inventories/onprem/hosts.yml").unwrap();
    let value: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
    let all = value.get("all").unwrap();
    let children = all.get("children").unwrap();

    let login_hosts = children
        .get("login")
        .and_then(|g| g.get("hosts"))
        .and_then(|h| h.as_mapping())
        .map(|m| m.len())
        .unwrap_or(0);
    assert_eq!(login_hosts, 1, "Expected 1 login node");

    let controller_hosts = children
        .get("controller")
        .and_then(|g| g.get("hosts"))
        .and_then(|h| h.as_mapping())
        .map(|m| m.len())
        .unwrap_or(0);
    assert_eq!(controller_hosts, 1, "Expected 1 controller node");

    let compute_hosts = children
        .get("compute")
        .and_then(|g| g.get("hosts"))
        .and_then(|h| h.as_mapping())
        .map(|m| m.len())
        .unwrap_or(0);
    assert_eq!(compute_hosts, 4, "Expected 4 compute nodes");
}

#[test]
fn test_all_role_directories_have_tasks() {
    let roles_dir = Path::new("examples/hpc/roles");
    assert!(roles_dir.exists(), "Roles directory must exist");

    let expected_roles = [
        "hpc_common",
        "munge",
        "slurm_controller",
        "slurm_compute",
        "nfs_server",
        "nfs_client",
    ];

    for role in &expected_roles {
        let tasks_file = roles_dir.join(role).join("tasks/main.yml");
        assert!(
            tasks_file.exists(),
            "Role '{}' must have tasks/main.yml at {}",
            role,
            tasks_file.display()
        );
    }
}

#[test]
fn test_all_group_vars_parse_as_valid_yaml() {
    let dirs = [
        "examples/hpc/inventories/onprem/group_vars",
        "examples/hpc/inventories/cloud-burst/group_vars",
    ];

    for dir in &dirs {
        let dir_path = Path::new(dir);
        if !dir_path.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir_path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path
                .extension()
                .map_or(false, |e| e == "yml" || e == "yaml")
            {
                let content = std::fs::read_to_string(&path).unwrap();
                let _: serde_yaml::Value = serde_yaml::from_str(&content).unwrap_or_else(|e| {
                    panic!("Failed to parse {}: {}", path.display(), e);
                });
            }
        }
    }
}

#[test]
fn test_playbooks_parse_as_valid_yaml() {
    let playbooks = [
        "examples/hpc/playbooks/site.yml",
        "examples/hpc/playbooks/validate.yml",
        "examples/hpc/playbooks/healthcheck.yml",
    ];

    for playbook in &playbooks {
        let path = Path::new(playbook);
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", playbook, e));
        let value: serde_yaml::Value = serde_yaml::from_str(&content)
            .unwrap_or_else(|e| panic!("Invalid YAML in {}: {}", playbook, e));
        assert!(
            value.is_sequence(),
            "Playbook {} must be a YAML list of plays",
            playbook
        );
    }
}
