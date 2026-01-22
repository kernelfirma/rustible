//! Integration tests for the Rustible execution engine
//!
//! These tests verify the core functionality of the executor module including:
//! - Playbook execution
//! - Task execution and result handling
//! - Variable scoping and precedence
//! - Handler notification and execution
//! - Parallel execution strategies

use std::collections::HashMap;

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::{ExecutionContext, RegisteredResult, RuntimeContext};
use rustible::executor::task::{Handler, LoopSource, Task, TaskResult, TaskStatus};
use rustible::executor::{
    DependencyGraph, ExecutionStats, ExecutionStrategy, Executor, ExecutorConfig, HostResult,
};

// ============================================================================
// Executor Configuration Tests
// ============================================================================

#[test]
fn test_executor_config_default() {
    let config = ExecutorConfig::default();

    assert_eq!(config.forks, 5);
    assert!(!config.check_mode);
    assert!(!config.diff_mode);
    assert_eq!(config.verbosity, 0);
    assert_eq!(config.strategy, ExecutionStrategy::Linear);
    assert_eq!(config.task_timeout, 300);
    assert!(config.gather_facts);
    assert!(config.extra_vars.is_empty());
}

#[test]
fn test_executor_config_custom() {
    let mut extra_vars = HashMap::new();
    extra_vars.insert("env".to_string(), serde_json::json!("production"));

    let config = ExecutorConfig {
        forks: 10,
        check_mode: true,
        diff_mode: true,
        verbosity: 2,
        strategy: ExecutionStrategy::Free,
        task_timeout: 600,
        gather_facts: false,
        extra_vars,
        ..Default::default()
    };

    assert_eq!(config.forks, 10);
    assert!(config.check_mode);
    assert!(config.diff_mode);
    assert_eq!(config.verbosity, 2);
    assert_eq!(config.strategy, ExecutionStrategy::Free);
    assert_eq!(config.task_timeout, 600);
    assert!(!config.gather_facts);
    assert_eq!(
        config.extra_vars.get("env"),
        Some(&serde_json::json!("production"))
    );
}

// ============================================================================
// Execution Strategy Tests
// ============================================================================

#[test]
fn test_execution_strategy_equality() {
    assert_eq!(ExecutionStrategy::Linear, ExecutionStrategy::Linear);
    assert_eq!(ExecutionStrategy::Free, ExecutionStrategy::Free);
    assert_eq!(ExecutionStrategy::HostPinned, ExecutionStrategy::HostPinned);
    assert_ne!(ExecutionStrategy::Linear, ExecutionStrategy::Free);
}

// ============================================================================
// Execution Stats Tests
// ============================================================================

#[test]
fn test_execution_stats_default() {
    let stats = ExecutionStats::default();

    assert_eq!(stats.ok, 0);
    assert_eq!(stats.changed, 0);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.unreachable, 0);
}

#[test]
fn test_execution_stats_merge() {
    let mut stats1 = ExecutionStats {
        ok: 5,
        changed: 3,
        failed: 1,
        skipped: 2,
        unreachable: 0,
    };

    let stats2 = ExecutionStats {
        ok: 2,
        changed: 1,
        failed: 0,
        skipped: 1,
        unreachable: 1,
    };

    stats1.merge(&stats2);

    assert_eq!(stats1.ok, 7);
    assert_eq!(stats1.changed, 4);
    assert_eq!(stats1.failed, 1);
    assert_eq!(stats1.skipped, 3);
    assert_eq!(stats1.unreachable, 1);
}

// ============================================================================
// Runtime Context Tests
// ============================================================================

#[test]
fn test_runtime_context_new() {
    let ctx = RuntimeContext::new();

    assert!(ctx.get_all_hosts().is_empty());
    assert!(ctx.get_all_groups().is_empty());
}

#[test]
fn test_runtime_context_add_host() {
    let mut ctx = RuntimeContext::new();

    ctx.add_host("server1".to_string(), None);
    ctx.add_host("server2".to_string(), Some("webservers"));
    ctx.add_host("server3".to_string(), Some("webservers"));

    let all_hosts = ctx.get_all_hosts();
    assert_eq!(all_hosts.len(), 3);
    assert!(all_hosts.contains(&"server1".to_string()));
    assert!(all_hosts.contains(&"server2".to_string()));
    assert!(all_hosts.contains(&"server3".to_string()));

    let web_hosts = ctx.get_group_hosts("webservers").unwrap();
    assert_eq!(web_hosts.len(), 2);
    assert!(web_hosts.contains(&"server2".to_string()));
    assert!(web_hosts.contains(&"server3".to_string()));
}

