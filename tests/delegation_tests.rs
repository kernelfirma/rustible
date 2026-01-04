//! Tests for task delegation functionality
//!
//! Tests delegate_to and delegate_facts directives

use rustible::executor::parallelization::ParallelizationManager;
use rustible::executor::runtime::ExecutionContext;
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{Task, TaskStatus};
use rustible::modules::ModuleRegistry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[tokio::test]
async fn test_delegate_to_basic() {
    // Setup runtime context with two hosts
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), None);
    runtime.add_host("localhost".to_string(), None);

    let runtime = Arc::new(RwLock::new(runtime));
    let handlers = Arc::new(RwLock::new(HashMap::new()));
    let notified = Arc::new(Mutex::new(HashSet::new()));
    let parallelization = Arc::new(ParallelizationManager::new());
    let module_registry = Arc::new(ModuleRegistry::with_builtins());

    // Create a task that delegates to localhost
    let task = Task::new("Debug on localhost", "debug").arg("msg", "Hello from delegated task");

    // Create task with delegate_to set
    let mut delegated_task = task.clone();
    delegated_task.delegate_to = Some("localhost".to_string());

    // Execute on web1 but delegate to localhost
    let ctx = ExecutionContext::new("web1");

    let result = delegated_task
        .execute(
            &ctx,
            &runtime,
            &handlers,
            &notified,
            &parallelization,
            &module_registry,
        )
        .await;

    assert!(result.is_ok());
    let task_result = result.unwrap();
    assert_eq!(task_result.status, TaskStatus::Ok);
}

#[tokio::test]
async fn test_delegate_facts_false() {
    // Setup runtime context
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), None);
    runtime.add_host("localhost".to_string(), None);

    let runtime = Arc::new(RwLock::new(runtime));
    let handlers = Arc::new(RwLock::new(HashMap::new()));
    let notified = Arc::new(Mutex::new(HashSet::new()));
    let parallelization = Arc::new(ParallelizationManager::new());
    let module_registry = Arc::new(ModuleRegistry::with_builtins());

    // Create a set_fact task that delegates to localhost but stores facts on web1
    let mut task = Task::new("Set fact", "set_fact");
    task.args
        .insert("test_var".to_string(), serde_json::json!("test_value"));
    task.delegate_to = Some("localhost".to_string());
    task.delegate_facts = Some(false); // Facts should go to original host (web1)

    let ctx = ExecutionContext::new("web1");

    let result = task
        .execute(
            &ctx,
            &runtime,
            &handlers,
            &notified,
            &parallelization,
            &module_registry,
        )
        .await;

    assert!(result.is_ok());

    // Check that fact was stored on web1 (original host), not localhost
    let rt = runtime.read().await;
    let web1_fact = rt.get_host_fact("web1", "test_var");
    let localhost_fact = rt.get_host_fact("localhost", "test_var");

    assert_eq!(web1_fact, Some(serde_json::json!("test_value")));
    assert_eq!(localhost_fact, None);
}

#[tokio::test]
async fn test_delegate_facts_true() {
    // Setup runtime context
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), None);
    runtime.add_host("localhost".to_string(), None);

    let runtime = Arc::new(RwLock::new(runtime));
    let handlers = Arc::new(RwLock::new(HashMap::new()));
    let notified = Arc::new(Mutex::new(HashSet::new()));
    let parallelization = Arc::new(ParallelizationManager::new());
    let module_registry = Arc::new(ModuleRegistry::with_builtins());

    // Create a set_fact task that delegates to localhost and stores facts there
    let mut task = Task::new("Set fact on delegate", "set_fact");
    task.args.insert(
        "delegate_var".to_string(),
        serde_json::json!("delegate_value"),
    );
    task.delegate_to = Some("localhost".to_string());
    task.delegate_facts = Some(true); // Facts should go to delegate host (localhost)

    let ctx = ExecutionContext::new("web1");

    let result = task
        .execute(
            &ctx,
            &runtime,
            &handlers,
            &notified,
            &parallelization,
            &module_registry,
        )
        .await;

    assert!(result.is_ok());

    // Check that fact was stored on localhost (delegate host), not web1
    let rt = runtime.read().await;
    let web1_fact = rt.get_host_fact("web1", "delegate_var");
    let localhost_fact = rt.get_host_fact("localhost", "delegate_var");

    assert_eq!(web1_fact, None);
    assert_eq!(localhost_fact, Some(serde_json::json!("delegate_value")));
}

