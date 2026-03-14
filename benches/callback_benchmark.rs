//! Performance Benchmarks for Rustible's Callback System
//!
//! This benchmark suite measures the performance characteristics of the callback
//! plugin system, which is critical for real-time feedback during playbook execution.
//!
//! ## Benchmarks Included:
//!
//! 1. **Callback Dispatch Overhead**:
//!    - Single callback invocation latency
//!    - Callback trait method dispatch
//!    - Event creation and cloning
//!    - Synchronous vs async callback overhead
//!
//! 2. **Multiple Plugin Performance**:
//!    - Aggregating multiple callbacks
//!    - Sequential callback chain execution
//!    - Parallel callback notification
//!    - Plugin registration/lookup overhead
//!
//! 3. **Large Event Data Handling**:
//!    - Events with large payloads (facts, results)
//!    - Serialization overhead for JSON/YAML output
//!    - String formatting for console output
//!    - Diff data handling
//!
//! 4. **Memory Usage During Execution**:
//!    - Callback state accumulation
//!    - Event buffer management
//!    - Statistics tracking overhead
//!    - Host stats aggregation
//!
//! 5. **With/Without Callbacks Comparison**:
//!    - Baseline execution without callbacks
//!    - Overhead of different callback configurations
//!    - Impact on parallel execution
//!    - Callback-free fast path

use async_trait::async_trait;
use criterion::{
    black_box, criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, BenchmarkId,
    Criterion, Throughput,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

use rustible::executor::task::{TaskResult, TaskStatus};
use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Callback Plugin Types for Benchmarking
// (Self-contained implementations to avoid unstable module dependencies)
// ============================================================================

/// Events that can occur during execution
#[derive(Debug, Clone)]
pub enum CallbackEvent {
    PlaybookStart {
        playbook: String,
    },
    PlaybookEnd {
        playbook: String,
        duration: Duration,
        stats: HashMap<String, HostStats>,
    },
    PlayStart {
        name: String,
        hosts_pattern: String,
    },
    PlayEnd {
        name: String,
    },
    TaskStart {
        name: String,
        module: String,
        args: HashMap<String, serde_json::Value>,
    },
    TaskResult {
        host: String,
        task_name: String,
        result: TaskResultInfo,
    },
    HandlerStart {
        name: String,
    },
    HandlerResult {
        host: String,
        handler_name: String,
        result: TaskResultInfo,
    },
    Warning {
        message: String,
    },
    Debug {
        host: Option<String>,
        message: String,
    },
}

/// Information about a task execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResultInfo {
    pub status: TaskStatus,
    pub changed: bool,
    pub msg: Option<String>,
    pub result: Option<serde_json::Value>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub rc: Option<i32>,
    pub duration: Option<Duration>,
}

impl From<TaskResult> for TaskResultInfo {
    fn from(result: TaskResult) -> Self {
        Self {
            status: result.status,
            changed: result.changed,
            msg: result.msg,
            result: result.result,
            stdout: None,
            stderr: None,
            rc: None,
            duration: None,
        }
    }
}

/// Statistics for a single host's execution
#[derive(Debug, Clone, Default)]
pub struct HostStats {
    pub ok: u32,
    pub changed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub unreachable: u32,
    pub rescued: u32,
    pub ignored: u32,
}

impl HostStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, status: TaskStatus, changed: bool) {
        match status {
            TaskStatus::Ok => {
                if changed {
                    self.changed += 1;
                } else {
                    self.ok += 1;
                }
            }
            TaskStatus::Changed => self.changed += 1,
            TaskStatus::Failed => self.failed += 1,
            TaskStatus::Skipped => self.skipped += 1,
            TaskStatus::Unreachable => self.unreachable += 1,
        }
    }

    pub fn has_failures(&self) -> bool {
        self.failed > 0 || self.unreachable > 0
    }

    pub fn total(&self) -> u32 {
        self.ok
            + self.changed
            + self.failed
            + self.skipped
            + self.unreachable
            + self.rescued
            + self.ignored
    }
}

/// Trait for callback plugins (sync version for benchmarking)
pub trait CallbackPlugin: Send + Sync {
    fn name(&self) -> &'static str;

    fn on_playbook_start(&self, _playbook: &str) {}
    fn on_playbook_end(
        &self,
        _playbook: &str,
        _duration: Duration,
        _stats: &HashMap<String, HostStats>,
    ) {
    }
    fn on_play_start(&self, _name: &str, _hosts_pattern: &str) {}
    fn on_play_end(&self, _name: &str) {}
    fn on_task_start(
        &self,
        _name: &str,
        _module: &str,
        _args: &HashMap<String, serde_json::Value>,
    ) {
    }
    fn on_task_result(&self, _host: &str, _task_name: &str, _result: &TaskResultInfo) {}
    fn on_handler_start(&self, _name: &str) {}
    fn on_handler_result(&self, _host: &str, _handler_name: &str, _result: &TaskResultInfo) {}
    fn on_warning(&self, _message: &str) {}
    fn on_debug(&self, _host: Option<&str>, _message: &str) {}

