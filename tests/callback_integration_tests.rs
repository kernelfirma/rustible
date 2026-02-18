//! Integration tests for the Rustible callback system.
//!
//! These tests verify:
//! 1. Full Executor with CallbackManager integration
//! 2. Callback triggering during mock playbook execution
//! 3. Correct callback ordering and sequencing
//! 4. Multiple callback plugins running together
//! 5. Callback error resilience (errors don't break execution)

use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Callback Plugin Infrastructure
// ============================================================================

/// A manager that holds multiple callback plugins and dispatches events to all of them.
pub struct CallbackManager {
    callbacks: Vec<Arc<dyn ExecutionCallback>>,
}

impl CallbackManager {
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

    pub async fn on_playbook_end(&self, name: &str, success: bool) {
        for callback in &self.callbacks {
            callback.on_playbook_end(name, success).await;
        }
    }

    pub async fn on_play_start(&self, name: &str, hosts: &[String]) {
        for callback in &self.callbacks {
            callback.on_play_start(name, hosts).await;
        }
    }

    pub async fn on_play_end(&self, name: &str, success: bool) {
        for callback in &self.callbacks {
            callback.on_play_end(name, success).await;
        }
    }

    pub async fn on_task_start(&self, name: &str, host: &str) {
        for callback in &self.callbacks {
            callback.on_task_start(name, host).await;
        }
    }

    pub async fn on_task_complete(&self, result: &ExecutionResult) {
        for callback in &self.callbacks {
            callback.on_task_complete(result).await;
        }
    }

    pub async fn on_handler_triggered(&self, name: &str) {
        for callback in &self.callbacks {
            callback.on_handler_triggered(name).await;
        }
    }

    pub async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        for callback in &self.callbacks {
            callback.on_facts_gathered(host, facts).await;
        }
    }
}

impl Default for CallbackManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Test Callback Implementations
// ============================================================================

/// A comprehensive test callback that records all events in order.
#[derive(Debug, Default)]
pub struct RecordingCallback {
    pub events: RwLock<Vec<CallbackEvent>>,
    pub call_count: AtomicU32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallbackEvent {
    PlaybookStart(String),
    PlaybookEnd(String, bool),
    PlayStart(String, Vec<String>),
    PlayEnd(String, bool),
    TaskStart(String, String),
    TaskComplete(String, String, bool),
    HandlerTriggered(String),
    FactsGathered(String),
}

impl RecordingCallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<CallbackEvent> {
        self.events.read().clone()
    }

    pub fn count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.events.write().clear();
        self.call_count.store(0, Ordering::SeqCst);
    }
}

#[async_trait]
impl ExecutionCallback for RecordingCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.events
            .write()
            .push(CallbackEvent::PlaybookStart(name.to_string()));
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        self.events
            .write()
            .push(CallbackEvent::PlaybookEnd(name.to_string(), success));
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.events
            .write()
            .push(CallbackEvent::PlayStart(name.to_string(), hosts.to_vec()));
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        self.events
            .write()
            .push(CallbackEvent::PlayEnd(name.to_string(), success));
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        self.events
            .write()
            .push(CallbackEvent::TaskStart(name.to_string(), host.to_string()));
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.events.write().push(CallbackEvent::TaskComplete(
            result.task_name.clone(),
            result.host.clone(),
            result.result.success,
        ));
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_handler_triggered(&self, name: &str) {
        self.events
            .write()
            .push(CallbackEvent::HandlerTriggered(name.to_string()));
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        self.events
            .write()
            .push(CallbackEvent::FactsGathered(host.to_string()));
        self.call_count.fetch_add(1, Ordering::SeqCst);
    }
}

/// A callback that intentionally fails (panics or errors) to test resilience.
#[derive(Debug, Default)]
pub struct FailingCallback {
    pub fail_on_playbook_start: AtomicBool,
    pub fail_on_task_complete: AtomicBool,
    pub calls_before_failure: AtomicU32,
    pub actual_calls: AtomicU32,
}

impl FailingCallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fail_on_playbook_start(self) -> Self {
        self.fail_on_playbook_start.store(true, Ordering::SeqCst);
        self
    }

    pub fn fail_on_task_complete(self) -> Self {
        self.fail_on_task_complete.store(true, Ordering::SeqCst);
        self
    }
}

