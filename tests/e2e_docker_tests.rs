//! End-to-End Docker-Based Integration Tests
//!
//! This test suite runs complete playbook scenarios against Docker containers
//! to validate Rustible's functionality in realistic deployment scenarios.
//!
//! # Scenarios Covered
//!
//! 1. **Web Server Setup**: nginx configuration, virtual hosts, SSL
//! 2. **Database Configuration**: PostgreSQL setup, users, backups
//! 3. **User Management**: System users, SSH keys, sudo configuration
//! 4. **Application Deployment**: Capistrano-style deployments, symlinks
//! 5. **Multi-Tier Application**: Complete stack with LB, app, and DB tiers
//!
//! # Running the Tests
//!
//! ```bash
//! # Build and start Docker containers
//! cd tests/e2e/docker
//! docker compose up -d --build
//!
//! # Run E2E tests
//! RUSTIBLE_E2E_DOCKER_ENABLED=1 cargo test --test e2e_docker_tests
//!
//! # Clean up
//! docker compose down -v
//! ```
//!
//! # Environment Variables
//!
//! - `RUSTIBLE_E2E_DOCKER_ENABLED`: Set to "1" to enable Docker E2E tests
//! - `RUSTIBLE_E2E_DOCKER_COMPOSE_FILE`: Path to docker-compose.yml (optional)
//! - `RUSTIBLE_E2E_SSH_USER`: SSH user for containers (default: testuser)
//! - `RUSTIBLE_E2E_SSH_PASS`: SSH password for containers (default: testpassword)
//! - `RUSTIBLE_E2E_VERBOSE`: Set to "1" for verbose output

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod common;

use rustible::executor::playbook::Playbook;
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::{Executor, ExecutorConfig};

// ============================================================================
// Test Configuration
// ============================================================================

/// Configuration for Docker E2E tests
#[derive(Debug, Clone)]
struct DockerE2EConfig {
    /// Whether Docker E2E tests are enabled
    enabled: bool,
    /// Path to docker-compose.yml
    #[allow(dead_code)]
    compose_file: PathBuf,
    /// SSH user for containers
    ssh_user: String,
    /// SSH password for containers
    ssh_password: String,
    /// Verbose output
    verbose: bool,
    /// Container host mappings (hostname -> (host, port))
    containers: HashMap<String, (String, u16)>,
}

impl DockerE2EConfig {
    fn from_env() -> Self {
        let enabled = env::var("RUSTIBLE_E2E_DOCKER_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let compose_file = env::var("RUSTIBLE_E2E_DOCKER_COMPOSE_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/e2e/docker/docker-compose.yml")
            });

        let ssh_user = env::var("RUSTIBLE_E2E_SSH_USER").unwrap_or_else(|_| "testuser".to_string());

        let ssh_password =
            env::var("RUSTIBLE_E2E_SSH_PASS").unwrap_or_else(|_| "testpassword".to_string());

        let verbose = env::var("RUSTIBLE_E2E_VERBOSE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        // Default container mappings matching docker-compose.yml
        let mut containers = HashMap::new();
        containers.insert("web1".to_string(), ("localhost".to_string(), 2221));
        containers.insert("web2".to_string(), ("localhost".to_string(), 2222));
        containers.insert("db1".to_string(), ("localhost".to_string(), 2223));
        containers.insert("app1".to_string(), ("localhost".to_string(), 2224));
        containers.insert("app2".to_string(), ("localhost".to_string(), 2225));

        Self {
            enabled,
            compose_file,
            ssh_user,
            ssh_password,
            verbose,
            containers,
        }
    }

    fn skip_message(&self) -> Option<&'static str> {
        if !self.enabled {
            Some("Docker E2E tests disabled (set RUSTIBLE_E2E_DOCKER_ENABLED=1)")
        } else {
            None
        }
    }
}

// ============================================================================
// Docker Container Management
// ============================================================================