    fn on_event(&self, event: &CallbackEvent) {
        match event {
            CallbackEvent::PlaybookStart { playbook } => self.on_playbook_start(playbook),
            CallbackEvent::PlaybookEnd {
                playbook,
                duration,
                stats,
            } => self.on_playbook_end(playbook, *duration, stats),
            CallbackEvent::PlayStart {
                name,
                hosts_pattern,
            } => self.on_play_start(name, hosts_pattern),
            CallbackEvent::PlayEnd { name } => self.on_play_end(name),
            CallbackEvent::TaskStart { name, module, args } => {
                self.on_task_start(name, module, args)
            }
            CallbackEvent::TaskResult {
                host,
                task_name,
                result,
            } => self.on_task_result(host, task_name, result),
            CallbackEvent::HandlerStart { name } => self.on_handler_start(name),
            CallbackEvent::HandlerResult {
                host,
                handler_name,
                result,
            } => self.on_handler_result(host, handler_name, result),
            CallbackEvent::Warning { message } => self.on_warning(message),
            CallbackEvent::Debug { host, message } => self.on_debug(host.as_deref(), message),
        }
    }

    fn is_buffered(&self) -> bool {
        false
    }
    fn flush(&self) {}
}

// ============================================================================
// Mock Callback Implementations for Benchmarking
// ============================================================================

/// A no-op callback that does absolutely nothing - baseline for overhead measurement
#[derive(Debug, Default)]
struct NoOpCallback;

impl CallbackPlugin for NoOpCallback {
    fn name(&self) -> &'static str {
        "noop"
    }
}

/// A minimal callback that only tracks counts - minimal overhead
#[derive(Debug, Default)]
struct CountingCallback {
    task_count: AtomicU32,
    event_count: AtomicU32,
}

impl CallbackPlugin for CountingCallback {
    fn name(&self) -> &'static str {
        "counting"
    }

    fn on_task_start(
        &self,
        _name: &str,
        _module: &str,
        _args: &HashMap<String, serde_json::Value>,
    ) {
        self.task_count.fetch_add(1, Ordering::Relaxed);
    }

    fn on_task_result(&self, _host: &str, _task_name: &str, _result: &TaskResultInfo) {
        self.event_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// A callback that collects all events into a buffer - simulates real-world logging
#[derive(Debug, Default)]
struct BufferingCallback {
    events: RwLock<Vec<String>>,
}

impl CallbackPlugin for BufferingCallback {
    fn name(&self) -> &'static str {
        "buffering"
    }

    fn on_playbook_start(&self, playbook: &str) {
        self.events
            .write()
            .push(format!("PLAYBOOK START: {}", playbook));
    }

    fn on_playbook_end(
        &self,
        playbook: &str,
        duration: Duration,
        stats: &HashMap<String, HostStats>,
    ) {
        self.events.write().push(format!(
            "PLAYBOOK END: {} ({:?}, {} hosts)",
            playbook,
            duration,
            stats.len()
        ));
    }

    fn on_play_start(&self, name: &str, hosts_pattern: &str) {
        self.events
            .write()
            .push(format!("PLAY: {} on {}", name, hosts_pattern));
    }

    fn on_task_start(&self, name: &str, module: &str, _args: &HashMap<String, serde_json::Value>) {
        self.events
            .write()
            .push(format!("TASK: {} ({})", name, module));
    }

    fn on_task_result(&self, host: &str, task_name: &str, result: &TaskResultInfo) {
        self.events.write().push(format!(
            "RESULT: {} on {} - {:?} (changed: {})",
            task_name, host, result.status, result.changed
        ));
    }

    fn is_buffered(&self) -> bool {
        true
    }

    fn flush(&self) {
        self.events.write().clear();
    }
}

/// A callback that serializes everything to JSON - simulates JSON output plugin
#[derive(Debug, Default)]
struct JsonSerializingCallback {
    output: RwLock<Vec<serde_json::Value>>,
}

impl CallbackPlugin for JsonSerializingCallback {
    fn name(&self) -> &'static str {
        "json"
    }

    fn on_playbook_start(&self, playbook: &str) {
        self.output.write().push(json!({
            "event": "playbook_start",
            "playbook": playbook
        }));
    }

    fn on_task_start(&self, name: &str, module: &str, args: &HashMap<String, serde_json::Value>) {
        self.output.write().push(json!({
            "event": "task_start",
            "task": name,
            "module": module,
            "args": args
        }));
    }

    fn on_task_result(&self, host: &str, task_name: &str, result: &TaskResultInfo) {
        self.output.write().push(json!({
            "event": "task_result",
            "host": host,
            "task": task_name,
            "status": format!("{:?}", result.status),
            "changed": result.changed,
            "msg": result.msg,
            "result": result.result,
            "duration_ms": result.duration.map(|d| d.as_millis())
        }));
    }
}

/// Async callback for ExecutionCallback trait benchmarking
#[derive(Debug, Default)]
struct AsyncMockCallback {
    task_count: AtomicU32,
    events: RwLock<Vec<String>>,
}

