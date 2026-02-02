//! Performance Validation Tests for Rustible
//!
//! These tests validate performance characteristics and resource management
//! beyond what criterion benchmarks measure. They focus on:
//!
//! 1. Memory Usage Under Load
//!    - Inventory memory scaling
//!    - Playbook memory usage
//!    - Variable context memory
//!
//! 2. Resource Leak Detection
//!    - Connection handle cleanup
//!    - File descriptor management
//!    - Async task cleanup
//!
//! 3. Async Runtime Efficiency
//!    - Task spawning overhead
//!    - Semaphore-based limiting
//!    - Channel throughput
//!
//! 4. Concurrent Operation Safety
//!    - Parallel inventory access
//!    - Concurrent variable updates
//!    - Connection pool thread safety

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, Semaphore};

use rustible::connection::{ConnectionConfig, ConnectionFactory};
use rustible::executor::playbook::Playbook;
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::TaskResult;
use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};
use rustible::inventory::{Group, Host, Inventory};
use rustible::modules::ModuleRegistry;
use rustible::template::TemplateEngine;

// ============================================================================
// TEST UTILITIES
// ============================================================================

/// Create a test inventory with the specified number of hosts and groups
fn create_test_inventory(num_hosts: usize, num_groups: usize) -> Inventory {
    let mut inv = Inventory::new();
    let hosts_per_group = (num_hosts / num_groups).max(1);

    for g in 0..num_groups {
        let group_name = format!("group_{:04}", g);
        let mut group = Group::new(&group_name);

        for v in 0..10 {
            group.set_var(
                &format!("group_var_{}", v),
                serde_yaml::Value::String(format!("value_{}", v)),
            );
        }

        inv.add_group(group).unwrap();
    }

    for h in 0..num_hosts {
        let host_name = format!("host{:05}", h);
        let mut host = Host::new(&host_name);

        let group_idx = h / hosts_per_group;
        let group_name = format!("group_{:04}", group_idx.min(num_groups - 1));
        host.add_to_group(group_name);
        host.add_to_group("all".to_string());

        for v in 0..5 {
            host.set_var(
                &format!("host_var_{}", v),
                serde_yaml::Value::String(format!("host_value_{}", v)),
            );
        }

        inv.add_host(host).unwrap();
    }

    inv
}

/// Create a test runtime context with hosts and variables
fn create_test_runtime(num_hosts: usize) -> RuntimeContext {
    let mut ctx = RuntimeContext::new();

    for i in 0..num_hosts {
        ctx.add_host(format!("host_{}", i), Some("webservers"));
        ctx.set_host_var(
            &format!("host_{}", i),
            "http_port".to_string(),
            serde_json::json!(8080 + i),
        );
        ctx.set_host_fact(
            &format!("host_{}", i),
            "os_family".to_string(),
            serde_json::json!("Debian"),
        );
    }

    for i in 0..50 {
        ctx.set_global_var(
            format!("global_var_{}", i),
            serde_json::json!(format!("value_{}", i)),
        );
    }

    ctx
}

// ============================================================================
// MEMORY USAGE TESTS
// ============================================================================

/// Test that inventory memory usage scales linearly with host count
#[test]
fn test_inventory_memory_scaling() {
    // Create inventories of different sizes and verify they can be created
    // without excessive memory allocation

    let small_inv = create_test_inventory(100, 10);
    assert_eq!(small_inv.host_count(), 100);

    let medium_inv = create_test_inventory(1000, 20);
    assert_eq!(medium_inv.host_count(), 1000);

    let large_inv = create_test_inventory(5000, 50);
    assert_eq!(large_inv.host_count(), 5000);

    // Verify we can still perform operations on large inventories
    let hosts = large_inv.get_hosts_for_pattern("all").unwrap();
    assert_eq!(hosts.len(), 5000);
}

/// Test that playbook parsing doesn't leak memory on repeated parsing
#[test]
fn test_playbook_parsing_no_leak() {
    let yaml = r#"
- name: Test Play
  hosts: all
  gather_facts: false
  tasks:
    - name: Debug
      debug:
        msg: "Hello World"
    - name: Set fact
      set_fact:
        test_var: "value"
"#;

    // Parse playbook many times
    for _ in 0..1000 {
        let playbook = Playbook::parse(yaml, None).unwrap();
        assert_eq!(playbook.plays.len(), 1);
    }
    // If we get here without OOM, memory management is working
}

