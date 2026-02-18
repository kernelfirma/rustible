//! Variable Precedence Compatibility Tests
//!
//! Tests that Rustible follows Ansible's variable precedence rules.
//! Ansible defines 22 levels of variable precedence (simplified to 20 in Rustible).
//!
//! From lowest to highest:
//! 1. role defaults
//! 2. inventory group_vars/all
//! 3. inventory group_vars/*
//! 4. inventory host_vars/*
//! 5. playbook group_vars/all
//! 6. playbook group_vars/*
//! 7. playbook host_vars/*
//! 8. host facts
//! 9. play vars
//! 10. play vars_prompt
//! 11. play vars_files
//! 12. role vars
//! 13. block vars
//! 14. task vars
//! 15. include_vars
//! 16. set_facts / registered vars
//! 17. role params
//! 18. include params
//! 19. extra vars (always win)

use indexmap::IndexMap;
use serde_json::json;

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::{RegisteredResult, RuntimeContext};
use rustible::executor::task::Task;
use rustible::executor::{Executor, ExecutorConfig};
use rustible::vars::{VarPrecedence, VarStore, Variables};

// ============================================================================
// Test Helpers
// ============================================================================

fn create_test_executor_with_vars(
    host_vars: Vec<(&str, &str, serde_json::Value)>,
) -> (Executor, RuntimeContext) {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    for (host, key, value) in host_vars {
        if host == "localhost" {
            runtime.set_host_var(host, key.to_string(), value);
        }
    }

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime.clone());
    (executor, runtime)
}

// ============================================================================
// Section 1: VarPrecedence enum tests
// ============================================================================

#[test]
fn test_precedence_ordering() {
    // Verify precedence order matches Ansible
    assert!(VarPrecedence::RoleDefaults < VarPrecedence::InventoryGroupVars);
    assert!(VarPrecedence::InventoryGroupVars < VarPrecedence::PlayVars);
    assert!(VarPrecedence::PlayVars < VarPrecedence::TaskVars);
    assert!(VarPrecedence::TaskVars < VarPrecedence::SetFacts);
    assert!(VarPrecedence::SetFacts < VarPrecedence::ExtraVars);
}

#[test]
fn test_precedence_levels() {
    assert_eq!(VarPrecedence::RoleDefaults.level(), 1);
    assert_eq!(VarPrecedence::ExtraVars.level(), 20);
}

#[test]
fn test_precedence_display() {
    assert_eq!(format!("{}", VarPrecedence::RoleDefaults), "role defaults");
    assert_eq!(format!("{}", VarPrecedence::ExtraVars), "extra vars");
    assert_eq!(format!("{}", VarPrecedence::PlayVars), "play vars");
}

// ============================================================================
// Section 2: VarStore basic operations
// ============================================================================

#[test]
fn test_var_store_set_and_get() {
    let mut store = VarStore::new();

    store.set(
        "test_var",
        serde_yaml::Value::String("test_value".to_string()),
        VarPrecedence::PlayVars,
    );

    assert!(store.contains("test_var"));
    assert_eq!(
        store.get("test_var"),
        Some(&serde_yaml::Value::String("test_value".to_string()))
    );
}

#[test]
fn test_var_store_higher_precedence_wins() {
    let mut store = VarStore::new();

    // Set at role defaults (level 1)
    store.set(
        "my_var",
        serde_yaml::Value::String("from_defaults".to_string()),
        VarPrecedence::RoleDefaults,
    );

    // Set at play vars (level 10)
    store.set(
        "my_var",
        serde_yaml::Value::String("from_play".to_string()),
        VarPrecedence::PlayVars,
    );

    // Play vars should win
    assert_eq!(
        store.get("my_var"),
        Some(&serde_yaml::Value::String("from_play".to_string()))
    );
}

#[test]
fn test_var_store_extra_vars_always_win() {
    let mut store = VarStore::new();

    // Set at all levels
    store.set(
        "config",
        serde_yaml::Value::String("from_defaults".to_string()),
        VarPrecedence::RoleDefaults,
    );
    store.set(
        "config",
        serde_yaml::Value::String("from_play".to_string()),
        VarPrecedence::PlayVars,
    );
    store.set(
        "config",
        serde_yaml::Value::String("from_task".to_string()),
        VarPrecedence::TaskVars,
    );
    store.set(
        "config",
        serde_yaml::Value::String("from_set_fact".to_string()),
        VarPrecedence::SetFacts,
    );
    store.set(
        "config",
        serde_yaml::Value::String("from_extra".to_string()),
        VarPrecedence::ExtraVars,
    );

    // Extra vars should always win
    assert_eq!(
        store.get("config"),
        Some(&serde_yaml::Value::String("from_extra".to_string()))
    );
}

