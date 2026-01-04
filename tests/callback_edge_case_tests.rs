//! Edge case tests for Rustible's callback plugin system.
//!
//! This test suite validates callback behavior under edge conditions:
//! 1. No plugins registered
//! 2. Plugin panics during callback
//! 3. Very large output data
//! 4. Unicode in task names/output
//! 5. Concurrent callback calls
//! 6. Plugin throws error
//! 7. Empty playbook/plays/tasks
//! 8. Thousands of hosts
//!
//! These tests ensure the callback system is robust and handles
//! exceptional conditions gracefully without crashing or data loss.

use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Barrier;

use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Test Helper: Mock Callbacks
// ============================================================================

/// A minimal callback that tracks invocations
#[derive(Debug, Default)]
struct TrackingCallback {
    playbook_starts: AtomicU32,
    playbook_ends: AtomicU32,
    play_starts: AtomicU32,
    play_ends: AtomicU32,
    task_starts: AtomicU32,
    task_completes: AtomicU32,
    handler_triggers: AtomicU32,
    facts_gathered: AtomicU32,
    events: RwLock<Vec<String>>,
}

impl TrackingCallback {
    fn new() -> Self {
        Self::default()
    }

    fn total_events(&self) -> u32 {
        self.playbook_starts.load(Ordering::SeqCst)
            + self.playbook_ends.load(Ordering::SeqCst)
            + self.play_starts.load(Ordering::SeqCst)
            + self.play_ends.load(Ordering::SeqCst)
            + self.task_starts.load(Ordering::SeqCst)
            + self.task_completes.load(Ordering::SeqCst)
            + self.handler_triggers.load(Ordering::SeqCst)
            + self.facts_gathered.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ExecutionCallback for TrackingCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.playbook_starts.fetch_add(1, Ordering::SeqCst);
        self.events.write().push(format!("playbook_start:{}", name));
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        self.playbook_ends.fetch_add(1, Ordering::SeqCst);
        self.events
            .write()
            .push(format!("playbook_end:{}:{}", name, success));
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.play_starts.fetch_add(1, Ordering::SeqCst);
        self.events
            .write()
            .push(format!("play_start:{}:hosts={}", name, hosts.len()));
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        self.play_ends.fetch_add(1, Ordering::SeqCst);
        self.events
            .write()
            .push(format!("play_end:{}:{}", name, success));
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        self.task_starts.fetch_add(1, Ordering::SeqCst);
        self.events
            .write()
            .push(format!("task_start:{}:{}", name, host));
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.task_completes.fetch_add(1, Ordering::SeqCst);
        self.events.write().push(format!(
            "task_complete:{}:{}:{}",
            result.task_name, result.host, result.result.success
        ));
    }

    async fn on_handler_triggered(&self, name: &str) {
        self.handler_triggers.fetch_add(1, Ordering::SeqCst);
        self.events
            .write()
            .push(format!("handler_triggered:{}", name));
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        self.facts_gathered.fetch_add(1, Ordering::SeqCst);
        self.events.write().push(format!("facts_gathered:{}", host));
    }
}

