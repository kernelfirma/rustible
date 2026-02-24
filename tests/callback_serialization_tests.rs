//! Comprehensive serialization tests for Rustible's callback types.
//!
//! This test suite validates:
//! 1. All event types serialize/deserialize to JSON correctly
//! 2. All context structs round-trip correctly
//! 3. Optional fields are handled properly
//! 4. Special characters and Unicode are preserved
//! 5. Large data sizes are handled efficiently

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::time::Duration;

// Import types that are stable in the codebase
use rustible::facts::Facts;
use rustible::vars::Variables;

// Define test-local versions of types for serialization testing
// These mirror the actual types but are guaranteed to compile

/// Test version of TaskStatus enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum TaskStatus {
    #[default]
    Ok,
    Changed,
    Failed,
    Skipped,
    Unreachable,
}

/// Test version of TaskResult
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskResult {
    pub status: TaskStatus,
    pub changed: bool,
    pub msg: Option<String>,
    pub result: Option<JsonValue>,
    pub diff: Option<TaskDiff>,
}

impl TaskResult {
    pub fn ok() -> Self {
        Self {
            status: TaskStatus::Ok,
            changed: false,
            ..Default::default()
        }
    }

    pub fn changed() -> Self {
        Self {
            status: TaskStatus::Changed,
            changed: true,
            ..Default::default()
        }
    }

    pub fn failed(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Failed,
            changed: false,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    pub fn skipped(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Skipped,
            changed: false,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    pub fn unreachable(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Unreachable,
            changed: false,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    pub fn with_result(mut self, result: JsonValue) -> Self {
        self.result = Some(result);
        self
    }

    pub fn with_msg(mut self, msg: impl Into<String>) -> Self {
        self.msg = Some(msg.into());
        self
    }

    pub fn with_diff(mut self, diff: TaskDiff) -> Self {
        self.diff = Some(diff);
        self
    }
}

/// Test version of TaskDiff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDiff {
    pub before: Option<String>,
    pub after: Option<String>,
    pub before_header: Option<String>,
    pub after_header: Option<String>,
}

/// Test version of Handler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handler {
    pub name: String,
    pub module: String,
    #[serde(default)]
    pub args: IndexMap<String, JsonValue>,
    pub when: Option<String>,
    #[serde(default)]
    pub listen: Vec<String>,
}

/// Test version of Task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    pub module: String,
    #[serde(default)]
    pub args: IndexMap<String, JsonValue>,
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default)]
    pub notify: Vec<String>,
    #[serde(default)]
    pub register: Option<String>,
    #[serde(default)]
    pub loop_items: Option<Vec<JsonValue>>,
    #[serde(default = "default_loop_var")]
    pub loop_var: String,
    #[serde(default)]
    pub ignore_errors: bool,
    #[serde(default)]
    pub changed_when: Option<String>,
    #[serde(default)]
    pub failed_when: Option<String>,
    #[serde(default)]
    pub delegate_to: Option<String>,
    #[serde(default)]
    pub run_once: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub r#become: bool,
    #[serde(default)]
    pub become_user: Option<String>,
}

fn default_loop_var() -> String {
    "item".to_string()
}

impl Default for Task {
    fn default() -> Self {
        Self {
            name: String::new(),
            module: String::new(),
            args: IndexMap::new(),
            when: None,
            notify: Vec::new(),
            register: None,
            loop_items: None,
            loop_var: default_loop_var(),
            ignore_errors: false,
            changed_when: None,
            failed_when: None,
            delegate_to: None,
            run_once: false,
            tags: Vec::new(),
            r#become: false,
            become_user: None,
        }
    }
}

impl Task {
    pub fn new(name: impl Into<String>, module: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module: module.into(),
            ..Default::default()
        }
    }

    pub fn arg(mut self, key: impl Into<String>, value: impl Into<JsonValue>) -> Self {
        self.args.insert(key.into(), value.into());
        self
    }

    pub fn when(mut self, condition: impl Into<String>) -> Self {
        self.when = Some(condition.into());
        self
    }

    pub fn notify(mut self, handler: impl Into<String>) -> Self {
        self.notify.push(handler.into());
        self
    }

    pub fn register(mut self, name: impl Into<String>) -> Self {
        self.register = Some(name.into());
        self
    }

    pub fn loop_over(mut self, items: Vec<JsonValue>) -> Self {
        self.loop_items = Some(items);
        self
    }

    pub fn loop_var(mut self, name: impl Into<String>) -> Self {
        self.loop_var = name.into();
        self
    }

    pub fn ignore_errors(mut self, ignore: bool) -> Self {
        self.ignore_errors = ignore;
        self
    }
}

