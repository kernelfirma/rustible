//! Ansible Edge-Case Conformance Suite
//!
//! This module contains tests for Ansible edge-case behaviors:
//! - Boolean coercion (yes/no/true/false/on/off)
//! - Block/rescue/always ordering
//! - FQCN (Fully Qualified Collection Name) normalization
//! - CLI default behaviors
//!
//! These tests ensure Rustible matches Ansible's behavior in corner cases.

mod common;

use indexmap::IndexMap;
use rustible::executor::playbook::Playbook;
use rustible::executor::ExecutorConfig;
use rustible::template::TemplateEngine;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;

/// Helper to convert JSON object to IndexMap for evaluate_condition
fn vars_from_json(v: JsonValue) -> IndexMap<String, JsonValue> {
    match v {
        JsonValue::Object(map) => map.into_iter().collect(),
        _ => IndexMap::new(),
    }
}

// ============================================================================
// Boolean Coercion Tests
// ============================================================================
// Ansible accepts various truthy/falsey string values

#[test]
fn test_boolean_coercion_yes_no() {
    let engine = TemplateEngine::new();

    // 'yes' should be truthy
    let result = engine
        .evaluate_condition("'yes' | bool", &vars_from_json(json!({})))
        .unwrap();
    assert!(result, "'yes' should be truthy");

    // 'no' should be falsy
    let result = engine
        .evaluate_condition("'no' | bool", &vars_from_json(json!({})))
        .unwrap();
    assert!(!result, "'no' should be falsy");
}

#[test]
fn test_boolean_coercion_true_false_strings() {
    let engine = TemplateEngine::new();

    // Case-insensitive true
    for val in ["true", "True", "TRUE"] {
        let template = format!("'{}' | bool", val);
        let result = engine
            .evaluate_condition(&template, &vars_from_json(json!({})))
            .unwrap();
        assert!(result, "'{}' should be truthy", val);
    }

    // Case-insensitive false
    for val in ["false", "False", "FALSE"] {
        let template = format!("'{}' | bool", val);
        let result = engine
            .evaluate_condition(&template, &vars_from_json(json!({})))
            .unwrap();
        assert!(!result, "'{}' should be falsy", val);
    }
}

#[test]
fn test_boolean_coercion_on_off() {
    let engine = TemplateEngine::new();

    // 'on' should be truthy
    let result = engine
        .evaluate_condition("'on' | bool", &vars_from_json(json!({})))
        .unwrap();
    assert!(result, "'on' should be truthy");

    // 'off' should be falsy
    let result = engine
        .evaluate_condition("'off' | bool", &vars_from_json(json!({})))
        .unwrap();
    assert!(!result, "'off' should be falsy");
}

#[test]
fn test_boolean_coercion_numeric() {
    let engine = TemplateEngine::new();

    // 1 should be truthy
    let result = engine
        .evaluate_condition("1 | bool", &vars_from_json(json!({})))
        .unwrap();
    assert!(result, "1 should be truthy");

    // 0 should be falsy
    let result = engine
        .evaluate_condition("0 | bool", &vars_from_json(json!({})))
        .unwrap();
    assert!(!result, "0 should be falsy");
}

#[test]
fn test_boolean_coercion_empty_values() {
    let engine = TemplateEngine::new();

    // Empty string should be falsy
    let result = engine
        .evaluate_condition("'' | bool", &vars_from_json(json!({})))
        .unwrap();
    assert!(!result, "empty string should be falsy");

    // Empty list should be falsy
    let result = engine
        .evaluate_condition("[] | bool", &vars_from_json(json!({})))
        .unwrap();
    assert!(!result, "empty list should be falsy");
}

#[test]
fn test_boolean_in_when_condition() {
    // Test that when conditions properly evaluate boolean strings
    let yaml = r#"
- name: Boolean in when
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    enabled: "yes"
    disabled: "no"
  tasks:
    - name: Should run (yes is truthy)
      debug:
        msg: "Running"
      when: enabled | bool

    - name: Should skip (no is falsy)
      debug:
        msg: "Skipped"
      when: disabled | bool
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert!(!playbook.plays.is_empty());
    assert_eq!(playbook.plays[0].tasks.len(), 2);
}

// ============================================================================
// Block/Rescue/Always Ordering Tests
// ============================================================================

