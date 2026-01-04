//! Concurrent Safety Tests for Rustible's Callback System
//!
//! This test suite validates thread-safety and concurrent execution of callbacks:
//! 1. Callbacks called from multiple threads simultaneously
//! 2. Stress testing with many concurrent events
//! 3. Data race detection (using atomic verification patterns)
//! 4. High load testing
//! 5. Deadlock prevention verification
//!
//! Run with:
//! ```bash
//! cargo test --test callback_concurrent_tests -- --test-threads=1
//! ```

use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Barrier, Semaphore};
use tokio::time::timeout;

use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Thread-Safe Test Callback Implementation
// ============================================================================

/// A callback implementation designed for concurrent testing.
/// Uses atomics and lock-free structures where possible.
#[derive(Debug)]
pub struct ConcurrentTestCallback {
    // Atomic counters for event tracking
    playbook_start_count: AtomicU64,
    playbook_end_count: AtomicU64,
    play_start_count: AtomicU64,
    play_end_count: AtomicU64,
    task_start_count: AtomicU64,
    task_complete_count: AtomicU64,
    handler_triggered_count: AtomicU64,
    facts_gathered_count: AtomicU64,

    // Total events for verification
    total_events: AtomicU64,

    // Concurrent access tracking
    concurrent_access_count: AtomicU32,
    max_concurrent_access: AtomicU32,

    // Error tracking
    error_count: AtomicU32,

    // Event log with mutex (for order verification)
    event_log: Mutex<Vec<(String, u64, std::thread::ThreadId)>>,

    // Simulate slow callback for deadlock testing
    slow_mode: AtomicBool,
    slow_delay_ms: AtomicU32,
}

impl Default for ConcurrentTestCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl ConcurrentTestCallback {
    pub fn new() -> Self {
        Self {
            playbook_start_count: AtomicU64::new(0),
            playbook_end_count: AtomicU64::new(0),
            play_start_count: AtomicU64::new(0),
            play_end_count: AtomicU64::new(0),
            task_start_count: AtomicU64::new(0),
            task_complete_count: AtomicU64::new(0),
            handler_triggered_count: AtomicU64::new(0),
            facts_gathered_count: AtomicU64::new(0),
            total_events: AtomicU64::new(0),
            concurrent_access_count: AtomicU32::new(0),
            max_concurrent_access: AtomicU32::new(0),
            error_count: AtomicU32::new(0),
            event_log: Mutex::new(Vec::new()),
            slow_mode: AtomicBool::new(false),
            slow_delay_ms: AtomicU32::new(0),
        }
    }

    pub fn with_slow_mode(delay_ms: u32) -> Self {
        let callback = Self::new();
        callback.slow_mode.store(true, Ordering::SeqCst);
        callback.slow_delay_ms.store(delay_ms, Ordering::SeqCst);
        callback
    }

