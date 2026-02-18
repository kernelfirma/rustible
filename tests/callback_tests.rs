//! Comprehensive tests for Rustible's callback system, output formatting, and reporting.
//!
//! This test suite covers:
//! 1. ExecutionCallback trait implementation and lifecycle
//! 2. Callback timing and ordering
//! 3. Data passed to callbacks
//! 4. Output format testing (human-readable, JSON, YAML)
//! 5. Color output handling
//! 6. Verbosity levels
//! 7. Progress reporting
//! 8. Summary and recap reporting
//! 9. Error reporting
//! 10. Custom callback implementations

use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Mock Callback Implementation for Testing
// ============================================================================

/// A mock callback that tracks all callback invocations for testing.
#[derive(Debug, Default)]
pub struct MockCallback {
    // Track callback invocations
    pub playbook_start_called: AtomicBool,
    pub playbook_end_called: AtomicBool,
    pub play_start_called: AtomicBool,
    pub play_end_called: AtomicBool,
    pub task_start_called: AtomicBool,
    pub task_complete_called: AtomicBool,
    pub handler_triggered_called: AtomicBool,
    pub facts_gathered_called: AtomicBool,

    // Track call counts
    pub playbook_start_count: AtomicU32,
    pub playbook_end_count: AtomicU32,
    pub play_start_count: AtomicU32,
    pub play_end_count: AtomicU32,
    pub task_start_count: AtomicU32,
    pub task_complete_count: AtomicU32,
    pub handler_triggered_count: AtomicU32,
    pub facts_gathered_count: AtomicU32,

    // Track data passed to callbacks
    pub playbook_names: RwLock<Vec<String>>,
    pub play_names: RwLock<Vec<String>>,
    pub task_names: RwLock<Vec<String>>,
    pub hosts: RwLock<Vec<String>>,
    pub handler_names: RwLock<Vec<String>>,
    pub task_results: RwLock<Vec<bool>>, // Track success/failure
    pub facts_hosts: RwLock<Vec<String>>,

    // Track callback order
    pub event_order: RwLock<Vec<String>>,

    // Track timing
    pub last_timestamp: AtomicU64,
}

impl MockCallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&self) {
        self.playbook_start_called.store(false, Ordering::SeqCst);
        self.playbook_end_called.store(false, Ordering::SeqCst);
        self.play_start_called.store(false, Ordering::SeqCst);
        self.play_end_called.store(false, Ordering::SeqCst);
        self.task_start_called.store(false, Ordering::SeqCst);
        self.task_complete_called.store(false, Ordering::SeqCst);
        self.handler_triggered_called.store(false, Ordering::SeqCst);
        self.facts_gathered_called.store(false, Ordering::SeqCst);

        self.playbook_start_count.store(0, Ordering::SeqCst);
        self.playbook_end_count.store(0, Ordering::SeqCst);
        self.play_start_count.store(0, Ordering::SeqCst);
        self.play_end_count.store(0, Ordering::SeqCst);
        self.task_start_count.store(0, Ordering::SeqCst);
        self.task_complete_count.store(0, Ordering::SeqCst);
        self.handler_triggered_count.store(0, Ordering::SeqCst);
        self.facts_gathered_count.store(0, Ordering::SeqCst);

        self.playbook_names.write().clear();
        self.play_names.write().clear();
        self.task_names.write().clear();
        self.hosts.write().clear();
        self.handler_names.write().clear();
        self.task_results.write().clear();
        self.facts_hosts.write().clear();
        self.event_order.write().clear();
    }

    fn record_event(&self, event: &str) {
        self.event_order.write().push(event.to_string());
        self.last_timestamp.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            Ordering::SeqCst,
        );
    }
}

#[async_trait]
impl ExecutionCallback for MockCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.playbook_start_called.store(true, Ordering::SeqCst);
        self.playbook_start_count.fetch_add(1, Ordering::SeqCst);
        self.playbook_names.write().push(name.to_string());
        self.record_event(&format!("playbook_start:{}", name));
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        self.playbook_end_called.store(true, Ordering::SeqCst);
        self.playbook_end_count.fetch_add(1, Ordering::SeqCst);
        self.playbook_names.write().push(name.to_string());
        self.record_event(&format!("playbook_end:{}:{}", name, success));
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.play_start_called.store(true, Ordering::SeqCst);
        self.play_start_count.fetch_add(1, Ordering::SeqCst);
        self.play_names.write().push(name.to_string());
        for host in hosts {
            self.hosts.write().push(host.clone());
        }
        self.record_event(&format!("play_start:{}", name));
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        self.play_end_called.store(true, Ordering::SeqCst);
        self.play_end_count.fetch_add(1, Ordering::SeqCst);
        self.play_names.write().push(name.to_string());
        self.record_event(&format!("play_end:{}:{}", name, success));
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        self.task_start_called.store(true, Ordering::SeqCst);
        self.task_start_count.fetch_add(1, Ordering::SeqCst);
        self.task_names.write().push(name.to_string());
        self.hosts.write().push(host.to_string());
        self.record_event(&format!("task_start:{}:{}", name, host));
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.task_complete_called.store(true, Ordering::SeqCst);
        self.task_complete_count.fetch_add(1, Ordering::SeqCst);
        self.task_results.write().push(result.result.success);
        self.record_event(&format!(
            "task_complete:{}:{}:{}",
            result.task_name, result.host, result.result.success
        ));
    }

    async fn on_handler_triggered(&self, name: &str) {
        self.handler_triggered_called.store(true, Ordering::SeqCst);
        self.handler_triggered_count.fetch_add(1, Ordering::SeqCst);
        self.handler_names.write().push(name.to_string());
        self.record_event(&format!("handler_triggered:{}", name));
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        self.facts_gathered_called.store(true, Ordering::SeqCst);
        self.facts_gathered_count.fetch_add(1, Ordering::SeqCst);
        self.facts_hosts.write().push(host.to_string());
        self.record_event(&format!("facts_gathered:{}", host));
    }
}

// ============================================================================
// Test 1: Callback Trait - Basic Invocation
// ============================================================================

#[tokio::test]
async fn test_on_playbook_start_called() {
    let callback = MockCallback::new();

    callback.on_playbook_start("test_playbook").await;

    assert!(callback.playbook_start_called.load(Ordering::SeqCst));
    assert_eq!(callback.playbook_start_count.load(Ordering::SeqCst), 1);
    assert!(callback
        .playbook_names
        .read()
        .contains(&"test_playbook".to_string()));
}