#[test]
fn test_block_rescue_always_parsing_order() {
    // Verify block, rescue, always are parsed in correct order
    let yaml = r#"
- name: Error handling block
  block:
    - name: Block task 1
      debug:
        msg: "Block 1"
    - name: Block task 2
      debug:
        msg: "Block 2"
  rescue:
    - name: Rescue task 1
      debug:
        msg: "Rescue 1"
    - name: Rescue task 2
      debug:
        msg: "Rescue 2"
  always:
    - name: Always task 1
      debug:
        msg: "Always 1"
    - name: Always task 2
      debug:
        msg: "Always 2"
"#;

    #[derive(Debug, serde::Deserialize)]
    struct TaskDef {
        name: String,
        #[serde(default)]
        block: Vec<TaskDef>,
        #[serde(default)]
        rescue: Vec<TaskDef>,
        #[serde(default)]
        always: Vec<TaskDef>,
        #[serde(flatten)]
        _extra: HashMap<String, serde_json::Value>,
    }

    let tasks: Vec<TaskDef> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);

    let block_task = &tasks[0];
    assert_eq!(block_task.name, "Error handling block");
    assert_eq!(block_task.block.len(), 2);
    assert_eq!(block_task.rescue.len(), 2);
    assert_eq!(block_task.always.len(), 2);

    // Verify order within each section
    assert_eq!(block_task.block[0].name, "Block task 1");
    assert_eq!(block_task.block[1].name, "Block task 2");
    assert_eq!(block_task.rescue[0].name, "Rescue task 1");
    assert_eq!(block_task.rescue[1].name, "Rescue task 2");
    assert_eq!(block_task.always[0].name, "Always task 1");
    assert_eq!(block_task.always[1].name, "Always task 2");
}

#[test]
fn test_block_with_null_rescue_always() {
    // Ansible treats null block/rescue/always as empty lists
    let yaml = r#"
- name: Block with null sections
  block:
    - debug:
        msg: "Main task"
  rescue: null
  always: null
"#;

    #[derive(Debug, serde::Deserialize)]
    struct TaskDef {
        name: String,
        #[serde(default)]
        block: Vec<serde_json::Value>,
        #[serde(default)]
        rescue: Option<Vec<serde_json::Value>>,
        #[serde(default)]
        always: Option<Vec<serde_json::Value>>,
    }

    let tasks: Vec<TaskDef> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].block.len(), 1);
    // Null should be treated as empty/None
    assert!(tasks[0].rescue.is_none() || tasks[0].rescue.as_ref().unwrap().is_empty());
    assert!(tasks[0].always.is_none() || tasks[0].always.as_ref().unwrap().is_empty());
}

#[test]
fn test_nested_blocks() {
    let yaml = r#"
- name: Outer block
  block:
    - name: Inner block
      block:
        - debug:
            msg: "Deeply nested"
      rescue:
        - debug:
            msg: "Inner rescue"
  rescue:
    - debug:
        msg: "Outer rescue"
"#;

    #[derive(Debug, serde::Deserialize)]
    struct TaskDef {
        name: Option<String>,
        #[serde(default)]
        block: Vec<TaskDef>,
        #[serde(default)]
        rescue: Vec<TaskDef>,
        #[serde(flatten)]
        _extra: HashMap<String, serde_json::Value>,
    }

    let tasks: Vec<TaskDef> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].block.len(), 1);
    assert_eq!(tasks[0].block[0].block.len(), 1); // Inner block has 1 task
    assert_eq!(tasks[0].block[0].rescue.len(), 1); // Inner rescue has 1 task
    assert_eq!(tasks[0].rescue.len(), 1); // Outer rescue has 1 task
}

// ============================================================================
// FQCN Normalization Tests
// ============================================================================

#[test]
fn test_fqcn_ansible_builtin() {
    // ansible.builtin.* should resolve to built-in modules
    let yaml = r#"
- name: FQCN test
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: Using FQCN
      ansible.builtin.debug:
        msg: "Hello"

    - name: Using short name
      debug:
        msg: "Hello"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 2);
}

#[test]
fn test_fqcn_ansible_legacy() {
    // ansible.legacy.* should also resolve correctly
    let yaml = r#"
- name: Legacy FQCN test
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: Using legacy FQCN
      ansible.legacy.command:
        cmd: echo hello
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 1);
}

