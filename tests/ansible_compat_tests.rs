//! Ansible Compatibility Tests for Rustible
//!
//! This test suite verifies that Rustible can correctly parse and execute
//! common Ansible patterns and playbook syntax. It tests compatibility with:
//! - Basic playbook syntax
//! - Variable templating ({{ var }}, filters)
//! - Inventory formats (YAML, INI)
//! - Module argument formats
//! - Conditional expressions (when)
//! - Loop syntax (loop, with_items)
//! - Handler notify syntax
//! - Include/import syntax
//! - Role syntax

use rustible::inventory::Inventory;
use rustible::playbook::{Playbook, When};
use std::io::Write;

// ============================================================================
// 1. Basic Playbook Syntax Compatibility
// ============================================================================

#[test]
fn test_simple_playbook_syntax() {
    let yaml = r#"
- name: Simple playbook
  hosts: all
  gather_facts: false
  tasks:
    - name: Test task
      debug:
        msg: "Hello World"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays.len(), 1);
    assert_eq!(pb.plays[0].name, "Simple playbook");
    assert_eq!(pb.plays[0].hosts, "all");
    assert!(!pb.plays[0].gather_facts);
    assert_eq!(pb.plays[0].tasks.len(), 1);
}

#[test]
fn test_multi_play_playbook() {
    let yaml = r#"
- name: Configure webservers
  hosts: webservers
  tasks:
    - name: Install nginx
      package:
        name: nginx
        state: present

- name: Configure databases
  hosts: databases
  tasks:
    - name: Install postgresql
      package:
        name: postgresql
        state: present
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays.len(), 2);
    assert_eq!(pb.plays[0].name, "Configure webservers");
    assert_eq!(pb.plays[1].name, "Configure databases");
}

#[test]
fn test_playbook_with_pre_post_tasks() {
    let yaml = r#"
- name: Deploy application
  hosts: appservers
  pre_tasks:
    - name: Backup before deploy
      command: /usr/local/bin/backup.sh

  tasks:
    - name: Deploy app
      copy:
        src: app.tar.gz
        dest: /opt/app/

  post_tasks:
    - name: Verify deployment
      command: /usr/local/bin/verify.sh
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].pre_tasks.len(), 1);
    assert_eq!(pb.plays[0].tasks.len(), 1);
    assert_eq!(pb.plays[0].post_tasks.len(), 1);
}

// ============================================================================
// 2. Variable Syntax Compatibility
// ============================================================================

#[test]
fn test_variable_templating_basic() {
    let yaml = r#"
- name: Variable test
  hosts: all
  vars:
    app_name: myapp
    app_version: "1.0.0"
  tasks:
    - name: Show app info
      debug:
        msg: "Deploying {{ app_name }} version {{ app_version }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(
        pb.plays[0].vars.as_map().get("app_name").unwrap(),
        &serde_json::json!("myapp")
    );
    assert_eq!(
        pb.plays[0].vars.as_map().get("app_version").unwrap(),
        &serde_json::json!("1.0.0")
    );
}

#[test]
fn test_nested_variable_syntax() {
    let yaml = r#"
- name: Nested vars test
  hosts: all
  vars:
    config:
      database:
        host: localhost
        port: 5432
        name: mydb
  tasks:
    - name: Show database config
      debug:
        msg: "DB: {{ config.database.name }} on {{ config.database.host }}:{{ config.database.port }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let config = pb.plays[0].vars.as_map().get("config").unwrap();
    assert!(config.is_object());
}

#[test]
fn test_list_variable_syntax() {
    let yaml = r#"
- name: List vars test
  hosts: all
  vars:
    packages:
      - nginx
      - postgresql
      - redis
  tasks:
    - name: Show first package
      debug:
        msg: "First package: {{ packages[0] }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let packages = pb.plays[0].vars.as_map().get("packages").unwrap();
    assert!(packages.is_array());
    assert_eq!(packages.as_array().unwrap().len(), 3);
}

// ============================================================================
// 3. Inventory File Format Compatibility
// ============================================================================

#[test]
fn test_yaml_inventory_format() {
    let yaml = r#"
all:
  hosts:
    web1:
      ansible_host: 192.168.1.10
      ansible_user: deploy
    web2:
      ansible_host: 192.168.1.11
      ansible_user: deploy
  children:
    webservers:
      hosts:
        web1:
        web2:
      vars:
        http_port: 80
    databases:
      hosts:
        db1:
          ansible_host: 192.168.1.20
      vars:
        db_port: 5432
"#;

    // Write to temp file and load via public API
    let temp_dir = std::env::temp_dir();
    let inventory_file = temp_dir.join("test_inventory.yml");
    let mut file = std::fs::File::create(&inventory_file).unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    drop(file);

    let inventory = Inventory::load(&inventory_file);
    assert!(inventory.is_ok());

    let inv = inventory.unwrap();
    assert!(inv.get_host("web1").is_some());
    assert!(inv.get_host("web2").is_some());
    assert!(inv.get_host("db1").is_some());
    assert!(inv.get_group("webservers").is_some());
    assert!(inv.get_group("databases").is_some());

    std::fs::remove_file(&inventory_file).ok();
}

#[test]
fn test_ini_inventory_format() {
    let ini = r#"
[webservers]
web1 ansible_host=192.168.1.10 ansible_user=deploy
web2 ansible_host=192.168.1.11 ansible_user=deploy

[databases]
db1 ansible_host=192.168.1.20

[webservers:vars]
http_port=80

[databases:vars]
db_port=5432

[production:children]
webservers
databases
"#;

    // Write to temp file and load via public API
    let temp_dir = std::env::temp_dir();
    let inventory_file = temp_dir.join("test_inventory.ini");
    let mut file = std::fs::File::create(&inventory_file).unwrap();
    file.write_all(ini.as_bytes()).unwrap();
    drop(file);

    let inventory = Inventory::load(&inventory_file);
    assert!(inventory.is_ok());

    let inv = inventory.unwrap();
    assert!(inv.get_host("web1").is_some());
    assert!(inv.get_host("web2").is_some());
    assert!(inv.get_host("db1").is_some());

    let webservers = inv.get_group("webservers").unwrap();
    assert!(webservers.has_var("http_port"));

    let production = inv.get_group("production").unwrap();
    assert!(production.children.contains("webservers"));
    assert!(production.children.contains("databases"));

    std::fs::remove_file(&inventory_file).ok();
}

#[test]
fn test_inventory_host_patterns() {
    let ini = r#"
[webservers]
web1
web2
web3

[databases]
db1
db2
"#;

    // Write to temp file and load via public API
    let temp_dir = std::env::temp_dir();
    let inventory_file = temp_dir.join("test_patterns.ini");
    let mut file = std::fs::File::create(&inventory_file).unwrap();
    file.write_all(ini.as_bytes()).unwrap();
    drop(file);

    let inventory = Inventory::load(&inventory_file).unwrap();

    // Test 'all' pattern
    let all = inventory.get_hosts_for_pattern("all").unwrap();
    assert_eq!(all.len(), 5);

    // Test group pattern
    let webs = inventory.get_hosts_for_pattern("webservers").unwrap();
    assert_eq!(webs.len(), 3);

    // Test wildcard pattern
    let web_wildcard = inventory.get_hosts_for_pattern("web*").unwrap();
    assert_eq!(web_wildcard.len(), 3);

    // Test single host
    let single = inventory.get_hosts_for_pattern("web1").unwrap();
    assert_eq!(single.len(), 1);

    std::fs::remove_file(&inventory_file).ok();
}

// ============================================================================
// 4. Common Module Argument Formats
// ============================================================================

#[test]
fn test_module_key_value_args() {
    let yaml = r#"
- name: Module args test
  hosts: all
  tasks:
    - name: Install package
      package:
        name: nginx
        state: present
        update_cache: yes
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert_eq!(task.module_name(), "package");

    let args = task.module_args();
    assert!(args.is_object());
}

#[test]
fn test_module_string_args() {
    let yaml = r#"
- name: Command module test
  hosts: all
  tasks:
    - name: Run command
      command: echo "Hello World"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert_eq!(task.module_name(), "command");
}

#[test]
fn test_module_multiline_args() {
    let yaml = r#"
- name: Multiline args
  hosts: all
  tasks:
    - name: Create file
      copy:
        dest: /etc/config.txt
        content: |
          Line 1
          Line 2
          Line 3
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert_eq!(task.module_name(), "copy");
}

// ============================================================================
// 5. When Conditions Compatibility
// ============================================================================

#[test]
fn test_when_condition_simple() {
    let yaml = r#"
- name: Conditional tasks
  hosts: all
  tasks:
    - name: Run on Debian
      debug:
        msg: "This is Debian"
      when: ansible_os_family == "Debian"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.when.is_some());

    if let Some(When::Single(condition)) = &task.when {
        assert_eq!(condition, "ansible_os_family == \"Debian\"");
    } else {
        panic!("Expected single when condition");
    }
}

#[test]
fn test_when_condition_boolean() {
    let yaml = r#"
- name: Boolean conditions
  hosts: all
  vars:
    install_nginx: true
  tasks:
    - name: Install if enabled
      package:
        name: nginx
      when: install_nginx
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.when.is_some());
}

#[test]
fn test_when_condition_multiple() {
    let yaml = r#"
- name: Multiple conditions
  hosts: all
  tasks:
    - name: Run with multiple conditions
      debug:
        msg: "All conditions met"
      when:
        - ansible_os_family == "Debian"
        - ansible_distribution_version >= "20.04"
        - deploy_mode == "production"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];

    if let Some(When::Multiple(conditions)) = &task.when {
        assert_eq!(conditions.len(), 3);
    } else {
        panic!("Expected multiple when conditions");
    }
}