#[tokio::test]
async fn test_delegate_facts_default_false() {
    // Setup runtime context
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), None);
    runtime.add_host("localhost".to_string(), None);

    let runtime = Arc::new(RwLock::new(runtime));
    let handlers = Arc::new(RwLock::new(HashMap::new()));
    let notified = Arc::new(Mutex::new(HashSet::new()));
    let parallelization = Arc::new(ParallelizationManager::new());
    let module_registry = Arc::new(ModuleRegistry::with_builtins());

    // Create a set_fact task that delegates but doesn't specify delegate_facts
    // Default should be false (facts go to original host)
    let mut task = Task::new("Set fact default", "set_fact");
    task.args.insert(
        "default_var".to_string(),
        serde_json::json!("default_value"),
    );
    task.delegate_to = Some("localhost".to_string());
    // delegate_facts not set, should default to false

    let ctx = ExecutionContext::new("web1");

    let result = task
        .execute(
            &ctx,
            &runtime,
            &handlers,
            &notified,
            &parallelization,
            &module_registry,
        )
        .await;

    assert!(result.is_ok());

    // Check that fact was stored on web1 (original host) by default
    let rt = runtime.read().await;
    let web1_fact = rt.get_host_fact("web1", "default_var");
    let localhost_fact = rt.get_host_fact("localhost", "default_var");

    assert_eq!(web1_fact, Some(serde_json::json!("default_value")));
    assert_eq!(localhost_fact, None);
}

#[tokio::test]
async fn test_delegate_with_register() {
    // Setup runtime context
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), None);
    runtime.add_host("localhost".to_string(), None);

    let runtime = Arc::new(RwLock::new(runtime));
    let handlers = Arc::new(RwLock::new(HashMap::new()));
    let notified = Arc::new(Mutex::new(HashSet::new()));
    let parallelization = Arc::new(ParallelizationManager::new());
    let module_registry = Arc::new(ModuleRegistry::with_builtins());

    // Create a task that delegates and registers result
    let mut task = Task::new("Debug and register", "debug");
    task.args
        .insert("msg".to_string(), serde_json::json!("Delegated message"));
    task.delegate_to = Some("localhost".to_string());
    task.register = Some("delegate_result".to_string());

    let ctx = ExecutionContext::new("web1");

    let result = task
        .execute(
            &ctx,
            &runtime,
            &handlers,
            &notified,
            &parallelization,
            &module_registry,
        )
        .await;

    assert!(result.is_ok());

    // Registered results should always go to the original host (web1)
    let rt = runtime.read().await;
    let web1_registered = rt.get_registered("web1", "delegate_result");
    let localhost_registered = rt.get_registered("localhost", "delegate_result");

    assert!(web1_registered.is_some());
    assert!(localhost_registered.is_none());
}

#[tokio::test]
async fn test_no_delegation() {
    // Test that tasks without delegate_to work normally
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), None);

    let runtime = Arc::new(RwLock::new(runtime));
    let handlers = Arc::new(RwLock::new(HashMap::new()));
    let notified = Arc::new(Mutex::new(HashSet::new()));
    let parallelization = Arc::new(ParallelizationManager::new());
    let module_registry = Arc::new(ModuleRegistry::with_builtins());

    let mut task = Task::new("Normal task", "set_fact");
    task.args
        .insert("normal_var".to_string(), serde_json::json!("normal_value"));

    let ctx = ExecutionContext::new("web1");

    let result = task
        .execute(
            &ctx,
            &runtime,
            &handlers,
            &notified,
            &parallelization,
            &module_registry,
        )
        .await;

    assert!(result.is_ok());

    // Fact should be on web1
    let rt = runtime.read().await;
    let fact = rt.get_host_fact("web1", "normal_var");
    assert_eq!(fact, Some(serde_json::json!("normal_value")));
}
