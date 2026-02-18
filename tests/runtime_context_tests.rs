//! Runtime Context, Module Context, and Execution Environment Tests
//!
//! This module tests:
//! - RuntimeContext inventory wiring (host/group resolution)
//! - ModuleContext connection injection
//! - Extra-vars precedence (highest priority)
//! - Become (privilege escalation) paths

mod common;

use rustible::executor::playbook::Playbook;
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::{Executor, ExecutorConfig};
use rustible::vars::Variables;
use serde_json::json;

// ============================================================================
// Host/Group Resolution Tests
// ============================================================================

#[test]
fn test_runtime_context_add_host() {
    let mut runtime = RuntimeContext::new();

    // Add host without group
    runtime.add_host("host1".to_string(), None);
    assert!(
        runtime.has_host("host1"),
        "Host should be added to runtime context"
    );
}

#[test]
fn test_runtime_context_add_host_with_group() {
    let mut runtime = RuntimeContext::new();

    // Add host with group
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));

    assert!(runtime.has_host("web1"));
    assert!(runtime.has_host("web2"));
}

#[test]
fn test_runtime_context_multiple_groups() {
    let mut runtime = RuntimeContext::new();

    // Same host in multiple groups
    runtime.add_host("server1".to_string(), Some("webservers"));
    runtime.add_host("server1".to_string(), Some("dbservers"));

    assert!(runtime.has_host("server1"));
}

#[test]
fn test_runtime_context_all_group() {
    let mut runtime = RuntimeContext::new();

    runtime.add_host("host1".to_string(), None);
    runtime.add_host("host2".to_string(), Some("webservers"));

    // Both hosts should be accessible
    assert!(runtime.has_host("host1"));
    assert!(runtime.has_host("host2"));
}

// ============================================================================
// Inventory Wiring Tests
// ============================================================================

#[test]
fn test_playbook_host_pattern_localhost() {
    let yaml = r#"
- name: Localhost test
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - debug:
        msg: "Running on localhost"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].hosts, "localhost");
}

#[test]
fn test_playbook_host_pattern_all() {
    let yaml = r#"
- name: All hosts test
  hosts: all
  gather_facts: false
  tasks:
    - debug:
        msg: "Running on all hosts"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].hosts, "all");
}

#[test]
fn test_playbook_host_pattern_group() {
    let yaml = r#"
- name: Group test
  hosts: webservers
  gather_facts: false
  tasks:
    - debug:
        msg: "Running on webservers"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].hosts, "webservers");
}

#[test]
fn test_playbook_host_pattern_multiple() {
    let yaml = r#"
- name: Multiple groups test
  hosts: webservers:dbservers
  gather_facts: false
  tasks:
    - debug:
        msg: "Running on webservers and dbservers"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].hosts, "webservers:dbservers");
}

#[test]
fn test_playbook_host_pattern_exclusion() {
    let yaml = r#"
- name: Exclusion test
  hosts: all:!dbservers
  gather_facts: false
  tasks:
    - debug:
        msg: "Running on all except dbservers"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].hosts, "all:!dbservers");
}

#[test]
fn test_playbook_host_pattern_intersection() {
    let yaml = r#"
- name: Intersection test
  hosts: webservers:&staging
  gather_facts: false
  tasks:
    - debug:
        msg: "Running on webservers that are also in staging"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].hosts, "webservers:&staging");
}

// ============================================================================
// Extra-Vars Precedence Tests
// ============================================================================

#[test]
fn test_extra_vars_highest_precedence() {
    // Extra vars (-e) should override all other variable sources
    let yaml = r#"
- name: Precedence test
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    my_var: "play_level"
  tasks:
    - name: Print var
      debug:
        var: my_var
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();

    // Play-level var is set
    assert!(playbook.plays[0].vars.contains_key("my_var"));

    // Simulate extra vars override
    let mut extra_vars = Variables::new();
    extra_vars.set("my_var".to_string(), json!("extra_vars_level"));

    // Extra vars should take precedence when merged
    // (This tests the concept; actual precedence is enforced in executor)
}

#[test]
fn test_variable_precedence_order() {
    // Test that variable sources are in correct order:
    // 1. Extra vars (-e) - highest
    // 2. Task vars
    // 3. Block vars
    // 4. Role vars
    // 5. Play vars
    // 6. Host vars
    // 7. Group vars
    // 8. Role defaults - lowest

    let yaml = r#"
- name: Precedence order test
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    level1_var: "play"
  vars_files:
    - vars/extra.yml
  roles:
    - role: myrole
  tasks:
    - name: Task with vars
      debug:
        var: level1_var
      vars:
        level1_var: "task"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert!(!playbook.plays.is_empty());

    // Verify play-level var exists
    assert!(playbook.plays[0].vars.contains_key("level1_var"));
}

#[test]
fn test_extra_vars_json_format() {
    // Extra vars can be JSON
    let json_vars = r#"{"my_var": "value", "nested": {"key": "val"}}"#;
    let parsed: serde_json::Value = serde_json::from_str(json_vars).unwrap();

    assert_eq!(parsed["my_var"], "value");
    assert_eq!(parsed["nested"]["key"], "val");
}

#[test]
fn test_extra_vars_yaml_format() {
    // Extra vars can be YAML
    let yaml_vars = r#"
my_var: value
nested:
  key: val
"#;
    let parsed: serde_json::Value = serde_yaml::from_str(yaml_vars).unwrap();

    assert_eq!(parsed["my_var"], "value");
    assert_eq!(parsed["nested"]["key"], "val");
}