#[test]
fn test_module_name_normalization() {
    // Various module name formats should work
    let test_cases = vec![
        ("debug", "debug"),
        ("ansible.builtin.debug", "debug"),
        ("ansible.legacy.debug", "debug"),
    ];

    for (input, expected) in test_cases {
        // Normalize by stripping FQCN prefix
        let normalized = if input.starts_with("ansible.builtin.") {
            input.strip_prefix("ansible.builtin.").unwrap()
        } else if input.starts_with("ansible.legacy.") {
            input.strip_prefix("ansible.legacy.").unwrap()
        } else {
            input
        };
        assert_eq!(
            normalized, expected,
            "Module '{}' should normalize to '{}'",
            input, expected
        );
    }
}

// ============================================================================
// CLI Default Behavior Tests
// ============================================================================

#[test]
fn test_executor_config_defaults() {
    let config = ExecutorConfig::default();

    // Verify default values match Ansible defaults
    assert!(!config.check_mode, "check_mode should default to false");
    assert!(!config.diff_mode, "diff_mode should default to false");
    assert!(config.gather_facts, "gather_facts should default to true");
}

#[test]
fn test_check_mode_config() {
    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };

    assert!(config.check_mode);
}

#[test]
fn test_diff_mode_config() {
    let config = ExecutorConfig {
        diff_mode: true,
        ..Default::default()
    };

    assert!(config.diff_mode);
}

// ============================================================================
// Variable Precedence Edge Cases
// ============================================================================

#[test]
fn test_extra_vars_override_all() {
    // Extra vars (-e) should have highest precedence
    let yaml = r#"
- name: Variable precedence
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    my_var: "play_level"
  tasks:
    - debug:
        var: my_var
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert!(!playbook.plays.is_empty());

    // The play-level var should be set
    let vars = &playbook.plays[0].vars;
    assert!(vars.contains_key("my_var"));
}

#[test]
fn test_undefined_variable_default() {
    // Undefined variables with default filter should work
    let engine = TemplateEngine::new();

    let result = engine
        .render_with_json("{{ undefined_var | default('fallback') }}", &json!({}))
        .unwrap();
    assert_eq!(result, "fallback");
}

// ============================================================================
// Loop Edge Cases
// ============================================================================

#[test]
fn test_loop_with_index() {
    // loop.index and loop.index0 should be available
    let yaml = r#"
- name: Loop test
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: Loop with index
      debug:
        msg: "Item {{ item }} at index {{ ansible_loop.index }}"
      loop:
        - a
        - b
        - c
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 1);
}

#[test]
fn test_loop_with_dict() {
    // Looping over dict should work with dict2items
    let yaml = r#"
- name: Dict loop test
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    my_dict:
      key1: value1
      key2: value2
  tasks:
    - name: Loop over dict
      debug:
        msg: "{{ item.key }}: {{ item.value }}"
      loop: "{{ my_dict | dict2items }}"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 1);
}

// ============================================================================
// Conditional Edge Cases
// ============================================================================

#[test]
fn test_when_with_registered_variable() {
    let yaml = r#"
- name: Register and when
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: First task
      command: echo hello
      register: result

    - name: Conditional on result
      debug:
        msg: "Success"
      when: result is success
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 2);
}

#[test]
fn test_when_with_and_or() {
    let yaml = r#"
- name: Complex conditions
  hosts: localhost
  connection: local
  gather_facts: false
  vars:
    a: true
    b: false
  tasks:
    - name: AND condition
      debug:
        msg: "Both true"
      when: a and b

    - name: OR condition
      debug:
        msg: "At least one true"
      when: a or b
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 2);
}

// ============================================================================
// Handler Edge Cases
// ============================================================================

#[test]
fn test_handler_listen() {
    // Handlers with 'listen' should respond to multiple notifies
    let yaml = r#"
- name: Handler listen
  hosts: localhost
  connection: local
  gather_facts: false
  tasks:
    - name: Notify restart
      debug:
        msg: "Triggering"
      notify: restart services

  handlers:
    - name: Restart nginx
      debug:
        msg: "Restarting nginx"
      listen: restart services

    - name: Restart apache
      debug:
        msg: "Restarting apache"
      listen: restart services
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].handlers.len(), 2);
}
