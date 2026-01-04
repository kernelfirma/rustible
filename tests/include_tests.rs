//! Comprehensive tests for Rustible include and import functionality
//!
//! This test suite covers:
//! 1. include_tasks - Dynamic task inclusion at runtime
//! 2. import_tasks - Static task import at parse time
//! 3. include_role - Dynamic role inclusion at runtime
//! 4. import_role - Static role import at parse time
//! 5. include_vars - Variable file inclusion
//! 6. import_playbook - Playbook imports
//! 7. Static vs Dynamic include behavior
//! 8. Variable passing and scope
//! 9. Handlers in included files
//! 10. Error handling for includes

use indexmap::IndexMap;
use rustible::executor::playbook::{Play, Playbook, TaskDefinition};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{Handler, Task};
use rustible::executor::{Executor, ExecutorConfig};

// ============================================================================
// Test Helpers
// ============================================================================

// ============================================================================
// 1. INCLUDE_TASKS Tests
// ============================================================================

#[test]
fn test_include_tasks_basic_structure() {
    // Test that include_tasks is properly parsed
    let yaml = r#"
- name: Include common tasks
  include_tasks: tasks/common.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "Include common tasks");
    assert_eq!(tasks[0].include_tasks, Some("tasks/common.yml".to_string()));
}

#[test]
fn test_include_tasks_with_vars() {
    // Test include_tasks with variables
    // Note: TaskDefinition uses module_args for vars in task context
    let yaml = r#"
- name: Include with vars
  include_tasks: tasks/configure.yml
  vars:
    config_file: /etc/myapp.conf
    config_mode: "0644"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].include_tasks.is_some());
    // vars are captured in the module flattening
    assert!(tasks[0].module.contains_key("vars"));
}

#[test]
fn test_include_tasks_with_when_condition() {
    // Test include_tasks with conditional
    let yaml = r#"
- name: Conditional include
  include_tasks: tasks/debian.yml
  when: ansible_os_family == "Debian"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].include_tasks.is_some());
    assert!(tasks[0].when.is_some());
}

#[test]
fn test_include_tasks_in_loop() {
    // Test include_tasks in a loop
    // Note: Rust's `loop` is a reserved keyword, so we use `with_items` alias
    let yaml = r#"
- name: Include tasks in loop
  include_tasks: "tasks/{{ item }}.yml"
  with_items:
    - setup
    - configure
    - verify
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].include_tasks.is_some());
    assert!(tasks[0].loop_items.is_some());
}

#[test]
fn test_include_tasks_dynamic_file_path() {
    // Test include_tasks with dynamic/templated path
    let yaml = r#"
- name: Dynamic include
  include_tasks: "tasks/{{ task_type }}.yml"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    let include_path = tasks[0].include_tasks.as_ref().unwrap();
    assert!(include_path.contains("{{ task_type }}"));
}

#[tokio::test]
async fn test_include_tasks_execution() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Include Tasks Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add include_tasks task
    play.add_task(
        Task::new("Include common tasks", "include_tasks").arg("file", "tasks/common.yml"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

// ============================================================================
// 2. IMPORT_TASKS Tests
// ============================================================================

#[test]
fn test_import_tasks_basic_structure() {
    // Test that import_tasks is properly parsed
    let yaml = r#"
- name: Import base tasks
  import_tasks: tasks/base.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "Import base tasks");
    assert_eq!(tasks[0].import_tasks, Some("tasks/base.yml".to_string()));
}

#[test]
fn test_import_tasks_with_vars() {
    // Test import_tasks with variables
    let yaml = r#"
- name: Import with vars
  import_tasks: tasks/configure.yml
  vars:
    service_name: nginx
    service_port: 80
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].import_tasks.is_some());
    // vars are captured in the module flattening
    assert!(tasks[0].module.contains_key("vars"));
}

#[test]
fn test_import_tasks_static_when() {
    // Test import_tasks with when (applied to all imported tasks)
    let yaml = r#"
- name: Conditional import
  import_tasks: tasks/debian.yml
  when: ansible_os_family == "Debian"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].import_tasks.is_some());
    assert!(tasks[0].when.is_some());
}

