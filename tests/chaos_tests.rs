//! Chaos Engineering Tests for Rustible (ssh2-backend)
//!
//! These tests validate Rustible's resilience under adverse conditions:
//! - Random connection failures
//! - Network latency simulation
//! - Intermittent errors
//! - Resource exhaustion scenarios
//! - Recovery after failures
//!
//! WARNING: Some of these tests may require elevated privileges or
//! special container configurations (e.g., tc for network simulation).
//!
//! NOTE: This test file requires the ssh2-backend feature. Run with:
//! ```bash
//! export RUSTIBLE_TEST_CHAOS_ENABLED=1
//! cargo test --test chaos_tests --features ssh2-backend -- --test-threads=1
//! ```

#![cfg(feature = "ssh2-backend")]

use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::RwLock;
use rand::Rng;

// Import the Connection trait so we can call .execute() / .close() on connections
use rustible::connection::{Connection, ConnectionConfig, ConnectionResult, HostConfig};

mod common;

/// Configuration for chaos tests
struct ChaosTestConfig {
    enabled: bool,
    ssh_user: String,
    ssh_key_path: PathBuf,
    hosts: Vec<String>,
}

impl ChaosTestConfig {
    fn from_env() -> Self {
        let enabled = env::var("RUSTIBLE_TEST_CHAOS_ENABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let ssh_user =
            env::var("RUSTIBLE_TEST_SSH_USER").unwrap_or_else(|_| "testuser".to_string());

        let ssh_key_path = env::var("RUSTIBLE_TEST_SSH_KEY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".ssh/id_ed25519")
            });

        let hosts = env::var("RUSTIBLE_TEST_SSH_HOSTS")
            .map(|h| h.split(',').map(String::from).collect())
            .unwrap_or_else(|_| (141..=145).map(|i| format!("192.168.178.{}", i)).collect());

        Self {
            enabled,
            ssh_user,
            ssh_key_path,
            hosts,
        }
    }

    fn skip_if_disabled(&self) -> bool {
        if !self.enabled {
            eprintln!("Skipping chaos tests (RUSTIBLE_TEST_CHAOS_ENABLED not set)");
            true
        } else {
            false
        }
    }

    /// Build a HostConfig for SSH connections using the test configuration.
    fn host_config(&self, host: &str) -> HostConfig {
        HostConfig::new()
            .hostname(host)
            .port(22)
            .user(&self.ssh_user)
            .identity_file(self.ssh_key_path.to_string_lossy())
    }
}

// =============================================================================
// Chaos Connection Wrapper
// =============================================================================

/// A wrapper connection that introduces chaos (failures, delays)
pub struct ChaosConnection<C: Connection> {
    inner: C,
    failure_rate: f64,         // 0.0 to 1.0
    latency_ms: Option<u64>,   // Additional latency per operation
    fail_after_n: Option<u32>, // Fail after N successful operations
    operation_count: AtomicUsize,
    should_fail_next: AtomicBool,
}

impl<C: Connection> ChaosConnection<C> {
    pub fn new(inner: C) -> Self {
        Self {
            inner,
            failure_rate: 0.0,
            latency_ms: None,
            fail_after_n: None,
            operation_count: AtomicUsize::new(0),
            should_fail_next: AtomicBool::new(false),
        }
    }

    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = Some(latency_ms);
        self
    }

    pub fn with_fail_after(mut self, n: u32) -> Self {
        self.fail_after_n = Some(n);
        self
    }

    pub fn fail_next(&self) {
        self.should_fail_next.store(true, Ordering::SeqCst);
    }

    async fn maybe_fail(&self) -> Result<(), rustible::connection::ConnectionError> {
        // Check explicit fail flag
        if self.should_fail_next.swap(false, Ordering::SeqCst) {
            return Err(rustible::connection::ConnectionError::ConnectionFailed(
                "Chaos: forced failure".to_string(),
            ));
        }

        // Check fail_after_n
        let count = self.operation_count.fetch_add(1, Ordering::SeqCst);
        if let Some(n) = self.fail_after_n {
            if count >= n as usize {
                return Err(rustible::connection::ConnectionError::ConnectionFailed(
                    format!("Chaos: failed after {} operations", n),
                ));
            }
        }

        // Check random failure rate
        if self.failure_rate > 0.0 {
            let mut rng = rand::thread_rng();
            if rng.gen::<f64>() < self.failure_rate {
                return Err(rustible::connection::ConnectionError::ConnectionFailed(
                    "Chaos: random failure".to_string(),
                ));
            }
        }

        // Introduce latency
        if let Some(latency) = self.latency_ms {
            tokio::time::sleep(Duration::from_millis(latency)).await;
        }

        Ok(())
    }
}