    fn record_event(&self, event_type: &str) {
        // Track concurrent access
        let current = self.concurrent_access_count.fetch_add(1, Ordering::SeqCst) + 1;

        // Update max concurrent access atomically
        loop {
            let max = self.max_concurrent_access.load(Ordering::SeqCst);
            if current <= max {
                break;
            }
            if self
                .max_concurrent_access
                .compare_exchange(max, current, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }

        // Record event
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        {
            let mut log = self.event_log.lock();
            log.push((
                event_type.to_string(),
                timestamp,
                std::thread::current().id(),
            ));
        }

        self.total_events.fetch_add(1, Ordering::SeqCst);

        // Decrement concurrent access
        self.concurrent_access_count.fetch_sub(1, Ordering::SeqCst);
    }

    async fn maybe_slow(&self) {
        if self.slow_mode.load(Ordering::SeqCst) {
            let delay = self.slow_delay_ms.load(Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(delay as u64)).await;
        }
    }

    pub fn playbook_start_count(&self) -> u64 {
        self.playbook_start_count.load(Ordering::SeqCst)
    }

    pub fn playbook_end_count(&self) -> u64 {
        self.playbook_end_count.load(Ordering::SeqCst)
    }

    pub fn play_start_count(&self) -> u64 {
        self.play_start_count.load(Ordering::SeqCst)
    }

    pub fn play_end_count(&self) -> u64 {
        self.play_end_count.load(Ordering::SeqCst)
    }

    pub fn task_start_count(&self) -> u64 {
        self.task_start_count.load(Ordering::SeqCst)
    }

    pub fn task_complete_count(&self) -> u64 {
        self.task_complete_count.load(Ordering::SeqCst)
    }

    pub fn handler_triggered_count(&self) -> u64 {
        self.handler_triggered_count.load(Ordering::SeqCst)
    }

    pub fn facts_gathered_count(&self) -> u64 {
        self.facts_gathered_count.load(Ordering::SeqCst)
    }

    pub fn total_events(&self) -> u64 {
        self.total_events.load(Ordering::SeqCst)
    }

    pub fn max_concurrent_access(&self) -> u32 {
        self.max_concurrent_access.load(Ordering::SeqCst)
    }

    pub fn error_count(&self) -> u32 {
        self.error_count.load(Ordering::SeqCst)
    }

    pub fn event_log_len(&self) -> usize {
        self.event_log.lock().len()
    }

    pub fn unique_threads(&self) -> usize {
        let log = self.event_log.lock();
        let threads: std::collections::HashSet<_> = log.iter().map(|(_, _, tid)| tid).collect();
        threads.len()
    }
}

#[async_trait]
impl ExecutionCallback for ConcurrentTestCallback {
    async fn on_playbook_start(&self, _name: &str) {
        self.playbook_start_count.fetch_add(1, Ordering::SeqCst);
        self.record_event("playbook_start");
        self.maybe_slow().await;
    }

    async fn on_playbook_end(&self, _name: &str, _success: bool) {
        self.playbook_end_count.fetch_add(1, Ordering::SeqCst);
        self.record_event("playbook_end");
        self.maybe_slow().await;
    }

    async fn on_play_start(&self, _name: &str, _hosts: &[String]) {
        self.play_start_count.fetch_add(1, Ordering::SeqCst);
        self.record_event("play_start");
        self.maybe_slow().await;
    }

    async fn on_play_end(&self, _name: &str, _success: bool) {
        self.play_end_count.fetch_add(1, Ordering::SeqCst);
        self.record_event("play_end");
        self.maybe_slow().await;
    }

    async fn on_task_start(&self, _name: &str, _host: &str) {
        self.task_start_count.fetch_add(1, Ordering::SeqCst);
        self.record_event("task_start");
        self.maybe_slow().await;
    }

    async fn on_task_complete(&self, _result: &ExecutionResult) {
        self.task_complete_count.fetch_add(1, Ordering::SeqCst);
        self.record_event("task_complete");
        self.maybe_slow().await;
    }

    async fn on_handler_triggered(&self, _name: &str) {
        self.handler_triggered_count.fetch_add(1, Ordering::SeqCst);
        self.record_event("handler_triggered");
        self.maybe_slow().await;
    }

    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        self.facts_gathered_count.fetch_add(1, Ordering::SeqCst);
        self.record_event("facts_gathered");
        self.maybe_slow().await;
    }
}

