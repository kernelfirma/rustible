//! Comprehensive integration tests for the Rustible parser system
//!
//! These tests verify Ansible-compatible YAML parsing including:
//! - Playbook structure parsing (simple and complex)
//! - Play-level attributes (name, hosts, tasks, handlers)
//! - Task parsing with all features (conditionals, loops, notify, etc.)
//! - Handler parsing and listener patterns
//! - Template detection and rendering
//! - Error handling for malformed YAML
//! - Edge cases and boundary conditions
//!
//! Note: These tests use only the public API (rustible::playbook, rustible::template)
//! since the parser module is private.

use rustible::playbook::{Handler, Play, Playbook, SerialSpec, Task, When};
use rustible::template::TemplateEngine;
use std::collections::HashMap;

// ============================================================================
// Playbook YAML Parsing Tests
// ============================================================================

#[test]
fn test_parse_simple_playbook() {
    let yaml = r#"
- name: Simple playbook
  hosts: all
  tasks:
    - name: Print message
      debug:
        msg: "Hello, World!"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 1);
    assert_eq!(playbook.plays[0].name, "Simple playbook");
    assert_eq!(playbook.plays[0].hosts, "all");
    assert_eq!(playbook.plays[0].tasks.len(), 1);
    assert_eq!(playbook.plays[0].tasks[0].name, "Print message");
}

#[test]
fn test_parse_multi_play_playbook() {
    let yaml = r#"
- name: First play
  hosts: webservers
  tasks:
    - name: Task 1
      ping:

- name: Second play
  hosts: databases
  tasks:
    - name: Task 2
      debug:
        msg: "Database task"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 2);
    assert_eq!(playbook.plays[0].name, "First play");
    assert_eq!(playbook.plays[0].hosts, "webservers");
    assert_eq!(playbook.plays[1].name, "Second play");
    assert_eq!(playbook.plays[1].hosts, "databases");
}

#[test]
fn test_parse_playbook_with_sections() {
    let yaml = r#"
- name: Complex playbook
  hosts: all
  gather_facts: true
  become: true
  become_method: sudo
  become_user: root
  vars_files:
    - vars/common.yml
    - vars/production.yml
  pre_tasks:
    - name: Update cache
      apt:
        update_cache: yes
  roles:
    - common
    - role: nginx
  tasks:
    - name: Deploy app
      copy:
        src: app.tar.gz
        dest: /tmp/app.tar.gz
  post_tasks:
    - name: Cleanup
      file:
        path: /tmp/cleanup
        state: absent
  handlers:
    - name: restart app
      service:
        name: myapp
        state: restarted
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let play = &playbook.plays[0];

    assert_eq!(play.name, "Complex playbook");
    assert_eq!(play.hosts, "all");
    assert!(play.gather_facts);
    assert_eq!(play.r#become, Some(true));
    assert_eq!(play.become_method, Some("sudo".to_string()));
    assert_eq!(play.become_user, Some("root".to_string()));
    assert_eq!(play.vars_files.len(), 2);
    assert_eq!(play.pre_tasks.len(), 1);
    assert_eq!(play.roles.len(), 2);
    assert_eq!(play.tasks.len(), 1);
    assert_eq!(play.post_tasks.len(), 1);
    assert_eq!(play.handlers.len(), 1);
}

// ============================================================================
// Play Parsing Tests
// ============================================================================

#[test]
fn test_parse_play_with_attributes() {
    let yaml = r#"
- name: Full play
  hosts: webservers
  gather_facts: false
  gather_subset:
    - "!all"
    - "!min"
    - network
  gather_timeout: 30
  remote_user: ansible
  become: true
  become_method: sudo
  become_user: root
  connection: ssh
  serial: 2
  strategy: free
  force_handlers: true
  ignore_unreachable: true
  tasks:
    - name: Test task
      debug:
        msg: "test"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let play = &playbook.plays[0];

    assert_eq!(play.name, "Full play");
    assert_eq!(play.hosts, "webservers");
    assert!(!play.gather_facts);
    assert_eq!(play.gather_subset.as_ref().unwrap().len(), 3);
    assert_eq!(play.gather_timeout, Some(30));
    assert_eq!(play.remote_user, Some("ansible".to_string()));
    assert_eq!(play.r#become, Some(true));
    assert_eq!(play.become_method, Some("sudo".to_string()));
    assert_eq!(play.become_user, Some("root".to_string()));
    assert_eq!(play.connection, Some("ssh".to_string()));
    assert!(play.serial.is_some());
    assert_eq!(play.strategy, Some("free".to_string()));
    assert!(play.force_handlers);
    assert!(play.ignore_unreachable);
}

#[test]
fn test_parse_play_serial_as_count() {
    let yaml = r#"
- hosts: all
  serial: 3
  tasks: []
"#;
    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert!(playbook.plays[0].serial.is_some());
    if let Some(SerialSpec::Fixed(n)) = &playbook.plays[0].serial {
        assert_eq!(*n, 3);
    } else {
        panic!("Expected SerialSpec::Fixed(3)");
    }
}

#[test]
fn test_parse_play_serial_as_percentage() {
    let yaml = r#"
- hosts: all
  serial: "30%"
  tasks: []
"#;
    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert!(playbook.plays[0].serial.is_some());
    if let Some(SerialSpec::Percentage(p)) = &playbook.plays[0].serial {
        assert_eq!(p, "30%");
    } else {
        panic!("Expected SerialSpec::Percentage");
    }
}

// ============================================================================
// Task Parsing Tests
// ============================================================================

#[test]
fn test_parse_task_in_playbook() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Simple task
      debug:
        msg: "Hello"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 1);
    assert_eq!(playbook.plays[0].tasks[0].name, "Simple task");
}