#[test]
fn test_when_condition_defined() {
    let yaml = r#"
- name: Defined check
  hosts: all
  tasks:
    - name: Run if variable defined
      debug:
        msg: "Variable is defined"
      when: some_var is defined
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.when.is_some());
}

#[test]
fn test_when_condition_in_list() {
    let yaml = r#"
- name: In list check
  hosts: all
  tasks:
    - name: Run if in list
      debug:
        msg: "Item in list"
      when: item in valid_items
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.when.is_some());
}

// ============================================================================
// 6. Loop Syntax Compatibility
// ============================================================================

#[test]
fn test_loop_basic() {
    let yaml = r#"
- name: Loop test
  hosts: all
  tasks:
    - name: Install packages
      package:
        name: "{{ item }}"
        state: present
      loop:
        - nginx
        - postgresql
        - redis
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.loop_.is_some());

    if let Some(loop_items) = &task.loop_ {
        assert!(loop_items.is_array());
        assert_eq!(loop_items.as_array().unwrap().len(), 3);
    }
}

#[test]
fn test_with_items() {
    let yaml = r#"
- name: With items test
  hosts: all
  tasks:
    - name: Create users
      user:
        name: "{{ item }}"
        state: present
      with_items:
        - alice
        - bob
        - charlie
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.with_items.is_some());
}

#[test]
fn test_loop_with_dict() {
    let yaml = r#"
- name: Loop with dictionaries
  hosts: all
  tasks:
    - name: Create users with home dirs
      user:
        name: "{{ item.name }}"
        home: "{{ item.home }}"
      loop:
        - name: alice
          home: /home/alice
        - name: bob
          home: /home/bob
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.loop_.is_some());
}

#[test]
fn test_loop_control() {
    let yaml = r#"
- name: Loop control test
  hosts: all
  tasks:
    - name: Install with custom loop var
      package:
        name: "{{ pkg }}"
      loop:
        - nginx
        - redis
      loop_control:
        loop_var: pkg
        pause: 2
        label: "Installing {{ pkg }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.loop_control.is_some());

    if let Some(ref control) = task.loop_control {
        assert_eq!(control.loop_var, "pkg");
        assert_eq!(control.pause, Some(2));
    }
}

// ============================================================================
// 7. Handler Notify Syntax
// ============================================================================

#[test]
fn test_handler_basic() {
    let yaml = r#"
- name: Handler test
  hosts: all
  tasks:
    - name: Update config
      copy:
        src: nginx.conf
        dest: /etc/nginx/nginx.conf
      notify: restart nginx

  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert_eq!(task.notify.len(), 1);
    assert_eq!(task.notify[0], "restart nginx");

    assert_eq!(pb.plays[0].handlers.len(), 1);
    assert_eq!(pb.plays[0].handlers[0].name, "restart nginx");
}

#[test]
fn test_handler_multiple_notify() {
    let yaml = r#"
- name: Multiple handlers
  hosts: all
  tasks:
    - name: Update config files
      copy:
        src: config.tar.gz
        dest: /etc/app/
      notify:
        - restart app
        - reload nginx
        - clear cache

  handlers:
    - name: restart app
      service:
        name: app
        state: restarted

    - name: reload nginx
      service:
        name: nginx
        state: reloaded

    - name: clear cache
      command: /usr/local/bin/clear-cache.sh
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert_eq!(task.notify.len(), 3);

    assert_eq!(pb.plays[0].handlers.len(), 3);
}

#[test]
fn test_handler_with_listen() {
    let yaml = r#"
- name: Handler listen test
  hosts: all
  tasks:
    - name: Update app
      copy:
        src: app.jar
        dest: /opt/app/
      notify: app updated

  handlers:
    - name: restart app service
      service:
        name: app
        state: restarted
      listen: app updated

    - name: clear app cache
      command: /usr/local/bin/clear-cache.sh
      listen: app updated
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].handlers.len(), 2);
    assert!(pb.plays[0].handlers[0]
        .listen
        .contains(&"app updated".to_string()));
}

// ============================================================================
// 8. Include/Import Syntax
// ============================================================================

#[test]
fn test_include_tasks_syntax() {
    let yaml = r#"
- name: Include test
  hosts: all
  tasks:
    - name: Include web server tasks
      include_tasks: tasks/webserver.yml

    - name: Include with vars
      include_tasks: tasks/database.yml
      vars:
        db_name: production
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 2);
}

#[test]
fn test_import_tasks_syntax() {
    let yaml = r#"
- name: Import test
  hosts: all
  tasks:
    - name: Import common tasks
      import_tasks: tasks/common.yml
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_include_vars_syntax() {
    let yaml = r#"
- name: Include vars test
  hosts: all
  tasks:
    - name: Include variable file
      include_vars: vars/app_config.yml

    - name: Include specific vars
      include_vars:
        file: vars/database.yml
        name: db_config
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 2);
}

// ============================================================================
// 9. Role Syntax
// ============================================================================

#[test]
fn test_role_simple() {
    let yaml = r#"
- name: Role test
  hosts: all
  roles:
    - common
    - webserver
    - database
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 3);
}

#[test]
fn test_role_with_vars() {
    let yaml = r#"
- name: Role with vars
  hosts: all
  roles:
    - role: nginx
      vars:
        nginx_port: 8080
        nginx_workers: 4
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 1);
}

#[test]
fn test_role_with_when() {
    let yaml = r#"
- name: Conditional role
  hosts: all
  roles:
    - role: docker
      when: install_docker is defined and install_docker
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 1);
}

#[test]
fn test_role_with_tags() {
    let yaml = r#"
- name: Role with tags
  hosts: all
  roles:
    - role: nginx
      tags:
        - webserver
        - nginx
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 1);
}

// ============================================================================
// 10. Advanced Playbook Features
// ============================================================================

#[test]
fn test_become_syntax() {
    let yaml = r#"
- name: Privilege escalation
  hosts: all
  become: true
  become_user: root
  become_method: sudo
  tasks:
    - name: Install system package
      package:
        name: vim
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].r#become, Some(true));
    assert_eq!(pb.plays[0].become_user, Some("root".to_string()));
    assert_eq!(pb.plays[0].become_method, Some("sudo".to_string()));
}

#[test]
fn test_vars_files() {
    let yaml = r#"
- name: Vars files test
  hosts: all
  vars_files:
    - vars/common.yml
    - vars/{{ environment }}.yml
  tasks:
    - name: Use vars
      debug:
        msg: "Environment: {{ environment }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].vars_files.len(), 2);
}

#[test]
fn test_tags() {
    let yaml = r#"
- name: Tagged tasks
  hosts: all
  tasks:
    - name: Install packages
      package:
        name: nginx
      tags:
        - packages
        - nginx

    - name: Configure nginx
      copy:
        src: nginx.conf
        dest: /etc/nginx/
      tags:
        - configuration
        - nginx
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks[0].tags.len(), 2);
    assert_eq!(pb.plays[0].tasks[1].tags.len(), 2);
}

#[test]
fn test_ignore_errors() {
    let yaml = r#"
- name: Error handling
  hosts: all
  tasks:
    - name: Command that might fail
      command: /usr/local/bin/risky-command.sh
      ignore_errors: yes
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].tasks[0].ignore_errors);
}

#[test]
fn test_register() {
    let yaml = r#"
- name: Register test
  hosts: all
  tasks:
    - name: Get system info
      command: uname -a
      register: system_info

    - name: Show system info
      debug:
        var: system_info.stdout
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(
        pb.plays[0].tasks[0].register,
        Some("system_info".to_string())
    );
}

#[test]
fn test_changed_when() {
    let yaml = r#"
- name: Changed when test
  hosts: all
  tasks:
    - name: Check service status
      command: systemctl status nginx
      register: result
      changed_when: false
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].tasks[0].changed_when.is_some());
}

#[test]
fn test_failed_when() {
    let yaml = r#"
- name: Failed when test
  hosts: all
  tasks:
    - name: Run command
      command: /usr/local/bin/check-status.sh
      register: result
      failed_when: result.rc != 0 and result.rc != 2
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].tasks[0].failed_when.is_some());
}

#[test]
fn test_delegate_to() {
    let yaml = r#"
- name: Delegation test
  hosts: all
  tasks:
    - name: Run on localhost
      command: echo "Running on controller"
      delegate_to: localhost
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(
        pb.plays[0].tasks[0].delegate_to,
        Some("localhost".to_string())
    );
}

#[test]
fn test_run_once() {
    let yaml = r#"
- name: Run once test
  hosts: all
  tasks:
    - name: Initialize database
      command: /usr/local/bin/init-db.sh
      run_once: true
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].tasks[0].run_once);
}

#[test]
fn test_environment_vars() {
    let yaml = r#"
- name: Environment test
  hosts: all
  environment:
    PATH: "/usr/local/bin:{{ ansible_env.PATH }}"
    http_proxy: "http://proxy.example.com:8080"
  tasks:
    - name: Run with environment
      command: env
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(!pb.plays[0].environment.is_empty());
}

#[test]
fn test_serial_execution() {
    let yaml = r#"
- name: Serial test
  hosts: all
  serial: 2
  tasks:
    - name: Update one at a time
      package:
        name: nginx
        state: latest
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].serial.is_some());
}

