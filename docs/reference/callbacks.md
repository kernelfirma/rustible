---
summary: Callback plugin reference covering 20+ built-in callbacks for output formatting, timing, filtering, logging, and external integrations like JUnit and email.
read_when: You want to customize execution output, integrate with CI/CD, send notifications, or develop custom callback plugins.
---

# Rustible Callback Plugin Reference

Callbacks receive notifications about execution events and can be used for logging, metrics collection, custom output formatting, or external integrations.

## Table of Contents

1. [Callback Architecture](#callback-architecture)
2. [Core Output Callbacks](#core-output-callbacks)
3. [Visual Callbacks](#visual-callbacks)
4. [Timing and Analysis Callbacks](#timing-and-analysis-callbacks)
5. [Filtering Callbacks](#filtering-callbacks)
6. [Logging Callbacks](#logging-callbacks)
7. [Integration Callbacks](#integration-callbacks)
8. [Custom Callback Development](#custom-callback-development)

---

## Callback Architecture

### ExecutionCallback Trait

All callbacks implement the `ExecutionCallback` trait:

```rust
use async_trait::async_trait;

#[async_trait]
pub trait ExecutionCallback: Send + Sync + std::fmt::Debug {
    // Playbook lifecycle
    async fn on_playbook_start(&self, _playbook: &str) {}
    async fn on_playbook_complete(&self) {}

    // Play lifecycle
    async fn on_play_start(&self, _play: &str, _hosts: &[String]) {}
    async fn on_play_complete(&self) {}

    // Task lifecycle
    async fn on_task_start(&self, _task: &str, _host: &str) {}
    async fn on_task_complete(&self, _result: &ExecutionResult) {}
    async fn on_task_skipped(&self, _task: &str, _host: &str, _reason: &str) {}

    // Host events
    async fn on_host_unreachable(&self, _host: &str, _error: &str) {}
    async fn on_host_ok(&self, _host: &str, _changed: bool) {}
    async fn on_host_failed(&self, _host: &str, _error: &str) {}

    // Handler events
    async fn on_handler_triggered(&self, _handler: &str, _by_task: &str) {}
    async fn on_handler_complete(&self, _handler: &str, _result: &ExecutionResult) {}

    // Recap
    async fn on_stats(&self, _stats: &PlayStats) {}
}
```

### ExecutionResult Structure

```rust
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub task_name: String,
    pub host: String,
    pub status: TaskStatus,
    pub changed: bool,
    pub msg: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub duration: Duration,
    pub diff: Option<Diff>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Ok,
    Changed,
    Failed,
    Skipped,
    Unreachable,
}
```

### Using Callbacks

```rust
use rustible::callback::prelude::*;

// Single callback
let executor = PlaybookExecutor::new()
    .with_callback(Box::new(DefaultCallback::new()))
    .build()?;

// Multiple callbacks with CompositeCallback
let callbacks = CompositeCallback::new()
    .with_callback(Box::new(ProgressCallback::new()))
    .with_callback(Box::new(TimerCallback::new()))
    .with_callback(Box::new(JsonCallback::to_file("output.json")));

let executor = PlaybookExecutor::new()
    .with_callback(Box::new(callbacks))
    .build()?;
```

---

## Core Output Callbacks

### DefaultCallback

Standard Ansible-like colored output.

```rust
use rustible::callback::{DefaultCallback, DefaultCallbackConfig, Verbosity};

// Basic usage
let callback = DefaultCallback::new();

// With configuration
let callback = DefaultCallback::builder()
    .verbosity(Verbosity::Verbose)
    .show_task_path(true)
    .show_skipped(true)
    .show_ok(true)
    .use_color(true)
    .build();
```

**Output Example:**
```
PLAY [Configure web servers] ***************************************************

TASK [Install nginx] ***********************************************************
ok: [webserver1]
changed: [webserver2]

TASK [Start nginx] *************************************************************
ok: [webserver1]
ok: [webserver2]

PLAY RECAP *********************************************************************
webserver1    : ok=2    changed=0    unreachable=0    failed=0    skipped=0
webserver2    : ok=2    changed=1    unreachable=0    failed=0    skipped=0
```

### MinimalCallback

Quiet output showing only failures and recap.

```rust
use rustible::callback::MinimalCallback;

let callback = MinimalCallback::new();
```

**Output Example:**
```
FAILED - TASK [Install nginx] (webserver3): Package not found

PLAY RECAP *********************************************************************
webserver1    : ok=5    changed=2
webserver2    : ok=5    changed=2
webserver3    : ok=0    changed=0    failed=1
```

### SummaryCallback

Silent during execution, comprehensive summary at end.

```rust
use rustible::callback::{SummaryCallback, SummaryConfig};

let callback = SummaryCallback::builder()
    .show_timing(true)
    .show_changes(true)
    .show_failures(true)
    .build();
```

**Output Example:**
```
================================================================================
                          EXECUTION SUMMARY
================================================================================

Duration: 2m 34s
Hosts: 5 total, 5 ok, 0 failed, 0 unreachable

Tasks Executed: 42
  - Changed: 12
  - Ok: 28
  - Skipped: 2
  - Failed: 0

Changes by Host:
  webserver1: 3 changes
  webserver2: 3 changes
  dbserver1: 4 changes
  dbserver2: 2 changes

Top 5 Slowest Tasks:
  1. Install packages (45.2s)
  2. Clone repository (23.1s)
  3. Run migrations (18.5s)
  4. Build application (15.3s)
  5. Restart services (8.2s)
================================================================================
```

### NullCallback

No output - useful for testing or programmatic use.

```rust
use rustible::callback::NullCallback;

let callback = NullCallback::new();
```

---

## Visual Callbacks

### ProgressCallback

Displays progress bars during execution.

```rust
use rustible::callback::{ProgressCallback, ProgressConfig};

let callback = ProgressCallback::builder()
    .style(ProgressStyle::Spinner)  // or Bar, Percentage
    .show_host_progress(true)
    .show_task_progress(true)
    .refresh_rate_ms(100)
    .build();
```

**Output Example:**
```
[=============>          ] 56% (14/25 tasks)
Current: Installing packages on webserver1, webserver2, webserver3
```

### DiffCallback

Shows before/after diffs for changed files.

```rust
use rustible::callback::{DiffCallback, DiffConfig};

let callback = DiffCallback::builder()
    .context_lines(3)
    .color_diff(true)
    .show_binary_files(false)
    .build();
```

**Output Example:**
```
TASK [Update nginx config] *****************************************************
--- before: /etc/nginx/nginx.conf
+++ after: /etc/nginx/nginx.conf
@@ -12,6 +12,7 @@
     worker_connections 1024;
+    multi_accept on;
 }
```

### DenseCallback

Compact single-line output per host/task.

```rust
use rustible::callback::{DenseCallback, DenseConfig};

let callback = DenseCallback::builder()
    .show_timestamps(true)
    .truncate_length(80)
    .build();
```

**Output Example:**
```
[14:32:01] webserver1 | Install nginx | CHANGED
[14:32:01] webserver2 | Install nginx | CHANGED
[14:32:02] webserver1 | Start nginx   | OK
[14:32:02] webserver2 | Start nginx   | OK
```

### OnelineCallback

Single line per task with all hosts.

```rust
use rustible::callback::{OnelineCallback, OnelineConfig};

let callback = OnelineCallback::builder()
    .show_changed_only(false)
    .build();
```

**Output Example:**
```
Install nginx | ok=2 changed=0 | webserver1, webserver2
Start nginx   | ok=2 changed=0 | webserver1, webserver2
```

### TreeCallback

Hierarchical tree-structured output.

```rust
use rustible::callback::{TreeCallback, TreeConfig};

let callback = TreeCallback::builder()
    .indent_size(2)
    .show_timing(true)
    .build();
```

**Output Example:**
```
playbook.yml
+-- Play: Configure web servers
|   +-- Task: Install nginx (2.3s)
|   |   +-- webserver1: OK
|   |   +-- webserver2: CHANGED
|   +-- Task: Start nginx (0.5s)
|       +-- webserver1: OK
|       +-- webserver2: OK
```

---

## Timing and Analysis Callbacks

### TimerCallback

Tracks and reports execution timing.

```rust
use rustible::callback::{TimerCallback, TimerConfig};

let callback = TimerCallback::builder()
    .show_per_task(true)
    .show_per_host(true)
    .show_total(true)
    .top_n_slowest(10)
    .build();
```

**Output Example:**
```
================================================================================
                              TIMING SUMMARY
================================================================================
Total execution time: 4m 23s

Top 10 Slowest Tasks:
  1. Install packages    : 1m 15s (28.5%)
  2. Clone repository    : 45.2s  (17.2%)
  3. Run database migrations : 32.1s  (12.2%)
  ...

Per-Host Timing:
  webserver1: 2m 11s
  webserver2: 2m 08s
  dbserver1 : 4m 23s
================================================================================
```

### StatsCallback

Collects detailed statistics about execution.

```rust
use rustible::callback::{StatsCallback, StatsConfig};

let callback = StatsCallback::builder()
    .track_module_stats(true)
    .track_host_stats(true)
    .track_timing_histogram(true)
    .build();

// Access stats after execution
let stats = callback.get_stats();
println!("Total tasks: {}", stats.total_tasks);
println!("Changed: {}", stats.changed_count);
println!("Module usage: {:?}", stats.module_counts);
```

### ContextCallback

Shows task context including variables and conditions.

```rust
use rustible::callback::{ContextCallback, ContextVerbosity};

let callback = ContextCallback::builder()
    .verbosity(ContextVerbosity::Full)
    .show_vars(true)
    .show_when(true)
    .show_loop_vars(true)
    .build();
```

**Output Example:**
```
TASK [Install nginx] ***********************************************************
  when: nginx_enabled == true
  vars: http_port=8080, worker_count=4
  loop_var: item=nginx-core
ok: [webserver1]
```

### CounterCallback

Counts tasks and provides running totals.

```rust
use rustible::callback::{CounterCallback, CounterConfig};

let callback = CounterCallback::builder()
    .show_running_count(true)
    .show_percentages(true)
    .build();
```

**Output Example:**
```
[3/25] TASK: Install nginx ....................................... OK
[4/25] TASK: Configure nginx ..................................... CHANGED
[5/25] TASK: Start nginx ......................................... OK
```

---

## Filtering Callbacks

### SelectiveCallback

Filters output by status, host, or patterns.

```rust
use rustible::callback::{SelectiveCallback, StatusFilter, FilterMode};

let callback = SelectiveCallback::builder()
    .filter_status(StatusFilter::ChangedAndFailed)
    .filter_hosts(vec!["prod-*".to_string()])
    .filter_tasks(vec!["*nginx*".to_string()])
    .filter_mode(FilterMode::Include)
    .build();
```

### SkippyCallback

Hides skipped tasks from output.

```rust
use rustible::callback::{SkippyCallback, SkippyConfig};

let callback = SkippyCallback::builder()
    .show_skip_summary(true)  // Show count at end
    .build();
```

### ActionableCallback

Only shows tasks that changed or failed.

```rust
use rustible::callback::{ActionableCallback, ActionableConfig};

let callback = ActionableCallback::builder()
    .show_changed(true)
    .show_failed(true)
    .show_unreachable(true)
    .build();
```

### FullSkipCallback

Detailed analysis of skipped tasks.

```rust
use rustible::callback::{FullSkipCallback, FullSkipConfig};

let callback = FullSkipCallback::builder()
    .group_by_reason(true)
    .show_conditions(true)
    .build();
```

**Output Example:**
```
================================================================================
                           SKIPPED TASKS ANALYSIS
================================================================================

Skipped due to 'when' condition: 12 tasks
  - Install nginx (nginx_enabled == false): 3 hosts
  - Configure firewall (firewall_managed == false): 3 hosts
  ...

Skipped due to tags: 5 tasks
  - Deploy application (tag: deploy): 3 hosts
  ...
================================================================================
```

---

## Logging Callbacks

### JsonCallback

Outputs execution events as JSON.

```rust
use rustible::callback::{JsonCallback, JsonConfig};

// To stdout
let callback = JsonCallback::new();

// To file
let callback = JsonCallback::to_file("output.json")?;

// With configuration
let callback = JsonCallback::builder()
    .pretty_print(true)
    .include_timing(true)
    .include_diff(true)
    .build();
```

**Output Example:**
```json
{
  "plays": [
    {
      "name": "Configure web servers",
      "hosts": ["webserver1", "webserver2"],
      "tasks": [
        {
          "name": "Install nginx",
          "hosts": {
            "webserver1": {
              "status": "ok",
              "changed": false,
              "duration_ms": 1234
            },
            "webserver2": {
              "status": "changed",
              "changed": true,
              "duration_ms": 2345
            }
          }
        }
      ]
    }
  ],
  "stats": {
    "ok": 4,
    "changed": 2,
    "failed": 0,
    "skipped": 0,
    "duration_ms": 15234
  }
}
```

### YamlCallback

Outputs execution events as YAML.

```rust
use rustible::callback::{YamlCallback, YamlConfig};

let callback = YamlCallback::builder()
    .output_file("output.yml")
    .include_vars(false)  // Don't include sensitive vars
    .build()?;
```

### LogFileCallback

Writes detailed logs to a file.

```rust
use rustible::callback::{LogFileCallback, LogFileConfig};

let callback = LogFileCallback::builder()
    .log_path("/var/log/rustible/execution.log")
    .rotation(LogRotation::Daily)
    .max_size_mb(100)
    .include_timestamps(true)
    .log_level(LogLevel::Debug)
    .build()?;
```

### SyslogCallback

Sends events to system syslog.

```rust
use rustible::callback::{SyslogCallback, SyslogConfig, SyslogFacility};

let callback = SyslogCallback::builder()
    .facility(SyslogFacility::Local0)
    .app_name("rustible")
    .include_host(true)
    .build()?;
```

### DebugCallback

Verbose debug output for development.

```rust
use rustible::callback::{DebugCallback, DebugConfig};

let callback = DebugCallback::builder()
    .show_args(true)
    .show_result_details(true)
    .show_connection_info(true)
    .build();
```

---

## Integration Callbacks

### JUnitCallback

Generates JUnit XML reports for CI/CD integration.

```rust
use rustible::callback::JUnitCallback;

let callback = JUnitCallback::new("test-results.xml")?;
```

**Output Example:**
```xml
<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="playbook.yml" tests="25" failures="1" errors="0" time="123.456">
  <testcase name="Install nginx" classname="webserver1" time="2.345"/>
  <testcase name="Install nginx" classname="webserver2" time="2.123"/>
  <testcase name="Configure nginx" classname="webserver3" time="1.234">
    <failure message="Package not found">
      stderr: E: Unable to locate package nginx
    </failure>
  </testcase>
</testsuite>
```

### MailCallback

Sends email notifications on completion or failure.

```rust
use rustible::callback::{MailCallback, MailConfig, TlsMode};

let callback = MailCallback::builder()
    .smtp_host("smtp.example.com")
    .smtp_port(587)
    .tls_mode(TlsMode::StartTls)
    .username("user@example.com")
    .password("password")
    .from("rustible@example.com")
    .to(vec!["admin@example.com".to_string()])
    .subject_prefix("[Rustible]")
    .send_on_failure(true)
    .send_on_success(false)
    .attach_log(true)
    .build()?;
```

### ForkedCallback

Output format for parallel/forked execution.

```rust
use rustible::callback::{ForkedCallback, ForkedConfig};

let callback = ForkedCallback::builder()
    .prefix_with_host(true)
    .color_by_host(true)
    .build();
```

**Output Example:**
```
[webserver1] Installing nginx...
[webserver2] Installing nginx...
[webserver1] OK: nginx installed
[webserver3] Installing nginx...
[webserver2] CHANGED: nginx installed
[webserver3] OK: nginx installed
```

---

## Custom Callback Development

### Basic Custom Callback

```rust
use rustible::callback::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct CustomMetricsCallback {
    task_count: AtomicUsize,
    change_count: AtomicUsize,
    failure_count: AtomicUsize,
    start_time: std::time::Instant,
}

impl CustomMetricsCallback {
    pub fn new() -> Self {
        Self {
            task_count: AtomicUsize::new(0),
            change_count: AtomicUsize::new(0),
            failure_count: AtomicUsize::new(0),
            start_time: std::time::Instant::now(),
        }
    }

    pub fn get_metrics(&self) -> Metrics {
        Metrics {
            total_tasks: self.task_count.load(Ordering::SeqCst),
            changes: self.change_count.load(Ordering::SeqCst),
            failures: self.failure_count.load(Ordering::SeqCst),
            duration: self.start_time.elapsed(),
        }
    }
}

#[async_trait]
impl ExecutionCallback for CustomMetricsCallback {
    async fn on_playbook_start(&self, playbook: &str) {
        println!("Starting playbook: {}", playbook);
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        self.task_count.fetch_add(1, Ordering::SeqCst);

        match result.status {
            TaskStatus::Changed => {
                self.change_count.fetch_add(1, Ordering::SeqCst);
            }
            TaskStatus::Failed => {
                self.failure_count.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        }
    }

    async fn on_playbook_complete(&self) {
        let metrics = self.get_metrics();
        println!("\n=== Execution Metrics ===");
        println!("Total tasks: {}", metrics.total_tasks);
        println!("Changes: {}", metrics.changes);
        println!("Failures: {}", metrics.failures);
        println!("Duration: {:?}", metrics.duration);
    }
}
```

### Webhook Callback Example

```rust
use rustible::callback::prelude::*;
use reqwest::Client;
use serde_json::json;

#[derive(Debug)]
pub struct WebhookCallback {
    client: Client,
    webhook_url: String,
}

impl WebhookCallback {
    pub fn new(webhook_url: &str) -> Self {
        Self {
            client: Client::new(),
            webhook_url: webhook_url.to_string(),
        }
    }
}

#[async_trait]
impl ExecutionCallback for WebhookCallback {
    async fn on_playbook_start(&self, playbook: &str) {
        let _ = self.client.post(&self.webhook_url)
            .json(&json!({
                "event": "playbook_start",
                "playbook": playbook,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
            .send()
            .await;
    }

    async fn on_host_failed(&self, host: &str, error: &str) {
        let _ = self.client.post(&self.webhook_url)
            .json(&json!({
                "event": "host_failed",
                "host": host,
                "error": error,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
            .send()
            .await;
    }

    async fn on_playbook_complete(&self) {
        let _ = self.client.post(&self.webhook_url)
            .json(&json!({
                "event": "playbook_complete",
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
            .send()
            .await;
    }
}
```

### Composing Multiple Callbacks

```rust
use rustible::callback::prelude::*;

// Create composite callback with multiple plugins
let composite = CompositeCallback::new()
    // Visual output
    .with_callback(Box::new(ProgressCallback::new()))
    // Timing information
    .with_callback(Box::new(TimerCallback::builder()
        .show_per_task(true)
        .build()))
    // JSON log for analysis
    .with_callback(Box::new(JsonCallback::to_file("execution.json")?))
    // Email on failure
    .with_callback(Box::new(MailCallback::builder()
        .smtp_host("smtp.example.com")
        .send_on_failure(true)
        .build()?))
    // Custom webhook
    .with_callback(Box::new(WebhookCallback::new("https://api.example.com/webhook")));

let executor = PlaybookExecutor::new()
    .with_callback(Box::new(composite))
    .build()?;
```