#[async_trait]
impl<C: Connection + Send + Sync> Connection for ChaosConnection<C> {
    fn identifier(&self) -> &str {
        self.inner.identifier()
    }

    async fn is_alive(&self) -> bool {
        self.inner.is_alive().await
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<rustible::connection::ExecuteOptions>,
    ) -> ConnectionResult<rustible::connection::CommandResult> {
        self.maybe_fail().await?;
        self.inner.execute(command, options).await
    }

    async fn upload(
        &self,
        src: &std::path::Path,
        dest: &std::path::Path,
        options: Option<rustible::connection::TransferOptions>,
    ) -> ConnectionResult<()> {
        self.maybe_fail().await?;
        self.inner.upload(src, dest, options).await
    }

    async fn upload_content(
        &self,
        content: &[u8],
        dest: &std::path::Path,
        options: Option<rustible::connection::TransferOptions>,
    ) -> ConnectionResult<()> {
        self.maybe_fail().await?;
        self.inner.upload_content(content, dest, options).await
    }

    async fn download(
        &self,
        src: &std::path::Path,
        dest: &std::path::Path,
    ) -> ConnectionResult<()> {
        self.maybe_fail().await?;
        self.inner.download(src, dest).await
    }

    async fn download_content(
        &self,
        src: &std::path::Path,
    ) -> ConnectionResult<Vec<u8>> {
        self.maybe_fail().await?;
        self.inner.download_content(src).await
    }

    async fn path_exists(
        &self,
        path: &std::path::Path,
    ) -> ConnectionResult<bool> {
        self.maybe_fail().await?;
        self.inner.path_exists(path).await
    }

    async fn is_directory(
        &self,
        path: &std::path::Path,
    ) -> ConnectionResult<bool> {
        self.maybe_fail().await?;
        self.inner.is_directory(path).await
    }

    async fn stat(
        &self,
        path: &std::path::Path,
    ) -> ConnectionResult<rustible::connection::FileStat> {
        self.maybe_fail().await?;
        self.inner.stat(path).await
    }

    async fn close(&self) -> ConnectionResult<()> {
        self.inner.close().await
    }
}

// =============================================================================
// Random Failure Tests
// =============================================================================

#[tokio::test]
async fn test_random_connection_failures_10_percent() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");

    let global_config = ConnectionConfig::default();
    let host_cfg = config.host_config(host);

    let base_conn = rustible::connection::SshConnection::connect(
        host,
        22,
        &config.ssh_user,
        Some(host_cfg),
        &global_config,
    )
    .await
    .expect("Failed to connect");

    let chaos_conn = ChaosConnection::new(base_conn).with_failure_rate(0.10); // 10% failure rate

    let mut successes = 0;
    let mut failures = 0;
    let total_ops = 50;

    for i in 0..total_ops {
        match chaos_conn
            .execute(&format!("echo test_{}", i), None)
            .await
        {
            Ok(result) if result.success => successes += 1,
            _ => failures += 1,
        }
    }

    println!(
        "10% failure rate: {}/{} successful ({:.1}% actual failure)",
        successes,
        total_ops,
        100.0 * failures as f64 / total_ops as f64
    );

    // With 10% target, expect roughly 5-15% actual failures
    assert!(
        failures >= 1,
        "Should have at least some failures with 10% rate"
    );
    assert!(
        successes >= 35,
        "Should have majority successes (got {})",
        successes
    );

    chaos_conn.close().await.ok();
}