#[test]
fn test_max_fail_percentage() {
    let yaml = r#"
- name: Failure tolerance
  hosts: all
  max_fail_percentage: 25
  tasks:
    - name: Risky operation
      command: /usr/local/bin/risky.sh
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].max_fail_percentage, Some(25));
}

// ============================================================================
// 11. Complex Real-World Playbook
// ============================================================================

#[test]
fn test_complex_real_world_playbook() {
    let yaml = r#"
---
- name: Deploy web application
  hosts: webservers
  become: true
  vars:
    app_name: myapp
    app_version: "2.0.0"
    deploy_user: deploy
    app_packages:
      - nginx
      - python3-pip
      - supervisor

  pre_tasks:
    - name: Update package cache
      apt:
        update_cache: yes
      when: ansible_os_family == "Debian"
      tags: packages

  tasks:
    - name: Install required packages
      package:
        name: "{{ item }}"
        state: present
      loop: "{{ app_packages }}"
      tags: packages

    - name: Create application user
      user:
        name: "{{ deploy_user }}"
        state: present
        shell: /bin/bash
      tags: users

    - name: Deploy application files
      copy:
        src: "{{ app_name }}-{{ app_version }}.tar.gz"
        dest: "/opt/{{ app_name }}/"
      notify:
        - restart application
        - reload nginx
      tags: deploy

    - name: Configure nginx
      template:
        src: nginx.conf.j2
        dest: "/etc/nginx/sites-available/{{ app_name }}"
      notify: reload nginx
      tags: configuration

    - name: Enable nginx site
      file:
        src: "/etc/nginx/sites-available/{{ app_name }}"
        dest: "/etc/nginx/sites-enabled/{{ app_name }}"
        state: link
      notify: reload nginx
      tags: configuration

  post_tasks:
    - name: Verify application is running
      uri:
        url: "http://localhost:8000/health"
        status_code: 200
      retries: 5
      delay: 3
      tags: verify

  handlers:
    - name: restart application
      service:
        name: "{{ app_name }}"
        state: restarted

    - name: reload nginx
      service:
        name: nginx
        state: reloaded

- name: Configure database servers
  hosts: databases
  become: true
  vars:
    db_name: production_db
    db_user: app_user

  tasks:
    - name: Install PostgreSQL
      package:
        name: postgresql
        state: present

    - name: Create database
      postgresql_db:
        name: "{{ db_name }}"
        state: present
      become_user: postgres

    - name: Create database user
      postgresql_user:
        name: "{{ db_user }}"
        db: "{{ db_name }}"
        priv: ALL
      become_user: postgres
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays.len(), 2);

    // First play validation
    assert_eq!(pb.plays[0].name, "Deploy web application");
    assert_eq!(pb.plays[0].hosts, "webservers");
    assert_eq!(pb.plays[0].r#become, Some(true));
    assert!(!pb.plays[0].vars.is_empty());
    assert_eq!(pb.plays[0].pre_tasks.len(), 1);
    assert_eq!(pb.plays[0].tasks.len(), 5);
    assert_eq!(pb.plays[0].post_tasks.len(), 1);
    assert_eq!(pb.plays[0].handlers.len(), 2);

    // Second play validation
    assert_eq!(pb.plays[1].name, "Configure database servers");
    assert_eq!(pb.plays[1].hosts, "databases");
    assert_eq!(pb.plays[1].tasks.len(), 3);
}

// ============================================================================
// 12. Validation Tests
// ============================================================================

#[test]
fn test_playbook_validation_no_plays() {
    let yaml = "[]";

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let validation = pb.validate();
    assert!(validation.is_err());
}

#[test]
fn test_playbook_validation_no_hosts() {
    let yaml = r#"
- name: Invalid play
  tasks:
    - name: Test
      debug:
        msg: "Hello"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    // Should parse but fail validation
    if let Ok(pb) = playbook {
        let validation = pb.validate();
        assert!(validation.is_err());
    }
}

#[test]
fn test_task_validation_no_module() {
    let yaml = r#"
- name: Invalid task
  hosts: all
  tasks:
    - name: Task without module
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    // Parser should handle this gracefully
    assert!(playbook.is_ok());
}

// ============================================================================
// 13. Fixture File Tests - Basic Playbook
// ============================================================================

#[test]
fn test_fixture_basic_playbook() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ansible_compat/basic_playbook.yml");

    if fixture_path.exists() {
        let content = std::fs::read_to_string(&fixture_path).unwrap();
        let playbook = Playbook::from_yaml(&content, Some(fixture_path));
        assert!(
            playbook.is_ok(),
            "Failed to parse basic_playbook.yml: {:?}",
            playbook.err()
        );

        let pb = playbook.unwrap();
        assert_eq!(pb.plays.len(), 1);
        assert_eq!(pb.plays[0].name, "Basic playbook test");
        assert_eq!(pb.plays[0].hosts, "all");
        assert!(!pb.plays[0].gather_facts);

        // Check vars
        assert!(pb.plays[0].vars.as_map().contains_key("app_name"));
        assert!(pb.plays[0].vars.as_map().contains_key("debug_mode"));

        // Check tasks
        assert!(pb.plays[0].tasks.len() >= 4);
    }
}

// ============================================================================
// 14. Fixture File Tests - Multi-Play Playbook
// ============================================================================

#[test]
fn test_fixture_multi_play_playbook() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ansible_compat/multi_play.yml");

    if fixture_path.exists() {
        let content = std::fs::read_to_string(&fixture_path).unwrap();
        let playbook = Playbook::from_yaml(&content, Some(fixture_path));
        assert!(
            playbook.is_ok(),
            "Failed to parse multi_play.yml: {:?}",
            playbook.err()
        );

        let pb = playbook.unwrap();
        assert_eq!(pb.plays.len(), 3);

        // First play
        assert_eq!(pb.plays[0].name, "Configure webservers");
        assert_eq!(pb.plays[0].hosts, "webservers");
        assert_eq!(pb.plays[0].r#become, Some(true));
        assert!(!pb.plays[0].handlers.is_empty());

        // Second play
        assert_eq!(pb.plays[1].name, "Configure database servers");
        assert_eq!(pb.plays[1].hosts, "databases");

        // Third play
        assert_eq!(pb.plays[2].name, "Run global cleanup");
        assert_eq!(pb.plays[2].hosts, "all");
        assert!(!pb.plays[2].gather_facts);
    }
}

// ============================================================================
// 15. Fixture File Tests - Roles Playbook
// ============================================================================

#[test]
fn test_fixture_with_roles_playbook() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ansible_compat/with_roles.yml");

    if fixture_path.exists() {
        let content = std::fs::read_to_string(&fixture_path).unwrap();
        let playbook = Playbook::from_yaml(&content, Some(fixture_path));
        assert!(
            playbook.is_ok(),
            "Failed to parse with_roles.yml: {:?}",
            playbook.err()
        );

        let pb = playbook.unwrap();
        assert_eq!(pb.plays.len(), 2);

        // First play with multiple role types
        assert!(pb.plays[0].roles.len() >= 4);

        // Check role names
        let role_names: Vec<&str> = pb.plays[0].roles.iter().map(|r| r.name()).collect();
        assert!(role_names.contains(&"common"));
        assert!(role_names.contains(&"nginx"));
        assert!(role_names.contains(&"docker"));
        assert!(role_names.contains(&"monitoring"));
    }
}

// ============================================================================
// 16. Fixture File Tests - Handlers Playbook
// ============================================================================

#[test]
fn test_fixture_with_handlers_playbook() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ansible_compat/with_handlers.yml");

    if fixture_path.exists() {
        let content = std::fs::read_to_string(&fixture_path).unwrap();
        let playbook = Playbook::from_yaml(&content, Some(fixture_path));
        assert!(
            playbook.is_ok(),
            "Failed to parse with_handlers.yml: {:?}",
            playbook.err()
        );

        let pb = playbook.unwrap();
        assert_eq!(pb.plays.len(), 1);

        // Check handlers with listen
        let handlers_with_listen: Vec<_> = pb.plays[0]
            .handlers
            .iter()
            .filter(|h| !h.listen.is_empty())
            .collect();
        assert!(handlers_with_listen.len() >= 3);

        // Check tasks have notify
        let tasks_with_notify: Vec<_> = pb.plays[0]
            .tasks
            .iter()
            .filter(|t| !t.notify.is_empty())
            .collect();
        assert!(tasks_with_notify.len() >= 4);
    }
}

// ============================================================================
// 17. Fixture File Tests - Includes Playbook
// ============================================================================

#[test]
fn test_fixture_with_includes_playbook() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ansible_compat/with_includes.yml");

    if fixture_path.exists() {
        let content = std::fs::read_to_string(&fixture_path).unwrap();
        let playbook = Playbook::from_yaml(&content, Some(fixture_path));
        assert!(
            playbook.is_ok(),
            "Failed to parse with_includes.yml: {:?}",
            playbook.err()
        );

        let pb = playbook.unwrap();
        assert_eq!(pb.plays.len(), 1);

        // Check vars_files
        assert!(pb.plays[0].vars_files.len() >= 2);

        // Check pre_tasks
        assert!(!pb.plays[0].pre_tasks.is_empty());

        // Check post_tasks
        assert!(!pb.plays[0].post_tasks.is_empty());
    }
}

// ============================================================================
// 18. Fixture File Tests - YAML Inventory
// ============================================================================