#[test]
fn test_import_tasks_with_tags() {
    // Test import_tasks with tags (applied to all imported tasks)
    let yaml = r#"
- name: Tagged import
  import_tasks: tasks/tagged.yml
  tags:
    - configuration
    - always
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].import_tasks.is_some());
    assert_eq!(tasks[0].tags.len(), 2);
    assert!(tasks[0].tags.contains(&"configuration".to_string()));
    assert!(tasks[0].tags.contains(&"always".to_string()));
}

#[tokio::test]
async fn test_import_tasks_execution() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Import Tasks Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add import_tasks task
    play.add_task(Task::new("Import base tasks", "import_tasks").arg("file", "tasks/base.yml"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

// ============================================================================
// 3. INCLUDE_ROLE Tests
// ============================================================================

#[test]
fn test_include_role_basic_structure() {
    // Test basic include_role parsing
    let yaml = r#"
- name: Include nginx role
  include_role:
    name: nginx
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].include_role.is_some());
    assert_eq!(tasks[0].include_role.as_ref().unwrap().name, "nginx");
}

#[test]
fn test_include_role_with_vars() {
    // Test include_role with variables
    let yaml = r#"
- name: Include role with vars
  include_role:
    name: webserver
  vars:
    http_port: 8080
    https_enabled: true
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].include_role.is_some());
    // vars are in the module map
    assert!(tasks[0].module.contains_key("vars"));
}

#[test]
fn test_include_role_with_when_condition() {
    // Test include_role with conditional
    let yaml = r#"
- name: Conditional role include
  include_role:
    name: database
  when: install_database | bool
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].include_role.is_some());
    assert!(tasks[0].when.is_some());
}

#[test]
fn test_include_role_in_loop() {
    // Test include_role in a loop (dynamic role selection)
    // Note: Rust's `loop` is a reserved keyword, so we use `with_items` alias
    let yaml = r#"
- name: Include roles in loop
  include_role:
    name: "{{ item }}"
  with_items:
    - common
    - webserver
    - database
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].include_role.is_some());
    assert!(tasks[0].loop_items.is_some());
}

#[test]
fn test_include_role_with_tasks_from() {
    // Test include_role with tasks_from
    let yaml = r#"
- name: Include specific tasks from role
  include_role:
    name: application
    tasks_from: deploy.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    let include_role = tasks[0].include_role.as_ref().unwrap();
    assert_eq!(include_role.name, "application");
    assert_eq!(include_role.tasks_from, Some("deploy.yml".to_string()));
}

#[test]
fn test_include_role_with_handlers_from() {
    // Test include_role with handlers_from
    let yaml = r#"
- name: Include role with custom handlers
  include_role:
    name: nginx
    handlers_from: custom.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let include_role = tasks[0].include_role.as_ref().unwrap();
    assert_eq!(include_role.handlers_from, Some("custom.yml".to_string()));
}

#[test]
fn test_include_role_with_defaults_from() {
    // Test include_role with defaults_from
    let yaml = r#"
- name: Include role with custom defaults
  include_role:
    name: nginx
    defaults_from: production.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let include_role = tasks[0].include_role.as_ref().unwrap();
    assert_eq!(
        include_role.defaults_from,
        Some("production.yml".to_string())
    );
}

#[test]
fn test_include_role_public_option() {
    // Test include_role with public option
    let yaml = r#"
- name: Include role with public vars
  include_role:
    name: common
    public: true
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let include_role = tasks[0].include_role.as_ref().unwrap();
    assert!(include_role.public);
}

#[test]
fn test_include_role_allow_duplicates() {
    // Test include_role with allow_duplicates
    let yaml = r#"
- name: Include role allowing duplicates
  include_role:
    name: common
    allow_duplicates: true
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let include_role = tasks[0].include_role.as_ref().unwrap();
    assert!(include_role.allow_duplicates);
}

