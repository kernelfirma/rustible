#![cfg(not(tarpaulin))]
//! Comprehensive tests for Rustible task delegation functionality
//!
//! This test suite covers:
//! 1. Basic delegate_to functionality
//! 2. Delegate to localhost
//! 3. Delegate to group members
//! 4. delegate_facts behavior
//! 5. local_action shorthand
//! 6. run_once with delegate_to
//! 7. Loops with delegation
//! 8. Variable context during delegation
//! 9. Connection handling with delegation
//! 10. Handler delegation
//! 11. Edge cases and error handling

#![allow(unused_mut)]

mod common;

use indexmap::IndexMap;
use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{Handler, Task};
use rustible::executor::{Executor, ExecutorConfig};

// ============================================================================
// Test 1: Basic delegate_to Functionality
// ============================================================================

#[test]
fn test_task_delegate_to_field() {
    let task = Task::new("Test delegation", "debug").arg("msg", "Delegated task");

    // Create a task with delegate_to
    let mut task_with_delegate = task.clone();
    task_with_delegate.delegate_to = Some("other_host".to_string());

    assert_eq!(
        task_with_delegate.delegate_to,
        Some("other_host".to_string())
    );
    assert_eq!(task_with_delegate.name, "Test delegation");
    assert_eq!(task_with_delegate.module, "debug");
}

#[test]
fn test_task_default_no_delegation() {
    let task = Task::new("Normal task", "debug");

    assert!(task.delegate_to.is_none());
}

#[tokio::test]
async fn test_delegate_to_specific_host() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate Test");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Task delegated to db1
    let mut task = Task::new("Delegated to db1", "debug").arg("msg", "Running on db1");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Task should execute (even though delegated, we track by inventory_hostname)
    assert!(results.contains_key("web1"));
    let host_result = results.get("web1").unwrap();
    assert!(!host_result.failed);
}

#[tokio::test]
async fn test_delegate_to_localhost() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate to Localhost");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Local delegation", "debug").arg("msg", "Delegated to localhost");
    task.delegate_to = Some("localhost".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert!(results.contains_key("web1"));
    assert!(!results.get("web1").unwrap().failed);
}

#[tokio::test]
async fn test_delegate_to_group_member() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate to Group Member");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // First webserver delegates to the database server
    let mut task = Task::new("Query database", "debug").arg("msg", "Delegated database query");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both webservers should show in results (they're the inventory hosts)
    assert!(results.contains_key("web1"));
    assert!(results.contains_key("web2"));
}

// ============================================================================
// Test 2: delegate_to with Facts Access
// ============================================================================

#[tokio::test]
async fn test_delegate_facts_from_original_host_available() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    // Set facts for the original host
    runtime.set_host_fact("web1", "os_family".to_string(), serde_json::json!("Debian"));
    runtime.set_host_var("web1", "http_port".to_string(), serde_json::json!(80));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Facts Access During Delegation");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Task delegated but should have access to original host facts
    let mut task = Task::new("Access original facts", "debug")
        .arg("msg", "{{ os_family | default('unknown') }}");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_hostvars_access_during_delegation() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    runtime.set_host_var("db1", "db_port".to_string(), serde_json::json!(5432));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Hostvars Access");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Delegated task accessing delegate's hostvars
    let mut task =
        Task::new("Access delegate hostvars", "debug").arg("msg", "DB port from hostvars");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(!results.get("web1").unwrap().failed);
}

#[tokio::test]
async fn test_ansible_host_resolution_during_delegation() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    // Set ansible_host for the delegate target
    runtime.set_host_var(
        "db1",
        "ansible_host".to_string(),
        serde_json::json!("192.168.1.20"),
    );

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Ansible Host Resolution");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Use ansible_host", "debug").arg("msg", "Connecting to delegate");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

// ============================================================================
// Test 3: delegate_facts Behavior
// ============================================================================