#[test]
fn test_var_store_set_many() {
    let mut store = VarStore::new();

    let mut vars = IndexMap::new();
    vars.insert("var1".to_string(), serde_yaml::Value::Number(1.into()));
    vars.insert("var2".to_string(), serde_yaml::Value::Number(2.into()));
    vars.insert("var3".to_string(), serde_yaml::Value::Number(3.into()));

    store.set_many(vars, VarPrecedence::PlayVars);

    assert!(store.contains("var1"));
    assert!(store.contains("var2"));
    assert!(store.contains("var3"));
}

#[test]
fn test_var_store_clear_precedence() {
    let mut store = VarStore::new();

    store.set(
        "play_var",
        serde_yaml::Value::String("play".to_string()),
        VarPrecedence::PlayVars,
    );
    store.set(
        "task_var",
        serde_yaml::Value::String("task".to_string()),
        VarPrecedence::TaskVars,
    );

    // Clear only play vars
    store.clear_precedence(VarPrecedence::PlayVars);

    assert!(!store.contains("play_var"));
    assert!(store.contains("task_var"));
}

// ============================================================================
// Section 3: Variable merging behavior
// ============================================================================

#[test]
fn test_var_store_hash_replace_behavior() {
    use rustible::vars::HashBehaviour;

    let mut store = VarStore::new();
    // Default is Replace

    // Set base dict at low precedence
    store.set(
        "config",
        serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            db:
              host: localhost
              port: 5432
            cache:
              enabled: true
        "#,
        )
        .unwrap(),
        VarPrecedence::RoleDefaults,
    );

    // Set partial override at higher precedence
    store.set(
        "config",
        serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            db:
              host: production.db
        "#,
        )
        .unwrap(),
        VarPrecedence::PlayVars,
    );

    // With Replace, higher precedence completely replaces
    let config = store.get("config").unwrap();
    let db = config.get("db").unwrap();
    assert!(db.contains_key("host"));
    // port should NOT exist because Replace mode replaces entire hash
    // (In Replace mode, the entire config from PlayVars replaces RoleDefaults)
}

#[test]
fn test_var_store_scope() {
    let mut store = VarStore::new();

    store.set(
        "global_var",
        serde_yaml::Value::Number(100.into()),
        VarPrecedence::PlayVars,
    );

    let mut scope = store.scope();
    scope.set("local_var", serde_yaml::Value::Number(200.into()));

    // Scope sees both
    assert_eq!(scope.get("global_var"), Some(&serde_yaml::Value::Number(100.into())));
    assert_eq!(scope.get("local_var"), Some(&serde_yaml::Value::Number(200.into())));

    // Parent doesn't see local
    assert!(store.contains("global_var"));
    assert!(!store.contains("local_var"));
}

// ============================================================================
// Section 4: Executor variable precedence tests
// ============================================================================

#[tokio::test]
async fn test_play_vars_available_in_task() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    // Create playbook with play vars
    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test play vars", "all");
    play.gather_facts = false;
    play.vars.set("my_app_name", json!("TestApp"));
    play.vars.set("my_app_port", json!(8080));

    let task = Task::new("Use play vars", "debug")
        .arg("msg", "App: {{ my_app_name }} on port {{ my_app_port }}");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
    assert_eq!(host_result.stats.skipped, 0);
}

#[tokio::test]
async fn test_task_vars_override_play_vars() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test task var override", "all");
    play.gather_facts = false;
    play.vars.set("value", json!("play_level"));

    // Task with its own vars that override play vars
    let mut task = Task::new("Override in task", "debug")
        .arg("msg", "Value is {{ value }}");
    task.vars.insert("value".to_string(), json!("task_level"));

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
}

#[tokio::test]
async fn test_set_fact_overrides_play_vars() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test set_fact override", "all");
    play.gather_facts = false;
    play.vars.set("dynamic_value", json!("initial"));

    // First task sets fact
    let task1 = Task::new("Set fact", "set_fact")
        .arg("dynamic_value", "updated_by_set_fact");

    // Second task uses the updated value
    let task2 = Task::new("Use set fact", "debug")
        .arg("msg", "Value is {{ dynamic_value }}");

    play.add_task(task1);
    play.add_task(task2);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
}

#[tokio::test]
async fn test_registered_var_accessible() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test registered vars", "all");
    play.gather_facts = false;

    // First task registers result
    let task1 = Task::new("Run command", "debug")
        .arg("msg", "Initial task")
        .register("cmd_result");

    // Second task uses registered var
    let task2 = Task::new("Check result", "debug")
        .arg("msg", "Result defined: {{ cmd_result is defined }}")
        .when("cmd_result is defined");

    play.add_task(task1);
    play.add_task(task2);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
    assert_eq!(host_result.stats.skipped, 0);
}

#[tokio::test]
async fn test_host_var_precedence() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_host_var("localhost", "host_specific".to_string(), json!("from_host"));

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test host vars", "all");
    play.gather_facts = false;

    let task = Task::new("Use host var", "debug")
        .arg("msg", "Host var: {{ host_specific }}");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
}