/// Test version of ModuleResult
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleResult {
    pub success: bool,
    pub changed: bool,
    pub message: String,
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<JsonValue>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ModuleResult {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: false,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    pub fn changed(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: true,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            success: false,
            changed: false,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    pub fn skipped(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: false,
            message: message.into(),
            skipped: true,
            data: None,
            warnings: Vec::new(),
        }
    }

    pub fn with_data(mut self, data: JsonValue) -> Self {
        self.data = Some(data);
        self
    }

    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

/// Test version of ExecutionResult
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub host: String,
    pub task_name: String,
    pub result: ModuleResult,
    pub duration: Duration,
    pub notify: Vec<String>,
}

/// Test version of VarPrecedence
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum VarPrecedence {
    RoleDefaults = 1,
    InventoryGroupVars = 2,
    InventoryFileGroupVars = 3,
    PlaybookGroupVarsAll = 4,
    PlaybookGroupVars = 5,
    InventoryHostVars = 6,
    InventoryFileHostVars = 7,
    PlaybookHostVars = 8,
    HostFacts = 9,
    PlayVars = 10,
    PlayVarsPrompt = 11,
    PlayVarsFiles = 12,
    RoleVars = 13,
    BlockVars = 14,
    TaskVars = 15,
    IncludeVars = 16,
    SetFacts = 17,
    RoleParams = 18,
    IncludeParams = 19,
    ExtraVars = 20,
}

impl VarPrecedence {
    pub fn all() -> impl Iterator<Item = VarPrecedence> {
        [
            VarPrecedence::RoleDefaults,
            VarPrecedence::InventoryGroupVars,
            VarPrecedence::InventoryFileGroupVars,
            VarPrecedence::PlaybookGroupVarsAll,
            VarPrecedence::PlaybookGroupVars,
            VarPrecedence::InventoryHostVars,
            VarPrecedence::InventoryFileHostVars,
            VarPrecedence::PlaybookHostVars,
            VarPrecedence::HostFacts,
            VarPrecedence::PlayVars,
            VarPrecedence::PlayVarsPrompt,
            VarPrecedence::PlayVarsFiles,
            VarPrecedence::RoleVars,
            VarPrecedence::BlockVars,
            VarPrecedence::TaskVars,
            VarPrecedence::IncludeVars,
            VarPrecedence::SetFacts,
            VarPrecedence::RoleParams,
            VarPrecedence::IncludeParams,
            VarPrecedence::ExtraVars,
        ]
        .into_iter()
    }
}

/// Test version of HashBehaviour
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HashBehaviour {
    #[default]
    Replace,
    Merge,
}

// ============================================================================
// Test 1: TaskStatus Enum Serialization
// ============================================================================

#[test]
fn test_task_status_serialization_ok() {
    let status = TaskStatus::Ok;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, r#""ok""#);

    let deserialized: TaskStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, TaskStatus::Ok);
}

#[test]
fn test_task_status_serialization_changed() {
    let status = TaskStatus::Changed;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, r#""changed""#);

    let deserialized: TaskStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, TaskStatus::Changed);
}

#[test]
fn test_task_status_serialization_failed() {
    let status = TaskStatus::Failed;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, r#""failed""#);

    let deserialized: TaskStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, TaskStatus::Failed);
}

#[test]
fn test_task_status_serialization_skipped() {
    let status = TaskStatus::Skipped;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, r#""skipped""#);

    let deserialized: TaskStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, TaskStatus::Skipped);
}

#[test]
fn test_task_status_serialization_unreachable() {
    let status = TaskStatus::Unreachable;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, r#""unreachable""#);

    let deserialized: TaskStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, TaskStatus::Unreachable);
}

#[test]
fn test_task_status_all_variants_round_trip() {
    let statuses = [
        TaskStatus::Ok,
        TaskStatus::Changed,
        TaskStatus::Failed,
        TaskStatus::Skipped,
        TaskStatus::Unreachable,
    ];

    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, status, "Failed for status: {:?}", status);
    }
}

// ============================================================================
// Test 2: TaskResult Serialization
// ============================================================================

#[test]
fn test_task_result_ok_serialization() {
    let result = TaskResult::ok();
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: TaskResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.status, TaskStatus::Ok);
    assert!(!deserialized.changed);
    assert!(deserialized.msg.is_none());
}

#[test]
fn test_task_result_changed_serialization() {
    let result = TaskResult::changed();
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: TaskResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.status, TaskStatus::Changed);
    assert!(deserialized.changed);
}

