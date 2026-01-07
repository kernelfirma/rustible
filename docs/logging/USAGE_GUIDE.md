# Rustible Structured Logging Guide

This guide explains how to use rustible's structured logging system based on the [loggingsucks.com philosophy](https://loggingsucks.com/).

## Overview

Rustible uses **wide-event logging** - one comprehensive JSON event per operation instead of scattered log lines. This enables:
- Queryable analytics (not just string search)
- Trace correlation across async operations
- Intelligent sampling (errors 100%, success 10%)
- Business context over code state

## Quick Start

### Initialize Logging

```rust
use rustible::logging::init_logging;

fn main() {
    init_logging();
}
```

### Log Wide Events

```rust
use rustible::logging::wide_event;

wide_event!(
    "task_execution",
    task_name = "Install nginx",
    host_id = "web-01.example.com",
    status = "changed",
    duration_ms = 234.5,
    packages_installed = vec!["nginx".to_string()],
);
```

### Log Errors (Always 100%)

```rust
use rustible::logging::wide_event_error;

wide_event_error!(
    "task_failure",
    task_name = "Start nginx",
    host_id = "web-01.example.com",
    error_type = "timeout",
    error_message = "Failed to start service",
);
```

## Event Structure

Every event contains 50+ fields organized into categories:

### Required Fields (Always Present)
- `trace_id` - Operation chain identifier
- `event_name` - Event type name
- `severity` - info/error/warn/debug
- `host_id` - Target host
- `status` - success/failure/skipped/changed

### Optional Fields (Context-Dependent)
- Performance: `duration_ms`, `cpu_time_ms`, `memory_bytes`
- Network: `ssh_host`, `ssh_port`, `ssh_connection_time_ms`
- Task: `task_name`, `module_name`, `task_id`
- Error: `error_type`, `error_message`, `error_stack_trace`
- Custom: `custom_fields` for extensibility

## Common Event Patterns

### 1. Playbook Execution

```rust
use rustible::logging::{wide_event, RustibleEvent};
use std::time::Instant;

pub async fn run_playbook(playbook: &Playbook, inventory: &Inventory) {
    let start = Instant::now();

    wide_event!(
        "playbook_execution",
        playbook_name = playbook.name,
        playbook_path = playbook.path,
        inventory_file = inventory.path,
        host_count = inventory.hosts.len() as u32,
        task_count = playbook.tasks.len() as u32,
        status = "started",
    );

    let result = execute_playbook(playbook, inventory).await;

    let duration = start.elapsed().as_millis() as f64;

    wide_event!(
        "playbook_execution",
        playbook_name = playbook.name,
        status = result.status,
        duration_ms = duration,
        hosts_successful = result.successful_count,
        hosts_failed = result.failed_count,
        duration_ms = duration,
    );
}
```

### 2. Task Execution

```rust
use rustible::logging::wide_event;

pub async fn execute_task(task: &Task, host: &Host) {
    let start = Instant::now();

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
        _ => Err(TaskError::UnknownModule(task.module.clone())),
    };

    let duration = start.elapsed().as_millis() as f64;

    match &result {
        Ok(task_result) => {
            wide_event!(
                "task_completion",
                task_name = task.name,
                host_id = host.name,
                status = task_result.status,
                changed = task_result.changed,
                duration_ms = duration,
            );
        }
        Err(e) => {
            wide_event_error!(
                "task_failure",
                task_name = task.name,
                host_id = host.name,
                error_type = e.error_type(),
                error_message = e.to_string(),
            );
        }
    }

    result
}
```

### 3. SSH Connection

```rust
use rustible::logging::wide_event;
use std::time::Instant;

pub async fn ssh_connect(host: &Host) -> Result<SshClient> {
    let start = Instant::now();

    wide_event!(
        "ssh_connection",
        ssh_host = host.hostname,
        ssh_port = host.port,
        ssh_user = host.username,
        status = "connecting",
    );

    let connection_time = Instant::now();
    let client = SshClient::connect(host).await?;

    let connect_duration = connection_time.elapsed().as_millis() as f64;
    let total_duration = start.elapsed().as_millis() as f64;

    wide_event!(
        "ssh_connection",
        ssh_host = host.hostname,
        ssh_port = host.port,
        ssh_user = host.username,
        ssh_auth_method = "key",
        ssh_connection_time_ms = connect_duration,
        duration_ms = total_duration,
        status = "success",
    );

    Ok(client)
}
```

## Sampling Behavior

The logging system automatically samples events to reduce volume:

### Always Logged (100%)
- All error events (`wide_event_error!`)
- All warnings
- Slow operations (>1000ms)
- SSH connection failures
- Events marked with `sampling_reason: "always"`

### Sampled Success Events
- Playbook executions: 10% random
- Task executions: 5% random
- Template renders: 2% random
- SSH connections (successful): 1% random

## Configuration

### Environment Variables

```bash
# Enable verbose logging
export RUST_LOG=debug,rustible=trace

# Log to file
export RUSTIBLE_LOG_FILE=/var/log/rustible/rustible.log

# Set sampling rate
export RUSTIBLE_SAMPLE_SUCCESS=0.05  # 5% sampling
export RUSTIBLE_SAMPLE_SLOW_THRESHOLD_MS=500
```