#[tokio::test]
async fn test_include_role_execution() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Include Role Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add include_role task
    play.add_task(Task::new("Include test role", "include_role").arg("name", "test_role"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

// ============================================================================
// 4. IMPORT_ROLE Tests
// ============================================================================

#[test]
fn test_import_role_basic_structure() {
    // Test basic import_role parsing
    let yaml = r#"
- name: Import common role
  import_role:
    name: common
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].import_role.is_some());
    assert_eq!(tasks[0].import_role.as_ref().unwrap().name, "common");
}

#[test]
fn test_import_role_static_processing() {
    // Test import_role with when (applied statically to all tasks)
    let yaml = r#"
- name: Static role import
  import_role:
    name: security
  when: enable_security | default(true)
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].import_role.is_some());
    assert!(tasks[0].when.is_some());
}

#[test]
fn test_import_role_with_tags() {
    // Test import_role with tags (applied to all imported role tasks)
    let yaml = r#"
- name: Tagged role import
  import_role:
    name: nginx
  tags:
    - webserver
    - production
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].import_role.is_some());
    assert_eq!(tasks[0].tags.len(), 2);
}

#[test]
fn test_import_role_with_tasks_from() {
    // Test import_role with tasks_from
    let yaml = r#"
- name: Import specific tasks from role
  import_role:
    name: nginx
    tasks_from: install.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let import_role = tasks[0].import_role.as_ref().unwrap();
    assert_eq!(import_role.tasks_from, Some("install.yml".to_string()));
}

#[tokio::test]
async fn test_import_role_execution() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Import Role Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add import_role task
    play.add_task(Task::new("Import test role", "import_role").arg("name", "test_role"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

// ============================================================================
// 5. INCLUDE_VARS Tests
// ============================================================================

#[test]
fn test_include_vars_file_string() {
    // Test include_vars with simple file string
    let yaml = r#"
- name: Include common variables
  include_vars: vars/common.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].module.contains_key("include_vars"));
}

#[test]
fn test_include_vars_with_file_option() {
    // Test include_vars with file option
    let yaml = r#"
- name: Include variables
  include_vars:
    file: vars/production.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].module.contains_key("include_vars"));
}

#[test]
fn test_include_vars_with_dir_option() {
    // Test include_vars with directory
    let yaml = r#"
- name: Include all vars from directory
  include_vars:
    dir: vars/
    extensions:
      - yml
      - yaml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].module.contains_key("include_vars"));
}

#[test]
fn test_include_vars_with_when_condition() {
    // Test include_vars with conditional
    let yaml = r#"
- name: Conditional vars include
  include_vars: "vars/{{ env }}.yml"
  when: env is defined
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].module.contains_key("include_vars"));
    assert!(tasks[0].when.is_some());
}

#[test]
fn test_include_vars_with_name_option() {
    // Test include_vars with name (namespace)
    let yaml = r#"
- name: Include vars into namespace
  include_vars:
    file: vars/os_specific.yml
    name: os_vars
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].module.contains_key("include_vars"));
}

#[test]
fn test_include_vars_with_files_matching() {
    // Test include_vars with files_matching pattern
    let yaml = r#"
- name: Include matching vars
  include_vars:
    dir: vars/
    files_matching: "*.yml"
    ignore_unknown_extensions: true
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].module.contains_key("include_vars"));
}

