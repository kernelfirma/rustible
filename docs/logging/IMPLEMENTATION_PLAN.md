# Rustible Logging Implementation Plan
# Based on https://loggingsucks.com/ philosophy

## Overview

This document outlines the implementation of structured, wide-event logging for rustible following the loggingsucks.com philosophy.

## Core Principles

1. **Wide Events**: Emit ONE comprehensive log event per operation (not scattered statements)
2. **Structured JSON**: Machine-readable key-value pairs for querying
3. **Business Context**: Log what happened to requests, not code state
4. **Trace ID Propagation**: Track operations across async boundaries
5. **Intelligent Sampling**: Keep 100% errors, slow operations, sample others
6. **Queryable by Design**: Optimized for analytics queries, not string search

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     rustible CLI                          │
│                    (entry point)                          │
└───────────────────┬─────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────────────────┐
│         Tracing Layer (new)                               │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐│
│  │ Trace ID Gen │  │  Sampling    │  │ Formatter   ││
│  │              │  │  Strategy    │  │ (JSON)      ││
│  └──────────────┘  └──────────────┘  └─────────────┘│
└───────────────────┬─────────────────────────────────────┘
                    │
        ┌───────────┼───────────┐
        ▼           ▼           ▼
   ┌────────┐ ┌────────┐ ┌────────┐
   │  SSH   │ │ Task   │ │ Template│
   │ Module │ │Engine  │ │ Engine │
   └────────┘ └────────┘ └────────┘
        │           │           │
        └───────────┼───────────┘
                    ▼
            ┌──────────────┐
            │   File/      │
            │  Stdout      │
            │  Sink        │
            └──────────────┘
```

## Wide Event Schema

### Core Fields (Always Present)

```rust
pub struct RustibleEvent {
    // Trace & Span
    trace_id: String,              // UUID for operation chain
    span_id: String,               // Current span ID
    parent_span_id: Option<String>,  // Parent span if nested

    // Timestamps
    timestamp_ns: u64,             // Nanosecond precision
    duration_ns: Option<u64>,       // Operation duration

    // Event Classification
    event_name: String,             // "ssh_connection", "task_execution", etc.
    event_type: String,            // "operation", "error", "metric"
    severity: String,              // "info", "error", "warn"

    // Operation Context
    operation_name: String,         // "playbook_run", "module_invoke"
    correlation_id: Option<String>, // Request/playbook identifier
    attempt_count: u32,            // Retry count

    // Host/Target Information
    host_id: String,               // Target hostname/IP
    host_labels: HashMap<String, String>, // From inventory
    inventory_group: Vec<String>,   // Groups host belongs to

    // User/Authentication
    user_id: Option<String>,        // Running user
    sudo_user: Option<String>,      // Become user
    authentication_method: Option<String>, // ssh, local, docker, k8s
    connection_type: String,        // "ssh", "local", "docker", "k8s"

    // Module/Task Information
    module_name: String,            // "package", "file", "template", etc.
    task_name: String,             // From playbook
    task_id: String,               // UUID for this task execution
    role_name: Option<String>,      // If in role
    playbook_name: String,          // Source playbook file

    // Performance Metrics
    duration_ms: f64,              // Duration in milliseconds
    cpu_time_ms: Option<f64>,     // CPU time consumed
    memory_bytes: Option<u64>,     // Memory used
    network_bytes_sent: Option<u64>,
    network_bytes_received: Option<u64>,

    // Execution Details
    parallel_workers: u32,          // Forks setting
    execution_strategy: String,       // "linear", "free", "serial"
    check_mode: bool,              // Dry run flag
    diff_mode: bool,               // Show changes flag

    // Results
    status: String,                // "success", "failure", "skipped", "changed"
    changed: bool,                // Ansible changed flag
    skipped: bool,                // If task was skipped
    failed: bool,                 // If task failed

    // Error Information (when applicable)
    error_code: Option<i32>,
    error_type: Option<String>,     // "timeout", "connection", "permission", etc.
    error_message: Option<String>,
    error_stack_trace: Option<String>,

    // Change Details
    files_changed: Option<Vec<String>>,
    packages_installed: Option<Vec<String>>,
    packages_removed: Option<Vec<String>>,
    services_started: Option<Vec<String>>,
    services_stopped: Option<Vec<String>>,