### Config File (`rustible.toml`)

```toml
[logging]
level = "info"
format = "json"
file = "/var/log/rustible/rustible.log"

[logging.sampling]
success_rate = 0.10
slow_threshold_ms = 1000
```

## Log Querying Examples

### Query 1: Find Slow SSH Connections

```bash
jq 'select(.event_name == "ssh_connection" and .duration_ms > 500)' rustible.log
```

### Query 2: Failed Tasks by Host

```bash
jq 'select(.event_name == "task_failure") | {host_id, task_name, error_type}' rustible.log
```

### Query 3: Most Common Errors

```bash
jq -r '.error_type' rustible.log | sort | uniq -c | sort -nr
```

### Query 4: Trace All Events for a Playbook

```bash
jq 'select(.trace_id == "a1b2c3d4-e5f6-7890-abcd-ef1234567890")' rustible.log
```

### Query 5: P95 Task Duration by Module

```bash
jq 'select(.event_name == "task_completion" and .module_name == "package") | .duration_ms' rustible.log | \
  awk 'NR%5==0' | sort -n | tail -1
```

## Best Practices

### 1. Emit One Event Per Operation

```rust
// Good: Single wide event with all context
wide_event!(
    "task_execution",
    task_name = "Install nginx",
    host_id = "web-01.example.com",
    status = "changed",
    packages_installed = vec!["nginx".to_string()],
    duration_ms = 234.5,
);

// Bad: Multiple scattered log lines
println!("Starting task: Install nginx");
println!("Installing package...");
println!("Package installed");
println!("Task completed");
```

### 2. Use Business Context

```rust
// Good: What happened to request
wide_event!(
    "playbook_execution",
    playbook_name = "deploy_webapp.yml",
    status = "success",
    hosts_changed = 42,
    duration_ms = 4523.7,
);

// Bad: What code is doing
println!("Running deployment loop");
println!("Iterating over hosts");
println!("Executing tasks");
```

### 3. Include Performance Metrics

```rust
// Good: Performance context
wide_event!(
    "task_execution",
    task_name = "Install nginx",
    host_id = "web-01.example.com",
    duration_ms = 234.5,
    cpu_time_ms = 45.2,
    memory_bytes = 10485760,
);

// Bad: No performance data
println!("Task completed");
```

### 4. Propagate Trace IDs

```rust
// Good: Trace ID across async operations
let trace_id = Uuid::new_v4().to_string();
tokio::spawn(async move {
    execute_task(trace_id.clone()).await;
});

// Bad: No trace correlation
tokio::spawn(async move {
    execute_task().await;
});
```

## Testing

### Test Logging

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rustible::logging::{wide_event, RustibleEvent};

    #[test]
    fn test_wide_event_macro() {
        wide_event!(
            "test_event",
            field1 = "value1",
            field2 = 42,
        );
    }

    #[test]
    fn test_event_builder() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_module("package".to_string())
        .with_duration(1_000_000_000);

        assert_eq!(event.module_name, Some("package".to_string()));
        assert_eq!(event.duration_ms, 1000.0);
    }
}
```

## Migration from Traditional Logging

### Before (Scattered Log Lines)

```rust
println!("[INFO] Connecting to web-01.example.com");
println!("[INFO] SSH connection established");
println!("[DEBUG] Executing: apt-get install -y nginx");
println!("[INFO] Package installed: nginx");
println!("[INFO] Task completed");
```

### After (Single Wide Event)

```rust
wide_event!(
    "task_execution",
    task_name = "Install nginx",
    host_id = "web-01.example.com",
    status = "changed",
    packages_installed = vec!["nginx".to_string()],
    duration_ms = 234.5,
    ssh_connection_time_ms = 45.2,
);
```

## Performance Impact

### Memory
- ~200 bytes per event (compressed JSON)
- 10,000 events → 2 MB

### CPU
- JSON serialization: ~10μs per event
- Sampling decision: ~1μs per event
- Total overhead: <2% of execution time

### Storage
- 1,000 hosts, 10 tasks/host: 10,000 events
- Avg event size: 2 KB → 20 MB per run
- 100 runs/day → 2 GB/day

## Troubleshooting

### No Events Appearing

```rust
// Ensure logging is initialized
use rustible::logging::init_logging;

fn main() {
    init_logging();
}
```

### Too Many Events

```bash
# Adjust sampling rate
export RUSTIBLE_SAMPLE_SUCCESS=0.01  # 1% instead of 10%
```

### Events Not Queryable

```bash
# Verify JSON format
jq . rustible.log | head -1

# Check for parse errors
jq . rustible.log 2>&1 | grep "parse error"
```

## References

- [Logging Sucks Philosophy](https://loggingsucks.com/)
- [Tracing Documentation](https://docs.rs/tracing/)
- [serde_json Documentation](https://docs.rs/serde_json/)

---

For questions or contributions, visit the rustible repository.
