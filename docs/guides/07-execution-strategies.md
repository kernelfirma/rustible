---
summary: Execution strategies (linear, free, host-pinned, debug) and parallelization controls including forks, serial, and async.
read_when: You want to control how tasks are distributed across hosts, tune parallelism, or implement rolling updates.
---

# Chapter 7: Execution Strategies - Parallelization

Execution strategies control how Rustible distributes and orders tasks across your hosts. Choosing the right strategy and tuning parallelism parameters can dramatically affect both the speed and safety of your automation runs.

## Strategy Overview

Rustible provides four built-in execution strategies:

| Strategy | Task Order | Use Case |
|----------|-----------|----------|
| `linear` | All hosts complete task N before task N+1 | Default, predictable |
| `free` | Each host runs independently | Maximum throughput |
| `host_pinned` | Dedicated worker per host | Connection reuse, cache locality |
| `debug` | Step-by-step with logging | Troubleshooting |

### Linear Strategy (Default)

The linear strategy runs each task on all targeted hosts before moving to the next task. This provides a predictable execution order and is the safest choice for most workloads.

```
Task 1: host1 -> host2 -> host3  (all complete)
Task 2: host1 -> host2 -> host3  (all complete)
Task 3: host1 -> host2 -> host3  (all complete)
```

```yaml
- hosts: webservers
  strategy: linear    # This is the default
  tasks:
    - name: Stop service
      service:
        name: myapp
        state: stopped

    - name: Deploy code
      copy:
        src: app.tar.gz
        dest: /opt/myapp/

    - name: Start service
      service:
        name: myapp
        state: started
```

Linear is the right choice when:

- Tasks on one host depend on tasks completing on other hosts
- You need predictable ordering for debugging
- You want to detect and halt on failures before proceeding

### Free Strategy

The free strategy lets each host run through the task list independently at maximum speed. Hosts do not wait for each other between tasks.

```
host1: Task 1 -> Task 2 -> Task 3  (runs at its own pace)
host2: Task 1 -> Task 2 -> Task 3  (runs at its own pace)
host3: Task 1 -> Task 2 -> Task 3  (runs at its own pace)
```

```yaml
- hosts: webservers
  strategy: free
  tasks:
    - name: Update packages
      package:
        name: '*'
        state: latest

    - name: Restart service
      service:
        name: myapp
        state: restarted
```

Free is the right choice when:

- Tasks are independent across hosts
- You want maximum throughput
- Host synchronization is not needed

### Host-Pinned Strategy

The host-pinned strategy assigns a dedicated worker to each host. All tasks for a given host run on the same worker, optimizing SSH connection reuse and CPU cache locality.

```yaml
- hosts: webservers
  strategy: host_pinned
  tasks:
    - name: Step 1
      command: echo "step 1"
    - name: Step 2
      command: echo "step 2"
    - name: Step 3
      command: echo "step 3"
```

Host-pinned is the right choice when:

- Playbooks have many tasks per host
- Connection setup cost is significant
- You want the throughput benefits of `free` with better resource locality

### Debug Strategy

The debug strategy executes tasks one at a time with detailed logging. It is intended for troubleshooting failing playbooks.

```yaml
- hosts: webservers
  strategy: debug
  tasks:
    - name: Suspect task
      command: /opt/app/problematic-script.sh
```

You can also enable debug strategy from the command line without modifying the playbook:

```bash
rustible run playbook.yml --strategy debug
```

## Forks: Controlling Parallelism

The `--forks` flag (or `forks` in configuration) controls how many hosts are processed simultaneously. The default is 10.

```bash
# Process 20 hosts at a time
rustible run playbook.yml --forks 20

# Process one host at a time (useful for debugging)
rustible run playbook.yml --forks 1
```

With the linear strategy, forks determines how many hosts execute the current task in parallel. With the free strategy, forks determines how many hosts run their task lists concurrently.

### Choosing a Fork Count

| Scenario | Recommended Forks |
|----------|------------------|
| Small inventory (<10 hosts) | Default (10) |
| Medium inventory (10-100 hosts) | 20-50 |
| Large inventory (100+ hosts) | 50-100 |
| Control node with limited CPU/memory | Lower values |
| Tasks that are mostly waiting (network I/O) | Higher values |