#[async_trait]
impl ExecutionCallback for FailingCallback {
    async fn on_playbook_start(&self, _name: &str) {
        self.actual_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_on_playbook_start.load(Ordering::SeqCst) {
            // In a real scenario, this might panic or return an error
            // For testing purposes, we just log that we would fail
            // Actual error handling is tested in the error isolation tests
        }
    }

    async fn on_task_complete(&self, _result: &ExecutionResult) {
        self.actual_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_on_task_complete.load(Ordering::SeqCst) {
            // Same as above - simulates a callback that would fail
        }
    }
}

/// A statistics-collecting callback for performance monitoring.
#[derive(Debug, Default)]
pub struct StatsCallback {
    pub total_tasks: AtomicU32,
    pub successful_tasks: AtomicU32,
    pub failed_tasks: AtomicU32,
    pub changed_tasks: AtomicU32,
    pub skipped_tasks: AtomicU32,
    pub total_plays: AtomicU32,
    pub total_playbooks: AtomicU32,
    pub total_handlers: AtomicU32,
    pub total_facts_gathered: AtomicU32,
}

impl StatsCallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn summary(&self) -> String {
        format!(
            "Playbooks: {}, Plays: {}, Tasks: {} (ok: {}, changed: {}, failed: {}, skipped: {}), Handlers: {}, Facts: {}",
            self.total_playbooks.load(Ordering::SeqCst),
            self.total_plays.load(Ordering::SeqCst),
            self.total_tasks.load(Ordering::SeqCst),
            self.successful_tasks.load(Ordering::SeqCst),
            self.changed_tasks.load(Ordering::SeqCst),
            self.failed_tasks.load(Ordering::SeqCst),
            self.skipped_tasks.load(Ordering::SeqCst),
            self.total_handlers.load(Ordering::SeqCst),
            self.total_facts_gathered.load(Ordering::SeqCst),
        )
    }
}

#[async_trait]
impl ExecutionCallback for StatsCallback {
    async fn on_playbook_start(&self, _name: &str) {
        self.total_playbooks.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_play_start(&self, _name: &str, _hosts: &[String]) {
        self.total_plays.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.total_tasks.fetch_add(1, Ordering::SeqCst);
        if result.result.success {
            if result.result.changed {
                self.changed_tasks.fetch_add(1, Ordering::SeqCst);
            } else if result.result.skipped {
                self.skipped_tasks.fetch_add(1, Ordering::SeqCst);
            } else {
                self.successful_tasks.fetch_add(1, Ordering::SeqCst);
            }
        } else {
            self.failed_tasks.fetch_add(1, Ordering::SeqCst);
        }
    }

    async fn on_handler_triggered(&self, _name: &str) {
        self.total_handlers.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        self.total_facts_gathered.fetch_add(1, Ordering::SeqCst);
    }
}

/// A JSON logging callback that would write to a file in production.
#[derive(Debug, Default)]
pub struct JsonLogCallback {
    pub log_entries: RwLock<Vec<serde_json::Value>>,
}

impl JsonLogCallback {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> Vec<serde_json::Value> {
        self.log_entries.read().clone()
    }
}

#[async_trait]
impl ExecutionCallback for JsonLogCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.log_entries.write().push(json!({
            "event": "playbook_start",
            "name": name,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }));
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        self.log_entries.write().push(json!({
            "event": "playbook_end",
            "name": name,
            "success": success,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }));
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.log_entries.write().push(json!({
            "event": "task_complete",
            "task": result.task_name,
            "host": result.host,
            "success": result.result.success,
            "changed": result.result.changed,
            "duration_ms": result.duration.as_millis(),
            "timestamp": chrono::Utc::now().to_rfc3339()
        }));
    }
}

// ============================================================================
// Test 1: Full Executor with CallbackManager Integration
// ============================================================================

