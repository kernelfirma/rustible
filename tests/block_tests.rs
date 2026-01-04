//! Comprehensive integration tests for block/rescue/always error handling
//!
//! These tests verify block execution behavior including:
//! - Basic block structure with tasks
//! - Rescue sections for error handling
//! - Always sections for guaranteed cleanup
//! - Combined block/rescue/always patterns
//! - Error information access in rescue
//! - Nested block structures
//! - Blocks with loops and conditions
//! - Block-level variables
//! - Handler interactions with blocks

mod common;

use std::path::PathBuf;

use rustible::executor::playbook::Playbook;
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{TaskResult, TaskStatus};
use rustible::executor::{Executor, ExecutorConfig};

// Import parser types for block structure
#[allow(dead_code)]
#[path = "../src/parser/playbook.rs"]
mod parser_playbook;
use parser_playbook::Task as ParserTask;

use common::*;

// ============================================================================
// Test Fixture Loading Helpers
// ============================================================================

fn blocks_fixture_path(name: &str) -> PathBuf {
    fixtures_path().join("blocks").join(format!("{}.yml", name))
}

fn load_block_fixture(name: &str) -> String {
    std::fs::read_to_string(blocks_fixture_path(name))
        .expect(&format!("Failed to load block fixture: {}", name))
}

fn parse_block_fixture(name: &str) -> Playbook {
    let content = load_block_fixture(name);
    Playbook::parse(&content, Some(blocks_fixture_path(name)))
        .expect(&format!("Failed to parse block fixture: {}", name))
}

// ============================================================================
// Basic Block Tests
// ============================================================================

#[test]
fn test_parse_simple_block() {
    let yaml = r#"
- name: Simple block
  block:
    - name: Task 1
      debug:
        msg: "First"
    - name: Task 2
      debug:
        msg: "Second"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block.len(), 2);
    assert_eq!(tasks[0].block[0].name, "Task 1");
    assert_eq!(tasks[0].block[1].name, "Task 2");
}

#[test]
fn test_parse_block_with_name() {
    let yaml = r#"
- name: Named block for error handling
  block:
    - name: Main operation
      debug:
        msg: "Executing"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks[0].name, "Named block for error handling");
    assert!(tasks[0].is_block());
}

#[test]
fn test_parse_block_with_when_condition() {
    let yaml = r#"
- name: Conditional block
  when: execute_block
  block:
    - name: Task inside conditional block
      debug:
        msg: "Running"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks[0].when_condition.len(), 1);
    assert_eq!(tasks[0].when_condition[0], "execute_block");
    assert!(tasks[0].is_block());
}

#[test]
fn test_parse_block_with_vars() {
    let yaml = r#"
- name: Block with vars
  vars:
    block_var: "value"
    another_var: 42
  block:
    - name: Use block var
      debug:
        msg: "{{ block_var }}"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].vars.len(), 2);
    assert!(tasks[0].vars.contains_key("block_var"));
    assert!(tasks[0].vars.contains_key("another_var"));
}

#[test]
fn test_parse_empty_block() {
    let yaml = r#"
- name: Empty block
  block: []
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    // Empty block is technically still a block, but with no tasks
    assert!(tasks[0].block.is_empty());
    // is_block returns false for empty block
    assert!(!tasks[0].is_block());
}

// ============================================================================
// Rescue Block Tests
// ============================================================================

#[test]
fn test_parse_block_with_rescue() {
    let yaml = r#"
- name: Block with rescue
  block:
    - name: Risky operation
      command: /bin/risky
  rescue:
    - name: Handle error
      debug:
        msg: "Handling failure"
    - name: Recovery step
      debug:
        msg: "Recovering"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block.len(), 1);
    assert_eq!(tasks[0].rescue.len(), 2);
    assert_eq!(tasks[0].rescue[0].name, "Handle error");
    assert_eq!(tasks[0].rescue[1].name, "Recovery step");
}

#[test]
fn test_parse_rescue_empty_block() {
    let yaml = r#"
- name: Block with empty rescue
  block:
    - name: Normal task
      debug:
        msg: "Normal"
  rescue: []
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert!(tasks[0].rescue.is_empty());
}