#[test]
fn test_runtime_context_variable_precedence() {
    let mut ctx = RuntimeContext::new();
    let host = "server1";
    ctx.add_host(host.to_string(), None);

    // Set variables at different levels
    ctx.set_global_var("var".to_string(), serde_json::json!("global"));
    assert_eq!(
        ctx.get_var("var", Some(host)),
        Some(serde_json::json!("global"))
    );

    // Play vars override global
    ctx.set_play_var("var".to_string(), serde_json::json!("play"));
    assert_eq!(
        ctx.get_var("var", Some(host)),
        Some(serde_json::json!("play"))
    );

    // Task vars override play
    ctx.set_task_var(host, "var".to_string(), serde_json::json!("task"));
    assert_eq!(
        ctx.get_var("var", Some(host)),
        Some(serde_json::json!("task"))
    );

    // Extra vars override all
    ctx.set_extra_var("var".to_string(), serde_json::json!("extra"));
    assert_eq!(
        ctx.get_var("var", Some(host)),
        Some(serde_json::json!("extra"))
    );

    // Clear task vars, should fall back to extra (highest)
    ctx.clear_task_vars(host);
    assert_eq!(
        ctx.get_var("var", Some(host)),
        Some(serde_json::json!("extra"))
    );
}

#[test]
fn test_runtime_context_host_facts() {
    let mut ctx = RuntimeContext::new();
    ctx.add_host("server1".to_string(), None);

    ctx.set_host_fact(
        "server1",
        "os_family".to_string(),
        serde_json::json!("Debian"),
    );
    ctx.set_host_fact(
        "server1",
        "distribution".to_string(),
        serde_json::json!("Ubuntu"),
    );

    assert_eq!(
        ctx.get_host_fact("server1", "os_family"),
        Some(serde_json::json!("Debian"))
    );
    assert_eq!(
        ctx.get_host_fact("server1", "distribution"),
        Some(serde_json::json!("Ubuntu"))
    );
    assert_eq!(ctx.get_host_fact("server1", "nonexistent"), None);
    assert_eq!(ctx.get_host_fact("nonexistent", "os_family"), None);
}

#[test]
fn test_runtime_context_registered_results() {
    let mut ctx = RuntimeContext::new();
    ctx.add_host("server1".to_string(), None);

    let result = RegisteredResult {
        changed: true,
        failed: false,
        skipped: false,
        rc: Some(0),
        stdout: Some("success".to_string()),
        stdout_lines: Some(vec!["success".to_string()]),
        stderr: None,
        stderr_lines: None,
        msg: Some("Task completed".to_string()),
        results: None,
        data: Default::default(),
    };

    ctx.register_result("server1", "task_result".to_string(), result);

    let registered = ctx.get_registered("server1", "task_result").unwrap();
    assert!(registered.changed);
    assert!(!registered.failed);
    assert_eq!(registered.rc, Some(0));
    assert_eq!(registered.stdout, Some("success".to_string()));
}

#[test]
fn test_runtime_context_merged_vars() {
    let mut ctx = RuntimeContext::new();
    ctx.add_host("server1".to_string(), Some("webservers"));

    ctx.set_global_var("global_var".to_string(), serde_json::json!("global_value"));
    ctx.set_play_var("play_var".to_string(), serde_json::json!("play_value"));
    ctx.set_host_var(
        "server1",
        "host_var".to_string(),
        serde_json::json!("host_value"),
    );

    let merged = ctx.get_merged_vars("server1");

    assert_eq!(
        merged.get("global_var"),
        Some(&serde_json::json!("global_value"))
    );
    assert_eq!(
        merged.get("play_var"),
        Some(&serde_json::json!("play_value"))
    );
    assert_eq!(
        merged.get("host_var"),
        Some(&serde_json::json!("host_value"))
    );
    assert_eq!(
        merged.get("inventory_hostname"),
        Some(&serde_json::json!("server1"))
    );
}