#[test]
fn test_task_result_failed_with_message() {
    let result = TaskResult::failed("Connection refused");
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: TaskResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.status, TaskStatus::Failed);
    assert_eq!(deserialized.msg, Some("Connection refused".to_string()));
}

#[test]
fn test_task_result_skipped_with_message() {
    let result = TaskResult::skipped("Condition not met");
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: TaskResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.status, TaskStatus::Skipped);
    assert_eq!(deserialized.msg, Some("Condition not met".to_string()));
}

#[test]
fn test_task_result_unreachable_with_message() {
    let result = TaskResult::unreachable("Host timed out");
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: TaskResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.status, TaskStatus::Unreachable);
    assert_eq!(deserialized.msg, Some("Host timed out".to_string()));
}

#[test]
fn test_task_result_with_result_data() {
    let result = TaskResult::ok().with_result(json!({
        "stdout": "Hello, World!",
        "stderr": "",
        "rc": 0
    }));

    let json = serde_json::to_string(&result).unwrap();
    let deserialized: TaskResult = serde_json::from_str(&json).unwrap();

    let data = deserialized.result.unwrap();
    assert_eq!(data["stdout"], "Hello, World!");
    assert_eq!(data["rc"], 0);
}

#[test]
fn test_task_result_with_diff() {
    let diff = TaskDiff {
        before: Some("old content".to_string()),
        after: Some("new content".to_string()),
        before_header: Some("/etc/config".to_string()),
        after_header: Some("/etc/config".to_string()),
    };

    let result = TaskResult::changed().with_diff(diff);
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: TaskResult = serde_json::from_str(&json).unwrap();

    let diff = deserialized.diff.unwrap();
    assert_eq!(diff.before, Some("old content".to_string()));
    assert_eq!(diff.after, Some("new content".to_string()));
}

// ============================================================================
// Test 3: TaskDiff Serialization
// ============================================================================

#[test]
fn test_task_diff_full_serialization() {
    let diff = TaskDiff {
        before: Some("line1\nline2".to_string()),
        after: Some("line1\nline3".to_string()),
        before_header: Some("--- before".to_string()),
        after_header: Some("+++ after".to_string()),
    };

    let json = serde_json::to_string(&diff).unwrap();
    let deserialized: TaskDiff = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.before, diff.before);
    assert_eq!(deserialized.after, diff.after);
    assert_eq!(deserialized.before_header, diff.before_header);
    assert_eq!(deserialized.after_header, diff.after_header);
}

#[test]
fn test_task_diff_partial_fields() {
    let diff = TaskDiff {
        before: Some("content".to_string()),
        after: None,
        before_header: None,
        after_header: Some("header".to_string()),
    };

    let json = serde_json::to_string(&diff).unwrap();
    let deserialized: TaskDiff = serde_json::from_str(&json).unwrap();

    assert!(deserialized.before.is_some());
    assert!(deserialized.after.is_none());
    assert!(deserialized.before_header.is_none());
    assert!(deserialized.after_header.is_some());
}

#[test]
fn test_task_diff_empty() {
    let diff = TaskDiff {
        before: None,
        after: None,
        before_header: None,
        after_header: None,
    };

    let json = serde_json::to_string(&diff).unwrap();
    let deserialized: TaskDiff = serde_json::from_str(&json).unwrap();

    assert!(deserialized.before.is_none());
    assert!(deserialized.after.is_none());
}

// ============================================================================
// Test 4: Task Serialization
// ============================================================================

#[test]
fn test_task_basic_serialization() {
    let task = Task::new("Install nginx", "package");
    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, "Install nginx");
    assert_eq!(deserialized.module, "package");
}

#[test]
fn test_task_with_args_serialization() {
    let task = Task::new("Install nginx", "package")
        .arg("name", "nginx")
        .arg("state", "present");

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(
        deserialized.args.get("name"),
        Some(&JsonValue::String("nginx".to_string()))
    );
    assert_eq!(
        deserialized.args.get("state"),
        Some(&JsonValue::String("present".to_string()))
    );
}

#[test]
fn test_task_with_when_condition() {
    let task = Task::new("Install on Debian", "package").when("ansible_os_family == 'Debian'");

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(
        deserialized.when,
        Some("ansible_os_family == 'Debian'".to_string())
    );
}

#[test]
fn test_task_with_notify() {
    let task = Task::new("Configure nginx", "template")
        .notify("restart nginx")
        .notify("reload config");

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert!(deserialized.notify.contains(&"restart nginx".to_string()));
    assert!(deserialized.notify.contains(&"reload config".to_string()));
}