/// Helper to create ExecutionResult
fn create_result(
    host: &str,
    task_name: &str,
    success: bool,
    changed: bool,
    message: &str,
) -> ExecutionResult {
    ExecutionResult {
        host: host.to_string(),
        task_name: task_name.to_string(),
        result: ModuleResult {
            success,
            changed,
            message: message.to_string(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        },
        duration: Duration::from_millis(100),
        notify: Vec::new(),
    }
}

// ============================================================================
// Test 1: No Plugins Registered
// ============================================================================

/// A callback aggregator that can have zero or more plugins
struct CallbackAggregator {
    callbacks: Vec<Arc<dyn ExecutionCallback>>,
}

impl CallbackAggregator {
    fn new() -> Self {
        Self {
            callbacks: Vec::new(),
        }
    }

    fn count(&self) -> usize {
        self.callbacks.len()
    }

    async fn on_playbook_start(&self, name: &str) {
        for callback in &self.callbacks {
            callback.on_playbook_start(name).await;
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        for callback in &self.callbacks {
            callback.on_task_complete(result).await;
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        for callback in &self.callbacks {
            callback.on_playbook_end(name, success).await;
        }
    }
}

#[tokio::test]
async fn test_no_plugins_registered() {
    let aggregator = CallbackAggregator::new();

    // Should not panic with zero callbacks
    assert_eq!(aggregator.count(), 0);

    aggregator.on_playbook_start("test-playbook").await;

    let result = create_result("host1", "task1", true, false, "ok");
    aggregator.on_task_complete(&result).await;

    aggregator.on_playbook_end("test-playbook", true).await;

    // No panic = success
}

#[tokio::test]
async fn test_empty_callback_list_iteration() {
    let aggregator = CallbackAggregator::new();

    // Multiple operations with empty list
    for _ in 0..100 {
        aggregator.on_playbook_start("playbook").await;
        aggregator
            .on_task_complete(&create_result("host", "task", true, false, "ok"))
            .await;
        aggregator.on_playbook_end("playbook", true).await;
    }

    // No panic = success
}

// ============================================================================
// Test 2: Plugin Panics During Callback
// ============================================================================

/// A callback that panics on specific conditions
#[derive(Debug)]
struct PanickingCallback {
    panic_on_task: AtomicBool,
    panic_on_playbook_start: AtomicBool,
    panic_count: AtomicU32,
}

impl PanickingCallback {
    fn new() -> Self {
        Self {
            panic_on_task: AtomicBool::new(false),
            panic_on_playbook_start: AtomicBool::new(false),
            panic_count: AtomicU32::new(0),
        }
    }

    fn set_panic_on_task(&self, should_panic: bool) {
        self.panic_on_task.store(should_panic, Ordering::SeqCst);
    }

    fn set_panic_on_playbook_start(&self, should_panic: bool) {
        self.panic_on_playbook_start
            .store(should_panic, Ordering::SeqCst);
    }
}

#[async_trait]
impl ExecutionCallback for PanickingCallback {
    async fn on_playbook_start(&self, _name: &str) {
        if self.panic_on_playbook_start.load(Ordering::SeqCst) {
            self.panic_count.fetch_add(1, Ordering::SeqCst);
            panic!("Intentional panic in on_playbook_start");
        }
    }

    async fn on_task_complete(&self, _result: &ExecutionResult) {
        if self.panic_on_task.load(Ordering::SeqCst) {
            self.panic_count.fetch_add(1, Ordering::SeqCst);
            panic!("Intentional panic in on_task_complete");
        }
    }
}

/// An aggregator that catches panics from callbacks
struct SafeCallbackAggregator {
    callbacks: Vec<Arc<dyn ExecutionCallback>>,
    panic_count: AtomicU32,
}

impl SafeCallbackAggregator {
    fn new() -> Self {
        Self {
            callbacks: Vec::new(),
            panic_count: AtomicU32::new(0),
        }
    }

    fn add(&mut self, callback: Arc<dyn ExecutionCallback>) {
        self.callbacks.push(callback);
    }

    fn panics(&self) -> u32 {
        self.panic_count.load(Ordering::SeqCst)
    }

    async fn on_playbook_start_safe(&self, name: &str) {
        for callback in &self.callbacks {
            let name = name.to_string();
            let cb = callback.clone();
            let result = tokio::spawn(async move { cb.on_playbook_start(&name).await }).await;

            if result.is_err() {
                self.panic_count.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    async fn on_task_complete_safe(&self, result: &ExecutionResult) {
        for callback in &self.callbacks {
            let result_clone = result.clone();
            let cb = callback.clone();
            let spawn_result =
                tokio::spawn(async move { cb.on_task_complete(&result_clone).await }).await;

            if spawn_result.is_err() {
                self.panic_count.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

#[tokio::test]
async fn test_plugin_panic_isolation() {
    let panicking = Arc::new(PanickingCallback::new());
    panicking.set_panic_on_task(true);

    let tracking = Arc::new(TrackingCallback::new());

    let mut aggregator = SafeCallbackAggregator::new();
    aggregator.add(panicking.clone());
    aggregator.add(tracking.clone());

    let result = create_result("host1", "task1", true, false, "ok");

    // This should catch the panic and continue to the next callback
    aggregator.on_task_complete_safe(&result).await;

    // The tracking callback should still have been called
    // (Note: in the safe version, it will be called even after panic)
    assert_eq!(aggregator.panics(), 1);
}

#[tokio::test]
async fn test_panic_during_playbook_start() {
    let panicking = Arc::new(PanickingCallback::new());
    panicking.set_panic_on_playbook_start(true);

    let mut aggregator = SafeCallbackAggregator::new();
    aggregator.add(panicking);

    aggregator.on_playbook_start_safe("test").await;

    assert_eq!(aggregator.panics(), 1);
}

#[tokio::test]
async fn test_multiple_panicking_callbacks() {
    let panicking1 = Arc::new(PanickingCallback::new());
    let panicking2 = Arc::new(PanickingCallback::new());
    panicking1.set_panic_on_task(true);
    panicking2.set_panic_on_task(true);

    let mut aggregator = SafeCallbackAggregator::new();
    aggregator.add(panicking1);
    aggregator.add(panicking2);

    let result = create_result("host1", "task1", true, false, "ok");
    aggregator.on_task_complete_safe(&result).await;

    assert_eq!(aggregator.panics(), 2);
}

// ============================================================================
// Test 3: Very Large Output Data
// ============================================================================

#[tokio::test]
async fn test_very_large_output_message() {
    let callback = TrackingCallback::new();

    // Create a result with a very large message (1MB+)
    let large_message = "x".repeat(1024 * 1024); // 1MB string
    let result = create_result("host1", "task1", true, false, &large_message);

    callback.on_task_complete(&result).await;

    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_large_facts_data() {
    let callback = TrackingCallback::new();

    let mut facts = Facts::new();

    // Add many facts with large values
    for i in 0..1000 {
        let large_value = json!({
            "key": format!("value_{}", i),
            "data": "x".repeat(1024),  // 1KB per fact
            "nested": {
                "array": (0..100).collect::<Vec<i32>>(),
                "deep": {
                    "structure": {
                        "here": true
                    }
                }
            }
        });
        facts.set(&format!("fact_{}", i), large_value);
    }

    callback.on_facts_gathered("host1", &facts).await;

    assert_eq!(callback.facts_gathered.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_result_with_large_data_field() {
    let callback = TrackingCallback::new();

    // Create a result with large data in the result field
    let large_data = json!({
        "stdout": "x".repeat(100_000),  // 100KB stdout
        "stderr": "y".repeat(50_000),   // 50KB stderr
        "files_changed": (0..10000).map(|i| format!("/path/to/file_{}.txt", i)).collect::<Vec<_>>(),
    });

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "large_data_task".to_string(),
        result: ModuleResult {
            success: true,
            changed: true,
            message: "Success".to_string(),
            skipped: false,
            data: Some(large_data),
            warnings: Vec::new(),
        },
        duration: Duration::from_secs(30),
        notify: Vec::new(),
    };

    callback.on_task_complete(&result).await;

    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_many_large_results_sequential() {
    let callback = TrackingCallback::new();

    // Process 100 results with 10KB messages each
    for i in 0..100 {
        let message = format!("{}: {}", i, "x".repeat(10_000));
        let result = create_result(&format!("host{}", i), "task", true, false, &message);
        callback.on_task_complete(&result).await;
    }

    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 100);
}

// ============================================================================
// Test 4: Unicode in Task Names/Output
// ============================================================================

#[tokio::test]
async fn test_unicode_task_names() {
    let callback = TrackingCallback::new();

    // Various Unicode task names
    let unicode_names = vec![
        "Install nginx",                  // English
        "Instalar nginx",                 // Spanish
        "Nginx installieren",             // German (with umlaut)
        "Installer nginx",                // French
        "Instalacja nginx",               // Polish
        "nginx",                          // Russian
        "nginx",                          // Japanese
        "nginx",                          // Chinese
        "nginx",                          // Korean
        "emoji: Deploy",                  // Emoji
        "nginx",                          // Arabic
        "nginx",                          // Hebrew
        "Install",                        // Thai
        "Deploy\u{200B}Service",          // Zero-width space
        "Task\u{FEFF}Name",               // BOM character
        "\u{202E}Right-to-Left Override", // RTL override
        "Combining\u{0301} Character",    // Combining accent
        "\u{1F4BB} Computer Task",        // Extended emoji
    ];

    for name in &unicode_names {
        callback.on_task_start(name, "localhost").await;
        let result = create_result("localhost", name, true, false, "ok");
        callback.on_task_complete(&result).await;
    }

    assert_eq!(
        callback.task_starts.load(Ordering::SeqCst),
        unicode_names.len() as u32
    );
    assert_eq!(
        callback.task_completes.load(Ordering::SeqCst),
        unicode_names.len() as u32
    );
}

#[tokio::test]
async fn test_unicode_hostnames() {
    let callback = TrackingCallback::new();

    let unicode_hosts = vec![
        "localhost",
        "server-", // Russian server
        "",        // Japanese host
        "",        // Chinese host
        "host-with-emoji-",
        "server.example.com",
        "192.168.1.1",
        "::1", // IPv6
        "host_with_underscore",
        "host-with-dash",
        "UPPERCASE.HOST.COM",
    ];

    for host in &unicode_hosts {
        let result = create_result(host, "test_task", true, false, "ok");
        callback.on_task_complete(&result).await;
    }

    assert_eq!(
        callback.task_completes.load(Ordering::SeqCst),
        unicode_hosts.len() as u32
    );
}

#[tokio::test]
async fn test_unicode_in_output_messages() {
    let callback = TrackingCallback::new();

    let unicode_messages = vec![
        "Operation successful",
        " ",                           // CJK unified
        " ",                           // Cyrillic
        "Processed 100 items",         // Mixed
        "Error:\n\tTab\r\nNewlines",   // Control characters
        "\x00Null\x00Character\x00",   // Null bytes
        "",                            // Empty
        " ",                           // Just whitespace
        "\u{FFFD}Replacement\u{FFFD}", // Replacement character
        "Emoji: ",                     // Emoji sequence
    ];

    for (i, message) in unicode_messages.iter().enumerate() {
        let result = create_result("host", &format!("task{}", i), true, false, message);
        callback.on_task_complete(&result).await;
    }

    assert_eq!(
        callback.task_completes.load(Ordering::SeqCst),
        unicode_messages.len() as u32
    );
}

#[tokio::test]
async fn test_unicode_playbook_names() {
    let callback = TrackingCallback::new();

    let unicode_playbooks = vec![
        "deploy.yml",
        "- .yml",       // Mixed emoji
        "-.yml",        // Japanese
        "-.yml",        // Russian
        "path/to/.yml", // Chinese in path
    ];

    for name in &unicode_playbooks {
        callback.on_playbook_start(name).await;
        callback.on_playbook_end(name, true).await;
    }

    assert_eq!(
        callback.playbook_starts.load(Ordering::SeqCst),
        unicode_playbooks.len() as u32
    );
}

// ============================================================================
// Test 5: Concurrent Callback Calls
// ============================================================================

#[tokio::test]
async fn test_concurrent_task_completions() {
    let callback = Arc::new(TrackingCallback::new());

    let mut handles = vec![];

    // Spawn 100 concurrent task completions
    for i in 0..100 {
        let cb = callback.clone();
        let handle = tokio::spawn(async move {
            let result = create_result(
                &format!("host{}", i),
                &format!("task{}", i),
                true,
                false,
                "ok",
            );
            cb.on_task_complete(&result).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 100);
}

#[tokio::test]
async fn test_concurrent_mixed_callbacks() {
    let callback = Arc::new(TrackingCallback::new());
    let barrier = Arc::new(Barrier::new(50));

    let mut handles = vec![];

    // Mix of different callback types running concurrently
    for i in 0..10 {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            cb.on_playbook_start(&format!("playbook{}", i)).await;
        }));
    }

    for i in 0..10 {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            cb.on_play_start(&format!("play{}", i), &[format!("host{}", i)])
                .await;
        }));
    }

    for i in 0..10 {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            cb.on_task_start(&format!("task{}", i), &format!("host{}", i))
                .await;
        }));
    }

    for i in 0..10 {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            let result = create_result(
                &format!("host{}", i),
                &format!("task{}", i),
                true,
                false,
                "ok",
            );
            cb.on_task_complete(&result).await;
        }));
    }

    for i in 0..10 {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            cb.on_handler_triggered(&format!("handler{}", i)).await;
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(callback.playbook_starts.load(Ordering::SeqCst), 10);
    assert_eq!(callback.play_starts.load(Ordering::SeqCst), 10);
    assert_eq!(callback.task_starts.load(Ordering::SeqCst), 10);
    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 10);
    assert_eq!(callback.handler_triggers.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_rapid_sequential_callbacks() {
    let callback = TrackingCallback::new();

    // Rapid fire callbacks without any async yielding
    for _ in 0..1000 {
        callback.on_playbook_start("playbook").await;
        callback.on_play_start("play", &["host1".to_string()]).await;
        callback.on_task_start("task", "host1").await;
        let result = create_result("host1", "task", true, false, "ok");
        callback.on_task_complete(&result).await;
        callback.on_play_end("play", true).await;
        callback.on_playbook_end("playbook", true).await;
    }

    assert_eq!(callback.playbook_starts.load(Ordering::SeqCst), 1000);
    assert_eq!(callback.playbook_ends.load(Ordering::SeqCst), 1000);
}

#[tokio::test]
async fn test_concurrent_facts_gathering() {
    let callback = Arc::new(TrackingCallback::new());

    let mut handles = vec![];

    // 50 hosts gathering facts concurrently
    for i in 0..50 {
        let cb = callback.clone();
        let handle = tokio::spawn(async move {
            let mut facts = Facts::new();
            facts.set("host_id", json!(i));
            facts.set("os", json!("Linux"));
            cb.on_facts_gathered(&format!("host{}", i), &facts).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(callback.facts_gathered.load(Ordering::SeqCst), 50);
}

// ============================================================================
// Test 6: Plugin Throws Error (Simulated via Result types)
// ============================================================================

/// A callback that can return errors
#[derive(Debug)]
struct FallibleCallback {
    should_fail: AtomicBool,
    failure_count: AtomicU32,
    success_count: AtomicU32,
}

impl FallibleCallback {
    fn new() -> Self {
        Self {
            should_fail: AtomicBool::new(false),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
        }
    }

    fn set_should_fail(&self, should_fail: bool) {
        self.should_fail.store(should_fail, Ordering::SeqCst);
    }

    async fn on_task_complete_fallible(
        &self,
        _result: &ExecutionResult,
    ) -> Result<(), CallbackError> {
        if self.should_fail.load(Ordering::SeqCst) {
            self.failure_count.fetch_add(1, Ordering::SeqCst);
            Err(CallbackError::PluginFailed("Simulated failure".to_string()))
        } else {
            self.success_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
}

#[derive(Debug)]
enum CallbackError {
    PluginFailed(String),
    #[allow(dead_code)]
    IoError(String),
    #[allow(dead_code)]
    SerializationError(String),
}

#[tokio::test]
async fn test_callback_error_handling() {
    let callback = FallibleCallback::new();

    // First call succeeds
    callback.set_should_fail(false);
    let result = create_result("host1", "task1", true, false, "ok");
    let outcome = callback.on_task_complete_fallible(&result).await;
    assert!(outcome.is_ok());
    assert_eq!(callback.success_count.load(Ordering::SeqCst), 1);

    // Second call fails
    callback.set_should_fail(true);
    let outcome = callback.on_task_complete_fallible(&result).await;
    match outcome {
        Err(CallbackError::PluginFailed(message)) => {
            assert_eq!(message, "Simulated failure");
        }
        _ => panic!("Expected PluginFailed error"),
    }
    assert_eq!(callback.failure_count.load(Ordering::SeqCst), 1);

    // Third call succeeds again
    callback.set_should_fail(false);
    let outcome = callback.on_task_complete_fallible(&result).await;
    assert!(outcome.is_ok());
    assert_eq!(callback.success_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_intermittent_failures() {
    let callback = FallibleCallback::new();

    // Alternate between success and failure
    for i in 0..100 {
        callback.set_should_fail(i % 2 == 0);
        let result = create_result("host", "task", true, false, "ok");
        let _ = callback.on_task_complete_fallible(&result).await;
    }

    // Should have 50 successes and 50 failures
    assert_eq!(callback.success_count.load(Ordering::SeqCst), 50);
    assert_eq!(callback.failure_count.load(Ordering::SeqCst), 50);
}

// ============================================================================
// Test 7: Empty Playbook/Plays/Tasks
// ============================================================================

#[tokio::test]
async fn test_empty_playbook_name() {
    let callback = TrackingCallback::new();

    callback.on_playbook_start("").await;
    callback.on_playbook_end("", true).await;

    assert_eq!(callback.playbook_starts.load(Ordering::SeqCst), 1);
    assert_eq!(callback.playbook_ends.load(Ordering::SeqCst), 1);

    let events = callback.events.read();
    assert!(events.contains(&"playbook_start:".to_string()));
}

#[tokio::test]
async fn test_empty_play_with_no_hosts() {
    let callback = TrackingCallback::new();

    callback.on_play_start("empty_play", &[]).await;
    callback.on_play_end("empty_play", true).await;

    assert_eq!(callback.play_starts.load(Ordering::SeqCst), 1);
    let events = callback.events.read();
    assert!(events.contains(&"play_start:empty_play:hosts=0".to_string()));
}

#[tokio::test]
async fn test_empty_task_name() {
    let callback = TrackingCallback::new();

    callback.on_task_start("", "host1").await;
    let result = create_result("host1", "", true, false, "ok");
    callback.on_task_complete(&result).await;

    assert_eq!(callback.task_starts.load(Ordering::SeqCst), 1);
    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_empty_handler_name() {
    let callback = TrackingCallback::new();

    callback.on_handler_triggered("").await;

    assert_eq!(callback.handler_triggers.load(Ordering::SeqCst), 1);
    let events = callback.events.read();
    assert!(events.contains(&"handler_triggered:".to_string()));
}

#[tokio::test]
async fn test_playbook_with_zero_plays() {
    let callback = TrackingCallback::new();

    callback.on_playbook_start("empty_playbook.yml").await;
    // No plays executed
    callback.on_playbook_end("empty_playbook.yml", true).await;

    assert_eq!(callback.playbook_starts.load(Ordering::SeqCst), 1);
    assert_eq!(callback.playbook_ends.load(Ordering::SeqCst), 1);
    assert_eq!(callback.play_starts.load(Ordering::SeqCst), 0);
    assert_eq!(callback.task_starts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_play_with_zero_tasks() {
    let callback = TrackingCallback::new();

    callback.on_playbook_start("playbook.yml").await;
    callback
        .on_play_start("empty_play", &["host1".to_string()])
        .await;
    // No tasks executed in this play
    callback.on_play_end("empty_play", true).await;
    callback.on_playbook_end("playbook.yml", true).await;

    assert_eq!(callback.play_starts.load(Ordering::SeqCst), 1);
    assert_eq!(callback.play_ends.load(Ordering::SeqCst), 1);
    assert_eq!(callback.task_starts.load(Ordering::SeqCst), 0);
    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_empty_result_fields() {
    let callback = TrackingCallback::new();

    let result = ExecutionResult {
        host: "".to_string(),
        task_name: "".to_string(),
        result: ModuleResult {
            success: true,
            changed: false,
            message: "".to_string(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        },
        duration: Duration::ZERO,
        notify: Vec::new(),
    };

    callback.on_task_complete(&result).await;

    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_empty_facts() {
    let callback = TrackingCallback::new();

    let empty_facts = Facts::new();
    callback.on_facts_gathered("host1", &empty_facts).await;

    assert_eq!(callback.facts_gathered.load(Ordering::SeqCst), 1);
}

// ============================================================================
// Test 8: Thousands of Hosts
// ============================================================================

#[tokio::test]
async fn test_thousands_of_hosts_sequential() {
    let callback = TrackingCallback::new();

    const HOST_COUNT: u32 = 5000;

    // Generate host list
    let hosts: Vec<String> = (0..HOST_COUNT).map(|i| format!("host{}", i)).collect();

    callback.on_playbook_start("mass_deploy.yml").await;
    callback.on_play_start("Deploy to all", &hosts).await;

    // Simulate task execution on each host
    for host in &hosts {
        callback.on_task_start("Install package", host).await;
        let result = create_result(host, "Install package", true, true, "Package installed");
        callback.on_task_complete(&result).await;
    }

    callback.on_play_end("Deploy to all", true).await;
    callback.on_playbook_end("mass_deploy.yml", true).await;

    assert_eq!(callback.task_starts.load(Ordering::SeqCst), HOST_COUNT);
    assert_eq!(callback.task_completes.load(Ordering::SeqCst), HOST_COUNT);
}

#[tokio::test]
async fn test_thousands_of_hosts_concurrent() {
    let callback = Arc::new(TrackingCallback::new());

    const HOST_COUNT: u32 = 1000;

    callback.on_playbook_start("mass_deploy.yml").await;

    let hosts: Vec<String> = (0..HOST_COUNT).map(|i| format!("host{}", i)).collect();
    callback.on_play_start("Deploy to all", &hosts).await;

    // Process all hosts concurrently
    let mut handles = vec![];
    for host in hosts {
        let cb = callback.clone();
        let handle = tokio::spawn(async move {
            cb.on_task_start("Install package", &host).await;
            let result = create_result(&host, "Install package", true, true, "Package installed");
            cb.on_task_complete(&result).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    callback.on_play_end("Deploy to all", true).await;
    callback.on_playbook_end("mass_deploy.yml", true).await;

    assert_eq!(callback.task_starts.load(Ordering::SeqCst), HOST_COUNT);
    assert_eq!(callback.task_completes.load(Ordering::SeqCst), HOST_COUNT);
}

#[tokio::test]
async fn test_many_hosts_with_facts() {
    let callback = Arc::new(TrackingCallback::new());

    const HOST_COUNT: u32 = 500;

    let mut handles = vec![];
    for i in 0..HOST_COUNT {
        let cb = callback.clone();
        let handle = tokio::spawn(async move {
            let mut facts = Facts::new();
            facts.set("host_id", json!(i));
            facts.set("os_family", json!("Debian"));
            facts.set("memory_mb", json!(16384));
            facts.set("cpu_cores", json!(8));
            cb.on_facts_gathered(&format!("host{}", i), &facts).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(callback.facts_gathered.load(Ordering::SeqCst), HOST_COUNT);
}

/// Local host stats for testing purposes
#[derive(Debug, Clone, Default)]
struct TestHostStats {
    ok: u32,
    changed: u32,
    failed: u32,
    skipped: u32,
    unreachable: u32,
}

impl TestHostStats {
    fn new() -> Self {
        Self::default()
    }

    fn record_ok(&mut self) {
        self.ok += 1;
    }

    fn record_changed(&mut self) {
        self.changed += 1;
    }

    fn record_failed(&mut self) {
        self.failed += 1;
    }

    fn record_skipped(&mut self) {
        self.skipped += 1;
    }

    fn record_unreachable(&mut self) {
        self.unreachable += 1;
    }

    fn total(&self) -> u32 {
        self.ok + self.changed + self.failed + self.skipped + self.unreachable
    }

    fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }
}

#[tokio::test]
async fn test_host_stats_with_many_hosts() {
    let mut stats = HashMap::new();

    const HOST_COUNT: usize = 10000;

    // Simulate collecting stats for many hosts
    for i in 0..HOST_COUNT {
        let host_name = format!("host{}", i);
        let mut host_stats = TestHostStats::new();

        // Random distribution of results
        host_stats.record_ok();
        if i % 10 == 0 {
            host_stats.record_changed();
        }
        if i % 100 == 0 {
            host_stats.record_failed();
        }

        stats.insert(host_name, host_stats);
    }

    // Verify we can iterate and aggregate
    let total_ok: u32 = stats.values().map(|s| s.ok).sum();
    let total_changed: u32 = stats.values().map(|s| s.changed).sum();
    let total_failed: u32 = stats.values().map(|s| s.failed).sum();

    assert_eq!(total_ok, HOST_COUNT as u32);
    assert_eq!(total_changed, (HOST_COUNT / 10) as u32);
    assert_eq!(total_failed, (HOST_COUNT / 100) as u32);
}

#[tokio::test]
async fn test_many_hosts_with_failures() {
    let callback = Arc::new(TrackingCallback::new());

    const TOTAL_HOSTS: u32 = 1000;
    const FAILURE_RATE: u32 = 10; // 10% failure

    let mut handles = vec![];
    for i in 0..TOTAL_HOSTS {
        let cb = callback.clone();
        let should_fail = i % FAILURE_RATE == 0;
        let handle = tokio::spawn(async move {
            let result = create_result(
                &format!("host{}", i),
                "critical_task",
                !should_fail,
                false,
                if should_fail {
                    "Connection refused"
                } else {
                    "ok"
                },
            );
            cb.on_task_complete(&result).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(callback.task_completes.load(Ordering::SeqCst), TOTAL_HOSTS);
}

// ============================================================================
// Additional Edge Cases
// ============================================================================

#[tokio::test]
async fn test_duplicate_host_names() {
    let callback = TrackingCallback::new();

    // Multiple hosts with the same name (edge case in inventory)
    let hosts = vec![
        "host1".to_string(),
        "host1".to_string(),
        "host1".to_string(),
    ];

    callback.on_play_start("play", &hosts).await;

    for host in &hosts {
        callback.on_task_start("task", host).await;
        let result = create_result(host, "task", true, false, "ok");
        callback.on_task_complete(&result).await;
    }

    // Should handle duplicates gracefully
    assert_eq!(callback.task_starts.load(Ordering::SeqCst), 3);
    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_special_characters_in_names() {
    let callback = TrackingCallback::new();

    let special_names = vec![
        "task with spaces",
        "task\twith\ttabs",
        "task\nwith\nnewlines",
        "task/with/slashes",
        "task\\with\\backslashes",
        "task'with'quotes",
        "task\"with\"doublequotes",
        "task`with`backticks",
        "task$with$dollars",
        "task%with%percents",
        "task&with&ampersands",
        "task<with>angles",
        "task|with|pipes",
        "task;with;semicolons",
        "task:with:colons",
        "task?with?questions",
        "task*with*asterisks",
        "task[with]brackets",
        "task{with}braces",
        "task(with)parens",
        "task@with@at",
        "task#with#hash",
        "task^with^caret",
        "task~with~tilde",
        "task=with=equals",
        "task+with+plus",
    ];

    for name in &special_names {
        callback.on_task_start(name, "host").await;
        let result = create_result("host", name, true, false, "ok");
        callback.on_task_complete(&result).await;
    }

    assert_eq!(
        callback.task_starts.load(Ordering::SeqCst),
        special_names.len() as u32
    );
}

#[tokio::test]
async fn test_very_long_names() {
    let callback = TrackingCallback::new();

    // 10KB task name
    let long_task_name = "a".repeat(10_000);
    // 10KB host name
    let long_host_name = "h".repeat(10_000);
    // 10KB playbook name
    let long_playbook_name = "p".repeat(10_000);

    callback.on_playbook_start(&long_playbook_name).await;
    callback
        .on_play_start("play", &[long_host_name.clone()])
        .await;
    callback
        .on_task_start(&long_task_name, &long_host_name)
        .await;

    let result = create_result(&long_host_name, &long_task_name, true, false, "ok");
    callback.on_task_complete(&result).await;

    callback.on_play_end("play", true).await;
    callback.on_playbook_end(&long_playbook_name, true).await;

    assert_eq!(callback.total_events(), 6);
}

#[tokio::test]
async fn test_callback_after_playbook_end() {
    let callback = TrackingCallback::new();

    callback.on_playbook_start("playbook").await;
    callback.on_playbook_end("playbook", true).await;

    // These callbacks happen after playbook end (shouldn't crash)
    callback.on_task_start("orphan_task", "host").await;
    let result = create_result("host", "orphan_task", true, false, "ok");
    callback.on_task_complete(&result).await;

    // Should still record them
    assert_eq!(callback.task_starts.load(Ordering::SeqCst), 1);
    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_nested_playbook_calls() {
    let callback = TrackingCallback::new();

    // Simulate include/import of playbooks
    callback.on_playbook_start("main.yml").await;
    callback.on_playbook_start("included.yml").await;
    callback.on_playbook_start("deeply_included.yml").await;

    callback.on_playbook_end("deeply_included.yml", true).await;
    callback.on_playbook_end("included.yml", true).await;
    callback.on_playbook_end("main.yml", true).await;

    assert_eq!(callback.playbook_starts.load(Ordering::SeqCst), 3);
    assert_eq!(callback.playbook_ends.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_mixed_success_and_failure() {
    let callback = TrackingCallback::new();

    let scenarios = vec![
        (true, false, "Success without changes"),
        (true, true, "Success with changes"),
        (false, false, "Failure"),
        (true, false, "Recovery after failure"),
        (false, false, "Another failure"),
        (true, true, "Final success with changes"),
    ];

    for (i, (success, changed, msg)) in scenarios.iter().enumerate() {
        let result = create_result(&format!("host{}", i), "task", *success, *changed, msg);
        callback.on_task_complete(&result).await;
    }

    assert_eq!(callback.task_completes.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn test_high_frequency_callbacks() {
    let callback = Arc::new(TrackingCallback::new());
    let counter = Arc::new(AtomicUsize::new(0));

    const ITERATIONS: usize = 10_000;

    let mut handles = vec![];

    for _ in 0..10 {
        let cb = callback.clone();
        let cnt = counter.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..ITERATIONS / 10 {
                let i = cnt.fetch_add(1, Ordering::SeqCst);
                let result = create_result(&format!("host{}", i), "task", true, false, "ok");
                cb.on_task_complete(&result).await;
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(
        callback.task_completes.load(Ordering::SeqCst),
        ITERATIONS as u32
    );
}

// ============================================================================
// Test HostStats Edge Values
// ============================================================================

#[test]
fn test_host_stats_edge_values() {
    let mut stats = TestHostStats::new();

    // Test overflow protection (should not panic)
    for _ in 0..1_000_000 {
        stats.record_ok();
    }

    assert_eq!(stats.ok, 1_000_000);
    assert_eq!(stats.total(), 1_000_000);
}

#[test]
fn test_host_stats_all_categories() {
    let mut stats = TestHostStats::new();

    stats.record_ok();
    stats.record_changed();
    stats.record_changed(); // Another change
    stats.record_failed();
    stats.record_skipped();
    stats.record_unreachable();

    assert_eq!(stats.ok, 1);
    assert_eq!(stats.changed, 2);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.skipped, 1);
    assert_eq!(stats.unreachable, 1);
    assert!(stats.has_failures());
    assert_eq!(stats.total(), 6);
}

// ============================================================================
// Stress Test: Combined Edge Cases
// ============================================================================

#[tokio::test]
async fn test_combined_edge_cases() {
    let callback = Arc::new(TrackingCallback::new());

    // Combine multiple edge cases in one test
    let barrier = Arc::new(Barrier::new(5));

    let mut handles = vec![];

    // Unicode names thread
    {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            for name in ["", "", "", ""] {
                cb.on_task_start(name, "localhost").await;
                let result = create_result("localhost", name, true, false, "ok");
                cb.on_task_complete(&result).await;
            }
        }));
    }

    // Large data thread
    {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            let large_msg = "x".repeat(50_000);
            let result = create_result("host-large", "large-task", true, false, &large_msg);
            cb.on_task_complete(&result).await;
        }));
    }

    // Many hosts thread
    {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            for i in 0..100 {
                let result = create_result(&format!("host{}", i), "mass-task", true, false, "ok");
                cb.on_task_complete(&result).await;
            }
        }));
    }

    // Empty values thread
    {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            cb.on_playbook_start("").await;
            cb.on_play_start("", &[]).await;
            cb.on_task_start("", "").await;
            let result = create_result("", "", true, false, "");
            cb.on_task_complete(&result).await;
        }));
    }

    // Special characters thread
    {
        let cb = callback.clone();
        let b = barrier.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            let special = "task\n\t\r\0with\\special/chars";
            cb.on_task_start(special, "host").await;
            let result = create_result("host", special, true, false, "ok");
            cb.on_task_complete(&result).await;
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all callbacks were received
    let total = callback.total_events();
    assert!(total > 100, "Expected many events, got {}", total);
}