// ============================================================================
// Task Tests
// ============================================================================

#[test]
fn test_task_builder() {
    let task = Task::new("Install nginx", "package")
        .arg("name", "nginx")
        .arg("state", "present")
        .when("ansible_os_family == 'Debian'")
        .notify("restart nginx")
        .register("install_result")
        .ignore_errors(true);

    assert_eq!(task.name, "Install nginx");
    assert_eq!(task.module, "package");
    assert_eq!(task.args.get("name"), Some(&serde_json::json!("nginx")));
    assert_eq!(task.args.get("state"), Some(&serde_json::json!("present")));
    assert_eq!(task.when, Some("ansible_os_family == 'Debian'".to_string()));
    assert!(task.notify.contains(&"restart nginx".to_string()));
    assert_eq!(task.register, Some("install_result".to_string()));
    assert!(task.ignore_errors);
}

#[test]
fn test_task_loop() {
    let task = Task::new("Install packages", "package")
        .arg("name", "{{ item }}")
        .arg("state", "present")
        .loop_over(vec![
            serde_json::json!("nginx"),
            serde_json::json!("php"),
            serde_json::json!("mysql"),
        ])
        .loop_var("item");

    assert!(task.loop_items.is_some());
    match task.loop_items.as_ref().unwrap() {
        LoopSource::Items(items) => assert_eq!(items.len(), 3),
        LoopSource::Template(_) => panic!("Expected Items, got Template"),
    }
    assert_eq!(task.loop_var, "item");
}

#[test]
fn test_task_result_states() {
    let ok = TaskResult::ok();
    assert_eq!(ok.status, TaskStatus::Ok);
    assert!(!ok.changed);

    let changed = TaskResult::changed();
    assert_eq!(changed.status, TaskStatus::Changed);
    assert!(changed.changed);

    let failed = TaskResult::failed("Something went wrong");
    assert_eq!(failed.status, TaskStatus::Failed);
    assert!(!failed.changed);
    assert_eq!(failed.msg, Some("Something went wrong".to_string()));

    let skipped = TaskResult::skipped("Condition not met");
    assert_eq!(skipped.status, TaskStatus::Skipped);
    assert!(!skipped.changed);
    assert_eq!(skipped.msg, Some("Condition not met".to_string()));

    let unreachable = TaskResult::unreachable("Host offline");
    assert_eq!(unreachable.status, TaskStatus::Unreachable);
    assert_eq!(unreachable.msg, Some("Host offline".to_string()));
}

#[test]
fn test_task_result_with_data() {
    let result = TaskResult::changed()
        .with_msg("Package installed")
        .with_result(serde_json::json!({"version": "1.0.0"}));

    assert!(result.changed);
    assert_eq!(result.msg, Some("Package installed".to_string()));
    assert_eq!(result.result, Some(serde_json::json!({"version": "1.0.0"})));
}

// ============================================================================
// Handler Tests
// ============================================================================

#[test]
fn test_handler_definition() {
    let handler = Handler {
        name: "restart nginx".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("nginx"));
            args.insert("state".to_string(), serde_json::json!("restarted"));
            args
        },
        when: None,
        listen: vec!["nginx config changed".to_string()],
    };

    assert_eq!(handler.name, "restart nginx");
    assert_eq!(handler.module, "service");
    assert!(handler.listen.contains(&"nginx config changed".to_string()));
}

// ============================================================================
// Playbook Tests
// ============================================================================

#[test]
fn test_playbook_new() {
    let playbook = Playbook::new("My Playbook");

    assert_eq!(playbook.name, "My Playbook");
    assert!(playbook.plays.is_empty());
    assert!(playbook.vars.is_empty());
}

#[test]
fn test_playbook_with_plays() {
    let mut playbook = Playbook::new("Multi-play Playbook");

    let mut play1 = Play::new("Configure webservers", "webservers");
    play1.add_task(Task::new("Install nginx", "package").arg("name", "nginx"));

    let mut play2 = Play::new("Configure databases", "databases");
    play2.add_task(Task::new("Install postgres", "package").arg("name", "postgresql"));

    playbook.add_play(play1);
    playbook.add_play(play2);

    assert_eq!(playbook.plays.len(), 2);
    assert_eq!(playbook.plays[0].name, "Configure webservers");
    assert_eq!(playbook.plays[1].name, "Configure databases");
}

