//! Tests for --plan flag functionality

use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_plan_flag_shows_execution_plan() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    // Create a simple playbook
    fs::write(
        &playbook_path,
        r#"---
- name: Test Play
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Install nginx
      package:
        name: nginx
        state: present

    - name: Start nginx service
      service:
        name: nginx
        state: started
        enabled: true
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains("EXECUTION PLAN"))
        .stdout(predicate::str::contains(
            "Rustible will perform the following actions",
        ))
        .stdout(predicate::str::contains("Test Play"))
        .stdout(predicate::str::contains("Install nginx"))
        .stdout(predicate::str::contains("will install package: nginx"))
        .stdout(predicate::str::contains("Start nginx service"))
        .stdout(predicate::str::contains(
            "will ensure service nginx is started",
        ))
        .stdout(predicate::str::contains("PLAN SUMMARY"))
        .stdout(predicate::str::contains(
            "To execute this plan, run the same command without --plan",
        ));
}

#[test]
fn test_plan_shows_task_count() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Multi-task Play
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Task 1
      debug:
        msg: "First task"

    - name: Task 2
      debug:
        msg: "Second task"

    - name: Task 3
      debug:
        msg: "Third task"
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains("Tasks: 3 tasks"))
        .stdout(predicate::str::contains("Plan: 3 tasks across 1 host"));
}

#[test]
fn test_plan_shows_module_details() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Module Test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Copy file
      copy:
        src: /tmp/source.txt
        dest: /tmp/dest.txt

    - name: Execute command
      command: echo "Hello World"

    - name: Create directory
      file:
        path: /tmp/testdir
        state: directory
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains("Module: copy"))
        .stdout(predicate::str::contains(
            "will copy /tmp/source.txt to /tmp/dest.txt",
        ))
        .stdout(predicate::str::contains("Module: command"))
        .stdout(predicate::str::contains(
            "will execute: echo \"Hello World\"",
        ))
        .stdout(predicate::str::contains("Module: file"))
        .stdout(predicate::str::contains(
            "will ensure /tmp/testdir exists as directory",
        ));
}

#[test]
fn test_plan_shows_conditional_tasks() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Conditional Test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Task with condition
      debug:
        msg: "This has a condition"
      when: ansible_os_family == "Debian"
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "When: ansible_os_family == \"Debian\"",
        ));
}

#[test]
fn test_plan_shows_notify_handlers() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Handler Test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Copy config
      copy:
        src: config.yml
        dest: /etc/app/config.yml
      notify:
        - restart app
        - reload config

  handlers:
    - name: restart app
      service:
        name: app
        state: restarted

    - name: reload config
      command: app-reload
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Notify: restart app, reload config",
        ));
}

#[test]
fn test_plan_with_variables() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Variable Test
  hosts: localhost
  gather_facts: false
  vars:
    package_name: nginx
    service_state: started
  tasks:
    - name: Install package
      package:
        name: "{{ package_name }}"
        state: present

    - name: Manage service
      service:
        name: "{{ package_name }}"
        state: "{{ service_state }}"
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains("nginx"));
}

#[test]
fn test_plan_multiple_plays() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: First Play
  hosts: localhost
  gather_facts: false
  tasks:
    - name: First task
      debug:
        msg: "Play 1"

- name: Second Play
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Second task
      debug:
        msg: "Play 2"
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains("[Play 1/2]"))
        .stdout(predicate::str::contains("First Play"))
        .stdout(predicate::str::contains("[Play 2/2]"))
        .stdout(predicate::str::contains("Second Play"));
}

#[test]
fn test_plan_with_tags_filter() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Tagged Tasks
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Install task
      debug:
        msg: "Installing"
      tags:
        - install

    - name: Configure task
      debug:
        msg: "Configuring"
      tags:
        - configure

    - name: Deploy task
      debug:
        msg: "Deploying"
      tags:
        - deploy
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .arg("--tags")
        .arg("install")
        .assert()
        .success()
        .stdout(predicate::str::contains("Install task"))
        .stdout(predicate::str::contains("Plan: 1 task"));
}

#[test]
fn test_plan_package_modules() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Package Management
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Install with apt
      apt:
        name: vim
        state: present

    - name: Remove with yum
      yum:
        name: telnet
        state: absent

    - name: Install Python package
      pip:
        name: requests
        state: present
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains("will install package: vim"))
        .stdout(predicate::str::contains("will remove package: telnet"))
        .stdout(predicate::str::contains("will install package: requests"));
}

#[test]
fn test_plan_user_and_group_modules() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: User Management
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Create user
      user:
        name: appuser
        state: present

    - name: Create group
      group:
        name: appgroup
        state: present

    - name: Remove user
      user:
        name: olduser
        state: absent
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains("will create/update user: appuser"))
        .stdout(predicate::str::contains(
            "will create/update group: appgroup",
        ))
        .stdout(predicate::str::contains("will remove user: olduser"));
}

#[test]
fn test_plan_git_module() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Git Operations
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Clone repository
      git:
        repo: https://github.com/example/repo.git
        dest: /opt/app
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "will clone/update https://github.com/example/repo.git to /opt/app",
        ));
}

#[test]
fn test_plan_template_module() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Template Test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Render template
      template:
        src: config.j2
        dest: /etc/app/config.yml
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "will render template config.j2 to /etc/app/config.yml",
        ));
}

#[test]
fn test_plan_shows_host_info() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Test Play
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Simple task
      debug:
        msg: "test"
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains("Hosts: localhost (1 host)"))
        .stdout(predicate::str::contains("[localhost]"));
}

#[test]
fn test_plan_warning_message() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Test task
      debug:
        msg: "test"
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Running in PLAN MODE - showing execution plan only",
        ));
}

#[test]
fn test_plan_exit_code_success() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Test task
      debug:
        msg: "test"
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .assert()
        .success(); // Should exit with code 0
}

#[test]
fn test_plan_with_extra_vars() {
    let temp = TempDir::new().unwrap();
    let playbook_path = temp.path().join("test.yml");

    fs::write(
        &playbook_path,
        r#"---
- name: Variable Test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Use variable
      debug:
        msg: "{{ custom_var }}"
"#,
    )
    .unwrap();

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("rustible");
    cmd.arg("run")
        .arg(&playbook_path)
        .arg("--plan")
        .arg("-e")
        .arg("custom_var=test_value")
        .assert()
        .success()
        .stdout(predicate::str::contains("test_value"));
}