#[test]
fn test_fixture_yaml_inventory() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ansible_compat/inventory.yml");

    if fixture_path.exists() {
        let inventory = Inventory::load(&fixture_path);
        assert!(
            inventory.is_ok(),
            "Failed to load inventory.yml: {:?}",
            inventory.err()
        );

        let inv = inventory.unwrap();

        // Check hosts exist
        assert!(inv.get_host("web1").is_some());
        assert!(inv.get_host("web2").is_some());
        assert!(inv.get_host("web3").is_some());
        assert!(inv.get_host("db1").is_some());
        assert!(inv.get_host("db2").is_some());
        assert!(inv.get_host("app1").is_some());

        // Check groups exist
        assert!(inv.get_group("webservers").is_some());
        assert!(inv.get_group("databases").is_some());
        assert!(inv.get_group("appservers").is_some());
        assert!(inv.get_group("production").is_some());

        // Check host variables
        let web1 = inv.get_host("web1").unwrap();
        assert_eq!(web1.ansible_host.as_deref(), Some("192.168.1.10"));

        // Check group has hosts
        let webservers = inv.get_group("webservers").unwrap();
        assert!(webservers.has_host("web1"));
        assert!(webservers.has_host("web2"));
        assert!(webservers.has_host("web3"));

        // Check group variables
        assert!(webservers.has_var("nginx_version"));
    }
}

// ============================================================================
// 19. Fixture File Tests - INI Inventory
// ============================================================================

#[test]
fn test_fixture_ini_inventory() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ansible_compat/inventory.ini");

    if fixture_path.exists() {
        let inventory = Inventory::load(&fixture_path);
        assert!(
            inventory.is_ok(),
            "Failed to load inventory.ini: {:?}",
            inventory.err()
        );

        let inv = inventory.unwrap();

        // Check hosts exist
        assert!(inv.get_host("web1").is_some());
        assert!(inv.get_host("db1").is_some());
        assert!(inv.get_host("app1").is_some());
        assert!(inv.get_host("staging1").is_some());

        // Check groups exist
        assert!(inv.get_group("webservers").is_some());
        assert!(inv.get_group("databases").is_some());
        assert!(inv.get_group("production").is_some());

        // Check nested groups (children)
        let production = inv.get_group("production").unwrap();
        assert!(production.children.contains("webservers"));
        assert!(production.children.contains("databases"));

        // Check group variables from :vars section
        let databases = inv.get_group("databases").unwrap();
        assert!(databases.has_var("db_engine"));
    }
}

// ============================================================================
// 20. Fixture File Tests - Variable Features Playbook
// ============================================================================

#[test]
fn test_fixture_variable_features_playbook() {
    let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ansible_compat/variable_features.yml");

    if fixture_path.exists() {
        let content = std::fs::read_to_string(&fixture_path).unwrap();
        let playbook = Playbook::from_yaml(&content, Some(fixture_path));
        assert!(
            playbook.is_ok(),
            "Failed to parse variable_features.yml: {:?}",
            playbook.err()
        );

        let pb = playbook.unwrap();
        let vars = pb.plays[0].vars.as_map();

        // Test simple types
        assert!(vars.contains_key("string_var"));
        assert!(vars.contains_key("integer_var"));
        assert!(vars.contains_key("boolean_true"));

        // Test list type
        let simple_list = vars.get("simple_list").unwrap();
        assert!(simple_list.is_array());

        // Test dict type
        let simple_dict = vars.get("simple_dict").unwrap();
        assert!(simple_dict.is_object());

        // Test nested dict
        let nested_dict = vars.get("nested_dict").unwrap();
        assert!(nested_dict.is_object());
        assert!(nested_dict.get("level1").is_some());

        // Test complex structure (users list)
        let users = vars.get("users").unwrap();
        assert!(users.is_array());
        assert_eq!(users.as_array().unwrap().len(), 3);
    }
}

// ============================================================================
// 21. Module Compatibility Tests
// ============================================================================

#[test]
fn test_command_module_syntax() {
    let yaml = r#"
- name: Command module tests
  hosts: all
  tasks:
    # Simple command string
    - name: Simple echo
      command: echo hello

    # Command with chdir
    - name: Command with chdir
      command: pwd
      args:
        chdir: /tmp

    # Command with creates
    - name: Command with creates
      command: touch /tmp/testfile
      args:
        creates: /tmp/testfile

    # Command with removes
    - name: Command with removes
      command: rm /tmp/testfile
      args:
        removes: /tmp/testfile
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 4);

    for task in &pb.plays[0].tasks {
        assert_eq!(task.module_name(), "command");
    }
}

#[test]
fn test_shell_module_syntax() {
    let yaml = r#"
- name: Shell module tests
  hosts: all
  tasks:
    # Shell with pipes
    - name: Shell with pipe
      shell: cat /etc/passwd | grep root

    # Shell with redirection
    - name: Shell with redirection
      shell: echo "test" > /tmp/output.txt

    # Shell with environment
    - name: Shell with env
      shell: echo $MY_VAR
      environment:
        MY_VAR: "test_value"

    # Multi-line shell
    - name: Multi-line shell
      shell: |
        echo "line 1"
        echo "line 2"
        echo "line 3"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 4);
}

#[test]
fn test_copy_module_syntax() {
    let yaml = r#"
- name: Copy module tests
  hosts: all
  tasks:
    # Copy from src
    - name: Copy file
      copy:
        src: files/config.txt
        dest: /etc/myapp/config.txt
        owner: root
        group: root
        mode: "0644"

    # Copy with content
    - name: Copy content
      copy:
        content: |
          line 1
          line 2
        dest: /tmp/content.txt

    # Copy with backup
    - name: Copy with backup
      copy:
        src: files/nginx.conf
        dest: /etc/nginx/nginx.conf
        backup: yes

    # Copy directory
    - name: Copy directory
      copy:
        src: files/app/
        dest: /opt/app/
        directory_mode: "0755"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 4);
}

#[test]
fn test_template_module_syntax() {
    let yaml = r#"
- name: Template module tests
  hosts: all
  tasks:
    # Basic template
    - name: Deploy template
      template:
        src: templates/config.j2
        dest: /etc/myapp/config.conf
        mode: "0644"

    # Template with variables
    - name: Template with vars
      template:
        src: templates/nginx.conf.j2
        dest: /etc/nginx/nginx.conf
      vars:
        server_name: "{{ inventory_hostname }}"
        listen_port: 80

    # Template with validation
    - name: Template with validate
      template:
        src: templates/sshd.j2
        dest: /etc/ssh/sshd_config
        validate: "/usr/sbin/sshd -t -f %s"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 3);
}

#[test]
fn test_package_module_syntax() {
    let yaml = r#"
- name: Package module tests
  hosts: all
  become: true
  tasks:
    # Single package present
    - name: Install package
      package:
        name: nginx
        state: present

    # Multiple packages
    - name: Install multiple
      package:
        name:
          - nginx
          - postgresql
          - redis
        state: present

    # Package absent
    - name: Remove package
      package:
        name: vim
        state: absent

    # Package latest
    - name: Update package
      package:
        name: curl
        state: latest

    # With update_cache
    - name: Install with cache update
      package:
        name: htop
        state: present
        update_cache: yes
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 5);
}

#[test]
fn test_service_module_syntax() {
    let yaml = r#"
- name: Service module tests
  hosts: all
  become: true
  tasks:
    # Start service
    - name: Start nginx
      service:
        name: nginx
        state: started

    # Enable and start
    - name: Enable and start
      service:
        name: postgresql
        state: started
        enabled: yes

    # Stop service
    - name: Stop service
      service:
        name: apache2
        state: stopped

    # Restart service
    - name: Restart service
      service:
        name: redis
        state: restarted

    # Reload service
    - name: Reload service
      service:
        name: nginx
        state: reloaded

    # Daemon reload (systemd)
    - name: Daemon reload
      service:
        name: myapp
        state: restarted
        daemon_reload: yes
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 6);
}

// ============================================================================
// 22. Block/Rescue/Always Syntax (Parsing Only)
// ============================================================================

#[test]
fn test_block_rescue_always_syntax() {
    // Note: block/rescue/always is parsed but the fields are not yet exposed
    // on the Task struct. This test validates the YAML parses successfully.
    let yaml = r#"
- name: Block tests
  hosts: all
  tasks:
    - name: Database migration
      block:
        - name: Create backup
          command: pg_dump -f /tmp/backup.sql

        - name: Run migration
          command: ./migrate.sh

        - name: Verify migration
          command: ./verify.sh

      rescue:
        - name: Restore from backup
          command: psql -f /tmp/backup.sql

        - name: Alert failure
          debug:
            msg: "Migration failed, restored from backup"

      always:
        - name: Remove backup
          file:
            path: /tmp/backup.sql
            state: absent

        - name: Log completion
          debug:
            msg: "Migration process completed"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    // Block syntax should parse without error (even if block fields aren't exposed)
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    // The block task should be parsed (name will be "Database migration")
    assert!(!pb.plays[0].tasks.is_empty());
}

// ============================================================================
// 23. Retry/Until Syntax
// ============================================================================

#[test]
fn test_retry_until_syntax() {
    let yaml = r#"
- name: Retry tests
  hosts: all
  tasks:
    - name: Wait for service
      uri:
        url: http://localhost:8080/health
        status_code: 200
      register: result
      until: result.status == 200
      retries: 10
      delay: 5

    - name: Wait for port
      wait_for:
        port: 8080
        timeout: 60
      retries: 3
      delay: 10
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];

    assert_eq!(task.retries, Some(10));
    assert_eq!(task.delay, Some(5));
    assert!(task.until.is_some());
}

// ============================================================================
// 24. Async/Poll Syntax
// ============================================================================

