//! Tests for include_tasks, import_tasks, and include_vars functionality

use rustible::include::{ImportTasksSpec, IncludeTasksSpec, TaskIncluder};
use rustible::vars::{VarPrecedence, VarStore};
use std::io::Write;
use tempfile::TempDir;

#[tokio::test]
async fn test_include_tasks_creates_separate_scope() {
    let temp_dir = TempDir::new().unwrap();
    let tasks_file = temp_dir.path().join("included.yml");

    let mut file = std::fs::File::create(&tasks_file).unwrap();
    write!(
        file,
        r#"
- name: Included task 1
  debug:
    msg: "Task with {{ include_var }}"

- name: Included task 2
  debug:
    msg: "Another task"
"#
    )
    .unwrap();

    let includer = TaskIncluder::new(temp_dir.path());
    let spec = IncludeTasksSpec::new("included.yml")
        .with_var("include_var", serde_json::json!("scoped_value"));

    let mut parent_vars = VarStore::new();
    parent_vars.set(
        "parent_var",
        serde_yaml::Value::String("parent_value".to_string()),
        VarPrecedence::PlayVars,
    );

    let (tasks, mut scope) = includer
        .load_include_tasks(&spec, &parent_vars)
        .await
        .unwrap();

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].name, "Included task 1");

    // Scope should have both parent and include vars
    assert!(scope.contains("parent_var"));
    assert!(scope.contains("include_var"));

    // Parent vars should NOT have include_var (separate scope)
    assert!(!parent_vars.contains("include_var"));
}

#[tokio::test]
async fn test_import_tasks_merges_into_parent() {
    let temp_dir = TempDir::new().unwrap();
    let tasks_file = temp_dir.path().join("imported.yml");

    let mut file = std::fs::File::create(&tasks_file).unwrap();
    write!(
        file,
        r#"
- name: Imported task
  debug:
    msg: "Using {{ import_var }}"
"#
    )
    .unwrap();

    let includer = TaskIncluder::new(temp_dir.path());
    let spec = ImportTasksSpec::new("imported.yml")
        .with_var("import_var", serde_json::json!("merged_value"));

    let mut parent_vars = VarStore::new();
    parent_vars.set(
        "parent_var",
        serde_yaml::Value::String("parent_value".to_string()),
        VarPrecedence::PlayVars,
    );

    let tasks = includer
        .load_import_tasks(&spec, &mut parent_vars)
        .await
        .unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "Imported task");

    // Parent vars SHOULD have import_var (merged scope)
    assert!(parent_vars.contains("import_var"));
    assert!(parent_vars.contains("parent_var"));
}

#[tokio::test]
async fn test_include_vars_loads_variable_file() {
    let temp_dir = TempDir::new().unwrap();
    let vars_file = temp_dir.path().join("variables.yml");

    let mut file = std::fs::File::create(&vars_file).unwrap();
    write!(
        file,
        r#"
app_name: "my_application"
app_version: "1.2.3"
app_config:
  debug: true
  port: 8080
"#
    )
    .unwrap();

    let includer = TaskIncluder::new(temp_dir.path());
    let mut var_store = VarStore::new();

    includer
        .load_vars_from_file("variables.yml", &mut var_store)
        .await
        .unwrap();

    assert!(var_store.contains("app_name"));
    assert!(var_store.contains("app_version"));
    assert!(var_store.contains("app_config"));

    let app_name = var_store.get("app_name").unwrap();
    assert_eq!(
        app_name,
        &serde_yaml::Value::String("my_application".to_string())
    );
}

#[tokio::test]
async fn test_include_vars_precedence() {
    let temp_dir = TempDir::new().unwrap();
    let vars_file = temp_dir.path().join("override.yml");

    let mut file = std::fs::File::create(&vars_file).unwrap();
    write!(
        file,
        r#"
override_var: "included_value"
"#
    )
    .unwrap();

    let mut var_store = VarStore::new();

    // Set a variable at lower precedence
    var_store.set(
        "override_var",
        serde_yaml::Value::String("play_value".to_string()),
        VarPrecedence::PlayVars,
    );

    let includer = TaskIncluder::new(temp_dir.path());
    includer
        .load_vars_from_file("override.yml", &mut var_store)
        .await
        .unwrap();

    // IncludeVars (precedence 16) should override PlayVars (precedence 10)
    let value = var_store.get("override_var").unwrap();
    assert_eq!(
        value,
        &serde_yaml::Value::String("included_value".to_string())
    );
}