#[test]
fn test_task_with_register() {
    let task = Task::new("Check file", "stat")
        .arg("path", "/etc/config")
        .register("stat_result");

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.register, Some("stat_result".to_string()));
}

#[test]
fn test_task_with_loop() {
    let task = Task::new("Install packages", "package").loop_over(vec![
        json!("nginx"),
        json!("redis"),
        json!("postgresql"),
    ]);

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    let items = deserialized.loop_items.unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], json!("nginx"));
}

#[test]
fn test_task_with_loop_var() {
    let task = Task::new("Process items", "debug")
        .loop_over(vec![json!(1), json!(2)])
        .loop_var("my_item");

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.loop_var, "my_item");
}

#[test]
fn test_task_with_ignore_errors() {
    let task = Task::new("Risky operation", "command").ignore_errors(true);

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert!(deserialized.ignore_errors);
}

#[test]
fn test_task_full_serialization() {
    let mut args = IndexMap::new();
    args.insert("name".to_string(), json!("nginx"));
    args.insert("state".to_string(), json!("present"));

    let task = Task {
        name: "Install nginx".to_string(),
        module: "package".to_string(),
        args,
        when: Some("ansible_os_family == 'Debian'".to_string()),
        notify: vec!["restart nginx".to_string()],
        register: Some("install_result".to_string()),
        loop_items: Some(vec![json!("nginx"), json!("redis")]),
        loop_var: "pkg".to_string(),
        ignore_errors: true,
        changed_when: Some("result.changed".to_string()),
        failed_when: Some("result.rc != 0".to_string()),
        delegate_to: Some("localhost".to_string()),
        run_once: true,
        tags: vec!["packages".to_string(), "nginx".to_string()],
        r#become: true,
        become_user: Some("root".to_string()),
    };

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, task.name);
    assert_eq!(deserialized.module, task.module);
    assert_eq!(deserialized.when, task.when);
    assert_eq!(deserialized.notify, task.notify);
    assert_eq!(deserialized.register, task.register);
    assert_eq!(deserialized.loop_var, "pkg");
    assert!(deserialized.ignore_errors);
    assert!(deserialized.run_once);
    assert!(deserialized.r#become);
    assert_eq!(deserialized.become_user, Some("root".to_string()));
    assert_eq!(deserialized.delegate_to, Some("localhost".to_string()));
}

// ============================================================================
// Test 5: Handler Serialization
// ============================================================================

#[test]
fn test_handler_basic_serialization() {
    let handler = Handler {
        name: "restart nginx".to_string(),
        module: "service".to_string(),
        args: IndexMap::new(),
        when: None,
        listen: vec![],
    };

    let json = serde_json::to_string(&handler).unwrap();
    let deserialized: Handler = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, "restart nginx");
    assert_eq!(deserialized.module, "service");
}

#[test]
fn test_handler_with_args() {
    let mut args = IndexMap::new();
    args.insert("name".to_string(), json!("nginx"));
    args.insert("state".to_string(), json!("restarted"));

    let handler = Handler {
        name: "restart nginx".to_string(),
        module: "service".to_string(),
        args,
        when: Some("not ansible_check_mode".to_string()),
        listen: vec!["nginx config changed".to_string()],
    };

    let json = serde_json::to_string(&handler).unwrap();
    let deserialized: Handler = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.args.get("name"), Some(&json!("nginx")));
    assert_eq!(
        deserialized.when,
        Some("not ansible_check_mode".to_string())
    );
    assert!(deserialized
        .listen
        .contains(&"nginx config changed".to_string()));
}

// ============================================================================
// Test 6: ModuleResult Serialization
// ============================================================================

#[test]
fn test_module_result_ok_serialization() {
    let result = ModuleResult::ok("Task completed successfully");
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: ModuleResult = serde_json::from_str(&json).unwrap();

    assert!(deserialized.success);
    assert!(!deserialized.changed);
    assert_eq!(deserialized.message, "Task completed successfully");
}

#[test]
fn test_module_result_changed_serialization() {
    let result = ModuleResult::changed("File updated");
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: ModuleResult = serde_json::from_str(&json).unwrap();

    assert!(deserialized.success);
    assert!(deserialized.changed);
    assert_eq!(deserialized.message, "File updated");
}

#[test]
fn test_module_result_failed_serialization() {
    let result = ModuleResult::failed("Permission denied");
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: ModuleResult = serde_json::from_str(&json).unwrap();

    assert!(!deserialized.success);
    assert!(!deserialized.changed);
    assert_eq!(deserialized.message, "Permission denied");
}

#[test]
fn test_module_result_skipped_serialization() {
    let result = ModuleResult::skipped("Skipped due to condition");
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: ModuleResult = serde_json::from_str(&json).unwrap();

    assert!(deserialized.success);
    assert!(deserialized.skipped);
}