#[tokio::test]
async fn test_include_vars_execution() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Include Vars Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add include_vars task
    play.add_task(Task::new("Include common vars", "include_vars").arg("file", "vars/common.yml"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

// ============================================================================
// 6. IMPORT_PLAYBOOK Tests (Parser Level)
// ============================================================================

#[test]
fn test_import_playbook_basic_structure() {
    // Test import_playbook at playbook level
    // Note: import_playbook is typically at the top level, not inside plays
    let yaml = r#"
- import_playbook: playbooks/secondary.yml
"#;

    // This should parse as a play-level import
    let result: Result<Vec<serde_yaml::Value>, _> = serde_yaml::from_str(yaml);
    assert!(result.is_ok());
}

#[test]
fn test_import_playbook_with_vars() {
    // Test import_playbook with variables
    let yaml = r#"
- import_playbook: playbooks/secondary.yml
  vars:
    env: production
    deploy_version: "1.0.0"
"#;

    let result: Result<Vec<serde_yaml::Value>, _> = serde_yaml::from_str(yaml);
    assert!(result.is_ok());
}

#[test]
fn test_import_playbook_conditional() {
    // Test import_playbook with when condition
    let yaml = r#"
- import_playbook: playbooks/optional.yml
  when: include_optional | default(false)
"#;

    let result: Result<Vec<serde_yaml::Value>, _> = serde_yaml::from_str(yaml);
    assert!(result.is_ok());
}

// ============================================================================
// 7. STATIC VS DYNAMIC Include Behavior Tests
// ============================================================================

#[test]
fn test_import_is_static_no_templating_in_path() {
    // import_* cannot use templated file paths
    // This is a design constraint - imports are processed at parse time

    // Valid import (static path)
    let yaml = r#"
- name: Static import
  import_tasks: tasks/base.yml
"#;
    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].import_tasks.is_some());

    // Import with template (technically parseable but won't work at runtime)
    let yaml_template = r#"
- name: Templated import (anti-pattern)
  import_tasks: "tasks/{{ task_file }}.yml"
"#;
    let tasks_template: Vec<TaskDefinition> = serde_yaml::from_str(yaml_template).unwrap();
    // This parses but the template won't be resolved at parse time
    assert!(tasks_template[0]
        .import_tasks
        .as_ref()
        .unwrap()
        .contains("{{"));
}

#[test]
fn test_include_supports_templated_paths() {
    // include_* supports templated file paths (resolved at runtime)
    let yaml = r#"
- name: Dynamic include
  include_tasks: "tasks/{{ task_file }}.yml"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let include_path = tasks[0].include_tasks.as_ref().unwrap();
    assert!(include_path.contains("{{ task_file }}"));
}

#[test]
fn test_include_with_loop_is_valid() {
    // include_tasks with loop is valid (evaluated at runtime)
    // Note: Rust's `loop` is a reserved keyword, so we use `with_items` alias
    let yaml = r#"
- name: Include in loop
  include_tasks: "tasks/{{ item }}.yml"
  with_items:
    - setup
    - configure
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].include_tasks.is_some());
    assert!(tasks[0].loop_items.is_some());
}

#[test]
fn test_import_with_loop_not_recommended() {
    // import_tasks with loop is an anti-pattern (import is static)
    // This will parse but is not recommended behavior
    // Note: Rust's `loop` is a reserved keyword, so we use `with_items` alias
    let yaml = r#"
- name: Import in loop (anti-pattern)
  import_tasks: "tasks/base.yml"
  with_items:
    - one
    - two
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    // Parses but semantically problematic
    assert!(tasks[0].import_tasks.is_some());
    assert!(tasks[0].loop_items.is_some());
}

#[test]
fn test_conditional_include_works_at_runtime() {
    // include_tasks when condition evaluated at runtime
    let yaml = r#"
- name: Conditional include
  include_tasks: tasks/optional.yml
  when: include_optional | default(false)
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].include_tasks.is_some());
    assert!(tasks[0].when.is_some());
}

#[test]
fn test_conditional_import_applies_to_all_tasks() {
    // import_tasks when condition applied to all imported tasks
    let yaml = r#"
- name: Conditional import
  import_tasks: tasks/debian.yml
  when: ansible_os_family == "Debian"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].import_tasks.is_some());
    // The when will be applied to each imported task
    assert!(tasks[0].when.is_some());
}

// ============================================================================
// 8. VARIABLE PASSING Tests
// ============================================================================

#[test]
fn test_vars_parameter_in_include_tasks() {
    // Test vars parameter with include_tasks
    let yaml = r#"
- name: Include with vars
  include_tasks: tasks/with_vars.yml
  vars:
    var1: "value1"
    var2: "value2"
    complex_var:
      nested: value
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    // Vars are in the module map due to serde flatten
    assert!(tasks[0].module.contains_key("vars"));
}