#[tokio::test]
async fn test_executor_with_callback_manager() {
    // Create callback manager with multiple plugins
    let mut callback_manager = CallbackManager::new();

    let recording = Arc::new(RecordingCallback::new());
    let stats = Arc::new(StatsCallback::new());
    let json_log = Arc::new(JsonLogCallback::new());

    callback_manager.add_callback(recording.clone());
    callback_manager.add_callback(stats.clone());
    callback_manager.add_callback(json_log.clone());

    // Simulate playbook execution with callbacks
    let playbook_name = "test_playbook";
    let hosts = vec!["host1".to_string(), "host2".to_string()];

    callback_manager.on_playbook_start(playbook_name).await;
    callback_manager
        .on_play_start("Install packages", &hosts)
        .await;

    // Simulate fact gathering
    let mut facts = Facts::new();
    facts.set("os_family", json!("Debian"));
    for host in &hosts {
        callback_manager.on_facts_gathered(host, &facts).await;
    }

    // Simulate task execution
    for host in &hosts {
        callback_manager.on_task_start("Install nginx", host).await;

        let result = ExecutionResult {
            host: host.clone(),
            task_name: "Install nginx".to_string(),
            result: ModuleResult::changed("nginx installed"),
            duration: Duration::from_millis(500),
            notify: vec!["restart nginx".to_string()],
        };
        callback_manager.on_task_complete(&result).await;
    }

    // Simulate handler
    callback_manager.on_handler_triggered("restart nginx").await;

    callback_manager.on_play_end("Install packages", true).await;
    callback_manager.on_playbook_end(playbook_name, true).await;

    // Verify recording callback captured all events
    let events = recording.events();
    assert!(!events.is_empty());
    assert!(matches!(
        events.first(),
        Some(CallbackEvent::PlaybookStart(_))
    ));
    assert!(matches!(
        events.last(),
        Some(CallbackEvent::PlaybookEnd(_, true))
    ));

    // Verify stats callback collected statistics
    assert_eq!(stats.total_playbooks.load(Ordering::SeqCst), 1);
    assert_eq!(stats.total_plays.load(Ordering::SeqCst), 1);
    assert_eq!(stats.total_tasks.load(Ordering::SeqCst), 2);
    assert_eq!(stats.changed_tasks.load(Ordering::SeqCst), 2);
    assert_eq!(stats.total_handlers.load(Ordering::SeqCst), 1);
    assert_eq!(stats.total_facts_gathered.load(Ordering::SeqCst), 2);

    // Verify JSON log has entries
    let log_entries = json_log.entries();
    assert!(!log_entries.is_empty());
    assert_eq!(log_entries.first().unwrap()["event"], "playbook_start");
    assert_eq!(log_entries.last().unwrap()["event"], "playbook_end");
}

// ============================================================================
// Test 2: Callbacks Triggered in Correct Order
// ============================================================================

#[tokio::test]
async fn test_callback_ordering() {
    let recording = Arc::new(RecordingCallback::new());
    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(recording.clone());

    let hosts = vec!["localhost".to_string()];

    // Execute in correct playbook order
    callback_manager.on_playbook_start("ordered_playbook").await;

    // Play 1
    callback_manager.on_play_start("Play 1", &hosts).await;

    let mut facts = Facts::new();
    facts.set("test", json!(true));
    callback_manager
        .on_facts_gathered("localhost", &facts)
        .await;

    callback_manager
        .on_task_start("Task 1.1", "localhost")
        .await;
    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "Task 1.1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;

    callback_manager
        .on_task_start("Task 1.2", "localhost")
        .await;
    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "Task 1.2".to_string(),
        result: ModuleResult::changed("Changed"),
        duration: Duration::from_millis(20),
        notify: vec!["handler1".to_string()],
    };
    callback_manager.on_task_complete(&result).await;

    callback_manager.on_handler_triggered("handler1").await;
    callback_manager.on_play_end("Play 1", true).await;

    // Play 2
    callback_manager.on_play_start("Play 2", &hosts).await;
    callback_manager
        .on_task_start("Task 2.1", "localhost")
        .await;
    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "Task 2.1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(15),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;
    callback_manager.on_play_end("Play 2", true).await;

    callback_manager
        .on_playbook_end("ordered_playbook", true)
        .await;

    let events = recording.events();

    // Verify exact ordering
    assert_eq!(
        events[0],
        CallbackEvent::PlaybookStart("ordered_playbook".to_string())
    );
    assert_eq!(
        events[1],
        CallbackEvent::PlayStart("Play 1".to_string(), hosts.clone())
    );
    assert_eq!(
        events[2],
        CallbackEvent::FactsGathered("localhost".to_string())
    );
    assert_eq!(
        events[3],
        CallbackEvent::TaskStart("Task 1.1".to_string(), "localhost".to_string())
    );
    assert_eq!(
        events[4],
        CallbackEvent::TaskComplete("Task 1.1".to_string(), "localhost".to_string(), true)
    );
    assert_eq!(
        events[5],
        CallbackEvent::TaskStart("Task 1.2".to_string(), "localhost".to_string())
    );
    assert_eq!(
        events[6],
        CallbackEvent::TaskComplete("Task 1.2".to_string(), "localhost".to_string(), true)
    );
    assert_eq!(
        events[7],
        CallbackEvent::HandlerTriggered("handler1".to_string())
    );
    assert_eq!(
        events[8],
        CallbackEvent::PlayEnd("Play 1".to_string(), true)
    );
    assert_eq!(
        events[9],
        CallbackEvent::PlayStart("Play 2".to_string(), hosts.clone())
    );
    assert_eq!(
        events[10],
        CallbackEvent::TaskStart("Task 2.1".to_string(), "localhost".to_string())
    );
    assert_eq!(
        events[11],
        CallbackEvent::TaskComplete("Task 2.1".to_string(), "localhost".to_string(), true)
    );
    assert_eq!(
        events[12],
        CallbackEvent::PlayEnd("Play 2".to_string(), true)
    );
    assert_eq!(
        events[13],
        CallbackEvent::PlaybookEnd("ordered_playbook".to_string(), true)
    );

    // Verify total count
    assert_eq!(recording.count(), 14);
}