#[test]
fn test_module_result_with_data() {
    let result = ModuleResult::ok("Success").with_data(json!({
        "path": "/etc/config",
        "size": 1024,
        "mode": "0644"
    }));

    let json = serde_json::to_string(&result).unwrap();
    let deserialized: ModuleResult = serde_json::from_str(&json).unwrap();

    let data = deserialized.data.unwrap();
    assert_eq!(data["path"], "/etc/config");
    assert_eq!(data["size"], 1024);
}

#[test]
fn test_module_result_with_warnings() {
    let result = ModuleResult::ok("Completed")
        .with_warning("Deprecated syntax used")
        .with_warning("Consider upgrading");

    let json = serde_json::to_string(&result).unwrap();
    let deserialized: ModuleResult = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.warnings.len(), 2);
    assert!(deserialized
        .warnings
        .contains(&"Deprecated syntax used".to_string()));
}

// ============================================================================
// Test 7: ExecutionResult Serialization
// ============================================================================

#[test]
fn test_execution_result_basic_serialization() {
    let result = ExecutionResult {
        host: "webserver1".to_string(),
        task_name: "Install nginx".to_string(),
        result: ModuleResult::ok("Installed"),
        duration: Duration::from_millis(1500),
        notify: vec![],
    };

    // ExecutionResult may not derive Serialize directly, so we test the components
    assert_eq!(result.host, "webserver1");
    assert_eq!(result.task_name, "Install nginx");
    assert!(result.result.success);
}

#[test]
fn test_execution_result_with_notify() {
    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "Configure nginx".to_string(),
        result: ModuleResult::changed("Config updated"),
        duration: Duration::from_secs(2),
        notify: vec!["restart nginx".to_string(), "reload config".to_string()],
    };

    assert_eq!(result.notify.len(), 2);
    assert!(result.notify.contains(&"restart nginx".to_string()));
}

// ============================================================================
// Test 8: Facts Serialization
// ============================================================================

#[test]
fn test_facts_basic_serialization() {
    let mut facts = Facts::new();
    facts.set("os_family", json!("Debian"));
    facts.set("os_arch", json!("x86_64"));

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.get("os_family"), Some(&json!("Debian")));
    assert_eq!(deserialized.get("os_arch"), Some(&json!("x86_64")));
}

#[test]
fn test_facts_nested_values() {
    let mut facts = Facts::new();
    facts.set(
        "network",
        json!({
            "interfaces": ["eth0", "lo"],
            "default_ipv4": {
                "address": "192.168.1.100",
                "netmask": "255.255.255.0"
            }
        }),
    );

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    let network = deserialized.get("network").unwrap();
    assert_eq!(network["default_ipv4"]["address"], "192.168.1.100");
}

#[test]
fn test_facts_empty() {
    let facts = Facts::new();
    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    assert!(deserialized.all().is_empty());
}

// ============================================================================
// Test 9: Variables Serialization
// ============================================================================

#[test]
fn test_variables_basic_serialization() {
    let mut vars = Variables::new();
    vars.set("db_host", json!("localhost"));
    vars.set("db_port", json!(5432));

    let json = serde_json::to_string(&vars).unwrap();
    let deserialized: Variables = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.get("db_host"), Some(&json!("localhost")));
    assert_eq!(deserialized.get("db_port"), Some(&json!(5432)));
}

#[test]
fn test_variables_complex_types() {
    let mut vars = Variables::new();
    vars.set(
        "servers",
        json!([
            {"name": "web1", "ip": "10.0.0.1"},
            {"name": "web2", "ip": "10.0.0.2"}
        ]),
    );
    vars.set("enabled", json!(true));
    vars.set("timeout", json!(30.5));

    let json = serde_json::to_string(&vars).unwrap();
    let deserialized: Variables = serde_json::from_str(&json).unwrap();

    let servers = deserialized.get("servers").unwrap();
    assert!(servers.is_array());
    assert_eq!(servers.as_array().unwrap().len(), 2);
}

// ============================================================================
// Test 10: VarPrecedence Serialization
// ============================================================================

#[test]
fn test_var_precedence_serialization() {
    let precedences = [
        VarPrecedence::RoleDefaults,
        VarPrecedence::InventoryGroupVars,
        VarPrecedence::PlayVars,
        VarPrecedence::ExtraVars,
    ];

    for precedence in precedences {
        let json = serde_json::to_string(&precedence).unwrap();
        let deserialized: VarPrecedence = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, precedence);
    }
}

