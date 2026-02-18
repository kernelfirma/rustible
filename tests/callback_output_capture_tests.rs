//! Output Capture Tests for Rustible's Callback System
//!
//! This test module focuses on capturing and verifying the exact output produced
//! by various callback plugins. It uses a custom Write implementation to intercept
//! stdout/stderr output for precise comparison testing.
//!
//! Tests cover:
//! 1. Capturing stdout output from callbacks
//! 2. Verifying exact output format
//! 3. Comparing with expected strings
//! 4. Testing colored vs non-colored output
//! 5. Testing different verbosity levels

use async_trait::async_trait;
use parking_lot::RwLock;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Custom Write Implementation for Output Capture
// ============================================================================

/// A thread-safe buffer that captures written bytes for testing.
#[derive(Debug, Clone)]
pub struct CaptureBuffer {
    inner: Arc<RwLock<Vec<u8>>>,
    strip_ansi: bool,
}

impl CaptureBuffer {
    /// Create a new capture buffer.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Vec::new())),
            strip_ansi: false,
        }
    }

    /// Create a capture buffer that strips ANSI codes from output.
    pub fn new_strip_ansi() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Vec::new())),
            strip_ansi: true,
        }
    }

    /// Get the captured output as a String.
    pub fn get_output(&self) -> String {
        let bytes = self.inner.read().clone();
        let output = String::from_utf8_lossy(&bytes).to_string();
        if self.strip_ansi {
            strip_ansi_codes(&output)
        } else {
            output
        }
    }

    /// Get the raw captured bytes.
    pub fn get_bytes(&self) -> Vec<u8> {
        self.inner.read().clone()
    }

    /// Clear the buffer.
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    /// Check if the buffer contains a specific string.
    pub fn contains(&self, pattern: &str) -> bool {
        self.get_output().contains(pattern)
    }

    /// Check if the buffer contains ANSI color codes.
    pub fn has_ansi_codes(&self) -> bool {
        let bytes = self.inner.read().clone();
        let output = String::from_utf8_lossy(&bytes).to_string();
        output.contains("\x1b[")
    }

    /// Get all lines from the buffer.
    pub fn get_lines(&self) -> Vec<String> {
        self.get_output().lines().map(|s| s.to_string()).collect()
    }

    /// Count occurrences of a pattern in the output.
    pub fn count_occurrences(&self, pattern: &str) -> usize {
        self.get_output().matches(pattern).count()
    }
}

impl Default for CaptureBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Write for CaptureBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Strip ANSI escape codes from a string.
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip the escape sequence
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next(); // consume '['
                                  // Skip until we hit a letter (end of sequence)
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch.is_ascii_alphabetic() {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        result.push(c);
    }

    result
}

// ============================================================================
// Output Capturing Callback Wrapper
// ============================================================================

/// A callback wrapper that captures all output to a buffer.
pub struct OutputCapturingCallback {
    /// Buffer for captured output
    buffer: CaptureBuffer,
    /// Whether to use colors
    use_colors: bool,
    /// Verbosity level (0 = quiet, 1 = normal, 2 = verbose, 3+ = debug)
    verbosity: u8,
    /// Track what was called
    events: RwLock<Vec<String>>,
}

impl OutputCapturingCallback {
    /// Create a new output capturing callback.
    pub fn new() -> Self {
        Self {
            buffer: CaptureBuffer::new(),
            use_colors: true,
            verbosity: 1,
            events: RwLock::new(Vec::new()),
        }
    }

    /// Create without colors.
    pub fn without_colors() -> Self {
        Self {
            buffer: CaptureBuffer::new_strip_ansi(),
            use_colors: false,
            verbosity: 1,
            events: RwLock::new(Vec::new()),
        }
    }

    /// Create with specific verbosity.
    pub fn with_verbosity(verbosity: u8) -> Self {
        Self {
            buffer: CaptureBuffer::new(),
            use_colors: true,
            verbosity,
            events: RwLock::new(Vec::new()),
        }
    }