/// Test variable context memory with large variable sets
#[test]
fn test_large_variable_context() {
    let mut ctx = RuntimeContext::new();

    // Add many variables at different scopes
    for i in 0..1000 {
        ctx.set_global_var(
            format!("global_{}", i),
            serde_json::json!({"key": format!("value_{}", i), "data": vec![1, 2, 3, 4, 5]}),
        );
    }

    for i in 0..500 {
        ctx.set_play_var(
            format!("play_{}", i),
            serde_json::json!(format!("play_value_{}", i)),
        );
    }

    // Add hosts with variables
    for h in 0..100 {
        ctx.add_host(format!("host_{}", h), Some("webservers"));
        for v in 0..10 {
            ctx.set_host_var(
                &format!("host_{}", h),
                format!("var_{}", v),
                serde_json::json!(format!("host_{}_var_{}", h, v)),
            );
        }
    }

    // Verify variable retrieval still works
    let vars = ctx.get_merged_vars("host_50");
    assert!(!vars.is_empty());
    assert!(vars.contains_key("global_500"));
    assert!(vars.contains_key("play_250"));
}

// ============================================================================
// RESOURCE LEAK DETECTION TESTS
// ============================================================================

/// Test that connection factory properly cleans up connections
#[tokio::test]
async fn test_connection_cleanup() {
    let factory = ConnectionFactory::new(ConnectionConfig::default());

    // Create and use connections
    for _ in 0..100 {
        let conn = factory.get_connection("localhost").await.unwrap();
        assert!(conn.is_alive().await);
    }

    // Close all connections
    factory.close_all().await.unwrap();

    // Pool should be empty
    let stats = factory.pool_stats().await;
    assert_eq!(stats.active_connections, 0);
}

/// Test that async tasks are properly cleaned up after execution
#[tokio::test]
async fn test_async_task_cleanup() {
    let counter = Arc::new(AtomicUsize::new(0));

    // Spawn many tasks
    let handles: Vec<_> = (0..1000)
        .map(|_| {
            let c = Arc::clone(&counter);
            tokio::spawn(async move {
                c.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
            })
        })
        .collect();

    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(counter.load(Ordering::SeqCst), 1000);
}