#[async_trait]
impl ExecutionCallback for AsyncMockCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.events.write().push(format!("playbook_start:{}", name));
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        self.events
            .write()
            .push(format!("playbook_end:{}:{}", name, success));
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.events
            .write()
            .push(format!("play_start:{}:{}", name, hosts.len()));
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        self.events
            .write()
            .push(format!("play_end:{}:{}", name, success));
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        self.task_count.fetch_add(1, Ordering::Relaxed);
        self.events
            .write()
            .push(format!("task_start:{}:{}", name, host));
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.events.write().push(format!(
            "task_complete:{}:{}:{}",
            result.task_name, result.host, result.result.success
        ));
    }

    async fn on_handler_triggered(&self, name: &str) {
        self.events
            .write()
            .push(format!("handler_triggered:{}", name));
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        self.events
            .write()
            .push(format!("facts_gathered:{}:{}", host, facts.all().len()));
    }
}

/// Callback aggregator for testing multiple plugins
struct CallbackAggregator {
    callbacks: Vec<Arc<dyn CallbackPlugin>>,
}

impl CallbackAggregator {
    fn new() -> Self {
        Self {
            callbacks: Vec::new(),
        }
    }

    fn add(&mut self, callback: Arc<dyn CallbackPlugin>) {
        self.callbacks.push(callback);
    }

    fn dispatch_task_start(
        &self,
        name: &str,
        module: &str,
        args: &HashMap<String, serde_json::Value>,
    ) {
        for callback in &self.callbacks {
            callback.on_task_start(name, module, args);
        }
    }

    fn dispatch_task_result(&self, host: &str, task_name: &str, result: &TaskResultInfo) {
        for callback in &self.callbacks {
            callback.on_task_result(host, task_name, result);
        }
    }

    fn dispatch_event(&self, event: &CallbackEvent) {
        for callback in &self.callbacks {
            callback.on_event(event);
        }
    }
}

// ============================================================================
// Data Generators
// ============================================================================

/// Generate a simple task result for benchmarking
fn generate_simple_result() -> TaskResultInfo {
    TaskResultInfo {
        status: TaskStatus::Ok,
        changed: false,
        msg: Some("Task completed successfully".to_string()),
        result: None,
        stdout: None,
        stderr: None,
        rc: None,
        duration: Some(Duration::from_millis(150)),
    }
}

/// Generate a complex task result with lots of data
fn generate_complex_result() -> TaskResultInfo {
    TaskResultInfo {
        status: TaskStatus::Changed,
        changed: true,
        msg: Some("Configuration file updated with new settings".to_string()),
        result: Some(json!({
            "path": "/etc/nginx/nginx.conf",
            "owner": "root",
            "group": "root",
            "mode": "0644",
            "size": 2048,
            "checksum": "abc123def456",
            "backup": "/etc/nginx/nginx.conf.backup.20240101",
            "attributes": {
                "readable": true,
                "writable": true,
                "executable": false
            }
        })),
        stdout: Some("nginx: the configuration file /etc/nginx/nginx.conf syntax is ok\nnginx: configuration file /etc/nginx/nginx.conf test is successful".to_string()),
        stderr: None,
        rc: Some(0),
        duration: Some(Duration::from_millis(2500)),
    }
}

/// Generate large facts data (simulating gathered facts)
fn generate_large_facts() -> Facts {
    let mut facts = Facts::new();

    // System facts
    facts.set("ansible_distribution", json!("Ubuntu"));
    facts.set("ansible_distribution_version", json!("22.04"));
    facts.set("ansible_os_family", json!("Debian"));
    facts.set("ansible_kernel", json!("5.15.0-91-generic"));
    facts.set("ansible_architecture", json!("x86_64"));

    // Hardware facts
    facts.set("ansible_memtotal_mb", json!(32768));
    facts.set("ansible_processor_count", json!(16));
    facts.set("ansible_processor_cores", json!(8));
    facts.set("ansible_processor_threads_per_core", json!(2));

    // Network facts (large nested structure)
    let interfaces = json!({
        "eth0": {
            "ipv4": {"address": "10.0.0.1", "netmask": "255.255.255.0"},
            "ipv6": [{"address": "fe80::1", "prefix": 64}],
            "macaddress": "00:11:22:33:44:55",
            "mtu": 1500,
            "active": true,
            "speed": 10000
        },
        "eth1": {
            "ipv4": {"address": "192.168.1.1", "netmask": "255.255.255.0"},
            "macaddress": "00:11:22:33:44:56",
            "mtu": 9000,
            "active": true,
            "speed": 25000
        },
        "lo": {
            "ipv4": {"address": "127.0.0.1", "netmask": "255.0.0.0"},
            "active": true
        }
    });
    facts.set("ansible_interfaces", interfaces);

    // Disk facts
    let mounts = json!([
        {"mount": "/", "device": "/dev/sda1", "fstype": "ext4", "size_total": 500000000000_i64, "size_available": 350000000000_i64},
        {"mount": "/home", "device": "/dev/sda2", "fstype": "ext4", "size_total": 1000000000000_i64, "size_available": 800000000000_i64},
        {"mount": "/var", "device": "/dev/sda3", "fstype": "ext4", "size_total": 200000000000_i64, "size_available": 150000000000_i64}
    ]);
    facts.set("ansible_mounts", mounts);

    // Package facts (simulating many packages)
    let packages: HashMap<String, serde_json::Value> = (0..100)
        .map(|i| {
            (
                format!("package_{}", i),
                json!({"version": format!("1.0.{}", i), "arch": "amd64"}),
            )
        })
        .collect();
    facts.set("ansible_packages", json!(packages));

    facts
}