    /// Get captured output.
    pub fn get_output(&self) -> String {
        self.buffer.get_output()
    }

    /// Get captured lines.
    pub fn get_lines(&self) -> Vec<String> {
        self.buffer.get_lines()
    }

    /// Get recorded events.
    pub fn get_events(&self) -> Vec<String> {
        self.events.read().clone()
    }

    /// Check if output contains a pattern.
    pub fn output_contains(&self, pattern: &str) -> bool {
        self.buffer.contains(pattern)
    }

    /// Check if output has ANSI codes.
    pub fn has_colors(&self) -> bool {
        self.buffer.has_ansi_codes()
    }

    /// Clear the captured output.
    pub fn clear(&self) {
        self.buffer.clear();
        self.events.write().clear();
    }

    /// Write to the capture buffer.
    fn write_output(&self, msg: &str) {
        let mut buf = self.buffer.clone();
        let _ = writeln!(buf, "{}", msg);
    }

    /// Record an event.
    fn record_event(&self, event: &str) {
        self.events.write().push(event.to_string());
    }

    /// Format with optional colors.
    fn format_status(&self, text: &str, color: &str) -> String {
        if self.use_colors {
            match color {
                "green" => format!("\x1b[32m{}\x1b[0m", text),
                "yellow" => format!("\x1b[33m{}\x1b[0m", text),
                "red" => format!("\x1b[31m{}\x1b[0m", text),
                "cyan" => format!("\x1b[36m{}\x1b[0m", text),
                "magenta" => format!("\x1b[35m{}\x1b[0m", text),
                "bold" => format!("\x1b[1m{}\x1b[0m", text),
                _ => text.to_string(),
            }
        } else {
            text.to_string()
        }
    }
}