    // Network Details (SSH)
    ssh_host: String,              // SSH target
    ssh_port: u16,                // SSH port (default 22)
    ssh_user: String,              // SSH username
    ssh_auth_method: String,        // "key", "password"
    ssh_connection_time_ms: Option<f64>,
    ssh_handshake_time_ms: Option<f64>,

    // Connection Pool
    pool_hits: u32,               // Cache hits
    pool_misses: u32,              // Cache misses
    pool_size: u32,                // Current pool size
    pool_max_size: u32,            // Max pool size

    // Template Details
    template_path: Option<String>,  // Template file path
    template_variables_count: u32,   // Number of variables
    template_render_time_ms: Option<f64>,

    // Inventory Details
    inventory_file: Option<String>,
    inventory_host_count: u32,
    inventory_group_count: u32,
    inventory_vars_count: u32,

    // Environment
    os_type: String,               // "linux", "darwin", "windows"
    os_version: Option<String>,
    arch: String,                  // "x86_64", "arm64", etc.
    rustible_version: String,       // Current version

    // Configuration
    config_file: Option<String>,    // Config file used
    config_profile: Option<String>, // Active profile
    feature_flags: Vec<String>,    // Enabled features

    // Resource Limits
    timeout_seconds: u32,          // Operation timeout
    max_retries: u32,             // Retry limit

    // Telemetry
    telemetry_enabled: bool,
    telemetry_sampled: bool,        // If this event was sampled
    sampling_reason: Option<String>, // "error", "slow", "random", "always"

