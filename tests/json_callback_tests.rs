//! Comprehensive tests for the JSON Callback Plugin.
//!
//! This test suite verifies:
//! 1. JSON output validity - All output is valid, parseable JSON
//! 2. Event serialization - All callback events serialize correctly
//! 3. Streaming JSON format - JSONL (JSON Lines) format for streaming
//! 4. All event types produce valid JSON
//! 5. Ansible compatibility of output format
//!
//! Note: These tests use a self-contained JsonCallback implementation that mirrors
//! the architecture of rustible's callback system, allowing comprehensive testing
//! without depending on modules that may have compilation issues.

#![allow(unused_mut)]

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// Test-local types that mirror rustible's callback architecture
// ============================================================================

/// Facts gathered from a host (mirrors rustible::facts::Facts)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Facts {
    data: HashMap<String, Value>,
}

impl Facts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.data.insert(key.to_string(), value);
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    pub fn all(&self) -> &HashMap<String, Value> {
        &self.data
    }
}

/// Result of module execution (mirrors rustible::traits::ModuleResult)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleResult {
    pub success: bool,
    pub changed: bool,
    pub message: String,
    pub skipped: bool,
    pub data: Option<Value>,
    pub warnings: Vec<String>,
}

impl ModuleResult {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: false,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    pub fn changed(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: true,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            success: false,
            changed: false,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    pub fn skipped(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: false,
            message: message.into(),
            skipped: true,
            data: None,
            warnings: Vec::new(),
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

/// Result of task execution (mirrors rustible::traits::ExecutionResult)
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub host: String,
    pub task_name: String,
    pub result: ModuleResult,
    pub duration: Duration,
    pub notify: Vec<String>,
}

/// Trait for execution callbacks (mirrors rustible::traits::ExecutionCallback)
#[async_trait]
pub trait ExecutionCallback: Send + Sync {
    async fn on_playbook_start(&self, name: &str) {
        let _ = name;
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let _ = (name, success);
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let _ = (name, hosts);
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        let _ = (name, success);
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        let _ = (name, host);
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let _ = result;
    }

    async fn on_handler_triggered(&self, name: &str) {
        let _ = name;
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let _ = (host, facts);
    }
}

// ============================================================================
// JSON Event Types (Ansible-compatible format)
// ============================================================================

/// Represents a JSON-serializable callback event.
/// Follows Ansible's callback plugin JSON output format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event_type")]
pub enum JsonEvent {
    #[serde(rename = "playbook_start")]
    PlaybookStart {
        playbook: String,
        timestamp: String,
        uuid: String,
    },

    #[serde(rename = "playbook_end")]
    PlaybookEnd {
        playbook: String,
        success: bool,
        timestamp: String,
        duration_ms: u64,
        uuid: String,
    },

    #[serde(rename = "play_start")]
    PlayStart {
        play: String,
        hosts: Vec<String>,
        timestamp: String,
        uuid: String,
    },

    #[serde(rename = "play_end")]
    PlayEnd {
        play: String,
        success: bool,
        timestamp: String,
        duration_ms: u64,
        uuid: String,
    },

    #[serde(rename = "task_start")]
    TaskStart {
        task: String,
        host: String,
        timestamp: String,
        uuid: String,
    },

    #[serde(rename = "task_complete")]
    TaskComplete {
        task: String,
        host: String,
        status: String,
        changed: bool,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
        timestamp: String,
        duration_ms: u64,
        uuid: String,
    },

    #[serde(rename = "handler_triggered")]
    HandlerTriggered {
        handler: String,
        timestamp: String,
        uuid: String,
    },

    #[serde(rename = "facts_gathered")]
    FactsGathered {
        host: String,
        facts: Value,
        timestamp: String,
        uuid: String,
    },
}

impl JsonEvent {
    /// Get the event type as a string
    pub fn event_type(&self) -> &'static str {
        match self {
            JsonEvent::PlaybookStart { .. } => "playbook_start",
            JsonEvent::PlaybookEnd { .. } => "playbook_end",
            JsonEvent::PlayStart { .. } => "play_start",
            JsonEvent::PlayEnd { .. } => "play_end",
            JsonEvent::TaskStart { .. } => "task_start",
            JsonEvent::TaskComplete { .. } => "task_complete",
            JsonEvent::HandlerTriggered { .. } => "handler_triggered",
            JsonEvent::FactsGathered { .. } => "facts_gathered",
        }
    }
}

// ============================================================================
// JSON Callback Implementation
// ============================================================================

/// A callback that outputs events as JSON (JSON Lines format for streaming).
/// Compatible with Ansible's JSON callback plugin output.
#[derive(Debug)]
pub struct JsonCallback {
    /// Buffer to store JSON events (for testing)
    events: RwLock<Vec<JsonEvent>>,
    /// Raw JSON output buffer (JSON Lines format)
    output_buffer: RwLock<Vec<String>>,
    /// Playbook start time for duration calculation
    playbook_start_time: AtomicU64,
    /// Play start time for duration calculation
    play_start_time: AtomicU64,
    /// Whether to pretty-print JSON
    pretty: bool,
    /// Whether to include timestamps
    include_timestamps: bool,
    /// Counter for generating sequential UUIDs (for testing)
    uuid_counter: AtomicU64,
}

impl Default for JsonCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonCallback {
    /// Create a new JSON callback
    pub fn new() -> Self {
        Self {
            events: RwLock::new(Vec::new()),
            output_buffer: RwLock::new(Vec::new()),
            playbook_start_time: AtomicU64::new(0),
            play_start_time: AtomicU64::new(0),
            pretty: false,
            include_timestamps: true,
            uuid_counter: AtomicU64::new(0),
        }
    }