## Serial Execution: Rolling Updates

The `serial` keyword controls how many hosts are processed per batch in a play. This is essential for rolling updates where you cannot take all hosts offline at once.

### Fixed Count

```yaml
- hosts: webservers
  serial: 2
  tasks:
    - name: Stop app
      service:
        name: myapp
        state: stopped

    - name: Deploy
      copy:
        src: app.tar.gz
        dest: /opt/myapp/

    - name: Start app
      service:
        name: myapp
        state: started
```

With 6 webservers, this runs the entire play on 2 hosts at a time (3 batches).

### Percentage-Based

```yaml
- hosts: webservers
  serial: "25%"
  tasks:
    - name: Rolling deploy
      command: /opt/deploy.sh
```

### Escalating Serial

Gradually increase batch size to detect problems early:

```yaml
- hosts: webservers
  serial:
    - 1       # First batch: 1 host (canary)
    - 5       # Second batch: 5 hosts
    - "25%"   # Remaining: 25% at a time
  max_fail_percentage: 10
  tasks:
    - name: Deploy application
      command: /opt/deploy.sh
```

### max_fail_percentage

Stop the rolling update if too many hosts fail:

```yaml
- hosts: webservers
  serial: 5
  max_fail_percentage: 20   # Abort if >20% of hosts fail
  tasks:
    - name: Deploy
      command: /opt/deploy.sh
```

## Async Execution

For long-running tasks, use `async` and `poll` to avoid blocking the connection:

```yaml
tasks:
  - name: Run long database migration
    command: /opt/scripts/migrate.sh
    async: 3600     # Maximum runtime in seconds
    poll: 30        # Check every 30 seconds

  - name: Fire and forget
    command: /opt/scripts/background-job.sh
    async: 3600
    poll: 0         # Do not wait for completion

  - name: Check on background job later
    async_status:
      jid: "{{ background_job.ansible_job_id }}"
    register: job_result
    until: job_result.finished
    retries: 60
    delay: 10
```

When `poll: 0` is set, the task starts the command and immediately moves on. You can check the result later using `async_status`.

## Connection Pooling

Rustible maintains a pool of SSH connections that are reused across tasks for the same host. This eliminates the overhead of establishing a new SSH handshake for every task.

Key behaviors:

- Connections are established on first use and kept alive
- Multiple tasks to the same host reuse the same connection
- The host-pinned strategy maximizes pooling benefits by keeping the same worker per host
- Connection timeouts and keepalives are configurable

This pooling is one of the main reasons Rustible achieves up to 11x faster SSH operations compared to Ansible's ControlMaster approach.

## Performance Comparison

| Aspect | Ansible | Rustible |
|--------|---------|----------|
| Default parallelism | 5 forks | 10 forks |
| Connection reuse | SSH ControlMaster | Built-in pooling (11x faster) |
| Module execution | Python interpreter per task | Native Rust or cached connection |
| Strategy overhead | Higher (Python GIL) | Lower (async Rust) |
| Memory per fork | ~50-100 MB | ~10-25 MB |

## Strategy Selection Heuristics

Rustible includes automatic strategy selection for small workloads. For very small workloads (1 host or 1 task), the linear strategy is always used to avoid unnecessary overhead.

For larger workloads, consider these guidelines:

| Workload Pattern | Recommended Strategy |
|-----------------|---------------------|
| Standard deployments | `linear` |
| Package updates, backups | `free` |
| Long playbooks, many tasks per host | `host_pinned` |
| Troubleshooting failures | `debug` |
| Rolling updates | `linear` + `serial` |

## Best Practices

1. **Start with linear**. It is the safest default and makes failures easy to diagnose.
2. **Use serial for production deployments** to limit blast radius. Start with a canary batch of 1.
3. **Increase forks gradually**. Monitor control node CPU and memory when raising fork counts.
4. **Use free strategy for independent operations** like package updates or log rotation where host ordering does not matter.
5. **Set async for long tasks** to avoid SSH timeouts on operations that take more than a few minutes.
6. **Combine max_fail_percentage with serial** to automatically halt rolling updates when problems are detected.

## Next Steps

- Learn about [Security and Vault](08-security.md)
- Explore [Templating](09-templating.md)
- See [Performance Tuning](performance-tuning.md) for advanced optimization