    // Custom Fields (extensible)
    custom_fields: HashMap<String, serde_json::Value>,
}
```

## Key Operation Types

### 1. Playbook Execution (top-level)

```json
{
  "trace_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "event_name": "playbook_execution",
  "event_type": "operation",
  "severity": "info",

  "timestamp_ns": 1640995200000000000,
  "duration_ms": 4523.7,

  "playbook_name": "deploy_webapp.yml",
  "playbook_path": "/playbooks/deploy_webapp.yml",
  "playbook_hash": "sha256:abc123...",

  "inventory_file": "/inventory/production.yml",
  "inventory_host_count": 45,
  "inventory_group_count": 12,

  "user_id": "admin",
  "config_file": "/etc/rustible/config.toml",

  "parallel_workers": 10,
  "execution_strategy": "linear",
  "check_mode": false,

  "status": "success",
  "hosts_total": 45,
  "hosts_successful": 43,
  "hosts_failed": 2,
  "hosts_skipped": 0,
  "hosts_changed": 41,
  "hosts_unreachable": 2,

  "tasks_total": 23,
  "tasks_executed": 22,
  "tasks_skipped": 1,
  "tasks_failed": 0,

  "total_play_time_ms": 4523.7,
  "avg_play_time_per_host_ms": 100.5,
  "p95_play_time_ms": 287.3,
  "p99_play_time_ms": 452.1,

  "changed_hosts_count": 41,
  "unchanged_hosts_count": 2,
  "failed_hosts_count": 2,

  "network_bytes_sent": 5242880,
  "network_bytes_received": 15728640,

  "cpu_time_ms": 1234.5,
  "memory_bytes": 10485760,

  "telemetry_sampled": false,
  "sampling_reason": "always"
}
```

### 2. Task Execution

```json
{
  "trace_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "span_id": "span_001",
  "parent_span_id": null,

  "event_name": "task_execution",
  "event_type": "operation",
  "severity": "info",

  "timestamp_ns": 1640995200100000000,
  "duration_ms": 234.5,

  "task_name": "Install nginx package",
  "task_id": "task_001",
  "module_name": "package",
  "module_args": {
    "name": "nginx",
    "state": "present"
  },

  "host_id": "web-01.example.com",
  "host_labels": {
    "environment": "production",
    "role": "webserver",
    "region": "us-east-1"
  },
  "inventory_group": ["webservers", "production"],

  "connection_type": "ssh",
  "ssh_host": "web-01.example.com",
  "ssh_port": 22,
  "ssh_user": "ansible",
  "ssh_auth_method": "key",

  "ssh_connection_time_ms": 45.2,
  "ssh_handshake_time_ms": 12.8,

  "status": "changed",
  "changed": true,
  "skipped": false,
  "failed": false,

  "packages_installed": ["nginx"],
  "packages_removed": [],

  "check_mode": false,
  "diff_mode": false,

  "pool_hits": 1,
  "pool_misses": 0,
  "pool_size": 10,
  "pool_max_size": 50,

  "telemetry_sampled": false,
  "sampling_reason": "always"
}
```

### 3. SSH Connection

```json
{
  "trace_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "span_id": "span_ssh_001",
  "parent_span_id": "span_001",

  "event_name": "ssh_connection",
  "event_type": "operation",
  "severity": "info",

  "timestamp_ns": 1640995200200000000,
  "duration_ms": 58.0,

  "ssh_host": "web-01.example.com",
  "ssh_port": 22,
  "ssh_user": "ansible",
  "ssh_auth_method": "key",

  "status": "success",

  "dns_resolution_time_ms": 8.2,
  "tcp_connection_time_ms": 15.3,
  "ssh_handshake_time_ms": 12.8,
  "authentication_time_ms": 15.7,

  "ssh_version": "SSH-2.0-OpenSSH_8.9p1",
  "ssh_cipher": "chacha20-poly1305@openssh.com",
  "ssh_mac": "hmac-sha2-256",
  "ssh_key_type": "ssh-rsa",

  "connection_reused": true,
  "connection_from_cache": true,

  "telemetry_sampled": false,
  "sampling_reason": "always"
}
```

### 4. Template Rendering

```json
{
  "trace_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "span_id": "span_tpl_001",
  "parent_span_id": "span_002",

  "event_name": "template_render",
  "event_type": "operation",
  "severity": "info",

  "timestamp_ns": 1640995200300000000,
  "duration_ms": 12.3,

  "template_path": "/templates/nginx.conf.j2",
  "template_type": "minijinja",
  "template_size_bytes": 2048,

  "template_variables_count": 12,
  "template_variables": {
    "worker_processes": 4,
    "worker_connections": 1024,
    "server_names": ["example.com", "www.example.com"]
  },

  "rendered_size_bytes": 2156,
  "render_time_ms": 12.3,

  "status": "success",
  "syntax_errors": [],

  "telemetry_sampled": true,
  "sampling_reason": "random"
}
```

### 5. Error Events (Always Logged)

```json
{
  "trace_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "span_id": "span_err_001",
  "parent_span_id": "span_003",

  "event_name": "operation_error",
  "event_type": "error",
  "severity": "error",

  "timestamp_ns": 1640995200400000000,
  "duration_ms": 15.7,

  "task_name": "Start nginx service",
  "task_id": "task_003",
  "module_name": "service",

  "host_id": "web-02.example.com",

  "status": "failure",
  "failed": true,

  "error_code": 1,
  "error_type": "timeout",
  "error_message": "Failed to start nginx.service: Timeout after 30s",
  "error_stack_trace": "error: Operation timeout\n   at modules/service.rs:234\n...",

  "retry_count": 3,
  "max_retries": 3,

  "last_successful_state": {
    "service_name": "nginx",
    "service_status": "stopped"
  },

  "attempted_remediations": [
    "systemctl start nginx.service",
    "systemctl daemon-reload",
    "systemctl restart nginx.service"
  ],

  "telemetry_sampled": false,
  "sampling_reason": "error"
}
```

## Sampling Strategy

### Keep 100% Of:
- All error events (severity: "error")
- All warnings (severity: "warn")
- Slow operations (duration_ms > P99)
- SSH connection failures
- Authentication failures
- Failed task executions
- Events with custom sampling_reason: "always"

### Sample Success Events:
- Playbook executions: 10% random sample
- Task executions: 5% random sample
- Template renders: 2% random sample
- SSH connections (successful): 1% random sample

### Adaptive Sampling:
- Increase sampling rate during active development (check_mode: true)
- Increase sampling for VIP hosts (marked in inventory)
- Decrease sampling during high load (parallel_workers > 100)

## Trace ID Propagation

### Implementation Strategy

```rust
use tracing::{span, Level, info};
use uuid::Uuid;

// Generate trace ID at entry point
pub fn generate_trace_id() -> String {
    Uuid::new_v4().to_string()
}

// Instrument main function
pub async fn run_playbook(
    playbook_path: &str,
    inventory: &str,
) -> Result<()> {
    let trace_id = generate_trace_id();

    let root_span = span!(
        Level::INFO,
        "playbook_execution",
        trace_id = %trace_id,
        playbook_path = %playbook_path,
        inventory = %inventory,
    );

    root_span.in_scope(|| {
        // All child spans inherit trace_id
        async {
            // Trace ID propagated to all async tasks
            tokio::spawn(async move {
                execute_tasks(trace_id.clone()).await;
            });
        }.await
    }).await
}