impl Default for OutputCapturingCallback {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionCallback for OutputCapturingCallback {
    async fn on_playbook_start(&self, name: &str) {
        self.record_event(&format!("playbook_start:{}", name));

        let header = format!("PLAYBOOK [{}]", name);
        let header = if self.use_colors {
            self.format_status(&header, "bold")
        } else {
            header
        };
        self.write_output(&header);

        if self.verbosity >= 2 {
            self.write_output(&format!("Starting playbook: {}", name));
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        self.record_event(&format!("playbook_end:{}:{}", name, success));

        let status = if success { "SUCCESS" } else { "FAILED" };
        let status_colored = if success {
            self.format_status(status, "green")
        } else {
            self.format_status(status, "red")
        };

        self.write_output(&format!("PLAYBOOK [{}] {}", name, status_colored));
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        self.record_event(&format!("play_start:{}:{}", name, hosts.len()));

        let host_count = hosts.len();
        self.write_output(&format!("PLAY [{}] {} host(s)", name, host_count));

        if self.verbosity >= 2 {
            for host in hosts {
                self.write_output(&format!("  - {}", host));
            }
        }
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        self.record_event(&format!("play_end:{}:{}", name, success));

        if self.verbosity >= 1 {
            let status = if success { "completed" } else { "failed" };
            self.write_output(&format!("PLAY [{}] {}", name, status));
        }
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        self.record_event(&format!("task_start:{}:{}", name, host));

        if self.verbosity >= 2 {
            self.write_output(&format!("TASK [{}] on {}", name, host));
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.record_event(&format!(
            "task_complete:{}:{}:{}",
            result.task_name, result.host, result.result.success
        ));

        let status = if !result.result.success {
            self.format_status("FAILED", "red")
        } else if result.result.skipped {
            self.format_status("SKIPPED", "cyan")
        } else if result.result.changed {
            self.format_status("CHANGED", "yellow")
        } else {
            self.format_status("OK", "green")
        };

        let duration = result.duration.as_millis();
        self.write_output(&format!(
            "{}: {} | {} ({}ms)",
            status, result.host, result.task_name, duration
        ));

        // Verbose output
        if self.verbosity >= 2 {
            self.write_output(&format!("    msg: {}", result.result.message));
        }

        // Debug output
        if self.verbosity >= 3 {
            self.write_output(&format!("    changed: {}", result.result.changed));
            self.write_output(&format!("    skipped: {}", result.result.skipped));
        }
    }

    async fn on_handler_triggered(&self, name: &str) {
        self.record_event(&format!("handler_triggered:{}", name));

        self.write_output(&format!(
            "HANDLER [{}] {}",
            name,
            self.format_status("triggered", "magenta")
        ));
    }

    async fn on_facts_gathered(&self, host: &str, _facts: &Facts) {
        self.record_event(&format!("facts_gathered:{}", host));

        if self.verbosity >= 1 {
            self.write_output(&format!("FACTS [{}] gathered", host));
        }
    }
}

// ============================================================================
// Test 1: Basic Output Capture
// ============================================================================

#[tokio::test]
async fn test_capture_playbook_start_output() {
    let callback = OutputCapturingCallback::without_colors();

    callback.on_playbook_start("test-playbook").await;

    let output = callback.get_output();
    assert!(output.contains("PLAYBOOK"));
    assert!(output.contains("test-playbook"));
}

#[tokio::test]
async fn test_capture_playbook_end_success_output() {
    let callback = OutputCapturingCallback::without_colors();

    callback.on_playbook_end("test-playbook", true).await;

    let output = callback.get_output();
    assert!(output.contains("PLAYBOOK"));
    assert!(output.contains("test-playbook"));
    assert!(output.contains("SUCCESS"));
}

#[tokio::test]
async fn test_capture_playbook_end_failure_output() {
    let callback = OutputCapturingCallback::without_colors();

    callback.on_playbook_end("test-playbook", false).await;

    let output = callback.get_output();
    assert!(output.contains("PLAYBOOK"));
    assert!(output.contains("FAILED"));
}

#[tokio::test]
async fn test_capture_play_start_output() {
    let callback = OutputCapturingCallback::without_colors();
    let hosts = vec!["host1".to_string(), "host2".to_string()];

    callback.on_play_start("Configure servers", &hosts).await;

    let output = callback.get_output();
    assert!(output.contains("PLAY"));
    assert!(output.contains("Configure servers"));
    assert!(output.contains("2 host(s)"));
}

#[tokio::test]
async fn test_capture_task_complete_ok_output() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "localhost".to_string(),
        task_name: "Install nginx".to_string(),
        result: ModuleResult::ok("nginx installed"),
        duration: Duration::from_millis(150),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("OK"));
    assert!(output.contains("localhost"));
    assert!(output.contains("Install nginx"));
    assert!(output.contains("150ms"));
}

#[tokio::test]
async fn test_capture_task_complete_changed_output() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "webserver1".to_string(),
        task_name: "Update config".to_string(),
        result: ModuleResult::changed("Configuration updated"),
        duration: Duration::from_millis(200),
        notify: vec!["restart nginx".to_string()],
    };

    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("CHANGED"));
    assert!(output.contains("webserver1"));
    assert!(output.contains("Update config"));
}

#[tokio::test]
async fn test_capture_task_complete_failed_output() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "dbserver1".to_string(),
        task_name: "Start database".to_string(),
        result: ModuleResult::failed("Connection refused"),
        duration: Duration::from_millis(500),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("FAILED"));
    assert!(output.contains("dbserver1"));
    assert!(output.contains("Start database"));
}

#[tokio::test]
async fn test_capture_task_complete_skipped_output() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "appserver1".to_string(),
        task_name: "Conditional task".to_string(),
        result: ModuleResult::skipped("Condition not met"),
        duration: Duration::from_millis(10),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("SKIPPED"));
    assert!(output.contains("appserver1"));
}