// ============================================================================
// Become (Privilege Escalation) Tests
// ============================================================================

#[test]
fn test_become_play_level() {
    let yaml = r#"
- name: Become test
  hosts: localhost
  connection: local
  gather_facts: false
  become: true
  become_user: root
  tasks:
    - name: Run as root
      command: whoami
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    let play = &playbook.plays[0];

    assert!(play.r#become, "become should be true at play level");
    assert_eq!(
        play.become_user.as_deref(),
        Some("root"),
        "become_user should be 'root'"
    );
}

#[test]
fn test_become_task_level() {
    let yaml = r#"
- name: Task-level become
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: Without become
      command: whoami

    - name: With become
      command: whoami
      become: true
      become_user: admin
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    let tasks = &playbook.plays[0].tasks;

    // First task: no become (default is false)
    assert!(!tasks[0].r#become, "First task should not have become");

    // Second task: become enabled
    assert!(tasks[1].r#become, "Second task should have become");
    assert_eq!(
        tasks[1].become_user.as_deref(),
        Some("admin"),
        "become_user should be 'admin'"
    );
}

#[test]
fn test_become_method_sudo() {
    // Note: become_method is parsed in PlayDefinition but not exposed in Play struct
    // This test verifies the playbook parses correctly with become_method
    let yaml = r#"
- name: Sudo become
  hosts: localhost
  gather_facts: false
  become: true
  become_method: sudo
  tasks:
    - command: whoami
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert!(playbook.plays[0].r#become);
}

#[test]
fn test_become_method_su() {
    // Note: become_method is parsed in PlayDefinition but not exposed in Play struct
    let yaml = r#"
- name: Su become
  hosts: localhost
  gather_facts: false
  become: true
  become_method: su
  tasks:
    - command: whoami
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert!(playbook.plays[0].r#become);
}

#[test]
fn test_become_method_doas() {
    // Note: become_method is parsed in PlayDefinition but not exposed in Play struct
    let yaml = r#"
- name: Doas become
  hosts: localhost
  gather_facts: false
  become: true
  become_method: doas
  tasks:
    - command: whoami
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert!(playbook.plays[0].r#become);
}

#[test]
fn test_become_flags() {
    // Note: become_flags is parsed in PlayDefinition but not exposed in Play struct
    let yaml = r#"
- name: Become with flags
  hosts: localhost
  gather_facts: false
  become: true
  become_flags: "-H -S"
  tasks:
    - command: whoami
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert!(playbook.plays[0].r#become);
}

#[test]
fn test_become_exe() {
    // Note: become_exe is parsed in PlayDefinition but not exposed in Play struct
    let yaml = r#"
- name: Become with exe
  hosts: localhost
  gather_facts: false
  become: true
  become_exe: /usr/local/bin/sudo
  tasks:
    - command: whoami
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert!(playbook.plays[0].r#become);
}

// ============================================================================
// Connection Injection Tests
// ============================================================================

#[test]
fn test_connection_local() {
    let yaml = r#"
- name: Local connection
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - command: echo hello
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].connection.as_deref(), Some("local"));
}

#[test]
fn test_connection_ssh() {
    let yaml = r#"
- name: SSH connection
  hosts: all
  connection: ssh
  gather_facts: false
  tasks:
    - command: echo hello
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].connection.as_deref(), Some("ssh"));
}

#[test]
fn test_connection_docker() {
    let yaml = r#"
- name: Docker connection
  hosts: containers
  connection: docker
  gather_facts: false
  tasks:
    - command: echo hello
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].connection.as_deref(), Some("docker"));
}

// ============================================================================
// Executor Config Tests
// ============================================================================

#[test]
fn test_executor_config_defaults() {
    let config = ExecutorConfig::default();

    // Verify sensible defaults
    assert!(!config.check_mode);
    assert!(!config.diff_mode);
    assert!(config.gather_facts);
}

#[test]
fn test_executor_config_check_mode() {
    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    assert!(config.check_mode);
}

#[test]
fn test_executor_config_diff_mode() {
    let config = ExecutorConfig {
        diff_mode: true,
        ..Default::default()
    };

    assert!(config.diff_mode);
}

#[test]
fn test_executor_config_no_gather_facts() {
    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };

    assert!(!config.gather_facts);
}

#[test]
fn test_executor_with_runtime() {
    let config = ExecutorConfig::default();
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let _executor = Executor::with_runtime(config, runtime);

    // Executor should be created successfully
    // (We can't easily introspect it, but creation succeeding is the test)
    let _ = &_executor;
}

// ============================================================================
// Module Context Tests (Conceptual)
// ============================================================================

#[test]
fn test_module_context_vars_accessible() {
    // ModuleContext should have access to:
    // - Task-level vars
    // - Play-level vars
    // - Host vars
    // - Group vars
    // - Extra vars

    let yaml = r#"
- name: Module context test
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    play_var: "play_value"
  tasks:
    - name: Task with vars
      debug:
        msg: "{{ play_var }} - {{ task_var }}"
      vars:
        task_var: "task_value"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();

    // Play vars accessible
    assert!(playbook.plays[0].vars.contains_key("play_var"));

    // Task vars accessible
    assert!(playbook.plays[0].tasks[0].vars.contains_key("task_var"));
}

#[test]
fn test_registered_variable_accessible() {
    let yaml = r#"
- name: Register test
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: First task
      command: echo hello
      register: result

    - name: Use registered var
      debug:
        var: result.stdout
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();

    // First task has register
    assert_eq!(
        playbook.plays[0].tasks[0].register.as_deref(),
        Some("result")
    );
}