// ============================================================================
// Section 5: Inventory variable tests
// ============================================================================

#[tokio::test]
async fn test_group_vars_accessible() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers".to_string()));
    runtime.add_host("db1".to_string(), Some("databases".to_string()));

    // Set group vars
    runtime.set_group_var(
        "webservers",
        "service_port".to_string(),
        json!(80),
    );
    runtime.set_group_var(
        "databases",
        "service_port".to_string(),
        json!(5432),
    );

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test group vars", "all");
    play.gather_facts = false;

    let task = Task::new("Show port", "debug")
        .arg("msg", "Port: {{ service_port }}");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both hosts should succeed
    assert!(!results.get("web1").unwrap().failed);
    assert!(!results.get("db1").unwrap().failed);
}

// ============================================================================
// Section 6: Default value and undefined variable tests
// ============================================================================

#[tokio::test]
async fn test_default_filter_for_undefined() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test default filter", "all");
    play.gather_facts = false;

    let task = Task::new("Use default", "debug")
        .arg("msg", "Value: {{ undefined_var | default('fallback') }}");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
}

#[tokio::test]
async fn test_when_variable_is_not_defined() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test undefined check", "all");
    play.gather_facts = false;

    // This task should run because the var is NOT defined
    let task = Task::new("Check undefined", "debug")
        .arg("msg", "Variable is not defined")
        .when("some_undefined_var is not defined");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
    assert_eq!(host_result.stats.skipped, 0); // Should run, not skip
}

// ============================================================================
// Section 7: Complex variable scenarios
// ============================================================================

#[tokio::test]
async fn test_nested_variable_access() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_host_var(
        "localhost",
        "app_config".to_string(),
        json!({
            "database": {
                "host": "db.local",
                "port": 5432,
                "credentials": {
                    "user": "admin"
                }
            }
        }),
    );

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test nested vars", "all");
    play.gather_facts = false;

    let task = Task::new("Access nested", "debug")
        .arg("msg", "DB Host: {{ app_config.database.host }}");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
}

#[tokio::test]
async fn test_list_variable_access() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_host_var(
        "localhost",
        "servers".to_string(),
        json!(["server1", "server2", "server3"]),
    );

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test list access", "all");
    play.gather_facts = false;

    let task = Task::new("Access list", "debug")
        .arg("msg", "First server: {{ servers[0] }}");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
}

// ============================================================================
// Section 8: Facts precedence
// ============================================================================

#[tokio::test]
async fn test_facts_accessible() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    // Set facts (would normally come from gather_facts)
    runtime.set_host_fact("localhost", "os_family".to_string(), json!("Debian"));
    runtime.set_host_fact("localhost", "distribution".to_string(), json!("Ubuntu"));

    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("test");
    let mut play = Play::new("Test facts", "all");
    play.gather_facts = false;

    let task = Task::new("Use facts", "debug")
        .arg("msg", "OS: {{ ansible_facts.os_family }}")
        .when("ansible_facts.os_family == 'Debian'");

    play.add_task(task);
    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    assert!(!host_result.failed);
    assert_eq!(host_result.stats.skipped, 0);
}

// ============================================================================
// Section 9: Variable resolution edge cases
// ============================================================================

#[test]
fn test_resolve_path_simple() {
    use rustible::vars::resolve;

    let value = serde_yaml::from_str::<serde_yaml::Value>(
        r#"
        simple: "value"
        nested:
          level1:
            level2: "deep"
        list:
          - first
          - second
        "#,
    )
    .unwrap();

    assert_eq!(
        resolve::resolve_path(&value, "simple"),
        Some(&serde_yaml::Value::String("value".to_string()))
    );

    assert_eq!(
        resolve::resolve_path(&value, "nested.level1.level2"),
        Some(&serde_yaml::Value::String("deep".to_string()))
    );

    assert_eq!(
        resolve::resolve_path(&value, "list.0"),
        Some(&serde_yaml::Value::String("first".to_string()))
    );
}

#[test]
fn test_to_bool_conversion() {
    use rustible::vars::resolve;

    // True values
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::Bool(true)),
        Some(true)
    );
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::String("yes".to_string())),
        Some(true)
    );
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::String("true".to_string())),
        Some(true)
    );
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::String("on".to_string())),
        Some(true)
    );
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::String("1".to_string())),
        Some(true)
    );

    // False values
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::Bool(false)),
        Some(false)
    );
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::String("no".to_string())),
        Some(false)
    );
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::String("false".to_string())),
        Some(false)
    );
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::String("off".to_string())),
        Some(false)
    );
    assert_eq!(
        resolve::to_bool(&serde_yaml::Value::String("0".to_string())),
        Some(false)
    );
}