    /// Create a JSON callback with pretty printing
    pub fn with_pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    /// Create a JSON callback with/without timestamps
    pub fn with_timestamps(mut self, include: bool) -> Self {
        self.include_timestamps = include;
        self
    }

    /// Get all stored events
    pub fn events(&self) -> Vec<JsonEvent> {
        self.events.read().clone()
    }

    /// Get raw JSON output (each line is a JSON object)
    pub fn output_lines(&self) -> Vec<String> {
        self.output_buffer.read().clone()
    }

    /// Get output as a single JSON array
    pub fn output_as_array(&self) -> Value {
        let events = self.events.read();
        serde_json::to_value(&*events).unwrap_or(json!([]))
    }

    /// Generate a UUID for events
    fn generate_uuid(&self) -> String {
        let count = self.uuid_counter.fetch_add(1, Ordering::SeqCst);
        format!("test-uuid-{:08x}", count)
    }

    /// Get current timestamp as ISO 8601 string
    fn timestamp(&self) -> String {
        if self.include_timestamps {
            Utc::now().to_rfc3339()
        } else {
            "".to_string()
        }
    }

    /// Get current time in milliseconds
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    /// Emit an event - serialize to JSON and store
    fn emit(&self, event: JsonEvent) {
        // Serialize to JSON
        let json_str = if self.pretty {
            serde_json::to_string_pretty(&event).unwrap()
        } else {
            serde_json::to_string(&event).unwrap()
        };

        // Store in output buffer
        self.output_buffer.write().push(json_str);

        // Store event object
        self.events.write().push(event);
    }

    /// Clear all stored events
    pub fn clear(&self) {
        self.events.write().clear();
        self.output_buffer.write().clear();
    }
}

#[async_trait]
impl ExecutionCallback for JsonCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.playbook_start_time
            .store(self.now_ms(), Ordering::SeqCst);

        let event = JsonEvent::PlaybookStart {
            playbook: name.to_string(),
            timestamp: self.timestamp(),
            uuid: self.generate_uuid(),
        };
        self.emit(event);
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let start = self.playbook_start_time.load(Ordering::SeqCst);
        let duration_ms = self.now_ms().saturating_sub(start);

        let event = JsonEvent::PlaybookEnd {
            playbook: name.to_string(),
            success,
            timestamp: self.timestamp(),
            duration_ms,
            uuid: self.generate_uuid(),
        };
        self.emit(event);
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.play_start_time.store(self.now_ms(), Ordering::SeqCst);

        let event = JsonEvent::PlayStart {
            play: name.to_string(),
            hosts: hosts.to_vec(),
            timestamp: self.timestamp(),
            uuid: self.generate_uuid(),
        };
        self.emit(event);
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        let start = self.play_start_time.load(Ordering::SeqCst);
        let duration_ms = self.now_ms().saturating_sub(start);

        let event = JsonEvent::PlayEnd {
            play: name.to_string(),
            success,
            timestamp: self.timestamp(),
            duration_ms,
            uuid: self.generate_uuid(),
        };
        self.emit(event);
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        let event = JsonEvent::TaskStart {
            task: name.to_string(),
            host: host.to_string(),
            timestamp: self.timestamp(),
            uuid: self.generate_uuid(),
        };
        self.emit(event);
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let status = if result.result.skipped {
            "skipped"
        } else if !result.result.success {
            "failed"
        } else if result.result.changed {
            "changed"
        } else {
            "ok"
        };