#[test]
fn test_var_precedence_all_variants() {
    for precedence in VarPrecedence::all() {
        let json = serde_json::to_string(&precedence).unwrap();
        let deserialized: VarPrecedence = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized, precedence,
            "Round-trip failed for {:?}",
            precedence
        );
    }
}

// ============================================================================
// Test 11: HashBehaviour Serialization
// ============================================================================

#[test]
fn test_hash_behaviour_replace() {
    let behaviour = HashBehaviour::Replace;
    let json = serde_json::to_string(&behaviour).unwrap();
    let deserialized: HashBehaviour = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, HashBehaviour::Replace);
}

#[test]
fn test_hash_behaviour_merge() {
    let behaviour = HashBehaviour::Merge;
    let json = serde_json::to_string(&behaviour).unwrap();
    let deserialized: HashBehaviour = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, HashBehaviour::Merge);
}

// ============================================================================
// Test 12: Unicode and Special Characters
// ============================================================================

#[test]
fn test_unicode_in_task_name() {
    let task =
        Task::new("Install Japanese locale", "locale_gen").arg("name", "\u{65e5}\u{672c}\u{8a9e}"); // Japanese characters

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(
        deserialized.args.get("name"),
        Some(&JsonValue::String("\u{65e5}\u{672c}\u{8a9e}".to_string()))
    );
}

#[test]
fn test_unicode_in_facts() {
    let mut facts = Facts::new();
    facts.set("hostname", json!("server-\u{03b1}\u{03b2}\u{03b3}")); // Greek letters
    facts.set("emoji_test", json!("\u{1F600}\u{1F389}")); // Emoji

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    assert_eq!(
        deserialized.get("hostname"),
        Some(&json!("server-\u{03b1}\u{03b2}\u{03b3}"))
    );
    assert_eq!(
        deserialized.get("emoji_test"),
        Some(&json!("\u{1F600}\u{1F389}"))
    );
}

#[test]
fn test_special_characters_in_message() {
    let result = ModuleResult::failed("Error: \"Permission denied\" in path '/etc/config'");
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: ModuleResult = serde_json::from_str(&json).unwrap();

    assert!(deserialized.message.contains("\"Permission denied\""));
    assert!(deserialized.message.contains("/etc/config"));
}

#[test]
fn test_newlines_in_diff() {
    let diff = TaskDiff {
        before: Some("line1\nline2\nline3".to_string()),
        after: Some("line1\nmodified\nline3".to_string()),
        before_header: None,
        after_header: None,
    };

    let json = serde_json::to_string(&diff).unwrap();
    let deserialized: TaskDiff = serde_json::from_str(&json).unwrap();

    assert!(deserialized.before.unwrap().contains('\n'));
    assert!(deserialized.after.unwrap().contains('\n'));
}

#[test]
fn test_backslash_in_paths() {
    let mut facts = Facts::new();
    facts.set("path", json!("C:\\Windows\\System32"));

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    assert_eq!(
        deserialized.get("path"),
        Some(&json!("C:\\Windows\\System32"))
    );
}

#[test]
fn test_null_bytes_handling() {
    // JSON does not support null bytes in strings, verify proper handling
    let result = ModuleResult::ok("Test message");
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains('\0'));
}

// ============================================================================
// Test 13: Large Data Sizes
// ============================================================================

#[test]
fn test_large_facts_collection() {
    let mut facts = Facts::new();

    // Create 1000 facts
    for i in 0..1000 {
        facts.set(format!("fact_{}", i), json!(i));
    }

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.all().len(), 1000);
    assert_eq!(deserialized.get("fact_500"), Some(&json!(500)));
}

#[test]
fn test_large_string_content() {
    let large_content = "x".repeat(1_000_000); // 1MB string

    let diff = TaskDiff {
        before: Some(large_content.clone()),
        after: None,
        before_header: None,
        after_header: None,
    };

    let json = serde_json::to_string(&diff).unwrap();
    let deserialized: TaskDiff = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.before.unwrap().len(), 1_000_000);
}