/// Check if Docker is available
fn docker_available() -> bool {
    Command::new("docker")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if containers are running
fn containers_running(config: &DockerE2EConfig) -> bool {
    for (name, _) in &config.containers {
        let container_name = format!("rustible-e2e-{}", name);
        let output = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", &container_name])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let running = String::from_utf8_lossy(&o.stdout).trim() == "true";
                if !running {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Wait for containers to be ready (SSH available)
async fn wait_for_containers(config: &DockerE2EConfig, timeout: Duration) -> bool {
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        let mut all_ready = true;

        for (name, (host, port)) in &config.containers {
            // Try to connect via netcat or simple socket check
            let check = Command::new("nc")
                .args(["-z", "-w", "1", host, &port.to_string()])
                .output();

            match check {
                Ok(o) if o.status.success() => {
                    if config.verbose {
                        println!("  Container {} ready on {}:{}", name, host, port);
                    }
                }
                _ => {
                    all_ready = false;
                    break;
                }
            }
        }

        if all_ready {
            return true;
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    false
}

// ============================================================================
// Test Helpers
// ============================================================================

/// Get the path to E2E test fixtures
fn e2e_fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/e2e")
}

/// Load a playbook from the E2E playbooks directory
fn load_e2e_playbook(name: &str) -> Result<Playbook, String> {
    let path = e2e_fixtures_path().join("playbooks").join(name);
    if !path.exists() {
        return Err(format!("Playbook not found: {:?}", path));
    }

    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read playbook: {}", e))?;

    Playbook::parse(&content, Some(path)).map_err(|e| format!("Failed to parse playbook: {}", e))
}

/// Create a runtime context from the E2E inventory
fn create_e2e_runtime(config: &DockerE2EConfig) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();

    // Add webservers
    for name in ["web1", "web2"] {
        if let Some((host, port)) = config.containers.get(name) {
            runtime.add_host(name.to_string(), Some("webservers"));
            runtime.set_host_var(name, "ansible_host".to_string(), serde_json::json!(host));
            runtime.set_host_var(name, "ansible_port".to_string(), serde_json::json!(port));
            runtime.set_host_var(
                name,
                "ansible_user".to_string(),
                serde_json::json!(&config.ssh_user),
            );
            runtime.set_host_var(
                name,
                "ansible_ssh_pass".to_string(),
                serde_json::json!(&config.ssh_password),
            );
            runtime.set_host_var(
                name,
                "ansible_connection".to_string(),
                serde_json::json!("ssh"),
            );
        }
    }

    // Add database
    if let Some((host, port)) = config.containers.get("db1") {
        runtime.add_host("db1".to_string(), Some("databases"));
        runtime.set_host_var("db1", "ansible_host".to_string(), serde_json::json!(host));
        runtime.set_host_var("db1", "ansible_port".to_string(), serde_json::json!(port));
        runtime.set_host_var(
            "db1",
            "ansible_user".to_string(),
            serde_json::json!(&config.ssh_user),
        );
        runtime.set_host_var(
            "db1",
            "ansible_ssh_pass".to_string(),
            serde_json::json!(&config.ssh_password),
        );
        runtime.set_host_var(
            "db1",
            "ansible_connection".to_string(),
            serde_json::json!("ssh"),
        );
    }

    // Add app servers
    for name in ["app1", "app2"] {
        if let Some((host, port)) = config.containers.get(name) {
            runtime.add_host(name.to_string(), Some("appservers"));
            runtime.set_host_var(name, "ansible_host".to_string(), serde_json::json!(host));
            runtime.set_host_var(name, "ansible_port".to_string(), serde_json::json!(port));
            runtime.set_host_var(
                name,
                "ansible_user".to_string(),
                serde_json::json!(&config.ssh_user),
            );
            runtime.set_host_var(
                name,
                "ansible_ssh_pass".to_string(),
                serde_json::json!(&config.ssh_password),
            );
            runtime.set_host_var(
                name,
                "ansible_connection".to_string(),
                serde_json::json!("ssh"),
            );
        }
    }

    runtime
}

/// Create an executor for E2E tests
fn create_e2e_executor(config: &DockerE2EConfig) -> Executor {
    let runtime = create_e2e_runtime(config);

    let executor_config = ExecutorConfig {
        forks: 5,
        check_mode: false,
        gather_facts: true,
        verbosity: if config.verbose { 2 } else { 0 },
        task_timeout: 60,
        ..Default::default()
    };

    Executor::with_runtime(executor_config, runtime)
}

/// Assert that playbook execution succeeded
fn assert_playbook_success(
    results: &HashMap<String, rustible::executor::HostResult>,
    expected_hosts: &[&str],
) {
    assert!(
        !results.is_empty(),
        "No hosts ran - check container connectivity"
    );

    for host in expected_hosts {
        let result = results.get(*host);
        assert!(result.is_some(), "Host {} did not run", host);

        let result = result.unwrap();
        assert!(
            !result.failed,
            "Host {} failed with stats: {:?}",
            host, result.stats
        );
        assert!(
            !result.unreachable,
            "Host {} was unreachable - check SSH connectivity",
            host
        );
    }
}

// ============================================================================
// E2E Test Suite
// ============================================================================

/// Prerequisite check - run before all tests
fn check_prerequisites(config: &DockerE2EConfig) -> Result<(), String> {
    if !docker_available() {
        return Err("Docker is not available".to_string());
    }

    if !containers_running(config) {
        return Err(
            "Docker containers are not running. Start them with: cd tests/e2e/docker && docker compose up -d".to_string()
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_e2e_docker_webserver_setup() {
    let config = DockerE2EConfig::from_env();

    if let Some(msg) = config.skip_message() {
        eprintln!("Skipping: {}", msg);
        return;
    }

    if let Err(e) = check_prerequisites(&config) {
        eprintln!("Prerequisites not met: {}", e);
        return;
    }

    println!("\n========================================");
    println!("E2E Test: Web Server Setup");
    println!("========================================\n");

    // Wait for containers
    if !wait_for_containers(&config, Duration::from_secs(30)).await {
        eprintln!("Containers not ready within timeout");
        return;
    }

    let executor = create_e2e_executor(&config);
    let playbook =
        load_e2e_playbook("01_webserver_setup.yml").expect("Failed to load webserver playbook");

    println!(
        "Executing {} tasks across webservers...",
        playbook.plays.iter().map(|p| p.tasks.len()).sum::<usize>()
    );

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Playbook execution failed");

    assert_playbook_success(&results, &["web1", "web2"]);

    // Verify specific outcomes
    for (host, result) in &results {
        println!(
            "Host {}: {} ok, {} changed, {} failed",
            host, result.stats.ok, result.stats.changed, result.stats.failed
        );
    }

    println!("\nWeb server setup test PASSED");
}

#[tokio::test]
async fn test_e2e_docker_database_setup() {
    let config = DockerE2EConfig::from_env();

    if let Some(msg) = config.skip_message() {
        eprintln!("Skipping: {}", msg);
        return;
    }

    if let Err(e) = check_prerequisites(&config) {
        eprintln!("Prerequisites not met: {}", e);
        return;
    }

    println!("\n========================================");
    println!("E2E Test: Database Configuration");
    println!("========================================\n");

    if !wait_for_containers(&config, Duration::from_secs(30)).await {
        eprintln!("Containers not ready within timeout");
        return;
    }

    let executor = create_e2e_executor(&config);
    let playbook =
        load_e2e_playbook("02_database_setup.yml").expect("Failed to load database playbook");

    println!(
        "Executing {} tasks on database server...",
        playbook.plays.iter().map(|p| p.tasks.len()).sum::<usize>()
    );

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Playbook execution failed");

    assert_playbook_success(&results, &["db1"]);

    println!("\nDatabase setup test PASSED");
}

#[tokio::test]
async fn test_e2e_docker_user_management() {
    let config = DockerE2EConfig::from_env();

    if let Some(msg) = config.skip_message() {
        eprintln!("Skipping: {}", msg);
        return;
    }

    if let Err(e) = check_prerequisites(&config) {
        eprintln!("Prerequisites not met: {}", e);
        return;
    }

    println!("\n========================================");
    println!("E2E Test: User Management");
    println!("========================================\n");

    if !wait_for_containers(&config, Duration::from_secs(30)).await {
        eprintln!("Containers not ready within timeout");
        return;
    }

    let executor = create_e2e_executor(&config);
    let playbook = load_e2e_playbook("03_user_management.yml")
        .expect("Failed to load user management playbook");

    println!(
        "Executing {} tasks across all hosts...",
        playbook.plays.iter().map(|p| p.tasks.len()).sum::<usize>()
    );

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Playbook execution failed");

    // This playbook runs on all hosts
    assert_playbook_success(&results, &["web1", "web2", "db1", "app1", "app2"]);

    println!("\nUser management test PASSED");
}

#[tokio::test]
async fn test_e2e_docker_app_deployment() {
    let config = DockerE2EConfig::from_env();

    if let Some(msg) = config.skip_message() {
        eprintln!("Skipping: {}", msg);
        return;
    }

    if let Err(e) = check_prerequisites(&config) {
        eprintln!("Prerequisites not met: {}", e);
        return;
    }

    println!("\n========================================");
    println!("E2E Test: Application Deployment");
    println!("========================================\n");

    if !wait_for_containers(&config, Duration::from_secs(30)).await {
        eprintln!("Containers not ready within timeout");
        return;
    }

    let executor = create_e2e_executor(&config);
    let playbook =
        load_e2e_playbook("04_app_deployment.yml").expect("Failed to load app deployment playbook");

    println!(
        "Executing {} tasks on app servers...",
        playbook.plays.iter().map(|p| p.tasks.len()).sum::<usize>()
    );

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Playbook execution failed");

    assert_playbook_success(&results, &["app1", "app2"]);

    println!("\nApplication deployment test PASSED");
}

#[tokio::test]
async fn test_e2e_docker_multi_tier_app() {
    let config = DockerE2EConfig::from_env();

    if let Some(msg) = config.skip_message() {
        eprintln!("Skipping: {}", msg);
        return;
    }

    if let Err(e) = check_prerequisites(&config) {
        eprintln!("Prerequisites not met: {}", e);
        return;
    }

    println!("\n========================================");
    println!("E2E Test: Multi-Tier Application");
    println!("========================================\n");

    if !wait_for_containers(&config, Duration::from_secs(30)).await {
        eprintln!("Containers not ready within timeout");
        return;
    }

    let executor = create_e2e_executor(&config);
    let playbook =
        load_e2e_playbook("05_multi_tier_app.yml").expect("Failed to load multi-tier playbook");

    let total_tasks: usize = playbook.plays.iter().map(|p| p.tasks.len()).sum();
    println!(
        "Executing {} tasks across {} plays on full stack...",
        total_tasks,
        playbook.plays.len()
    );

    let start = std::time::Instant::now();
    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Playbook execution failed");
    let duration = start.elapsed();

    // All hosts should be covered
    assert_playbook_success(&results, &["web1", "web2", "db1", "app1", "app2"]);

    println!(
        "\nMulti-tier deployment completed in {:.2}s",
        duration.as_secs_f64()
    );
    println!("Multi-tier application test PASSED");
}

#[tokio::test]
async fn test_e2e_docker_idempotency() {
    let config = DockerE2EConfig::from_env();

    if let Some(msg) = config.skip_message() {
        eprintln!("Skipping: {}", msg);
        return;
    }

    if let Err(e) = check_prerequisites(&config) {
        eprintln!("Prerequisites not met: {}", e);
        return;
    }

    println!("\n========================================");
    println!("E2E Test: Idempotency Verification");
    println!("========================================\n");

    if !wait_for_containers(&config, Duration::from_secs(30)).await {
        eprintln!("Containers not ready within timeout");
        return;
    }

    // Run webserver setup twice and verify idempotency
    let playbook =
        load_e2e_playbook("01_webserver_setup.yml").expect("Failed to load webserver playbook");

    // First run
    println!("First run (should make changes)...");
    let executor1 = create_e2e_executor(&config);
    let results1 = executor1
        .run_playbook(&playbook)
        .await
        .expect("First run failed");

    let first_changed: usize = results1.values().map(|r| r.stats.changed).sum();
    println!("First run: {} tasks changed", first_changed);

    // Second run (should be idempotent)
    println!("\nSecond run (should be mostly idempotent)...");
    let executor2 = create_e2e_executor(&config);
    let results2 = executor2
        .run_playbook(&playbook)
        .await
        .expect("Second run failed");

    let second_changed: usize = results2.values().map(|r| r.stats.changed).sum();
    println!("Second run: {} tasks changed", second_changed);

    // Second run should have fewer changes
    assert!(
        second_changed <= first_changed,
        "Second run should not have more changes than first (idempotency): first={}, second={}",
        first_changed,
        second_changed
    );

    println!(
        "\nIdempotency verified: {} -> {} changes",
        first_changed, second_changed
    );
    println!("Idempotency test PASSED");
}

#[tokio::test]
async fn test_e2e_docker_check_mode() {
    let config = DockerE2EConfig::from_env();

    if let Some(msg) = config.skip_message() {
        eprintln!("Skipping: {}", msg);
        return;
    }

    if let Err(e) = check_prerequisites(&config) {
        eprintln!("Prerequisites not met: {}", e);
        return;
    }

    println!("\n========================================");
    println!("E2E Test: Check Mode (Dry Run)");
    println!("========================================\n");

    if !wait_for_containers(&config, Duration::from_secs(30)).await {
        eprintln!("Containers not ready within timeout");
        return;
    }

    // Create executor in check mode
    let runtime = create_e2e_runtime(&config);
    let executor_config = ExecutorConfig {
        forks: 5,
        check_mode: true,
        diff_mode: true,
        gather_facts: true,
        verbosity: if config.verbose { 2 } else { 0 },
        ..Default::default()
    };
    let executor = Executor::with_runtime(executor_config, runtime);

    let playbook =
        load_e2e_playbook("01_webserver_setup.yml").expect("Failed to load webserver playbook");

    println!("Running in check mode (no actual changes)...");

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Check mode run failed");

    // All hosts should complete without errors
    for (host, result) in &results {
        assert!(
            !result.failed,
            "Host {} failed in check mode: {:?}",
            host, result.stats
        );
        println!(
            "Host {} (check mode): {} would change",
            host, result.stats.changed
        );
    }

    println!("\nCheck mode test PASSED");
}

#[tokio::test]
async fn test_e2e_docker_parallel_execution() {
    let config = DockerE2EConfig::from_env();

    if let Some(msg) = config.skip_message() {
        eprintln!("Skipping: {}", msg);
        return;
    }

    if let Err(e) = check_prerequisites(&config) {
        eprintln!("Prerequisites not met: {}", e);
        return;
    }

    println!("\n========================================");
    println!("E2E Test: Parallel Execution Performance");
    println!("========================================\n");

    if !wait_for_containers(&config, Duration::from_secs(30)).await {
        eprintln!("Containers not ready within timeout");
        return;
    }

    let playbook =
        load_e2e_playbook("05_multi_tier_app.yml").expect("Failed to load multi-tier playbook");

    // Test with different fork counts
    for forks in [1, 3, 5] {
        let runtime = create_e2e_runtime(&config);
        let executor_config = ExecutorConfig {
            forks,
            check_mode: false,
            gather_facts: true,
            ..Default::default()
        };
        let executor = Executor::with_runtime(executor_config, runtime);

        let start = std::time::Instant::now();
        let results = executor
            .run_playbook(&playbook)
            .await
            .expect("Playbook failed");
        let duration = start.elapsed();

        let total_tasks: usize = results.values().map(|r| r.stats.ok + r.stats.changed).sum();
        println!(
            "Forks={}: {} tasks in {:.2}s ({:.1} tasks/sec)",
            forks,
            total_tasks,
            duration.as_secs_f64(),
            total_tasks as f64 / duration.as_secs_f64()
        );
    }

    println!("\nParallel execution test PASSED");
}