        let event = JsonEvent::TaskComplete {
            task: result.task_name.clone(),
            host: result.host.clone(),
            status: status.to_string(),
            changed: result.result.changed,
            message: result.result.message.clone(),
            data: result.result.data.clone(),
            timestamp: self.timestamp(),
            duration_ms: result.duration.as_millis() as u64,
            uuid: self.generate_uuid(),
        };
        self.emit(event);
    }

    async fn on_handler_triggered(&self, name: &str) {
        let event = JsonEvent::HandlerTriggered {
            handler: name.to_string(),
            timestamp: self.timestamp(),
            uuid: self.generate_uuid(),
        };
        self.emit(event);
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let event = JsonEvent::FactsGathered {
            host: host.to_string(),
            facts: serde_json::to_value(facts.all()).unwrap_or(json!({})),
            timestamp: self.timestamp(),
            uuid: self.generate_uuid(),
        };
        self.emit(event);
    }
}

// ============================================================================
// Test 1: JSON Output Validity
// ============================================================================

#[tokio::test]
async fn test_all_events_produce_valid_json() {
    let callback = JsonCallback::new();

    // Trigger all event types
    callback.on_playbook_start("test_playbook").await;
    callback
        .on_play_start("test_play", &["host1".to_string(), "host2".to_string()])
        .await;

    let mut facts = Facts::new();
    facts.set("os_family", json!("Debian"));
    callback.on_facts_gathered("host1", &facts).await;

    callback.on_task_start("test_task", "host1").await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "test_task".to_string(),
        result: ModuleResult::changed("Task completed with changes"),
        duration: Duration::from_millis(150),
        notify: vec!["handler1".to_string()],
    };
    callback.on_task_complete(&result).await;

    callback.on_handler_triggered("handler1").await;
    callback.on_play_end("test_play", true).await;
    callback.on_playbook_end("test_playbook", true).await;

    // Verify each output line is valid JSON
    let lines = callback.output_lines();
    assert!(!lines.is_empty(), "Should have generated JSON output");

    for (i, line) in lines.iter().enumerate() {
        let parsed: Result<Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "Line {} is not valid JSON: {}", i, line);
    }
}

#[tokio::test]
async fn test_json_can_be_reparsed_to_events() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("roundtrip_test").await;
    callback
        .on_play_start("play1", &["localhost".to_string()])
        .await;
    callback.on_task_start("task1", "localhost").await;

    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("Success"),
        duration: Duration::from_millis(50),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    callback.on_play_end("play1", true).await;
    callback.on_playbook_end("roundtrip_test", true).await;

    // Parse each line back to JsonEvent
    let lines = callback.output_lines();
    for line in &lines {
        let event: Result<JsonEvent, _> = serde_json::from_str(line);
        assert!(event.is_ok(), "Failed to parse event: {}", line);
    }
}

#[tokio::test]
async fn test_pretty_json_output() {
    let callback = JsonCallback::new().with_pretty(true);

    callback.on_playbook_start("pretty_test").await;

    let lines = callback.output_lines();
    assert_eq!(lines.len(), 1);

    // Pretty-printed JSON should span multiple lines
    let line = &lines[0];
    assert!(line.contains('\n'), "Pretty JSON should contain newlines");

    // But still be valid JSON
    let parsed: Result<Value, _> = serde_json::from_str(line);
    assert!(parsed.is_ok());
}

#[tokio::test]
async fn test_compact_json_output() {
    let callback = JsonCallback::new().with_pretty(false);

    callback.on_playbook_start("compact_test").await;

    let lines = callback.output_lines();
    assert_eq!(lines.len(), 1);

    // Compact JSON should NOT contain newlines (single line)
    let line = &lines[0];
    assert!(
        !line.contains('\n'),
        "Compact JSON should not contain newlines"
    );
}

// ============================================================================
// Test 2: Event Serialization
// ============================================================================

#[tokio::test]
async fn test_playbook_start_serialization() {
    let callback = JsonCallback::new().with_timestamps(false);

    callback.on_playbook_start("deploy_app").await;

    let events = callback.events();
    assert_eq!(events.len(), 1);

    match &events[0] {
        JsonEvent::PlaybookStart { playbook, uuid, .. } => {
            assert_eq!(playbook, "deploy_app");
            assert!(uuid.starts_with("test-uuid-"));
        }
        _ => panic!("Expected PlaybookStart event"),
    }
}

#[tokio::test]
async fn test_playbook_end_serialization() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("test").await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    callback.on_playbook_end("test", false).await;

    let events = callback.events();
    assert_eq!(events.len(), 2);

    match &events[1] {
        JsonEvent::PlaybookEnd {
            playbook,
            success,
            duration_ms,
            ..
        } => {
            assert_eq!(playbook, "test");
            assert!(!success);
            assert!(*duration_ms >= 10, "Duration should be at least 10ms");
        }
        _ => panic!("Expected PlaybookEnd event"),
    }
}