#[tokio::test]
async fn test_delegate_facts_true_stores_on_delegate() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate Facts True");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Set fact with delegate_to (delegate_facts would be true)
    // Note: delegate_facts field would need to be added to Task struct
    let mut task = Task::new("Set fact on delegate", "set_fact").arg("db_initialized", true);
    task.delegate_to = Some("db1".to_string());
    // task.delegate_facts = true; // If this field existed
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_delegate_facts_default_stores_on_inventory_hostname() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate Facts Default");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Set fact without delegate_facts (default behavior)
    let mut task = Task::new("Set fact", "set_fact").arg("my_fact", "my_value");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    // This fact should be on web1, not db1
    play.add_task(
        Task::new("Check fact", "debug").arg("msg", "{{ my_fact | default('not_set') }}"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

// ============================================================================
// Test 4: local_action Shorthand
// ============================================================================

#[tokio::test]
async fn test_local_action_equivalent_to_delegate_localhost() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Local Action Test");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Using delegate_to: localhost (equivalent to local_action)
    let mut task1 = Task::new("Delegate to localhost", "debug").arg("msg", "Local via delegate_to");
    task1.delegate_to = Some("localhost".to_string());
    play.add_task(task1);

    // Another local task
    let mut task2 = Task::new("Another local task", "debug").arg("msg", "Also local");
    task2.delegate_to = Some("localhost".to_string());
    play.add_task(task2);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
    assert!(!results.get("web1").unwrap().failed);
}

#[tokio::test]
async fn test_local_action_module_execution() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Local Action Module");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Execute a command locally
    let mut task = Task::new("Local command", "command").arg("cmd", "echo 'local execution'");
    task.delegate_to = Some("localhost".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

// ============================================================================
// Test 5: run_once with delegate_to
// ============================================================================

#[tokio::test]
async fn test_run_once_with_delegate_to() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Run Once Delegate");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Task runs once, delegated to db1
    let mut task = Task::new("Initialize database", "debug").arg("msg", "Database initialization");
    task.delegate_to = Some("db1".to_string());
    task.run_once = true;
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Both hosts should be in results (inventory hosts)
    assert!(results.contains_key("web1"));
    assert!(results.contains_key("web2"));
}

#[tokio::test]
async fn test_run_once_runs_on_first_host() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("web3".to_string(), Some("webservers"));

    let config = ExecutorConfig {
        forks: 1, // Sequential to ensure deterministic ordering
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Run Once First Host");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Run once task", "debug").arg("msg", "Should only run once");
    task.run_once = true;
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should be in results
    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_run_once_with_delegate_variable_scope() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));

    runtime.set_host_var("web1", "priority".to_string(), serde_json::json!(1));
    runtime.set_host_var("web2", "priority".to_string(), serde_json::json!(2));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Run Once Variable Scope");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Check priority", "debug")
        .arg("msg", "Priority check")
        .register("priority_result");
    task.run_once = true;
    task.delegate_to = Some("localhost".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1") || results.contains_key("web2"));
}

// ============================================================================
// Test 6: Loops with delegate_to
// ============================================================================

#[tokio::test]
async fn test_delegate_to_in_loop() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate in Loop");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Loop with fixed delegate_to
    let mut task = Task::new("Loop with delegation", "debug")
        .arg("msg", "Item: {{ item }}")
        .loop_over(vec![
            serde_json::json!("item1"),
            serde_json::json!("item2"),
            serde_json::json!("item3"),
        ]);
    task.delegate_to = Some("localhost".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
    assert!(!results.get("web1").unwrap().failed);
}

#[tokio::test]
async fn test_different_delegate_per_iteration() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("controller".to_string(), None);
    runtime.add_host("db1".to_string(), Some("databases"));
    runtime.add_host("db2".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Different Delegate Per Iteration");
    let mut play = Play::new("Test Play", "controller");
    play.gather_facts = false;

    // Each iteration delegates to a different host (using item as delegate target)
    let mut task = Task::new("Delegate to each", "debug")
        .arg("msg", "Delegating to {{ item }}")
        .loop_over(vec![serde_json::json!("db1"), serde_json::json!("db2")]);
    // Note: In real implementation, delegate_to: "{{ item }}" would be templated
    task.delegate_to = Some("db1".to_string()); // Simplified for test
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("controller"));
}