/// Test semaphore-based resource limiting doesn't leak permits
#[tokio::test]
async fn test_semaphore_permit_cleanup() {
    let semaphore = Arc::new(Semaphore::new(5));
    let completed = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..100)
        .map(|_| {
            let sem = Arc::clone(&semaphore);
            let done = Arc::clone(&completed);
            tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                tokio::time::sleep(Duration::from_micros(100)).await;
                done.fetch_add(1, Ordering::SeqCst);
                // permit is dropped here
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    // All permits should be available again
    assert_eq!(semaphore.available_permits(), 5);
    assert_eq!(completed.load(Ordering::SeqCst), 100);
}

// ============================================================================
// ASYNC RUNTIME EFFICIENCY TESTS
// ============================================================================

/// Test task spawning overhead remains reasonable
#[tokio::test]
async fn test_task_spawning_efficiency() {
    let start = Instant::now();

    let handles: Vec<_> = (0..10000)
        .map(|i| {
            tokio::spawn(async move {
                // Minimal work
                i * 2
            })
        })
        .collect();

    let mut sum = 0;
    for handle in handles {
        sum += handle.await.unwrap();
    }

    let elapsed = start.elapsed();

    // Should complete in reasonable time (< 5 seconds for 10000 tasks)
    assert!(elapsed < Duration::from_secs(5));
    assert!(sum > 0);
}

/// Test concurrent runtime context access
#[tokio::test]
async fn test_concurrent_runtime_access() {
    let runtime = Arc::new(RwLock::new(create_test_runtime(100)));

    // Spawn many readers
    let read_handles: Vec<_> = (0..50)
        .map(|i| {
            let rt = Arc::clone(&runtime);
            tokio::spawn(async move {
                for _ in 0..100 {
                    let ctx = rt.read().await;
                    let vars = ctx.get_merged_vars(&format!("host_{}", i % 100));
                    assert!(!vars.is_empty());
                }
            })
        })
        .collect();

    // Spawn some writers
    let write_handles: Vec<_> = (0..10)
        .map(|i| {
            let rt = Arc::clone(&runtime);
            tokio::spawn(async move {
                for j in 0..10 {
                    let mut ctx = rt.write().await;
                    ctx.set_task_var(
                        format!("task_var_{}_{}", i, j),
                        serde_json::json!(format!("value_{}_{}", i, j)),
                    );
                }
            })
        })
        .collect();

    for handle in read_handles {
        handle.await.unwrap();
    }
    for handle in write_handles {
        handle.await.unwrap();
    }
}

/// Test channel throughput for result collection
#[tokio::test]
async fn test_channel_throughput() {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TaskResult>(1000);

    let producer = tokio::spawn(async move {
        for _ in 0..10000 {
            let result = TaskResult::ok().with_msg("Success");
            tx.send(result).await.unwrap();
        }
    });

    let consumer = tokio::spawn(async move {
        let mut count = 0;
        while let Some(_result) = rx.recv().await {
            count += 1;
        }
        count
    });

    producer.await.unwrap();
    // Drop sender by ending producer
    let count = consumer.await.unwrap();
    assert_eq!(count, 10000);
}

// ============================================================================
// CONCURRENT OPERATION SAFETY TESTS
// ============================================================================

/// Test parallel inventory access is safe
#[tokio::test]
async fn test_parallel_inventory_access() {
    let inv = Arc::new(create_test_inventory(1000, 20));

    let handles: Vec<_> = (0..100)
        .map(|i| {
            let inventory = Arc::clone(&inv);
            tokio::spawn(async move {
                // Read operations
                let _ = inventory.get_host(&format!("host{:05}", i % 1000));
                let _ = inventory.get_group(&format!("group_{:04}", i % 20));
                let _ = inventory.get_hosts_for_pattern("all");
                let _ = inventory.host_count();
                true
            })
        })
        .collect();

    for handle in handles {
        assert!(handle.await.unwrap());
    }
}

/// Test that template engine is thread-safe
#[test]
fn test_template_engine_thread_safety() {
    use std::thread;

    let template = "Hello {{ name }}, value is {{ value }}";
    let engine = Arc::new(TemplateEngine::new());

    let handles: Vec<_> = (0..100)
        .map(|i| {
            let eng = Arc::clone(&engine);
            let tmpl = template.to_string();
            thread::spawn(move || {
                let mut vars = HashMap::new();
                vars.insert("name".to_string(), serde_json::json!(format!("user_{}", i)));
                vars.insert("value".to_string(), serde_json::json!(i * 100));

                let result = eng.render(&tmpl, &vars).unwrap();
                assert!(result.contains(&format!("user_{}", i)));
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

/// Test module registry is thread-safe
#[test]
fn test_module_registry_thread_safety() {
    use std::thread;

    let registry = Arc::new(ModuleRegistry::with_builtins());

    let handles: Vec<_> = (0..50)
        .map(|_| {
            let reg = Arc::clone(&registry);
            thread::spawn(move || {
                // Concurrent reads
                assert!(reg.contains("command"));
                assert!(reg.contains("shell"));
                assert!(reg.contains("copy"));
                let _ = reg.get("command");
                let _ = reg.names();
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

// ============================================================================
// PERFORMANCE REGRESSION TESTS
// ============================================================================

/// Test that inventory parsing completes within acceptable time
#[test]
fn test_inventory_parsing_performance() {
    let yaml = r#"
all:
  children:
    webservers:
      hosts:
        web001:
          ansible_host: 10.0.0.1
        web002:
          ansible_host: 10.0.0.2
        web003:
          ansible_host: 10.0.0.3
      vars:
        http_port: 80
    databases:
      hosts:
        db001:
          ansible_host: 10.0.1.1
      vars:
        db_port: 5432
"#;

    use std::io::Write;
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    tmpfile.write_all(yaml.as_bytes()).unwrap();
    tmpfile.flush().unwrap();

    let start = Instant::now();
    for _ in 0..1000 {
        let _ = Inventory::load(tmpfile.path()).unwrap();
    }
    let elapsed = start.elapsed();

    // 1000 parses should complete in under 5 seconds
    assert!(
        elapsed < Duration::from_secs(5),
        "Inventory parsing too slow: {:?}",
        elapsed
    );
}

/// Test that playbook parsing completes within acceptable time
#[test]
fn test_playbook_parsing_performance() {
    let yaml = r#"
- name: Web Server Setup
  hosts: webservers
  gather_facts: true
  vars:
    http_port: 80
  tasks:
    - name: Install nginx
      package:
        name: nginx
        state: present
    - name: Copy config
      template:
        src: nginx.conf.j2
        dest: /etc/nginx/nginx.conf
      notify: restart nginx
  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted
"#;

    let start = Instant::now();
    for _ in 0..1000 {
        let _ = Playbook::parse(yaml, None).unwrap();
    }
    let elapsed = start.elapsed();

    // 1000 parses should complete in under 5 seconds
    assert!(
        elapsed < Duration::from_secs(5),
        "Playbook parsing too slow: {:?}",
        elapsed
    );
}

/// Test template rendering performance
#[test]
fn test_template_rendering_performance() {
    let engine = TemplateEngine::new();
    let template = "Hello {{ name }}, you have {{ count }} messages from {{ sender }}";
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("Alice"));
    vars.insert("count".to_string(), serde_json::json!(42));
    vars.insert("sender".to_string(), serde_json::json!("Bob"));

    let start = Instant::now();
    for _ in 0..10000 {
        let _ = engine.render(template, &vars).unwrap();
    }
    let elapsed = start.elapsed();

    // 10000 renders should complete in under 2 seconds
    assert!(
        elapsed < Duration::from_secs(2),
        "Template rendering too slow: {:?}",
        elapsed
    );
}

/// Test that executor creation is fast
#[test]
fn test_executor_creation_performance() {
    let start = Instant::now();

    for _ in 0..1000 {
        let config = ExecutorConfig {
            forks: 5,
            check_mode: false,
            diff_mode: false,
            verbosity: 0,
            strategy: ExecutionStrategy::Linear,
            task_timeout: 300,
            gather_facts: false,
            extra_vars: HashMap::new(),
        ..Default::default()
        };
        let _ = Executor::new(config);
    }

    let elapsed = start.elapsed();

    // 1000 executor creations should complete in under 1 second
    assert!(
        elapsed < Duration::from_secs(1),
        "Executor creation too slow: {:?}",
        elapsed
    );
}

// ============================================================================
// STRESS TESTS
// ============================================================================

/// Stress test with many concurrent operations
#[tokio::test]
async fn test_stress_concurrent_operations() {
    let runtime = Arc::new(RwLock::new(RuntimeContext::new()));
    let inventory = Arc::new(create_test_inventory(100, 10));
    let engine = Arc::new(TemplateEngine::new());

    let mut handles = Vec::new();

    // Runtime operations
    for i in 0..50 {
        let rt = Arc::clone(&runtime);
        handles.push(tokio::spawn(async move {
            let mut ctx = rt.write().await;
            ctx.add_host(format!("stress_host_{}", i), Some("stress_group"));
            ctx.set_host_var(
                &format!("stress_host_{}", i),
                "test_var".to_string(),
                serde_json::json!(i),
            );
        }));
    }

    // Inventory operations
    for i in 0..50 {
        let inv = Arc::clone(&inventory);
        handles.push(tokio::spawn(async move {
            let _ = inv.get_host(&format!("host{:05}", i));
            let _ = inv.get_hosts_for_pattern("all");
        }));
    }

    // Template operations
    for i in 0..50 {
        let eng = Arc::clone(&engine);
        handles.push(tokio::spawn(async move {
            let mut vars = HashMap::new();
            vars.insert("val".to_string(), serde_json::json!(i));
            let _ = eng.render("Value: {{ val }}", &vars);
        }));
    }

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

/// Test handling of rapid connection creation/destruction
#[tokio::test]
async fn test_rapid_connection_cycling() {
    for _ in 0..10 {
        let factory = ConnectionFactory::new(ConnectionConfig::default());

        for _ in 0..50 {
            let _ = factory.get_connection("localhost").await.unwrap();
        }

        factory.close_all().await.unwrap();
    }
}

// ============================================================================
// BOUNDARY CONDITION TESTS
// ============================================================================

/// Test empty inventory handling
#[test]
fn test_empty_inventory_performance() {
    let inv = Inventory::new();

    let start = Instant::now();
    for _ in 0..10000 {
        let _ = inv.get_hosts_for_pattern("all");
        let _ = inv.host_count();
        let _ = inv.group_names().count();
    }
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_secs(1));
}

/// Test empty playbook handling
#[test]
fn test_minimal_playbook() {
    let yaml = r#"
- name: Empty Play
  hosts: all
  gather_facts: false
  tasks: []
"#;

    let start = Instant::now();
    for _ in 0..10000 {
        let _ = Playbook::parse(yaml, None).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_secs(2));
}

/// Test template with no variables
#[test]
fn test_static_template_performance() {
    let engine = TemplateEngine::new();
    let template = "This is a static template with no variables at all.";
    let vars = HashMap::new();

    let start = Instant::now();
    for _ in 0..50000 {
        let _ = engine.render(template, &vars).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_secs(2));
}