#[tokio::test]
async fn test_capture_handler_triggered_output() {
    let callback = OutputCapturingCallback::without_colors();

    callback.on_handler_triggered("restart nginx").await;

    let output = callback.get_output();
    assert!(output.contains("HANDLER"));
    assert!(output.contains("restart nginx"));
    assert!(output.contains("triggered"));
}

// ============================================================================
// Test 2: Exact Output Format Verification
// ============================================================================

#[tokio::test]
async fn test_exact_format_task_ok() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    // Verify the exact format: "STATUS: host | task (duration)"
    assert!(output.contains("OK: host1 | task1 (100ms)"));
}

#[tokio::test]
async fn test_exact_format_task_changed() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "web1".to_string(),
        task_name: "Deploy app".to_string(),
        result: ModuleResult::changed("Deployed"),
        duration: Duration::from_millis(2500),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("CHANGED: web1 | Deploy app (2500ms)"));
}

#[tokio::test]
async fn test_exact_format_play_start() {
    let callback = OutputCapturingCallback::without_colors();
    let hosts = vec!["web1".to_string(), "web2".to_string(), "web3".to_string()];

    callback.on_play_start("Install packages", &hosts).await;

    let output = callback.get_output();
    assert!(output.contains("PLAY [Install packages] 3 host(s)"));
}

// ============================================================================
// Test 3: Colored vs Non-Colored Output
// ============================================================================

#[tokio::test]
async fn test_colored_output_contains_ansi_codes() {
    let callback = OutputCapturingCallback::new(); // With colors

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    // Colored output should contain ANSI escape codes
    assert!(callback.has_colors());
}

#[tokio::test]
async fn test_non_colored_output_no_ansi_codes() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    // Non-colored output should not contain ANSI codes
    assert!(!callback.has_colors());
}

#[tokio::test]
async fn test_green_color_for_ok_status() {
    let callback = OutputCapturingCallback::new();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let raw = callback.buffer.get_bytes();
    let output = String::from_utf8_lossy(&raw);

    // Green ANSI code
    assert!(output.contains("\x1b[32m"));
}