#[test]
fn test_async_poll_syntax() {
    let yaml = r#"
- name: Async tests
  hosts: all
  tasks:
    - name: Long running task
      command: /usr/local/bin/long-process.sh
      async: 3600
      poll: 10

    - name: Fire and forget
      command: /usr/local/bin/background-task.sh
      async: 3600
      poll: 0
      register: background_job

    - name: Check job status
      async_status:
        jid: "{{ background_job.ansible_job_id }}"
      register: job_result
      until: job_result.finished
      retries: 30
      delay: 60
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];

    assert_eq!(task.async_, Some(3600));
    assert_eq!(task.poll, Some(10));
}

// ============================================================================
// 25. Inventory Pattern Matching Tests
// ============================================================================

#[test]
fn test_inventory_complex_patterns() {
    let ini = r#"
[webservers]
web-prod-1
web-prod-2
web-staging-1

[databases]
db-prod-1
db-staging-1

[production:children]
webservers

[staging]
web-staging-1
db-staging-1
"#;

    let temp_dir = std::env::temp_dir();
    let inventory_file = temp_dir.join("pattern_test.ini");
    std::fs::write(&inventory_file, ini).unwrap();

    let inventory = Inventory::load(&inventory_file).unwrap();

    // Test wildcard patterns
    let prod_webs = inventory.get_hosts_for_pattern("web-prod-*").unwrap();
    assert_eq!(prod_webs.len(), 2);

    // Test group pattern
    let all_webs = inventory.get_hosts_for_pattern("webservers").unwrap();
    assert_eq!(all_webs.len(), 3);

    // Test all pattern
    let all_hosts = inventory.get_hosts_for_pattern("all").unwrap();
    assert_eq!(all_hosts.len(), 5);

    std::fs::remove_file(&inventory_file).ok();
}

// ============================================================================
// 26. Execution Semantics Tests
// ============================================================================

#[test]
fn test_task_ordering_preserved() {
    let yaml = r#"
- name: Task ordering test
  hosts: all
  tasks:
    - name: Task 1
      debug:
        msg: "First"

    - name: Task 2
      debug:
        msg: "Second"

    - name: Task 3
      debug:
        msg: "Third"

    - name: Task 4
      debug:
        msg: "Fourth"

    - name: Task 5
      debug:
        msg: "Fifth"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 5);

    // Verify task ordering is preserved
    assert_eq!(pb.plays[0].tasks[0].name, "Task 1");
    assert_eq!(pb.plays[0].tasks[1].name, "Task 2");
    assert_eq!(pb.plays[0].tasks[2].name, "Task 3");
    assert_eq!(pb.plays[0].tasks[3].name, "Task 4");
    assert_eq!(pb.plays[0].tasks[4].name, "Task 5");
}

#[test]
fn test_pre_tasks_tasks_post_tasks_ordering() {
    let yaml = r#"
- name: Task phase ordering
  hosts: all
  pre_tasks:
    - name: Pre task 1
      debug:
        msg: "Pre 1"
    - name: Pre task 2
      debug:
        msg: "Pre 2"
  tasks:
    - name: Main task 1
      debug:
        msg: "Main 1"
    - name: Main task 2
      debug:
        msg: "Main 2"
  post_tasks:
    - name: Post task 1
      debug:
        msg: "Post 1"
    - name: Post task 2
      debug:
        msg: "Post 2"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let play = &pb.plays[0];

    assert_eq!(play.pre_tasks.len(), 2);
    assert_eq!(play.tasks.len(), 2);
    assert_eq!(play.post_tasks.len(), 2);

    // Check all_tasks iterator ordering
    let all_tasks: Vec<_> = play.all_tasks().collect();
    assert_eq!(all_tasks.len(), 6);
    assert_eq!(all_tasks[0].name, "Pre task 1");
    assert_eq!(all_tasks[1].name, "Pre task 2");
    assert_eq!(all_tasks[2].name, "Main task 1");
    assert_eq!(all_tasks[3].name, "Main task 2");
    assert_eq!(all_tasks[4].name, "Post task 1");
    assert_eq!(all_tasks[5].name, "Post task 2");
}

#[test]
fn test_loop_variable_naming() {
    let yaml = r#"
- name: Loop variable test
  hosts: all
  tasks:
    # Default loop variable
    - name: Default item
      debug:
        msg: "{{ item }}"
      loop:
        - a
        - b

    # Custom loop variable
    - name: Custom loop var
      debug:
        msg: "{{ pkg }}"
      loop:
        - nginx
        - redis
      loop_control:
        loop_var: pkg

    # With index
    - name: With index
      debug:
        msg: "{{ idx }}: {{ pkg }}"
      loop:
        - a
        - b
      loop_control:
        loop_var: pkg
        index_var: idx
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();

    // Check default loop variable
    assert!(pb.plays[0].tasks[0].loop_.is_some());
    assert!(pb.plays[0].tasks[0].loop_control.is_none());

    // Check custom loop variable
    assert!(pb.plays[0].tasks[1].loop_control.is_some());
    assert_eq!(
        pb.plays[0].tasks[1].loop_control.as_ref().unwrap().loop_var,
        "pkg"
    );

    // Check index variable
    assert!(pb.plays[0].tasks[2].loop_control.is_some());
    assert_eq!(
        pb.plays[0].tasks[2]
            .loop_control
            .as_ref()
            .unwrap()
            .index_var,
        Some("idx".to_string())
    );
}

// ============================================================================
// 27. Edge Cases and Error Handling
// ============================================================================

#[test]
fn test_empty_tasks_list() {
    let yaml = r#"
- name: Empty tasks
  hosts: all
  tasks: []
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 0);
}

#[test]
fn test_special_characters_in_names() {
    let yaml = r#"
- name: "Play with 'quotes' and \"double quotes\""
  hosts: all
  tasks:
    - name: "Task with special chars: <>&"
      debug:
        msg: "Message with {{ variable }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_boolean_variations() {
    let yaml = r#"
- name: Boolean variations
  hosts: all
  gather_facts: yes
  become: True
  tasks:
    - name: Test booleans
      debug:
        msg: "Test"
      ignore_errors: true
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].gather_facts);
}

#[test]
fn test_module_defaults() {
    let yaml = r#"
- name: Module defaults
  hosts: all
  module_defaults:
    yum:
      state: present
    copy:
      mode: "0644"
      owner: root
  tasks:
    - name: Install package
      yum:
        name: nginx
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(!pb.plays[0].module_defaults.is_empty());
}

#[test]
fn test_force_handlers() {
    let yaml = r#"
- name: Force handlers
  hosts: all
  force_handlers: true
  tasks:
    - name: Failing task
      command: /bin/false
      ignore_errors: true
      notify: handler

  handlers:
    - name: handler
      debug:
        msg: "Handler runs even after failure"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].force_handlers);
}

// ============================================================================
// 28. Variable Precedence Tests (Ansible Compatibility)
// ============================================================================
//
// These tests verify that Rustible follows Ansible's variable precedence
// rules (from lowest to highest priority):
// 1. role defaults
// 2. inventory group vars
// 3. playbook group_vars/all
// 4. playbook group_vars/*
// 5. inventory host vars
// 6. playbook host_vars/*
// 7. host facts
// 8. play vars
// 9. play vars_files
// 10. role vars
// 11. block vars
// 12. task vars
// 13. include vars
// 14. set_facts
// 15. role params
// 16. include params
// 17. extra vars (-e)

#[test]
fn test_var_precedence_levels_defined() {
    // Verify that VarPrecedence enum has all 20 levels as documented
    use rustible::vars::VarPrecedence;

    // Test all precedence levels are accessible
    let levels: Vec<VarPrecedence> = VarPrecedence::all().collect();
    assert_eq!(levels.len(), 20, "Should have 20 precedence levels");

    // Verify ordering (lower number = lower priority)
    assert!(VarPrecedence::RoleDefaults.level() < VarPrecedence::ExtraVars.level());
    assert!(VarPrecedence::PlayVars.level() < VarPrecedence::ExtraVars.level());
    assert!(VarPrecedence::SetFacts.level() < VarPrecedence::ExtraVars.level());
    assert!(VarPrecedence::TaskVars.level() < VarPrecedence::SetFacts.level());
    assert!(VarPrecedence::BlockVars.level() < VarPrecedence::TaskVars.level());
}

#[test]
fn test_var_precedence_extra_vars_highest() {
    // Extra vars (-e) should always have highest precedence
    use rustible::vars::{VarPrecedence, VarStore};
    use serde_yaml::Value;

    let mut store = VarStore::new();

    // Set at different precedence levels
    store.set(
        "myvar",
        Value::String("role_default".to_string()),
        VarPrecedence::RoleDefaults,
    );
    store.set(
        "myvar",
        Value::String("play_var".to_string()),
        VarPrecedence::PlayVars,
    );
    store.set(
        "myvar",
        Value::String("set_fact".to_string()),
        VarPrecedence::SetFacts,
    );
    store.set(
        "myvar",
        Value::String("extra_var".to_string()),
        VarPrecedence::ExtraVars,
    );

    // Extra vars should win
    let result = store.get("myvar");
    assert_eq!(result, Some(&Value::String("extra_var".to_string())));
}

#[test]
fn test_var_precedence_set_fact_overrides_play_vars() {
    use rustible::vars::{VarPrecedence, VarStore};
    use serde_yaml::Value;

    let mut store = VarStore::new();

    store.set(
        "myvar",
        Value::String("play_var".to_string()),
        VarPrecedence::PlayVars,
    );
    store.set(
        "myvar",
        Value::String("set_fact_value".to_string()),
        VarPrecedence::SetFacts,
    );

    let result = store.get("myvar");
    assert_eq!(result, Some(&Value::String("set_fact_value".to_string())));
}