#[test]
fn test_play_with_handlers() {
    let mut play = Play::new("Web Setup", "all");

    play.add_task(
        Task::new("Copy nginx config", "copy")
            .arg("src", "nginx.conf")
            .arg("dest", "/etc/nginx/nginx.conf")
            .notify("restart nginx"),
    );

    play.add_handler(Handler {
        name: "restart nginx".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("nginx"));
            args.insert("state".to_string(), serde_json::json!("restarted"));
            args
        },
        when: None,
        listen: vec![],
    });

    assert_eq!(play.tasks.len(), 1);
    assert_eq!(play.handlers.len(), 1);
    assert!(play.tasks[0].notify.contains(&"restart nginx".to_string()));
}

// ============================================================================
// Playbook Parsing Tests
// ============================================================================

#[test]
fn test_parse_simple_playbook() {
    let yaml = r#"
- name: Test Play
  hosts: all
  gather_facts: false
  tasks:
    - name: Debug message
      debug:
        msg: "Hello World"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();

    assert_eq!(playbook.plays.len(), 1);
    assert_eq!(playbook.plays[0].name, "Test Play");
    assert_eq!(playbook.plays[0].hosts, "all");
    assert!(!playbook.plays[0].gather_facts);
    assert_eq!(playbook.plays[0].tasks.len(), 1);
}

#[test]
fn test_parse_playbook_with_vars() {
    let yaml = r#"
- name: Play with vars
  hosts: webservers
  vars:
    http_port: 80
    server_name: example.com
  tasks:
    - name: Show vars
      debug:
        msg: "Port: {{ http_port }}"
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();

    assert_eq!(playbook.plays[0].vars.len(), 2);
    assert_eq!(
        playbook.plays[0].vars.get("http_port"),
        Some(&serde_json::json!(80))
    );
}

#[test]
fn test_parse_playbook_with_handlers() {
    let yaml = r#"
- name: Web config
  hosts: webservers
  tasks:
    - name: Update config
      copy:
        src: app.conf
        dest: /etc/app/app.conf
      notify: restart app
  handlers:
    - name: restart app
      service:
        name: app
        state: restarted
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();

    assert_eq!(playbook.plays[0].handlers.len(), 1);
    assert_eq!(playbook.plays[0].handlers[0].name, "restart app");
}

#[test]
fn test_parse_playbook_with_become() {
    let yaml = r#"
- name: Privileged tasks
  hosts: all
  become: true
  become_user: root
  tasks:
    - name: Install package
      package:
        name: vim
"#;

    let playbook = Playbook::parse(yaml, None).unwrap();

    assert!(playbook.plays[0].r#become);
    assert_eq!(playbook.plays[0].become_user, Some("root".to_string()));
}

// ============================================================================
// Dependency Graph Tests
// ============================================================================

#[test]
fn test_dependency_graph_empty() {
    let graph = DependencyGraph::new();
    let order = graph.topological_sort().unwrap();
    assert!(order.is_empty());
}

#[test]
fn test_dependency_graph_single_chain() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("task3", "task2");
    graph.add_dependency("task2", "task1");

    let order = graph.topological_sort().unwrap();

    // task1 should come before task2, task2 before task3
    let pos1 = order.iter().position(|x| x == "task1").unwrap();
    let pos2 = order.iter().position(|x| x == "task2").unwrap();
    let pos3 = order.iter().position(|x| x == "task3").unwrap();

    assert!(pos1 < pos2);
    assert!(pos2 < pos3);
}

#[test]
fn test_dependency_graph_diamond() {
    let mut graph = DependencyGraph::new();
    // Diamond dependency: task4 depends on task2 and task3, both depend on task1
    graph.add_dependency("task4", "task2");
    graph.add_dependency("task4", "task3");
    graph.add_dependency("task2", "task1");
    graph.add_dependency("task3", "task1");

    let order = graph.topological_sort().unwrap();

    let pos1 = order.iter().position(|x| x == "task1").unwrap();
    let pos2 = order.iter().position(|x| x == "task2").unwrap();
    let pos3 = order.iter().position(|x| x == "task3").unwrap();
    let pos4 = order.iter().position(|x| x == "task4").unwrap();

    assert!(pos1 < pos2);
    assert!(pos1 < pos3);
    assert!(pos2 < pos4);
    assert!(pos3 < pos4);
}