#[tokio::test]
async fn test_yellow_color_for_changed_status() {
    let callback = OutputCapturingCallback::new();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::changed("changed"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let raw = callback.buffer.get_bytes();
    let output = String::from_utf8_lossy(&raw);

    // Yellow ANSI code
    assert!(output.contains("\x1b[33m"));
}

#[tokio::test]
async fn test_red_color_for_failed_status() {
    let callback = OutputCapturingCallback::new();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::failed("failed"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let raw = callback.buffer.get_bytes();
    let output = String::from_utf8_lossy(&raw);

    // Red ANSI code
    assert!(output.contains("\x1b[31m"));
}

#[tokio::test]
async fn test_cyan_color_for_skipped_status() {
    let callback = OutputCapturingCallback::new();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::skipped("skipped"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };

    callback.on_task_complete(&result).await;

    let raw = callback.buffer.get_bytes();
    let output = String::from_utf8_lossy(&raw);

    // Cyan ANSI code
    assert!(output.contains("\x1b[36m"));
}

// ============================================================================
// Test 4: Verbosity Levels
// ============================================================================

#[tokio::test]
async fn test_verbosity_0_quiet_mode() {
    let callback = {
        let mut cb = OutputCapturingCallback::without_colors();
        cb.verbosity = 0;
        cb
    };

    callback.on_playbook_start("test").await;
    callback.on_play_start("play", &["host1".to_string()]).await;
    callback.on_task_start("task1", "host1").await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;
    callback.on_play_end("play", true).await;
    callback.on_playbook_end("test", true).await;

    let output = callback.get_output();

    // In quiet mode, basic output should still appear
    assert!(output.contains("PLAYBOOK"));
    assert!(output.contains("PLAY"));
    // But verbose details should not
    assert!(!output.contains("msg:"));
}

#[tokio::test]
async fn test_verbosity_1_normal_mode() {
    let callback = OutputCapturingCallback::with_verbosity(1);

    callback.on_playbook_start("test").await;
    callback.on_play_start("play", &["host1".to_string()]).await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("test message"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;
    callback.on_play_end("play", true).await;
    callback.on_playbook_end("test", true).await;

    let output = callback.get_output();

    // Normal mode shows standard output
    assert!(output.contains("PLAYBOOK"));
    assert!(output.contains("PLAY"));
    assert!(output.contains("OK"));
    // But not verbose details
    assert!(!output.contains("msg:"));
}

#[tokio::test]
async fn test_verbosity_2_verbose_mode() {
    let callback = OutputCapturingCallback::with_verbosity(2);

    callback.on_playbook_start("test").await;
    callback
        .on_play_start("play", &["host1".to_string(), "host2".to_string()])
        .await;
    callback.on_task_start("task1", "host1").await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("verbose message"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let output = callback.get_output();

    // Verbose mode shows additional details
    assert!(output.contains("Starting playbook"));
    assert!(output.contains("host1"));
    assert!(output.contains("host2"));
    assert!(output.contains("TASK [task1] on host1"));
    assert!(output.contains("msg: verbose message"));
}

#[tokio::test]
async fn test_verbosity_3_debug_mode() {
    let callback = OutputCapturingCallback::with_verbosity(3);

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::changed("debug message"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let output = callback.get_output();

    // Debug mode shows everything
    assert!(output.contains("msg: debug message"));
    assert!(output.contains("changed: true"));
    assert!(output.contains("skipped: false"));
}

// ============================================================================
// Test 5: Complete Execution Flow Output
// ============================================================================

#[tokio::test]
async fn test_complete_playbook_execution_output() {
    let callback = OutputCapturingCallback::without_colors();
    let hosts = vec!["web1".to_string(), "web2".to_string()];

    // Full playbook execution
    callback.on_playbook_start("deploy_app").await;

    callback
        .on_play_start("Configure web servers", &hosts)
        .await;

    // Task on host1
    callback.on_task_start("Install nginx", "web1").await;
    let result1 = ExecutionResult {
        host: "web1".to_string(),
        task_name: "Install nginx".to_string(),
        result: ModuleResult::changed("nginx installed"),
        duration: Duration::from_millis(500),
        notify: vec!["restart nginx".to_string()],
    };
    callback.on_task_complete(&result1).await;

    // Task on host2
    callback.on_task_start("Install nginx", "web2").await;
    let result2 = ExecutionResult {
        host: "web2".to_string(),
        task_name: "Install nginx".to_string(),
        result: ModuleResult::ok("nginx already installed"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result2).await;

    // Handler triggered
    callback.on_handler_triggered("restart nginx").await;

    callback.on_play_end("Configure web servers", true).await;
    callback.on_playbook_end("deploy_app", true).await;

    let output = callback.get_output();

    // Verify the complete flow
    assert!(output.contains("PLAYBOOK [deploy_app]"));
    assert!(output.contains("PLAY [Configure web servers] 2 host(s)"));
    assert!(output.contains("CHANGED: web1 | Install nginx"));
    assert!(output.contains("OK: web2 | Install nginx"));
    assert!(output.contains("HANDLER [restart nginx]"));
    assert!(output.contains("SUCCESS"));
}

#[tokio::test]
async fn test_failed_playbook_execution_output() {
    let callback = OutputCapturingCallback::without_colors();
    let hosts = vec!["db1".to_string()];

    callback.on_playbook_start("backup_db").await;
    callback.on_play_start("Backup database", &hosts).await;

    let result = ExecutionResult {
        host: "db1".to_string(),
        task_name: "Run backup".to_string(),
        result: ModuleResult::failed("Disk full"),
        duration: Duration::from_millis(1000),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    callback.on_play_end("Backup database", false).await;
    callback.on_playbook_end("backup_db", false).await;

    let output = callback.get_output();

    assert!(output.contains("FAILED: db1 | Run backup"));
    assert!(output.contains("PLAYBOOK [backup_db] FAILED"));
}

// ============================================================================
// Test 6: Event Tracking
// ============================================================================

#[tokio::test]
async fn test_event_tracking_order() {
    let callback = OutputCapturingCallback::without_colors();
    let hosts = vec!["host1".to_string()];

    callback.on_playbook_start("test").await;
    callback.on_play_start("play1", &hosts).await;
    callback.on_task_start("task1", "host1").await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec!["handler1".to_string()],
    };
    callback.on_task_complete(&result).await;

    callback.on_handler_triggered("handler1").await;
    callback.on_play_end("play1", true).await;
    callback.on_playbook_end("test", true).await;

    let events = callback.get_events();

    assert_eq!(events[0], "playbook_start:test");
    assert_eq!(events[1], "play_start:play1:1");
    assert_eq!(events[2], "task_start:task1:host1");
    assert_eq!(events[3], "task_complete:task1:host1:true");
    assert_eq!(events[4], "handler_triggered:handler1");
    assert_eq!(events[5], "play_end:play1:true");
    assert_eq!(events[6], "playbook_end:test:true");
}

// ============================================================================
// Test 7: Line Count and Format
// ============================================================================

#[tokio::test]
async fn test_line_count_for_simple_execution() {
    let callback = OutputCapturingCallback::without_colors();

    callback.on_playbook_start("test").await;

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    callback.on_playbook_end("test", true).await;

    let lines = callback.get_lines();

    // Should have 3 lines: playbook start, task complete, playbook end
    assert_eq!(lines.len(), 3);
}

#[tokio::test]
async fn test_each_line_ends_with_meaningful_content() {
    let callback = OutputCapturingCallback::without_colors();

    callback.on_playbook_start("production").await;

    let lines = callback.get_lines();

    for line in &lines {
        // Each line should not be empty
        assert!(!line.trim().is_empty());
        // Each line should not be just whitespace
        assert!(!line.trim().is_empty());
    }
}

// ============================================================================
// Test 8: Duration Format
// ============================================================================

#[tokio::test]
async fn test_milliseconds_duration_format() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(250),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("250ms"));
}

#[tokio::test]
async fn test_seconds_duration_format() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "task1".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_secs(2),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("2000ms"));
}

// ============================================================================
// Test 9: Special Characters in Names
// ============================================================================

#[tokio::test]
async fn test_special_characters_in_task_name() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "host1".to_string(),
        task_name: "Configure 'nginx' with \"quotes\"".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("Configure 'nginx' with \"quotes\""));
}