/// Generate module arguments of various sizes
fn generate_args(size: usize) -> HashMap<String, serde_json::Value> {
    let mut args = HashMap::new();
    for i in 0..size {
        args.insert(format!("arg_{}", i), json!(format!("value_{}", i)));
    }
    args
}

/// Generate host stats for multiple hosts
fn generate_host_stats(num_hosts: usize) -> HashMap<String, HostStats> {
    let mut stats = HashMap::new();
    for i in 0..num_hosts {
        let mut host_stats = HostStats::new();
        host_stats.ok = (i * 3) as u32;
        host_stats.changed = (i * 2) as u32;
        host_stats.failed = if i % 10 == 0 { 1 } else { 0 };
        host_stats.skipped = (i % 5) as u32;
        stats.insert(format!("host_{:04}", i), host_stats);
    }
    stats
}

fn configure_group(group: &mut BenchmarkGroup<'_, WallTime>, sample_size: usize) {
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(sample_size);
}

// ============================================================================
// 1. Callback Dispatch Overhead Benchmarks
// ============================================================================

fn bench_callback_dispatch_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("callback_dispatch_overhead");
    configure_group(&mut group, 20);

    // Baseline: NoOp callback
    let noop_callback = NoOpCallback;
    let simple_result = generate_simple_result();
    let args = generate_args(5);

    group.bench_function("noop_task_start", |b| {
        b.iter(|| {
            noop_callback.on_task_start(
                black_box("Test Task"),
                black_box("debug"),
                black_box(&args),
            );
        })
    });

    group.bench_function("noop_task_result", |b| {
        b.iter(|| {
            noop_callback.on_task_result(
                black_box("localhost"),
                black_box("Test Task"),
                black_box(&simple_result),
            );
        })
    });

    // Counting callback (minimal work)
    let counting_callback = CountingCallback::default();

    group.bench_function("counting_task_start", |b| {
        b.iter(|| {
            counting_callback.on_task_start(
                black_box("Test Task"),
                black_box("debug"),
                black_box(&args),
            );
        })
    });

    group.bench_function("counting_task_result", |b| {
        b.iter(|| {
            counting_callback.on_task_result(
                black_box("localhost"),
                black_box("Test Task"),
                black_box(&simple_result),
            );
        })
    });

    // Buffering callback (string formatting)
    let buffering_callback = BufferingCallback::default();

    group.bench_function("buffering_task_start", |b| {
        b.iter(|| {
            buffering_callback.on_task_start(
                black_box("Test Task"),
                black_box("debug"),
                black_box(&args),
            );
        })
    });

    group.bench_function("buffering_task_result", |b| {
        b.iter(|| {
            buffering_callback.on_task_result(
                black_box("localhost"),
                black_box("Test Task"),
                black_box(&simple_result),
            );
        })
    });

    // JSON serializing callback (most expensive)
    let json_callback = JsonSerializingCallback::default();

    group.bench_function("json_task_start", |b| {
        b.iter(|| {
            json_callback.on_task_start(
                black_box("Test Task"),
                black_box("debug"),
                black_box(&args),
            );
        })
    });

    group.bench_function("json_task_result", |b| {
        b.iter(|| {
            json_callback.on_task_result(
                black_box("localhost"),
                black_box("Test Task"),
                black_box(&simple_result),
            );
        })
    });

    // Event creation overhead
    group.bench_function("event_creation_task_start", |b| {
        let args = generate_args(5);
        b.iter(|| {
            let event = CallbackEvent::TaskStart {
                name: black_box("Test Task".to_string()),
                module: black_box("debug".to_string()),
                args: black_box(args.clone()),
            };
            black_box(event)
        })
    });

    group.bench_function("event_creation_task_result", |b| {
        let result = generate_simple_result();
        b.iter(|| {
            let event = CallbackEvent::TaskResult {
                host: black_box("localhost".to_string()),
                task_name: black_box("Test Task".to_string()),
                result: black_box(result.clone()),
            };
            black_box(event)
        })
    });

    // Event cloning overhead
    let event = CallbackEvent::TaskResult {
        host: "localhost".to_string(),
        task_name: "Test Task".to_string(),
        result: generate_complex_result(),
    };

    group.bench_function("event_clone_complex", |b| {
        b.iter(|| {
            let cloned = black_box(&event).clone();
            black_box(cloned)
        })
    });

    group.finish();
}