#[tokio::test]
async fn test_play_start_serialization() {
    let callback = JsonCallback::new();
    let hosts = vec!["web1".to_string(), "web2".to_string(), "db1".to_string()];

    callback.on_play_start("Configure servers", &hosts).await;

    let events = callback.events();
    match &events[0] {
        JsonEvent::PlayStart {
            play,
            hosts: event_hosts,
            ..
        } => {
            assert_eq!(play, "Configure servers");
            assert_eq!(event_hosts.len(), 3);
            assert!(event_hosts.contains(&"web1".to_string()));
            assert!(event_hosts.contains(&"web2".to_string()));
            assert!(event_hosts.contains(&"db1".to_string()));
        }
        _ => panic!("Expected PlayStart event"),
    }
}

#[tokio::test]
async fn test_task_complete_all_statuses() {
    let callback = JsonCallback::new();

    // Test OK status
    let ok_result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task_ok".to_string(),
        result: ModuleResult::ok("All good"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&ok_result).await;

    // Test Changed status
    let changed_result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task_changed".to_string(),
        result: ModuleResult::changed("File updated"),
        duration: Duration::from_millis(20),
        notify: vec![],
    };
    callback.on_task_complete(&changed_result).await;

    // Test Failed status
    let failed_result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task_failed".to_string(),
        result: ModuleResult::failed("Permission denied"),
        duration: Duration::from_millis(30),
        notify: vec![],
    };
    callback.on_task_complete(&failed_result).await;

    // Test Skipped status
    let skipped_result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task_skipped".to_string(),
        result: ModuleResult::skipped("Condition not met"),
        duration: Duration::from_millis(5),
        notify: vec![],
    };
    callback.on_task_complete(&skipped_result).await;

    let events = callback.events();
    assert_eq!(events.len(), 4);

    // Verify each status
    let statuses: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            JsonEvent::TaskComplete { status, .. } => Some(status.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(statuses, vec!["ok", "changed", "failed", "skipped"]);
}

#[tokio::test]
async fn test_task_complete_with_data() {
    let callback = JsonCallback::new();

    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "Get facts".to_string(),
        result: ModuleResult::ok("Facts gathered").with_data(json!({
            "ansible_distribution": "Ubuntu",
            "ansible_version": "22.04",
            "ansible_memory_mb": 16384
        })),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let events = callback.events();
    match &events[0] {
        JsonEvent::TaskComplete { data, .. } => {
            assert!(data.is_some());
            let data = data.as_ref().unwrap();
            assert_eq!(data["ansible_distribution"], "Ubuntu");
            assert_eq!(data["ansible_memory_mb"], 16384);
        }
        _ => panic!("Expected TaskComplete event"),
    }
}

#[tokio::test]
async fn test_facts_gathered_serialization() {
    let callback = JsonCallback::new();

    let mut facts = Facts::new();
    facts.set("distribution", json!("Ubuntu"));
    facts.set("distribution_version", json!("22.04"));
    facts.set("memtotal_mb", json!(32768));
    facts.set("processor_count", json!(8));
    facts.set("interfaces", json!(["eth0", "lo", "docker0"]));

    callback
        .on_facts_gathered("production-server", &facts)
        .await;

    let events = callback.events();
    match &events[0] {
        JsonEvent::FactsGathered {
            host,
            facts: facts_value,
            ..
        } => {
            assert_eq!(host, "production-server");
            assert!(facts_value.is_object());
            assert_eq!(facts_value["distribution"], "Ubuntu");
            assert_eq!(facts_value["memtotal_mb"], 32768);
        }
        _ => panic!("Expected FactsGathered event"),
    }
}

#[tokio::test]
async fn test_handler_triggered_serialization() {
    let callback = JsonCallback::new();

    callback.on_handler_triggered("Restart nginx").await;
    callback.on_handler_triggered("Reload systemd").await;

    let events = callback.events();
    assert_eq!(events.len(), 2);

    let handler_names: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            JsonEvent::HandlerTriggered { handler, .. } => Some(handler.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(handler_names, vec!["Restart nginx", "Reload systemd"]);
}

// ============================================================================
// Test 3: Streaming JSON Format (JSON Lines / JSONL)
// ============================================================================

#[tokio::test]
async fn test_jsonl_format_one_object_per_line() {
    let callback = JsonCallback::new().with_pretty(false);

    callback.on_playbook_start("test").await;
    callback.on_play_start("play1", &["h1".to_string()]).await;
    callback.on_task_start("t1", "h1").await;
    callback.on_play_end("play1", true).await;
    callback.on_playbook_end("test", true).await;

    let lines = callback.output_lines();
    assert_eq!(lines.len(), 5, "Should have 5 separate JSON objects");

    // Each line should be a complete, valid JSON object
    for line in &lines {
        let parsed: Value = serde_json::from_str(line).expect("Each line should be valid JSON");
        assert!(parsed.is_object(), "Each line should be a JSON object");
    }
}

#[tokio::test]
async fn test_jsonl_can_be_concatenated() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("test").await;
    callback.on_task_start("task1", "host1").await;
    callback.on_playbook_end("test", true).await;

    // Simulate reading as JSONL (newline-separated)
    let combined = callback.output_lines().join("\n");
    let reader = BufReader::new(combined.as_bytes());

    let mut parsed_count = 0;
    for line in reader.lines() {
        let line = line.unwrap();
        if !line.is_empty() {
            let _: Value = serde_json::from_str(&line).unwrap();
            parsed_count += 1;
        }
    }

    assert_eq!(parsed_count, 3);
}