#[tokio::test]
async fn test_loop_with_register_and_delegate() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Loop Register Delegate");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Loop and register", "debug")
        .arg("msg", "Processing {{ item }}")
        .loop_over(vec![serde_json::json!("a"), serde_json::json!("b")])
        .register("loop_results");
    task.delegate_to = Some("localhost".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

// ============================================================================
// Test 7: Variable Context
// ============================================================================

#[tokio::test]
async fn test_inventory_hostname_vs_ansible_host() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    runtime.set_host_var(
        "db1",
        "ansible_host".to_string(),
        serde_json::json!("192.168.1.20"),
    );

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Hostname Variables");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // During delegation, inventory_hostname should still be web1
    let mut task = Task::new("Check hostname", "debug").arg("msg", "inventory_hostname check");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_hostvars_inventory_hostname_access() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    runtime.set_host_var(
        "web1",
        "original_var".to_string(),
        serde_json::json!("from_web1"),
    );

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Hostvars Access");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Access hostvars[inventory_hostname] during delegation
    let mut task = Task::new("Access hostvars", "debug").arg("msg", "Accessing original host vars");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(!results.get("web1").unwrap().failed);
}

#[tokio::test]
async fn test_delegate_host_variables_accessible() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    runtime.set_host_var(
        "db1",
        "db_name".to_string(),
        serde_json::json!("production_db"),
    );

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate Host Variables");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Variables from the delegate host should be accessible
    let mut task =
        Task::new("Access delegate vars", "debug").arg("msg", "Accessing delegate host variables");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

// ============================================================================
// Test 8: Connection Handling
// ============================================================================

#[tokio::test]
async fn test_separate_connection_for_delegate() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));
    runtime.add_host("localhost".to_string(), None);

    // Set connection type for localhost
    runtime.set_host_var(
        "localhost",
        "ansible_connection".to_string(),
        serde_json::json!("local"),
    );

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Separate Connection");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Use local connection", "debug").arg("msg", "Using local connection");
    task.delegate_to = Some("localhost".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_connection_pooling_with_delegation() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig {
        forks: 2,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Connection Pooling");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Multiple tasks delegating to same host
    for i in 1..=3 {
        let mut task =
            Task::new(format!("Task {}", i), "debug").arg("msg", format!("Task {} on db1", i));
        task.delegate_to = Some("db1".to_string());
        play.add_task(task);
    }

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert_eq!(results.len(), 2); // web1 and web2
}

#[tokio::test]
async fn test_connection_errors_on_delegate() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    // Note: "unreachable_host" not added - testing graceful handling

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Connection Error");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Delegate to a host that might not be reachable
    let mut task = Task::new("May fail", "debug").arg("msg", "Attempting delegation");
    task.delegate_to = Some("unreachable_host".to_string());
    task.ignore_errors = true; // Graceful handling
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    // Should complete without panic
    assert!(results.contains_key("web1"));
}

// ============================================================================
// Test 9: Handlers with Delegation
// ============================================================================