// Trace ID in async context
pub async fn execute_tasks(trace_id: String) {
    let task_span = span!(
        Level::INFO,
        "task_execution",
        trace_id = %trace_id,
        task_name = %task.name,
    );

    info!(
        parent: &task_span,
        "Executing task",
        trace_id = %trace_id,
        task_name = %task.name,
        module = %task.module
    );
}
```

## Implementation Modules

### 1. Tracing Initialization (`src/logging/`)

```rust
// src/logging/init.rs
use tracing_subscriber::{EnvFilter, Registry};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init_logging() {
    let filter = EnvFilter::from_default_env()
        .add_directive("rustible=debug".parse().unwrap())
        .add_directive("russh=warn".parse().unwrap());

    let subscriber = Registry::default()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_span_events(FmtSpan::CLOSE)
                .with_current_span(false)
                .with_span_list(true)
        );

    subscriber.init();
}
```

### 2. Wide Event Macros (`src/logging/wide_event.rs`)

```rust
// src/logging/wide_event.rs
use tracing::{span, Level, info, error, warn};
use serde_json::json;

#[macro_export]
macro_rules! wide_event {
    (
        $event_name:expr,
        $( $key:ident = $value:expr ),*
    ) => {
        {
            let fields = json!({
                $( stringify!($key): $value, )*
            });

            info!(
                event_name = $event_name,
                %fields
            );
        }
    };
}

// Usage:
wide_event!(
    "task_execution",
    task_name = "Install nginx",
    host_id = "web-01.example.com",
    status = "changed",
    duration_ms = 234.5
);
```

### 3. Sampling Layer (`src/logging/sampling.rs`)

```rust
// src/logging/sampling.rs
use tracing::Level;

pub struct SamplingDecision {
    pub should_log: bool,
    pub sampling_reason: String,
}

pub fn should_sample(
    level: &Level,
    event_name: &str,
    duration_ms: Option<f64>,
) -> SamplingDecision {
    // Always log errors
    if *level == Level::ERROR || *level == Level::WARN {
        return SamplingDecision {
            should_log: true,
            sampling_reason: "error_or_warn".to_string(),
        };
    }

    // Slow operations
    if let Some(duration) = duration_ms {
        if duration > 1000.0 { // 1 second threshold
            return SamplingDecision {
                should_log: true,
                sampling_reason: "slow_operation".to_string(),
            };
        }
    }

    // Sampling based on event type
    match event_name {
        "playbook_execution" => {
            if rand::random::<f32>() < 0.1 {
                return SamplingDecision {
                    should_log: true,
                    sampling_reason: "random".to_string(),
                };
            }
        }
        "task_execution" => {
            if rand::random::<f32>() < 0.05 {
                return SamplingDecision {
                    should_log: true,
                    sampling_reason: "random".to_string(),
                };
            }
        }
        _ => {}
    }

    SamplingDecision {
        should_log: false,
        sampling_reason: "sampled_out".to_string(),
    }
}
```

### 4. Instrumentation Layer (`src/logging/instrument.rs`)

```rust
// src/logging/instrument.rs
use tracing::{instrument, info, error, Span};

#[instrument(skip(self))]
pub trait InstrumentedTask {
    async fn execute_instrumented(&self, context: &TaskContext) -> Result<TaskResult> {
        info!(
            "Task started",
            task_name = %context.task_name,
            host_id = %context.host_id,
        );

        match self.execute(context).await {
            Ok(result) => {
                info!(
                    "Task completed",
                    task_name = %context.task_name,
                    status = %result.status,
                    changed = result.changed,
                    duration_ms = result.duration_ms,
                );
                Ok(result)
            }
            Err(e) => {
                error!(
                    "Task failed",
                    task_name = %context.task_name,
                    error = %e,
                    error_type = %e.error_type(),
                );
                Err(e)
            }
        }
    }
}
```

## Integration Points

### 1. SSH Module (`src/ssh/`)

```rust
// Add instrumentation to SSH connections
use tracing::{instrument, info, error};

