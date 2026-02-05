# Execution Engine & Reliability Capabilities

> **Last Updated:** 2026-02-05
> **Rustible Version:** 0.1.x
> **HPC Initiative Phase:** 1B - Execution, Concurrency, and Failure Handling

This document provides a comprehensive capability matrix for Rustible's execution engine, concurrency controls, retry/timeout behavior, and reliability features.

---

## Quick Reference

| Category | Capability | Status | HPC-Ready |
|----------|-----------|--------|-----------|
| Execution Strategies | Linear, Free, HostPinned | ✅ Stable | ✅ Yes |
| Concurrency Control | Fork-based parallelism | ✅ Stable | ✅ Yes |
| Work Stealing | Dynamic load balancing | ✅ Stable | ✅ Yes |
| Batch Processing | Loop coalescing (87x speedup) | ✅ Stable | ✅ Yes |
| Retry Policies | 5 backoff strategies | ✅ Stable | ✅ Yes |
| Checkpoints | Save/resume execution | ✅ Stable | ✅ Yes |
| Rollback | State change undo | ✅ Stable | ✅ Yes |
| Circuit Breakers | Fault isolation | ✅ Stable | ✅ Yes |
| Check Mode | Dry-run execution | ✅ Stable | ✅ Yes |
| Plan Mode | Terraform-style preview | ✅ Stable | ✅ Yes |
| Block/Rescue/Always | Error handling blocks | ✅ Stable | ✅ Yes |
| Throttling | Rate limiting | ✅ Stable | ✅ Yes |
| Idempotency | Module-level guarantees | ✅ Stable | ✅ Yes |

---

## 1. Execution Strategies

### 1.1 Strategy Types