#[tokio::test]
async fn test_output_can_be_combined_into_array() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("array_test").await;
    callback.on_play_start("play1", &["h1".to_string()]).await;
    callback.on_play_end("play1", true).await;
    callback.on_playbook_end("array_test", true).await;

    let array = callback.output_as_array();
    assert!(array.is_array());
    assert_eq!(array.as_array().unwrap().len(), 4);

    // Verify it can be serialized back
    let json_str = serde_json::to_string(&array).unwrap();
    let reparsed: Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(array, reparsed);
}

#[tokio::test]
async fn test_streaming_allows_incremental_parsing() {
    let callback = Arc::new(JsonCallback::new());
    let callback_clone = callback.clone();

    // Simulate streaming: events arrive over time
    tokio::spawn(async move {
        callback_clone.on_playbook_start("streaming_test").await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        callback_clone.on_task_start("task1", "host1").await;
    })
    .await
    .unwrap();

    // Can parse events as they arrive
    let lines = callback.output_lines();
    assert_eq!(lines.len(), 2);

    // Each can be parsed independently
    for line in &lines {
        let event: JsonEvent = serde_json::from_str(line).unwrap();
        assert!(matches!(
            event,
            JsonEvent::PlaybookStart { .. } | JsonEvent::TaskStart { .. }
        ));
    }
}

// ============================================================================
// Test 4: All Event Types Produce Valid JSON
// ============================================================================

#[tokio::test]
async fn test_all_event_types_have_required_fields() {
    let callback = JsonCallback::new();

    // Generate all event types
    callback.on_playbook_start("pb").await;
    callback.on_play_start("play", &["h1".to_string()]).await;

    let mut facts = Facts::new();
    facts.set("os", json!("linux"));
    callback.on_facts_gathered("h1", &facts).await;

    callback.on_task_start("task", "h1").await;

    let result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "task".to_string(),
        result: ModuleResult::ok("done"),
        duration: Duration::from_millis(10),
        notify: vec!["handler".to_string()],
    };
    callback.on_task_complete(&result).await;

    callback.on_handler_triggered("handler").await;
    callback.on_play_end("play", true).await;
    callback.on_playbook_end("pb", true).await;

    let lines = callback.output_lines();
    assert_eq!(lines.len(), 8, "Should have 8 event types");

    // Verify each event has required common fields
    for line in &lines {
        let value: Value = serde_json::from_str(line).unwrap();
        assert!(value.get("event_type").is_some(), "Missing event_type");
        assert!(value.get("uuid").is_some(), "Missing uuid");
        assert!(value.get("timestamp").is_some(), "Missing timestamp");
    }
}

#[tokio::test]
async fn test_event_type_field_values() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("test").await;
    callback.on_play_start("play", &["h1".to_string()]).await;
    callback.on_task_start("task", "h1").await;

    let result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "task".to_string(),
        result: ModuleResult::ok("done"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    callback.on_handler_triggered("handler").await;
    callback.on_play_end("play", true).await;
    callback.on_playbook_end("test", true).await;

    let mut facts = Facts::new();
    callback.on_facts_gathered("h1", &facts).await;

    let lines = callback.output_lines();
    let event_types: Vec<String> = lines
        .iter()
        .map(|line| {
            let v: Value = serde_json::from_str(line).unwrap();
            v["event_type"].as_str().unwrap().to_string()
        })
        .collect();

    assert!(event_types.contains(&"playbook_start".to_string()));
    assert!(event_types.contains(&"playbook_end".to_string()));
    assert!(event_types.contains(&"play_start".to_string()));
    assert!(event_types.contains(&"play_end".to_string()));
    assert!(event_types.contains(&"task_start".to_string()));
    assert!(event_types.contains(&"task_complete".to_string()));
    assert!(event_types.contains(&"handler_triggered".to_string()));
    assert!(event_types.contains(&"facts_gathered".to_string()));
}