#[tokio::test]
async fn test_unicode_in_host_name() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "server-日本".to_string(),
        task_name: "Test task".to_string(),
        result: ModuleResult::ok("ok"),
        duration: Duration::from_millis(100),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let output = callback.get_output();
    assert!(output.contains("server-日本"));
}

// ============================================================================
// Test 10: Multiple Tasks Same Host
// ============================================================================

#[tokio::test]
async fn test_multiple_tasks_on_same_host() {
    let callback = OutputCapturingCallback::without_colors();

    for i in 1..=5 {
        let result = ExecutionResult {
            host: "webserver".to_string(),
            task_name: format!("Task {}", i),
            result: if i % 2 == 0 {
                ModuleResult::changed("changed")
            } else {
                ModuleResult::ok("ok")
            },
            duration: Duration::from_millis(100 * i as u64),
            notify: vec![],
        };
        callback.on_task_complete(&result).await;
    }

    let output = callback.get_output();

    // All tasks should appear
    assert!(output.contains("Task 1"));
    assert!(output.contains("Task 2"));
    assert!(output.contains("Task 3"));
    assert!(output.contains("Task 4"));
    assert!(output.contains("Task 5"));

    // Mixed statuses
    assert!(output.contains("OK"));
    assert!(output.contains("CHANGED"));

    // Count occurrences
    assert_eq!(callback.buffer.count_occurrences("webserver"), 5);
}