#[instrument(skip(client, command))]
pub async fn execute_ssh_command(
    client: &RusshClient,
    command: &str,
) -> Result<String, SshError> {
    let start = std::time::Instant::now();

    info!(
        "SSH command starting",
        command = %command,
        host_id = %client.host(),
    );

    match client.run_command(command).await {
        Ok(output) => {
            let duration = start.elapsed().as_millis() as f64;

            info!(
                "SSH command completed",
                command = %command,
                duration_ms = duration,
                exit_code = output.exit_code,
                stdout_size = output.stdout.len(),
                stderr_size = output.stderr.len(),
            );

            Ok(output.stdout)
        }
        Err(e) => {
            let duration = start.elapsed().as_millis() as f64;

            error!(
                "SSH command failed",
                command = %command,
                duration_ms = duration,
                error = %e,
                error_type = "ssh_execution",
            );

            Err(e)
        }
    }
}
```

### 2. Task Engine (`src/tasks/`)

```rust
// Add wide events to task execution
use crate::logging::wide_event;

pub async fn execute_task(task: &Task, host: &Host) -> Result<TaskResult> {
    let start = std::time::Instant::now();

    wide_event!(
        "task_start",
        task_name = task.name,
        task_id = task.id,
        host_id = host.name,
        module_name = task.module,
    );

    let result = match task.module.as_str() {
        "package" => modules::package::execute(task, host).await,
        "service" => modules::service::execute(task, host).await,
        "file" => modules::file::execute(task, host).await,
        _ => Err(TaskError::UnknownModule(task.module.clone())),
    };

    let duration = start.elapsed().as_millis() as f64;

    match &result {
        Ok(task_result) => {
            wide_event!(
                "task_completion",
                task_name = task.name,
                task_id = task.id,
                host_id = host.name,
                status = task_result.status,
                changed = task_result.changed,
                duration_ms = duration,
            );
        }
        Err(e) => {
            wide_event!(
                "task_failure",
                task_name = task.name,
                task_id = task.id,
                host_id = host.name,
                error_type = e.error_type(),
                error_message = e.message(),
                duration_ms = duration,
            );
        }
    }

    result
}
```

### 3. Playbook Engine (`src/playbook/`)

```rust
// Add playbook-level instrumentation
use tracing::{info, instrument};

#[instrument(skip(playbook, inventory))]
pub async fn run_playbook(
    playbook: &Playbook,
    inventory: &Inventory,
) -> Result<PlaybookResult> {
    let start = std::time::Instant::now();

    info!(
        "Playbook execution started",
        playbook_name = playbook.name,
        playbook_path = playbook.path,
        host_count = inventory.hosts.len(),
        task_count = playbook.tasks.len(),
    );

    let mut results = vec![];

    for task in &playbook.tasks {
        let task_result = execute_tasks_parallel(task, inventory).await?;
        results.push(task_result);
    }

    let duration = start.elapsed().as_millis() as f64;
    let successful = results.iter().filter(|r| r.success).count();
    let failed = results.iter().filter(|r| !r.success).count();

    info!(
        "Playbook execution completed",
        playbook_name = playbook.name,
        duration_ms = duration,
        tasks_total = results.len(),
        tasks_successful = successful,
        tasks_failed = failed,
    );

    Ok(PlaybookResult {
        duration_ms: duration,
        tasks_successful: successful,
        tasks_failed: failed,
        results,
    })
}
```

## Configuration

### Environment Variables

```bash
# Enable JSON logging
export RUST_LOG=debug,rustible=info

# Enable verbose logging
export RUST_LOG=debug,rustible=trace

# Disable third-party noise
export RUST_LOG=rustible=debug,russh=warn,tokio=warn

# Log to file
export RUSTIBLE_LOG_FILE=/var/log/rustible/rustible.log

# Enable sampling
export RUSTIBLE_SAMPLE_SUCCESS=0.10
export RUSTIBLE_SAMPLE_SLOW_THRESHOLD_MS=1000
```

### Config File (`rustible.toml`)

```toml
[logging]
# Enable structured JSON logging
format = "json"

# Log level
level = "info"

# Log file path (optional, defaults to stdout)
file = "/var/log/rustible/rustible.log"

# Sampling configuration
[logging.sampling]
# Sample rate for successful operations (0.0 to 1.0)
success_rate = 0.10

# Slow operation threshold in milliseconds
slow_threshold_ms = 1000

# Always log these event patterns
always_log_patterns = [
    "error",
    "failure",
    "timeout",
    "ssh_connection"
]
```

## Log Querying Examples

### Query 1: Find Slow SSH Connections

```bash
# Using jq
jq 'select(.event_name == "ssh_connection" and .duration_ms > 500)' rustible.log