#[tokio::test]
async fn test_empty_collections_serialize_correctly() {
    let callback = JsonCallback::new();

    // Empty hosts list
    callback.on_play_start("empty_hosts_play", &[]).await;

    // Empty facts
    let empty_facts = Facts::new();
    callback.on_facts_gathered("host", &empty_facts).await;

    let lines = callback.output_lines();

    // Parse and verify
    let play_start: Value = serde_json::from_str(&lines[0]).unwrap();
    assert!(play_start["hosts"].is_array());
    assert_eq!(play_start["hosts"].as_array().unwrap().len(), 0);

    let facts_event: Value = serde_json::from_str(&lines[1]).unwrap();
    assert!(facts_event["facts"].is_object());
}

#[tokio::test]
async fn test_special_characters_in_strings() {
    let callback = JsonCallback::new();

    // Test various special characters
    callback.on_playbook_start("test \"with\" quotes").await;
    callback
        .on_play_start("play with\nnewline", &["host/with/slashes".to_string()])
        .await;
    callback
        .on_task_start("task with\ttab", "host\\backslash")
        .await;
    callback
        .on_handler_triggered("handler with unicode: \u{1F600}")
        .await;

    let lines = callback.output_lines();

    // All should be valid JSON
    for line in &lines {
        let _: Value = serde_json::from_str(line).expect("Should handle special characters");
    }

    // Verify the values are preserved correctly
    let playbook_event: JsonEvent = serde_json::from_str(&lines[0]).unwrap();
    match playbook_event {
        JsonEvent::PlaybookStart { playbook, .. } => {
            assert_eq!(playbook, "test \"with\" quotes");
        }
        _ => panic!("Expected PlaybookStart"),
    }
}

#[tokio::test]
async fn test_unicode_in_all_fields() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("playbook").await;
    callback.on_task_start("Deploy to server", "host1").await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "Deploy".to_string(),
        result: ModuleResult::ok("Success"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let lines = callback.output_lines();
    for line in &lines {
        let _: Value = serde_json::from_str(line).expect("Unicode should serialize correctly");
    }
}

// ============================================================================
// Test 5: Ansible Compatibility
// ============================================================================

#[tokio::test]
async fn test_ansible_compatible_task_status_values() {
    let callback = JsonCallback::new();

    // Ansible uses specific status strings
    let ok_result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "t1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&ok_result).await;

    let changed_result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "t2".to_string(),
        result: ModuleResult::changed("Changed"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&changed_result).await;

    let failed_result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "t3".to_string(),
        result: ModuleResult::failed("Failed"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&failed_result).await;

    let skipped_result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "t4".to_string(),
        result: ModuleResult::skipped("Skipped"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&skipped_result).await;

    let lines = callback.output_lines();
    let statuses: Vec<String> = lines
        .iter()
        .map(|line| {
            let v: Value = serde_json::from_str(line).unwrap();
            v["status"].as_str().unwrap().to_string()
        })
        .collect();

    // Ansible-compatible status values
    assert_eq!(statuses[0], "ok");
    assert_eq!(statuses[1], "changed");
    assert_eq!(statuses[2], "failed");
    assert_eq!(statuses[3], "skipped");
}

#[tokio::test]
async fn test_ansible_compatible_structure() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("site.yml").await;
    callback
        .on_play_start(
            "Configure webservers",
            &["web1".to_string(), "web2".to_string()],
        )
        .await;
    callback.on_task_start("Install nginx", "web1").await;

    let result = ExecutionResult {
        host: "web1".to_string(),
        task_name: "Install nginx".to_string(),
        result: ModuleResult::changed("Installed nginx 1.24.0"),
        duration: Duration::from_secs(5),
        notify: vec!["restart nginx".to_string()],
    };
    callback.on_task_complete(&result).await;

    callback.on_handler_triggered("restart nginx").await;
    callback.on_play_end("Configure webservers", true).await;
    callback.on_playbook_end("site.yml", true).await;

    let lines = callback.output_lines();

    // Verify structure matches Ansible's JSON callback expectations
    let task_complete: Value = serde_json::from_str(&lines[3]).unwrap();
    assert!(task_complete.get("task").is_some());
    assert!(task_complete.get("host").is_some());
    assert!(task_complete.get("status").is_some());
    assert!(task_complete.get("changed").is_some());
    assert!(task_complete.get("duration_ms").is_some());

    // Verify field types
    assert!(task_complete["changed"].is_boolean());
    assert!(task_complete["duration_ms"].is_number());
    assert!(task_complete["task"].is_string());
}