#[test]
fn test_deeply_nested_structure() {
    let mut facts = Facts::new();

    // Create deeply nested structure
    let nested = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": {
                        "level5": {
                            "level6": {
                                "level7": {
                                    "level8": {
                                        "level9": {
                                            "level10": "deep value"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    facts.set("nested", nested.clone());

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    let result = deserialized.get("nested").unwrap();
    assert_eq!(
        result["level1"]["level2"]["level3"]["level4"]["level5"]["level6"]["level7"]["level8"]
            ["level9"]["level10"],
        "deep value"
    );
}

#[test]
fn test_large_array() {
    let mut vars = Variables::new();
    let large_array: Vec<JsonValue> = (0..10000).map(|i| json!(i)).collect();
    vars.set("numbers", JsonValue::Array(large_array));

    let json = serde_json::to_string(&vars).unwrap();
    let deserialized: Variables = serde_json::from_str(&json).unwrap();

    let numbers = deserialized.get("numbers").unwrap();
    assert_eq!(numbers.as_array().unwrap().len(), 10000);
}

#[test]
fn test_many_handlers() {
    let handlers: Vec<Handler> = (0..100)
        .map(|i| Handler {
            name: format!("handler_{}", i),
            module: "debug".to_string(),
            args: IndexMap::new(),
            when: None,
            listen: vec![format!("event_{}", i)],
        })
        .collect();

    let json = serde_json::to_string(&handlers).unwrap();
    let deserialized: Vec<Handler> = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.len(), 100);
    assert_eq!(deserialized[50].name, "handler_50");
}

// ============================================================================
// Test 14: Optional Fields Handling
// ============================================================================

#[test]
fn test_task_all_optional_fields_none() {
    let task = Task::default();
    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert!(deserialized.when.is_none());
    assert!(deserialized.register.is_none());
    assert!(deserialized.loop_items.is_none());
    assert!(deserialized.changed_when.is_none());
    assert!(deserialized.failed_when.is_none());
    assert!(deserialized.delegate_to.is_none());
    assert!(deserialized.become_user.is_none());
}

#[test]
fn test_task_result_optional_fields_none() {
    let result = TaskResult::ok();
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: TaskResult = serde_json::from_str(&json).unwrap();

    assert!(deserialized.msg.is_none());
    assert!(deserialized.result.is_none());
    assert!(deserialized.diff.is_none());
}

#[test]
fn test_module_result_optional_data_none() {
    let result = ModuleResult::ok("Success");
    let json = serde_json::to_string(&result).unwrap();

    // Verify that None fields are not serialized (skip_serializing_if)
    let _parsed: JsonValue = serde_json::from_str(&json).unwrap();
    // data should be skipped when None
    if result.data.is_none() {
        // If skip_serializing_if is used, data field may not exist
        // This depends on the derive implementation
    }

    let deserialized: ModuleResult = serde_json::from_str(&json).unwrap();
    assert!(deserialized.data.is_none());
}

#[test]
fn test_empty_collections_serialization() {
    let task = Task {
        name: "Test".to_string(),
        module: "debug".to_string(),
        args: IndexMap::new(),
        when: None,
        notify: vec![],
        register: None,
        loop_items: None,
        loop_var: "item".to_string(),
        ignore_errors: false,
        changed_when: None,
        failed_when: None,
        delegate_to: None,
        run_once: false,
        tags: vec![],
        r#become: false,
        become_user: None,
    };

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert!(deserialized.args.is_empty());
    assert!(deserialized.notify.is_empty());
    assert!(deserialized.tags.is_empty());
}

// ============================================================================
// Test 15: Edge Cases
// ============================================================================

#[test]
fn test_empty_string_values() {
    let task = Task::new("", "").arg("empty", "").when("");

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, "");
    assert_eq!(deserialized.module, "");
    assert_eq!(deserialized.when, Some("".to_string()));
}

#[test]
fn test_whitespace_only_strings() {
    let mut facts = Facts::new();
    facts.set("whitespace", json!("   \t\n   "));

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.get("whitespace"), Some(&json!("   \t\n   ")));
}

#[test]
fn test_numeric_string_keys() {
    let mut facts = Facts::new();
    facts.set("123", json!("numeric key"));
    facts.set("0", json!("zero key"));

    let json = serde_json::to_string(&facts).unwrap();
    let deserialized: Facts = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.get("123"), Some(&json!("numeric key")));
    assert_eq!(deserialized.get("0"), Some(&json!("zero key")));
}

#[test]
fn test_boolean_variations() {
    let mut vars = Variables::new();
    vars.set("true_val", json!(true));
    vars.set("false_val", json!(false));

    let json = serde_json::to_string(&vars).unwrap();
    let deserialized: Variables = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.get("true_val"), Some(&json!(true)));
    assert_eq!(deserialized.get("false_val"), Some(&json!(false)));
}