fn bench_async_callback_overhead(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("async_callback_overhead");
    configure_group(&mut group, 20);

    let callback = Arc::new(AsyncMockCallback::default());
    let hosts = vec!["host1".to_string(), "host2".to_string()];

    group.bench_function("async_playbook_start", |b| {
        let cb = callback.clone();
        b.to_async(&rt).iter(|| {
            let cb = cb.clone();
            async move {
                cb.on_playbook_start(black_box("test_playbook")).await;
            }
        })
    });

    group.bench_function("async_play_start", |b| {
        let cb = callback.clone();
        let hosts = hosts.clone();
        b.to_async(&rt).iter(|| {
            let cb = cb.clone();
            let hosts = hosts.clone();
            async move {
                cb.on_play_start(black_box("Test Play"), black_box(&hosts))
                    .await;
            }
        })
    });

    group.bench_function("async_task_start", |b| {
        let cb = callback.clone();
        b.to_async(&rt).iter(|| {
            let cb = cb.clone();
            async move {
                cb.on_task_start(black_box("Test Task"), black_box("localhost"))
                    .await;
            }
        })
    });

    group.bench_function("async_task_complete", |b| {
        let cb = callback.clone();
        b.to_async(&rt).iter(|| {
            let cb = cb.clone();
            async move {
                let result = ExecutionResult {
                    host: "localhost".to_string(),
                    task_name: "Test Task".to_string(),
                    result: ModuleResult::ok("Success"),
                    duration: Duration::from_millis(100),
                    notify: vec![],
                };
                cb.on_task_complete(black_box(&result)).await;
            }
        })
    });

    group.finish();
}

// ============================================================================
// 2. Multiple Plugin Performance Benchmarks
// ============================================================================

fn bench_multiple_plugins(c: &mut Criterion) {
    let mut group = c.benchmark_group("multiple_plugins");
    configure_group(&mut group, 20);

    let args = generate_args(5);
    let result = generate_simple_result();

    // Test with different numbers of plugins
    for num_plugins in [1, 2, 5, 10].iter() {
        group.throughput(Throughput::Elements(*num_plugins as u64));

        // All NoOp plugins (baseline)
        let mut aggregator = CallbackAggregator::new();
        for _ in 0..*num_plugins {
            aggregator.add(Arc::new(NoOpCallback));
        }

        group.bench_with_input(
            BenchmarkId::new("noop_plugins", num_plugins),
            num_plugins,
            |b, _| {
                b.iter(|| {
                    aggregator.dispatch_task_start(
                        black_box("Test Task"),
                        black_box("debug"),
                        black_box(&args),
                    );
                    aggregator.dispatch_task_result(
                        black_box("localhost"),
                        black_box("Test Task"),
                        black_box(&result),
                    );
                })
            },
        );

        // All Counting plugins
        let mut aggregator = CallbackAggregator::new();
        for _ in 0..*num_plugins {
            aggregator.add(Arc::new(CountingCallback::default()));
        }

        group.bench_with_input(
            BenchmarkId::new("counting_plugins", num_plugins),
            num_plugins,
            |b, _| {
                b.iter(|| {
                    aggregator.dispatch_task_start(
                        black_box("Test Task"),
                        black_box("debug"),
                        black_box(&args),
                    );
                    aggregator.dispatch_task_result(
                        black_box("localhost"),
                        black_box("Test Task"),
                        black_box(&result),
                    );
                })
            },
        );

        // Mixed plugins (realistic scenario)
        let mut aggregator = CallbackAggregator::new();
        aggregator.add(Arc::new(BufferingCallback::default())); // Console output
        aggregator.add(Arc::new(JsonSerializingCallback::default())); // JSON logging
        for _ in 2..*num_plugins {
            aggregator.add(Arc::new(CountingCallback::default())); // Metrics
        }

        group.bench_with_input(
            BenchmarkId::new("mixed_plugins", num_plugins),
            num_plugins,
            |b, _| {
                b.iter(|| {
                    aggregator.dispatch_task_start(
                        black_box("Test Task"),
                        black_box("debug"),
                        black_box(&args),
                    );
                    aggregator.dispatch_task_result(
                        black_box("localhost"),
                        black_box("Test Task"),
                        black_box(&result),
                    );
                })
            },
        );
    }

    // Event-based dispatch vs direct method calls
    let event = CallbackEvent::TaskStart {
        name: "Test Task".to_string(),
        module: "debug".to_string(),
        args: args.clone(),
    };

    let mut aggregator = CallbackAggregator::new();
    for _ in 0..5 {
        aggregator.add(Arc::new(BufferingCallback::default()));
    }

    group.bench_function("event_dispatch_5_plugins", |b| {
        b.iter(|| {
            aggregator.dispatch_event(black_box(&event));
        })
    });

    group.bench_function("direct_dispatch_5_plugins", |b| {
        b.iter(|| {
            aggregator.dispatch_task_start(
                black_box("Test Task"),
                black_box("debug"),
                black_box(&args),
            );
        })
    });

    group.finish();
}

fn bench_plugin_registration(c: &mut Criterion) {
    let mut group = c.benchmark_group("plugin_registration");
    configure_group(&mut group, 20);

    // Benchmark plugin addition
    for num_plugins in [1, 10, 50].iter() {
        group.bench_with_input(
            BenchmarkId::new("add_plugins", num_plugins),
            num_plugins,
            |b, &n| {
                b.iter(|| {
                    let mut aggregator = CallbackAggregator::new();
                    for _ in 0..n {
                        aggregator.add(Arc::new(CountingCallback::default()));
                    }
                    black_box(aggregator)
                })
            },
        );
    }

    // Benchmark lookup in plugin list
    let mut aggregator = CallbackAggregator::new();
    for i in 0..50 {
        if i % 2 == 0 {
            aggregator.add(Arc::new(CountingCallback::default()));
        } else {
            aggregator.add(Arc::new(BufferingCallback::default()));
        }
    }

    group.bench_function("find_plugin_by_name", |b| {
        b.iter(|| {
            let found = aggregator
                .callbacks
                .iter()
                .find(|c| c.name() == black_box("buffering"));
            black_box(found)
        })
    });

    group.finish();
}