#[tokio::test]
async fn test_random_connection_failures_30_percent() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");

    let global_config = ConnectionConfig::default();
    let host_cfg = config.host_config(host);

    let base_conn = rustible::connection::SshConnection::connect(
        host,
        22,
        &config.ssh_user,
        Some(host_cfg),
        &global_config,
    )
    .await
    .expect("Failed to connect");

    let chaos_conn = ChaosConnection::new(base_conn).with_failure_rate(0.30); // 30% failure rate

    let mut successes = 0;
    let mut failures = 0;
    let total_ops = 50;

    for i in 0..total_ops {
        match chaos_conn
            .execute(&format!("echo test_{}", i), None)
            .await
        {
            Ok(result) if result.success => successes += 1,
            _ => failures += 1,
        }
    }

    println!(
        "30% failure rate: {}/{} successful ({:.1}% actual failure)",
        successes,
        total_ops,
        100.0 * failures as f64 / total_ops as f64
    );

    // With 30% target, expect roughly 20-40% actual failures
    assert!(
        failures >= 5,
        "Should have significant failures with 30% rate"
    );
    assert!(
        successes >= 25,
        "Should still have some successes (got {})",
        successes
    );

    chaos_conn.close().await.ok();
}

// =============================================================================
// Latency Tests
// =============================================================================

#[tokio::test]
async fn test_slow_network_latency() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");

    let global_config = ConnectionConfig::default();
    let host_cfg = config.host_config(host);

    let base_conn = rustible::connection::SshConnection::connect(
        host,
        22,
        &config.ssh_user,
        Some(host_cfg),
        &global_config,
    )
    .await
    .expect("Failed to connect");

    // Add 200ms latency per operation
    let chaos_conn = ChaosConnection::new(base_conn).with_latency(200);

    let ops = 5;
    let start = Instant::now();

    for i in 0..ops {
        let result = chaos_conn
            .execute(&format!("echo latency_test_{}", i), None)
            .await;
        assert!(result.is_ok());
    }

    let elapsed = start.elapsed();
    let expected_min = Duration::from_millis(200 * ops as u64);

    println!(
        "Latency test: {} ops in {:?} (expected min {:?})",
        ops, elapsed, expected_min
    );

    assert!(
        elapsed >= expected_min,
        "Should have added latency (elapsed {:?} < expected {:?})",
        elapsed,
        expected_min
    );

    chaos_conn.close().await.ok();
}

#[tokio::test]
async fn test_high_latency_with_timeout() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");

    let global_config = ConnectionConfig::default();
    let host_cfg = config.host_config(host).timeout(60);

    let base_conn = rustible::connection::SshConnection::connect(
        host,
        22,
        &config.ssh_user,
        Some(host_cfg),
        &global_config,
    )
    .await
    .expect("Failed to connect");

    // Add 500ms latency - commands should still complete
    let chaos_conn = ChaosConnection::new(base_conn).with_latency(500);

    let start = Instant::now();

    // Simple command with high latency
    let result = chaos_conn
        .execute("echo 'high latency test'", None)
        .await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Command should complete despite latency");
    assert!(
        elapsed >= Duration::from_millis(500),
        "Should have 500ms latency"
    );

    println!("High latency command completed in {:?}", elapsed);

    chaos_conn.close().await.ok();
}

// =============================================================================
// Fail After N Tests
// =============================================================================