#[tokio::test]
async fn test_nested_includes() {
    let temp_dir = TempDir::new().unwrap();

    // Create first level include
    let level1_file = temp_dir.path().join("level1.yml");
    let mut file = std::fs::File::create(&level1_file).unwrap();
    write!(
        file,
        r#"
- name: Level 1 task
  debug:
    msg: "Level 1"
"#
    )
    .unwrap();

    // Create second level include
    let level2_file = temp_dir.path().join("level2.yml");
    let mut file = std::fs::File::create(&level2_file).unwrap();
    write!(
        file,
        r#"
- name: Level 2 task
  debug:
    msg: "Level 2"
"#
    )
    .unwrap();

    let includer = TaskIncluder::new(temp_dir.path());

    // Load level 1
    let spec1 = IncludeTasksSpec::new("level1.yml");
    let parent_vars = VarStore::new();
    let (tasks1, scope1) = includer
        .load_include_tasks(&spec1, &parent_vars)
        .await
        .unwrap();
    assert_eq!(tasks1.len(), 1);

    // Load level 2 with scope from level 1
    let spec2 = IncludeTasksSpec::new("level2.yml");
    let (tasks2, _scope2) = includer.load_include_tasks(&spec2, &scope1).await.unwrap();
    assert_eq!(tasks2.len(), 1);
}

#[tokio::test]
async fn test_include_with_multiple_vars() {
    let temp_dir = TempDir::new().unwrap();
    let tasks_file = temp_dir.path().join("multi_var.yml");

    let mut file = std::fs::File::create(&tasks_file).unwrap();
    write!(
        file,
        r#"
- name: Multi var task
  debug:
    msg: "{{ var1 }} {{ var2 }} {{ var3 }}"
"#
    )
    .unwrap();

    let includer = TaskIncluder::new(temp_dir.path());
    let spec = IncludeTasksSpec::new("multi_var.yml")
        .with_var("var1", serde_json::json!("value1"))
        .with_var("var2", serde_json::json!(123))
        .with_var("var3", serde_json::json!(true));

    let parent_vars = VarStore::new();
    let (_tasks, mut scope) = includer
        .load_include_tasks(&spec, &parent_vars)
        .await
        .unwrap();

    assert!(scope.contains("var1"));
    assert!(scope.contains("var2"));
    assert!(scope.contains("var3"));
}

#[tokio::test]
async fn test_include_tasks_file_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let includer = TaskIncluder::new(temp_dir.path());

    let spec = IncludeTasksSpec::new("nonexistent.yml");
    let parent_vars = VarStore::new();

    let result = includer.load_include_tasks(&spec, &parent_vars).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_include_vars_file_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let includer = TaskIncluder::new(temp_dir.path());
    let mut var_store = VarStore::new();

    let result = includer
        .load_vars_from_file("nonexistent.yml", &mut var_store)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_absolute_path_include() {
    let temp_dir = TempDir::new().unwrap();
    let tasks_file = temp_dir.path().join("absolute.yml");

    let mut file = std::fs::File::create(&tasks_file).unwrap();
    write!(
        file,
        r#"
- name: Absolute path task
  debug:
    msg: "Loaded"
"#
    )
    .unwrap();

    // Use absolute path - base directory must contain the temp file
    // (on macOS, /tmp is symlinked to /private/tmp, and tempdir uses /var/folders which
    // resolves to /private/var/folders, so we need to use temp_dir.path() as base)
    let includer = TaskIncluder::new(temp_dir.path().to_str().unwrap());
    let spec = IncludeTasksSpec::new(tasks_file.to_str().unwrap());

    let parent_vars = VarStore::new();
    let (tasks, _) = includer
        .load_include_tasks(&spec, &parent_vars)
        .await
        .unwrap();

    assert_eq!(tasks.len(), 1);
}

#[tokio::test]
async fn test_include_vars_complex_structure() {
    let temp_dir = TempDir::new().unwrap();
    let vars_file = temp_dir.path().join("complex.yml");

    let mut file = std::fs::File::create(&vars_file).unwrap();
    write!(
        file,
        r#"
database:
  host: "localhost"
  port: 5432
  credentials:
    username: "admin"
    password: "secret"

servers:
  - name: "web1"
    ip: "192.168.1.10"
  - name: "web2"
    ip: "192.168.1.11"
"#
    )
    .unwrap();

    let includer = TaskIncluder::new(temp_dir.path());
    let mut var_store = VarStore::new();

    includer
        .load_vars_from_file("complex.yml", &mut var_store)
        .await
        .unwrap();

    assert!(var_store.contains("database"));
    assert!(var_store.contains("servers"));
}