#[test]
fn test_var_precedence_task_vars_override_block_vars() {
    use rustible::vars::{VarPrecedence, VarStore};
    use serde_yaml::Value;

    let mut store = VarStore::new();

    store.set(
        "myvar",
        Value::String("block_var".to_string()),
        VarPrecedence::BlockVars,
    );
    store.set(
        "myvar",
        Value::String("task_var".to_string()),
        VarPrecedence::TaskVars,
    );

    let result = store.get("myvar");
    assert_eq!(result, Some(&Value::String("task_var".to_string())));
}

#[test]
fn test_var_precedence_role_vars_override_role_defaults() {
    use rustible::vars::{VarPrecedence, VarStore};
    use serde_yaml::Value;

    let mut store = VarStore::new();

    store.set(
        "myvar",
        Value::String("role_default".to_string()),
        VarPrecedence::RoleDefaults,
    );
    store.set(
        "myvar",
        Value::String("role_var".to_string()),
        VarPrecedence::RoleVars,
    );

    let result = store.get("myvar");
    assert_eq!(result, Some(&Value::String("role_var".to_string())));
}

#[test]
fn test_var_precedence_inventory_host_vars_override_group_vars() {
    use rustible::vars::{VarPrecedence, VarStore};
    use serde_yaml::Value;

    let mut store = VarStore::new();

    store.set(
        "myvar",
        Value::String("group_var".to_string()),
        VarPrecedence::InventoryGroupVars,
    );
    store.set(
        "myvar",
        Value::String("host_var".to_string()),
        VarPrecedence::InventoryHostVars,
    );

    let result = store.get("myvar");
    assert_eq!(result, Some(&Value::String("host_var".to_string())));
}

#[test]
fn test_var_precedence_display_names() {
    use rustible::vars::VarPrecedence;

    // Verify display names match Ansible terminology
    assert_eq!(format!("{}", VarPrecedence::RoleDefaults), "role defaults");
    assert_eq!(format!("{}", VarPrecedence::ExtraVars), "extra vars");
    assert_eq!(format!("{}", VarPrecedence::PlayVars), "play vars");
    assert_eq!(format!("{}", VarPrecedence::SetFacts), "set_facts");
    assert_eq!(format!("{}", VarPrecedence::TaskVars), "task vars");
    assert_eq!(format!("{}", VarPrecedence::BlockVars), "block vars");
}

// ============================================================================
// 29. Conditional Evaluation Tests (Ansible when: Compatibility)
// ============================================================================

#[test]
fn test_when_condition_is_defined_syntax() {
    let yaml = r#"
- name: Test is defined
  hosts: all
  vars:
    defined_var: "value"
  tasks:
    - name: Run if defined
      debug:
        msg: "Variable is defined"
      when: defined_var is defined

    - name: Skip if not defined
      debug:
        msg: "This should run"
      when: undefined_var is not defined
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 2);

    // Check the when conditions are parsed correctly
    if let Some(When::Single(cond)) = &pb.plays[0].tasks[0].when {
        assert!(cond.contains("is defined"));
    } else {
        panic!("Expected single when condition");
    }
}

#[test]
fn test_when_condition_comparison_operators() {
    let yaml = r#"
- name: Test comparisons
  hosts: all
  vars:
    number_var: 42
    string_var: "hello"
  tasks:
    - name: Test greater than
      debug:
        msg: "number > 40"
      when: number_var > 40

    - name: Test less than
      debug:
        msg: "number < 50"
      when: number_var < 50

    - name: Test equals
      debug:
        msg: "string equals hello"
      when: string_var == "hello"

    - name: Test not equals
      debug:
        msg: "number not 0"
      when: number_var != 0

    - name: Test greater or equal
      debug:
        msg: "number >= 42"
      when: number_var >= 42

    - name: Test less or equal
      debug:
        msg: "number <= 42"
      when: number_var <= 42
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 6);
}

#[test]
fn test_when_condition_boolean_operators() {
    let yaml = r#"
- name: Test boolean operators
  hosts: all
  vars:
    var_a: true
    var_b: false
    os_family: "Debian"
    version: 20
  tasks:
    - name: Test and condition
      debug:
        msg: "Both true"
      when: var_a and os_family == "Debian"

    - name: Test or condition
      debug:
        msg: "One is true"
      when: var_a or var_b

    - name: Test not condition
      debug:
        msg: "Not false"
      when: not var_b

    - name: Test complex condition
      debug:
        msg: "Complex"
      when: (var_a and not var_b) or version > 18

    - name: Test multiple conditions list (implicit and)
      debug:
        msg: "All conditions met"
      when:
        - os_family == "Debian"
        - version >= 20
        - var_a
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 5);

    // Check that multiple conditions are parsed as list (implicit AND)
    if let Some(When::Multiple(conditions)) = &pb.plays[0].tasks[4].when {
        assert_eq!(conditions.len(), 3);
        assert!(conditions[0].contains("Debian"));
        assert!(conditions[1].contains("20"));
        assert!(conditions[2].contains("var_a"));
    } else {
        panic!("Expected multiple when conditions");
    }
}

#[test]
fn test_when_condition_in_operator() {
    let yaml = r#"
- name: Test in operator
  hosts: all
  vars:
    my_list:
      - item1
      - item2
      - item3
    my_value: "item2"
  tasks:
    - name: Test in list
      debug:
        msg: "Value in list"
      when: my_value in my_list

    - name: Test not in list
      debug:
        msg: "Value not in list"
      when: "'item4' not in my_list"

    - name: Test string in string
      debug:
        msg: "Substring found"
      when: "'item' in my_value"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 3);
}

#[test]
fn test_when_with_registered_variable() {
    let yaml = r#"
- name: Test registered variable in condition
  hosts: all
  tasks:
    - name: Run command
      command: echo "test"
      register: cmd_result

    - name: Check result success
      debug:
        msg: "Command succeeded"
      when: cmd_result.rc == 0

    - name: Check result changed
      debug:
        msg: "Command changed something"
      when: cmd_result.changed

    - name: Check stdout
      debug:
        msg: "Expected output found"
      when: "'test' in cmd_result.stdout"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 4);
    assert!(pb.plays[0].tasks[0].register.is_some());
    assert_eq!(
        pb.plays[0].tasks[0].register,
        Some("cmd_result".to_string())
    );
}

// ============================================================================
// 30. Loop Behavior Tests (Ansible loop: Compatibility)
// ============================================================================

#[test]
fn test_loop_basic_list() {
    let yaml = r#"
- name: Test basic loop
  hosts: all
  tasks:
    - name: Loop over items
      debug:
        msg: "Item: {{ item }}"
      loop:
        - first
        - second
        - third
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.loop_.is_some());

    let items = task.loop_.as_ref().unwrap().as_array().unwrap();
    assert_eq!(items.len(), 3);
}

#[test]
fn test_with_items_legacy_syntax() {
    let yaml = r#"
- name: Test with_items (legacy)
  hosts: all
  tasks:
    - name: Loop with with_items
      debug:
        msg: "Item: {{ item }}"
      with_items:
        - alpha
        - beta
        - gamma
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.with_items.is_some());
}

#[test]
fn test_loop_with_dict_items() {
    let yaml = r#"
- name: Test loop with dicts
  hosts: all
  tasks:
    - name: Create users
      user:
        name: "{{ item.name }}"
        uid: "{{ item.uid }}"
        groups: "{{ item.groups }}"
      loop:
        - name: alice
          uid: 1001
          groups:
            - wheel
            - developers
        - name: bob
          uid: 1002
          groups:
            - developers
        - name: charlie
          uid: 1003
          groups:
            - users
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    let items = task.loop_.as_ref().unwrap().as_array().unwrap();
    assert_eq!(items.len(), 3);

    // Verify dict structure is preserved
    assert!(items[0].is_object());
    assert!(items[0].get("name").is_some());
    assert!(items[0].get("uid").is_some());
}

#[test]
fn test_loop_control_full() {
    let yaml = r#"
- name: Test loop_control
  hosts: all
  tasks:
    - name: Full loop control
      debug:
        msg: "{{ idx }}: {{ pkg }} ({{ outer_item }})"
      loop:
        - nginx
        - redis
        - postgresql
      loop_control:
        loop_var: pkg
        index_var: idx
        pause: 2
        label: "Installing {{ pkg }}"
        extended: true
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];

    assert!(task.loop_control.is_some());
    let lc = task.loop_control.as_ref().unwrap();
    assert_eq!(lc.loop_var, "pkg");
    assert_eq!(lc.index_var, Some("idx".to_string()));
    assert_eq!(lc.pause, Some(2));
    assert!(lc.extended);
}

#[test]
fn test_loop_with_when_filter() {
    let yaml = r#"
- name: Test loop with when
  hosts: all
  tasks:
    - name: Selective loop
      debug:
        msg: "Processing {{ item.name }}"
      loop:
        - name: enabled_item
          enabled: true
        - name: disabled_item
          enabled: false
        - name: another_enabled
          enabled: true
      when: item.enabled
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.loop_.is_some());
    assert!(task.when.is_some());
}

#[test]
fn test_loop_with_register() {
    let yaml = r#"
- name: Test loop register
  hosts: all
  tasks:
    - name: Loop and register
      command: echo "{{ item }}"
      loop:
        - one
        - two
        - three
      register: loop_results

    - name: Use registered loop results
      debug:
        msg: "Result {{ item.item }}: {{ item.stdout }}"
      loop: "{{ loop_results.results }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(
        pb.plays[0].tasks[0].register,
        Some("loop_results".to_string())
    );
}

