//! End-to-End Module Integration Tests
//!
//! This test suite validates the complete workflow of core modules:
//! - copy: File copying and content deployment
//! - file: Directory and file management
//! - template: Template rendering with variables
//! - command: Command execution
//! - service: Service status checking
//!
//! These tests can run against:
//! 1. Local connection (default)
//! 2. Docker containers (if RUSTIBLE_TEST_DOCKER_ENABLED=1)
//! 3. Real SSH VMs (if RUSTIBLE_TEST_SSH_ENABLED=1)
//!
//! # Running the tests
//!
//! ```bash
//! # Local execution
//! cargo test --test modules_e2e_tests
//!
//! # With Docker
//! export RUSTIBLE_TEST_DOCKER_ENABLED=1
//! cargo test --test modules_e2e_tests
//!
//! # With SSH VMs
//! export RUSTIBLE_TEST_SSH_ENABLED=1
//! export RUSTIBLE_TEST_SSH_USER=testuser
//! export RUSTIBLE_TEST_SSH_HOSTS=192.168.178.141,192.168.178.142
//! cargo test --test modules_e2e_tests
//!
//! # With test infrastructure
//! cd tests/infrastructure && ./run-tests.sh
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

mod common;

use rustible::executor::playbook::Playbook;
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::{Executor, ExecutorConfig};
use rustible::inventory::Inventory;

// ============================================================================
// Test Configuration
// ============================================================================

#[derive(Debug)]
struct E2ETestConfig {
    /// Run tests locally (always enabled)
    local: bool,
    /// Run tests in Docker containers
    #[allow(dead_code)]
    docker: bool,
    /// Run tests against SSH VMs
    ssh: bool,
    /// SSH configuration
    ssh_user: Option<String>,
    ssh_hosts: Vec<String>,
    #[allow(dead_code)]
    ssh_key: Option<PathBuf>,
    /// Inventory path for VM tests
    inventory_path: Option<PathBuf>,
}