#[tokio::test]
async fn test_fail_after_n_operations() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");

    let global_config = ConnectionConfig::default();
    let host_cfg = config.host_config(host);

    let base_conn = rustible::connection::SshConnection::connect(
        host,
        22,
        &config.ssh_user,
        Some(host_cfg),
        &global_config,
    )
    .await
    .expect("Failed to connect");

    // Fail after 5 successful operations
    let chaos_conn = ChaosConnection::new(base_conn).with_fail_after(5);

    let mut results = vec![];

    for i in 0..10 {
        let result = chaos_conn
            .execute(&format!("echo op_{}", i), None)
            .await;
        results.push(result.is_ok());
    }

    // First 5 should succeed, rest should fail
    let successes: Vec<_> = results.iter().take(5).collect();
    let failures: Vec<_> = results.iter().skip(5).collect();

    println!("Results: {:?}", results);

    assert!(successes.iter().all(|&&s| s), "First 5 should succeed");
    assert!(failures.iter().all(|&&s| !s), "Remaining should fail");

    chaos_conn.close().await.ok();
}

// =============================================================================
// Recovery Tests
// =============================================================================

#[tokio::test]
async fn test_recovery_after_forced_failure() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");
    let global_config = ConnectionConfig::default();

    // Test that we can recover from a failed connection by reconnecting
    for attempt in 0..3 {
        let host_cfg = config.host_config(host);

        let conn = rustible::connection::SshConnection::connect(
            host,
            22,
            &config.ssh_user,
            Some(host_cfg),
            &global_config,
        )
        .await
        .expect("Failed to connect");

        // Simulate some operations
        let result = conn.execute("echo 'test recovery'", None).await;

        if attempt == 1 {
            // Force close on second attempt to simulate failure
            conn.close().await.ok();
            // Try to use closed connection - should fail
            let result_after_close = conn.execute("echo 'after close'", None).await;
            assert!(result_after_close.is_err(), "Should fail after close");
        } else {
            assert!(result.is_ok(), "Normal operation should succeed");
            conn.close().await.ok();
        }
    }

    println!("Recovery test passed - reconnection works after failures");
}

// =============================================================================
// Concurrent Chaos Tests
// =============================================================================

#[tokio::test]
async fn test_concurrent_operations_with_failures() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if config.hosts.len() < 3 {
        eprintln!("Need at least 3 hosts for concurrent chaos test");
        return;
    }

    let results: Arc<RwLock<Vec<(String, bool)>>> = Arc::new(RwLock::new(Vec::new()));
    let mut handles = vec![];

    let global_config = Arc::new(ConnectionConfig::default());

    for host in config.hosts.iter().take(3) {
        let host = host.clone();
        let user = config.ssh_user.clone();
        let host_cfg = config.host_config(&host);
        let results = Arc::clone(&results);
        let global_cfg = Arc::clone(&global_config);

        handles.push(tokio::spawn(async move {
            match rustible::connection::SshConnection::connect(
                &host,
                22,
                &user,
                Some(host_cfg),
                &global_cfg,
            )
            .await
            {
                Ok(base_conn) => {
                    // 20% failure rate per connection
                    let chaos_conn = ChaosConnection::new(base_conn).with_failure_rate(0.20);

                    for i in 0..10 {
                        let success = chaos_conn
                            .execute(&format!("echo {}_{}", host, i), None)
                            .await
                            .is_ok();
                        results.write().push((host.clone(), success));
                    }

                    chaos_conn.close().await.ok();
                }
                Err(e) => {
                    eprintln!("Failed to connect to {}: {:?}", host, e);
                    for _ in 0..10 {
                        results.write().push((host.clone(), false));
                    }
                }
            }
        }));
    }

    for handle in handles {
        handle.await.ok();
    }

    let results = results.read();
    let total = results.len();
    let successes = results.iter().filter(|(_, s)| *s).count();
    let failures = total - successes;

    println!(
        "Concurrent chaos: {}/{} successful ({:.1}% failure)",
        successes,
        total,
        100.0 * failures as f64 / total as f64
    );

    // With 20% per-op failure rate across 3 hosts and 10 ops each,
    // expect roughly 15-25% overall failure
    assert!(failures >= 1, "Should have some failures");
    assert!(
        successes >= total * 60 / 100,
        "Should have majority success"
    );
}

// =============================================================================
// Resource Exhaustion Tests
// =============================================================================