#[test]
fn test_loop_variable_from_playbook_var() {
    let yaml = r#"
- name: Test loop from variable
  hosts: all
  vars:
    packages_to_install:
      - nginx
      - vim
      - curl
  tasks:
    - name: Install from variable
      package:
        name: "{{ item }}"
        state: present
      loop: "{{ packages_to_install }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    // Loop referencing a variable should be stored as template string
    assert!(task.loop_.is_some());
}

// ============================================================================
// 31. Handler Notification Tests (Ansible notify/listen Compatibility)
// ============================================================================

#[test]
fn test_handler_simple_notify() {
    let yaml = r#"
- name: Test handler notify
  hosts: all
  tasks:
    - name: Update config
      copy:
        src: config.txt
        dest: /etc/app/config.txt
      notify: restart app

  handlers:
    - name: restart app
      service:
        name: myapp
        state: restarted
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks[0].notify.len(), 1);
    assert_eq!(pb.plays[0].tasks[0].notify[0], "restart app");
    assert_eq!(pb.plays[0].handlers.len(), 1);
    assert_eq!(pb.plays[0].handlers[0].name, "restart app");
}

#[test]
fn test_handler_multiple_notify_extended() {
    let yaml = r#"
- name: Test multiple notify
  hosts: all
  tasks:
    - name: Update everything
      copy:
        src: config.tar.gz
        dest: /etc/app/
      notify:
        - restart app
        - reload nginx
        - clear cache
        - update metrics

  handlers:
    - name: restart app
      service:
        name: myapp
        state: restarted

    - name: reload nginx
      service:
        name: nginx
        state: reloaded

    - name: clear cache
      command: /usr/bin/clear-cache

    - name: update metrics
      command: /usr/bin/update-metrics
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks[0].notify.len(), 4);
    assert_eq!(pb.plays[0].handlers.len(), 4);
}

#[test]
fn test_handler_listen_syntax() {
    let yaml = r#"
- name: Test handler listen
  hosts: all
  tasks:
    - name: Update app files
      copy:
        src: app.jar
        dest: /opt/app/
      notify: application updated

  handlers:
    - name: restart backend
      service:
        name: backend
        state: restarted
      listen: application updated

    - name: restart frontend
      service:
        name: frontend
        state: restarted
      listen: application updated

    - name: clear caches
      command: /usr/bin/clear-caches
      listen: application updated

    - name: send notification
      debug:
        msg: "App was updated"
      listen: application updated
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].handlers.len(), 4);

    // All handlers should listen to "application updated"
    for handler in &pb.plays[0].handlers {
        assert!(handler.listen.contains(&"application updated".to_string()));
    }
}

#[test]
fn test_handler_with_when_condition() {
    let yaml = r#"
- name: Test conditional handler
  hosts: all
  vars:
    enable_restart: true
  tasks:
    - name: Update config
      copy:
        src: config.txt
        dest: /etc/app/
      notify: maybe restart

  handlers:
    - name: maybe restart
      service:
        name: myapp
        state: restarted
      when: enable_restart
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].handlers[0].task.when.is_some());
}

#[test]
fn test_handler_flush_handlers_meta() {
    let yaml = r#"
- name: Test flush handlers
  hosts: all
  tasks:
    - name: Update config
      copy:
        src: config.txt
        dest: /etc/app/
      notify: restart app

    - name: Flush handlers now
      meta: flush_handlers

    - name: Continue after handlers
      debug:
        msg: "Handlers have run"

  handlers:
    - name: restart app
      service:
        name: myapp
        state: restarted
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 3);

    // Second task should be meta module
    assert_eq!(pb.plays[0].tasks[1].module_name(), "meta");
}

#[test]
fn test_handler_listen_multiple_names() {
    let yaml = r#"
- name: Test multiple listen names
  hosts: all
  tasks:
    - name: Update web config
      copy:
        src: web.conf
        dest: /etc/web/
      notify: web config changed

    - name: Update app config
      copy:
        src: app.conf
        dest: /etc/app/
      notify: app config changed

  handlers:
    - name: restart services
      debug:
        msg: "Restarting all services"
      listen:
        - web config changed
        - app config changed
        - any config changed
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let handler = &pb.plays[0].handlers[0];
    assert_eq!(handler.listen.len(), 3);
}

// ============================================================================
// 32. Block/Rescue/Always Tests (Ansible Error Handling Compatibility)
// ============================================================================

#[test]
fn test_block_basic_structure() {
    let yaml = r#"
- name: Test basic block
  hosts: all
  tasks:
    - name: Grouped tasks
      block:
        - name: Task 1 in block
          debug:
            msg: "First task"
        - name: Task 2 in block
          debug:
            msg: "Second task"
        - name: Task 3 in block
          debug:
            msg: "Third task"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_block_rescue_always() {
    let yaml = r#"
- name: Test block/rescue/always
  hosts: all
  tasks:
    - name: Handle potential failure
      block:
        - name: Risky operation
          command: /usr/bin/risky-command
        - name: Another risky operation
          command: /usr/bin/more-risk

      rescue:
        - name: Handle failure
          debug:
            msg: "An error occurred, recovering..."
        - name: Rollback changes
          command: /usr/bin/rollback
        - name: Send alert
          debug:
            msg: "Alert sent!"

      always:
        - name: Always cleanup
          command: /usr/bin/cleanup
        - name: Log completion
          debug:
            msg: "Block execution completed"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(
        playbook.is_ok(),
        "Block/rescue/always should parse successfully"
    );
}

#[test]
fn test_block_with_when_condition() {
    let yaml = r#"
- name: Test conditional block
  hosts: all
  vars:
    run_risky_tasks: true
  tasks:
    - name: Conditional block
      block:
        - name: Risky task 1
          command: /usr/bin/risky1
        - name: Risky task 2
          command: /usr/bin/risky2
      when: run_risky_tasks
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_block_with_become() {
    let yaml = r#"
- name: Test block with become
  hosts: all
  tasks:
    - name: Privileged block
      block:
        - name: Install package
          package:
            name: nginx
            state: present
        - name: Configure service
          copy:
            src: nginx.conf
            dest: /etc/nginx/
      become: true
      become_user: root
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_nested_blocks() {
    let yaml = r#"
- name: Test nested blocks
  hosts: all
  tasks:
    - name: Outer block
      block:
        - name: Inner block 1
          block:
            - name: Deep task 1
              debug:
                msg: "Deep 1"
          rescue:
            - name: Handle inner failure
              debug:
                msg: "Inner rescue"

        - name: After inner block
          debug:
            msg: "After inner"

      rescue:
        - name: Outer rescue
          debug:
            msg: "Outer rescue"

      always:
        - name: Outer always
          debug:
            msg: "Outer always"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_block_with_notify() {
    let yaml = r#"
- name: Test block with notify
  hosts: all
  tasks:
    - name: Config block
      block:
        - name: Update config
          copy:
            src: config.txt
            dest: /etc/app/
      notify: restart services

  handlers:
    - name: restart services
      debug:
        msg: "Restarting..."
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

// ============================================================================
// 33. Role Loading Tests (Ansible Roles Compatibility)
// ============================================================================

#[test]
fn test_role_simple_string() {
    let yaml = r#"
- name: Test simple role reference
  hosts: all
  roles:
    - common
    - webserver
    - database
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 3);
    assert_eq!(pb.plays[0].roles[0].name(), "common");
    assert_eq!(pb.plays[0].roles[1].name(), "webserver");
    assert_eq!(pb.plays[0].roles[2].name(), "database");
}

#[test]
fn test_role_with_vars_extended() {
    let yaml = r#"
- name: Test role with variables
  hosts: all
  roles:
    - role: nginx
      vars:
        nginx_port: 8080
        nginx_workers: 4
        nginx_user: www-data
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 1);
    assert_eq!(pb.plays[0].roles[0].name(), "nginx");
}

#[test]
fn test_role_with_when_extended() {
    let yaml = r#"
- name: Test conditional role
  hosts: all
  vars:
    install_docker: true
  roles:
    - role: docker
      when: install_docker
    - role: kubernetes
      when: install_docker and use_k8s is defined
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 2);
}

#[test]
fn test_role_with_tags_extended() {
    let yaml = r#"
- name: Test role with tags
  hosts: all
  roles:
    - role: nginx
      tags:
        - webserver
        - proxy
    - role: postgresql
      tags:
        - database
        - postgres
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 2);
}

#[test]
fn test_role_with_become() {
    let yaml = r#"
- name: Test role with become
  hosts: all
  roles:
    - role: system-update
      become: true
      become_user: root
    - role: app-deploy
      become: true
      become_user: deploy
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 2);
}

#[test]
fn test_role_mixed_format() {
    let yaml = r#"
- name: Test mixed role formats
  hosts: all
  roles:
    # Simple string reference
    - common

    # Role key format with vars
    - role: nginx
      vars:
        port: 80

    # Role key format
    - role: postgresql
      vars:
        db_name: myapp

    # With all options
    - role: app
      vars:
        app_version: "1.0.0"
      when: deploy_app | default(true)
      tags:
        - application
        - deploy
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].roles.len(), 4);
}