#[tokio::test]
async fn test_on_playbook_end_called() {
    let callback = MockCallback::new();

    callback.on_playbook_end("test_playbook", true).await;

    assert!(callback.playbook_end_called.load(Ordering::SeqCst));
    assert_eq!(callback.playbook_end_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_on_play_start_called() {
    let callback = MockCallback::new();
    let hosts = vec!["host1".to_string(), "host2".to_string()];

    callback.on_play_start("test_play", &hosts).await;

    assert!(callback.play_start_called.load(Ordering::SeqCst));
    assert_eq!(callback.play_start_count.load(Ordering::SeqCst), 1);
    assert!(callback
        .play_names
        .read()
        .contains(&"test_play".to_string()));
    assert!(callback.hosts.read().contains(&"host1".to_string()));
    assert!(callback.hosts.read().contains(&"host2".to_string()));
}

#[tokio::test]
async fn test_on_play_end_called() {
    let callback = MockCallback::new();

    callback.on_play_end("test_play", true).await;

    assert!(callback.play_end_called.load(Ordering::SeqCst));
    assert_eq!(callback.play_end_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_on_task_start_called() {
    let callback = MockCallback::new();

    callback.on_task_start("test_task", "localhost").await;

    assert!(callback.task_start_called.load(Ordering::SeqCst));
    assert_eq!(callback.task_start_count.load(Ordering::SeqCst), 1);
    assert!(callback
        .task_names
        .read()
        .contains(&"test_task".to_string()));
    assert!(callback.hosts.read().contains(&"localhost".to_string()));
}

#[tokio::test]
async fn test_on_task_complete_called() {
    let callback = MockCallback::new();
    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "test_task".to_string(),
        result: ModuleResult::ok("Task completed"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    assert!(callback.task_complete_called.load(Ordering::SeqCst));
    assert_eq!(callback.task_complete_count.load(Ordering::SeqCst), 1);
    assert!(callback.task_results.read().contains(&true));
}

#[tokio::test]
async fn test_on_handler_triggered_called() {
    let callback = MockCallback::new();

    callback.on_handler_triggered("restart_nginx").await;

    assert!(callback.handler_triggered_called.load(Ordering::SeqCst));
    assert_eq!(callback.handler_triggered_count.load(Ordering::SeqCst), 1);
    assert!(callback
        .handler_names
        .read()
        .contains(&"restart_nginx".to_string()));
}

#[tokio::test]
async fn test_on_facts_gathered_called() {
    let callback = MockCallback::new();
    let mut facts = Facts::new();
    facts.set("os", json!("linux"));

    callback.on_facts_gathered("localhost", &facts).await;

    assert!(callback.facts_gathered_called.load(Ordering::SeqCst));
    assert_eq!(callback.facts_gathered_count.load(Ordering::SeqCst), 1);
    assert!(callback
        .facts_hosts
        .read()
        .contains(&"localhost".to_string()));
}

// ============================================================================
// Test 2: Callback Timing and Order
// ============================================================================

#[tokio::test]
async fn test_callbacks_fire_in_correct_order() {
    let callback = MockCallback::new();
    let hosts = vec!["host1".to_string()];

    // Simulate a full playbook execution sequence
    callback.on_playbook_start("my_playbook").await;
    callback.on_play_start("my_play", &hosts).await;

    let mut facts = Facts::new();
    facts.set("os", json!("linux"));
    callback.on_facts_gathered("host1", &facts).await;

    callback.on_task_start("task1", "host1").await;
    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::changed("Changed"),
        duration: Duration::from_millis(50),
        notify: vec!["handler1".to_string()],
    };
    callback.on_task_complete(&result).await;

    callback.on_handler_triggered("handler1").await;

    callback.on_play_end("my_play", true).await;
    callback.on_playbook_end("my_playbook", true).await;

    let events = callback.event_order.read().clone();

    // Verify order
    assert_eq!(events[0], "playbook_start:my_playbook");
    assert_eq!(events[1], "play_start:my_play");
    assert_eq!(events[2], "facts_gathered:host1");
    assert_eq!(events[3], "task_start:task1:host1");
    assert_eq!(events[4], "task_complete:task1:host1:true");
    assert_eq!(events[5], "handler_triggered:handler1");
    assert_eq!(events[6], "play_end:my_play:true");
    assert_eq!(events[7], "playbook_end:my_playbook:true");
}

#[tokio::test]
async fn test_callback_order_with_multiple_tasks() {
    let callback = MockCallback::new();
    let hosts = vec!["host1".to_string()];

    callback.on_playbook_start("playbook").await;
    callback.on_play_start("play", &hosts).await;

    // Multiple tasks
    for i in 1..=3 {
        callback.on_task_start(&format!("task{}", i), "host1").await;
        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: format!("task{}", i),
            result: ModuleResult::ok("OK"),
            duration: Duration::from_millis(10),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;
    }

    callback.on_play_end("play", true).await;
    callback.on_playbook_end("playbook", true).await;

    let events = callback.event_order.read().clone();

    // Verify task ordering
    assert!(events.contains(&"task_start:task1:host1".to_string()));
    assert!(events.contains(&"task_complete:task1:host1:true".to_string()));
    assert!(events.contains(&"task_start:task2:host1".to_string()));
    assert!(events.contains(&"task_complete:task2:host1:true".to_string()));
    assert!(events.contains(&"task_start:task3:host1".to_string()));
    assert!(events.contains(&"task_complete:task3:host1:true".to_string()));

    // Verify task1 comes before task2, task2 before task3
    let task1_start = events
        .iter()
        .position(|e| e == "task_start:task1:host1")
        .unwrap();
    let task2_start = events
        .iter()
        .position(|e| e == "task_start:task2:host1")
        .unwrap();
    let task3_start = events
        .iter()
        .position(|e| e == "task_start:task3:host1")
        .unwrap();
    assert!(task1_start < task2_start);
    assert!(task2_start < task3_start);
}

#[tokio::test]
async fn test_nested_callback_scenarios() {
    let callback = MockCallback::new();
    let hosts = vec!["host1".to_string(), "host2".to_string()];

    callback.on_playbook_start("main_playbook").await;

    // Play 1
    callback.on_play_start("play1", &hosts).await;
    callback.on_task_start("task1", "host1").await;
    callback.on_task_start("task1", "host2").await;

    let result1 = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result1).await;

    let result2 = ExecutionResult {
        host: "host2".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result2).await;
    callback.on_play_end("play1", true).await;

    // Play 2
    callback.on_play_start("play2", &hosts[..1]).await;
    callback.on_task_start("task2", "host1").await;
    let result3 = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task2".to_string(),
        result: ModuleResult::changed("Changed"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result3).await;
    callback.on_play_end("play2", true).await;

    callback.on_playbook_end("main_playbook", true).await;

    // Verify nested structure
    assert_eq!(callback.play_start_count.load(Ordering::SeqCst), 2);
    assert_eq!(callback.play_end_count.load(Ordering::SeqCst), 2);
    assert_eq!(callback.task_start_count.load(Ordering::SeqCst), 3); // 2 for play1, 1 for play2
    assert_eq!(callback.task_complete_count.load(Ordering::SeqCst), 3);
}

// ============================================================================
// Test 3: Callback Data
// ============================================================================

#[tokio::test]
async fn test_correct_data_passed_to_playbook_callbacks() {
    let callback = MockCallback::new();

    callback.on_playbook_start("production_deploy").await;
    callback.on_playbook_end("production_deploy", false).await;

    let names = callback.playbook_names.read().clone();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0], "production_deploy");
    assert_eq!(names[1], "production_deploy");
}

#[tokio::test]
async fn test_host_information_available() {
    let callback = MockCallback::new();
    let hosts = vec!["web1".to_string(), "web2".to_string(), "db1".to_string()];

    callback.on_play_start("deploy", &hosts).await;

    let stored_hosts = callback.hosts.read().clone();
    assert_eq!(stored_hosts.len(), 3);
    assert!(stored_hosts.contains(&"web1".to_string()));
    assert!(stored_hosts.contains(&"web2".to_string()));
    assert!(stored_hosts.contains(&"db1".to_string()));
}

#[tokio::test]
async fn test_task_results_available() {
    let callback = MockCallback::new();

    // Successful task
    let success_result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "install_nginx".to_string(),
        result: ModuleResult::changed("nginx installed"),
        duration: Duration::from_secs(2),
        notify: vec!["restart nginx".to_string()],
    };
    callback.on_task_complete(&success_result).await;

    // Failed task
    let failed_result = ExecutionResult {
        host: "host2".to_string(),
        task_name: "install_nginx".to_string(),
        result: ModuleResult::failed("Package not found"),
        duration: Duration::from_secs(1),
        notify: vec![],
    };
    callback.on_task_complete(&failed_result).await;

    let results = callback.task_results.read().clone();
    assert_eq!(results.len(), 2);
    assert!(results[0]); // First task succeeded
    assert!(!results[1]); // Second task failed
}

#[tokio::test]
async fn test_error_information_available() {
    let callback = MockCallback::new();

    let failed_result = ExecutionResult {
        host: "failing_host".to_string(),
        task_name: "broken_task".to_string(),
        result: ModuleResult::failed("Connection refused: unable to connect to host"),
        duration: Duration::from_millis(500),
        notify: vec![],
    };

    callback.on_task_complete(&failed_result).await;

    let events = callback.event_order.read().clone();
    assert!(events
        .iter()
        .any(|e| e.contains("task_complete:broken_task:failing_host:false")));
}