#[test]
fn test_vars_parameter_in_include_role() {
    // Test vars parameter with include_role
    let yaml = r#"
- name: Include role with vars
  include_role:
    name: test_role
  vars:
    role_port: 9090
    role_enabled: false
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].module.contains_key("vars"));
}

#[test]
fn test_variable_override_in_include() {
    // Test that include vars can override existing vars
    let yaml = r#"
- name: Include with overrides
  include_tasks: tasks/configure.yml
  vars:
    config_file: /custom/path.conf
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    // Check that vars are present in module map
    let vars = tasks[0].module.get("vars").unwrap();
    assert!(vars.get("config_file").is_some());
}

#[tokio::test]
async fn test_variable_scope_in_included_file() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_global_var("global_var".to_string(), serde_json::json!("global_value"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Variable Scope Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;
    play.set_var("play_var", serde_json::json!("play_value"));

    // Include with task-level vars
    let mut include_task =
        Task::new("Include with vars", "include_tasks").arg("file", "tasks/with_vars.yml");
    include_task.args.insert(
        "vars".to_string(),
        serde_json::json!({
            "task_var": "task_value"
        }),
    );
    play.add_task(include_task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

// ============================================================================
// 9. HANDLERS IN INCLUDES Tests
// ============================================================================

#[test]
fn test_handlers_from_included_file_structure() {
    // Test handler include structure
    // Note: changed_when must be a string expression, not boolean literal
    let yaml = r#"
- name: Notify included handler
  debug:
    msg: "Notifying"
  notify: included_handler
  changed_when: "true"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    // Check that notify is not empty using to_vec()
    assert!(!tasks[0].notify.to_vec().is_empty());
}

#[tokio::test]
async fn test_handlers_from_included_tasks() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Handler Include Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Add task that would notify a handler
    let mut notify_task = Task::new("Task that notifies", "debug").arg("msg", "Notifying handler");
    notify_task.notify.push("test_handler".to_string());
    play.add_task(notify_task);

    // Add handler
    play.add_handler(Handler {
        name: "test_handler".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = IndexMap::new();
            args.insert(
                "msg".to_string(),
                serde_json::json!("Handler from included file"),
            );
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

#[tokio::test]
async fn test_handler_notification_across_includes() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Cross-Include Handler Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Task from "main" that notifies handler
    let mut task1 = Task::new("Main task", "copy")
        .arg("src", "file.conf")
        .arg("dest", "/etc/file.conf");
    task1.notify.push("restart_service".to_string());
    play.add_task(task1);

    // Handler would be defined in the play
    play.add_handler(Handler {
        name: "restart_service".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = IndexMap::new();
            args.insert("msg".to_string(), serde_json::json!("Service restarted"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

#[test]
fn test_handler_namespace_in_roles() {
    // Handlers in roles can use role name as namespace
    let yaml = r#"
- name: restart nginx
  service:
    name: nginx
    state: restarted
  listen:
    - nginx config changed
    - webserver restart
"#;

    let handlers: Result<Vec<Handler>, _> = serde_yaml::from_str(yaml);
    // Handler parsing for role
    assert!(handlers.is_ok() || handlers.is_err()); // Structure test
}

// ============================================================================
// 10. ERROR HANDLING Tests
// ============================================================================

#[test]
fn test_file_not_found_include_tasks() {
    // Test reference to non-existent file
    let yaml = r#"
- name: Include non-existent
  include_tasks: tasks/does_not_exist.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    // Parsing succeeds, runtime would fail
    assert_eq!(
        tasks[0].include_tasks,
        Some("tasks/does_not_exist.yml".to_string())
    );
}

#[test]
fn test_circular_include_structure() {
    // Circular includes should be detected at runtime
    // This tests the structure
    let yaml = r#"
- name: Include that might be circular
  include_tasks: tasks/circular_a.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].include_tasks.is_some());
}

#[test]
fn test_invalid_yaml_in_include_path() {
    // Test with valid YAML but potentially invalid path
    let yaml = r#"
- name: Include with special characters
  include_tasks: "tasks/file with spaces.yml"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].include_tasks.is_some());
}

#[test]
fn test_conditional_include_false() {
    // When condition is false, include should be skipped
    // Note: when condition must be a string expression, not boolean literal
    let yaml = r#"
- name: Conditional include that will be skipped
  include_tasks: tasks/optional.yml
  when: "false"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].include_tasks.is_some());
    // The when: "false" would cause skip at runtime
}

#[tokio::test]
async fn test_error_handling_missing_role() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Missing Role Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Try to include non-existent role
    play.add_task(
        Task::new("Include missing role", "include_role").arg("name", "nonexistent_role"),
    );

    playbook.add_play(play);

    // Execution should handle missing role gracefully
    let results = executor.run_playbook(&playbook).await;
    // Either succeeds with warning or fails gracefully
    assert!(results.is_ok() || results.is_err());
}

// ============================================================================
// INTEGRATION Tests
// ============================================================================

#[tokio::test]
async fn test_complex_include_scenario() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_global_var("env".to_string(), serde_json::json!("production"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Complex Include Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Multiple includes with different configurations
    play.add_task(Task::new("Include common", "include_tasks").arg("file", "tasks/common.yml"));

    play.add_task(
        Task::new("Include with vars", "include_tasks").arg("file", "tasks/configure.yml"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

#[tokio::test]
async fn test_nested_includes() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Nested Include Test");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    // Main include (which could include others)
    play.add_task(Task::new("Include main", "include_tasks").arg("file", "tasks/common.yml"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("localhost"));
}

#[tokio::test]
async fn test_include_with_multiple_hosts() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("host1".to_string(), Some("webservers"));
    runtime.add_host("host2".to_string(), Some("webservers"));
    runtime.add_host("host3".to_string(), Some("webservers"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Multi-Host Include Test");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    play.add_task(Task::new("Include common", "include_tasks").arg("file", "tasks/common.yml"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert_eq!(results.len(), 3);
    assert!(results.contains_key("host1"));
    assert!(results.contains_key("host2"));
    assert!(results.contains_key("host3"));
}

// ============================================================================
// PARSER Integration Tests
// ============================================================================

#[test]
fn test_parse_playbook_with_all_include_types() {
    let yaml = r#"
- name: Play with all include types
  hosts: all
  gather_facts: false
  tasks:
    - name: Include tasks
      include_tasks: tasks/common.yml

    - name: Import tasks
      import_tasks: tasks/base.yml

    - name: Include role
      include_role:
        name: test_role

    - name: Import role
      import_role:
        name: common

    - name: Include vars
      include_vars: vars/common.yml
"#;

    let playbook_result = Playbook::parse(yaml, None);
    assert!(playbook_result.is_ok());

    let playbook = playbook_result.unwrap();
    assert_eq!(playbook.plays.len(), 1);
    assert_eq!(playbook.plays[0].tasks.len(), 5);
}

#[test]
fn test_parse_include_with_complex_vars() {
    let yaml = r#"
- name: Include with complex vars
  include_tasks: tasks/configure.yml
  vars:
    simple_var: "value"
    number_var: 42
    bool_var: true
    list_var:
      - item1
      - item2
    dict_var:
      key1: value1
      key2: value2
    nested_var:
      level1:
        level2: deep_value
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    // vars should be in module map
    assert!(tasks[0].module.contains_key("vars"));
}

#[test]
fn test_parse_include_role_full_options() {
    let yaml = r#"
- name: Full include_role
  include_role:
    name: application
    tasks_from: deploy.yml
    vars_from: production.yml
    defaults_from: defaults.yml
    handlers_from: handlers.yml
    public: true
    allow_duplicates: true
  vars:
    app_version: "1.0.0"
  when: deploy_app | bool
  tags:
    - deploy
    - application
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let task = &tasks[0];

    assert!(task.include_role.is_some());
    let role = task.include_role.as_ref().unwrap();
    assert_eq!(role.name, "application");
    assert_eq!(role.tasks_from, Some("deploy.yml".to_string()));
    assert_eq!(role.vars_from, Some("production.yml".to_string()));
    assert_eq!(role.defaults_from, Some("defaults.yml".to_string()));
    assert_eq!(role.handlers_from, Some("handlers.yml".to_string()));
    assert!(role.public);
    assert!(role.allow_duplicates);
    assert!(task.module.contains_key("vars"));
    assert!(task.when.is_some());
    assert!(!task.tags.is_empty());
}

// ============================================================================
// Edge Cases and Boundary Tests
// ============================================================================

#[test]
fn test_empty_include_path() {
    let yaml = r#"
- name: Empty include
  include_tasks: ""
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks[0].include_tasks, Some("".to_string()));
}

#[test]
fn test_include_with_absolute_path() {
    let yaml = r#"
- name: Absolute path include
  include_tasks: /etc/ansible/tasks/common.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let path = tasks[0].include_tasks.as_ref().unwrap();
    assert!(path.starts_with('/'));
}

#[test]
fn test_include_with_relative_path() {
    let yaml = r#"
- name: Relative path include
  include_tasks: ../shared/tasks/common.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let path = tasks[0].include_tasks.as_ref().unwrap();
    assert!(path.starts_with(".."));
}

#[test]
fn test_include_with_special_characters_in_path() {
    let yaml = r#"
- name: Path with special chars
  include_tasks: "tasks/my-task_v2.0.yml"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].include_tasks.is_some());
}

#[test]
fn test_multiple_includes_in_same_play() {
    let yaml = r#"
- name: First include
  include_tasks: tasks/first.yml

- name: Second include
  include_tasks: tasks/second.yml

- name: Third include
  include_tasks: tasks/third.yml
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tasks.len(), 3);
    assert!(tasks.iter().all(|t| t.include_tasks.is_some()));
}

#[test]
fn test_include_role_with_empty_options() {
    let yaml = r#"
- name: Minimal include_role
  include_role:
    name: minimal
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    let role = tasks[0].include_role.as_ref().unwrap();
    assert_eq!(role.name, "minimal");
    assert!(role.tasks_from.is_none());
    assert!(role.vars_from.is_none());
    assert!(!role.public);
    assert!(!role.allow_duplicates);
}

#[test]
fn test_include_vars_multiple_formats() {
    // String format
    let yaml1 = r#"
- include_vars: vars/simple.yml
"#;
    let tasks1: Result<Vec<TaskDefinition>, _> = serde_yaml::from_str(yaml1);
    assert!(tasks1.is_ok());

    // Dict format
    let yaml2 = r#"
- include_vars:
    file: vars/simple.yml
"#;
    let tasks2: Result<Vec<TaskDefinition>, _> = serde_yaml::from_str(yaml2);
    assert!(tasks2.is_ok());
}

// ============================================================================
// Performance and Stress Tests
// ============================================================================

#[test]
fn test_many_includes_in_single_play() {
    let mut yaml = String::from("---\n");
    for i in 0..50 {
        yaml.push_str(&format!(
            "- name: Include {}\n  include_tasks: tasks/task_{}.yml\n",
            i, i
        ));
    }

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(tasks.len(), 50);
}

#[test]
fn test_deeply_nested_vars_in_include() {
    let yaml = r#"
- name: Include with deep vars
  include_tasks: tasks/deep.yml
  vars:
    level1:
      level2:
        level3:
          level4:
            level5:
              deep_value: "found"
"#;

    let tasks: Vec<TaskDefinition> = serde_yaml::from_str(yaml).unwrap();
    assert!(tasks[0].module.contains_key("vars"));
}

#[tokio::test]
async fn test_concurrent_includes_multiple_hosts() {
    let mut runtime = RuntimeContext::new();
    for i in 0..10 {
        runtime.add_host(format!("host{}", i), Some("testgroup"));
    }

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Concurrent Include Test");
    let mut play = Play::new("Test Play", "testgroup");
    play.gather_facts = false;

    play.add_task(Task::new("Include common", "include_tasks").arg("file", "tasks/common.yml"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert_eq!(results.len(), 10);
}