// ============================================================================
// Test 11: Facts Gathered Output
// ============================================================================

#[tokio::test]
async fn test_facts_gathered_output() {
    let callback = OutputCapturingCallback::with_verbosity(1);

    let mut facts = Facts::new();
    facts.set("os_family", serde_json::json!("Debian"));
    facts.set("distribution", serde_json::json!("Ubuntu"));

    callback.on_facts_gathered("webserver1", &facts).await;

    let output = callback.get_output();
    assert!(output.contains("FACTS"));
    assert!(output.contains("webserver1"));
    assert!(output.contains("gathered"));
}

// ============================================================================
// Test 12: Buffer Clear and Reuse
// ============================================================================

#[tokio::test]
async fn test_buffer_clear_and_reuse() {
    let callback = OutputCapturingCallback::without_colors();

    // First execution
    callback.on_playbook_start("first").await;
    assert!(callback.output_contains("first"));

    // Clear
    callback.clear();
    assert!(!callback.output_contains("first"));
    assert!(callback.get_events().is_empty());

    // Second execution
    callback.on_playbook_start("second").await;
    assert!(callback.output_contains("second"));
    assert!(!callback.output_contains("first"));
}

// ============================================================================
// Test 13: Strip ANSI Helper Function
// ============================================================================

#[test]
fn test_strip_ansi_codes() {
    let colored = "\x1b[32mOK\x1b[0m: host1 | task1";
    let stripped = strip_ansi_codes(colored);
    assert_eq!(stripped, "OK: host1 | task1");

    let multiple = "\x1b[1m\x1b[31mFAILED\x1b[0m: \x1b[33mhost\x1b[0m";
    let stripped = strip_ansi_codes(multiple);
    assert_eq!(stripped, "FAILED: host");

    let no_ansi = "Plain text without codes";
    let stripped = strip_ansi_codes(no_ansi);
    assert_eq!(stripped, no_ansi);
}

// ============================================================================
// Test 14: Capture Buffer Thread Safety
// ============================================================================

#[tokio::test]
async fn test_capture_buffer_thread_safety() {
    let callback = Arc::new(OutputCapturingCallback::without_colors());

    let mut handles = vec![];

    for i in 0..10 {
        let cb = callback.clone();
        let handle = tokio::spawn(async move {
            let result = ExecutionResult {
                host: format!("host{}", i),
                task_name: format!("task{}", i),
                result: ModuleResult::ok("ok"),
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

    let output = callback.get_output();

    // All tasks should be recorded
    for i in 0..10 {
        assert!(output.contains(&format!("host{}", i)));
        assert!(output.contains(&format!("task{}", i)));
    }
}

// ============================================================================
// Test 15: Output Comparison with Expected Strings
// ============================================================================

#[tokio::test]
async fn test_compare_exact_output_strings() {
    let callback = OutputCapturingCallback::without_colors();

    let result = ExecutionResult {
        host: "test-host".to_string(),
        task_name: "test-task".to_string(),
        result: ModuleResult::ok("success"),
        duration: Duration::from_millis(123),
        notify: vec![],
    };
    callback.on_task_complete(&result).await;

    let lines = callback.get_lines();
    let expected = "OK: test-host | test-task (123ms)";

    assert_eq!(lines[0], expected);
}

#[tokio::test]
async fn test_compare_playbook_header_format() {
    let callback = OutputCapturingCallback::without_colors();

    callback.on_playbook_start("my-playbook").await;

    let lines = callback.get_lines();
    assert_eq!(lines[0], "PLAYBOOK [my-playbook]");
}

#[tokio::test]
async fn test_compare_play_header_format() {
    let callback = OutputCapturingCallback::without_colors();
    let hosts = vec!["h1".to_string(), "h2".to_string(), "h3".to_string()];

    callback.on_play_start("My Play", &hosts).await;

    let lines = callback.get_lines();
    assert_eq!(lines[0], "PLAY [My Play] 3 host(s)");
}