#[test]
fn test_parse_task_with_conditionals() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Conditional task
      debug:
        msg: "Running"
      when:
        - ansible_os_family == "Debian"
        - ansible_distribution_version >= "20.04"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    if let Some(When::Multiple(conditions)) = &task.when {
        assert_eq!(conditions.len(), 2);
    } else {
        panic!("Expected When::Multiple with 2 conditions");
    }
}

#[test]
fn test_parse_task_with_register_and_notify() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Task with register and notify
      command: /usr/bin/some-command
      register: command_result
      notify:
        - restart nginx
        - reload php-fpm
      ignore_errors: true
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    assert_eq!(task.register, Some("command_result".to_string()));
    assert_eq!(task.notify.len(), 2);
    assert_eq!(task.notify[0], "restart nginx");
    assert_eq!(task.notify[1], "reload php-fpm");
    assert!(task.ignore_errors);
}

#[test]
fn test_parse_task_with_privilege_escalation() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Task with become
      command: systemctl restart nginx
      become: true
      become_user: root
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    assert_eq!(task.r#become, Some(true));
    assert_eq!(task.become_user, Some("root".to_string()));
}

#[test]
fn test_parse_task_with_delegation() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Delegated task
      command: echo "delegate"
      delegate_to: localhost
      run_once: true
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    assert_eq!(task.delegate_to, Some("localhost".to_string()));
    assert!(task.run_once);
}

#[test]
fn test_parse_task_with_retries() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Task with retries
      uri:
        url: http://example.com/api
        method: GET
      retries: 5
      delay: 10
      until: result.status == 200
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    assert_eq!(task.retries, Some(5));
    assert_eq!(task.delay, Some(10));
    assert!(task.until.is_some());
}

#[test]
fn test_parse_task_with_tags() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Tagged task
      debug:
        msg: "test"
      tags:
        - always
        - production
        - critical
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    assert_eq!(task.tags.len(), 3);
    assert!(task.tags.contains(&"always".to_string()));
    assert!(task.tags.contains(&"production".to_string()));
    assert!(task.tags.contains(&"critical".to_string()));
}

// ============================================================================
// Handler Parsing Tests
// ============================================================================

#[test]
fn test_parse_handlers_in_playbook() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Change something
      command: echo "changed"
      notify:
        - restart nginx
  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].handlers.len(), 1);
    assert_eq!(playbook.plays[0].handlers[0].name, "restart nginx");
}

#[test]
fn test_parse_handler_with_listen() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Change something
      command: echo "changed"
      notify:
        - restart nginx
  handlers:
    - name: restart web services
      service:
        name: nginx
        state: restarted
      listen:
        - restart nginx
        - reload nginx
        - web services changed
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let handler = &playbook.plays[0].handlers[0];

    assert_eq!(handler.name, "restart web services");
    assert_eq!(handler.listen.len(), 3);

    // Test trigger_names method
    let trigger_names = handler.trigger_names();
    assert!(trigger_names.contains(&"restart web services"));
    assert!(trigger_names.contains(&"restart nginx"));
    assert!(trigger_names.contains(&"reload nginx"));
    assert!(trigger_names.contains(&"web services changed"));
}