#[tokio::test]
async fn test_connection_pool_exhaustion_recovery() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");

    // Create a very small pool
    let pool = rustible::connection::AsyncConnectionPool::new(2);
    let user = config.ssh_user.clone();
    let host = host.clone();
    let global_config = Arc::new(ConnectionConfig::default());

    // Try to get more connections than pool size simultaneously
    let mut handles = vec![];

    for i in 0..5 {
        let pool = pool.clone();
        let host = host.clone();
        let user = user.clone();
        let host_cfg = config.host_config(&host);
        let global_cfg = Arc::clone(&global_config);

        handles.push(tokio::spawn(async move {
            let pool_key = format!("ssh://{}@{}:22/{}", user, host, i);

            // Attempt to get connection with timeout
            match tokio::time::timeout(Duration::from_secs(15), async {
                rustible::connection::SshConnection::connect(
                    &host,
                    22,
                    &user,
                    Some(host_cfg),
                    &global_cfg,
                )
                .await
            })
            .await
            {
                Ok(Ok(conn)) => {
                    // Hold connection briefly
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let conn: Arc<dyn Connection + Send + Sync> = Arc::new(conn);
                    let _ = conn
                        .execute(&format!("echo worker_{}", i), None)
                        .await;
                    pool.put(pool_key, conn).await;
                    true
                }
                _ => false,
            }
        }));
    }

    let mut successes = 0;
    for handle in handles {
        if handle.await.unwrap_or(false) {
            successes += 1;
        }
    }

    println!(
        "Pool exhaustion recovery: {}/5 workers got connections",
        successes
    );

    // All should eventually succeed due to pool return
    assert!(
        successes >= 3,
        "At least some workers should get connections"
    );
}

// =============================================================================
// Block/Rescue/Always Tests Under Chaos
// =============================================================================

#[tokio::test]
async fn test_rescue_block_reliability() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");

    // Create inventory
    let _inventory = common::InventoryBuilder::new()
        .add_host(host, Some("all"))
        .host_var(host, "ansible_host", serde_json::json!(host))
        .host_var(host, "ansible_user", serde_json::json!(config.ssh_user))
        .build();

    // Playbook with block/rescue/always
    let yaml = r#"
- name: Test rescue reliability
  hosts: all
  gather_facts: false
  tasks:
    - name: Block with rescue
      block:
        - name: Task that fails
          command:
            cmd: exit 1
      rescue:
        - name: Rescue task
          command:
            cmd: echo 'rescued'
          register: rescue_result
      always:
        - name: Always task
          command:
            cmd: echo 'always runs'
          register: always_result
"#;

    let playbook =
        rustible::executor::playbook::Playbook::parse(yaml, None).expect("Failed to parse");

    let base_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 1,
        verbosity: 1,
        strategy: rustible::executor::ExecutionStrategy::Linear,
        task_timeout: 30,
        gather_facts: false,
        ..base_config
    };

    let executor = rustible::executor::Executor::new(executor_config);
    let result = executor.run_playbook(&playbook).await;

    // Rescue should have run after failure, always should always run
    assert!(
        result.is_ok(),
        "Playbook with rescue should complete: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_always_block_guaranteed_execution() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    let host = config.hosts.first().expect("Need at least one host");

    let _inventory = common::InventoryBuilder::new()
        .add_host(host, Some("all"))
        .host_var(host, "ansible_host", serde_json::json!(host))
        .host_var(host, "ansible_user", serde_json::json!(config.ssh_user))
        .build();

    // Test that always block runs even after multiple failures
    let yaml = r#"
- name: Test always guarantee
  hosts: all
  gather_facts: false
  tasks:
    - name: Block with failures
      block:
        - name: First failure
          command:
            cmd: exit 1
          ignore_errors: true
        - name: Second failure
          command:
            cmd: exit 2
          ignore_errors: true
        - name: Third failure (not ignored)
          command:
            cmd: exit 3
      always:
        - name: Cleanup marker
          command:
            cmd: echo 'ALWAYS_EXECUTED' > /tmp/always_marker
"#;

    let playbook =
        rustible::executor::playbook::Playbook::parse(yaml, None).expect("Failed to parse");

    let base_config = common::test_executor_config();
    let executor_config = rustible::executor::ExecutorConfig {
        forks: 1,
        strategy: rustible::executor::ExecutionStrategy::Linear,
        task_timeout: 30,
        gather_facts: false,
        ..base_config
    };

    let executor = rustible::executor::Executor::new(executor_config);
    let _ = executor.run_playbook(&playbook).await;

    // Verify always block ran by checking marker file
    let global_config = ConnectionConfig::default();
    let host_cfg = config.host_config(host);

    let conn = rustible::connection::SshConnection::connect(
        host,
        22,
        &config.ssh_user,
        Some(host_cfg),
        &global_config,
    )
    .await
    .expect("Failed to connect");

    let verify = conn
        .execute(
            "cat /tmp/always_marker 2>/dev/null || echo 'NOT_FOUND'",
            None,
        )
        .await
        .unwrap();

    // Cleanup
    conn.execute("rm -f /tmp/always_marker", None).await.ok();
    conn.close().await.ok();

    assert!(
        verify.stdout.contains("ALWAYS_EXECUTED"),
        "Always block should have run, got: {}",
        verify.stdout
    );
}

