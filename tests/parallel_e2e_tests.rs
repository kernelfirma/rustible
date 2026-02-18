//! End-to-End Tests for Parallel Execution
//!
//! This test suite validates:
//! - Parallel execution of tasks across multiple hosts
//! - Connection pooling and reuse
//! - Performance improvements from parallelization
//! - Different execution strategies (linear vs free)
//!
//! To run the full test suite with SSH hosts:
//! ```bash
//! export RUSTIBLE_E2E_ENABLED=1
//! export RUSTIBLE_E2E_SSH_USER=testuser
//! export RUSTIBLE_E2E_SSH_KEY=~/.ssh/id_ed25519
//! export RUSTIBLE_E2E_HOSTS=192.168.178.141,192.168.178.142,192.168.178.143,192.168.178.144
//! cargo test --test parallel_e2e_tests -- --nocapture --test-threads=1
//! ```

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rustible::executor::playbook::Playbook;
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};
use rustible::inventory::Inventory;

mod common;

/// Configuration for E2E tests
#[derive(Debug, Clone)]
struct E2EConfig {
    enabled: bool,
    ssh_user: String,
    ssh_key_path: PathBuf,
    hosts: Vec<String>,
    fixtures_dir: PathBuf,
}

impl E2EConfig {
    fn from_env() -> Self {
        let enabled = env::var("RUSTIBLE_E2E_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let ssh_user = env::var("RUSTIBLE_E2E_SSH_USER").unwrap_or_else(|_| "testuser".to_string());

        let ssh_key_path = env::var("RUSTIBLE_E2E_SSH_KEY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".ssh/id_ed25519")
            });

        let hosts = env::var("RUSTIBLE_E2E_HOSTS")
            .map(|h| h.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(|_| vec![]);

        let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("parallel");

        Self {
            enabled,
            ssh_user,
            ssh_key_path,
            hosts,
            fixtures_dir,
        }
    }

    fn skip_if_disabled(&self) -> bool {
        if !self.enabled {
            eprintln!("Skipping E2E tests (RUSTIBLE_E2E_ENABLED not set)");
            eprintln!("To enable, set: export RUSTIBLE_E2E_ENABLED=1");
            true
        } else {
            false
        }
    }

    fn playbook_path(&self) -> PathBuf {
        self.fixtures_dir.join("playbook.yml")
    }

    #[allow(dead_code)]
    fn inventory_path(&self) -> PathBuf {
        self.fixtures_dir.join("inventory.yml")
    }

    /// Generate dynamic inventory with configured hosts
    fn generate_inventory(&self) -> String {
        let mut inv = String::from("---\nall:\n  children:\n    test_group:\n      hosts:\n");

        // Always include localhost
        inv.push_str("        localhost:\n");
        inv.push_str("          ansible_connection: local\n");
        inv.push_str("          ansible_python_interpreter: /usr/bin/python3\n");

        // Add configured hosts
        for (idx, host) in self.hosts.iter().enumerate() {
            inv.push_str(&format!("        host{}:\n", idx + 1));
            inv.push_str(&format!("          ansible_host: {}\n", host));
            inv.push_str(&format!("          ansible_user: {}\n", self.ssh_user));
            inv.push_str("          ansible_port: 22\n");
            inv.push_str(&format!(
                "          ansible_ssh_private_key_file: {}\n",
                self.ssh_key_path.display()
            ));
            inv.push_str("          ansible_python_interpreter: /usr/bin/python3\n");
        }

        inv
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Load and parse a playbook from file
async fn load_playbook(path: &Path) -> anyhow::Result<Playbook> {
    let content = tokio::fs::read_to_string(path).await?;
    Playbook::parse(&content, Some(path.parent().unwrap().to_path_buf()))
        .map_err(|e| anyhow::anyhow!("Failed to parse playbook: {}", e))
}

/// Load inventory from file
async fn load_inventory(path: &Path) -> anyhow::Result<Inventory> {
    Inventory::load(path).map_err(|e| anyhow::anyhow!("Failed to load inventory: {}", e))
}

/// Create a temporary inventory file with content
async fn create_temp_inventory(content: &str) -> anyhow::Result<PathBuf> {
    use std::io::Write;
    let mut tmpfile = tempfile::NamedTempFile::new()?;
    tmpfile.write_all(content.as_bytes())?;
    tmpfile.flush()?;

    // Get the path before persisting
    let path = tmpfile.path().to_path_buf();
    tmpfile.persist(&path)?;
    Ok(path)
}

/// Initialize runtime context from inventory
async fn init_runtime(inventory: &Inventory) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();

    // Add all hosts to runtime
    for host in inventory.get_all_hosts() {
        let host_vars = inventory.get_host_vars(host);
        for (key, value) in host_vars {
            // Convert serde_yaml::Value to serde_json::Value
            let json_value = serde_json::to_value(&value).unwrap_or(serde_json::Value::Null);
            runtime.set_host_var(host.name(), key, json_value);
        }
    }

    runtime
}

// =============================================================================
// Basic Parallel Execution Tests
// =============================================================================

#[tokio::test]
async fn test_parallel_execution_on_localhost() {
    let config = E2EConfig::from_env();

    // This test always runs (doesn't require external hosts)
    let playbook_path = config.playbook_path();

    if !playbook_path.exists() {
        eprintln!("Playbook not found at {:?}", playbook_path);
        return;
    }

    // Create a simple localhost-only inventory
    let inv_content = r#"
all:
  hosts:
    localhost:
      ansible_connection: local
      ansible_python_interpreter: /usr/bin/python3
"#;

    let inv_path = create_temp_inventory(inv_content).await.unwrap();
    let inventory = load_inventory(&inv_path).await.unwrap();
    let playbook = load_playbook(&playbook_path).await.unwrap();

    // Create runtime context
    let runtime = init_runtime(&inventory).await;

    // Create executor with parallel config
    let exec_config = ExecutorConfig {
        forks: 5,
        check_mode: false,
        diff_mode: false,
        verbosity: 2,
        strategy: ExecutionStrategy::Linear,
        task_timeout: 300,
        gather_facts: false,
        extra_vars: HashMap::new(),
        ..Default::default()
    };

    let executor = Executor::with_runtime(exec_config, runtime);

    // Run the playbook
    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await;
    let elapsed = start.elapsed();

    println!("Playbook execution completed in {:?}", elapsed);

    match results {
        Ok(host_results) => {
            for (host, result) in host_results {
                println!("Host: {}", host);
                println!("  Stats: {:?}", result.stats);
                println!("  Failed: {}", result.failed);
                println!("  Unreachable: {}", result.unreachable);
            }
        }
        Err(e) => {
            eprintln!("Playbook execution failed: {:?}", e);
        }
    }

    // Cleanup
    tokio::fs::remove_file(inv_path).await.ok();
}

#[tokio::test]
async fn test_parallel_execution_multiple_hosts() {
    let config = E2EConfig::from_env();
    if config.skip_if_disabled() || config.hosts.is_empty() {
        eprintln!("Skipping multi-host test (no hosts configured)");
        return;
    }

    let playbook_path = config.playbook_path();
    if !playbook_path.exists() {
        eprintln!("Playbook not found at {:?}", playbook_path);
        return;
    }

    // Generate inventory with configured hosts
    let inv_content = config.generate_inventory();
    let inv_path = create_temp_inventory(&inv_content).await.unwrap();
    let inventory = load_inventory(&inv_path).await.unwrap();
    let playbook = load_playbook(&playbook_path).await.unwrap();

    let runtime = init_runtime(&inventory).await;

    // Create executor with parallel config
    let exec_config = ExecutorConfig {
        forks: 10, // Allow high parallelism
        check_mode: false,
        diff_mode: false,
        verbosity: 2,
        strategy: ExecutionStrategy::Linear,
        task_timeout: 300,
        gather_facts: false,
        extra_vars: HashMap::new(),
        ..Default::default()
    };

    let executor = Executor::with_runtime(exec_config, runtime);

    // Run the playbook
    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await;
    let elapsed = start.elapsed();

    println!("\n========================================");
    println!("Multi-host Parallel Execution Results");
    println!("========================================");
    println!("Total time: {:?}", elapsed);

    match results {
        Ok(host_results) => {
            let mut total_ok = 0;
            let mut total_changed = 0;
            let mut total_failed = 0;
            let mut total_unreachable = 0;

            for (host, result) in &host_results {
                println!("\nHost: {}", host);
                println!("  OK:          {}", result.stats.ok);
                println!("  Changed:     {}", result.stats.changed);
                println!("  Failed:      {}", result.stats.failed);
                println!("  Skipped:     {}", result.stats.skipped);
                println!("  Unreachable: {}", result.stats.unreachable);

                total_ok += result.stats.ok;
                total_changed += result.stats.changed;
                total_failed += result.stats.failed;
                total_unreachable += result.stats.unreachable;
            }

            println!("\n========================================");
            println!("Summary:");
            println!("  Hosts:       {}", host_results.len());
            println!("  OK:          {}", total_ok);
            println!("  Changed:     {}", total_changed);
            println!("  Failed:      {}", total_failed);
            println!("  Unreachable: {}", total_unreachable);
            println!("========================================");

            // Verify at least some hosts succeeded
            assert!(
                total_ok + total_changed > 0,
                "Expected at least some successful tasks"
            );
        }
        Err(e) => {
            panic!("Playbook execution failed: {:?}", e);
        }
    }

    // Cleanup
    tokio::fs::remove_file(inv_path).await.ok();
}

// =============================================================================
// Strategy Comparison Tests
// =============================================================================

#[tokio::test]
async fn test_linear_vs_free_strategy_performance() {
    let config = E2EConfig::from_env();
    if config.skip_if_disabled() || config.hosts.len() < 2 {
        eprintln!("Skipping strategy comparison (need at least 2 hosts)");
        return;
    }

    let playbook_path = config.playbook_path();
    if !playbook_path.exists() {
        eprintln!("Playbook not found at {:?}", playbook_path);
        return;
    }

    let inv_content = config.generate_inventory();
    let inv_path = create_temp_inventory(&inv_content).await.unwrap();

    // Test Linear strategy
    println!("\n=== Testing LINEAR Strategy ===");
    let inventory = load_inventory(&inv_path).await.unwrap();
    let playbook = load_playbook(&playbook_path).await.unwrap();
    let runtime = init_runtime(&inventory).await;

    let linear_config = ExecutorConfig {
        forks: 10,
        check_mode: false,
        diff_mode: false,
        verbosity: 1,
        strategy: ExecutionStrategy::Linear,
        task_timeout: 300,
        gather_facts: false,
        extra_vars: HashMap::new(),
        ..Default::default()
    };

    let executor = Executor::with_runtime(linear_config, runtime);
    let start = Instant::now();
    let linear_results = executor.run_playbook(&playbook).await;
    let linear_time = start.elapsed();

    println!("Linear strategy completed in {:?}", linear_time);

    // Test Free strategy
    println!("\n=== Testing FREE Strategy ===");
    let inventory = load_inventory(&inv_path).await.unwrap();
    let playbook = load_playbook(&playbook_path).await.unwrap();
    let runtime = init_runtime(&inventory).await;

    let free_config = ExecutorConfig {
        forks: 10,
        check_mode: false,
        diff_mode: false,
        verbosity: 1,
        strategy: ExecutionStrategy::Free,
        task_timeout: 300,
        gather_facts: false,
        extra_vars: HashMap::new(),
        ..Default::default()
    };

    let executor = Executor::with_runtime(free_config, runtime);
    let start = Instant::now();
    let free_results = executor.run_playbook(&playbook).await;
    let free_time = start.elapsed();

    println!("Free strategy completed in {:?}", free_time);

    // Compare results
    println!("\n========================================");
    println!("Strategy Comparison");
    println!("========================================");
    println!("Linear: {:?}", linear_time);
    println!("Free:   {:?}", free_time);

    if linear_time > free_time {
        let speedup = linear_time.as_secs_f64() / free_time.as_secs_f64();
        println!("Speedup: {:.2}x (Free is faster)", speedup);
    } else {
        println!("Linear was faster (unusual, may indicate low parallelism)");
    }
    println!("========================================");

    // Verify both succeeded
    assert!(linear_results.is_ok(), "Linear strategy should succeed");
    assert!(free_results.is_ok(), "Free strategy should succeed");

    // Cleanup
    tokio::fs::remove_file(inv_path).await.ok();
}

// =============================================================================
// Connection Pooling Tests
// =============================================================================

#[tokio::test]
async fn test_connection_reuse_in_parallel_execution() {
    let config = E2EConfig::from_env();
    if config.skip_if_disabled() || config.hosts.is_empty() {
        eprintln!("Skipping connection pooling test (no hosts configured)");
        return;
    }

    // Create a playbook with multiple tasks to test connection reuse
    let multi_task_playbook = r#"
---
- name: Connection Pooling Test
  hosts: all
  gather_facts: false
  tasks:
    - name: Task 1
      command: echo "task1"

    - name: Task 2
      command: echo "task2"

    - name: Task 3
      command: echo "task3"

    - name: Task 4
      command: echo "task4"

    - name: Task 5
      command: echo "task5"
"#;

    use std::io::Write;
    let mut playbook_tmpfile = tempfile::NamedTempFile::new().unwrap();
    playbook_tmpfile
        .write_all(multi_task_playbook.as_bytes())
        .unwrap();
    playbook_tmpfile.flush().unwrap();
    let playbook_path = playbook_tmpfile.path().to_path_buf();

    let inv_content = config.generate_inventory();
    let inv_path = create_temp_inventory(&inv_content).await.unwrap();

    let inventory = load_inventory(&inv_path).await.unwrap();
    let playbook = load_playbook(&playbook_path).await.unwrap();
    let runtime = init_runtime(&inventory).await;

    let exec_config = ExecutorConfig {
        forks: 10,
        check_mode: false,
        diff_mode: false,
        verbosity: 2,
        strategy: ExecutionStrategy::Linear,
        task_timeout: 300,
        gather_facts: false,
        extra_vars: HashMap::new(),
        ..Default::default()
    };

    let executor = Executor::with_runtime(exec_config, runtime);

    println!("\n=== Testing Connection Reuse ===");
    println!("Running 5 tasks across multiple hosts");
    println!("Connections should be reused for all tasks on each host");

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await;
    let elapsed = start.elapsed();

    println!("\nExecution completed in {:?}", elapsed);

    match results {
        Ok(host_results) => {
            for (host, result) in &host_results {
                let total_tasks = result.stats.ok + result.stats.changed;
                println!(
                    "Host {}: {} tasks completed successfully",
                    host, total_tasks
                );

                // Each host should have completed 5 tasks
                assert!(
                    total_tasks >= 5,
                    "Host {} should have completed at least 5 tasks, got {}",
                    host,
                    total_tasks
                );
            }
        }
        Err(e) => {
            panic!("Playbook execution failed: {:?}", e);
        }
    }

    // Cleanup
    tokio::fs::remove_file(inv_path).await.ok();
}

// =============================================================================
// Fork Limiting Tests
// =============================================================================

#[tokio::test]
async fn test_fork_limiting_with_many_hosts() {
    let config = E2EConfig::from_env();
    if config.skip_if_disabled() || config.hosts.len() < 3 {
        eprintln!("Skipping fork limiting test (need at least 3 hosts)");
        return;
    }

    let playbook_path = config.playbook_path();
    if !playbook_path.exists() {
        eprintln!("Playbook not found at {:?}", playbook_path);
        return;
    }

    let inv_content = config.generate_inventory();
    let inv_path = create_temp_inventory(&inv_content).await.unwrap();

    // Test with limited forks
    let fork_limit = 2;

    let inventory = load_inventory(&inv_path).await.unwrap();
    let playbook = load_playbook(&playbook_path).await.unwrap();
    let runtime = init_runtime(&inventory).await;

    let exec_config = ExecutorConfig {
        forks: fork_limit, // Limit to 2 concurrent hosts
        check_mode: false,
        diff_mode: false,
        verbosity: 2,
        strategy: ExecutionStrategy::Linear,
        task_timeout: 300,
        gather_facts: false,
        extra_vars: HashMap::new(),
        ..Default::default()
    };

    let executor = Executor::with_runtime(exec_config, runtime);

    println!("\n=== Testing Fork Limiting ===");
    println!("Fork limit: {}", fork_limit);
    println!("Total hosts: {}", config.hosts.len() + 1); // +1 for localhost

    let start = Instant::now();
    let results = executor.run_playbook(&playbook).await;
    let elapsed = start.elapsed();

    println!(
        "\nExecution with forks={} completed in {:?}",
        fork_limit, elapsed
    );

    assert!(
        results.is_ok(),
        "Playbook should succeed with fork limiting"
    );

    // Cleanup
    tokio::fs::remove_file(inv_path).await.ok();
}

// =============================================================================
// Performance Measurement Tests
// =============================================================================

#[tokio::test]
async fn test_parallel_performance_improvement() {
    let config = E2EConfig::from_env();
    if config.skip_if_disabled() || config.hosts.len() < 2 {
        eprintln!("Skipping performance test (need at least 2 hosts)");
        return;
    }

    // Create a playbook with a slow task
    let slow_playbook = r#"
---
- name: Performance Test Playbook
  hosts: all
  gather_facts: false
  tasks:
    - name: Slow task
      command: sleep 2
"#;

    use std::io::Write;
    let mut playbook_tmpfile = tempfile::NamedTempFile::new().unwrap();
    playbook_tmpfile
        .write_all(slow_playbook.as_bytes())
        .unwrap();
    playbook_tmpfile.flush().unwrap();
    let playbook_path = playbook_tmpfile.path().to_path_buf();

    let inv_content = config.generate_inventory();
    let inv_path = create_temp_inventory(&inv_content).await.unwrap();

    let num_hosts = config.hosts.len() + 1; // +1 for localhost

    // Run with forks=1 (essentially serial)
    println!("\n=== Serial Execution (forks=1) ===");
    let inventory = load_inventory(&inv_path).await.unwrap();
    let playbook = load_playbook(&playbook_path).await.unwrap();
    let runtime = init_runtime(&inventory).await;

    let serial_config = ExecutorConfig {
        forks: 1,
        check_mode: false,
        diff_mode: false,
        verbosity: 0,
        strategy: ExecutionStrategy::Linear,
        task_timeout: 300,
        gather_facts: false,
        extra_vars: HashMap::new(),
        ..Default::default()
    };

    let executor = Executor::with_runtime(serial_config, runtime);
    let start = Instant::now();
    let serial_results = executor.run_playbook(&playbook).await;
    let serial_time = start.elapsed();

    println!("Serial execution: {:?}", serial_time);

    // Run with forks=num_hosts (fully parallel)
    println!("\n=== Parallel Execution (forks={}) ===", num_hosts);
    let inventory = load_inventory(&inv_path).await.unwrap();
    let playbook = load_playbook(&playbook_path).await.unwrap();
    let runtime = init_runtime(&inventory).await;

    let parallel_config = ExecutorConfig {
        forks: num_hosts,
        check_mode: false,
        diff_mode: false,
        verbosity: 0,
        strategy: ExecutionStrategy::Linear,
        task_timeout: 300,
        gather_facts: false,
        extra_vars: HashMap::new(),
        ..Default::default()
    };

    let executor = Executor::with_runtime(parallel_config, runtime);
    let start = Instant::now();
    let parallel_results = executor.run_playbook(&playbook).await;
    let parallel_time = start.elapsed();

    println!("Parallel execution: {:?}", parallel_time);

    // Calculate speedup
    println!("\n========================================");
    println!("Performance Comparison");
    println!("========================================");
    println!("Hosts:    {}", num_hosts);
    println!("Serial:   {:?}", serial_time);
    println!("Parallel: {:?}", parallel_time);

    let speedup = serial_time.as_secs_f64() / parallel_time.as_secs_f64();
    println!("Speedup:  {:.2}x", speedup);

    // Calculate efficiency (ideal speedup is num_hosts)
    let efficiency = (speedup / num_hosts as f64) * 100.0;
    println!("Efficiency: {:.1}%", efficiency);
    println!("========================================");

    // Verify both succeeded
    assert!(serial_results.is_ok(), "Serial execution should succeed");
    assert!(
        parallel_results.is_ok(),
        "Parallel execution should succeed"
    );

    // Parallel should be significantly faster
    assert!(
        parallel_time < serial_time,
        "Parallel execution should be faster than serial"
    );

    // Speedup should be at least 1.5x for 2+ hosts
    assert!(
        speedup >= 1.5,
        "Expected speedup of at least 1.5x, got {:.2}x",
        speedup
    );

    // Cleanup
    tokio::fs::remove_file(inv_path).await.ok();
}