#[test]
fn test_parse_multiple_handlers() {
    let yaml = r#"
- name: Test play
  hosts: all
  tasks: []
  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted

    - name: reload php-fpm
      service:
        name: php-fpm
        state: reloaded

    - name: restart mysql
      service:
        name: mysql
        state: restarted
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let handlers = &playbook.plays[0].handlers;

    assert_eq!(handlers.len(), 3);
    assert_eq!(handlers[0].name, "restart nginx");
    assert_eq!(handlers[1].name, "reload php-fpm");
    assert_eq!(handlers[2].name, "restart mysql");
}

// ============================================================================
// Template Detection Tests (using TemplateEngine)
// ============================================================================

#[test]
fn test_template_detection() {
    // Jinja2 variable syntax
    assert!(TemplateEngine::is_template("{{ variable }}"));
    assert!(TemplateEngine::is_template("Hello {{ name }}!"));
    assert!(TemplateEngine::is_template("{{ item.value }}"));

    // Jinja2 statement syntax
    assert!(TemplateEngine::is_template("{% if condition %}"));
    assert!(TemplateEngine::is_template("{% for item in items %}"));
    assert!(TemplateEngine::is_template("{% set var = value %}"));

    // Jinja2 comment syntax
    assert!(TemplateEngine::is_template("{# This is a comment #}"));

    // Plain strings
    assert!(!TemplateEngine::is_template("plain string"));
    assert!(!TemplateEngine::is_template("string with { curly } braces"));
    assert!(!TemplateEngine::is_template(""));
}

#[test]
fn test_render_template_simple() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("World"));

    let result = engine.render("Hello, {{ name }}!", &vars).unwrap();
    assert_eq!(result, "Hello, World!");
}

#[test]
fn test_render_template_with_filters() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("world"));

    let result = engine.render("Hello, {{ name | upper }}!", &vars).unwrap();
    assert_eq!(result, "Hello, WORLD!");

    let result = engine
        .render("Hello, {{ name | capitalize }}!", &vars)
        .unwrap();
    assert_eq!(result, "Hello, World!");
}

#[test]
fn test_render_template_with_conditionals() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("enabled".to_string(), serde_json::json!(true));

    let template = "{% if enabled %}Feature is enabled{% else %}Feature is disabled{% endif %}";
    let result = engine.render(template, &vars).unwrap();
    assert_eq!(result, "Feature is enabled");
}

#[test]
fn test_render_template_with_loops() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert(
        "items".to_string(),
        serde_json::json!(["one", "two", "three"]),
    );

    let template = "{% for item in items %}{{ item }} {% endfor %}";
    let result = engine.render(template, &vars).unwrap();
    assert_eq!(result, "one two three ");
}

// ============================================================================
// Role Parsing Tests
// ============================================================================

#[test]
fn test_parse_role_simple() {
    let yaml = r#"
- name: Test play
  hosts: all
  roles:
    - common
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let roles = &playbook.plays[0].roles;

    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].name(), "common");
}

#[test]
fn test_parse_role_syntax_variations() {
    let yaml = r#"
- name: Test role syntax
  hosts: all
  roles:
    - common
    - role: nginx
    - role: postgresql
      tags:
        - database
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let roles = &playbook.plays[0].roles;

    assert_eq!(roles.len(), 3);
    assert_eq!(roles[0].name(), "common");
    assert_eq!(roles[1].name(), "nginx");
    assert_eq!(roles[2].name(), "postgresql");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_parse_malformed_yaml() {
    let yaml = r#"
- name: Bad playbook
  hosts: all
  tasks:
    - name: Task
      debug:
        msg: "unclosed quote
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_playbook() {
    let yaml = "";
    let result = Playbook::from_yaml(yaml, None);
    // Empty YAML may error or return empty
    // Just ensure it doesn't panic
    let _ = result;
}

#[test]
fn test_parse_playbook_with_null_values() {
    let yaml = r#"
- name: Play with nulls
  hosts: all
  remote_user: null
  become_user: null
  tasks:
    - name: Task
      debug:
        msg: "test"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let play = &playbook.plays[0];

    assert!(play.remote_user.is_none());
    assert!(play.become_user.is_none());
}

// ============================================================================
// Edge Cases and Boundary Conditions Tests
// ============================================================================

#[test]
fn test_parse_empty_play() {
    let yaml = r#"
- hosts: all
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 1);
    let play = &playbook.plays[0];

    assert_eq!(play.hosts, "all");
    assert_eq!(play.name, "");
    assert!(play.tasks.is_empty());
    assert!(play.handlers.is_empty());
}

#[test]
fn test_parse_play_with_empty_task_list() {
    let yaml = r#"
- name: Play with empty sections
  hosts: all
  vars_files: []
  pre_tasks: []
  roles: []
  tasks: []
  post_tasks: []
  handlers: []
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let play = &playbook.plays[0];

    assert!(play.vars_files.is_empty());
    assert!(play.pre_tasks.is_empty());
    assert!(play.roles.is_empty());
    assert!(play.tasks.is_empty());
    assert!(play.post_tasks.is_empty());
    assert!(play.handlers.is_empty());
}

#[test]
fn test_parse_task_with_unicode() {
    let yaml = r#"
- name: Unicode test
  hosts: all
  tasks:
    - name: "Task with unicode"
      debug:
        msg: "Hello World!"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 1);
    assert!(playbook.plays[0].tasks[0].name.contains("unicode"));
}