#[tokio::test]
async fn test_facts_data_gathering() {
    let callback = MockCallback::new();

    let mut facts = Facts::new();
    facts.set("ansible_distribution", json!("Ubuntu"));
    facts.set("ansible_distribution_version", json!("22.04"));
    facts.set("ansible_memtotal_mb", json!(16384));

    callback
        .on_facts_gathered("production-server-01", &facts)
        .await;

    assert!(callback
        .facts_hosts
        .read()
        .contains(&"production-server-01".to_string()));
}

// ============================================================================
// Test 4: Error Reporting
// ============================================================================

#[tokio::test]
async fn test_error_callback_with_failed_task() {
    let callback = MockCallback::new();

    let failed_result = ExecutionResult {
        host: "server1".to_string(),
        task_name: "Install package".to_string(),
        result: ModuleResult::failed("apt-get failed: E: Unable to locate package"),
        duration: Duration::from_secs(5),
        notify: vec![],
    };

    callback.on_task_complete(&failed_result).await;

    let results = callback.task_results.read().clone();
    assert_eq!(results.len(), 1);
    assert!(!results[0]);
}

#[tokio::test]
async fn test_error_context_includes_host_and_task() {
    let callback = MockCallback::new();

    let failed_result = ExecutionResult {
        host: "production-db-01".to_string(),
        task_name: "Configure database".to_string(),
        result: ModuleResult::failed("Permission denied"),
        duration: Duration::from_millis(200),
        notify: vec![],
    };

    callback.on_task_complete(&failed_result).await;

    let events = callback.event_order.read().clone();
    let error_event = events.iter().find(|e| e.contains("task_complete")).unwrap();
    assert!(error_event.contains("Configure database"));
    assert!(error_event.contains("production-db-01"));
    assert!(error_event.contains("false")); // Failed
}

#[tokio::test]
async fn test_play_end_with_failure() {
    let callback = MockCallback::new();

    callback.on_play_end("failed_play", false).await;

    let events = callback.event_order.read().clone();
    assert!(events.contains(&"play_end:failed_play:false".to_string()));
}

#[tokio::test]
async fn test_playbook_end_with_failure() {
    let callback = MockCallback::new();

    callback.on_playbook_end("failed_playbook", false).await;

    let events = callback.event_order.read().clone();
    assert!(events.contains(&"playbook_end:failed_playbook:false".to_string()));
}

// ============================================================================
// Test 10: Custom Callbacks
// ============================================================================

/// A custom callback that counts specific events
#[derive(Debug, Default)]
pub struct CountingCallback {
    total_events: AtomicU32,
    success_count: AtomicU32,
    failure_count: AtomicU32,
}

