# Large Scale and Failure Injection Validation (1,000-10,000+ Nodes)

Phase 5C of the HPC Initiative - Validation methodology for testing Rustible at large HPC scales with comprehensive failure injection and chaos engineering scenarios.

## Table of Contents

1. [Validation Objectives](#1-validation-objectives)
2. [Large Scale Test Environment](#2-large-scale-test-environment)
3. [Scale Validation Tests](#3-scale-validation-tests)
4. [Failure Injection Framework](#4-failure-injection-framework)
5. [Node Failure Scenarios](#5-node-failure-scenarios)
6. [Storage Failure Scenarios](#6-storage-failure-scenarios)
7. [Network Failure Scenarios](#7-network-failure-scenarios)
8. [Recovery and Rollback Validation](#8-recovery-and-rollback-validation)
9. [Failure Mode Catalog](#9-failure-mode-catalog)
10. [Gap Mapping and Mitigation](#10-gap-mapping-and-mitigation)
11. [Execution Runbook](#11-execution-runbook)

---

## 1. Validation Objectives

### 1.1 Primary Goals

| Goal | Description | Success Criteria |
|------|-------------|------------------|
| **Scale Validation** | Prove operations work at 1K-10K+ nodes | Complete execution without degradation |
| **Failure Resilience** | Validate graceful degradation | Continue operations during failures |
| **Recovery Validation** | Verify checkpoint/resume | <5 min recovery at 10K nodes |
| **Data Integrity** | Ensure no data corruption | 100% integrity after failures |

### 1.2 Scale Band Targets (from Phase 2D)

| Scale Band | Node Range | Provisioning SLO | Uptime SLO | Recovery SLO |
|------------|------------|------------------|------------|--------------|
| Large | 1,000-10,000 | <30 min | 99.95% | <30 min |
| Hyperscale | 10,000+ | <60 min | 99.99% | <60 min |

### 1.3 Failure Categories

| Category | Examples | Priority |
|----------|----------|----------|
| **Node Failures** | Process crash, hardware failure, OOM | Critical |
| **Storage Failures** | Filesystem unavailable, quota exceeded, I/O errors | Critical |
| **Network Failures** | Partition, latency spike, packet loss | Critical |
| **Service Failures** | Slurm down, LDAP timeout, NTP drift | High |
| **Resource Exhaustion** | Memory, CPU, file descriptors | High |

---

## 2. Large Scale Test Environment

### 2.1 Environment Specifications

#### 2.1.1 Bare-Metal Configuration (1,000-10,000 nodes)

```yaml
# Environment: large-hpc-validation
controller:
  count: 1-3  # HA for 10K+
  type: bare-metal
  specs:
    cpu: 16-32 cores
    memory: 64-128 GB
    storage: 500 GB NVMe SSD
    network: 10GbE × 2 (bonded)
  os: Rocky Linux 9.3

compute_nodes:
  count: 1000-10000
  type: bare-metal
  specs:
    cpu: 2+ cores
    memory: 4+ GB
    storage: 50+ GB
  os: Rocky Linux 9.3

network:
  management: 10GbE
  fabric: InfiniBand HDR (optional)
  topology: Fat-tree or Dragonfly
  latency: <100µs intra-rack

scheduler:
  type: Slurm
  version: 23.02+
  controllers: 2 (HA)

storage:
  type: Lustre
  capacity: 100+ TB
  mds: 2+ (HA)
  oss: 4+
```

#### 2.1.2 Cloud Configuration (AWS)

| Component | 1,000 Nodes | 5,000 Nodes | 10,000 Nodes |
|-----------|-------------|-------------|--------------|
| Controller | c5.4xlarge | c5.9xlarge | c5.18xlarge × 2 |
| Compute | c5.xlarge × 1000 | c5.xlarge × 5000 | c5n.xlarge × 10000 |
| Network | Placement group | Cluster placement | Cluster + EFA |
| Storage | FSx Lustre 10TB | FSx Lustre 50TB | FSx Lustre 100TB |
| Est. cost/hr | ~$500 | ~$2,500 | ~$5,000+ |

### 2.2 Controller Scaling

| Node Count | Controller Specs | Fork Count | Memory Budget |
|------------|------------------|------------|---------------|
| 1,000 | 16 core, 64 GB | 100 | 32 GB |
| 5,000 | 32 core, 128 GB | 200 | 64 GB |
| 10,000 | 64 core, 256 GB | 500 | 128 GB |
| 10,000+ | HA cluster | 500+ | 256+ GB |

### 2.3 Monitoring Infrastructure

```yaml
# monitoring/prometheus-stack.yml
monitoring:
  prometheus:
    instances: 2 (HA)
    retention: 30 days
    scrape_interval: 15s

  grafana:
    instances: 1
    dashboards:
      - rustible-execution
      - node-health
      - network-metrics
      - storage-performance

  alertmanager:
    instances: 2 (HA)
    alerts:
      - execution_failure
      - node_unreachable
      - memory_exhaustion
      - storage_full

  node_exporter:
    on_all_nodes: true
    collectors:
      - cpu
      - memory
      - network
      - filesystem
```

---

## 3. Scale Validation Tests

### 3.1 Test: Maximum Scale Provisioning (SCALE-001)

**Objective**: Validate provisioning at maximum scale.

**Scale targets**: 1000, 5000, 10000 nodes

**Steps**:
1. Prepare inventory for target scale
2. Execute full provisioning playbook
3. Monitor memory and CPU on controller
4. Verify all nodes configured correctly
5. Document timing and resource usage

**Expected Results**:

| Scale | Target Time | Max Memory | Max CPU | Success Rate |
|-------|-------------|------------|---------|--------------|
| 1,000 | <15 min | <16 GB | <80% | ≥99.9% |
| 5,000 | <30 min | <64 GB | <90% | ≥99.9% |
| 10,000 | <60 min | <128 GB | <95% | ≥99.9% |

---

### 3.2 Test: High Parallelism Stress (SCALE-002)

**Objective**: Validate maximum concurrent connections.

**Scale targets**: 1000, 5000, 10000 nodes with increasing fork counts

**Fork configurations**:
| Scale | Forks 100 | Forks 200 | Forks 500 | Forks 1000 |
|-------|-----------|-----------|-----------|------------|
| 1,000 | ✓ | ✓ | ✓ | - |
| 5,000 | ✓ | ✓ | ✓ | ✓ |
| 10,000 | ✓ | ✓ | ✓ | ✓ |

**Expected Results**:

| Scale | Forks | Connections | Memory | Throughput |
|-------|-------|-------------|--------|------------|
| 1,000 | 200 | 200 | ~8 GB | 500 hosts/min |
| 5,000 | 500 | 500 | ~32 GB | 1000 hosts/min |
| 10,000 | 1000 | 1000 | ~64 GB | 2000 hosts/min |

---

### 3.3 Test: Sustained Operations (SCALE-003)

**Objective**: Validate sustained operations over extended period.

**Scale targets**: 1000, 5000 nodes

**Duration**: 4 hours continuous

**Workload**:
- Cycle: Config check → Update → Verify → Sleep 5 min
- Repeat for 4 hours
- Monitor for memory leaks, connection leaks

**Expected Results**:

| Scale | Duration | Memory Growth | Connection Leaks | Failures |
|-------|----------|---------------|------------------|----------|
| 1,000 | 4 hr | <10% | 0 | 0 |
| 5,000 | 4 hr | <10% | 0 | 0 |

---

### 3.4 Test: Checkpoint at Scale (SCALE-004)

**Objective**: Validate checkpoint creation and recovery at scale.

**Scale targets**: 1000, 5000, 10000 nodes

**Steps**:
1. Start long-running playbook (100 tasks)
2. Create checkpoint at 50% completion
3. Terminate execution
4. Resume from checkpoint
5. Verify completion and state

**Expected Results**:

| Scale | Checkpoint Time | Checkpoint Size | Resume Time |
|-------|-----------------|-----------------|-------------|
| 1,000 | <30 sec | <50 MB | <30 sec |
| 5,000 | <1 min | <200 MB | <1 min |
| 10,000 | <2 min | <500 MB | <2 min |

---

## 4. Failure Injection Framework

### 4.1 Chaos Engineering Principles

Following [chaos engineering best practices](https://chaos-mesh.org/):

1. **Hypothesis-driven**: Define expected behavior before injection
2. **Controlled blast radius**: Limit impact to subset of nodes
3. **Automated recovery**: Verify automatic healing
4. **Observability**: Full metrics and logging during injection
5. **Rollback capability**: Ability to stop and recover

### 4.2 Failure Injection Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                     Failure Injection Framework                          │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    Chaos Controller                               │   │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐        │   │
│  │  │   Scenario    │  │   Injection   │  │   Recovery    │        │   │
│  │  │   Scheduler   │  │   Engine      │  │   Validator   │        │   │
│  │  └───────────────┘  └───────────────┘  └───────────────┘        │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                              │                                          │
│            ┌─────────────────┼─────────────────┐                       │
│            │                 │                 │                        │
│            ▼                 ▼                 ▼                        │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐              │
│  │ Node Failures │  │Storage Failures│  │Network Failures│             │
│  │   - Kill      │  │   - Unmount    │  │   - Partition  │             │
│  │   - OOM       │  │   - Fill       │  │   - Latency    │             │
│  │   - Reboot    │  │   - Readonly   │  │   - Packet loss│             │
│  └───────────────┘  └───────────────┘  └───────────────┘              │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.3 Injection Methods

| Method | Tool | Blast Radius Control |
|--------|------|---------------------|
| **Node kill** | systemctl, kill -9 | Per-node, percentage |
| **Memory pressure** | stress-ng, cgroups | Per-node |
| **Disk fill** | dd, fallocate | Per-filesystem |
| **Network partition** | iptables, tc | Per-node, rack, subnet |
| **Latency injection** | tc netem | Per-interface |
| **Packet loss** | tc netem | Per-interface, percentage |
| **Service failure** | systemctl stop | Per-service |

### 4.4 Injection Playbook Template

```yaml
# chaos/inject_failure.yml
- name: Inject failure scenario
  hosts: "{{ target_hosts }}"
  become: yes
  vars:
    failure_type: "{{ failure }}"
    duration_seconds: "{{ duration | default(60) }}"
    blast_radius_percent: "{{ blast_radius | default(10) }}"

  tasks:
    - name: Select target subset
      set_fact:
        is_target: "{{ (999999999 | random(seed=inventory_hostname)) % 100 < blast_radius_percent }}"

    - name: Pre-injection checkpoint
      include_tasks: checkpoint_state.yml
      when: is_target

    - name: Inject failure
      include_tasks: "inject_{{ failure_type }}.yml"
      when: is_target

    - name: Wait for duration
      pause:
        seconds: "{{ duration_seconds }}"
      when: is_target

    - name: Recover from failure
      include_tasks: "recover_{{ failure_type }}.yml"
      when: is_target

    - name: Validate recovery
      include_tasks: validate_recovery.yml
      when: is_target
```

---

## 5. Node Failure Scenarios

### 5.1 Test: Process Termination (NODE-001)

**Objective**: Validate handling of slurmd/sssd process crashes.

**Scale**: 1000, 5000 nodes (10% failure rate)

**Steps**:
1. Start configuration update playbook
2. Kill slurmd on 10% of nodes mid-execution
3. Verify playbook continues for reachable nodes
4. Verify failed nodes are logged
5. Re-run to remediate failed nodes

**Injection**:
```yaml
# chaos/inject_process_kill.yml
- name: Kill target process
  shell: "pkill -9 {{ process_name }}"
  ignore_errors: yes
```

**Expected Results**:

| Scale | Failed Nodes | Healthy Completion | Remediation Time |
|-------|--------------|-------------------|------------------|
| 1,000 | 100 | 900 (100%) | <5 min |
| 5,000 | 500 | 4500 (100%) | <15 min |

---

### 5.2 Test: Node Reboot (NODE-002)

**Objective**: Validate handling of unexpected node reboots.

**Scale**: 1000, 5000 nodes (5% failure rate)

**Steps**:
1. Start long-running playbook
2. Force reboot 5% of nodes mid-execution
3. Verify playbook handles unreachable nodes
4. Verify nodes rejoin after reboot
5. Validate final state consistency

**Injection**:
```yaml
# chaos/inject_reboot.yml
- name: Force immediate reboot
  command: "reboot -f"
  async: 1
  poll: 0
```

**Expected Results**:

| Scale | Rebooted | Recovery Time | State Consistency |
|-------|----------|---------------|-------------------|
| 1,000 | 50 | <10 min | 100% |
| 5,000 | 250 | <15 min | 100% |

---

### 5.3 Test: Memory Exhaustion (NODE-003)

**Objective**: Validate handling of OOM conditions on controller.

**Scale**: Controller during 5000 node operation

**Steps**:
1. Start high-parallelism operation (forks=500)
2. Inject memory pressure on controller
3. Monitor for graceful degradation
4. Verify checkpoint triggered
5. Verify recovery after memory released

**Injection**:
```yaml
# chaos/inject_memory_pressure.yml
- name: Create memory pressure
  command: "stress-ng --vm 4 --vm-bytes 80% --timeout {{ duration }}s"
  async: "{{ duration }}"
  poll: 0
```

**Expected Results**:

| Memory Pressure | Execution | Checkpoint | Recovery |
|-----------------|-----------|------------|----------|
| 70% | Continues | Optional | N/A |
| 80% | Degraded | Triggered | <2 min |
| 90% | Paused | Triggered | <5 min |

---

### 5.4 Test: Rack Failure Simulation (NODE-004)

**Objective**: Validate handling of multi-node failures (rack down).

**Scale**: 5000, 10000 nodes (1 rack = 48-96 nodes)

**Steps**:
1. Identify rack group in inventory
2. Start cluster-wide operation
3. Simulate rack failure (all nodes unreachable)
4. Verify operations continue for other racks
5. Verify isolation and logging

**Expected Results**:

| Scale | Rack Size | Affected | Continued | Logged |
|-------|-----------|----------|-----------|--------|
| 5,000 | 48 | 48 (1%) | 4952 | 48 |
| 10,000 | 96 | 96 (1%) | 9904 | 96 |

---

## 6. Storage Failure Scenarios

### 6.1 Test: Lustre Mount Failure (STOR-001)

**Objective**: Validate handling of shared filesystem unavailability.

**Scale**: 1000, 5000 nodes (10% affected)

**Steps**:
1. Start operation requiring /scratch access
2. Unmount Lustre on 10% of nodes
3. Verify operation fails gracefully on affected nodes
4. Verify other nodes continue
5. Remount and verify recovery

**Injection**:
```yaml
# chaos/inject_mount_failure.yml
- name: Force unmount Lustre
  command: "umount -l /scratch"
  ignore_errors: yes
```

**Expected Results**:

| Scale | Unmounted | Graceful Failure | Recovery |
|-------|-----------|------------------|----------|
| 1,000 | 100 | 100 | <2 min |
| 5,000 | 500 | 500 | <5 min |

---

### 6.2 Test: Quota Exceeded (STOR-002)

**Objective**: Validate handling of disk quota errors.

**Scale**: 1000 nodes (10% at quota)

**Steps**:
1. Set tight quota on test users
2. Start operation writing to /home
3. Verify quota error handled gracefully
4. Verify operation continues for other users
5. Clear quota and verify retry

**Expected Results**:

| Affected | Error Handling | Retry Success |
|----------|----------------|---------------|
| 100 | Graceful | 100% |

---

### 6.3 Test: NFS Server Failure (STOR-003)

**Objective**: Validate handling of central NFS failure.

**Scale**: 1000 nodes (all affected)

**Steps**:
1. Start operation requiring /home access
2. Stop NFS server
3. Verify operations pause gracefully
4. Verify checkpoint created
5. Restart NFS and verify resume

**Expected Results**:

| Detection Time | Checkpoint | Resume Time | Data Loss |
|----------------|------------|-------------|-----------|
| <30 sec | Automatic | <2 min | None |

---

### 6.4 Test: Read-Only Filesystem (STOR-004)

**Objective**: Validate handling of filesystem errors (read-only remount).

**Scale**: 1000 nodes (5% affected)

**Steps**:
1. Start configuration update
2. Remount /etc read-only on 5% of nodes
3. Verify write failure handled
4. Verify error logged clearly
5. Remount read-write and retry

**Injection**:
```yaml
# chaos/inject_readonly.yml
- name: Remount read-only
  command: "mount -o remount,ro /"
```

**Expected Results**:

| Affected | Error Type | Logged | Recovery |
|----------|------------|--------|----------|
| 50 | Write failed | Yes | 100% |

---

## 7. Network Failure Scenarios

### 7.1 Test: Network Partition (NET-001)

**Objective**: Validate handling of network partitions.

**Scale**: 5000, 10000 nodes (partition 10% into isolated segment)

**Steps**:
1. Identify partition group
2. Start cluster-wide operation
3. Inject iptables rules to create partition
4. Verify controller handles unreachable nodes
5. Remove partition and verify recovery

**Injection**:
```yaml
# chaos/inject_partition.yml
- name: Create network partition
  iptables:
    chain: INPUT
    source: "{{ controller_ip }}"
    jump: DROP
    comment: "Chaos test partition"

- name: Create network partition (egress)
  iptables:
    chain: OUTPUT
    destination: "{{ controller_ip }}"
    jump: DROP
    comment: "Chaos test partition"
```

**Expected Results**:

| Scale | Partitioned | Detection Time | Healthy Completion |
|-------|-------------|----------------|-------------------|
| 5,000 | 500 | <30 sec | 4500 (100%) |
| 10,000 | 1000 | <30 sec | 9000 (100%) |

---

### 7.2 Test: Latency Injection (NET-002)

**Objective**: Validate behavior under high network latency.

**Scale**: 5000 nodes (50ms, 100ms, 500ms latency)

**Steps**:
1. Start timing-sensitive operation
2. Inject latency using tc netem
3. Measure impact on execution time
4. Verify timeout handling
5. Remove latency and verify recovery

**Injection**:
```yaml
# chaos/inject_latency.yml
- name: Add network latency
  command: "tc qdisc add dev eth0 root netem delay {{ latency_ms }}ms"
```

**Expected Results**:

| Latency | Execution Slowdown | Timeout Triggers | Retries |
|---------|-------------------|------------------|---------|
| 50ms | ~1.5x | 0 | 0 |
| 100ms | ~2x | <1% | <1% |
| 500ms | ~3x | <5% | <5% |

---

### 7.3 Test: Packet Loss (NET-003)

**Objective**: Validate behavior under packet loss conditions.

**Scale**: 5000 nodes (1%, 5%, 10% packet loss)

**Steps**:
1. Start standard operation
2. Inject packet loss
3. Measure retry rates and failures
4. Verify data integrity
5. Remove packet loss

**Injection**:
```yaml
# chaos/inject_packet_loss.yml
- name: Add packet loss
  command: "tc qdisc add dev eth0 root netem loss {{ loss_percent }}%"
```

**Expected Results**:

| Packet Loss | Retry Rate | Task Failure | Data Integrity |
|-------------|------------|--------------|----------------|
| 1% | <5% | <0.1% | 100% |
| 5% | <20% | <1% | 100% |
| 10% | <50% | <5% | 100% |

---

### 7.4 Test: DNS Failure (NET-004)

**Objective**: Validate handling of DNS resolution failures.

**Scale**: 1000 nodes (DNS unavailable for 10%)

**Steps**:
1. Break /etc/resolv.conf on 10% of nodes
2. Start operation using hostnames
3. Verify fallback to IP (if cached)
4. Verify error handling for failed lookups
5. Restore DNS and verify

**Expected Results**:

| Affected | Cache Hit | Resolution Failure | Recovery |
|----------|-----------|-------------------|----------|
| 100 | 80% | 20% | 100% |

---

## 8. Recovery and Rollback Validation

### 8.1 Test: Checkpoint Recovery (RECOV-001)

**Objective**: Validate checkpoint-based recovery at scale.

**Scale**: 5000, 10000 nodes

**Steps**:
1. Enable checkpointing (interval: 2 min)
2. Start 30-minute playbook
3. Terminate controller at 15 minutes
4. Restart and resume from checkpoint
5. Verify completion and consistency

**Expected Results**:

| Scale | Checkpoint Interval | Resume Time | Work Lost |
|-------|---------------------|-------------|-----------|
| 5,000 | 2 min | <2 min | <2 min work |
| 10,000 | 2 min | <5 min | <2 min work |

---

### 8.2 Test: Automatic Rollback (RECOV-002)

**Objective**: Validate automatic rollback on widespread failure.

**Scale**: 5000 nodes

**Configuration**: Rollback threshold = 10% failure

**Steps**:
1. Enable automatic rollback
2. Start configuration update
3. Inject failure causing >10% task failures
4. Verify automatic rollback triggered
5. Verify original state restored

**Expected Results**:

| Failure Rate | Rollback Triggered | Rollback Time | State Restored |
|--------------|-------------------|---------------|----------------|
| 10% | Yes | <5 min | 100% |
| 5% | No | N/A | N/A |

---

### 8.3 Test: Manual Rollback at Scale (RECOV-003)

**Objective**: Validate manual rollback at large scale.

**Scale**: 10000 nodes

**Steps**:
1. Create checkpoint (full cluster state)
2. Apply configuration change
3. Verify unintended consequence
4. Trigger manual rollback
5. Verify complete restoration

**Expected Results**:

| Scale | Checkpoint Size | Rollback Time | Verification Time |
|-------|-----------------|---------------|-------------------|
| 10,000 | <1 GB | <10 min | <5 min |

---

### 8.4 Test: Partial Cluster Recovery (RECOV-004)

**Objective**: Validate recovery of failed subset while preserving successful changes.

**Scale**: 5000 nodes (500 failed, 4500 successful)

**Steps**:
1. Apply change to all nodes
2. 500 nodes fail mid-execution
3. Verify successful nodes retain changes
4. Retry only failed nodes
5. Verify complete cluster consistency

**Expected Results**:

| Successful | Failed | Retry Time | Final Consistency |
|------------|--------|------------|-------------------|
| 4500 | 500 | <5 min | 100% |

---

## 9. Failure Mode Catalog

### 9.1 Node Failure Modes

| ID | Failure Mode | Symptoms | Detection | Mitigation |
|----|--------------|----------|-----------|------------|
| NF-01 | Process crash | Task timeout | SSH check fails | Retry with process restart |
| NF-02 | Node reboot | Connection reset | SSH unreachable | Wait and retry |
| NF-03 | OOM kill | Process terminated | Exit code 137 | Reduce parallelism |
| NF-04 | Hardware failure | All services unavailable | Multiple SSH failures | Mark node failed, skip |
| NF-05 | Hung process | Task timeout | SSH succeeds but task hangs | Kill and retry |

### 9.2 Storage Failure Modes

| ID | Failure Mode | Symptoms | Detection | Mitigation |
|----|--------------|----------|-----------|------------|
| SF-01 | Mount unavailable | I/O error | Check mount point | Remount or fail task |
| SF-02 | Quota exceeded | EDQUOT error | Check quota | Report and skip writes |
| SF-03 | Filesystem full | ENOSPC error | Check df | Alert and pause |
| SF-04 | Read-only FS | EROFS error | Check mount flags | Remount or fail |
| SF-05 | NFS timeout | Stale handle | Check NFS status | Reconnect or fail |

### 9.3 Network Failure Modes

| ID | Failure Mode | Symptoms | Detection | Mitigation |
|----|--------------|----------|-----------|------------|
| NTF-01 | Host unreachable | Connection timeout | Ping fails | Mark unreachable, skip |
| NTF-02 | Port blocked | Connection refused | Port check fails | Report, alternate port |
| NTF-03 | DNS failure | Name resolution fails | DNS lookup fails | Use IP fallback |
| NTF-04 | High latency | Slow responses | Timing metrics | Increase timeout |
| NTF-05 | Packet loss | Intermittent failures | Retry count | Increase retries |

### 9.4 Service Failure Modes

| ID | Failure Mode | Symptoms | Detection | Mitigation |
|----|--------------|----------|-----------|------------|
| SVF-01 | Slurm down | scontrol fails | Service check | Restart or skip |
| SVF-02 | LDAP timeout | User lookup fails | LDAP query timeout | Use cache, retry |
| SVF-03 | NTP drift | Time sync errors | Offset check | Restart NTP |
| SVF-04 | Kerberos expired | Auth failures | Kinit fails | Renew tickets |
| SVF-05 | SSSD failure | ID lookup fails | getent fails | Restart SSSD |

---

## 10. Gap Mapping and Mitigation

### 10.1 Failure Modes Mapped to Phase 4 Gaps

| Failure Mode | Related Gap (Phase 4A) | Severity | Mitigation Status |
|--------------|------------------------|----------|-------------------|
| NF-03 OOM | GAP-EXE-001 Memory efficiency | High | Needs implementation |
| NF-05 Hung process | GAP-EXE-005 Task timeout handling | Medium | Partial |
| SF-01 Mount unavailable | GAP-STO-001 Lustre module | Critical | Planned |
| SF-05 NFS timeout | GAP-STO-003 NFS failure handling | High | Partial |
| NTF-01 Host unreachable | GAP-EXE-003 Unreachable handling | Medium | Implemented |
| NTF-04 High latency | GAP-EXE-004 Adaptive timeout | Medium | Planned |
| SVF-01 Slurm down | GAP-SCH-001 Slurm module | Critical | In progress |
| SVF-02 LDAP timeout | GAP-IDT-002 LDAP handling | High | Planned |

### 10.2 Recommended Mitigations

#### Critical Priority

| Gap | Mitigation | Implementation |
|-----|------------|----------------|
| Memory efficiency | Connection pooling, streaming | Core executor changes |
| Lustre module | Native Lustre mount/quota module | New module |
| Slurm module | Native scontrol/sacctmgr module | New module |

#### High Priority

| Gap | Mitigation | Implementation |
|-----|------------|----------------|
| NFS timeout handling | Configurable timeouts, retry logic | Module enhancement |
| LDAP timeout | Connection pool, local cache | Provider enhancement |
| Checkpoint at scale | Incremental checkpoints | Executor enhancement |

#### Medium Priority

| Gap | Mitigation | Implementation |
|-----|------------|----------------|
| Task timeout handling | Per-task timeout configuration | Task runner enhancement |
| Adaptive timeout | Dynamic timeout based on latency | Network layer |
| Hung process detection | Heartbeat monitoring | Task runner |

### 10.3 Gap Closure Validation Tests

| Gap | Validation Test | Pass Criteria |
|-----|-----------------|---------------|
| GAP-EXE-001 | NODE-003 (Memory exhaustion) | <8GB at 10K nodes |
| GAP-EXE-003 | NODE-001, NODE-004 | 100% graceful handling |
| GAP-STO-001 | STOR-001 | Mount/unmount without module |
| GAP-SCH-001 | SCALE-001 with Slurm | Native scontrol operations |
| GAP-IDT-002 | NET-004 (DNS) extended | Cache and retry |

---

## 11. Execution Runbook

### 11.1 Pre-Execution Checklist

```markdown
## Large Scale Validation Checklist

### Environment
- [ ] Controller(s) provisioned (spec meets scale target)
- [ ] Compute nodes ready (1K/5K/10K)
- [ ] Network connectivity verified
- [ ] Monitoring infrastructure deployed
- [ ] Alert rules configured

### Tools
- [ ] Rustible installed with latest patches
- [ ] Chaos injection scripts tested on single node
- [ ] Checkpoint storage available (>10GB free)
- [ ] Log aggregation configured

### Safety
- [ ] Blast radius limits configured
- [ ] Kill switch procedure documented
- [ ] Rollback playbooks tested
- [ ] Emergency contacts identified
```

### 11.2 Execution Schedule

```markdown
## Week 1: Scale Validation (1K-5K nodes)

### Day 1-2: 1000 Node Tests
- SCALE-001 (provisioning)
- SCALE-002 (parallelism)
- SCALE-003 (4hr sustained)
- SCALE-004 (checkpoint)

### Day 3-4: 5000 Node Tests
- SCALE-001 (provisioning)
- SCALE-002 (parallelism)
- SCALE-004 (checkpoint)

### Day 5: Analysis and Issue Documentation

## Week 2: Failure Injection (1K-5K nodes)

### Day 1: Node Failures
- NODE-001 (process kill)
- NODE-002 (reboot)
- NODE-003 (OOM)

### Day 2: Storage Failures
- STOR-001 (mount failure)
- STOR-002 (quota)
- STOR-003 (NFS down)

### Day 3: Network Failures
- NET-001 (partition)
- NET-002 (latency)
- NET-003 (packet loss)

### Day 4: Recovery Tests
- RECOV-001 (checkpoint)
- RECOV-002 (auto rollback)
- RECOV-003 (manual rollback)

### Day 5: Analysis and Gap Mapping

## Week 3: Large Scale (10K nodes) - If Available

### Day 1-2: Core Operations
- SCALE-001 at 10K
- SCALE-004 at 10K

### Day 3-4: Failure Injection
- NODE-004 (rack failure)
- NET-001 at 10K (partition)

### Day 5: Final Analysis and Reporting
```

### 11.3 Emergency Procedures

```markdown
## Emergency Stop Procedure

1. **Immediate Stop**
   ```bash
   # Kill all Rustible processes
   pkill -9 rustible

   # If chaos injection active, remove rules
   ansible all -m iptables -a "chain=INPUT flush=yes"
   ansible all -m command -a "tc qdisc del dev eth0 root"
   ```

2. **Restore Network**
   ```bash
   ansible all -m command -a "systemctl restart network"
   ```

3. **Verify Connectivity**
   ```bash
   ansible all -m ping
   ```

4. **Document Incident**
   - Time of incident
   - Test in progress
   - Failure mode
   - Recovery actions
```

### 11.4 Results Collection

```bash
# Collect all results
./scripts/collect_results.sh large-scale

# Generate failure mode report
./scripts/generate_failure_catalog.sh results/large-scale/

# Map to gaps
./scripts/gap_mapping.sh results/large-scale/ docs/hpc/gap-matrix-rustible-hpc.md

# Create final report
./scripts/generate_report.sh results/large-scale/ \
    --include-failures \
    --include-gap-mapping \
    --output docs/hpc/large-scale-validation-report.md
```

---

## Appendix: Quick Reference

### A.1 Test ID Reference

| ID | Category | Name | Scale |
|----|----------|------|-------|
| SCALE-001 | Scale | Maximum Scale Provisioning | 1K-10K |
| SCALE-002 | Scale | High Parallelism Stress | 1K-10K |
| SCALE-003 | Scale | Sustained Operations | 1K-5K |
| SCALE-004 | Scale | Checkpoint at Scale | 1K-10K |
| NODE-001 | Node Failure | Process Termination | 1K-5K |
| NODE-002 | Node Failure | Node Reboot | 1K-5K |
| NODE-003 | Node Failure | Memory Exhaustion | Controller |
| NODE-004 | Node Failure | Rack Failure Simulation | 5K-10K |
| STOR-001 | Storage | Lustre Mount Failure | 1K-5K |
| STOR-002 | Storage | Quota Exceeded | 1K |
| STOR-003 | Storage | NFS Server Failure | 1K |
| STOR-004 | Storage | Read-Only Filesystem | 1K |
| NET-001 | Network | Network Partition | 5K-10K |
| NET-002 | Network | Latency Injection | 5K |
| NET-003 | Network | Packet Loss | 5K |
| NET-004 | Network | DNS Failure | 1K |
| RECOV-001 | Recovery | Checkpoint Recovery | 5K-10K |
| RECOV-002 | Recovery | Automatic Rollback | 5K |
| RECOV-003 | Recovery | Manual Rollback at Scale | 10K |
| RECOV-004 | Recovery | Partial Cluster Recovery | 5K |

### A.2 SLO Reference (from Phase 2D)

| Scale | Provisioning | Uptime | Recovery |
|-------|--------------|--------|----------|
| 1K-10K | <30 min | 99.95% | <30 min |
| 10K+ | <60 min | 99.99% | <60 min |