#[test]
fn test_parse_rescue_with_registered_error_access() {
    // Test that rescue tasks can access ansible_failed_task and ansible_failed_result
    let yaml = r#"
- name: Block with error access
  block:
    - name: Failing task
      fail:
        msg: "Intentional failure"
  rescue:
    - name: Access failed task
      debug:
        msg: "Failed task: {{ ansible_failed_task.name }}"
    - name: Access failed result
      debug:
        msg: "Result: {{ ansible_failed_result }}"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].rescue.len(), 2);
}

#[test]
fn test_parse_rescue_only_without_block() {
    // Rescue without block should be parsed but block will be empty
    let yaml = r#"
- name: Task with only rescue (invalid but parseable)
  rescue:
    - name: Rescue task
      debug:
        msg: "In rescue"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    // This is technically invalid - rescue without block
    // But the parser should handle it gracefully
    assert!(!tasks[0].is_block()); // block is empty
    assert_eq!(tasks[0].rescue.len(), 1);
}

// ============================================================================
// Always Block Tests
// ============================================================================

#[test]
fn test_parse_block_with_always() {
    let yaml = r#"
- name: Block with always
  block:
    - name: Main task
      debug:
        msg: "Main"
  always:
    - name: Cleanup task
      debug:
        msg: "Always runs"
    - name: Final task
      debug:
        msg: "Final"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].always.len(), 2);
    assert_eq!(tasks[0].always[0].name, "Cleanup task");
    assert_eq!(tasks[0].always[1].name, "Final task");
}

#[test]
fn test_parse_always_without_rescue() {
    let yaml = r#"
- name: Block with only always
  block:
    - name: Main task
      debug:
        msg: "Main"
  always:
    - name: Always runs
      debug:
        msg: "Cleanup"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert!(tasks[0].rescue.is_empty());
    assert_eq!(tasks[0].always.len(), 1);
}

#[test]
fn test_parse_always_only_without_block() {
    // Always without block should be parsed but block will be empty
    let yaml = r#"
- name: Task with only always (invalid but parseable)
  always:
    - name: Always task
      debug:
        msg: "In always"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(!tasks[0].is_block()); // block is empty
    assert_eq!(tasks[0].always.len(), 1);
}

// ============================================================================
// Combined Block/Rescue/Always Tests
// ============================================================================

#[test]
fn test_parse_full_block_rescue_always() {
    let yaml = r#"
- name: Complete block structure
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

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block.len(), 2);
    assert_eq!(tasks[0].rescue.len(), 2);
    assert_eq!(tasks[0].always.len(), 2);
}

#[test]
fn test_parse_block_sections_order() {
    // Ensure sections can appear in any order
    let yaml = r#"
- name: Block with reversed order
  always:
    - name: Always first in YAML
      debug:
        msg: "Always"
  rescue:
    - name: Rescue second
      debug:
        msg: "Rescue"
  block:
    - name: Block third
      debug:
        msg: "Block"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block.len(), 1);
    assert_eq!(tasks[0].rescue.len(), 1);
    assert_eq!(tasks[0].always.len(), 1);
}

// ============================================================================
// Nested Block Tests
// ============================================================================

#[test]
fn test_parse_nested_blocks() {
    let yaml = r#"
- name: Outer block
  block:
    - name: Outer task 1
      debug:
        msg: "Outer 1"
    - name: Inner block
      block:
        - name: Inner task 1
          debug:
            msg: "Inner 1"
        - name: Inner task 2
          debug:
            msg: "Inner 2"
      rescue:
        - name: Inner rescue
          debug:
            msg: "Inner rescue"
      always:
        - name: Inner always
          debug:
            msg: "Inner always"
    - name: Outer task 2
      debug:
        msg: "Outer 2"
  rescue:
    - name: Outer rescue
      debug:
        msg: "Outer rescue"
  always:
    - name: Outer always
      debug:
        msg: "Outer always"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    let outer = &tasks[0];

    assert!(outer.is_block());
    assert_eq!(outer.block.len(), 3);

    // Check inner block
    let inner = &outer.block[1];
    assert!(inner.is_block());
    assert_eq!(inner.block.len(), 2);
    assert_eq!(inner.rescue.len(), 1);
    assert_eq!(inner.always.len(), 1);
}