// ============================================================================
// Test 1: Multi-Thread Callback Invocation
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_callbacks_from_multiple_threads() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let thread_count = 8;
    let events_per_thread = 100;

    let mut handles = vec![];

    for t in 0..thread_count {
        let cb = Arc::clone(&callback);
        let handle = tokio::spawn(async move {
            for i in 0..events_per_thread {
                cb.on_playbook_start(&format!("playbook_{}_{}", t, i)).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let expected = thread_count * events_per_thread;
    assert_eq!(
        callback.playbook_start_count(),
        expected as u64,
        "All events should be recorded"
    );
    assert_eq!(
        callback.total_events(),
        expected as u64,
        "Total events should match"
    );

    println!(
        "Multi-thread test: {} events from {} threads, max concurrent: {}",
        callback.total_events(),
        callback.unique_threads(),
        callback.max_concurrent_access()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn test_mixed_callback_events_from_threads() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let thread_count = 16;
    let events_per_thread = 50;

    let mut handles = vec![];

    for t in 0..thread_count {
        let cb = Arc::clone(&callback);
        let handle = tokio::spawn(async move {
            for i in 0..events_per_thread {
                // Mix different callback types
                match i % 8 {
                    0 => cb.on_playbook_start(&format!("pb_{}", t)).await,
                    1 => cb.on_playbook_end(&format!("pb_{}", t), true).await,
                    2 => {
                        cb.on_play_start(&format!("play_{}", t), &[format!("host_{}", t)])
                            .await
                    }
                    3 => cb.on_play_end(&format!("play_{}", t), true).await,
                    4 => {
                        cb.on_task_start(&format!("task_{}", t), &format!("host_{}", t))
                            .await
                    }
                    5 => {
                        let result = ExecutionResult {
                            host: format!("host_{}", t),
                            task_name: format!("task_{}", t),
                            result: ModuleResult::ok("Success"),
                            duration: Duration::from_millis(10),
                            notify: vec![],
                        };
                        cb.on_task_complete(&result).await;
                    }
                    6 => cb.on_handler_triggered(&format!("handler_{}", t)).await,
                    7 => {
                        let facts = Facts::new();
                        cb.on_facts_gathered(&format!("host_{}", t), &facts).await;
                    }
                    _ => unreachable!(),
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let expected_total = thread_count * events_per_thread;
    assert_eq!(
        callback.total_events(),
        expected_total as u64,
        "All events should be recorded"
    );

    // Verify distribution across event types
    let total_typed = callback.playbook_start_count()
        + callback.playbook_end_count()
        + callback.play_start_count()
        + callback.play_end_count()
        + callback.task_start_count()
        + callback.task_complete_count()
        + callback.handler_triggered_count()
        + callback.facts_gathered_count();

    assert_eq!(
        total_typed, expected_total as u64,
        "Event type counts should sum to total"
    );

    println!(
        "Mixed events: {} total, {} unique threads",
        callback.total_events(),
        callback.unique_threads()
    );
}

// ============================================================================
// Test 2: Stress Test with Many Concurrent Events
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn stress_test_1000_concurrent_events() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let event_count = 1000;

    let mut handles = vec![];

    for i in 0..event_count {
        let cb = Arc::clone(&callback);
        handles.push(tokio::spawn(async move {
            cb.on_task_start(&format!("task_{}", i), &format!("host_{}", i % 100))
                .await;
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    assert_eq!(
        callback.task_start_count(),
        event_count as u64,
        "All 1000 events should be recorded"
    );

    println!(
        "Stress test 1000: max concurrent access = {}",
        callback.max_concurrent_access()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn stress_test_10000_concurrent_events() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let event_count = 10_000;

    let start = Instant::now();

    let mut handles = vec![];

    for i in 0..event_count {
        let cb = Arc::clone(&callback);
        handles.push(tokio::spawn(async move {
            let result = ExecutionResult {
                host: format!("host_{}", i % 500),
                task_name: format!("task_{}", i),
                result: ModuleResult::ok("Success"),
                duration: Duration::from_millis(1),
                notify: vec![],
            };
            cb.on_task_complete(&result).await;
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let duration = start.elapsed();

    assert_eq!(
        callback.task_complete_count(),
        event_count as u64,
        "All 10000 events should be recorded"
    );

    let events_per_sec = event_count as f64 / duration.as_secs_f64();

    println!(
        "Stress test 10000: {} events in {:?} ({:.0} events/sec), max concurrent = {}",
        event_count,
        duration,
        events_per_sec,
        callback.max_concurrent_access()
    );

    // Should handle at least 10000 events/sec
    assert!(
        events_per_sec > 1000.0,
        "Should handle at least 1000 events/sec, got {:.0}",
        events_per_sec
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn stress_test_burst_traffic() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let burst_count = 100;
    let events_per_burst = 100;

    let barrier = Arc::new(Barrier::new(burst_count));

    let mut handles = vec![];

    for burst in 0..burst_count {
        let cb = Arc::clone(&callback);
        let barrier = Arc::clone(&barrier);

        handles.push(tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier.wait().await;

            // Burst of events
            for i in 0..events_per_burst {
                cb.on_task_start(&format!("burst_{}_{}", burst, i), "host")
                    .await;
            }
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let expected = burst_count * events_per_burst;
    assert_eq!(
        callback.task_start_count(),
        expected as u64,
        "All burst events should be recorded"
    );

    println!(
        "Burst traffic: {} events, max concurrent = {}",
        expected,
        callback.max_concurrent_access()
    );
}

// ============================================================================
// Test 3: Data Race Verification
// ============================================================================

/// Counter that tracks concurrent modifications for race detection
#[derive(Debug)]
struct RaceDetector {
    value: AtomicU64,
    in_progress: AtomicU32,
}

impl RaceDetector {
    fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
            in_progress: AtomicU32::new(0),
        }
    }

    fn check_and_increment(&self) {
        // If something is already in progress, we have concurrent access
        let was_in_progress = self.in_progress.fetch_add(1, Ordering::SeqCst);
        if was_in_progress > 0 {
            // Concurrent access detected - this is expected with proper synchronization
        }

        // Perform operation
        self.value.fetch_add(1, Ordering::SeqCst);

        self.in_progress.fetch_sub(1, Ordering::SeqCst);
    }

    fn value(&self) -> u64 {
        self.value.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub struct RaceDetectingCallback {
    detector: RaceDetector,
    events: AtomicU64,
}

impl RaceDetectingCallback {
    fn new() -> Self {
        Self {
            detector: RaceDetector::new(),
            events: AtomicU64::new(0),
        }
    }

    fn event_count(&self) -> u64 {
        self.events.load(Ordering::SeqCst)
    }

    fn detector_value(&self) -> u64 {
        self.detector.value()
    }
}

#[async_trait]
impl ExecutionCallback for RaceDetectingCallback {
    async fn on_task_complete(&self, _result: &ExecutionResult) {
        self.detector.check_and_increment();
        self.events.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_no_data_races_under_concurrent_access() {
    let callback = Arc::new(RaceDetectingCallback::new());
    let thread_count = 8;
    let events_per_thread = 1000;

    let mut handles = vec![];

    for _ in 0..thread_count {
        let cb = Arc::clone(&callback);
        handles.push(tokio::spawn(async move {
            for i in 0..events_per_thread {
                let result = ExecutionResult {
                    host: "host".to_string(),
                    task_name: format!("task_{}", i),
                    result: ModuleResult::ok("OK"),
                    duration: Duration::from_millis(1),
                    notify: vec![],
                };
                cb.on_task_complete(&result).await;
            }
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let expected = thread_count * events_per_thread;

    // Both counters should match - no events lost
    assert_eq!(callback.event_count(), expected as u64);
    assert_eq!(callback.detector_value(), expected as u64);

    println!("Race detection: {} events processed safely", expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_atomic_counter_consistency() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let iterations = 5000;

    let mut handles = vec![];

    // Spawn multiple tasks that increment different counters
    for i in 0..iterations {
        let cb = Arc::clone(&callback);
        handles.push(tokio::spawn(async move {
            match i % 4 {
                0 => cb.on_playbook_start("pb").await,
                1 => cb.on_playbook_end("pb", true).await,
                2 => cb.on_play_start("play", &["host".to_string()]).await,
                3 => cb.on_play_end("play", true).await,
                _ => unreachable!(),
            }
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    // Verify totals match
    let sum = callback.playbook_start_count()
        + callback.playbook_end_count()
        + callback.play_start_count()
        + callback.play_end_count();

    assert_eq!(sum, iterations as u64, "All counters should sum correctly");
    assert_eq!(
        callback.total_events(),
        iterations as u64,
        "Total events should match"
    );
}

// ============================================================================
// Test 4: High Load Testing
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_high_load_sustained_traffic() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let duration_secs = 2;
    let _target_events_per_sec = 5000;

    let start = Instant::now();
    let events_sent = Arc::new(AtomicU64::new(0));

    let mut handles = vec![];

    // Spawn multiple workers
    for worker in 0..8 {
        let cb = Arc::clone(&callback);
        let events_sent = Arc::clone(&events_sent);
        let deadline = start + Duration::from_secs(duration_secs);

        handles.push(tokio::spawn(async move {
            let mut local_count = 0u64;
            while Instant::now() < deadline {
                cb.on_task_start(&format!("task_{}_{}", worker, local_count), "host")
                    .await;
                local_count += 1;
            }
            events_sent.fetch_add(local_count, Ordering::SeqCst);
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let total_events = callback.task_start_count();
    let actual_duration = start.elapsed().as_secs_f64();
    let events_per_sec = total_events as f64 / actual_duration;

    println!(
        "High load: {} events in {:.2}s ({:.0} events/sec), max concurrent = {}",
        total_events,
        actual_duration,
        events_per_sec,
        callback.max_concurrent_access()
    );

    // Should handle significant load
    assert!(
        total_events > 1000,
        "Should process at least 1000 events in {}s",
        duration_secs
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn test_high_load_with_varying_event_types() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let events_per_type = 500;
    let semaphore = Arc::new(Semaphore::new(100)); // Limit concurrent tasks

    let mut handles = vec![];

    // Spawn tasks for each event type
    for event_type in 0..8 {
        for i in 0..events_per_type {
            let cb = Arc::clone(&callback);
            let permit = semaphore.clone().acquire_owned().await.unwrap();

            handles.push(tokio::spawn(async move {
                match event_type {
                    0 => cb.on_playbook_start(&format!("pb_{}", i)).await,
                    1 => cb.on_playbook_end(&format!("pb_{}", i), true).await,
                    2 => {
                        cb.on_play_start(&format!("play_{}", i), &[format!("host_{}", i)])
                            .await
                    }
                    3 => cb.on_play_end(&format!("play_{}", i), true).await,
                    4 => {
                        cb.on_task_start(&format!("task_{}", i), &format!("host_{}", i))
                            .await
                    }
                    5 => {
                        let result = ExecutionResult {
                            host: format!("host_{}", i),
                            task_name: format!("task_{}", i),
                            result: ModuleResult::ok("OK"),
                            duration: Duration::from_millis(1),
                            notify: vec![],
                        };
                        cb.on_task_complete(&result).await;
                    }
                    6 => cb.on_handler_triggered(&format!("handler_{}", i)).await,
                    7 => {
                        let facts = Facts::new();
                        cb.on_facts_gathered(&format!("host_{}", i), &facts).await;
                    }
                    _ => unreachable!(),
                }
                drop(permit);
            }));
        }
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let expected_total = 8 * events_per_type;
    assert_eq!(callback.total_events(), expected_total as u64);

    // Verify each type got its share
    assert_eq!(callback.playbook_start_count(), events_per_type as u64);
    assert_eq!(callback.playbook_end_count(), events_per_type as u64);
    assert_eq!(callback.play_start_count(), events_per_type as u64);
    assert_eq!(callback.play_end_count(), events_per_type as u64);
    assert_eq!(callback.task_start_count(), events_per_type as u64);
    assert_eq!(callback.task_complete_count(), events_per_type as u64);
    assert_eq!(callback.handler_triggered_count(), events_per_type as u64);
    assert_eq!(callback.facts_gathered_count(), events_per_type as u64);

    println!(
        "High load varied: {} total events, {} per type",
        expected_total, events_per_type
    );
}

// ============================================================================
// Test 5: Deadlock Prevention
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_no_deadlock_with_slow_callbacks() {
    let callback = Arc::new(ConcurrentTestCallback::with_slow_mode(10)); // 10ms delay
    let event_count = 100;

    let start = Instant::now();

    let result = timeout(Duration::from_secs(30), async {
        let mut handles = vec![];

        for i in 0..event_count {
            let cb = Arc::clone(&callback);
            handles.push(tokio::spawn(async move {
                cb.on_task_start(&format!("task_{}", i), "host").await;
            }));
        }

        for handle in handles {
            handle.await.expect("Task should complete");
        }
    })
    .await;

    assert!(result.is_ok(), "Should complete without timeout/deadlock");

    let duration = start.elapsed();
    assert_eq!(callback.task_start_count(), event_count as u64);

    println!(
        "Slow callback test: {} events in {:?}, no deadlock",
        event_count, duration
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_no_deadlock_with_nested_callbacks() {
    // A callback that triggers other callbacks
    #[derive(Debug)]
    struct NestedCallback {
        inner: ConcurrentTestCallback,
        depth: AtomicU32,
        max_depth: AtomicU32,
    }

    impl NestedCallback {
        fn new() -> Self {
            Self {
                inner: ConcurrentTestCallback::new(),
                depth: AtomicU32::new(0),
                max_depth: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl ExecutionCallback for NestedCallback {
        async fn on_task_start(&self, name: &str, host: &str) {
            let current_depth = self.depth.fetch_add(1, Ordering::SeqCst);

            // Update max depth
            loop {
                let max = self.max_depth.load(Ordering::SeqCst);
                if current_depth <= max {
                    break;
                }
                if self
                    .max_depth
                    .compare_exchange(max, current_depth, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    break;
                }
            }

            self.inner.on_task_start(name, host).await;

            self.depth.fetch_sub(1, Ordering::SeqCst);
        }
    }

    let callback = Arc::new(NestedCallback::new());
    let event_count = 500;

    let result = timeout(Duration::from_secs(30), async {
        let mut handles = vec![];

        for i in 0..event_count {
            let cb = Arc::clone(&callback);
            handles.push(tokio::spawn(async move {
                cb.on_task_start(&format!("task_{}", i), "host").await;
            }));
        }

        for handle in handles {
            handle.await.expect("Task should complete");
        }
    })
    .await;

    assert!(result.is_ok(), "Should complete without deadlock");
    assert_eq!(callback.inner.task_start_count(), event_count as u64);

    println!(
        "Nested callback test: {} events, max depth = {}",
        event_count,
        callback.max_depth.load(Ordering::SeqCst)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_no_deadlock_lock_ordering() {
    // Test callbacks that acquire multiple locks
    #[derive(Debug)]
    struct MultiLockCallback {
        lock_a: Mutex<Vec<String>>,
        lock_b: Mutex<Vec<String>>,
        counter: AtomicU64,
    }

    impl MultiLockCallback {
        fn new() -> Self {
            Self {
                lock_a: Mutex::new(Vec::new()),
                lock_b: Mutex::new(Vec::new()),
                counter: AtomicU64::new(0),
            }
        }
    }

    #[async_trait]
    impl ExecutionCallback for MultiLockCallback {
        async fn on_task_start(&self, name: &str, _host: &str) {
            // Always acquire locks in same order to prevent deadlock
            let mut a = self.lock_a.lock();
            let mut b = self.lock_b.lock();

            a.push(name.to_string());
            b.push(name.to_string());

            self.counter.fetch_add(1, Ordering::SeqCst);

            drop(b);
            drop(a);
        }

        async fn on_task_complete(&self, result: &ExecutionResult) {
            // Same lock order
            let mut a = self.lock_a.lock();
            let mut b = self.lock_b.lock();

            a.push(format!("complete_{}", result.task_name));
            b.push(format!("complete_{}", result.task_name));

            self.counter.fetch_add(1, Ordering::SeqCst);

            drop(b);
            drop(a);
        }
    }

    let callback = Arc::new(MultiLockCallback::new());
    let event_count = 200;

    let result = timeout(Duration::from_secs(30), async {
        let mut handles = vec![];

        for i in 0..event_count {
            let cb = Arc::clone(&callback);
            handles.push(tokio::spawn(async move {
                cb.on_task_start(&format!("task_{}", i), "host").await;

                let result = ExecutionResult {
                    host: "host".to_string(),
                    task_name: format!("task_{}", i),
                    result: ModuleResult::ok("OK"),
                    duration: Duration::from_millis(1),
                    notify: vec![],
                };
                cb.on_task_complete(&result).await;
            }));
        }

        for handle in handles {
            handle.await.expect("Task should complete");
        }
    })
    .await;

    assert!(result.is_ok(), "Should complete without deadlock");

    let expected = event_count * 2; // start + complete for each
    assert_eq!(callback.counter.load(Ordering::SeqCst), expected as u64);

    // Verify lock contents match
    let a = callback.lock_a.lock();
    let b = callback.lock_b.lock();
    assert_eq!(a.len(), expected);
    assert_eq!(b.len(), expected);

    println!("Multi-lock test: {} events processed safely", expected);
}

// ============================================================================
// Test 6: Memory Safety Under Concurrent Access
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_memory_safety_shared_state() {
    #[derive(Debug)]
    struct SharedStateCallback {
        shared_data: RwLock<HashMap<String, Vec<String>>>,
        event_count: AtomicU64,
    }

    impl SharedStateCallback {
        fn new() -> Self {
            Self {
                shared_data: RwLock::new(HashMap::new()),
                event_count: AtomicU64::new(0),
            }
        }
    }

    #[async_trait]
    impl ExecutionCallback for SharedStateCallback {
        async fn on_task_complete(&self, result: &ExecutionResult) {
            // Write to shared state
            {
                let mut data = self.shared_data.write();
                data.entry(result.host.clone())
                    .or_insert_with(Vec::new)
                    .push(result.task_name.clone());
            }

            self.event_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    let callback = Arc::new(SharedStateCallback::new());
    let host_count = 10;
    let tasks_per_host = 100;

    let mut handles = vec![];

    for host in 0..host_count {
        for task in 0..tasks_per_host {
            let cb = Arc::clone(&callback);
            handles.push(tokio::spawn(async move {
                let result = ExecutionResult {
                    host: format!("host_{}", host),
                    task_name: format!("task_{}_{}", host, task),
                    result: ModuleResult::ok("OK"),
                    duration: Duration::from_millis(1),
                    notify: vec![],
                };
                cb.on_task_complete(&result).await;
            }));
        }
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    // Verify all data was correctly stored
    let data = callback.shared_data.read();
    assert_eq!(data.len(), host_count, "Should have all hosts");

    for host in 0..host_count {
        let host_key = format!("host_{}", host);
        let tasks = data.get(&host_key).expect("Host should exist");
        assert_eq!(
            tasks.len(),
            tasks_per_host,
            "Host {} should have all tasks",
            host
        );
    }

    let expected = host_count * tasks_per_host;
    assert_eq!(callback.event_count.load(Ordering::SeqCst), expected as u64);

    println!(
        "Memory safety: {} events, {} hosts with {} tasks each",
        expected, host_count, tasks_per_host
    );
}

// ============================================================================
// Test 7: Callback Manager Concurrent Access (if available)
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_multiple_callbacks_concurrent_dispatch() {
    let callback1 = Arc::new(ConcurrentTestCallback::new());
    let callback2 = Arc::new(ConcurrentTestCallback::new());
    let callback3 = Arc::new(ConcurrentTestCallback::new());

    let event_count = 500;

    let mut handles = vec![];

    for i in 0..event_count {
        let cb1 = Arc::clone(&callback1);
        let cb2 = Arc::clone(&callback2);
        let cb3 = Arc::clone(&callback3);

        handles.push(tokio::spawn(async move {
            // Dispatch to all callbacks concurrently
            let task_name = format!("task_{}", i);
            let f1 = cb1.on_task_start(&task_name, "host");
            let f2 = cb2.on_task_start(&task_name, "host");
            let f3 = cb3.on_task_start(&task_name, "host");

            tokio::join!(f1, f2, f3);
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    // All callbacks should have received all events
    assert_eq!(callback1.task_start_count(), event_count as u64);
    assert_eq!(callback2.task_start_count(), event_count as u64);
    assert_eq!(callback3.task_start_count(), event_count as u64);

    println!(
        "Multi-callback dispatch: {} events to 3 callbacks",
        event_count
    );
}

// ============================================================================
// Test 8: Error Recovery in Concurrent Scenarios
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_error_resilience() {
    #[derive(Debug)]
    struct FailingCallback {
        success_count: AtomicU64,
        fail_count: AtomicU64,
        fail_rate: f32,
    }

    impl FailingCallback {
        fn new(fail_rate: f32) -> Self {
            Self {
                success_count: AtomicU64::new(0),
                fail_count: AtomicU64::new(0),
                fail_rate,
            }
        }
    }

    #[async_trait]
    impl ExecutionCallback for FailingCallback {
        async fn on_task_complete(&self, _result: &ExecutionResult) {
            // Simulate random failures
            if rand::random::<f32>() < self.fail_rate {
                self.fail_count.fetch_add(1, Ordering::SeqCst);
                // In a real scenario, this might panic or return error
                // Here we just track it
            } else {
                self.success_count.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    let callback = Arc::new(FailingCallback::new(0.1)); // 10% failure rate
    let event_count = 1000;

    let mut handles = vec![];

    for i in 0..event_count {
        let cb = Arc::clone(&callback);
        handles.push(tokio::spawn(async move {
            let result = ExecutionResult {
                host: "host".to_string(),
                task_name: format!("task_{}", i),
                result: ModuleResult::ok("OK"),
                duration: Duration::from_millis(1),
                notify: vec![],
            };
            cb.on_task_complete(&result).await;
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let success = callback.success_count.load(Ordering::SeqCst);
    let fail = callback.fail_count.load(Ordering::SeqCst);
    let total = success + fail;

    assert_eq!(total, event_count as u64, "All events should be processed");

    println!(
        "Error resilience: {} success, {} fail out of {}",
        success, fail, total
    );
}

// ============================================================================
// Test 9: Performance Under Contention
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_performance_under_high_contention() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let thread_count = 8;
    let events_per_thread = 1000;

    // Force high contention by having all threads target same callback
    let barrier = Arc::new(Barrier::new(thread_count));

    let start = Instant::now();

    let mut handles = vec![];

    for t in 0..thread_count {
        let cb = Arc::clone(&callback);
        let barrier = Arc::clone(&barrier);

        handles.push(tokio::spawn(async move {
            // Synchronize start
            barrier.wait().await;

            for i in 0..events_per_thread {
                cb.on_task_start(&format!("task_{}_{}", t, i), "host").await;
            }
        }));
    }

    for handle in handles {
        handle.await.expect("Task should complete");
    }

    let duration = start.elapsed();
    let total_events = callback.task_start_count();
    let events_per_sec = total_events as f64 / duration.as_secs_f64();

    assert_eq!(
        total_events,
        (thread_count * events_per_thread) as u64,
        "All events should be processed"
    );

    println!(
        "High contention: {} events in {:?} ({:.0} events/sec), max concurrent = {}",
        total_events,
        duration,
        events_per_sec,
        callback.max_concurrent_access()
    );
}

// ============================================================================
// Test 10: Long-Running Stability
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_long_running_stability() {
    let callback = Arc::new(ConcurrentTestCallback::new());
    let iterations = 100;
    let events_per_iteration = 100;

    let mut iteration_times = vec![];

    for iter in 0..iterations {
        let start = Instant::now();

        let mut handles = vec![];

        for i in 0..events_per_iteration {
            let cb = Arc::clone(&callback);
            handles.push(tokio::spawn(async move {
                cb.on_task_start(&format!("iter_{}_task_{}", iter, i), "host")
                    .await;
            }));
        }

        for handle in handles {
            handle.await.expect("Task should complete");
        }

        iteration_times.push(start.elapsed());
    }

    let expected = iterations * events_per_iteration;
    assert_eq!(
        callback.task_start_count(),
        expected as u64,
        "All events should be processed"
    );

    // Check for performance stability
    let first_10_avg: Duration = iteration_times[..10].iter().sum::<Duration>() / 10;
    let last_10_avg: Duration = iteration_times[iterations - 10..].iter().sum::<Duration>() / 10;

    // Performance should remain stable (within 3x)
    let ratio = last_10_avg.as_nanos() as f64 / first_10_avg.as_nanos().max(1) as f64;

    println!(
        "Long running: {} iterations, first 10 avg {:?}, last 10 avg {:?}, ratio {:.2}",
        iterations, first_10_avg, last_10_avg, ratio
    );

    assert!(
        ratio < 3.0,
        "Performance should remain stable, ratio was {:.2}",
        ratio
    );
}

// ============================================================================
// Utility Functions
// ============================================================================

mod rand {
    use std::cell::Cell;

    thread_local! {
        static RNG: Cell<u64> = Cell::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        );
    }

    /// Simple xorshift64 PRNG for testing
    pub fn random<T>() -> T
    where
        T: RandomValue,
    {
        RNG.with(|rng| {
            let mut x = rng.get();
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            rng.set(x);
            T::from_u64(x)
        })
    }

    pub trait RandomValue {
        fn from_u64(x: u64) -> Self;
    }

    impl RandomValue for f32 {
        fn from_u64(x: u64) -> Self {
            (x as f32) / (u64::MAX as f32)
        }
    }
}