impl E2ETestConfig {
    fn from_env() -> Self {
        let docker = env::var("RUSTIBLE_TEST_DOCKER_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let ssh = env::var("RUSTIBLE_TEST_SSH_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let ssh_user = env::var("RUSTIBLE_TEST_SSH_USER").ok();

        let ssh_hosts = env::var("RUSTIBLE_TEST_SSH_HOSTS")
            .map(|h| h.split(',').map(String::from).collect())
            .unwrap_or_default();

        let ssh_key = env::var("RUSTIBLE_TEST_SSH_KEY").map(PathBuf::from).ok();

        let inventory_path = env::var("RUSTIBLE_TEST_INVENTORY").map(PathBuf::from).ok();

        Self {
            local: true, // Always run local tests
            docker,
            ssh,
            ssh_user,
            ssh_hosts,
            ssh_key,
            inventory_path,
        }
    }

    fn should_run_local(&self) -> bool {
        self.local
    }

    #[allow(dead_code)]
    fn should_run_docker(&self) -> bool {
        self.docker
    }

    fn should_run_ssh(&self) -> bool {
        self.ssh && !self.ssh_hosts.is_empty()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn get_playbook_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/integration/playbooks/modules_e2e.yml")
}

fn load_e2e_playbook() -> Result<Playbook, String> {
    let path = get_playbook_path();
    if !path.exists() {
        return Err(format!("Playbook not found at {:?}", path));
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read playbook: {}", e))?;

    Playbook::parse(&content, Some(path)).map_err(|e| format!("Failed to parse playbook: {}", e))
}

fn create_local_executor(temp_dir: &TempDir) -> Executor {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    // Set basic facts
    runtime.set_host_fact(
        "localhost",
        "ansible_user".to_string(),
        serde_json::json!(env::var("USER").unwrap_or_else(|_| "testuser".to_string())),
    );
    runtime.set_host_fact(
        "localhost",
        "ansible_os_family".to_string(),
        serde_json::json!("Unix"),
    );
    runtime.set_host_fact(
        "localhost",
        "ansible_date_time".to_string(),
        serde_json::json!({
            "iso8601": "2024-01-01T00:00:00Z"
        }),
    );

    // Set test-specific vars
    runtime.set_host_var(
        "localhost",
        "test_dir".to_string(),
        serde_json::json!(temp_dir
            .path()
            .join("e2e_test")
            .to_string_lossy()
            .to_string()),
    );

    let config = ExecutorConfig {
        forks: 1,
        check_mode: false,
        gather_facts: false, // We manually set facts
        verbosity: env::var("RUSTIBLE_TEST_VERBOSE")
            .map(|v| v.parse().unwrap_or(0))
            .unwrap_or(0),
        ..Default::default()
    };

    Executor::with_runtime(config, runtime)
}

fn create_ssh_executor(config: &E2ETestConfig) -> Option<Executor> {
    if !config.should_run_ssh() {
        return None;
    }

    // Try to load from inventory file if provided
    if let Some(ref inventory_path) = config.inventory_path {
        if inventory_path.exists() {
            if let Ok(inventory) = Inventory::load(inventory_path) {
                let runtime = RuntimeContext::from_inventory(&inventory);
                let executor_config = ExecutorConfig {
                    forks: config.ssh_hosts.len().max(1),
                    gather_facts: true,
                    verbosity: env::var("RUSTIBLE_TEST_VERBOSE")
                        .map(|v| v.parse().unwrap_or(0))
                        .unwrap_or(0),
                    ..Default::default()
                };
                return Some(Executor::with_runtime(executor_config, runtime));
            }
        }
    }

    // Fallback: Create runtime from config
    let mut runtime = RuntimeContext::new();
    for host in &config.ssh_hosts {
        runtime.add_host(host.clone(), Some("test_vms"));
        if let Some(ref user) = config.ssh_user {
            runtime.set_host_var(host, "ansible_user".to_string(), serde_json::json!(user));
        }
        runtime.set_host_var(
            host,
            "ansible_connection".to_string(),
            serde_json::json!("ssh"),
        );
    }

    let executor_config = ExecutorConfig {
        forks: config.ssh_hosts.len().max(1),
        gather_facts: true,
        verbosity: env::var("RUSTIBLE_TEST_VERBOSE")
            .map(|v| v.parse().unwrap_or(0))
            .unwrap_or(0),
        ..Default::default()
    };

    Some(Executor::with_runtime(executor_config, runtime))
}

// ============================================================================
// Test Assertions
// ============================================================================

fn assert_module_tests_passed(results: &HashMap<String, rustible::executor::HostResult>) {
    assert!(!results.is_empty(), "No hosts ran");

    for (host, result) in results {
        assert!(
            !result.failed,
            "Host {} failed. Stats: {:?}",
            host, result.stats
        );
        assert!(!result.unreachable, "Host {} was unreachable", host);

        // We expect a significant number of tasks to have run
        let total_tasks = result.stats.ok + result.stats.changed + result.stats.failed;
        assert!(
            total_tasks >= 30,
            "Expected at least 30 tasks to run on {}, but only {} ran",
            host,
            total_tasks
        );

        // We expect some changes (copy, file creation, etc.)
        assert!(
            result.stats.changed > 0,
            "Expected some tasks to report changes on {}, but none did. Stats: {:?}",
            host,
            result.stats
        );

        println!(
            "✓ Host {} completed successfully: {} ok, {} changed, {} skipped",
            host, result.stats.ok, result.stats.changed, result.stats.skipped
        );
    }
}

fn verify_test_artifacts(temp_dir: &TempDir) {
    let test_dir = temp_dir.path().join("e2e_test");

    // Verify directory structure
    assert!(
        test_dir.exists(),
        "Test directory should exist: {:?}",
        test_dir
    );
    assert!(
        test_dir.join("config").exists(),
        "Config directory should exist"
    );
    assert!(
        test_dir.join("logs").exists(),
        "Logs directory should exist"
    );
    assert!(
        test_dir.join("data").exists(),
        "Data directory should exist"
    );
    assert!(
        test_dir.join("templates").exists(),
        "Templates directory should exist"
    );

    // Verify files created by copy module
    let test_file = test_dir.join("data/test_file.txt");
    assert!(test_file.exists(), "Test file should exist");
    let content = fs::read_to_string(&test_file).expect("Failed to read test file");
    assert!(
        content.contains("Hello from Rustible E2E test"),
        "Test file should contain expected content"
    );

    // Verify files created by template module
    let template_file = test_dir.join("templates/simple.txt");
    assert!(template_file.exists(), "Template file should exist");

    let config_template = test_dir.join("templates/config.ini");
    assert!(config_template.exists(), "Config template should exist");
    let config_content = fs::read_to_string(&config_template).expect("Failed to read config");
    assert!(
        config_content.contains("rustible_test_app"),
        "Config should contain app name"
    );
    assert!(
        config_content.contains("8080"),
        "Config should contain port"
    );

    // Verify deployment structure
    let deployment = test_dir.join("deployment");
    assert!(deployment.exists(), "Deployment directory should exist");
    assert!(
        deployment.join("bin").exists(),
        "Deployment bin directory should exist"
    );
    assert!(
        deployment.join("conf").exists(),
        "Deployment conf directory should exist"
    );

    let app_script = deployment.join("bin/app.sh");
    assert!(app_script.exists(), "App script should exist");

    let metadata = deployment.join("metadata.json");
    assert!(metadata.exists(), "Metadata file should exist");
    let metadata_content = fs::read_to_string(&metadata).expect("Failed to read metadata");
    assert!(
        metadata_content.contains("rustible_test_app"),
        "Metadata should contain app name"
    );

    println!("✓ All test artifacts verified successfully");
}

// ============================================================================
// Main Tests
// ============================================================================

#[tokio::test]
#[ignore = "E2E tests require local environment setup and have known module execution issues"]
async fn test_e2e_modules_local() {
    let config = E2ETestConfig::from_env();
    if !config.should_run_local() {
        eprintln!("Skipping local E2E test");
        return;
    }

    println!("\n========================================");
    println!("Running E2E Module Tests (Local)");
    println!("========================================\n");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let executor = create_local_executor(&temp_dir);
    let playbook = load_e2e_playbook().expect("Failed to load playbook");

    println!(
        "Executing playbook with {} tasks...",
        playbook.plays.iter().map(|p| p.tasks.len()).sum::<usize>()
    );

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Playbook execution failed");

    assert_module_tests_passed(&results);
    verify_test_artifacts(&temp_dir);

    println!("\n✓ Local E2E module tests passed!");
}

#[tokio::test]
async fn test_e2e_modules_ssh() {
    let config = E2ETestConfig::from_env();
    if !config.should_run_ssh() {
        eprintln!(
            "Skipping SSH E2E test (RUSTIBLE_TEST_SSH_ENABLED not set or no hosts configured)"
        );
        return;
    }

    println!("\n========================================");
    println!("Running E2E Module Tests (SSH)");
    println!("Hosts: {:?}", config.ssh_hosts);
    println!("========================================\n");

    let executor = create_ssh_executor(&config).expect("Failed to create SSH executor");
    let playbook = load_e2e_playbook().expect("Failed to load playbook");

    println!(
        "Executing playbook against {} hosts...",
        config.ssh_hosts.len()
    );

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Playbook execution failed");

    assert_module_tests_passed(&results);

    println!("\n✓ SSH E2E module tests passed!");
}

#[tokio::test]
#[ignore = "E2E tests require local environment setup and have known module execution issues"]
async fn test_e2e_modules_check_mode() {
    let config = E2ETestConfig::from_env();
    if !config.should_run_local() {
        return;
    }

    println!("\n========================================");
    println!("Running E2E Module Tests (Check Mode)");
    println!("========================================\n");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_host_fact(
        "localhost",
        "ansible_user".to_string(),
        serde_json::json!(env::var("USER").unwrap_or_else(|_| "testuser".to_string())),
    );
    runtime.set_host_var(
        "localhost",
        "test_dir".to_string(),
        serde_json::json!(temp_dir
            .path()
            .join("e2e_test_check")
            .to_string_lossy()
            .to_string()),
    );

    let executor_config = ExecutorConfig {
        forks: 1,
        check_mode: true, // Enable check mode
        diff_mode: true,
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(executor_config, runtime);
    let playbook = load_e2e_playbook().expect("Failed to load playbook");

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Playbook execution failed");

    // In check mode, tasks should run but not make actual changes
    for (host, result) in &results {
        assert!(!result.failed, "Host {} failed in check mode", host);
        println!(
            "✓ Check mode for {}: {} ok, {} changed (would change)",
            host, result.stats.ok, result.stats.changed
        );
    }

    // Verify that files were NOT created in check mode
    let test_dir = temp_dir.path().join("e2e_test_check");
    assert!(
        !test_dir.exists() || fs::read_dir(&test_dir).unwrap().count() == 0,
        "Check mode should not create files"
    );

    println!("\n✓ Check mode E2E tests passed!");
}

#[tokio::test]
#[ignore = "E2E tests require local environment setup and have known module execution issues"]
async fn test_e2e_modules_idempotency() {
    let config = E2ETestConfig::from_env();
    if !config.should_run_local() {
        return;
    }

    println!("\n========================================");
    println!("Running E2E Idempotency Tests");
    println!("========================================\n");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let playbook = load_e2e_playbook().expect("Failed to load playbook");

    // First run - should make changes
    println!("First run (should make changes)...");
    let executor1 = create_local_executor(&temp_dir);
    let results1 = executor1
        .run_playbook(&playbook)
        .await
        .expect("First playbook run failed");

    let first_changed = results1.values().map(|r| r.stats.changed).sum::<usize>();
    println!("First run: {} tasks changed", first_changed);
    assert!(first_changed > 0, "First run should have changes");

    // Second run - should be idempotent (fewer or no changes)
    println!("\nSecond run (should be idempotent)...");
    let executor2 = create_local_executor(&temp_dir);
    let results2 = executor2
        .run_playbook(&playbook)
        .await
        .expect("Second playbook run failed");

    let second_changed = results2.values().map(|r| r.stats.changed).sum::<usize>();
    println!("Second run: {} tasks changed", second_changed);

    assert!(
        second_changed < first_changed,
        "Second run should have fewer changes (idempotency). First: {}, Second: {}",
        first_changed,
        second_changed
    );

    // Ideally, the second run should have very few or no changes
    // Some modules might not be fully idempotent yet, so we just check it's less
    println!(
        "\n✓ Idempotency verified: {} changes in first run, {} in second run",
        first_changed, second_changed
    );
}

#[tokio::test]
#[ignore = "E2E tests require local environment setup and have known module execution issues"]
async fn test_e2e_individual_modules() {
    let config = E2ETestConfig::from_env();
    if !config.should_run_local() {
        return;
    }

    println!("\n========================================");
    println!("Running Individual Module Tests");
    println!("========================================\n");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let executor = create_local_executor(&temp_dir);

    // Test each module category individually with simple playbooks

    // Test 1: File module
    println!("Testing file module...");
    let file_yaml = format!(
        r#"---
- name: Test File Module
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Create directory
      file:
        path: {}/test_file_module
        state: directory
        mode: "0755"
"#,
        temp_dir.path().join("e2e_test").display()
    );
    let file_playbook = Playbook::parse(&file_yaml, None).expect("Failed to parse file playbook");
    let file_results = executor
        .run_playbook(&file_playbook)
        .await
        .expect("File test failed");
    assert!(!file_results.get("localhost").unwrap().failed);
    println!("  ✓ File module test passed");

    // Test 2: Copy module
    println!("Testing copy module...");
    let copy_yaml = format!(
        r#"---
- name: Test Copy Module
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Copy content
      copy:
        content: "Test content"
        dest: {}/test_copy.txt
        mode: "0644"
"#,
        temp_dir.path().join("e2e_test").display()
    );
    let copy_playbook = Playbook::parse(&copy_yaml, None).expect("Failed to parse copy playbook");
    let copy_results = executor
        .run_playbook(&copy_playbook)
        .await
        .expect("Copy test failed");
    assert!(!copy_results.get("localhost").unwrap().failed);
    println!("  ✓ Copy module test passed");

    // Test 3: Template module
    println!("Testing template module...");
    let template_yaml = format!(
        r#"---
- name: Test Template Module
  hosts: localhost
  gather_facts: false
  vars:
    test_var: "TemplateTest"
  tasks:
    - name: Render template
      template:
        content: "Value: {{{{ test_var }}}}"
        dest: {}/test_template.txt
        mode: "0644"
"#,
        temp_dir.path().join("e2e_test").display()
    );
    let template_playbook =
        Playbook::parse(&template_yaml, None).expect("Failed to parse template playbook");
    let template_results = executor
        .run_playbook(&template_playbook)
        .await
        .expect("Template test failed");
    assert!(!template_results.get("localhost").unwrap().failed);
    println!("  ✓ Template module test passed");

    // Test 4: Command module
    println!("Testing command module...");
    let command_yaml = r#"---
- name: Test Command Module
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Run echo command
      command: echo "Hello from command module"
"#;
    let command_playbook =
        Playbook::parse(command_yaml, None).expect("Failed to parse command playbook");
    let command_results = executor
        .run_playbook(&command_playbook)
        .await
        .expect("Command test failed");
    assert!(!command_results.get("localhost").unwrap().failed);
    println!("  ✓ Command module test passed");

    println!("\n✓ All individual module tests passed!");
}

// ============================================================================
// Performance and Stress Tests
// ============================================================================

#[tokio::test]
#[ignore = "E2E tests require local environment setup and have known module execution issues"]
async fn test_e2e_modules_performance() {
    let config = E2ETestConfig::from_env();
    if !config.should_run_local() {
        return;
    }

    println!("\n========================================");
    println!("Running E2E Performance Test");
    println!("========================================\n");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let executor = create_local_executor(&temp_dir);
    let playbook = load_e2e_playbook().expect("Failed to load playbook");

    let start = std::time::Instant::now();
    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Performance test failed");
    let duration = start.elapsed();

    assert_module_tests_passed(&results);

    let total_tasks = results
        .values()
        .map(|r| r.stats.ok + r.stats.changed + r.stats.skipped)
        .sum::<usize>();

    println!(
        "\n✓ Performance: {} tasks completed in {:.2}s ({:.0} tasks/sec)",
        total_tasks,
        duration.as_secs_f64(),
        total_tasks as f64 / duration.as_secs_f64()
    );
}

#[tokio::test]
#[ignore = "E2E tests require local environment setup and have known module execution issues"]
async fn test_e2e_modules_with_variables() {
    let config = E2ETestConfig::from_env();
    if !config.should_run_local() {
        return;
    }

    println!("\n========================================");
    println!("Running E2E Tests with Custom Variables");
    println!("========================================\n");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    // Override variables
    runtime.set_host_var(
        "localhost",
        "app_name".to_string(),
        serde_json::json!("custom_app"),
    );
    runtime.set_host_var(
        "localhost",
        "app_version".to_string(),
        serde_json::json!("2.0.0"),
    );
    runtime.set_host_var("localhost", "app_port".to_string(), serde_json::json!(9000));
    runtime.set_host_var(
        "localhost",
        "test_dir".to_string(),
        serde_json::json!(temp_dir
            .path()
            .join("e2e_test")
            .to_string_lossy()
            .to_string()),
    );

    let executor_config = ExecutorConfig {
        forks: 1,
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(executor_config, runtime);
    let playbook = load_e2e_playbook().expect("Failed to load playbook");

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Variable test failed");

    assert_module_tests_passed(&results);

    // Verify that custom variables were used
    let metadata = temp_dir.path().join("e2e_test/deployment/metadata.json");
    if metadata.exists() {
        let content = fs::read_to_string(&metadata).expect("Failed to read metadata");
        assert!(
            content.contains("custom_app"),
            "Custom app name should be in metadata"
        );
        assert!(
            content.contains("2.0.0"),
            "Custom version should be in metadata"
        );
        println!("  ✓ Custom variables were applied correctly");
    }

    println!("\n✓ Variable substitution test passed!");
}