// ============================================================================
// 3. Large Event Data Handling Benchmarks
// ============================================================================

fn bench_large_event_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_event_data");
    configure_group(&mut group, 20);

    // Different payload sizes
    for size in [1, 10, 50].iter() {
        let args = generate_args(*size);

        group.bench_with_input(BenchmarkId::new("args_size", size), size, |b, _| {
            let callback = BufferingCallback::default();
            b.iter(|| {
                callback.on_task_start(
                    black_box("Test Task"),
                    black_box("template"),
                    black_box(&args),
                );
            })
        });

        group.bench_with_input(BenchmarkId::new("args_size_json", size), size, |b, _| {
            let callback = JsonSerializingCallback::default();
            b.iter(|| {
                callback.on_task_start(
                    black_box("Test Task"),
                    black_box("template"),
                    black_box(&args),
                );
            })
        });
    }

    // Complex result with diff data
    let complex_result = generate_complex_result();
    let simple_result = generate_simple_result();

    group.bench_function("simple_result_dispatch", |b| {
        let callback = JsonSerializingCallback::default();
        b.iter(|| {
            callback.on_task_result(
                black_box("localhost"),
                black_box("Test Task"),
                black_box(&simple_result),
            );
        })
    });

    group.bench_function("complex_result_dispatch", |b| {
        let callback = JsonSerializingCallback::default();
        b.iter(|| {
            callback.on_task_result(
                black_box("localhost"),
                black_box("Test Task"),
                black_box(&complex_result),
            );
        })
    });

    // Large facts data
    let large_facts = generate_large_facts();

    group.bench_function("facts_event_creation", |b| {
        b.iter(|| {
            let event = CallbackEvent::Debug {
                host: Some(black_box("localhost".to_string())),
                message: black_box(format!("Gathered {} facts", large_facts.all().len())),
            };
            black_box(event)
        })
    });

    // Host stats with many hosts
    for num_hosts in [10, 100].iter() {
        let stats = generate_host_stats(*num_hosts);

        group.bench_with_input(
            BenchmarkId::new("playbook_end_hosts", num_hosts),
            num_hosts,
            |b, _| {
                let callback = BufferingCallback::default();
                b.iter(|| {
                    callback.on_playbook_end(
                        black_box("test.yml"),
                        black_box(Duration::from_secs(120)),
                        black_box(&stats),
                    );
                })
            },
        );
    }

    group.finish();
}

fn bench_serialization_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_overhead");
    configure_group(&mut group, 20);

    let result = generate_complex_result();

    // JSON serialization
    group.bench_function("result_to_json", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&result.result));
            black_box(json)
        })
    });

    group.bench_function("result_to_json_pretty", |b| {
        b.iter(|| {
            let json = serde_json::to_string_pretty(black_box(&result.result));
            black_box(json)
        })
    });

    // YAML serialization
    group.bench_function("result_to_yaml", |b| {
        b.iter(|| {
            let yaml = serde_yaml::to_string(black_box(&result.result));
            black_box(yaml)
        })
    });

    // TaskResultInfo cloning (happens during event creation)
    group.bench_function("result_clone_simple", |b| {
        let simple = generate_simple_result();
        b.iter(|| {
            let cloned = black_box(&simple).clone();
            black_box(cloned)
        })
    });

    group.bench_function("result_clone_complex", |b| {
        b.iter(|| {
            let cloned = black_box(&result).clone();
            black_box(cloned)
        })
    });

    // String formatting overhead
    group.bench_function("format_task_line", |b| {
        b.iter(|| {
            let line = format!(
                "TASK [{}] {}",
                black_box("Install nginx"),
                black_box("*".repeat(50))
            );
            black_box(line)
        })
    });

    group.bench_function("format_result_line", |b| {
        b.iter(|| {
            let line = format!(
                "changed: [{}] => (item={}) msg={}",
                black_box("localhost"),
                black_box("nginx"),
                black_box("Package installed")
            );
            black_box(line)
        })
    });

    group.finish();
}

// ============================================================================
// 4. Memory Usage During Execution Benchmarks
// ============================================================================