#[test]
fn test_parse_task_with_very_long_strings() {
    let long_msg = "x".repeat(1000);
    let yaml = format!(
        r#"
- name: Long message test
  hosts: all
  tasks:
    - name: Long message
      debug:
        msg: "{}"
"#,
        long_msg
    );

    let playbook = Playbook::from_yaml(&yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 1);
}

#[test]
fn test_parse_multiline_strings() {
    let yaml = r#"
- name: Multiline test
  hosts: all
  tasks:
    - name: Multiline task
      debug:
        msg: |
          This is a
          multiline
          message
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 1);
}

// ============================================================================
// Playbook Validation Tests
// ============================================================================

#[test]
fn test_validate_play_all_tasks() {
    let mut play = Play::new("Test", "all");
    play.pre_tasks
        .push(Task::new("Pre", "debug", serde_json::json!({})));
    play.tasks
        .push(Task::new("Main", "debug", serde_json::json!({})));
    play.post_tasks
        .push(Task::new("Post", "debug", serde_json::json!({})));

    let all_task_names: Vec<String> = play.all_tasks().map(|t| t.name.clone()).collect();
    assert_eq!(all_task_names, vec!["Pre", "Main", "Post"]);
}

#[test]
fn test_playbook_task_count() {
    let yaml = r#"
- name: Play 1
  hosts: all
  tasks:
    - name: Task 1
      debug:
        msg: "1"
    - name: Task 2
      debug:
        msg: "2"

- name: Play 2
  hosts: web
  tasks:
    - name: Task 3
      debug:
        msg: "3"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 2);
    assert_eq!(playbook.task_count(), 3);
}

// ============================================================================
// Template Filter Tests
// ============================================================================

#[test]
fn test_filter_lower() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();
    let result = engine.render("{{ 'HELLO WORLD' | lower }}", &vars).unwrap();
    assert_eq!(result, "hello world");
}

#[test]
fn test_filter_upper() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();
    let result = engine.render("{{ 'hello world' | upper }}", &vars).unwrap();
    assert_eq!(result, "HELLO WORLD");
}

#[test]
fn test_filter_capitalize() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();
    let result = engine
        .render("{{ 'hello world' | capitalize }}", &vars)
        .unwrap();
    assert_eq!(result, "Hello world");
}

#[test]
fn test_filter_title() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();
    let result = engine.render("{{ 'hello world' | title }}", &vars).unwrap();
    assert_eq!(result, "Hello World");
}

#[test]
fn test_filter_trim() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();
    let result = engine.render("{{ '  hello  ' | trim }}", &vars).unwrap();
    assert_eq!(result, "hello");
}

#[test]
fn test_filter_replace() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();
    let result = engine
        .render("{{ 'hello world' | replace('world', 'universe') }}", &vars)
        .unwrap();
    assert_eq!(result, "hello universe");
}

#[test]
fn test_filter_default() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();
    let result = engine
        .render("{{ undefined_var | default('fallback') }}", &vars)
        .unwrap();
    assert_eq!(result, "fallback");
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_playbook_integration() {
    let yaml = r#"
- name: Complete integration test
  hosts: webservers
  gather_facts: true
  become: true
  vars_files:
    - vars/common.yml

  pre_tasks:
    - name: Update apt cache
      apt:
        update_cache: yes
      when: ansible_os_family == "Debian"

  roles:
    - common
    - role: security

  tasks:
    - name: Install packages
      apt:
        name: nginx
        state: present
      notify:
        - packages changed

    - name: Deploy application
      copy:
        src: app.tar.gz
        dest: /opt/app/

  post_tasks:
    - name: Verify deployment
      uri:
        url: "http://localhost:8080/health"
        status_code: 200
      retries: 5
      delay: 2

  handlers:
    - name: packages changed
      service:
        name: myapp
        state: restarted
      listen:
        - restart myapp
        - reload myapp
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 1);

    let play = &playbook.plays[0];
    assert_eq!(play.name, "Complete integration test");
    assert_eq!(play.hosts, "webservers");
    assert!(play.gather_facts);
    assert_eq!(play.r#become, Some(true));
    assert_eq!(play.pre_tasks.len(), 1);
    assert_eq!(play.roles.len(), 2);
    assert_eq!(play.tasks.len(), 2);
    assert_eq!(play.post_tasks.len(), 1);
    assert_eq!(play.handlers.len(), 1);
}