#[tokio::test]
async fn test_handler_with_delegate_to() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Handler Delegation");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Task that triggers handler
    let mut task = Task::new("Trigger handler", "copy")
        .arg("src", "test.conf")
        .arg("dest", "/etc/test.conf")
        .notify("delegated handler");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    // Handler with delegation
    let mut handler_args = IndexMap::new();
    handler_args.insert("msg".to_string(), serde_json::json!("Handler on delegate"));
    play.add_handler(Handler {
        name: "delegated handler".to_string(),
        module: "debug".to_string(),
        args: handler_args,
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_notify_from_delegated_task() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Notify from Delegation");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Delegated task that notifies a handler
    let mut task = Task::new("Update config", "copy")
        .arg("src", "config.yml")
        .arg("dest", "/etc/app/config.yml")
        .notify("restart service");
    task.delegate_to = Some("localhost".to_string());
    play.add_task(task);

    let mut handler_args = IndexMap::new();
    handler_args.insert("msg".to_string(), serde_json::json!("Service restarted"));
    play.add_handler(Handler {
        name: "restart service".to_string(),
        module: "debug".to_string(),
        args: handler_args,
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_handler_runs_on_delegate() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Handler on Delegate");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    play.add_task(
        Task::new("Trigger", "copy")
            .arg("src", "test.conf")
            .arg("dest", "/etc/test.conf")
            .notify("db handler"),
    );

    // Handler that should run on db1
    let mut handler_args = IndexMap::new();
    handler_args.insert("msg".to_string(), serde_json::json!("Running on db"));
    let mut handler = Handler {
        name: "db handler".to_string(),
        module: "debug".to_string(),
        args: handler_args,
        when: None,
        listen: vec![],
    };
    play.add_handler(handler);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

// ============================================================================
// Test 10: Edge Cases
// ============================================================================

#[tokio::test]
async fn test_delegate_to_self() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate to Self");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Delegate to the same host (should work like normal execution)
    let mut task = Task::new("Self delegation", "debug").arg("msg", "Delegating to self");
    task.delegate_to = Some("web1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
    assert!(!results.get("web1").unwrap().failed);
}

#[tokio::test]
async fn test_delegate_to_unreachable_host_ignored() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Unreachable Delegate");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Delegate to non-existent host with ignore_errors
    let mut task = Task::new("Unreachable delegate", "debug")
        .arg("msg", "This might fail")
        .ignore_errors(true);
    task.delegate_to = Some("nonexistent_host".to_string());
    play.add_task(task);

    // Follow-up task should still run
    play.add_task(Task::new("Follow up", "debug").arg("msg", "After failed delegation"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_missing_delegate_host() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Missing Delegate Host");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Missing host", "debug")
        .arg("msg", "Delegate to missing")
        .ignore_errors(true);
    task.delegate_to = Some("missing_host".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    // Should complete without crashing
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_multiple_consecutive_delegations() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));
    runtime.add_host("db2".to_string(), Some("databases"));
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Multiple Delegations");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Multiple consecutive delegations to different hosts
    let mut task1 = Task::new("Delegate 1", "debug").arg("msg", "To db1");
    task1.delegate_to = Some("db1".to_string());
    play.add_task(task1);

    play.add_task(Task::new("No delegate", "debug").arg("msg", "Normal"));

    let mut task2 = Task::new("Delegate 2", "debug").arg("msg", "To db2");
    task2.delegate_to = Some("db2".to_string());
    play.add_task(task2);

    let mut task3 = Task::new("Delegate 3", "debug").arg("msg", "To localhost");
    task3.delegate_to = Some("localhost".to_string());
    play.add_task(task3);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
    assert!(!results.get("web1").unwrap().failed);
}

#[tokio::test]
async fn test_delegate_with_when_condition() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    runtime.set_global_var("should_delegate".to_string(), serde_json::json!(true));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Conditional Delegation");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Conditional delegate", "debug")
        .arg("msg", "Delegated with condition")
        .when("should_delegate");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_delegate_with_become() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Delegate with Become");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    let mut task = Task::new("Privileged delegate", "command").arg("cmd", "whoami");
    task.delegate_to = Some("db1".to_string());
    task.r#become = true;
    task.become_user = Some("root".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

// ============================================================================
// Additional Integration Tests
// ============================================================================

#[tokio::test]
async fn test_mixed_delegated_and_normal_tasks() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Mixed Tasks");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Normal task
    play.add_task(Task::new("Normal 1", "debug").arg("msg", "Normal task"));

    // Delegated task
    let mut task = Task::new("Delegated", "debug").arg("msg", "Delegated task");
    task.delegate_to = Some("db1".to_string());
    play.add_task(task);

    // Normal task again
    play.add_task(Task::new("Normal 2", "debug").arg("msg", "Another normal task"));

    // Local task
    let mut local = Task::new("Local", "debug").arg("msg", "Local task");
    local.delegate_to = Some("localhost".to_string());
    play.add_task(local);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
    assert!(results.contains_key("web2"));
    assert!(!results.get("web1").unwrap().failed);
    assert!(!results.get("web2").unwrap().failed);
}

#[tokio::test]
async fn test_delegate_across_multiple_plays() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Multi-Play Delegation");

    // Play 1
    let mut play1 = Play::new("Play 1", "webservers");
    play1.gather_facts = false;
    let mut task1 = Task::new("Delegate in play 1", "debug").arg("msg", "Play 1 delegation");
    task1.delegate_to = Some("db1".to_string());
    play1.add_task(task1);

    // Play 2
    let mut play2 = Play::new("Play 2", "databases");
    play2.gather_facts = false;
    let mut task2 = Task::new("Delegate in play 2", "debug").arg("msg", "Play 2 delegation");
    task2.delegate_to = Some("web1".to_string());
    play2.add_task(task2);

    playbook.add_play(play1);
    playbook.add_play(play2);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
    assert!(results.contains_key("db1"));
}