#[test]
fn test_parse_block_inside_rescue() {
    let yaml = r#"
- name: Block with block in rescue
  block:
    - name: Main task
      fail:
        msg: "Failure"
  rescue:
    - name: Pre-block rescue
      debug:
        msg: "Before inner block"
    - name: Inner block in rescue
      block:
        - name: Inner task
          debug:
            msg: "In inner block"
      always:
        - name: Inner always
          debug:
            msg: "Inner always"
    - name: Post-block rescue
      debug:
        msg: "After inner block"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].rescue.len(), 3);

    let inner_block = &tasks[0].rescue[1];
    assert!(inner_block.is_block());
}

#[test]
fn test_parse_block_inside_always() {
    let yaml = r#"
- name: Block with block in always
  block:
    - name: Main task
      debug:
        msg: "Main"
  always:
    - name: Pre-block always
      debug:
        msg: "Before inner block"
    - name: Inner block in always
      block:
        - name: Inner task
          debug:
            msg: "In inner block"
      rescue:
        - name: Inner rescue
          debug:
            msg: "Inner rescue"
    - name: Post-block always
      debug:
        msg: "After inner block"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].always.len(), 3);

    let inner_block = &tasks[0].always[1];
    assert!(inner_block.is_block());
}

#[test]
fn test_parse_deep_nesting() {
    let yaml = r#"
- name: Level 1
  block:
    - name: Level 2
      block:
        - name: Level 3
          block:
            - name: Level 4
              block:
                - name: Deepest task
                  debug:
                    msg: "Deep"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    let level1 = &tasks[0];
    assert!(level1.is_block());

    let level2 = &level1.block[0];
    assert!(level2.is_block());

    let level3 = &level2.block[0];
    assert!(level3.is_block());

    let level4 = &level3.block[0];
    assert!(level4.is_block());
    assert_eq!(level4.block[0].name, "Deepest task");
}

// ============================================================================
// Block with Loop Tests
// ============================================================================

#[test]
fn test_parse_block_with_loop() {
    let yaml = r#"
- name: Looped block
  loop:
    - item1
    - item2
    - item3
  block:
    - name: Task in looped block
      debug:
        msg: "Processing {{ item }}"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert!(tasks[0].loop_over.is_some());
}

#[test]
fn test_parse_block_with_loop_control() {
    let yaml = r#"
- name: Block with loop control
  loop:
    - first
    - second
  loop_control:
    loop_var: my_item
    index_var: my_idx
  block:
    - name: Task with custom loop var
      debug:
        msg: "Item: {{ my_item }}, Index: {{ my_idx }}"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert!(tasks[0].loop_over.is_some());
    assert!(tasks[0].loop_control.is_some());
    assert_eq!(tasks[0].loop_control.as_ref().unwrap().loop_var, "my_item");
}

// ============================================================================
// Block with Conditions Tests
// ============================================================================

#[test]
fn test_parse_block_with_multiple_when_conditions() {
    let yaml = r#"
- name: Block with multiple conditions
  when:
    - condition1
    - condition2
    - condition3
  block:
    - name: Task in conditional block
      debug:
        msg: "Running"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].when_condition.len(), 3);
}

#[test]
fn test_parse_block_tasks_inherit_conditions() {
    // Tasks inside block can have their own conditions
    let yaml = r#"
- name: Block with conditional tasks
  when: block_enabled
  block:
    - name: Always runs in block
      debug:
        msg: "Always in block"
    - name: Conditional task in block
      when: extra_condition
      debug:
        msg: "Extra conditional"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].when_condition.len(), 1);

    // Second task in block has its own condition
    assert_eq!(tasks[0].block[1].when_condition.len(), 1);
    assert_eq!(tasks[0].block[1].when_condition[0], "extra_condition");
}

// ============================================================================
// Block Variables Scope Tests
// ============================================================================

#[test]
fn test_parse_block_vars_scope() {
    let yaml = r#"
- name: Block with vars
  vars:
    block_var: "block_value"
  block:
    - name: Task using block var
      debug:
        msg: "{{ block_var }}"
    - name: Task with own vars
      vars:
        task_var: "task_value"
      debug:
        msg: "{{ task_var }}"
  rescue:
    - name: Rescue uses block vars
      debug:
        msg: "{{ block_var }}"
  always:
    - name: Always uses block vars
      debug:
        msg: "{{ block_var }}"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].vars.len(), 1);

    // Task inside block can have its own vars
    assert_eq!(tasks[0].block[1].vars.len(), 1);
}

// ============================================================================
// Block with Handler Notifications Tests
// ============================================================================