| Strategy | Description | Use Case | Evidence |
|----------|-------------|----------|----------|
| **Linear** | All hosts complete task N before task N+1 | Sequential deployments, rolling updates | [`src/executor/strategies.rs:42-192`](../../src/executor/strategies.rs#L42) |
| **Free** | Each host runs independently through all tasks | Independent servers, max parallelism | [`src/executor/strategies.rs:193-372`](../../src/executor/strategies.rs#L193) |
| **HostPinned** | Dedicated worker thread per host | Long-running playbooks, connection reuse | [`src/executor/host_pinned.rs:21-46`](../../src/executor/host_pinned.rs#L21) |

### 1.2 Linear Strategy Details

```yaml
# Default Ansible-compatible behavior
# All hosts complete each task before moving to next
strategy: linear
```

**Characteristics:**
- Preserves task ordering across hosts
- Supports block/rescue/always error handling
- Tracks failed/rescued blocks per host
- Connection pooling for SSH efficiency

**Source:** [`src/executor/strategies.rs:42-192`](../../src/executor/strategies.rs#L42)

### 1.3 Free Strategy Details

```yaml
# Maximum parallelism - each host independent
strategy: free
```

**Characteristics:**
- Each host gets independent task execution
- No cross-host synchronization points
- Optimal for stateless, independent servers
- Connection pooling with async execution

**Source:** [`src/executor/strategies.rs:193-372`](../../src/executor/strategies.rs#L193)

### 1.4 Host-Pinned Strategy Details

```yaml
# Dedicated worker per host (persistent connections)
strategy: host_pinned
```

**Configuration:**
| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_hosts` | 50 | Maximum concurrent host workers |
| `queue_depth` | 100 | Task queue per host |
| `idle_timeout` | 60s | Worker cleanup after idle |
| `enable_coalescing` | true | Batch similar tasks |
| `keepalive_interval` | 30s | SSH keepalive |

**Source:** [`src/executor/host_pinned.rs:21-46`](../../src/executor/host_pinned.rs#L21)

---

## 2. Concurrency Control

### 2.1 Fork-Based Parallelism

| Parameter | Default | Range | Description |
|-----------|---------|-------|-------------|
| `forks` | 5 | 1-∞ | Maximum concurrent host connections |
| `serial` | N/A | 1-N or % | Batch size for rolling updates |

**Configuration (rustible.yml):**
```yaml
executor:
  forks: 50              # Concurrent hosts
  task_timeout: 300      # Seconds per task
  gather_facts: true     # Auto-gather on play start
```

**Source:** [`src/executor/config.rs:1-143`](../../src/executor/config.rs)

### 2.2 Work-Stealing Scheduler

Dynamic load balancing across worker threads:

| Config | I/O-Bound | CPU-Bound | Description |
|--------|-----------|-----------|-------------|
| `num_workers` | 2×CPU | 1×CPU | Worker thread count |
| `steal_threshold` | 1 | 4 | Min items before stealing |
| `batch_steal` | true | true | Steal half of queue at once |
| `spin_count` | 8 | 64 | Spins before parking |

**Statistics Available:**
- `queue_sizes`: Items in each worker queue
- `queue_weights`: Total weight per queue
- `items_processed`: Completed work items
- `items_stolen`: Work stolen between workers
- `load_imbalance()`: 0.0=perfect, 1.0=all on one queue
- `steal_ratio()`: Percentage of work stolen

**Source:** [`src/executor/work_stealing.rs:1-999`](../../src/executor/work_stealing.rs)

### 2.3 Batch Processing (Loop Optimization)

Addresses Ansible's 87x loop slowdown by coalescing operations:

| Strategy | Modules | Speedup | Description |
|----------|---------|---------|-------------|
| `PackageList` | apt, yum, dnf, pip, package | ~80% | Single install call |
| `CommandPipeline` | command, shell | ~60% | Single SSH session |
| `ParallelTransfer` | copy, template, fetch | ~30% | Connection reuse |
| `Generic` | others | ~20% | Grouped execution |

**Configuration:**
```rust
BatchConfig {
    enabled: true,
    max_batch_size: 100,
    min_batch_size: 2,
    accumulation_timeout: 50ms,
}
```

**Source:** [`src/executor/batch_processor.rs:1-595`](../../src/executor/batch_processor.rs)

---

## 3. Throttling & Rate Limiting

### 3.1 Throttle Configuration

| Parameter | Type | Description |
|-----------|------|-------------|
| `global_limit` | usize | Max concurrent tasks globally |
| `per_host_limit` | usize | Max concurrent tasks per host |
| `module_rate_limits` | HashMap | Rate limits by module |
| `rate_per_second` | f64 | Target ops/second |
| `burst_size` | usize | Token bucket burst |

**Token Bucket Algorithm:**
- Refills at `rate_per_second`
- Allows bursts up to `burst_size`
- Blocks when bucket empty

**Source:** [`src/executor/throttle.rs`](../../src/executor/throttle.rs)

---

## 4. Retry Policies

### 4.1 Backoff Strategies

| Strategy | Formula | Best For |
|----------|---------|----------|
| **Constant** | `delay` | Predictable failures |
| **Linear** | `base + (attempt × base)` | Gradually increasing load |
| **Exponential** | `base × 2^attempt` | Network failures, rate limits |
| **Fibonacci** | `fib(attempt) × base` | Balanced escalation |
| **DecorrelatedJitter** | `random(base, prev×3)` | Thundering herd prevention |

### 4.2 Default Retry Configuration

```rust
RetryPolicy {
    max_retries: 3,
    backoff: Exponential { base: 1s, max: 60s },
    max_duration: 5min,
    jitter: 0.1,  // 10% random variance
    retryable_errors: [Timeout, Connection, Transient],
}
```

### 4.3 Retry Policy Methods

| Method | Description |
|--------|-------------|
| `should_retry(attempt, error)` | Check if retry appropriate |
| `delay_for(attempt)` | Calculate delay with jitter |
| `with_jitter(factor)` | Add randomization |
| `with_timeout(duration)` | Set max retry duration |

**Source:** [`src/recovery/retry.rs`](../../src/recovery/retry.rs)

---

## 5. Checkpoint & Resume

### 5.1 Checkpoint Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `checkpoint_dir` | `/tmp/rustible/checkpoints` | Storage location |
| `auto_checkpoint_interval` | 20 tasks | Auto-save frequency |
| `compress` | false | Gzip compression |
| `max_age_hours` | 24h | Expiration time |
| `max_checkpoints_per_playbook` | 5 | Retention limit |
| `include_results` | true | Store task results |
| `include_variables` | true | Store variable state |

### 5.2 Production Configuration

```rust
CheckpointConfig::production() = {
    checkpoint_dir: "/var/lib/rustible/checkpoints",
    auto_checkpoint_interval: 10,
    compress: true,
    max_age_hours: 72,
    max_checkpoints_per_playbook: 10,
    include_results: true,
    include_variables: true,
}
```

### 5.3 Checkpoint State Tracking

| Level | Fields Tracked |
|-------|---------------|
| **Playbook** | name, total_plays, total_tasks, completed_tasks, current_play/task |
| **Host** | current_play, current_task, failed, unreachable, variables |
| **Task** | name, index, status, result, completed_at |

**Task Statuses:** Pending, InProgress, Completed, Failed, Skipped

**Source:** [`src/recovery/checkpoint.rs:1-753`](../../src/recovery/checkpoint.rs)

---

## 6. Rollback & State Management

### 6.1 State Change Types

| Change Type | Undo Operation | Priority |
|-------------|---------------|----------|
| `FileCreated` | Delete file | 10 |
| `FileModified` | Restore from backup | 20 |
| `FileDeleted` | Restore from backup | 20 |
| `DirectoryCreated` | Delete directory (recursive) | 5 |
| `ServiceStateChanged` | Restore previous state | 30 |
| `PackageInstalled` | Remove package | 15 |
| `PackageRemoved` | Reinstall package | 15 |
| `UserCreated` | Delete user | 25 |
| `UserModified` | Restore previous state | 25 |
| `UserDeleted` | Restore user | 25 |
| `Custom` | Execute undo command | 0 |

### 6.2 Rollback Context States

```
Active → RollingBack → RolledBack
   ↓           ↓
   ↓      → Failed
   ↓
   → Committed (no rollback needed)
```

### 6.3 Rollback Plan Generation

```rust
// Rollback actions are created in reverse order
// and sorted by priority (higher = execute first)
manager.create_rollback_plan(&context_id)?;
```

**Source:** [`src/recovery/rollback.rs:1-1014`](../../src/recovery/rollback.rs)

---

## 7. Circuit Breaker Pattern

### 7.1 Circuit States

| State | Behavior |
|-------|----------|
| **Closed** | Normal operation, tracking failures |
| **Open** | All calls fail immediately |
| **HalfOpen** | Allow limited calls to test recovery |

### 7.2 Configuration

| Parameter | Description |
|-----------|-------------|
| `failure_threshold` | Failures before opening |
| `success_threshold` | Successes to close (in half-open) |
| `timeout` | Time before half-open attempt |
| `volume_threshold` | Min calls before tripping |

**Source:** [`src/recovery/mod.rs`](../../src/recovery/mod.rs)

---

## 8. Check Mode (Dry-Run)

### 8.1 CLI Usage

```bash
# Run playbook in check mode
rustible check playbook.yml -i inventory.yml

# Equivalent to:
rustible run playbook.yml -i inventory.yml --check
```

### 8.2 Behavior

| Module Type | Check Mode Behavior |
|-------------|---------------------|
| File modules | Report what would change |
| Package modules | Report install/remove |
| Service modules | Report state changes |
| Command/shell | Always "changed" (cannot predict) |
| Debug/set_fact | Execute normally |

**Source:** [`src/cli/commands/check.rs:1-126`](../../src/cli/commands/check.rs)

---

## 9. Plan Mode (Terraform-Style Preview)

### 9.1 Action Types

| Symbol | Type | Description |
|--------|------|-------------|
| `+` | Create | Resource will be created |
| `~` | Modify | Resource will be modified |
| `-` | Delete | Resource will be deleted |
| ` ` | NoChange | No action needed |
| `?` | Unknown | Cannot determine (command/shell) |

### 9.2 Output Format

```
─────────────────────────────────────────────────
Plan: 3 to add, 2 to change, 1 to destroy
      across 5 host(s)
─────────────────────────────────────────────────

Host: web1.example.com:
  2 to add, 1 to change, 0 to destroy

  + Install nginx (apt)
    Resource: nginx
    will create package: nginx

  ~ Configure nginx.conf (template)
    Resource: /etc/nginx/nginx.conf
    will modify from template: /etc/nginx/nginx.conf

    --- before
    +++ after
    @@ -1,3 +1,3 @@
    -worker_connections 1024;
    +worker_connections 2048;
```

**Source:** [`src/cli/plan.rs:1-673`](../../src/cli/plan.rs)

---

## 10. Block/Rescue/Always Error Handling

### 10.1 Block Structure

```yaml
- block:
    - name: Try risky operation
      command: /opt/risky.sh
    - name: Another task
      service: name=app state=restarted
  rescue:
    - name: Handle failure
      debug: msg="Block failed, running rescue"
    - name: Restore backup
      copy: src=/backup/config dest=/etc/app/config
  always:
    - name: Cleanup
      file: path=/tmp/lock state=absent
```

### 10.2 Block Execution Logic

| Phase | Execution Condition |
|-------|---------------------|
| **block** | Always runs first |
| **rescue** | Runs if any block task fails (and not already rescued) |
| **always** | Always runs regardless of block/rescue outcome |

### 10.3 Block State Tracking

```rust
struct BlockState {
    failed: bool,        // Block task failed
    rescue_failed: bool, // Rescue task failed
    always_started: bool, // Always section started
}
```

**Source:** [`src/executor/strategies.rs:616-810`](../../src/executor/strategies.rs#L616)

---

## 11. Idempotency Guarantees

### 11.1 Module-Level Idempotency

| Module | Idempotency | Mechanism |
|--------|-------------|-----------|
| `file` | ✅ Full | State comparison |
| `copy` | ✅ Full | Checksum comparison |
| `template` | ✅ Full | Content hash |
| `package` | ✅ Full | Installed version check |
| `service` | ✅ Full | State query |
| `user`/`group` | ✅ Full | Attribute comparison |
| `lineinfile` | ✅ Full | Line presence check |
| `blockinfile` | ✅ Full | Block presence check |
| `command` | ⚠️ Conditional | `creates`/`removes` checks |
| `shell` | ⚠️ Conditional | `creates`/`removes` checks |

### 11.2 Idempotency Helpers

```yaml
# Command with idempotency guard
- name: Create database
  command: createdb myapp
  args:
    creates: /var/lib/pgsql/data/myapp

# Shell with removal guard
- name: Run cleanup
  shell: rm -rf /tmp/cache/*
  args:
    removes: /tmp/cache
```

**Source:** [`src/traits.rs`](../../src/traits.rs) (Idempotent trait)

---

## 12. Timeout Configuration

### 12.1 Timeout Levels

| Level | Default | Override |
|-------|---------|----------|
| Task timeout | 300s | `timeout:` in task |
| Connection timeout | 30s | `ansible_connection_timeout` |
| SSH timeout | 10s | `ansible_ssh_timeout` |
| Gather facts timeout | 60s | `gather_timeout` |

### 12.2 Example Configuration

```yaml
- name: Long-running task
  command: /opt/build.sh
  timeout: 3600  # 1 hour

- name: Quick check
  command: ping -c 1 host
  timeout: 10
```

---

## 13. Partial Failure Handling

### 13.1 Host Failure Modes

| Mode | Behavior |
|------|----------|
| `any_errors_fatal: false` | Continue on other hosts |
| `any_errors_fatal: true` | Abort entire play |
| `max_fail_percentage: N` | Abort if >N% hosts fail |
| `ignore_errors: true` | Task-level ignore |
| `ignore_unreachable: true` | Skip unreachable hosts |

### 13.2 Task-Level Control

```yaml
- name: Non-critical task
  command: /opt/optional.sh
  ignore_errors: true
  register: optional_result

- name: Check result
  debug: msg="Optional failed but continuing"
  when: optional_result is failed
```

---

## 14. Recovery Manager Integration

### 14.1 Recovery Strategies

| Strategy | Description |
|----------|-------------|
| **Retry** | Re-attempt failed operations |
| **Checkpoint** | Save/resume execution state |
| **Rollback** | Undo completed changes |
| **Degrade** | Graceful degradation |

### 14.2 Production Recovery Config

```rust
RecoveryConfig::production() = {
    retry_policy: RetryPolicy::exponential(),
    checkpoint_enabled: true,
    rollback_enabled: true,
    graceful_degradation: true,
    circuit_breaker: CircuitBreakerConfig::default(),
}
```

**Source:** [`src/recovery/mod.rs`](../../src/recovery/mod.rs)

---

## 15. Known Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| No distributed checkpoints | Multi-node resume requires coordination | Use shared storage for checkpoint dir | v1.0 |
| Rollback requires backup paths | Must pre-configure backup locations | Always specify `backup_path` for modified files | N/A |
| Command/shell idempotency | Cannot predict state changes | Use `creates`/`removes` guards | N/A |
| No transaction isolation | Concurrent plays may conflict | Use serial execution for critical sections | v1.0 |

---

## 16. HPC-Specific Considerations

### 16.1 Recommended Settings for Large Clusters

```yaml
# rustible.yml for 1000+ nodes
executor:
  forks: 100                    # High parallelism
  task_timeout: 600             # Allow for slow nodes
  gather_facts: false           # Explicit fact gathering
  strategy: free                # Maximum parallelism

recovery:
  checkpoint_enabled: true
  checkpoint_interval: 50       # Frequent saves
  retry_max: 5
  retry_backoff: exponential

throttle:
  global_limit: 500             # Prevent thundering herd
  per_host_limit: 10
```

### 16.2 Batch Processing for Package Operations

```yaml
# Optimized for large package lists
- name: Install packages (batched)
  apt:
    name: "{{ packages }}"      # Pass list, not loop
    state: present
  vars:
    packages:
      - nginx
      - vim
      - htop
      # ... many more
```

---

## Evidence Links

All capabilities documented above are implemented in the codebase:

| Feature | Source File |
|---------|-------------|
| Execution strategies | `src/executor/strategies.rs` |
| Work-stealing scheduler | `src/executor/work_stealing.rs` |
| Batch processor | `src/executor/batch_processor.rs` |
| Host-pinned workers | `src/executor/host_pinned.rs` |
| Retry policies | `src/recovery/retry.rs` |
| Checkpoint/resume | `src/recovery/checkpoint.rs` |
| Rollback manager | `src/recovery/rollback.rs` |
| Check mode CLI | `src/cli/commands/check.rs` |
| Plan output | `src/cli/plan.rs` |
| Block/rescue/always | `src/executor/strategies.rs:616-810` |
| Throttling | `src/executor/throttle.rs` |
| Task execution | `src/executor/task.rs` |
| Executor config | `src/executor/config.rs` |

---

*For inventory and state management capabilities, see [provisioning-state-capabilities.md](./provisioning-state-capabilities.md)*
*For Terraform integration details, see [terraform.md](./terraform.md)*