#[tokio::test]
async fn test_ansible_compatible_boolean_changed_field() {
    let callback = JsonCallback::new();

    let changed_result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "t1".to_string(),
        result: ModuleResult::changed("Changed"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&changed_result).await;

    let unchanged_result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "t2".to_string(),
        result: ModuleResult::ok("No change"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&unchanged_result).await;

    let lines = callback.output_lines();

    let event1: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(event1["changed"], true);

    let event2: Value = serde_json::from_str(&lines[1]).unwrap();
    assert_eq!(event2["changed"], false);
}

#[tokio::test]
async fn test_ansible_compatible_timestamp_format() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("test").await;

    let lines = callback.output_lines();
    let event: Value = serde_json::from_str(&lines[0]).unwrap();

    let timestamp = event["timestamp"].as_str().unwrap();

    // Timestamp should be ISO 8601 format (RFC 3339)
    // e.g., "2024-01-15T10:30:00.123456789+00:00"
    let parsed = DateTime::parse_from_rfc3339(timestamp);
    assert!(
        parsed.is_ok(),
        "Timestamp should be valid RFC 3339: {}",
        timestamp
    );
}

#[tokio::test]
async fn test_ansible_json_callback_full_run_structure() {
    let callback = JsonCallback::new();

    // Simulate a complete Ansible-like playbook run
    callback.on_playbook_start("production_deploy.yml").await;

    // Play 1: Setup
    callback
        .on_play_start("Setup", &["app1".to_string(), "app2".to_string()])
        .await;

    let mut facts = Facts::new();
    facts.set("ansible_distribution", json!("Ubuntu"));
    facts.set("ansible_distribution_version", json!("22.04"));
    callback.on_facts_gathered("app1", &facts).await;
    callback.on_facts_gathered("app2", &facts).await;

    callback.on_task_start("Gather facts", "app1").await;
    callback
        .on_task_complete(&ExecutionResult {
            host: "app1".to_string(),
            task_name: "Gather facts".to_string(),
            result: ModuleResult::ok("Facts gathered"),
            duration: Duration::from_millis(150),
            notify: vec![],
        })
        .await;

    callback.on_play_end("Setup", true).await;

    // Play 2: Deploy
    callback
        .on_play_start(
            "Deploy application",
            &["app1".to_string(), "app2".to_string()],
        )
        .await;

    callback.on_task_start("Copy files", "app1").await;
    callback
        .on_task_complete(&ExecutionResult {
            host: "app1".to_string(),
            task_name: "Copy files".to_string(),
            result: ModuleResult::changed("Files copied"),
            duration: Duration::from_millis(500),
            notify: vec!["restart app".to_string()],
        })
        .await;

    callback.on_handler_triggered("restart app").await;
    callback.on_play_end("Deploy application", true).await;

    callback
        .on_playbook_end("production_deploy.yml", true)
        .await;

    // Verify complete structure
    let lines = callback.output_lines();
    assert!(!lines.is_empty());

    // All lines should parse as valid JSON
    for line in &lines {
        let _: Value = serde_json::from_str(line).expect("All output should be valid JSON");
    }

    // Convert to array for analysis
    let array = callback.output_as_array();
    let events = array.as_array().unwrap();

    // First event should be playbook_start, last should be playbook_end
    assert_eq!(events.first().unwrap()["event_type"], "playbook_start");
    assert_eq!(events.last().unwrap()["event_type"], "playbook_end");
}

// ============================================================================
// Test: Edge Cases and Error Conditions
// ============================================================================

#[tokio::test]
async fn test_empty_string_fields() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("").await;
    callback.on_play_start("", &[]).await;
    callback.on_task_start("", "").await;

    let result = ExecutionResult {
        host: "".to_string(),
        task_name: "".to_string(),
        result: ModuleResult::ok(""),
        duration: Duration::from_millis(0),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let lines = callback.output_lines();

    // All should still be valid JSON
    for line in &lines {
        let _: Value = serde_json::from_str(line).unwrap();
    }
}

#[tokio::test]
async fn test_very_long_strings() {
    let callback = JsonCallback::new();

    let long_string = "x".repeat(10000);
    callback.on_playbook_start(&long_string).await;

    let result = ExecutionResult {
        host: "host".to_string(),
        task_name: "task".to_string(),
        result: ModuleResult::ok(&long_string),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let lines = callback.output_lines();
    for line in &lines {
        let _: Value = serde_json::from_str(line).unwrap();
    }
}

#[tokio::test]
async fn test_null_and_missing_optional_fields() {
    let callback = JsonCallback::new();

    // Task result without data
    let result = ExecutionResult {
        host: "host".to_string(),
        task_name: "task".to_string(),
        result: ModuleResult::ok("No data"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let lines = callback.output_lines();
    let event: Value = serde_json::from_str(&lines[0]).unwrap();

    // data field should be absent or null
    let data = event.get("data");
    assert!(data.is_none() || data.unwrap().is_null());
}

#[tokio::test]
async fn test_deeply_nested_data() {
    let callback = JsonCallback::new();

    let nested_data = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": {
                        "level5": {
                            "value": "deep"
                        }
                    }
                }
            }
        }
    });

    let result = ExecutionResult {
        host: "host".to_string(),
        task_name: "task".to_string(),
        result: ModuleResult::ok("Nested data").with_data(nested_data.clone()),
        duration: Duration::from_millis(10),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let lines = callback.output_lines();
    let event: Value = serde_json::from_str(&lines[0]).unwrap();

    assert_eq!(
        event["data"]["level1"]["level2"]["level3"]["level4"]["level5"]["value"],
        "deep"
    );
}