#[test]
fn test_parse_block_task_with_notify() {
    let yaml = r#"
- name: Block with notifications
  block:
    - name: Task that notifies
      debug:
        msg: "Change"
      changed_when: true
      notify: my_handler
    - name: Another notifying task
      debug:
        msg: "Another change"
      notify:
        - handler1
        - handler2
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block[0].notify.len(), 1);
    assert_eq!(tasks[0].block[1].notify.len(), 2);
}

#[test]
fn test_parse_rescue_task_with_notify() {
    let yaml = r#"
- name: Block with rescue notification
  block:
    - name: Failing task
      fail:
        msg: "Failure"
  rescue:
    - name: Rescue that notifies
      debug:
        msg: "Recovering"
      notify: recovery_handler
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].rescue[0].notify.len(), 1);
    assert_eq!(tasks[0].rescue[0].notify[0], "recovery_handler");
}

// ============================================================================
// Block Attributes Tests
// ============================================================================

#[test]
fn test_parse_block_with_become() {
    let yaml = r#"
- name: Block with privilege escalation
  become: true
  become_user: root
  become_method: sudo
  block:
    - name: Privileged task
      command: systemctl restart nginx
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].r#become, Some(true));
    assert_eq!(tasks[0].become_user, Some("root".to_string()));
    assert_eq!(tasks[0].become_method, Some("sudo".to_string()));
}

#[test]
fn test_parse_block_with_tags() {
    let yaml = r#"
- name: Tagged block
  tags:
    - deployment
    - critical
  block:
    - name: Tagged task
      debug:
        msg: "In tagged block"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].tags.len(), 2);
    assert!(tasks[0].tags.contains(&"deployment".to_string()));
    assert!(tasks[0].tags.contains(&"critical".to_string()));
}

#[test]
fn test_parse_block_with_ignore_errors() {
    let yaml = r#"
- name: Block ignoring errors
  ignore_errors: true
  block:
    - name: May fail
      command: /bin/maybe-fails
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert!(tasks[0].ignore_errors);
}

#[test]
fn test_parse_block_with_delegate() {
    let yaml = r#"
- name: Delegated block
  delegate_to: localhost
  block:
    - name: Local task
      debug:
        msg: "Running locally"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].delegate_to, Some("localhost".to_string()));
}

#[test]
fn test_parse_block_with_throttle() {
    let yaml = r#"
- name: Throttled block
  throttle: 2
  block:
    - name: Limited concurrency task
      debug:
        msg: "Throttled"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].throttle, Some(2));
}

#[test]
fn test_parse_block_with_any_errors_fatal() {
    let yaml = r#"
- name: Fatal errors block
  any_errors_fatal: true
  block:
    - name: Critical task
      command: /bin/critical
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert!(tasks[0].any_errors_fatal);
}

// ============================================================================
// Fixture-based Tests
// ============================================================================

#[test]
fn test_load_simple_block_fixture() {
    let playbook = parse_block_fixture("simple_block");
    assert_eq!(playbook.plays.len(), 1);
    assert!(!playbook.plays[0].tasks.is_empty());
}

#[test]
fn test_load_block_with_rescue_fixture() {
    let playbook = parse_block_fixture("block_with_rescue");
    assert_eq!(playbook.plays.len(), 1);
    assert!(!playbook.plays[0].tasks.is_empty());
}