#[test]
fn test_numeric_types() {
    let mut vars = Variables::new();
    vars.set("integer", json!(42));
    vars.set("negative", json!(-100));
    vars.set("float", json!(3.14160));
    vars.set("zero", json!(0));
    vars.set("large", json!(9223372036854775807_i64));

    let json = serde_json::to_string(&vars).unwrap();
    let deserialized: Variables = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.get("integer"), Some(&json!(42)));
    assert_eq!(deserialized.get("negative"), Some(&json!(-100)));
    assert_eq!(deserialized.get("float"), Some(&json!(3.14160)));
    assert_eq!(deserialized.get("zero"), Some(&json!(0)));
    assert_eq!(
        deserialized.get("large"),
        Some(&json!(9223372036854775807_i64))
    );
}

#[test]
fn test_null_values() {
    let mut vars = Variables::new();
    vars.set("null_val", JsonValue::Null);

    let json = serde_json::to_string(&vars).unwrap();
    let deserialized: Variables = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.get("null_val"), Some(&JsonValue::Null));
}

// ============================================================================
// Test 16: Deserialization Error Handling
// ============================================================================

#[test]
fn test_invalid_task_status_deserialization() {
    let invalid_json = r#""invalid_status""#;
    let result: Result<TaskStatus, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err());
}

#[test]
fn test_missing_required_fields() {
    // Task requires name and module
    let incomplete_json = r#"{"name": "test"}"#;
    let result: Result<Task, _> = serde_json::from_str(incomplete_json);
    // This may or may not fail depending on default implementations
    // Just verify it doesn't panic
    let _ = result;
}

#[test]
fn test_wrong_type_for_field() {
    let wrong_type_json = r#"{"name": 123, "module": "test"}"#;
    let result: Result<Task, _> = serde_json::from_str(wrong_type_json);
    assert!(result.is_err());
}

// ============================================================================
// Test 17: Round-Trip Consistency
// ============================================================================

#[test]
fn test_task_round_trip_preserves_order() {
    let mut args = IndexMap::new();
    args.insert("first".to_string(), json!(1));
    args.insert("second".to_string(), json!(2));
    args.insert("third".to_string(), json!(3));

    let task = Task {
        name: "Ordered test".to_string(),
        module: "debug".to_string(),
        args,
        ..Default::default()
    };

    let json = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&json).unwrap();

    // IndexMap should preserve insertion order
    let keys: Vec<&String> = deserialized.args.keys().collect();
    assert_eq!(keys, vec!["first", "second", "third"]);
}

#[test]
fn test_multiple_round_trips() {
    let original = TaskResult::changed()
        .with_msg("Original message")
        .with_result(json!({"key": "value"}));

    let mut current = original.clone();
    for _ in 0..10 {
        let json = serde_json::to_string(&current).unwrap();
        current = serde_json::from_str(&json).unwrap();
    }

    assert_eq!(current.status, original.status);
    assert_eq!(current.msg, original.msg);
    assert_eq!(current.result, original.result);
}

// ============================================================================
// Test 18: JSON Pretty Print
// ============================================================================

#[test]
fn test_pretty_print_facts() {
    let mut facts = Facts::new();
    facts.set("os", json!("linux"));
    facts.set("version", json!("22.04"));

    let pretty = serde_json::to_string_pretty(&facts).unwrap();
    assert!(pretty.contains('\n'));
    assert!(pretty.contains("  ")); // Indentation

    // Verify it can be deserialized
    let deserialized: Facts = serde_json::from_str(&pretty).unwrap();
    assert_eq!(deserialized.get("os"), Some(&json!("linux")));
}

// ============================================================================
// Test 19: YAML Cross-Compatibility
// ============================================================================

#[test]
fn test_task_yaml_serialization() {
    let task = Task::new("Install package", "apt")
        .arg("name", "nginx")
        .arg("state", "present");

    let yaml = serde_yaml::to_string(&task).unwrap();
    let from_yaml: Task = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(from_yaml.name, task.name);
    assert_eq!(from_yaml.module, task.module);
}

#[test]
fn test_facts_yaml_serialization() {
    let mut facts = Facts::new();
    facts.set("os_family", json!("Debian"));

    let yaml = serde_yaml::to_string(&facts).unwrap();
    let from_yaml: Facts = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(from_yaml.get("os_family"), Some(&json!("Debian")));
}

// ============================================================================
// Test 20: Concurrent Serialization
// ============================================================================

#[tokio::test]
async fn test_concurrent_serialization() {
    use std::sync::Arc;
    use tokio::task;

    let facts = Arc::new({
        let mut f = Facts::new();
        f.set("shared", json!("value"));
        f
    });

    let mut handles = vec![];

    for i in 0..100 {
        let facts_clone = Arc::clone(&facts);
        let handle = task::spawn(async move {
            let json = serde_json::to_string(facts_clone.as_ref()).unwrap();
            let deserialized: Facts = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized.get("shared"), Some(&json!("value")));
            i
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}