#[test]
fn test_role_delegate_to() {
    let yaml = r#"
- name: Test role delegation
  hosts: webservers
  roles:
    - role: loadbalancer-config
      delegate_to: lb_host
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

// ============================================================================
// 34. Serial Execution Tests
// ============================================================================

#[test]
fn test_serial_single_value() {
    let yaml = r#"
- name: Test serial single value
  hosts: all
  serial: 2
  tasks:
    - name: Rolling update
      package:
        name: nginx
        state: latest
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert!(pb.plays[0].serial.is_some());
}

#[test]
fn test_serial_percentage() {
    let yaml = r#"
- name: Test serial percentage
  hosts: all
  serial: "25%"
  tasks:
    - name: Gradual rollout
      command: /deploy.sh
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_serial_list() {
    let yaml = r#"
- name: Test serial list
  hosts: all
  serial:
    - 1
    - 5
    - 10
    - "25%"
  tasks:
    - name: Staged rollout
      command: /deploy.sh
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

// ============================================================================
// 35. Include/Import Tests (Enhanced)
// ============================================================================

#[test]
fn test_include_tasks_with_vars() {
    let yaml = r#"
- name: Test include_tasks with vars
  hosts: all
  tasks:
    - name: Include with variables
      include_tasks: tasks/configure.yml
      vars:
        config_file: /etc/app/config.yml
        restart_service: true
        settings:
          debug: true
          log_level: info
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    let task = &pb.plays[0].tasks[0];
    assert!(task.vars.as_map().contains_key("config_file"));
}

#[test]
fn test_import_tasks_static() {
    let yaml = r#"
- name: Test import_tasks (static)
  hosts: all
  tasks:
    - name: Import common tasks
      import_tasks: tasks/common.yml

    - name: Import with tags
      import_tasks: tasks/web.yml
      tags:
        - webserver
        - deploy
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 2);
}

#[test]
fn test_include_tasks_loop() {
    let yaml = r#"
- name: Test include_tasks in loop
  hosts: all
  tasks:
    - name: Include for each environment
      include_tasks: "tasks/{{ item }}.yml"
      loop:
        - development
        - staging
        - production
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_include_vars_module() {
    let yaml = r#"
- name: Test include_vars
  hosts: all
  tasks:
    - name: Include simple vars file
      include_vars: vars/common.yml

    - name: Include vars with namespace
      include_vars:
        file: vars/database.yml
        name: db_config

    - name: Include vars from directory
      include_vars:
        dir: vars/services/
        extensions:
          - yml
          - yaml
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks.len(), 3);
}

// ============================================================================
// 36. Delegation Tests (Enhanced)
// ============================================================================

#[test]
fn test_delegate_to_localhost() {
    let yaml = r#"
- name: Test delegate to localhost
  hosts: webservers
  tasks:
    - name: Run locally
      command: echo "Running on controller"
      delegate_to: localhost
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(
        pb.plays[0].tasks[0].delegate_to,
        Some("localhost".to_string())
    );
}

#[test]
fn test_delegate_facts() {
    let yaml = r#"
- name: Test delegate_facts
  hosts: webservers
  tasks:
    - name: Gather facts from database
      setup:
      delegate_to: "{{ groups['databases'][0] }}"
      delegate_facts: true
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks[0].delegate_facts, Some(true));
}

// ============================================================================
// 37. Strategy Tests
// ============================================================================

#[test]
fn test_strategy_linear() {
    let yaml = r#"
- name: Test linear strategy
  hosts: all
  strategy: linear
  tasks:
    - name: Task in linear mode
      debug:
        msg: "Running linearly"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].strategy, Some("linear".to_string()));
}

#[test]
fn test_strategy_free() {
    let yaml = r#"
- name: Test free strategy
  hosts: all
  strategy: free
  tasks:
    - name: Task in free mode
      debug:
        msg: "Running freely"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].strategy, Some("free".to_string()));
}

// ============================================================================
// 38. Jinja2 Filter Compatibility Tests
// ============================================================================

#[test]
fn test_jinja2_default_filter_syntax() {
    let yaml = r#"
- name: Test default filter
  hosts: all
  tasks:
    - name: Use default for undefined
      debug:
        msg: "Value: {{ undefined_var | default('fallback') }}"

    - name: Use default with empty check
      debug:
        msg: "Value: {{ empty_var | default('fallback', true) }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_jinja2_string_filters_syntax() {
    let yaml = r#"
- name: Test string filters
  hosts: all
  vars:
    my_string: "  Hello World  "
  tasks:
    - name: Upper filter
      debug:
        msg: "{{ my_string | upper }}"

    - name: Lower filter
      debug:
        msg: "{{ my_string | lower }}"

    - name: Trim filter
      debug:
        msg: "{{ my_string | trim }}"

    - name: Replace filter
      debug:
        msg: "{{ my_string | replace('World', 'Ansible') }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

#[test]
fn test_jinja2_list_filters_syntax() {
    let yaml = r#"
- name: Test list filters
  hosts: all
  vars:
    my_list:
      - one
      - two
      - three
  tasks:
    - name: First filter
      debug:
        msg: "{{ my_list | first }}"

    - name: Last filter
      debug:
        msg: "{{ my_list | last }}"

    - name: Length filter
      debug:
        msg: "{{ my_list | length }}"

    - name: Join filter
      debug:
        msg: "{{ my_list | join(', ') }}"

    - name: Sort filter
      debug:
        msg: "{{ my_list | sort }}"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());
}

// ============================================================================
// 39. Error Handling Edge Cases
// ============================================================================

#[test]
fn test_any_errors_fatal() {
    let yaml = r#"
- name: Test any_errors_fatal
  hosts: all
  any_errors_fatal: true
  tasks:
    - name: Risky task
      command: /bin/might-fail
"#;

    // Note: any_errors_fatal is parsed but handled at execution time
    // Just verify the playbook parses correctly
    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays.len(), 1);
}

#[test]
fn test_fail_module() {
    let yaml = r#"
- name: Test fail module
  hosts: all
  tasks:
    - name: Check condition
      fail:
        msg: "This task intentionally fails"
      when: some_condition
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks[0].module_name(), "fail");
}

#[test]
fn test_assert_module() {
    let yaml = r#"
- name: Test assert module
  hosts: all
  vars:
    my_version: "2.0"
  tasks:
    - name: Validate configuration
      assert:
        that:
          - my_version is defined
          - "my_version | version('1.0', '>=')"
        fail_msg: "Invalid version configuration"
        success_msg: "Configuration validated successfully"
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok());

    let pb = playbook.unwrap();
    assert_eq!(pb.plays[0].tasks[0].module_name(), "assert");
}

// ============================================================================
// 40. Complex Real-World Scenario Tests
// ============================================================================

#[test]
fn test_full_production_deploy_playbook() {
    let yaml = r#"
---
- name: Pre-deployment checks
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Check deployment prerequisites
      assert:
        that:
          - deploy_version is defined
          - deploy_env in ['staging', 'production']
        fail_msg: "Missing required deployment variables"

- name: Deploy to webservers
  hosts: webservers
  become: true
  serial: "25%"
  max_fail_percentage: 10
  any_errors_fatal: false

  pre_tasks:
    - name: Check disk space
      command: df -h /opt
      register: disk_check
      changed_when: false

  roles:
    - role: common
      tags: [always]
    - role: app-deploy
      vars:
        app_version: "{{ deploy_version }}"
      tags: [deploy]

  tasks:
    - name: Deploy application
      block:
        - name: Stop old version
          service:
            name: myapp
            state: stopped

        - name: Copy new version
          copy:
            src: "app-{{ deploy_version }}.tar.gz"
            dest: /opt/myapp/

        - name: Start new version
          service:
            name: myapp
            state: started

      rescue:
        - name: Rollback on failure
          command: /opt/myapp/rollback.sh

        - name: Send failure alert
          debug:
            msg: "Deployment failed, rollback completed"

      always:
        - name: Clean up temp files
          file:
            path: /tmp/deploy-*
            state: absent

      notify:
        - reload nginx
        - clear cache

  post_tasks:
    - name: Health check
      uri:
        url: "http://{{ inventory_hostname }}:8080/health"
        status_code: 200
      retries: 10
      delay: 5
      register: health
      until: health.status == 200

  handlers:
    - name: reload nginx
      service:
        name: nginx
        state: reloaded

    - name: clear cache
      command: /opt/myapp/clear-cache.sh
      listen: cache operations

- name: Post-deployment validation
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Run integration tests
      command: /tests/run-integration.sh
      delegate_to: test-runner
      register: test_result

    - name: Notify success
      debug:
        msg: "Deployment {{ deploy_version }} completed successfully"
      when: test_result.rc == 0
"#;

    let playbook = Playbook::from_yaml(yaml, None);
    assert!(playbook.is_ok(), "Complex production playbook should parse");

    let pb = playbook.unwrap();
    assert_eq!(pb.plays.len(), 3, "Should have 3 plays");

    // First play - pre-deployment checks
    assert_eq!(pb.plays[0].name, "Pre-deployment checks");
    assert!(!pb.plays[0].gather_facts);

    // Second play - main deployment
    assert_eq!(pb.plays[1].name, "Deploy to webservers");
    assert_eq!(pb.plays[1].r#become, Some(true));
    assert!(pb.plays[1].serial.is_some());
    assert_eq!(pb.plays[1].max_fail_percentage, Some(10));
    // Note: any_errors_fatal is handled at execution time, not stored on Play
    assert_eq!(pb.plays[1].pre_tasks.len(), 1);
    assert_eq!(pb.plays[1].roles.len(), 2);
    assert_eq!(pb.plays[1].post_tasks.len(), 1);
    assert_eq!(pb.plays[1].handlers.len(), 2);

    // Third play - post-deployment
    assert_eq!(pb.plays[2].name, "Post-deployment validation");
}