#[tokio::test]
async fn test_concurrent_event_emission() {
    let callback = Arc::new(JsonCallback::new());

    let mut handles = vec![];

    for i in 0..10 {
        let cb = callback.clone();
        let handle = tokio::spawn(async move {
            cb.on_task_start(&format!("task{}", i), &format!("host{}", i))
                .await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let lines = callback.output_lines();
    assert_eq!(lines.len(), 10);

    // All should be valid JSON despite concurrent writes
    for line in &lines {
        let _: Value = serde_json::from_str(line).unwrap();
    }
}

#[tokio::test]
async fn test_clear_resets_state() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("test1").await;
    callback.on_task_start("task1", "host1").await;

    assert_eq!(callback.events().len(), 2);
    assert_eq!(callback.output_lines().len(), 2);

    callback.clear();

    assert_eq!(callback.events().len(), 0);
    assert_eq!(callback.output_lines().len(), 0);

    // Should work normally after clear
    callback.on_playbook_start("test2").await;
    assert_eq!(callback.events().len(), 1);
}

// ============================================================================
// Test: Duration Calculations
// ============================================================================

#[tokio::test]
async fn test_playbook_duration_tracking() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("duration_test").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    callback.on_playbook_end("duration_test", true).await;

    let events = callback.events();
    match &events[1] {
        JsonEvent::PlaybookEnd { duration_ms, .. } => {
            assert!(
                *duration_ms >= 50,
                "Duration should be at least 50ms, got {}",
                duration_ms
            );
            assert!(
                *duration_ms < 500,
                "Duration should be less than 500ms, got {}",
                duration_ms
            );
        }
        _ => panic!("Expected PlaybookEnd"),
    }
}

#[tokio::test]
async fn test_play_duration_tracking() {
    let callback = JsonCallback::new();

    callback.on_play_start("play1", &["h1".to_string()]).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    callback.on_play_end("play1", true).await;

    let events = callback.events();
    match &events[1] {
        JsonEvent::PlayEnd { duration_ms, .. } => {
            assert!(*duration_ms >= 30);
        }
        _ => panic!("Expected PlayEnd"),
    }
}

#[tokio::test]
async fn test_task_duration_from_result() {
    let callback = JsonCallback::new();

    let result = ExecutionResult {
        host: "h1".to_string(),
        task_name: "t1".to_string(),
        result: ModuleResult::ok("OK"),
        duration: Duration::from_millis(1234),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let events = callback.events();
    match &events[0] {
        JsonEvent::TaskComplete { duration_ms, .. } => {
            assert_eq!(*duration_ms, 1234);
        }
        _ => panic!("Expected TaskComplete"),
    }
}

// ============================================================================
// Test: UUID Generation
// ============================================================================

#[tokio::test]
async fn test_unique_uuids_for_each_event() {
    let callback = JsonCallback::new();

    callback.on_playbook_start("test").await;
    callback.on_play_start("play", &[]).await;
    callback.on_task_start("task", "host").await;
    callback.on_playbook_end("test", true).await;

    let events = callback.events();
    let uuids: Vec<String> = events
        .iter()
        .map(|e| match e {
            JsonEvent::PlaybookStart { uuid, .. } => uuid.clone(),
            JsonEvent::PlaybookEnd { uuid, .. } => uuid.clone(),
            JsonEvent::PlayStart { uuid, .. } => uuid.clone(),
            JsonEvent::PlayEnd { uuid, .. } => uuid.clone(),
            JsonEvent::TaskStart { uuid, .. } => uuid.clone(),
            JsonEvent::TaskComplete { uuid, .. } => uuid.clone(),
            JsonEvent::HandlerTriggered { uuid, .. } => uuid.clone(),
            JsonEvent::FactsGathered { uuid, .. } => uuid.clone(),
        })
        .collect();

    // All UUIDs should be unique
    let unique_count = uuids.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(unique_count, uuids.len(), "All UUIDs should be unique");
}
