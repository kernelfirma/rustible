---
summary: Comprehensive performance optimization guide covering connection tuning, execution strategies, playbook optimization, and benchmarking.
read_when: You need to optimize execution speed, reduce memory usage, or tune for specific network conditions.
---

# Performance Tuning Guide

This guide provides comprehensive recommendations for optimizing Rustible performance across different deployment scenarios.

## Table of Contents

- [Understanding Performance](#understanding-performance)
- [Connection Optimization](#connection-optimization)
- [Execution Strategies](#execution-strategies)
- [Playbook Optimization](#playbook-optimization)
- [Module Selection](#module-selection)
- [Memory Management](#memory-management)
- [Network Optimization](#network-optimization)
- [Benchmarking Your Setup](#benchmarking-your-setup)

---

## Understanding Performance

### Rustible Performance Advantages

Rustible is significantly faster than Ansible due to:

| Optimization | Impact |
|-------------|--------|
| Connection pooling | 11x faster SSH operations |
| Compiled modules | 40-70x faster module load |
| Native async | 2x better parallel scaling |
| Zero-copy architecture | Lower memory, faster parsing |

### Performance Bottlenecks

Common bottlenecks to address:

1. **Network latency**: High latency to remote hosts
2. **Connection overhead**: Establishing SSH connections
3. **Fact gathering**: Collecting host information
4. **Module execution**: Running modules on remote hosts
5. **File transfers**: Copying files to remote hosts

---

## Connection Optimization

### SSH Configuration

Optimize SSH settings for your environment:

```toml
# rustible.toml
[ssh]
# Enable connection reuse (automatic in Rustible)
control_master = true
control_persist = 300  # Keep connections for 5 minutes

# Enable pipelining (reduces round-trips)
pipelining = true

# Disable host key checking (only in trusted networks!)
# host_key_checking = false

# Connection timeout
timeout = 30
```

### SSH Server Configuration

On target hosts, optimize sshd:

```bash
# /etc/ssh/sshd_config
MaxSessions 20              # Allow multiple sessions
MaxStartups 30:50:100       # Handle connection bursts
UseDNS no                   # Skip reverse DNS lookup
```

### Connection Pool Sizing

Configure pool size based on workload:

```toml
[defaults]
# Small deployments (<10 hosts)
forks = 10

# Medium deployments (10-100 hosts)
forks = 20

# Large fleets (100+ hosts)
forks = 50  # May need to lower pool_size
```

### Reduce Connection Time

Use key-based authentication with Ed25519 keys (fastest):

```bash
# Generate Ed25519 key (faster than RSA)
ssh-keygen -t ed25519 -f ~/.ssh/deploy_key

# Configure in rustible.toml
private_key_file = "~/.ssh/deploy_key"
```

---

## Execution Strategies

### Strategy Selection

Choose the right strategy for your workload:

| Strategy | Use Case | Speed |
|----------|----------|-------|
| `linear` | Tasks with dependencies | Baseline |
| `free` | Independent tasks | 2x faster |
| `host_pinned` | Stateful operations | Moderate |

### Free Strategy (Fastest)

Use when tasks don't depend on each other:

```yaml
- hosts: webservers
  strategy: free
  tasks:
    - name: Independent task 1
      command: /opt/app/update.sh

    - name: Independent task 2
      command: /opt/app/reload-config.sh
```

### Serial Execution for Safety

Balance speed with safety:

```yaml
# Progressive deployment
- hosts: webservers
  serial: [1, "25%", "100%"]  # Canary pattern
  max_fail_percentage: 10

  tasks:
    - name: Deploy application
      # ...
```

Serial options explained:

```yaml
# Fixed number
serial: 2  # 2 hosts at a time

# Percentage
serial: "50%"  # Half of hosts at a time

# Progressive
serial: [1, 5, 10]  # 1, then 5, then 10
serial: ["10%", "50%", "100%"]  # Canary pattern
```

### Parallel Host Limits

Tune parallelism:

```bash
# For most environments (default)
rustible run playbook.yml -f 10

# For high-bandwidth networks
rustible run playbook.yml -f 50

# For limited resources
rustible run playbook.yml -f 5
```

---

## Playbook Optimization

### Disable Fact Gathering

Fact gathering adds 3-5 seconds per host:

```yaml
# Skip facts when not needed
- hosts: all
  gather_facts: false
  tasks:
    - name: Quick operation
      command: echo "hello"

# Gather facts only when needed
- hosts: all
  gather_facts: false
  tasks:
    - name: Gather facts now
      setup:
      when: need_facts | default(false)
```

### Selective Fact Gathering

Gather only what you need:

```yaml
- hosts: all
  gather_facts: true
  gather_subset:
    - network
    - hardware
  # Skips: software, virtual, facter, ohai
```

### Use Tags for Partial Runs

Skip unnecessary tasks:

```yaml
tasks:
  - name: Full system update
    package:
      name: '*'
      state: latest
    tags:
      - slow
      - update

  - name: Deploy application
    copy:
      src: app.tar.gz
      dest: /opt/
    tags:
      - deploy
```

Run only what you need:

```bash
# Skip slow operations
rustible run playbook.yml --skip-tags slow

# Run only deployment
rustible run playbook.yml --tags deploy
```

### Batch File Operations

Reduce transfer overhead:

```yaml
# Slow: Many small transfers
- name: Copy files individually
  copy:
    src: "{{ item }}"
    dest: /opt/app/
  loop: "{{ files }}"  # 100 files = 100 transfers

# Fast: Single archive transfer
- name: Create local archive
  local_action:
    module: archive
    path: files/
    dest: /tmp/app.tar.gz
  run_once: true

- name: Deploy archive
  unarchive:
    src: /tmp/app.tar.gz
    dest: /opt/app/
```

### Minimize Loops

Loops add overhead:

```yaml
# Slower: Loop with individual operations
- name: Install packages one by one
  package:
    name: "{{ item }}"
    state: present
  loop:
    - nginx
    - curl
    - htop

# Faster: Single operation with list
- name: Install all packages at once
  package:
    name:
      - nginx
      - curl
      - htop
    state: present
```

### Reduce Handler Runs

Handlers run once at end of play:

```yaml
tasks:
  - name: Update config 1
    template:
      src: config1.j2
      dest: /etc/app/config1
    notify: Restart app

  - name: Update config 2
    template:
      src: config2.j2
      dest: /etc/app/config2
    notify: Restart app

  # App only restarts once!

handlers:
  - name: Restart app
    service:
      name: app
      state: restarted
```

---

## Module Selection

### Use Native Modules

Native Rust modules are fastest:

| Tier | Modules | Speed |
|------|---------|-------|
| LocalLogic | `debug`, `set_fact`, `assert` | Instant |
| NativeTransport | `copy`, `template`, `file`, `stat` | Fast |
| RemoteCommand | `command`, `shell`, `service` | Moderate |
| PythonFallback | Ansible modules | Slowest |

### Prefer file over command

```yaml
# Slower: Shell commands
- name: Create directory
  command: mkdir -p /opt/app/logs
  args:
    creates: /opt/app/logs

# Faster: Native module
- name: Create directory
  file:
    path: /opt/app/logs
    state: directory
```

### Command vs Shell

`command` is faster than `shell`:

```yaml
# Faster: No shell processing
- name: Run script
  command: /opt/script.sh arg1 arg2

# Slower: Shell parsing overhead
- name: Run script with shell
  shell: /opt/script.sh arg1 arg2
```

Use `shell` only when you need shell features:
- Pipes: `cmd1 | cmd2`
- Redirects: `cmd > file`
- Environment expansion: `$HOME`
- Glob patterns: `*.txt`

---

## Memory Management

### Memory Scaling

Memory usage scales with inventory:

| Inventory Size | Expected Memory |
|----------------|-----------------|
| 10 hosts | ~25 MB |
| 100 hosts | ~70 MB |
| 1,000 hosts | ~400 MB |
| 5,000 hosts | ~1.8 GB |

### Reduce Memory Usage

1. **Limit forks for large inventories**:
   ```bash
   rustible run playbook.yml -f 10
   ```

2. **Split large playbooks**:
   ```bash
   rustible run part1.yml -i inventory.yml
   rustible run part2.yml -i inventory.yml
   ```

3. **Use limits for subset operations**:
   ```bash
   rustible run playbook.yml --limit 'webservers[0:99]'
   ```

### Variable Memory Impact

Large variables consume memory:

```yaml
# Avoid storing large data in variables
- name: Load large file
  slurp:
    src: /var/log/huge.log  # 100MB file
  register: log_contents  # Uses 100MB+ memory!

# Better: Process in chunks on remote
- name: Process log file
  shell: tail -1000 /var/log/huge.log | grep ERROR
  register: errors
```

---

## Network Optimization

### High-Latency Networks

For connections with high latency:

```toml
# rustible.toml
[ssh]
# Longer timeout
timeout = 60

# Enable pipelining (reduces round-trips)
pipelining = true
```

### Low-Bandwidth Networks

For limited bandwidth:

```yaml
# Compress files before transfer
- name: Create compressed archive
  local_action:
    module: archive
    path: large_directory/
    dest: /tmp/files.tar.gz
    format: gz

- name: Transfer compressed
  copy:
    src: /tmp/files.tar.gz
    dest: /tmp/
```

### Reduce Network Calls

Batch operations:

```yaml
# Multiple network calls
- command: hostname
  register: hn
- command: uptime
  register: ut
- command: df -h
  register: df

# Single call
- shell: |
    echo "hostname: $(hostname)"
    echo "uptime: $(uptime)"
    echo "disk: $(df -h | head -2)"
  register: info
```

---

## Benchmarking Your Setup

### Time Your Playbooks

```bash
# Basic timing
time rustible run playbook.yml -i inventory.yml

# Detailed timing per task
rustible run playbook.yml -v 2>&1 | grep 'TASK\|elapsed'
```

### Compare Strategies

```bash
# Linear strategy
time rustible run playbook.yml --extra-vars "strategy=linear"

# Free strategy
time rustible run playbook.yml --extra-vars "strategy=free"
```

### Profile Connection Time

```bash
# Test SSH connection speed
for i in {1..10}; do
  time ssh host1 'echo test' 2>/dev/null
done
```

### Test Parallel Scaling

```bash
# Test different fork counts
for forks in 5 10 20 50; do
  echo "Testing with $forks forks:"
  time rustible run playbook.yml -f $forks
done
```

---

## Configuration Reference

### Optimal Settings by Scenario

#### Small Team (<10 hosts)

```toml
[defaults]
forks = 10
timeout = 30

[ssh]
pipelining = true
control_persist = 300
```

#### Medium Fleet (10-100 hosts)

```toml
[defaults]
forks = 20
timeout = 30

[ssh]
pipelining = true
control_persist = 600
```

#### Large Fleet (100+ hosts)

```toml
[defaults]
forks = 50
timeout = 60

[ssh]
pipelining = true
control_persist = 900
```

#### High-Latency Network

```toml
[defaults]
forks = 10  # Lower to reduce congestion
timeout = 120

[ssh]
pipelining = true
control_persist = 1800
```

---

## Performance Checklist

Before deploying:

- [ ] Facts gathering disabled or limited
- [ ] Using appropriate execution strategy
- [ ] Tags configured for partial runs
- [ ] File transfers batched/compressed
- [ ] Native modules preferred over shell commands
- [ ] Connection settings optimized
- [ ] Fork count tuned for environment

For critical deployments:

- [ ] Benchmark with representative workload
- [ ] Test with production inventory size
- [ ] Monitor memory usage during execution
- [ ] Verify network bandwidth is sufficient
- [ ] Test rollback procedure timing

---

## Expected Performance

With optimizations applied:

| Scenario | Before | After | Improvement |
|----------|--------|-------|-------------|
| 10 hosts, 20 tasks | 45s | 8s | 5.6x |
| 50 hosts, 10 tasks | 2m 30s | 15s | 10x |
| 100 hosts, 5 tasks | 3m | 20s | 9x |
| File copy (1MB, 20 hosts) | 2m | 12s | 10x |

Performance varies based on:
- Network latency and bandwidth
- Target host resources
- Task complexity
- Module selection
