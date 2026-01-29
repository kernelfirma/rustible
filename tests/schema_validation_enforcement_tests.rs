//! Schema Validation Enforcement Test Suite for Issue #285
//!
//! These tests use the production schema validator to ensure parse-time
//! validation errors are surfaced for core modules.

use rustible::schema::{SchemaValidator, ValidationResult};

fn validate(playbook: &str) -> ValidationResult {
    SchemaValidator::new()
        .validate_yaml(playbook)
        .expect("schema validation should not error")
}

fn indent_lines(value: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    value
        .lines()
        .map(|line| format!("{}{}", pad, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn playbook_with_task(task: &str) -> String {
    format!(
        "- name: Test play\n  hosts: all\n  tasks:\n    - name: Task\n{}",
        indent_lines(task, 6)
    )
}

#[test]
fn fails_missing_required_args_for_copy() {
    let playbook = playbook_with_task("copy:\n  src: /tmp/source");
    let result = validate(&playbook);

    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|err| err.message.contains("Missing required argument: dest")));
}

#[test]
fn fails_mutually_exclusive_args_for_copy() {
    let playbook =
        playbook_with_task("copy:\n  dest: /tmp/file\n  src: /tmp/source\n  content: hi");
    let result = validate(&playbook);

    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|err| err.message.contains("Mutually exclusive arguments")));
}

#[test]
fn fails_invalid_choice_for_service_state() {
    let playbook = playbook_with_task("service:\n  name: nginx\n  state: running");
    let result = validate(&playbook);

    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|err| err.message.contains("Invalid value 'running'")));
}

#[test]
fn fails_invalid_type_for_copy_backup() {
    let playbook = playbook_with_task("copy:\n  dest: /tmp/file\n  backup: \"yes\"");
    let result = validate(&playbook);

    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|err| err.message.contains("Expected type Boolean")));
}

#[test]
fn valid_copy_task_passes() {
    let playbook = playbook_with_task("copy:\n  src: /tmp/source\n  dest: /tmp/file");
    let result = validate(&playbook);

    assert!(result.valid);
    assert!(result.errors.is_empty());
}