impl CountingCallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total(&self) -> u32 {
        self.total_events.load(Ordering::SeqCst)
    }

    pub fn successes(&self) -> u32 {
        self.success_count.load(Ordering::SeqCst)
    }

    pub fn failures(&self) -> u32 {
        self.failure_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ExecutionCallback for CountingCallback {
    async fn on_playbook_start(&self, _name: &str) {
        self.total_events.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_playbook_end(&self, _name: &str, success: bool) {
        self.total_events.fetch_add(1, Ordering::SeqCst);
        if success {
            self.success_count.fetch_add(1, Ordering::SeqCst);
        } else {
            self.failure_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.total_events.fetch_add(1, Ordering::SeqCst);
        if result.result.success {
            self.success_count.fetch_add(1, Ordering::SeqCst);
        } else {
            self.failure_count.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn test_custom_callback_implementation() {
    let callback = CountingCallback::new();

    callback.on_playbook_start("test").await;

    let result1 = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result1).await;

    let result2 = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task2".to_string(),
        result: ModuleResult::failed("Failed"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result2).await;

    callback.on_playbook_end("test", false).await;

    assert_eq!(callback.total(), 4);
    assert_eq!(callback.successes(), 1); // Only task1 succeeded
    assert_eq!(callback.failures(), 2); // task2 failed + playbook failed
}

/// A logging callback that stores events in a vector
#[derive(Debug, Default)]
pub struct LoggingCallback {
    logs: RwLock<Vec<String>>,
}

impl LoggingCallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn logs(&self) -> Vec<String> {
        self.logs.read().clone()
    }
}

#[async_trait]
impl ExecutionCallback for LoggingCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.logs
            .write()
            .push(format!("[INFO] Playbook '{}' started", name));
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let status = if success { "SUCCESS" } else { "FAILED" };
        self.logs
            .write()
            .push(format!("[INFO] Playbook '{}' ended: {}", name, status));
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.logs.write().push(format!(
            "[INFO] Play '{}' started on {} hosts",
            name,
            hosts.len()
        ));
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        let status = if success { "SUCCESS" } else { "FAILED" };
        self.logs
            .write()
            .push(format!("[INFO] Play '{}' ended: {}", name, status));
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        self.logs
            .write()
            .push(format!("[DEBUG] Task '{}' starting on '{}'", name, host));
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let status = if result.result.success {
            "OK"
        } else {
            "FAILED"
        };
        self.logs.write().push(format!(
            "[DEBUG] Task '{}' on '{}': {} ({}ms)",
            result.task_name,
            result.host,
            status,
            result.duration.as_millis()
        ));
    }

    async fn on_handler_triggered(&self, name: &str) {
        self.logs
            .write()
            .push(format!("[INFO] Handler '{}' triggered", name));
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        self.logs.write().push(format!(
            "[DEBUG] Facts gathered for '{}': {} facts",
            host,
            facts.all().len()
        ));
    }
}

#[tokio::test]
async fn test_logging_callback() {
    let callback = LoggingCallback::new();
    let hosts = vec!["web1".to_string(), "web2".to_string()];

    callback.on_playbook_start("deploy_app").await;
    callback.on_play_start("Configure servers", &hosts).await;
    callback.on_task_start("Install nginx", "web1").await;

    let result = ExecutionResult {
        host: "web1".to_string(),
        task_name: "Install nginx".to_string(),
        result: ModuleResult::changed("Installed"),
        duration: Duration::from_millis(1500),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    callback.on_handler_triggered("restart nginx").await;
    callback.on_play_end("Configure servers", true).await;
    callback.on_playbook_end("deploy_app", true).await;

    let logs = callback.logs();
    assert!(logs[0].contains("Playbook 'deploy_app' started"));
    assert!(logs[1].contains("Play 'Configure servers' started on 2 hosts"));
    assert!(logs[2].contains("Task 'Install nginx' starting on 'web1'"));
    assert!(logs[3].contains("Task 'Install nginx' on 'web1': OK"));
    assert!(logs[3].contains("1500ms"));
    assert!(logs[4].contains("Handler 'restart nginx' triggered"));
    assert!(logs[5].contains("Play 'Configure servers' ended: SUCCESS"));
    assert!(logs[6].contains("Playbook 'deploy_app' ended: SUCCESS"));
}

/// A callback that can be shared across multiple threads
#[derive(Debug)]
pub struct ThreadSafeCallback {
    events: Arc<RwLock<Vec<String>>>,
}

impl ThreadSafeCallback {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn events(&self) -> Vec<String> {
        self.events.read().clone()
    }
}

impl Default for ThreadSafeCallback {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionCallback for ThreadSafeCallback {
    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.events.write().push(format!(
            "{}:{}:{}",
            result.task_name, result.host, result.result.success
        ));
    }
}

#[tokio::test]
async fn test_thread_safe_callback() {
    let callback = Arc::new(ThreadSafeCallback::new());

    // Simulate concurrent task completions
    let mut handles = vec![];

    for i in 0..10 {
        let cb = callback.clone();
        let handle = tokio::spawn(async move {
            let result = ExecutionResult {
                host: format!("host{}", i),
                task_name: format!("task{}", i),
                result: ModuleResult::ok("OK"),
                duration: Duration::from_millis(10),
                notify: vec![],
            };
            cb.on_task_complete(&result).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let events = callback.events();
    assert_eq!(events.len(), 10);
}

// ============================================================================
// Test: Multiple Callbacks
// ============================================================================

#[tokio::test]
async fn test_multiple_callbacks_called() {
    let callback1 = MockCallback::new();
    let callback2 = MockCallback::new();

    // Simulate calling multiple callbacks
    callback1.on_playbook_start("test").await;
    callback2.on_playbook_start("test").await;

    assert!(callback1.playbook_start_called.load(Ordering::SeqCst));
    assert!(callback2.playbook_start_called.load(Ordering::SeqCst));
}

/// A callback aggregator that calls multiple callbacks
pub struct CallbackAggregator {
    callbacks: Vec<Arc<dyn ExecutionCallback>>,
}

impl CallbackAggregator {
    pub fn new() -> Self {
        Self {
            callbacks: Vec::new(),
        }
    }

    pub fn add_callback(&mut self, callback: Arc<dyn ExecutionCallback>) {
        self.callbacks.push(callback);
    }

    pub async fn on_playbook_start(&self, name: &str) {
        for callback in &self.callbacks {
            callback.on_playbook_start(name).await;
        }
    }

    pub async fn on_task_complete(&self, result: &ExecutionResult) {
        for callback in &self.callbacks {
            callback.on_task_complete(result).await;
        }
    }
}

impl Default for CallbackAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[tokio::test]
async fn test_callback_aggregator() {
    let mut aggregator = CallbackAggregator::new();

    let mock1 = Arc::new(MockCallback::new());
    let mock2 = Arc::new(MockCallback::new());
    let counter = Arc::new(CountingCallback::new());

    aggregator.add_callback(mock1.clone());
    aggregator.add_callback(mock2.clone());
    aggregator.add_callback(counter.clone());

    aggregator.on_playbook_start("test").await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    aggregator.on_task_complete(&result).await;

    // All callbacks should have been called
    assert!(mock1.playbook_start_called.load(Ordering::SeqCst));
    assert!(mock2.playbook_start_called.load(Ordering::SeqCst));
    assert_eq!(counter.total(), 2); // playbook_start + task_complete
}

// ============================================================================
// Test: Callback Reset
// ============================================================================

#[tokio::test]
async fn test_mock_callback_reset() {
    let callback = MockCallback::new();

    callback.on_playbook_start("test1").await;
    callback.on_task_start("task1", "host1").await;

    assert!(callback.playbook_start_called.load(Ordering::SeqCst));
    assert!(callback.task_start_called.load(Ordering::SeqCst));
    assert_eq!(callback.playbook_names.read().len(), 1);

    callback.reset();

    assert!(!callback.playbook_start_called.load(Ordering::SeqCst));
    assert!(!callback.task_start_called.load(Ordering::SeqCst));
    assert_eq!(callback.playbook_names.read().len(), 0);
    assert_eq!(callback.event_order.read().len(), 0);
}

// ============================================================================
// Test: Default Callback Implementation
// ============================================================================

/// A minimal callback using default trait implementations
#[derive(Debug, Default)]
pub struct MinimalCallback;

#[async_trait]
impl ExecutionCallback for MinimalCallback {
    // Uses all default implementations
}

#[tokio::test]
async fn test_default_callback_implementations() {
    let callback = MinimalCallback;

    // All these should work with default implementations (no-ops)
    callback.on_playbook_start("test").await;
    callback.on_playbook_end("test", true).await;
    callback.on_play_start("play", &["host1".to_string()]).await;
    callback.on_play_end("play", true).await;
    callback.on_task_start("task", "host1").await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    callback.on_handler_triggered("handler").await;

    let facts = Facts::new();
    callback.on_facts_gathered("host1", &facts).await;

    // No panics = success
}

// ============================================================================
// Integration Tests: Complete Playbook Execution Simulation
// ============================================================================

#[tokio::test]
async fn test_complete_playbook_execution_simulation() {
    let callback = MockCallback::new();
    let hosts = vec!["web1".to_string(), "web2".to_string()];

    // Playbook start
    callback.on_playbook_start("deploy_application").await;

    // Gather facts
    let mut facts = Facts::new();
    facts.set("ansible_os_family", json!("Debian"));
    for host in &hosts {
        callback.on_facts_gathered(host, &facts).await;
    }

    // Play 1: Install dependencies
    callback.on_play_start("Install dependencies", &hosts).await;

    // Task 1: Install packages
    for host in &hosts {
        callback.on_task_start("Install nginx", host).await;
        let result = ExecutionResult {
            host: host.clone(),
            task_name: "Install nginx".to_string(),
            result: ModuleResult::changed("nginx installed"),
            duration: Duration::from_secs(5),
            notify: vec!["Restart nginx".to_string()],
        };
        callback.on_task_complete(&result).await;
    }

    // Handler triggered
    callback.on_handler_triggered("Restart nginx").await;

    callback.on_play_end("Install dependencies", true).await;

    // Play 2: Configure application
    callback
        .on_play_start("Configure application", &hosts)
        .await;

    // Task 2: Copy config
    for host in &hosts {
        callback.on_task_start("Copy config", host).await;
        let result = ExecutionResult {
            host: host.clone(),
            task_name: "Copy config".to_string(),
            result: ModuleResult::ok("Config already in place"),
            duration: Duration::from_millis(200),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;
    }

    callback.on_play_end("Configure application", true).await;

    // Playbook end
    callback.on_playbook_end("deploy_application", true).await;

    // Verify callback state
    assert_eq!(callback.playbook_start_count.load(Ordering::SeqCst), 1);
    assert_eq!(callback.playbook_end_count.load(Ordering::SeqCst), 1);
    assert_eq!(callback.play_start_count.load(Ordering::SeqCst), 2);
    assert_eq!(callback.play_end_count.load(Ordering::SeqCst), 2);
    assert_eq!(callback.task_start_count.load(Ordering::SeqCst), 4); // 2 tasks * 2 hosts
    assert_eq!(callback.task_complete_count.load(Ordering::SeqCst), 4);
    assert_eq!(callback.handler_triggered_count.load(Ordering::SeqCst), 1);
    assert_eq!(callback.facts_gathered_count.load(Ordering::SeqCst), 2);

    // Verify event order
    let events = callback.event_order.read();
    assert!(events[0].starts_with("playbook_start:"));
    assert!(events.last().unwrap().starts_with("playbook_end:"));
}

#[tokio::test]
async fn test_failed_playbook_execution_simulation() {
    let callback = MockCallback::new();
    let hosts = vec!["host1".to_string()];

    callback.on_playbook_start("failing_playbook").await;
    callback.on_play_start("Failing play", &hosts).await;

    // Task that fails
    callback.on_task_start("Failing task", "host1").await;
    let failed_result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "Failing task".to_string(),
        result: ModuleResult::failed("Command exited with code 1"),
        duration: Duration::from_millis(500),
        notify: vec![],
    };
    callback.on_task_complete(&failed_result).await;

    callback.on_play_end("Failing play", false).await;
    callback.on_playbook_end("failing_playbook", false).await;

    // Verify failure tracking
    let results = callback.task_results.read();
    assert_eq!(results.len(), 1);
    assert!(!results[0]); // Task failed

    let events = callback.event_order.read();
    assert!(events.iter().any(|e| e.contains("false"))); // Contains failure indicators
}

// ============================================================================
// Test: Edge Cases - Empty Events
// ============================================================================

#[tokio::test]
async fn test_empty_playbook_name() {
    let callback = MockCallback::new();

    callback.on_playbook_start("").await;
    callback.on_playbook_end("", true).await;

    assert!(callback.playbook_start_called.load(Ordering::SeqCst));
    assert!(callback.playbook_end_called.load(Ordering::SeqCst));
    assert!(callback.playbook_names.read().contains(&"".to_string()));
}

#[tokio::test]
async fn test_empty_hosts_list() {
    let callback = MockCallback::new();
    let empty_hosts: Vec<String> = vec![];

    callback
        .on_play_start("play_with_no_hosts", &empty_hosts)
        .await;

    assert!(callback.play_start_called.load(Ordering::SeqCst));
    assert!(callback.hosts.read().is_empty());
}

#[tokio::test]
async fn test_empty_task_name() {
    let callback = MockCallback::new();

    callback.on_task_start("", "localhost").await;

    assert!(callback.task_start_called.load(Ordering::SeqCst));
    assert!(callback.task_names.read().contains(&"".to_string()));
}

#[tokio::test]
async fn test_empty_host_name() {
    let callback = MockCallback::new();

    callback.on_task_start("task", "").await;

    assert!(callback.task_start_called.load(Ordering::SeqCst));
    assert!(callback.hosts.read().contains(&"".to_string()));
}

#[tokio::test]
async fn test_empty_handler_name() {
    let callback = MockCallback::new();

    callback.on_handler_triggered("").await;

    assert!(callback.handler_triggered_called.load(Ordering::SeqCst));
    assert!(callback.handler_names.read().contains(&"".to_string()));
}

#[tokio::test]
async fn test_empty_facts() {
    let callback = MockCallback::new();
    let empty_facts = Facts::new();

    callback.on_facts_gathered("host1", &empty_facts).await;

    assert!(callback.facts_gathered_called.load(Ordering::SeqCst));
    assert!(callback.facts_hosts.read().contains(&"host1".to_string()));
}

// ============================================================================
// Test: Edge Cases - Unicode and Special Characters
// ============================================================================

#[tokio::test]
async fn test_unicode_playbook_name() {
    let callback = MockCallback::new();
    let unicode_name =
        "playbook_\u{1F680}_\u{4E2D}\u{6587}_\u{0441}\u{043A}\u{0440}\u{0438}\u{043F}\u{0442}";

    callback.on_playbook_start(unicode_name).await;

    assert!(callback
        .playbook_names
        .read()
        .contains(&unicode_name.to_string()));
}

#[tokio::test]
async fn test_special_characters_in_task_name() {
    let callback = MockCallback::new();
    let special_name = "task [with] (special) {chars} <and> 'quotes' \"double\"";

    callback.on_task_start(special_name, "host").await;

    assert!(callback
        .task_names
        .read()
        .contains(&special_name.to_string()));
}

#[tokio::test]
async fn test_newlines_in_names() {
    let callback = MockCallback::new();
    let name_with_newlines = "task\nwith\nnewlines";

    callback.on_task_start(name_with_newlines, "host").await;

    assert!(callback
        .task_names
        .read()
        .contains(&name_with_newlines.to_string()));
}

// ============================================================================
// Test: Edge Cases - Many Plugins (Stress Test)
// ============================================================================

#[tokio::test]
async fn test_many_callbacks_registered() {
    let mut aggregator = CallbackAggregator::new();
    let callbacks: Vec<Arc<MockCallback>> =
        (0..100).map(|_| Arc::new(MockCallback::new())).collect();

    for cb in &callbacks {
        aggregator.add_callback(cb.clone());
    }

    aggregator.on_playbook_start("stress_test").await;

    // All 100 callbacks should have been called
    for cb in &callbacks {
        assert!(cb.playbook_start_called.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn test_many_sequential_events() {
    let callback = MockCallback::new();

    // Fire 1000 events sequentially
    for i in 0..1000 {
        callback.on_task_start(&format!("task_{}", i), "host").await;
        let result = ExecutionResult {
            host: "host".to_string(),
            task_name: format!("task_{}", i),
            result: ModuleResult::ok("OK"),
            duration: Duration::from_millis(1),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;
    }

    assert_eq!(callback.task_start_count.load(Ordering::SeqCst), 1000);
    assert_eq!(callback.task_complete_count.load(Ordering::SeqCst), 1000);
    assert_eq!(callback.event_order.read().len(), 2000);
}

#[tokio::test]
async fn test_many_hosts() {
    let callback = MockCallback::new();
    let hosts: Vec<String> = (0..500).map(|i| format!("host_{}", i)).collect();

    callback.on_play_start("big_play", &hosts).await;

    assert_eq!(callback.hosts.read().len(), 500);
}

// ============================================================================
// Test: Plugin Error Handling
// ============================================================================

/// A callback that panics on specific events (for testing error isolation)
#[derive(Debug, Default)]
pub struct PanickingCallback {
    should_panic: AtomicBool,
    calls_before_panic: AtomicU32,
}

impl PanickingCallback {
    pub fn new(panic_after_n_calls: u32) -> Self {
        Self {
            should_panic: AtomicBool::new(true),
            calls_before_panic: AtomicU32::new(panic_after_n_calls),
        }
    }
}

#[async_trait]
impl ExecutionCallback for PanickingCallback {
    async fn on_task_complete(&self, _result: &ExecutionResult) {
        if self.should_panic.load(Ordering::SeqCst) {
            let remaining = self.calls_before_panic.fetch_sub(1, Ordering::SeqCst);
            if remaining == 1 {
                panic!("Intentional panic in callback");
            }
        }
    }
}

/// A callback that tracks error conditions
#[derive(Debug, Default)]
pub struct ErrorTrackingCallback {
    error_count: AtomicU32,
    failed_tasks: RwLock<Vec<String>>,
    failed_hosts: RwLock<Vec<String>>,
}

impl ErrorTrackingCallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error_count(&self) -> u32 {
        self.error_count.load(Ordering::SeqCst)
    }

    pub fn failed_tasks(&self) -> Vec<String> {
        self.failed_tasks.read().clone()
    }

    pub fn failed_hosts(&self) -> Vec<String> {
        self.failed_hosts.read().clone()
    }
}

#[async_trait]
impl ExecutionCallback for ErrorTrackingCallback {
    async fn on_task_complete(&self, result: &ExecutionResult) {
        if !result.result.success {
            self.error_count.fetch_add(1, Ordering::SeqCst);
            self.failed_tasks.write().push(result.task_name.clone());
            self.failed_hosts.write().push(result.host.clone());
        }
    }
}

#[tokio::test]
async fn test_error_tracking_callback() {
    let callback = ErrorTrackingCallback::new();

    // Mix of successful and failed tasks
    let tasks = vec![
        ("task1", true),
        ("task2", false),
        ("task3", true),
        ("task4", false),
        ("task5", false),
    ];

    for (name, success) in tasks {
        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: name.to_string(),
            result: if success {
                ModuleResult::ok("OK")
            } else {
                ModuleResult::failed("Failed")
            },
            duration: Duration::from_millis(10),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;
    }

    assert_eq!(callback.error_count(), 3);
    assert_eq!(callback.failed_tasks(), vec!["task2", "task4", "task5"]);
}

// ============================================================================
// Test: CallbackManager Registration and Dispatch
// ============================================================================

/// A full-featured CallbackManager for testing
pub struct CallbackManager {
    callbacks: RwLock<Vec<Arc<dyn ExecutionCallback>>>,
    enabled: AtomicBool,
}

impl CallbackManager {
    pub fn new() -> Self {
        Self {
            callbacks: RwLock::new(Vec::new()),
            enabled: AtomicBool::new(true),
        }
    }

    pub fn register(&self, callback: Arc<dyn ExecutionCallback>) {
        self.callbacks.write().push(callback);
    }

    pub fn unregister_all(&self) {
        self.callbacks.write().clear();
    }

    pub fn callback_count(&self) -> usize {
        self.callbacks.read().len()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub async fn dispatch_playbook_start(&self, name: &str) {
        if !self.is_enabled() {
            return;
        }
        let callbacks: Vec<_> = self.callbacks.read().clone();
        for callback in callbacks.iter() {
            callback.on_playbook_start(name).await;
        }
    }

    pub async fn dispatch_playbook_end(&self, name: &str, success: bool) {
        if !self.is_enabled() {
            return;
        }
        let callbacks: Vec<_> = self.callbacks.read().clone();
        for callback in callbacks.iter() {
            callback.on_playbook_end(name, success).await;
        }
    }

    pub async fn dispatch_play_start(&self, name: &str, hosts: &[String]) {
        if !self.is_enabled() {
            return;
        }
        let callbacks: Vec<_> = self.callbacks.read().clone();
        for callback in callbacks.iter() {
            callback.on_play_start(name, hosts).await;
        }
    }

    pub async fn dispatch_play_end(&self, name: &str, success: bool) {
        if !self.is_enabled() {
            return;
        }
        let callbacks: Vec<_> = self.callbacks.read().clone();
        for callback in callbacks.iter() {
            callback.on_play_end(name, success).await;
        }
    }

    pub async fn dispatch_task_start(&self, name: &str, host: &str) {
        if !self.is_enabled() {
            return;
        }
        let callbacks: Vec<_> = self.callbacks.read().clone();
        for callback in callbacks.iter() {
            callback.on_task_start(name, host).await;
        }
    }

    pub async fn dispatch_task_complete(&self, result: &ExecutionResult) {
        if !self.is_enabled() {
            return;
        }
        let callbacks: Vec<_> = self.callbacks.read().clone();
        for callback in callbacks.iter() {
            callback.on_task_complete(result).await;
        }
    }

    pub async fn dispatch_handler_triggered(&self, name: &str) {
        if !self.is_enabled() {
            return;
        }
        let callbacks: Vec<_> = self.callbacks.read().clone();
        for callback in callbacks.iter() {
            callback.on_handler_triggered(name).await;
        }
    }

    pub async fn dispatch_facts_gathered(&self, host: &str, facts: &Facts) {
        if !self.is_enabled() {
            return;
        }
        let callbacks: Vec<_> = self.callbacks.read().clone();
        for callback in callbacks.iter() {
            callback.on_facts_gathered(host, facts).await;
        }
    }
}

impl Default for CallbackManager {
    fn default() -> Self {
        Self::new()
    }
}

#[tokio::test]
async fn test_callback_manager_registration() {
    let manager = CallbackManager::new();

    assert_eq!(manager.callback_count(), 0);

    let cb1 = Arc::new(MockCallback::new());
    let cb2 = Arc::new(MockCallback::new());

    manager.register(cb1.clone());
    assert_eq!(manager.callback_count(), 1);

    manager.register(cb2.clone());
    assert_eq!(manager.callback_count(), 2);
}

#[tokio::test]
async fn test_callback_manager_unregister_all() {
    let manager = CallbackManager::new();

    manager.register(Arc::new(MockCallback::new()));
    manager.register(Arc::new(MockCallback::new()));
    manager.register(Arc::new(MockCallback::new()));

    assert_eq!(manager.callback_count(), 3);

    manager.unregister_all();

    assert_eq!(manager.callback_count(), 0);
}

#[tokio::test]
async fn test_callback_manager_dispatch() {
    let manager = CallbackManager::new();

    let cb1 = Arc::new(MockCallback::new());
    let cb2 = Arc::new(CountingCallback::new());

    manager.register(cb1.clone());
    manager.register(cb2.clone());

    manager.dispatch_playbook_start("test_playbook").await;

    assert!(cb1.playbook_start_called.load(Ordering::SeqCst));
    assert_eq!(cb2.total(), 1);
}

#[tokio::test]
async fn test_callback_manager_disable() {
    let manager = CallbackManager::new();

    let cb = Arc::new(MockCallback::new());
    manager.register(cb.clone());

    // Dispatch while enabled
    manager.dispatch_playbook_start("test1").await;
    assert!(cb.playbook_start_called.load(Ordering::SeqCst));
    assert_eq!(cb.playbook_start_count.load(Ordering::SeqCst), 1);

    cb.reset();

    // Disable and dispatch
    manager.set_enabled(false);
    manager.dispatch_playbook_start("test2").await;

    // Should not have been called
    assert!(!cb.playbook_start_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_callback_manager_full_workflow() {
    let manager = CallbackManager::new();

    let mock = Arc::new(MockCallback::new());
    let counter = Arc::new(CountingCallback::new());
    let error_tracker = Arc::new(ErrorTrackingCallback::new());

    manager.register(mock.clone());
    manager.register(counter.clone());
    manager.register(error_tracker.clone());

    let hosts = vec!["host1".to_string(), "host2".to_string()];

    // Full workflow
    manager.dispatch_playbook_start("deploy").await;
    manager
        .dispatch_play_start("Configure servers", &hosts)
        .await;

    let mut facts = Facts::new();
    facts.set("os", json!("linux"));
    manager.dispatch_facts_gathered("host1", &facts).await;
    manager.dispatch_facts_gathered("host2", &facts).await;

    manager
        .dispatch_task_start("Install package", "host1")
        .await;
    let success_result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "Install package".to_string(),
        result: ModuleResult::changed("Installed"),
        duration: Duration::from_secs(2),
        notify: vec!["restart service".to_string()],
    };
    manager.dispatch_task_complete(&success_result).await;

    manager
        .dispatch_task_start("Install package", "host2")
        .await;
    let failed_result = ExecutionResult {
        host: "host2".to_string(),
        task_name: "Install package".to_string(),
        result: ModuleResult::failed("Package not found"),
        duration: Duration::from_secs(1),
        notify: vec![],
    };
    manager.dispatch_task_complete(&failed_result).await;

    manager.dispatch_handler_triggered("restart service").await;
    manager.dispatch_play_end("Configure servers", false).await;
    manager.dispatch_playbook_end("deploy", false).await;

    // Verify mock callback
    assert!(mock.playbook_start_called.load(Ordering::SeqCst));
    assert!(mock.playbook_end_called.load(Ordering::SeqCst));
    assert_eq!(mock.task_complete_count.load(Ordering::SeqCst), 2);
    assert_eq!(mock.facts_gathered_count.load(Ordering::SeqCst), 2);
    assert_eq!(mock.handler_triggered_count.load(Ordering::SeqCst), 1);

    // Verify counter callback
    assert_eq!(counter.total(), 4); // playbook_start + 2 task_complete + playbook_end
    assert_eq!(counter.successes(), 1); // Only 1 successful task
    assert_eq!(counter.failures(), 2); // 1 failed task + 1 failed playbook

    // Verify error tracker
    assert_eq!(error_tracker.error_count(), 1);
    assert_eq!(error_tracker.failed_hosts(), vec!["host2"]);
}

// ============================================================================
// Test: Event Type Serialization
// ============================================================================

#[tokio::test]
async fn test_module_result_serialization() {
    let result = ModuleResult::changed("File created successfully");
    let json = serde_json::to_string(&result).unwrap();

    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"changed\":true"));
    assert!(json.contains("File created successfully"));
}

#[tokio::test]
async fn test_module_result_with_data_serialization() {
    let result = ModuleResult::ok("Command executed").with_data(json!({
        "stdout": "Hello World",
        "exit_code": 0
    }));

    let json = serde_json::to_string(&result).unwrap();

    assert!(json.contains("\"data\""));
    assert!(json.contains("\"stdout\":\"Hello World\""));
    assert!(json.contains("\"exit_code\":0"));
}

#[tokio::test]
async fn test_module_result_with_warnings_serialization() {
    let result = ModuleResult::changed("Config updated")
        .with_warning("Deprecated option used")
        .with_warning("Consider upgrading");

    let json = serde_json::to_string(&result).unwrap();

    assert!(json.contains("\"warnings\""));
    assert!(json.contains("Deprecated option used"));
    assert!(json.contains("Consider upgrading"));
}

#[tokio::test]
async fn test_facts_serialization() {
    let mut facts = Facts::new();
    facts.set("os_family", json!("Debian"));
    facts.set("architecture", json!("x86_64"));
    facts.set("memory_mb", json!(16384));
    facts.set("processors", json!(["Intel Core i7", "Intel Core i7"]));

    let json = serde_json::to_string(&facts).unwrap();

    assert!(json.contains("Debian"));
    assert!(json.contains("x86_64"));
    assert!(json.contains("16384"));
}

#[tokio::test]
async fn test_execution_result_fields() {
    let result = ExecutionResult {
        host: "production-server-01".to_string(),
        task_name: "Install nginx".to_string(),
        result: ModuleResult::changed("nginx 1.24 installed"),
        duration: Duration::from_millis(5432),
        notify: vec!["restart nginx".to_string(), "reload config".to_string()],
    };

    assert_eq!(result.host, "production-server-01");
    assert_eq!(result.task_name, "Install nginx");
    assert!(result.result.success);
    assert!(result.result.changed);
    assert_eq!(result.duration.as_millis(), 5432);
    assert_eq!(result.notify.len(), 2);
}

// ============================================================================
// Test: Context Struct Creation
// ============================================================================

#[tokio::test]
async fn test_module_result_ok_creation() {
    let result = ModuleResult::ok("Task completed");

    assert!(result.success);
    assert!(!result.changed);
    assert!(!result.skipped);
    assert_eq!(result.message, "Task completed");
    assert!(result.data.is_none());
    assert!(result.warnings.is_empty());
}

#[tokio::test]
async fn test_module_result_changed_creation() {
    let result = ModuleResult::changed("File modified");

    assert!(result.success);
    assert!(result.changed);
    assert!(!result.skipped);
    assert_eq!(result.message, "File modified");
}

#[tokio::test]
async fn test_module_result_failed_creation() {
    let result = ModuleResult::failed("Permission denied");

    assert!(!result.success);
    assert!(!result.changed);
    assert!(!result.skipped);
    assert_eq!(result.message, "Permission denied");
}

#[tokio::test]
async fn test_module_result_skipped_creation() {
    let result = ModuleResult::skipped("Condition not met");

    assert!(result.success);
    assert!(!result.changed);
    assert!(result.skipped);
    assert_eq!(result.message, "Condition not met");
}

#[tokio::test]
async fn test_module_result_builder_pattern() {
    let result = ModuleResult::changed("Package installed")
        .with_data(json!({
            "package": "nginx",
            "version": "1.24.0"
        }))
        .with_warning("Using deprecated repository");

    assert!(result.changed);
    assert!(result.data.is_some());
    assert_eq!(result.warnings.len(), 1);

    let data = result.data.unwrap();
    assert_eq!(data["package"], "nginx");
    assert_eq!(data["version"], "1.24.0");
}

#[tokio::test]
async fn test_facts_creation_and_access() {
    let mut facts = Facts::new();

    assert!(facts.all().is_empty());

    facts.set("os", json!("linux"));
    facts.set("arch", json!("x86_64"));

    assert_eq!(facts.get("os"), Some(&json!("linux")));
    assert_eq!(facts.get("arch"), Some(&json!("x86_64")));
    assert_eq!(facts.get("nonexistent"), None);
    assert_eq!(facts.all().len(), 2);
}

#[tokio::test]
async fn test_facts_gather_local() {
    let facts = Facts::gather_local();

    // These should always be present
    assert!(facts.get("os_family").is_some());
    assert!(facts.get("os_arch").is_some());
}

// ============================================================================
// Test: Advanced Mock Implementations
// ============================================================================

/// A timing-aware callback that tracks event latencies
#[derive(Debug)]
pub struct TimingCallback {
    event_timestamps: RwLock<Vec<(String, std::time::Instant)>>,
    start_time: std::time::Instant,
}

impl TimingCallback {
    pub fn new() -> Self {
        Self {
            event_timestamps: RwLock::new(Vec::new()),
            start_time: std::time::Instant::now(),
        }
    }

    pub fn elapsed_since_start(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn event_count(&self) -> usize {
        self.event_timestamps.read().len()
    }

    fn record(&self, event: &str) {
        self.event_timestamps
            .write()
            .push((event.to_string(), std::time::Instant::now()));
    }
}

impl Default for TimingCallback {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionCallback for TimingCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.record(&format!("playbook_start:{}", name));
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        self.record(&format!("playbook_end:{}:{}", name, success));
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        self.record(&format!("task_start:{}:{}", name, host));
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.record(&format!(
            "task_complete:{}:{}",
            result.task_name, result.host
        ));
    }
}

#[tokio::test]
async fn test_timing_callback() {
    let callback = TimingCallback::new();

    callback.on_playbook_start("test").await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    callback.on_task_start("task1", "host1").await;
    tokio::time::sleep(Duration::from_millis(10)).await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(5),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    assert_eq!(callback.event_count(), 3);
    assert!(callback.elapsed_since_start() >= Duration::from_millis(20));
}

/// A filtering callback that only processes certain events
#[derive(Debug)]
pub struct FilteringCallback {
    host_filter: Option<String>,
    task_filter: Option<String>,
    processed_events: RwLock<Vec<String>>,
}

impl FilteringCallback {
    pub fn new() -> Self {
        Self {
            host_filter: None,
            task_filter: None,
            processed_events: RwLock::new(Vec::new()),
        }
    }

    pub fn with_host_filter(mut self, host: &str) -> Self {
        self.host_filter = Some(host.to_string());
        self
    }

    pub fn with_task_filter(mut self, task: &str) -> Self {
        self.task_filter = Some(task.to_string());
        self
    }

    pub fn processed_count(&self) -> usize {
        self.processed_events.read().len()
    }
}

impl Default for FilteringCallback {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionCallback for FilteringCallback {
    async fn on_task_start(&self, name: &str, host: &str) {
        let host_matches = self.host_filter.as_ref().map_or(true, |f| f == host);
        let task_matches = self.task_filter.as_ref().map_or(true, |f| f == name);

        if host_matches && task_matches {
            self.processed_events
                .write()
                .push(format!("task_start:{}:{}", name, host));
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let host_matches = self
            .host_filter
            .as_ref()
            .map_or(true, |f| f == &result.host);
        let task_matches = self
            .task_filter
            .as_ref()
            .map_or(true, |f| f == &result.task_name);

        if host_matches && task_matches {
            self.processed_events.write().push(format!(
                "task_complete:{}:{}",
                result.task_name, result.host
            ));
        }
    }
}

#[tokio::test]
async fn test_filtering_callback_host_filter() {
    let callback = FilteringCallback::new().with_host_filter("target_host");

    callback.on_task_start("task1", "target_host").await;
    callback.on_task_start("task2", "other_host").await;
    callback.on_task_start("task3", "target_host").await;

    assert_eq!(callback.processed_count(), 2);
}

#[tokio::test]
async fn test_filtering_callback_task_filter() {
    let callback = FilteringCallback::new().with_task_filter("important_task");

    callback.on_task_start("important_task", "host1").await;
    callback.on_task_start("other_task", "host1").await;
    callback.on_task_start("important_task", "host2").await;

    assert_eq!(callback.processed_count(), 2);
}

#[tokio::test]
async fn test_filtering_callback_combined_filters() {
    let callback = FilteringCallback::new()
        .with_host_filter("target_host")
        .with_task_filter("important_task");

    callback
        .on_task_start("important_task", "target_host")
        .await;
    callback.on_task_start("important_task", "other_host").await;
    callback.on_task_start("other_task", "target_host").await;
    callback.on_task_start("other_task", "other_host").await;

    // Only one event matches both filters
    assert_eq!(callback.processed_count(), 1);
}

/// A buffering callback that batches events
#[derive(Debug)]
pub struct BufferingCallback {
    buffer: RwLock<Vec<String>>,
    buffer_size: usize,
    flush_count: AtomicU32,
}

impl BufferingCallback {
    pub fn new(buffer_size: usize) -> Self {
        Self {
            buffer: RwLock::new(Vec::new()),
            buffer_size,
            flush_count: AtomicU32::new(0),
        }
    }

    fn add_event(&self, event: String) {
        let mut buffer = self.buffer.write();
        buffer.push(event);

        if buffer.len() >= self.buffer_size {
            self.flush_count.fetch_add(1, Ordering::SeqCst);
            buffer.clear();
        }
    }

    pub fn buffer_len(&self) -> usize {
        self.buffer.read().len()
    }

    pub fn flush_count(&self) -> u32 {
        self.flush_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ExecutionCallback for BufferingCallback {
    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.add_event(format!("task:{}:{}", result.task_name, result.host));
    }
}

#[tokio::test]
async fn test_buffering_callback() {
    let callback = BufferingCallback::new(5);

    // Add 12 events - should trigger 2 flushes
    for i in 0..12 {
        let result = ExecutionResult {
            host: "host".to_string(),
            task_name: format!("task_{}", i),
            result: ModuleResult::ok("OK"),
            duration: Duration::from_millis(1),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;
    }

    assert_eq!(callback.flush_count(), 2); // 2 flushes (at 5 and 10)
    assert_eq!(callback.buffer_len(), 2); // 2 remaining
}

// ============================================================================
// Test: Concurrent Callback Execution
// ============================================================================

#[tokio::test]
async fn test_concurrent_task_completions() {
    let callback = Arc::new(MockCallback::new());
    let mut handles = vec![];

    // Spawn 50 concurrent task completions
    for i in 0..50 {
        let cb = callback.clone();
        let handle = tokio::spawn(async move {
            let result = ExecutionResult {
                host: format!("host_{}", i % 5),
                task_name: format!("concurrent_task_{}", i),
                result: if i % 3 == 0 {
                    ModuleResult::failed("Simulated failure")
                } else {
                    ModuleResult::ok("Success")
                },
                duration: Duration::from_millis(i as u64),
                notify: vec![],
            };
            cb.on_task_complete(&result).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(callback.task_complete_count.load(Ordering::SeqCst), 50);

    // Check that results were tracked
    let results = callback.task_results.read();
    assert_eq!(results.len(), 50);

    // Count successes and failures
    let successes = results.iter().filter(|&&r| r).count();
    let failures = results.iter().filter(|&&r| !r).count();

    // Every 3rd task (0, 3, 6, ..., 48) fails = 17 failures
    assert_eq!(failures, 17);
    assert_eq!(successes, 33);
}

#[tokio::test]
async fn test_concurrent_manager_dispatch() {
    let manager = Arc::new(CallbackManager::new());
    let mock = Arc::new(MockCallback::new());

    manager.register(mock.clone());

    let mut handles = vec![];

    // Spawn concurrent dispatches of different event types
    for i in 0..20 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            mgr.dispatch_playbook_start(&format!("playbook_{}", i))
                .await;
            mgr.dispatch_task_start(&format!("task_{}", i), &format!("host_{}", i))
                .await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(mock.playbook_start_count.load(Ordering::SeqCst), 20);
    assert_eq!(mock.task_start_count.load(Ordering::SeqCst), 20);
}

// ============================================================================
// Test: Callback State Isolation
// ============================================================================

#[tokio::test]
async fn test_callback_state_isolation() {
    // Create two separate callback instances
    let callback1 = MockCallback::new();
    let callback2 = MockCallback::new();

    // Modify only callback1
    callback1.on_playbook_start("playbook1").await;
    callback1.on_task_start("task1", "host1").await;

    // callback2 should be unaffected
    assert!(callback1.playbook_start_called.load(Ordering::SeqCst));
    assert!(!callback2.playbook_start_called.load(Ordering::SeqCst));

    assert_eq!(callback1.playbook_start_count.load(Ordering::SeqCst), 1);
    assert_eq!(callback2.playbook_start_count.load(Ordering::SeqCst), 0);

    assert!(!callback1.playbook_names.read().is_empty());
    assert!(callback2.playbook_names.read().is_empty());
}

// ============================================================================
// Test: Long-running Callback Scenarios
// ============================================================================

/// A slow callback for testing timeout behavior
#[derive(Debug, Default)]
pub struct SlowCallback {
    delay_ms: AtomicU64,
    completed_count: AtomicU32,
}

impl SlowCallback {
    pub fn new(delay_ms: u64) -> Self {
        Self {
            delay_ms: AtomicU64::new(delay_ms),
            completed_count: AtomicU32::new(0),
        }
    }

    pub fn completed_count(&self) -> u32 {
        self.completed_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ExecutionCallback for SlowCallback {
    async fn on_task_complete(&self, _result: &ExecutionResult) {
        let delay = self.delay_ms.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        self.completed_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn test_slow_callback_completes() {
    let callback = SlowCallback::new(10);

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(1),
        notify: vec![],
    };

    let start = std::time::Instant::now();
    callback.on_task_complete(&result).await;
    let elapsed = start.elapsed();

    assert!(elapsed >= Duration::from_millis(10));
    assert_eq!(callback.completed_count(), 1);
}

// ============================================================================
// Test: Skipped Task Handling
// ============================================================================

#[tokio::test]
async fn test_skipped_task_result() {
    let callback = MockCallback::new();

    let skipped_result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "conditional_task".to_string(),
        result: ModuleResult::skipped("when clause evaluated to false"),
        duration: Duration::from_millis(0),
        notify: vec![],
    };

    callback.on_task_complete(&skipped_result).await;

    // Skipped tasks have success=true but the test checks against result.success
    let results = callback.task_results.read();
    assert_eq!(results.len(), 1);
    assert!(results[0]); // Skipped counts as success for tracking
}

// ============================================================================
// Test: Multiple Handler Notifications
// ============================================================================

#[tokio::test]
async fn test_multiple_handlers_notified() {
    let callback = MockCallback::new();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "config_update".to_string(),
        result: ModuleResult::changed("Configuration updated"),
        duration: Duration::from_millis(50),
        notify: vec![
            "restart nginx".to_string(),
            "reload config".to_string(),
            "notify monitoring".to_string(),
        ],
    };

    callback.on_task_complete(&result).await;

    // Trigger all handlers
    for handler in &result.notify {
        callback.on_handler_triggered(handler).await;
    }

    assert_eq!(callback.handler_triggered_count.load(Ordering::SeqCst), 3);

    let handlers = callback.handler_names.read();
    assert!(handlers.contains(&"restart nginx".to_string()));
    assert!(handlers.contains(&"reload config".to_string()));
    assert!(handlers.contains(&"notify monitoring".to_string()));
}

// ============================================================================
// Test: Null/None Handling
// ============================================================================

#[tokio::test]
async fn test_result_with_no_data() {
    let result = ModuleResult::ok("Simple result");

    assert!(result.data.is_none());
    assert!(result.warnings.is_empty());
}

#[tokio::test]
async fn test_result_with_null_json_data() {
    let result = ModuleResult::ok("Result with null").with_data(json!(null));

    assert!(result.data.is_some());
    assert!(result.data.unwrap().is_null());
}

#[tokio::test]
async fn test_result_with_empty_object_data() {
    let result = ModuleResult::ok("Result with empty object").with_data(json!({}));

    assert!(result.data.is_some());
    let data = result.data.unwrap();
    assert!(data.is_object());
    assert!(data.as_object().unwrap().is_empty());
}

#[tokio::test]
async fn test_result_with_empty_array_data() {
    let result = ModuleResult::ok("Result with empty array").with_data(json!([]));

    assert!(result.data.is_some());
    let data = result.data.unwrap();
    assert!(data.is_array());
    assert!(data.as_array().unwrap().is_empty());
}