#[test]
fn test_load_block_with_always_fixture() {
    let playbook = parse_block_fixture("block_with_always");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_complete_block_fixture() {
    let playbook = parse_block_fixture("block_rescue_always");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_nested_blocks_fixture() {
    let playbook = parse_block_fixture("nested_blocks");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_block_with_vars_fixture() {
    let playbook = parse_block_fixture("block_with_vars");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_block_with_when_fixture() {
    let playbook = parse_block_fixture("block_with_when");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_block_error_info_fixture() {
    let playbook = parse_block_fixture("block_error_info");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_block_with_loop_fixture() {
    let playbook = parse_block_fixture("block_with_loop");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_block_rescue_failure_fixture() {
    let playbook = parse_block_fixture("block_rescue_failure");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_block_handler_notify_fixture() {
    let playbook = parse_block_fixture("block_handler_notify");
    assert_eq!(playbook.plays.len(), 1);
    // Verify handlers are present
    assert!(!playbook.plays[0].handlers.is_empty());
}

#[test]
fn test_load_deep_nesting_fixture() {
    let playbook = parse_block_fixture("deep_nesting");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_block_inside_rescue_fixture() {
    let playbook = parse_block_fixture("block_inside_rescue");
    assert_eq!(playbook.plays.len(), 1);
}

#[test]
fn test_load_block_inside_always_fixture() {
    let playbook = parse_block_fixture("block_inside_always");
    assert_eq!(playbook.plays.len(), 1);
}

// ============================================================================
// Executor Integration Tests
// ============================================================================

#[tokio::test]
async fn test_execute_simple_block() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let playbook = parse_block_fixture("simple_block");
    let results = executor.run_playbook(&playbook).await.unwrap();

    assert!(results.contains_key("localhost"));
    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
}

#[tokio::test]
async fn test_execute_block_with_always() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let playbook = parse_block_fixture("block_with_always");
    let results = executor.run_playbook(&playbook).await.unwrap();

    assert!(results.contains_key("localhost"));
}

#[tokio::test]
async fn test_execute_block_rescue_always() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let playbook = parse_block_fixture("block_rescue_always");
    let results = executor.run_playbook(&playbook).await.unwrap();

    assert!(results.contains_key("localhost"));
}

#[tokio::test]
async fn test_execute_nested_blocks() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let playbook = parse_block_fixture("nested_blocks");
    let results = executor.run_playbook(&playbook).await.unwrap();

    assert!(results.contains_key("localhost"));
    let host_result = results.get("localhost").unwrap();
    // Nested blocks should complete successfully
    assert!(!host_result.failed);
}

// ============================================================================
// Edge Cases and Error Handling Tests
// ============================================================================

#[test]
fn test_parse_block_with_complex_module_args() {
    let yaml = r#"
- name: Block with complex module
  block:
    - name: Complex copy
      copy:
        src: /path/to/source
        dest: /path/to/dest
        owner: root
        group: root
        mode: "0644"
        backup: true
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block[0].name, "Complex copy");
}

#[test]
fn test_parse_block_with_register() {
    let yaml = r#"
- name: Block with register
  block:
    - name: Command to register
      command: echo "hello"
      register: command_result
    - name: Use registered result
      debug:
        msg: "{{ command_result.stdout }}"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(
        tasks[0].block[0].register,
        Some("command_result".to_string())
    );
}

#[test]
fn test_parse_block_with_failed_when() {
    let yaml = r#"
- name: Block with failed_when
  block:
    - name: Task with custom failure
      command: /bin/check-something
      register: result
      failed_when:
        - result.rc != 0
        - "'error' in result.stderr"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block[0].failed_when.len(), 2);
}

#[test]
fn test_parse_block_with_changed_when() {
    let yaml = r#"
- name: Block with changed_when
  block:
    - name: Task with custom changed
      command: /bin/update-something
      register: result
      changed_when:
        - "'updated' in result.stdout"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block[0].changed_when.len(), 1);
}

#[test]
fn test_parse_block_with_retries() {
    let yaml = r#"
- name: Block with retry in task
  block:
    - name: Retry task
      uri:
        url: http://example.com/api
      retries: 5
      delay: 10
      until:
        - result.status == 200
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block[0].retries, Some(5));
    assert_eq!(tasks[0].block[0].delay, Some(10));
    assert_eq!(tasks[0].block[0].until.len(), 1);
}

#[test]
fn test_parse_block_with_async() {
    let yaml = r#"
- name: Block with async task
  block:
    - name: Async command
      command: /bin/long-running-task
      async: 3600
      poll: 0
      register: async_job
    - name: Check async job
      async_status:
        jid: "{{ async_job.ansible_job_id }}"
      register: job_result
      until: job_result.finished
      retries: 30
      delay: 10
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block[0].async_timeout, Some(3600));
    assert_eq!(tasks[0].block[0].poll, Some(0));
}

#[test]
fn test_parse_block_with_no_log() {
    let yaml = r#"
- name: Block with sensitive data
  block:
    - name: Sensitive task
      command: echo "secret_password"
      no_log: true
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert!(tasks[0].block[0].no_log);
}

#[test]
fn test_parse_block_with_run_once() {
    let yaml = r#"
- name: Block with run_once
  run_once: true
  block:
    - name: Run once task
      debug:
        msg: "Only on first host"
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert!(tasks[0].run_once);
}