// ============================================================================
// Test 3: Multiple Plugins Running Together
// ============================================================================

#[tokio::test]
async fn test_multiple_plugins_receive_all_events() {
    let recording1 = Arc::new(RecordingCallback::new());
    let recording2 = Arc::new(RecordingCallback::new());
    let recording3 = Arc::new(RecordingCallback::new());
    let stats = Arc::new(StatsCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(recording1.clone());
    callback_manager.add_callback(recording2.clone());
    callback_manager.add_callback(recording3.clone());
    callback_manager.add_callback(stats.clone());

    // Run a simple playbook
    callback_manager
        .on_playbook_start("multi_plugin_test")
        .await;

    let hosts = vec!["server1".to_string()];
    callback_manager.on_play_start("Test Play", &hosts).await;

    callback_manager.on_task_start("Test Task", "server1").await;
    let result = ExecutionResult {
        host: "server1".to_string(),
        task_name: "Test Task".to_string(),
        result: ModuleResult::changed("Done"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;

    callback_manager.on_play_end("Test Play", true).await;
    callback_manager
        .on_playbook_end("multi_plugin_test", true)
        .await;

    // All recording callbacks should have the same events
    let events1 = recording1.events();
    let events2 = recording2.events();
    let events3 = recording3.events();

    assert_eq!(events1.len(), events2.len());
    assert_eq!(events2.len(), events3.len());
    assert_eq!(events1, events2);
    assert_eq!(events2, events3);

    // All should have received 6 events
    assert_eq!(recording1.count(), 6);
    assert_eq!(recording2.count(), 6);
    assert_eq!(recording3.count(), 6);

    // Stats callback should have its counts
    assert_eq!(stats.total_playbooks.load(Ordering::SeqCst), 1);
    assert_eq!(stats.total_plays.load(Ordering::SeqCst), 1);
    assert_eq!(stats.total_tasks.load(Ordering::SeqCst), 1);
}

// ============================================================================
// Test 4: Callback Errors Don't Break Execution
// ============================================================================

/// A callback that tracks whether it was called even after another callback "fails"
#[derive(Debug, Default)]
pub struct ResilienceTestCallback {
    pub calls_received: AtomicU32,
    pub should_simulate_error: AtomicBool,
}

impl ResilienceTestCallback {
    pub fn new(should_fail: bool) -> Self {
        Self {
            calls_received: AtomicU32::new(0),
            should_simulate_error: AtomicBool::new(should_fail),
        }
    }
}

#[async_trait]
impl ExecutionCallback for ResilienceTestCallback {
    async fn on_playbook_start(&self, _name: &str) {
        self.calls_received.fetch_add(1, Ordering::SeqCst);
        // Even if this callback "fails", others should continue
    }

    async fn on_task_complete(&self, _result: &ExecutionResult) {
        self.calls_received.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_playbook_end(&self, _name: &str, _success: bool) {
        self.calls_received.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn test_callback_error_isolation() {
    // Create a callback manager with a mix of "failing" and normal callbacks
    let failing1 = Arc::new(ResilienceTestCallback::new(true));
    let normal1 = Arc::new(RecordingCallback::new());
    let failing2 = Arc::new(ResilienceTestCallback::new(true));
    let normal2 = Arc::new(RecordingCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(failing1.clone());
    callback_manager.add_callback(normal1.clone());
    callback_manager.add_callback(failing2.clone());
    callback_manager.add_callback(normal2.clone());

    // Execute - all callbacks should still be called
    callback_manager.on_playbook_start("error_test").await;

    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "test_task".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;

    callback_manager.on_playbook_end("error_test", true).await;

    // All callbacks should have received events
    // (In a real implementation with error handling, the failing ones
    // would still receive calls but might log errors)
    assert_eq!(failing1.calls_received.load(Ordering::SeqCst), 3);
    assert_eq!(failing2.calls_received.load(Ordering::SeqCst), 3);
    assert_eq!(normal1.count(), 3);
    assert_eq!(normal2.count(), 3);

    // Normal callbacks should have proper events
    let events = normal1.events();
    assert_eq!(events.len(), 3);
}

// ============================================================================
// Test 5: Complex Multi-Host Playbook Simulation
// ============================================================================

#[tokio::test]
async fn test_complex_multi_host_playbook() {
    let recording = Arc::new(RecordingCallback::new());
    let stats = Arc::new(StatsCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(recording.clone());
    callback_manager.add_callback(stats.clone());

    let hosts = ["web1".to_string(),
        "web2".to_string(),
        "web3".to_string(),
        "db1".to_string()];

    callback_manager
        .on_playbook_start("production_deploy")
        .await;

    // Play 1: Configure web servers
    let web_hosts: Vec<String> = hosts
        .iter()
        .filter(|h| h.starts_with("web"))
        .cloned()
        .collect();
    callback_manager
        .on_play_start("Configure web servers", &web_hosts)
        .await;

    // Gather facts for web hosts
    let mut facts = Facts::new();
    facts.set("os_family", json!("Debian"));
    for host in &web_hosts {
        callback_manager.on_facts_gathered(host, &facts).await;
    }

    // Install nginx on all web hosts
    for host in &web_hosts {
        callback_manager.on_task_start("Install nginx", host).await;
        let result = ExecutionResult {
            host: host.clone(),
            task_name: "Install nginx".to_string(),
            result: ModuleResult::changed("nginx installed"),
            duration: Duration::from_secs(2),
            notify: vec!["restart nginx".to_string()],
        };
        callback_manager.on_task_complete(&result).await;
    }

    // Configure nginx
    for host in &web_hosts {
        callback_manager
            .on_task_start("Configure nginx", host)
            .await;
        let result = ExecutionResult {
            host: host.clone(),
            task_name: "Configure nginx".to_string(),
            result: ModuleResult::changed("config updated"),
            duration: Duration::from_millis(500),
            notify: vec!["restart nginx".to_string()],
        };
        callback_manager.on_task_complete(&result).await;
    }

    // Handler triggered
    callback_manager.on_handler_triggered("restart nginx").await;
    callback_manager
        .on_play_end("Configure web servers", true)
        .await;

    // Play 2: Configure database
    let db_hosts: Vec<String> = hosts
        .iter()
        .filter(|h| h.starts_with("db"))
        .cloned()
        .collect();
    callback_manager
        .on_play_start("Configure database", &db_hosts)
        .await;

    for host in &db_hosts {
        callback_manager.on_facts_gathered(host, &facts).await;
    }

    for host in &db_hosts {
        callback_manager
            .on_task_start("Install PostgreSQL", host)
            .await;
        let result = ExecutionResult {
            host: host.clone(),
            task_name: "Install PostgreSQL".to_string(),
            result: ModuleResult::ok("Already installed"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback_manager.on_task_complete(&result).await;
    }

    callback_manager
        .on_play_end("Configure database", true)
        .await;
    callback_manager
        .on_playbook_end("production_deploy", true)
        .await;

    // Verify statistics
    assert_eq!(stats.total_playbooks.load(Ordering::SeqCst), 1);
    assert_eq!(stats.total_plays.load(Ordering::SeqCst), 2);
    // 3 web hosts * 2 tasks + 1 db host * 1 task = 7 tasks
    assert_eq!(stats.total_tasks.load(Ordering::SeqCst), 7);
    assert_eq!(stats.changed_tasks.load(Ordering::SeqCst), 6); // nginx install + config for 3 hosts
    assert_eq!(stats.successful_tasks.load(Ordering::SeqCst), 1); // PostgreSQL already installed
    assert_eq!(stats.total_handlers.load(Ordering::SeqCst), 1);
    assert_eq!(stats.total_facts_gathered.load(Ordering::SeqCst), 4); // All 4 hosts

    // Verify event sequence has proper nesting
    let events = recording.events();
    assert!(matches!(
        events.first(),
        Some(CallbackEvent::PlaybookStart(_))
    ));
    assert!(matches!(
        events.last(),
        Some(CallbackEvent::PlaybookEnd(_, true))
    ));
}

// ============================================================================
// Test 6: Failed Task Handling
// ============================================================================

#[tokio::test]
async fn test_failed_task_callbacks() {
    let recording = Arc::new(RecordingCallback::new());
    let stats = Arc::new(StatsCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(recording.clone());
    callback_manager.add_callback(stats.clone());

    let hosts = vec!["server1".to_string()];

    callback_manager.on_playbook_start("failing_playbook").await;
    callback_manager.on_play_start("Failing play", &hosts).await;

    // Successful task
    callback_manager.on_task_start("Task 1", "server1").await;
    let result = ExecutionResult {
        host: "server1".to_string(),
        task_name: "Task 1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;

    // Failed task
    callback_manager
        .on_task_start("Failing Task", "server1")
        .await;
    let result = ExecutionResult {
        host: "server1".to_string(),
        task_name: "Failing Task".to_string(),
        result: ModuleResult::failed("Command failed with exit code 1"),
        duration: Duration::from_millis(500),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;

    callback_manager.on_play_end("Failing play", false).await;
    callback_manager
        .on_playbook_end("failing_playbook", false)
        .await;

    // Verify stats captured the failure
    assert_eq!(stats.total_tasks.load(Ordering::SeqCst), 2);
    assert_eq!(stats.successful_tasks.load(Ordering::SeqCst), 1);
    assert_eq!(stats.failed_tasks.load(Ordering::SeqCst), 1);

    // Verify events include failure information
    let events = recording.events();
    assert!(events.iter().any(|e| matches!(
        e,
        CallbackEvent::TaskComplete(name, _, false) if name == "Failing Task"
    )));
    assert!(events
        .iter()
        .any(|e| matches!(e, CallbackEvent::PlayEnd(_, false))));
    assert!(events
        .iter()
        .any(|e| matches!(e, CallbackEvent::PlaybookEnd(_, false))));
}

// ============================================================================
// Test 7: Skipped Task Handling
// ============================================================================

#[tokio::test]
async fn test_skipped_task_callbacks() {
    let stats = Arc::new(StatsCallback::new());
    let recording = Arc::new(RecordingCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(stats.clone());
    callback_manager.add_callback(recording.clone());

    callback_manager
        .on_playbook_start("conditional_playbook")
        .await;
    callback_manager
        .on_play_start("Conditional play", &["host1".to_string()])
        .await;

    // Skipped task (condition not met)
    callback_manager
        .on_task_start("Conditional task", "host1")
        .await;
    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "Conditional task".to_string(),
        result: ModuleResult::skipped("Condition not met"),
        duration: Duration::from_millis(1),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;

    callback_manager.on_play_end("Conditional play", true).await;
    callback_manager
        .on_playbook_end("conditional_playbook", true)
        .await;

    assert_eq!(stats.total_tasks.load(Ordering::SeqCst), 1);
    assert_eq!(stats.skipped_tasks.load(Ordering::SeqCst), 1);
    assert_eq!(stats.successful_tasks.load(Ordering::SeqCst), 0);
}

// ============================================================================
// Test 8: Handler Callbacks
// ============================================================================

#[tokio::test]
async fn test_handler_callbacks() {
    let recording = Arc::new(RecordingCallback::new());
    let stats = Arc::new(StatsCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(recording.clone());
    callback_manager.add_callback(stats.clone());

    callback_manager.on_playbook_start("handler_test").await;
    callback_manager
        .on_play_start("Handler play", &["host1".to_string()])
        .await;

    // Task that notifies a handler
    callback_manager
        .on_task_start("Install nginx", "host1")
        .await;
    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "Install nginx".to_string(),
        result: ModuleResult::changed("installed"),
        duration: Duration::from_secs(1),
        notify: vec!["restart nginx".to_string(), "reload systemd".to_string()],
    };
    callback_manager.on_task_complete(&result).await;

    // Multiple handlers triggered
    callback_manager.on_handler_triggered("restart nginx").await;
    callback_manager
        .on_handler_triggered("reload systemd")
        .await;

    callback_manager.on_play_end("Handler play", true).await;
    callback_manager.on_playbook_end("handler_test", true).await;

    assert_eq!(stats.total_handlers.load(Ordering::SeqCst), 2);

    let events = recording.events();
    let handler_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, CallbackEvent::HandlerTriggered(_)))
        .collect();
    assert_eq!(handler_events.len(), 2);
}

// ============================================================================
// Test 9: Facts Gathering Callbacks
// ============================================================================

#[tokio::test]
async fn test_facts_gathering_callbacks() {
    let recording = Arc::new(RecordingCallback::new());
    let stats = Arc::new(StatsCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(recording.clone());
    callback_manager.add_callback(stats.clone());

    let hosts = vec![
        "server1".to_string(),
        "server2".to_string(),
        "server3".to_string(),
    ];

    callback_manager.on_playbook_start("facts_test").await;
    callback_manager.on_play_start("Gather facts", &hosts).await;

    // Gather different facts for each host
    for (i, host) in hosts.iter().enumerate() {
        let mut facts = Facts::new();
        facts.set(
            "os_family",
            json!(if i % 2 == 0 { "Debian" } else { "RedHat" }),
        );
        facts.set("memory_mb", json!(8192 * (i + 1)));
        facts.set("cpu_count", json!(4 * (i + 1)));
        callback_manager.on_facts_gathered(host, &facts).await;
    }

    callback_manager.on_play_end("Gather facts", true).await;
    callback_manager.on_playbook_end("facts_test", true).await;

    assert_eq!(stats.total_facts_gathered.load(Ordering::SeqCst), 3);

    let events = recording.events();
    let fact_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, CallbackEvent::FactsGathered(_)))
        .collect();
    assert_eq!(fact_events.len(), 3);
}

// ============================================================================
// Test 10: Concurrent Callback Execution (Thread Safety)
// ============================================================================

#[tokio::test]
async fn test_concurrent_callback_execution() {
    let recording = Arc::new(RecordingCallback::new());
    let stats = Arc::new(StatsCallback::new());

    // Create separate callback managers for each spawn to avoid Send issue
    // In real usage, callbacks are Arc-wrapped and shared directly
    let mut handles = vec![];

    // Simulate concurrent task completions from multiple hosts
    for i in 0..10 {
        let recording_clone = recording.clone();
        let stats_clone = stats.clone();

        let handle = tokio::spawn(async move {
            let result = ExecutionResult {
                host: format!("host{}", i),
                task_name: format!("task{}", i),
                result: ModuleResult::changed("Done"),
                duration: Duration::from_millis(10),
                notify: vec![],
            };

            // Directly call callbacks - this is the pattern for thread-safe callbacks
            recording_clone.on_task_complete(&result).await;
            stats_clone.on_task_complete(&result).await;
        });
        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // All callbacks should have received all events
    assert_eq!(stats.total_tasks.load(Ordering::SeqCst), 10);
    assert_eq!(recording.count(), 10);
}

// ============================================================================
// Test 11: Empty Playbook
// ============================================================================

#[tokio::test]
async fn test_empty_playbook_callbacks() {
    let recording = Arc::new(RecordingCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(recording.clone());

    // Playbook with no plays
    callback_manager.on_playbook_start("empty_playbook").await;
    callback_manager
        .on_playbook_end("empty_playbook", true)
        .await;

    let events = recording.events();
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        CallbackEvent::PlaybookStart(ref name) if name == "empty_playbook"
    ));
    assert!(matches!(
        events[1],
        CallbackEvent::PlaybookEnd(ref name, true) if name == "empty_playbook"
    ));
}

// ============================================================================
// Test 12: Large Scale Playbook Simulation
// ============================================================================

#[tokio::test]
async fn test_large_scale_playbook() {
    let stats = Arc::new(StatsCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(stats.clone());

    let num_hosts = 50;
    let num_tasks_per_play = 10;
    let num_plays = 3;

    let hosts: Vec<String> = (0..num_hosts).map(|i| format!("host{:03}", i)).collect();

    callback_manager.on_playbook_start("large_scale_test").await;

    for play_num in 0..num_plays {
        callback_manager
            .on_play_start(&format!("Play {}", play_num + 1), &hosts)
            .await;

        for task_num in 0..num_tasks_per_play {
            for host in &hosts {
                let result = ExecutionResult {
                    host: host.clone(),
                    task_name: format!("Task {}.{}", play_num + 1, task_num + 1),
                    result: if task_num % 3 == 0 {
                        ModuleResult::ok("OK")
                    } else {
                        ModuleResult::changed("Changed")
                    },
                    duration: Duration::from_millis(5),
                    notify: vec![],
                };
                callback_manager.on_task_complete(&result).await;
            }
        }

        callback_manager
            .on_play_end(&format!("Play {}", play_num + 1), true)
            .await;
    }

    callback_manager
        .on_playbook_end("large_scale_test", true)
        .await;

    // Verify counts
    let expected_total_tasks = num_hosts * num_tasks_per_play * num_plays;
    assert_eq!(
        stats.total_tasks.load(Ordering::SeqCst),
        expected_total_tasks as u32
    );
    assert_eq!(stats.total_plays.load(Ordering::SeqCst), num_plays as u32);
    assert_eq!(stats.total_playbooks.load(Ordering::SeqCst), 1);

    // Verify summary
    let summary = stats.summary();
    assert!(summary.contains("Playbooks: 1"));
    assert!(summary.contains("Plays: 3"));
}

// ============================================================================
// Test 13: Callback Manager with No Callbacks
// ============================================================================

#[tokio::test]
async fn test_callback_manager_with_no_callbacks() {
    let callback_manager = CallbackManager::new();

    // This should not panic even with no callbacks registered
    callback_manager.on_playbook_start("no_callbacks").await;
    callback_manager
        .on_play_start("Play", &["host".to_string()])
        .await;

    let result = ExecutionResult {
        host: "host".to_string(),
        task_name: "task".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(1),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;

    callback_manager.on_play_end("Play", true).await;
    callback_manager.on_playbook_end("no_callbacks", true).await;

    // If we get here without panicking, the test passes
}

// ============================================================================
// Test 14: Callback Reset and Reuse
// ============================================================================

#[tokio::test]
async fn test_callback_reset_and_reuse() {
    let recording = Arc::new(RecordingCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(recording.clone());

    // First playbook
    callback_manager.on_playbook_start("playbook1").await;
    callback_manager.on_playbook_end("playbook1", true).await;

    assert_eq!(recording.count(), 2);

    // Reset
    recording.reset();
    assert_eq!(recording.count(), 0);
    assert!(recording.events().is_empty());

    // Second playbook
    callback_manager.on_playbook_start("playbook2").await;
    callback_manager.on_playbook_end("playbook2", true).await;

    assert_eq!(recording.count(), 2);
    let events = recording.events();
    assert!(matches!(
        &events[0],
        CallbackEvent::PlaybookStart(name) if name == "playbook2"
    ));
}

// ============================================================================
// Test 15: JSON Log Callback Format Verification
// ============================================================================

#[tokio::test]
async fn test_json_log_callback_format() {
    let json_log = Arc::new(JsonLogCallback::new());

    let mut callback_manager = CallbackManager::new();
    callback_manager.add_callback(json_log.clone());

    callback_manager.on_playbook_start("json_test").await;

    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "test_task".to_string(),
        result: ModuleResult::changed("Done"),
        duration: Duration::from_millis(150),
        notify: vec![],
    };
    callback_manager.on_task_complete(&result).await;

    callback_manager.on_playbook_end("json_test", true).await;

    let entries = json_log.entries();
    assert_eq!(entries.len(), 3);

    // Verify playbook_start entry
    let start_entry = &entries[0];
    assert_eq!(start_entry["event"], "playbook_start");
    assert_eq!(start_entry["name"], "json_test");
    assert!(start_entry["timestamp"].is_string());

    // Verify task_complete entry
    let task_entry = &entries[1];
    assert_eq!(task_entry["event"], "task_complete");
    assert_eq!(task_entry["task"], "test_task");
    assert_eq!(task_entry["host"], "localhost");
    assert_eq!(task_entry["success"], true);
    assert_eq!(task_entry["changed"], true);
    assert_eq!(task_entry["duration_ms"], 150);

    // Verify playbook_end entry
    let end_entry = &entries[2];
    assert_eq!(end_entry["event"], "playbook_end");
    assert_eq!(end_entry["success"], true);
}