# Using grep -A (less efficient)
grep "ssh_connection" rustible.log | grep '"duration_ms": [5-9][0-9][0-9]'
```

### Query 2: Failed Tasks by Host

```bash
jq 'select(.event_name == "task_failure") | {host_id, task_name, error_type}' rustible.log
```

### Query 3: Most Common Errors

```bash
jq -r '.error_type' rustible.log | sort | uniq -c | sort -nr
```

### Query 4: P95 Task Duration by Module

```bash
jq 'select(.event_name == "task_completion" and .module_name == "package") | .duration_ms' rustible.log | \
  awk 'NR%5==0' | sort -n | tail -1
```

### Query 5: Trace All Events for a Playbook

```bash
jq 'select(.trace_id == "a1b2c3d4-e5f6-7890-abcd-ef1234567890")' rustible.log
```

### Query 6: Host Performance Summary

```bash
jq 'select(.event_name == "playbook_execution") | {
    host_id: .host_id,
    avg_duration_ms: .avg_play_time_per_host_ms,
    p99_duration_ms: .p99_play_time_ms,
    success_rate: (.hosts_successful / .hosts_total)
}' rustible.log
```

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sampling_decision_errors() {
        let decision = should_sample(
            &Level::ERROR,
            "task_execution",
            Some(100.0),
        );
        assert!(decision.should_log);
        assert_eq!(decision.sampling_reason, "error_or_warn");
    }

    #[test]
    fn test_sampling_decision_slow() {
        let decision = should_sample(
            &Level::INFO,
            "task_execution",
            Some(1500.0),
        );
        assert!(decision.should_log);
        assert_eq!(decision.sampling_reason, "slow_operation");
    }

    #[test]
    fn test_wide_event_macro() {
        wide_event!(
            "test_event",
            field1 = "value1",
            field2 = 42,
            field3 = true
        );
    }
}
```

### Integration Tests

```bash
# Run playbook and capture logs
rustible run playbook.yml -i inventory.yml > test.log 2>&1

# Verify wide events are present
jq 'select(.event_name == "playbook_execution")' test.log | \
  jq 'has("trace_id") and has("duration_ms") and has("hosts_total")' | \
  jq 'select(. == true)' | \
  wc -l
```

## Performance Impact

### Memory:
- ~200 bytes per log event (compressed JSON)
- With 1000 hosts, 100 events/host: 200 KB

### CPU:
- JSON serialization: ~10μs per event
- Sampling decision: ~1μs per event
- Total overhead: <2% of execution time

### Storage:
- 1000 hosts, 10 tasks/host: 10,000 events
- Avg event size: 2 KB → 20 MB per run
- With 100 runs/day: 2 GB/day
- Recommended: Rotate logs daily, keep 30 days

## Migration Strategy

### Phase 1: Add Tracing Layer (Week 1)
1. Initialize tracing in `main.rs`
2. Add `src/logging/` module
3. Implement wide event macros

### Phase 2: Instrument Core Modules (Week 2-3)
1. Add instrumentation to SSH module
2. Add instrumentation to task engine
3. Add instrumentation to playbook engine

### Phase 3: Implement Sampling (Week 4)
1. Add sampling layer
2. Implement adaptive sampling
3. Add sampling configuration

### Phase 4: Full Integration (Week 5-6)
1. Instrument all remaining modules
2. Add trace ID propagation
3. Update documentation

### Phase 5: Testing & Validation (Week 7)
1. Write unit tests
2. Write integration tests
3. Performance benchmarking

### Phase 6: Documentation & Examples (Week 8)
1. Write user documentation
2. Create query examples
3. Update README

## Success Metrics

### Before:
- Multiple log lines per operation
- String-based filtering
- No trace correlation
- Manual log aggregation

### After:
- 1 wide event per operation
- Queryable JSON logs
- Trace ID correlation across async ops
- Automated sampling and filtering
- 50+ fields per event for analytics

## Next Steps

1. Create `src/logging/` module structure
2. Implement wide event macros
3. Add tracing initialization to `main.rs`
4. Instrument SSH module first (critical path)
5. Instrument task engine
6. Implement sampling layer
7. Add configuration options
8. Write comprehensive tests
9. Update documentation
10. Release as feature flag (`structured-logging`)

---

*This plan follows the loggingsucks.com philosophy: wide events, structured JSON, trace propagation, intelligent sampling, and queryable logs.*