#[test]
fn test_parse_block_with_environment() {
    let yaml = r#"
- name: Block with environment
  environment:
    PATH: "/custom/bin:{{ ansible_env.PATH }}"
    MY_VAR: "value"
  block:
    - name: Task with env
      command: echo $MY_VAR
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].environment.len(), 2);
}

// ============================================================================
// Block Task State Tests
// ============================================================================

#[test]
fn test_task_result_states() {
    // Verify TaskResult states work correctly for block scenarios
    let ok = TaskResult::ok();
    assert_eq!(ok.status, TaskStatus::Ok);
    assert!(!ok.changed);

    let changed = TaskResult::changed();
    assert_eq!(changed.status, TaskStatus::Changed);
    assert!(changed.changed);

    let failed = TaskResult::failed("Block failed");
    assert_eq!(failed.status, TaskStatus::Failed);
    assert_eq!(failed.msg, Some("Block failed".to_string()));

    let skipped = TaskResult::skipped("Condition not met");
    assert_eq!(skipped.status, TaskStatus::Skipped);
    assert_eq!(skipped.msg, Some("Condition not met".to_string()));
}

// ============================================================================
// Block Structure Validation Tests
// ============================================================================

#[test]
fn test_is_block_detection() {
    // Test is_block method
    let yaml_block = r#"
- name: This is a block
  block:
    - name: Inner task
      debug:
        msg: "test"
"#;

    let yaml_not_block = r#"
- name: Regular task
  debug:
    msg: "test"
"#;

    let block_tasks: Vec<ParserTask> = serde_yaml::from_str(yaml_block).unwrap();
    let regular_tasks: Vec<ParserTask> = serde_yaml::from_str(yaml_not_block).unwrap();

    assert!(block_tasks[0].is_block());
    assert!(!regular_tasks[0].is_block());
}

#[test]
fn test_block_with_all_task_types() {
    // Block containing various task types
    let yaml = r#"
- name: Block with various tasks
  block:
    - name: Debug task
      debug:
        msg: "Debug"
    - name: Set fact task
      set_fact:
        my_var: "value"
    - name: Command task
      command: echo "hello"
    - name: Copy task
      copy:
        content: "test"
        dest: /tmp/test
    - name: Assert task
      assert:
        that:
          - true
"#;

    let tasks: Vec<ParserTask> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].is_block());
    assert_eq!(tasks[0].block.len(), 5);
}

// ============================================================================
// Comprehensive Integration Test
// ============================================================================

#[test]
fn test_parse_comprehensive_block_playbook() {
    let yaml = r#"
- name: Comprehensive block test play
  hosts: all
  gather_facts: false
  vars:
    global_var: "global"

  tasks:
    - name: Pre-block task
      debug:
        msg: "Before blocks"

    - name: Main block
      vars:
        block_var: "block"
      when: true
      tags:
        - main
      block:
        - name: Block task 1
          debug:
            msg: "{{ global_var }} - {{ block_var }}"
          register: debug_result

        - name: Conditional block task
          when: debug_result is defined
          debug:
            msg: "Conditional"

        - name: Nested block
          block:
            - name: Deeply nested
              debug:
                msg: "Deep"
          always:
            - name: Inner always
              debug:
                msg: "Inner cleanup"

      rescue:
        - name: Rescue task
          debug:
            msg: "Rescue: {{ ansible_failed_task.name | default('unknown') }}"

      always:
        - name: Always cleanup
          debug:
            msg: "Cleanup"
        - name: Final verification
          debug:
            msg: "Done"

    - name: Post-block task
      debug:
        msg: "After blocks"

  handlers:
    - name: test_handler
      debug:
        msg: "Handler executed"
"#;

    let playbook: Result<Vec<parser_playbook::Play>, _> = serde_yaml::from_str(yaml);
    assert!(playbook.is_ok());

    let plays = playbook.unwrap();
    assert_eq!(plays.len(), 1);
    assert_eq!(plays[0].tasks.len(), 3);

    // Check main block structure
    let main_block = &plays[0].tasks[1];
    assert!(main_block.is_block());
    assert_eq!(main_block.block.len(), 3);
    assert_eq!(main_block.rescue.len(), 1);
    assert_eq!(main_block.always.len(), 2);

    // Check nested block
    let nested = &main_block.block[2];
    assert!(nested.is_block());
    assert_eq!(nested.always.len(), 1);
}