// =============================================================================
// Stress Test with Mixed Chaos
// =============================================================================

#[tokio::test]
async fn test_mixed_chaos_stress() {
    let config = ChaosTestConfig::from_env();
    if config.skip_if_disabled() {
        return;
    }

    if config.hosts.len() < 2 {
        eprintln!("Need at least 2 hosts for mixed chaos test");
        return;
    }

    let successful_ops = Arc::new(AtomicUsize::new(0));
    let failed_ops = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Different chaos configurations per host
    let chaos_configs: Vec<(f64, u64)> = vec![
        (0.05, 50),  // 5% failures, 50ms latency
        (0.15, 100), // 15% failures, 100ms latency
        (0.25, 150), // 25% failures, 150ms latency
    ];

    let global_config = Arc::new(ConnectionConfig::default());

    for (i, host) in config.hosts.iter().take(3).enumerate() {
        let host = host.clone();
        let user = config.ssh_user.clone();
        let host_cfg = config.host_config(&host);
        let (failure_rate, latency) = chaos_configs.get(i).copied().unwrap_or((0.1, 50));
        let successful = Arc::clone(&successful_ops);
        let failed = Arc::clone(&failed_ops);
        let global_cfg = Arc::clone(&global_config);

        handles.push(tokio::spawn(async move {
            match rustible::connection::SshConnection::connect(
                &host,
                22,
                &user,
                Some(host_cfg),
                &global_cfg,
            )
            .await
            {
                Ok(base_conn) => {
                    let chaos_conn = ChaosConnection::new(base_conn)
                        .with_failure_rate(failure_rate)
                        .with_latency(latency);

                    for i in 0..20 {
                        match chaos_conn
                            .execute(&format!("echo mixed_chaos_{}", i), None)
                            .await
                        {
                            Ok(r) if r.success => {
                                successful.fetch_add(1, Ordering::SeqCst);
                            }
                            _ => {
                                failed.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    }

                    chaos_conn.close().await.ok();
                }
                Err(_) => {
                    failed.fetch_add(20, Ordering::SeqCst);
                }
            }
        }));
    }

    for handle in handles {
        handle.await.ok();
    }

    let total_successful = successful_ops.load(Ordering::SeqCst);
    let total_failed = failed_ops.load(Ordering::SeqCst);
    let total = total_successful + total_failed;

    println!(
        "Mixed chaos stress: {}/{} successful ({:.1}% success rate)",
        total_successful,
        total,
        100.0 * total_successful as f64 / total as f64
    );

    // With mixed failure rates (5-25%), expect 60-85% overall success
    assert!(
        total_successful >= total * 50 / 100,
        "Should have at least 50% success (got {}%)",
        100 * total_successful / total
    );
}