#[test]
fn test_dependency_graph_cycle_detection() {
    let mut graph = DependencyGraph::new();
    graph.add_dependency("task1", "task2");
    graph.add_dependency("task2", "task3");
    graph.add_dependency("task3", "task1"); // Creates cycle

    let result = graph.topological_sort();
    assert!(result.is_err());
}

// ============================================================================
// Async Execution Tests
// ============================================================================

#[tokio::test]
async fn test_executor_creation() {
    let config = ExecutorConfig::default();
    let executor = Executor::new(config);

    assert!(!executor.is_check_mode());
}

#[tokio::test]
async fn test_executor_with_check_mode() {
    let config = ExecutorConfig {
        check_mode: true,
        ..Default::default()
    };
    let executor = Executor::new(config);

    assert!(executor.is_check_mode());
}

#[tokio::test]
async fn test_executor_with_runtime() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("server1".to_string(), Some("webservers"));
    runtime.add_host("server2".to_string(), Some("webservers"));

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let runtime_lock = executor.runtime();
    let runtime = runtime_lock.read().await;

    assert_eq!(runtime.get_all_hosts().len(), 2);
}

#[tokio::test]
async fn test_executor_run_simple_playbook() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    // Create a simple playbook
    let mut playbook = Playbook::new("Test Playbook");
    let mut play = Play::new("Test Play", "all");
    play.gather_facts = false;

    play.add_task(Task::new("Debug test", "debug").arg("msg", "Hello"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    assert!(results.contains_key("localhost"));
    let host_result = results.get("localhost").unwrap();
    assert!(!host_result.failed);
}

#[tokio::test]
async fn test_executor_summarize_results() {
    let mut results = HashMap::new();

    results.insert(
        "host1".to_string(),
        HostResult {
            host: "host1".to_string(),
            stats: ExecutionStats {
                ok: 5,
                changed: 2,
                failed: 0,
                skipped: 1,
                unreachable: 0,
            },
            failed: false,
            unreachable: false,
        },
    );

    results.insert(
        "host2".to_string(),
        HostResult {
            host: "host2".to_string(),
            stats: ExecutionStats {
                ok: 3,
                changed: 4,
                failed: 1,
                skipped: 0,
                unreachable: 0,
            },
            failed: true,
            unreachable: false,
        },
    );

    let summary = Executor::summarize_results(&results);

    assert_eq!(summary.ok, 8);
    assert_eq!(summary.changed, 6);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.skipped, 1);
    assert_eq!(summary.unreachable, 0);
}

// ============================================================================
// Execution Context Tests
// ============================================================================

#[test]
fn test_execution_context_new() {
    let ctx = ExecutionContext::new("server1");

    assert_eq!(ctx.host, "server1");
    assert!(!ctx.check_mode);
    assert!(!ctx.diff_mode);
}

#[test]
fn test_execution_context_builder() {
    let ctx = ExecutionContext::new("server1")
        .with_check_mode(true)
        .with_diff_mode(true);

    assert_eq!(ctx.host, "server1");
    assert!(ctx.check_mode);
    assert!(ctx.diff_mode);
}

// ============================================================================
// Registered Result Tests
// ============================================================================

#[test]
fn test_registered_result_ok() {
    let result = RegisteredResult::ok(true);

    assert!(result.changed);
    assert!(!result.failed);
    assert!(!result.skipped);
}

#[test]
fn test_registered_result_failed() {
    let result = RegisteredResult::failed("Error message");

    assert!(!result.changed);
    assert!(result.failed);
    assert_eq!(result.msg, Some("Error message".to_string()));
}

#[test]
fn test_registered_result_skipped() {
    let result = RegisteredResult::skipped("Condition not met");

    assert!(!result.changed);
    assert!(!result.failed);
    assert!(result.skipped);
    assert_eq!(result.msg, Some("Condition not met".to_string()));
}