#[test]
fn test_parse_and_template_detection() {
    let yaml = r#"
- name: Template test
  hosts: "{{ target_hosts }}"
  tasks:
    - name: "Greeting task"
      debug:
        msg: "Hello!"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let play = &playbook.plays[0];

    // Verify template strings are preserved
    assert!(TemplateEngine::is_template(&play.hosts));
}

// ============================================================================
// Additional Error Case Tests
// ============================================================================

#[test]
fn test_parse_empty_string() {
    let result = Playbook::from_yaml("", None);
    // Empty string may error - just ensure it doesn't panic
    let _ = result;
}

#[test]
fn test_parse_only_comments() {
    let yaml = r#"
# This is a comment
# Another comment
"#;
    let result = Playbook::from_yaml(yaml, None);
    // Should handle gracefully
    let _ = result;
}

#[test]
fn test_parse_valid_yaml_but_wrong_structure() {
    // Valid YAML but not a playbook structure
    let yaml = r#"
key: value
another_key:
  - item1
  - item2
"#;
    let result = Playbook::from_yaml(yaml, None);
    // Should handle gracefully (might error or produce empty playbook)
    let _ = result;
}

// ============================================================================
// Handler Notification Pattern Tests
// ============================================================================

#[test]
fn test_handler_notification_list() {
    let yaml = r#"
- name: Test handler patterns
  hosts: all
  tasks:
    - name: Task with multiple notify
      command: echo "changed"
      notify:
        - restart nginx
        - reload php-fpm

  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted

    - name: reload php-fpm
      service:
        name: php-fpm
        state: reloaded
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let play = &playbook.plays[0];

    assert_eq!(play.tasks.len(), 1);
    assert_eq!(play.tasks[0].notify.len(), 2);
    assert_eq!(play.handlers.len(), 2);
}

// ============================================================================
// When Condition Tests
// ============================================================================

#[test]
fn test_when_condition_single() {
    let yaml = r#"
- name: Test
  hosts: all
  tasks:
    - name: Single condition
      debug:
        msg: "test"
      when: ansible_os_family == "Debian"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    if let Some(When::Single(condition)) = &task.when {
        assert_eq!(condition, "ansible_os_family == \"Debian\"");
    } else {
        panic!("Expected When::Single");
    }
}

#[test]
fn test_when_condition_multiple() {
    let yaml = r#"
- name: Test
  hosts: all
  tasks:
    - name: Multiple conditions
      debug:
        msg: "test"
      when:
        - ansible_os_family == "Debian"
        - ansible_distribution_version >= "20.04"
        - deploy_enabled | bool
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    if let Some(When::Multiple(conditions)) = &task.when {
        assert_eq!(conditions.len(), 3);
    } else {
        panic!("Expected When::Multiple");
    }
}

// ============================================================================
// Play/Task Construction Tests
// ============================================================================

#[test]
fn test_play_new() {
    let play = Play::new("Test Play", "localhost");
    assert_eq!(play.name, "Test Play");
    assert_eq!(play.hosts, "localhost");
    assert!(play.gather_facts);
    assert!(play.tasks.is_empty());
}

#[test]
fn test_task_new() {
    let task = Task::new("Test Task", "debug", serde_json::json!({"msg": "hello"}));
    assert_eq!(task.name, "Test Task");
    assert_eq!(task.module_name(), "debug");
}

#[test]
fn test_handler_trigger_names() {
    let task = Task::new("handler task", "service", serde_json::json!({}));
    let mut handler = Handler::new("main handler", task);
    handler.listen = vec!["alias1".to_string(), "alias2".to_string()];

    let names = handler.trigger_names();
    assert!(names.contains(&"main handler"));
    assert!(names.contains(&"alias1"));
    assert!(names.contains(&"alias2"));
    assert_eq!(names.len(), 3);
}