fn bench_callback_state_accumulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("callback_state_accumulation");
    configure_group(&mut group, 10);

    // Test event buffer growth
    for num_events in [100, 1000, 2500].iter() {
        let result = generate_simple_result();
        let args = generate_args(5);

        group.throughput(Throughput::Elements(*num_events as u64));
        group.bench_with_input(
            BenchmarkId::new("buffer_growth", num_events),
            num_events,
            |b, &n| {
                b.iter(|| {
                    let callback = BufferingCallback::default();
                    for i in 0..n {
                        callback.on_task_start(
                            black_box(&format!("Task {}", i)),
                            black_box("debug"),
                            black_box(&args),
                        );
                        callback.on_task_result(
                            black_box(&format!("host_{}", i % 10)),
                            black_box(&format!("Task {}", i)),
                            black_box(&result),
                        );
                    }
                    black_box(callback)
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("json_buffer_growth", num_events),
            num_events,
            |b, &n| {
                b.iter(|| {
                    let callback = JsonSerializingCallback::default();
                    for i in 0..n {
                        callback.on_task_start(
                            black_box(&format!("Task {}", i)),
                            black_box("debug"),
                            black_box(&args),
                        );
                        callback.on_task_result(
                            black_box(&format!("host_{}", i % 10)),
                            black_box(&format!("Task {}", i)),
                            black_box(&result),
                        );
                    }
                    black_box(callback)
                })
            },
        );
    }

    group.finish();
}

fn bench_host_stats_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("host_stats_aggregation");
    configure_group(&mut group, 15);

    // HostStats operations
    group.bench_function("hoststats_new", |b| {
        b.iter(|| {
            let stats = HostStats::new();
            black_box(stats)
        })
    });

    group.bench_function("hoststats_record", |b| {
        let mut stats = HostStats::new();
        b.iter(|| {
            stats.record(black_box(TaskStatus::Ok), black_box(false));
            stats.record(black_box(TaskStatus::Changed), black_box(true));
        })
    });

    group.bench_function("hoststats_total", |b| {
        let mut stats = HostStats::new();
        stats.ok = 50;
        stats.changed = 30;
        stats.failed = 2;
        stats.skipped = 10;
        b.iter(|| {
            let total = stats.total();
            black_box(total)
        })
    });

    // HashMap of host stats
    for num_hosts in [10, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("aggregate_host_stats", num_hosts),
            num_hosts,
            |b, &n| {
                b.iter(|| {
                    let mut all_stats: HashMap<String, HostStats> = HashMap::new();
                    for i in 0..n {
                        let mut stats = HostStats::new();
                        stats.record(TaskStatus::Ok, false);
                        stats.record(TaskStatus::Changed, true);
                        all_stats.insert(format!("host_{:04}", i), stats);
                    }
                    black_box(all_stats)
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("summarize_host_stats", num_hosts),
            num_hosts,
            |b, &n| {
                let stats = generate_host_stats(n);
                b.iter(|| {
                    let total_ok: u32 = stats.values().map(|s| s.ok).sum();
                    let total_changed: u32 = stats.values().map(|s| s.changed).sum();
                    let total_failed: u32 = stats.values().map(|s| s.failed).sum();
                    let has_failures = stats.values().any(|s| s.has_failures());
                    black_box((total_ok, total_changed, total_failed, has_failures))
                })
            },
        );
    }

    group.finish();
}

fn bench_buffer_flush(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_flush");
    configure_group(&mut group, 15);

    // Pre-populate buffer and measure flush
    for buffer_size in [100, 1000, 2500].iter() {
        let callback = BufferingCallback::default();
        let args = generate_args(5);
        let result = generate_simple_result();

        // Fill buffer
        for i in 0..*buffer_size {
            callback.on_task_start(&format!("Task {}", i), "debug", &args);
            callback.on_task_result(&format!("host_{}", i % 10), &format!("Task {}", i), &result);
        }

        group.bench_with_input(
            BenchmarkId::new("flush_buffer", buffer_size),
            buffer_size,
            |b, _| {
                b.iter(|| {
                    callback.flush();
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// 5. With/Without Callbacks Comparison
// ============================================================================

fn bench_with_without_callbacks(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("with_without_callbacks");
    configure_group(&mut group, 10);

    // Simulate task execution workload
    let num_tasks = 50;
    let num_hosts = 5;

    // Baseline: No callbacks at all
    group.bench_function("no_callbacks", |b| {
        b.to_async(&rt).iter(|| async move {
            for task_id in 0..num_tasks {
                for host_id in 0..num_hosts {
                    let _host = format!("host_{}", host_id);
                    let _task = format!("task_{}", task_id);
                    tokio::task::yield_now().await;
                }
            }
        })
    });

    // With NoOp callback
    let noop_callback = Arc::new(NoOpCallback);
    group.bench_function("with_noop_callback", |b| {
        let args = generate_args(5);
        let result = generate_simple_result();
        let cb = noop_callback.clone();
        b.to_async(&rt).iter(|| {
            let cb = cb.clone();
            let args = args.clone();
            let result = result.clone();
            async move {
                for task_id in 0..num_tasks {
                    cb.on_task_start(&format!("task_{}", task_id), "debug", &args);
                    for host_id in 0..num_hosts {
                        tokio::task::yield_now().await;
                        cb.on_task_result(
                            &format!("host_{}", host_id),
                            &format!("task_{}", task_id),
                            &result,
                        );
                    }
                }
            }
        })
    });

    // With Counting callback
    let counting_callback = Arc::new(CountingCallback::default());
    group.bench_function("with_counting_callback", |b| {
        let args = generate_args(5);
        let result = generate_simple_result();
        let cb = counting_callback.clone();
        b.to_async(&rt).iter(|| {
            let cb = cb.clone();
            let args = args.clone();
            let result = result.clone();
            async move {
                for task_id in 0..num_tasks {
                    cb.on_task_start(&format!("task_{}", task_id), "debug", &args);
                    for host_id in 0..num_hosts {
                        tokio::task::yield_now().await;
                        cb.on_task_result(
                            &format!("host_{}", host_id),
                            &format!("task_{}", task_id),
                            &result,
                        );
                    }
                }
            }
        })
    });

    // With Buffering callback (realistic)
    group.bench_function("with_buffering_callback", |b| {
        let args = generate_args(5);
        let result = generate_simple_result();
        b.to_async(&rt).iter(|| {
            let args = args.clone();
            let result = result.clone();
            async move {
                let callback = BufferingCallback::default();
                for task_id in 0..num_tasks {
                    callback.on_task_start(&format!("task_{}", task_id), "debug", &args);
                    for host_id in 0..num_hosts {
                        tokio::task::yield_now().await;
                        callback.on_task_result(
                            &format!("host_{}", host_id),
                            &format!("task_{}", task_id),
                            &result,
                        );
                    }
                }
                callback.flush();
            }
        })
    });

    // With JSON callback (most expensive)
    group.bench_function("with_json_callback", |b| {
        let args = generate_args(5);
        let result = generate_simple_result();
        b.to_async(&rt).iter(|| {
            let args = args.clone();
            let result = result.clone();
            async move {
                let callback = JsonSerializingCallback::default();
                for task_id in 0..num_tasks {
                    callback.on_task_start(&format!("task_{}", task_id), "debug", &args);
                    for host_id in 0..num_hosts {
                        tokio::task::yield_now().await;
                        callback.on_task_result(
                            &format!("host_{}", host_id),
                            &format!("task_{}", task_id),
                            &result,
                        );
                    }
                }
            }
        })
    });

    // With multiple callbacks (3 plugins)
    group.bench_function("with_3_callbacks", |b| {
        let args = generate_args(5);
        let result = generate_simple_result();
        b.to_async(&rt).iter(|| {
            let args = args.clone();
            let result = result.clone();
            async move {
                let mut aggregator = CallbackAggregator::new();
                aggregator.add(Arc::new(BufferingCallback::default()));
                aggregator.add(Arc::new(JsonSerializingCallback::default()));
                aggregator.add(Arc::new(CountingCallback::default()));

                for task_id in 0..num_tasks {
                    aggregator.dispatch_task_start(&format!("task_{}", task_id), "debug", &args);
                    for host_id in 0..num_hosts {
                        tokio::task::yield_now().await;
                        aggregator.dispatch_task_result(
                            &format!("host_{}", host_id),
                            &format!("task_{}", task_id),
                            &result,
                        );
                    }
                }
            }
        })
    });

    group.finish();
}

fn bench_callback_impact_on_parallelism(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("callback_parallelism_impact");
    configure_group(&mut group, 10);

    let num_tasks = 30;

    // Test different levels of parallelism
    for concurrency in [1, 5, 10].iter() {
        // Without callbacks
        group.bench_with_input(
            BenchmarkId::new("no_callbacks", concurrency),
            concurrency,
            |b, &conc| {
                b.to_async(&rt).iter(|| async move {
                    let mut handles = vec![];
                    for batch in (0..num_tasks).collect::<Vec<_>>().chunks(conc) {
                        for &task_id in batch {
                            handles.push(tokio::spawn(async move {
                                tokio::task::yield_now().await;
                                task_id
                            }));
                        }
                    }
                    for handle in handles {
                        black_box(handle.await).ok();
                    }
                })
            },
        );

        // With buffering callback (thread-safe via RwLock)
        group.bench_with_input(
            BenchmarkId::new("with_buffering_callback", concurrency),
            concurrency,
            |b, &conc| {
                let callback = Arc::new(BufferingCallback::default());
                let args = generate_args(3);
                let result = generate_simple_result();
                b.to_async(&rt).iter(|| {
                    let callback = callback.clone();
                    let args = args.clone();
                    let result = result.clone();
                    async move {
                        let mut handles = vec![];
                        for batch in (0..num_tasks).collect::<Vec<_>>().chunks(conc) {
                            for &task_id in batch {
                                let cb = callback.clone();
                                let args = args.clone();
                                let result = result.clone();
                                handles.push(tokio::spawn(async move {
                                    cb.on_task_start(&format!("task_{}", task_id), "debug", &args);
                                    tokio::task::yield_now().await;
                                    cb.on_task_result(
                                        "localhost",
                                        &format!("task_{}", task_id),
                                        &result,
                                    );
                                    task_id
                                }));
                            }
                        }
                        for handle in handles {
                            black_box(handle.await).ok();
                        }
                    }
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Criterion Groups and Main
// ============================================================================

criterion_group!(
    dispatch_benches,
    bench_callback_dispatch_overhead,
    bench_async_callback_overhead,
);

criterion_group!(
    multiple_plugin_benches,
    bench_multiple_plugins,
    bench_plugin_registration,
);

criterion_group!(
    large_data_benches,
    bench_large_event_data,
    bench_serialization_overhead,
);

criterion_group!(
    memory_benches,
    bench_callback_state_accumulation,
    bench_host_stats_aggregation,
    bench_buffer_flush,
);

criterion_group!(
    comparison_benches,
    bench_with_without_callbacks,
    bench_callback_impact_on_parallelism,
);

criterion_main!(
    dispatch_benches,
    multiple_plugin_benches,
    large_data_benches,
    memory_benches,
    comparison_benches,
);