#[test]
fn test_registered_result_to_json() {
    let result = RegisteredResult {
        changed: true,
        failed: false,
        skipped: false,
        rc: Some(0),
        stdout: Some("output".to_string()),
        ..Default::default()
    };

    let json = result.to_json();

    assert!(json.is_object());
    assert_eq!(json.get("changed"), Some(&serde_json::json!(true)));
    assert_eq!(json.get("rc"), Some(&serde_json::json!(0)));
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_full_playbook_execution() {
    // Set up a runtime with multiple hosts
    let mut runtime = RuntimeContext::new();
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("db1".to_string(), Some("databases"));

    // Add some variables
    runtime.set_global_var("environment".to_string(), serde_json::json!("staging"));

    let config = ExecutorConfig {
        forks: 3,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    // Create a multi-play playbook
    let mut playbook = Playbook::new("Full Integration Test");

    // Play 1: Configure webservers
    let mut web_play = Play::new("Configure Web Servers", "webservers");
    web_play.gather_facts = false;
    web_play.add_task(Task::new("Install nginx", "package").arg("name", "nginx"));
    web_play.add_task(
        Task::new("Configure nginx", "template")
            .arg("src", "nginx.conf.j2")
            .arg("dest", "/etc/nginx/nginx.conf")
            .notify("restart nginx"),
    );
    web_play.add_handler(Handler {
        name: "restart nginx".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("nginx"));
            args.insert("state".to_string(), serde_json::json!("restarted"));
            args
        },
        when: None,
        listen: vec![],
    });
    playbook.add_play(web_play);

    // Play 2: Configure databases
    let mut db_play = Play::new("Configure Databases", "databases");
    db_play.gather_facts = false;
    db_play.add_task(Task::new("Install postgres", "package").arg("name", "postgresql"));
    playbook.add_play(db_play);

    // Execute
    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify results
    assert_eq!(results.len(), 3);
    assert!(results.contains_key("web1"));
    assert!(results.contains_key("web2"));
    assert!(results.contains_key("db1"));

    // Web servers should have run more tasks
    let web1 = results.get("web1").unwrap();
    let db1 = results.get("db1").unwrap();

    // Web1 should have more stats than db1
    assert!(web1.stats.changed >= db1.stats.changed || web1.stats.ok >= db1.stats.ok);
}

#[tokio::test]
async fn test_conditional_task_execution() {
    // Use localhost to ensure a local connection (no SSH needed)
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_host_var(
        "localhost",
        "install_nginx".to_string(),
        serde_json::json!(true),
    );

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Conditional Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    // Task with truthy variable condition - should run
    play.add_task(
        Task::new("Should run", "debug")
            .arg("msg", "Running because install_nginx is true")
            .when("install_nginx"),
    );

    // Task with literal false condition - should skip
    play.add_task(
        Task::new("Should skip", "debug")
            .arg("msg", "This should never appear")
            .when("false"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("localhost").unwrap();

    // Verify no connection issues
    assert_eq!(
        host_result.stats.unreachable, 0,
        "Host should be reachable (using local connection)"
    );

    // Verify the conditional task ran (ok or changed, depending on module)
    assert!(
        host_result.stats.ok >= 1 || host_result.stats.changed >= 1,
        "Expected at least one task to succeed, got: ok={}, changed={}",
        host_result.stats.ok,
        host_result.stats.changed
    );

    // Verify the false-condition task was skipped
    assert!(
        host_result.stats.skipped >= 1,
        "Expected at least one task to be skipped, got: skipped={}",
        host_result.stats.skipped
    );
}

#[tokio::test]
async fn test_loop_task_execution() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("server1".to_string(), None);

    let config = ExecutorConfig::default();
    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Loop Test");
    let mut play = Play::new("Test", "all");
    play.gather_facts = false;

    play.add_task(
        Task::new("Install packages", "debug")
            .arg("msg", "Installing {{ item }}")
            .loop_over(vec![
                serde_json::json!("package1"),
                serde_json::json!("package2"),
                serde_json::json!("package3"),
            ]),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();
    let host_result = results.get("server1").unwrap();

    // Loop ran successfully
    assert!(!host_result.failed);
}