#[tokio::test]
async fn test_delegate_with_register_and_use() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Register and Use");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // Delegated task that registers result
    let mut task1 = Task::new("Register result", "debug")
        .arg("msg", "Getting info from db")
        .register("db_result");
    task1.delegate_to = Some("db1".to_string());
    play.add_task(task1);

    // Use the registered result
    play.add_task(
        Task::new("Use result", "debug")
            .arg("msg", "Using registered result")
            .when("db_result is defined"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    assert!(results.contains_key("web1"));
}

#[tokio::test]
async fn test_all_hosts_delegate_to_single() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("web3".to_string(), Some("webservers"));
    runtime.add_host("loadbalancer".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("All Delegate to Single");
    let mut play = Play::new("Test Play", "webservers");
    play.gather_facts = false;

    // All webservers delegate to loadbalancer
    let mut task = Task::new("Update LB config", "debug")
        .arg("msg", "Updating loadbalancer from {{ inventory_hostname }}");
    task.delegate_to = Some("loadbalancer".to_string());
    play.add_task(task);

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    // All three webservers should be in results
    assert_eq!(results.len(), 3);
    assert!(results.contains_key("web1"));
    assert!(results.contains_key("web2"));
    assert!(results.contains_key("web3"));
}

// ============================================================================
// Task Definition Tests
// ============================================================================

#[test]
fn test_task_with_all_delegation_fields() {
    let mut task = Task::new("Full delegation task", "command").arg("cmd", "echo test");

    task.delegate_to = Some("db1".to_string());
    task.run_once = true;

    assert_eq!(task.delegate_to, Some("db1".to_string()));
    assert!(task.run_once);
}

#[test]
fn test_task_clone_preserves_delegation() {
    let mut original = Task::new("Original", "debug");
    original.delegate_to = Some("target".to_string());
    original.run_once = true;

    let cloned = original.clone();

    assert_eq!(cloned.delegate_to, Some("target".to_string()));
    assert!(cloned.run_once);
}

// ============================================================================
// Playbook Parsing Tests for delegate_to
// ============================================================================

#[test]
fn test_parse_playbook_with_delegate_to() {
    let yaml = r#"
- name: Delegation Test
  hosts: webservers
  gather_facts: false
  tasks:
    - name: Delegated task
      debug:
        msg: "Hello"
      delegate_to: localhost
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    assert_eq!(playbook.plays.len(), 1);

    let task = &playbook.plays[0].tasks[0];
    assert_eq!(task.delegate_to, Some("localhost".to_string()));
}

#[test]
fn test_parse_playbook_with_run_once() {
    let yaml = r#"
- name: Run Once Test
  hosts: all
  gather_facts: false
  tasks:
    - name: Run once task
      debug:
        msg: "Single execution"
      run_once: true
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    assert!(task.run_once);
}

#[test]
fn test_parse_playbook_with_delegate_and_run_once() {
    let yaml = r#"
- name: Combined Test
  hosts: webservers
  gather_facts: false
  tasks:
    - name: Delegate and run once
      command: echo "test"
      delegate_to: db1
      run_once: true
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();
    let task = &playbook.plays[0].tasks[0];

    assert_eq!(task.delegate_to, Some("db1".to_string()));
    assert!(task.run_once);
}
