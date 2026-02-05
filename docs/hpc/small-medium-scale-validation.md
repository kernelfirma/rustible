# Small and Medium Scale Validation (10-1,000 Nodes)

Phase 5B of the HPC Initiative - Validation methodology and results framework for testing Rustible at small (10-100 nodes) and medium (100-1,000 nodes) HPC scales.

## Table of Contents

1. [Validation Objectives](#1-validation-objectives)
2. [Test Environment Specifications](#2-test-environment-specifications)
3. [Provisioning Tests](#3-provisioning-tests)
4. [Reconfiguration Tests](#4-reconfiguration-tests)
5. [Rollback Tests](#5-rollback-tests)
6. [Drift Detection Tests](#6-drift-detection-tests)
7. [Partial Failure Tests](#7-partial-failure-tests)
8. [Results Framework](#8-results-framework)
9. [Issue Tracking Template](#9-issue-tracking-template)
10. [Execution Runbook](#10-execution-runbook)

---

## 1. Validation Objectives

### 1.1 Primary Goals

| Goal | Description | Success Criteria |
|------|-------------|------------------|
| **Functional Validation** | Verify core operations work correctly | 100% pass rate on all test cases |
| **Performance Baseline** | Establish timing benchmarks | Meet Phase 2D SLO targets |
| **Reliability Assessment** | Measure failure rates | <0.1% task failure rate |
| **Comparison Baseline** | Compare with Ansible | Document speedup ratios |

### 1.2 Scale Band Targets

| Scale Band | Node Range | Target SLOs (from Phase 2D) |
|------------|------------|----------------------------|
| **Small** | 10-100 | Provisioning <5 min, uptime 99.5% |
| **Medium** | 100-1,000 | Provisioning <15 min, uptime 99.9% |

### 1.3 Test Coverage Matrix

| Test Category | Small (10-100) | Medium (100-1K) |
|---------------|----------------|-----------------|
| Provisioning | Full | Full |
| Reconfiguration | Full | Full |
| Rollback | Full | Full |
| Drift Detection | Full | Sampling |
| Partial Failure | Full | Full |

---

## 2. Test Environment Specifications

### 2.1 Small Scale Environment (10-100 nodes)

```yaml
# Environment: small-hpc-validation
controller:
  type: vm
  specs:
    cpu: 4 cores
    memory: 16 GB
    storage: 100 GB SSD
  os: Rocky Linux 9.3

compute_nodes:
  count: 100
  type: vm or bare-metal
  specs:
    cpu: 2 cores minimum
    memory: 4 GB minimum
    storage: 50 GB
  os: Rocky Linux 9.3

network:
  type: 1GbE or 10GbE
  latency: <1ms within cluster
  bandwidth: 1 Gbps minimum

scheduler:
  type: Slurm
  version: 23.02+

storage:
  type: NFS or Lustre
  capacity: 1 TB shared
```

### 2.2 Medium Scale Environment (100-1,000 nodes)

```yaml
# Environment: medium-hpc-validation
controller:
  type: bare-metal or high-spec VM
  specs:
    cpu: 8 cores
    memory: 32 GB
    storage: 200 GB SSD
  os: Rocky Linux 9.3

compute_nodes:
  count: 1000
  type: bare-metal preferred
  specs:
    cpu: 2 cores minimum
    memory: 4 GB minimum
    storage: 50 GB
  os: Rocky Linux 9.3

network:
  type: 10GbE + InfiniBand (optional)
  latency: <1ms within cluster
  bandwidth: 10 Gbps minimum

scheduler:
  type: Slurm
  version: 23.02+

storage:
  type: Lustre
  capacity: 10 TB shared
```

### 2.3 Cloud Environment Alternative (AWS)

| Component | Small (100 nodes) | Medium (1000 nodes) |
|-----------|-------------------|---------------------|
| Controller | c5.xlarge | c5.2xlarge |
| Compute | t3.medium × 100 | t3.medium × 1000 |
| Network | VPC default | Placement group |
| Storage | EFS | FSx for Lustre |
| Est. cost/hr | ~$10 | ~$100 |

---

## 3. Provisioning Tests

### 3.1 Test: Initial Node Provisioning (PROV-001)

**Objective**: Validate initial configuration of compute nodes from bare state.

**Scale targets**: 10, 50, 100, 500, 1000 nodes

**Steps**:
1. Start with clean OS installation on target nodes
2. Execute base provisioning playbook
3. Measure time to completion
4. Verify all services running
5. Validate cluster membership

**Playbook outline**:
```yaml
# provisioning/base_provision.yml
- name: Base node provisioning
  hosts: compute_nodes
  become: yes
  tasks:
    - name: Configure hostname
      hostname:
        name: "{{ inventory_hostname }}"

    - name: Configure /etc/hosts
      template:
        src: hosts.j2
        dest: /etc/hosts

    - name: Install base packages
      package:
        name: "{{ base_packages }}"
        state: present

    - name: Configure NTP
      include_role:
        name: ntp

    - name: Configure SSSD
      include_role:
        name: sssd

    - name: Mount shared storage
      mount:
        path: /home
        src: "{{ nfs_server }}:/home"
        fstype: nfs
        opts: defaults,hard,intr
        state: mounted

    - name: Install Slurm client
      include_role:
        name: slurm_client

    - name: Start services
      service:
        name: "{{ item }}"
        state: started
        enabled: yes
      loop:
        - sssd
        - slurmd
```

**Expected Results**:

| Scale | Target Time | Max Memory | Success Rate |
|-------|-------------|------------|--------------|
| 10 | <1 min | <500 MB | 100% |
| 50 | <2 min | <1 GB | 100% |
| 100 | <5 min | <2 GB | ≥99.9% |
| 500 | <10 min | <4 GB | ≥99.9% |
| 1000 | <15 min | <8 GB | ≥99.9% |

**Metrics to collect**:
- Total execution time
- Time per host
- Memory usage on controller
- Task success/failure counts
- Network connections count

---

### 3.2 Test: Slurm Node Registration (PROV-002)

**Objective**: Validate nodes correctly register with Slurm controller.

**Scale targets**: 10, 100, 1000 nodes

**Steps**:
1. Execute Slurm registration playbook
2. Verify nodes appear in `sinfo`
3. Verify node states are "idle"
4. Test job submission to new nodes

**Playbook outline**:
```yaml
# provisioning/slurm_register.yml
- name: Register nodes with Slurm
  hosts: slurm_controller
  become: yes
  tasks:
    - name: Update slurm.conf with new nodes
      slurm_node:
        name: "{{ item }}"
        state: present
        cpus: "{{ hostvars[item].slurm_cpus }}"
        memory: "{{ hostvars[item].slurm_memory }}"
        partition: compute
      loop: "{{ groups['compute_nodes'] }}"
      notify: reconfigure slurm

    - name: Reconfigure Slurm
      command: scontrol reconfigure
      when: slurm_conf_changed

- name: Verify Slurm registration
  hosts: localhost
  tasks:
    - name: Check node status
      command: sinfo -N -h -o "%N %T"
      register: sinfo_output
      changed_when: false

    - name: Verify all nodes idle
      assert:
        that:
          - "'down' not in sinfo_output.stdout"
          - "'drain' not in sinfo_output.stdout"
        fail_msg: "Some nodes not in expected state"
```

**Expected Results**:

| Scale | Target Time | Verification Time |
|-------|-------------|-------------------|
| 10 | <30 sec | <10 sec |
| 100 | <2 min | <30 sec |
| 1000 | <5 min | <2 min |

---

### 3.3 Test: GPU Node Provisioning (PROV-003)

**Objective**: Validate NVIDIA driver and CUDA toolkit installation.

**Scale targets**: 10, 50, 100 GPU nodes

**Steps**:
1. Install NVIDIA drivers
2. Install CUDA toolkit
3. Configure GPU persistence mode
4. Verify nvidia-smi output
5. Run GPU validation test

**Expected Results**:

| Scale | Target Time | GPU Detect Rate |
|-------|-------------|-----------------|
| 10 | <5 min | 100% |
| 50 | <10 min | ≥99% |
| 100 | <15 min | ≥99% |

---

## 4. Reconfiguration Tests

### 4.1 Test: Configuration Update (RECONF-001)

**Objective**: Validate configuration changes propagate correctly.

**Scale targets**: 10, 100, 500, 1000 nodes

**Scenarios**:
1. Update NTP server configuration
2. Modify Slurm node properties
3. Change resource limits
4. Update environment modules

**Playbook outline**:
```yaml
# reconfiguration/update_config.yml
- name: Update cluster configuration
  hosts: compute_nodes
  become: yes
  tasks:
    - name: Update NTP configuration
      template:
        src: chrony.conf.j2
        dest: /etc/chrony.conf
      notify: restart chronyd

    - name: Update resource limits
      pam_limits:
        domain: '*'
        limit_type: '-'
        limit_item: "{{ item.item }}"
        value: "{{ item.value }}"
      loop:
        - { item: nofile, value: 65536 }
        - { item: nproc, value: 65536 }
        - { item: memlock, value: unlimited }

    - name: Update module defaults
      copy:
        content: |
          module load gcc/13.2.0
          module load openmpi/5.0.0
        dest: /etc/profile.d/modules.sh
```

**Expected Results**:

| Scale | Target Time | Change Detection |
|-------|-------------|------------------|
| 10 | <30 sec | 100% |
| 100 | <2 min | 100% |
| 500 | <5 min | 100% |
| 1000 | <10 min | 100% |

---

### 4.2 Test: Service Restart Coordination (RECONF-002)

**Objective**: Validate coordinated service restarts across cluster.

**Scale targets**: 100, 500, 1000 nodes

**Scenarios**:
1. Rolling restart of slurmd (batch of 10%)
2. Synchronized restart (all at once)
3. Canary restart (1 node first, then rest)

**Playbook outline**:
```yaml
# reconfiguration/rolling_restart.yml
- name: Rolling restart slurmd
  hosts: compute_nodes
  serial: "10%"
  become: yes
  tasks:
    - name: Drain node
      delegate_to: "{{ slurm_controller }}"
      command: scontrol update NodeName={{ inventory_hostname }} State=DRAIN Reason="Maintenance"

    - name: Wait for jobs to complete
      delegate_to: "{{ slurm_controller }}"
      command: squeue -h -w {{ inventory_hostname }}
      register: jobs
      until: jobs.stdout == ""
      retries: 60
      delay: 10

    - name: Restart slurmd
      service:
        name: slurmd
        state: restarted

    - name: Resume node
      delegate_to: "{{ slurm_controller }}"
      command: scontrol update NodeName={{ inventory_hostname }} State=RESUME
```

**Expected Results**:

| Scale | Rolling (10%) | Synchronized |
|-------|---------------|--------------|
| 100 | <10 min | <2 min |
| 500 | <30 min | <5 min |
| 1000 | <60 min | <10 min |

---

### 4.3 Test: Package Updates (RECONF-003)

**Objective**: Validate package updates across cluster.

**Scale targets**: 100, 500, 1000 nodes

**Scenarios**:
1. Security patch updates
2. Minor version upgrades
3. Major version upgrades (kernel)

**Expected Results**:

| Scale | Security Patch | Minor Upgrade |
|-------|----------------|---------------|
| 100 | <5 min | <10 min |
| 500 | <15 min | <30 min |
| 1000 | <30 min | <60 min |

---

## 5. Rollback Tests

### 5.1 Test: Configuration Rollback (ROLL-001)

**Objective**: Validate ability to revert configuration changes.

**Scale targets**: 10, 100, 500, 1000 nodes

**Steps**:
1. Apply configuration change
2. Detect failure/issue
3. Trigger rollback
4. Verify original state restored
5. Measure rollback time

**Playbook outline**:
```yaml
# rollback/config_rollback.yml
- name: Test configuration rollback
  hosts: compute_nodes
  become: yes
  vars:
    checkpoint_dir: /var/lib/rustible/checkpoints
  tasks:
    - name: Create checkpoint
      rustible_checkpoint:
        files:
          - /etc/slurm/slurm.conf
          - /etc/chrony.conf
          - /etc/sssd/sssd.conf
        dest: "{{ checkpoint_dir }}/{{ ansible_date_time.iso8601 }}"

    - name: Apply new configuration
      template:
        src: slurm.conf.j2
        dest: /etc/slurm/slurm.conf
      register: config_change

    - name: Verify configuration
      command: slurmd -C
      register: verify
      failed_when: verify.rc != 0
      ignore_errors: yes

    - name: Rollback on failure
      rustible_restore:
        src: "{{ checkpoint_dir }}/{{ checkpoint_id }}"
      when: verify.failed
```

**Expected Results**:

| Scale | Checkpoint Time | Rollback Time | Success Rate |
|-------|-----------------|---------------|--------------|
| 10 | <10 sec | <10 sec | 100% |
| 100 | <30 sec | <30 sec | 100% |
| 500 | <1 min | <1 min | 100% |
| 1000 | <2 min | <2 min | 100% |

---

### 5.2 Test: Partial Rollback (ROLL-002)

**Objective**: Validate rollback of subset of nodes.

**Scale targets**: 100, 500, 1000 nodes

**Scenario**: 10% of nodes fail configuration, rollback only those nodes.

**Expected Results**:

| Scale | Failed Nodes | Rollback Time |
|-------|--------------|---------------|
| 100 | 10 | <15 sec |
| 500 | 50 | <30 sec |
| 1000 | 100 | <1 min |

---

### 5.3 Test: Transaction Rollback (ROLL-003)

**Objective**: Validate atomic transaction rollback on mid-execution failure.

**Scale targets**: 100, 500 nodes

**Steps**:
1. Begin multi-step transaction
2. Simulate failure at step 3 of 5
3. Verify automatic rollback of steps 1-2
4. Confirm no partial state

**Expected Results**:

| Scale | Detection Time | Rollback Time | State Consistency |
|-------|----------------|---------------|-------------------|
| 100 | <5 sec | <30 sec | 100% |
| 500 | <10 sec | <1 min | 100% |

---

## 6. Drift Detection Tests

### 6.1 Test: Configuration Drift Detection (DRIFT-001)

**Objective**: Detect manual configuration changes on nodes.

**Scale targets**: 100, 500, 1000 nodes

**Drift scenarios**:
1. Manual file edit (/etc/hosts)
2. Service stopped manually
3. Package removed
4. Permission change
5. User modification

**Playbook outline**:
```yaml
# drift/detect_drift.yml
- name: Detect configuration drift
  hosts: compute_nodes
  become: yes
  tasks:
    - name: Check file checksums
      stat:
        path: "{{ item }}"
        checksum_algorithm: sha256
      loop: "{{ managed_files }}"
      register: file_stats

    - name: Compare with expected checksums
      assert:
        that:
          - item.stat.checksum == expected_checksums[item.item]
        quiet: yes
      loop: "{{ file_stats.results }}"
      register: drift_check
      ignore_errors: yes

    - name: Report drift
      set_fact:
        drifted_files: "{{ drift_check.results | selectattr('failed') | map(attribute='item.item') | list }}"

    - name: Check service states
      service_facts:

    - name: Verify expected services
      assert:
        that:
          - ansible_facts.services[item].state == 'running'
        quiet: yes
      loop: "{{ required_services }}"
      register: service_drift
      ignore_errors: yes
```

**Expected Results**:

| Scale | Detection Time | Accuracy | False Positive Rate |
|-------|----------------|----------|---------------------|
| 100 | <1 min | 100% | 0% |
| 500 | <3 min | 100% | 0% |
| 1000 | <5 min | 100% | 0% |

---

### 6.2 Test: Drift Remediation (DRIFT-002)

**Objective**: Automatically correct detected drift.

**Scale targets**: 100, 500 nodes

**Steps**:
1. Inject drift (modify 10% of nodes)
2. Run drift detection
3. Apply remediation
4. Verify correction

**Expected Results**:

| Scale | Drifted Nodes | Remediation Time |
|-------|---------------|------------------|
| 100 | 10 | <30 sec |
| 500 | 50 | <2 min |

---

### 6.3 Test: Continuous Drift Monitoring (DRIFT-003)

**Objective**: Validate periodic drift checking at scale.

**Scale targets**: 100, 500, 1000 nodes

**Configuration**:
- Check interval: 5 minutes
- Sampling rate at 1000 nodes: 10% per cycle

**Expected Results**:

| Scale | Check Duration | Resource Usage |
|-------|----------------|----------------|
| 100 | <30 sec | <200 MB |
| 500 | <1 min | <500 MB |
| 1000 | <2 min (sample) | <1 GB |

---

## 7. Partial Failure Tests

### 7.1 Test: Node Unreachable Handling (FAIL-001)

**Objective**: Validate graceful handling of unreachable nodes.

**Scale targets**: 100, 500, 1000 nodes

**Failure scenarios**:
1. 1% nodes unreachable (network issue)
2. 5% nodes unreachable
3. 10% nodes unreachable
4. Single rack failure (affects 10-50 nodes)

**Playbook outline**:
```yaml
# failure/handle_unreachable.yml
- name: Handle unreachable nodes
  hosts: compute_nodes
  gather_facts: yes
  ignore_unreachable: yes
  tasks:
    - name: Apply configuration
      template:
        src: config.j2
        dest: /etc/app/config

  post_tasks:
    - name: Collect unreachable hosts
      set_fact:
        unreachable_hosts: "{{ ansible_play_hosts_all | difference(ansible_play_hosts) }}"
      run_once: yes

    - name: Report unreachable
      debug:
        msg: "Unreachable nodes: {{ unreachable_hosts | length }}"
      run_once: yes

    - name: Log for retry
      copy:
        content: "{{ unreachable_hosts | to_nice_yaml }}"
        dest: /var/log/rustible/unreachable_{{ ansible_date_time.iso8601 }}.yml
      delegate_to: localhost
      run_once: yes
```

**Expected Results**:

| Scale | Unreachable % | Execution | Reachable Success |
|-------|---------------|-----------|-------------------|
| 100 | 1% | Complete | 100% |
| 100 | 5% | Complete | 100% |
| 100 | 10% | Complete | 100% |
| 500 | 5% | Complete | 100% |
| 1000 | 5% | Complete | ≥99.9% |

---

### 7.2 Test: Mid-Execution Failure Recovery (FAIL-002)

**Objective**: Validate recovery from controller/network interruption.

**Scale targets**: 100, 500 nodes

**Failure scenarios**:
1. Controller process crash at 50% completion
2. Network partition at 50% completion
3. Controller OOM at high parallelism

**Steps**:
1. Start long-running playbook with checkpointing
2. Inject failure at 50% completion
3. Restart/recover
4. Verify checkpoint resume
5. Verify final state consistency

**Expected Results**:

| Scale | Checkpoint Interval | Recovery Time | State Consistency |
|-------|---------------------|---------------|-------------------|
| 100 | 1 min | <30 sec | 100% |
| 500 | 2 min | <1 min | 100% |

---

### 7.3 Test: Cascading Failure Prevention (FAIL-003)

**Objective**: Validate failure isolation prevents cascade.

**Scale targets**: 100, 500 nodes

**Scenario**: Failing task on one node should not affect other nodes.

**Expected Results**:

| Scale | Failing Nodes | Unaffected Nodes |
|-------|---------------|------------------|
| 100 | 5 | 95 (continue) |
| 500 | 25 | 475 (continue) |

---

## 8. Results Framework

### 8.1 Results Directory Structure

```
results/
├── small-scale/
│   ├── provisioning/
│   │   ├── PROV-001-10nodes-run1.json
│   │   ├── PROV-001-50nodes-run1.json
│   │   └── PROV-001-100nodes-run1.json
│   ├── reconfiguration/
│   ├── rollback/
│   ├── drift/
│   └── failure/
├── medium-scale/
│   ├── provisioning/
│   ├── reconfiguration/
│   ├── rollback/
│   ├── drift/
│   └── failure/
├── comparison/
│   ├── ansible-vs-rustible-100nodes.json
│   └── ansible-vs-rustible-1000nodes.json
└── summary/
    ├── small-scale-report.md
    └── medium-scale-report.md
```

### 8.2 Result Schema

```json
{
  "test_id": "PROV-001",
  "test_name": "Initial Node Provisioning",
  "run_id": "uuid",
  "timestamp": "ISO8601",
  "scale": 100,
  "environment": "aws|bare-metal",

  "configuration": {
    "forks": 50,
    "timeout": 300,
    "retry_count": 3
  },

  "results": {
    "status": "pass|fail|partial",
    "execution_time_seconds": 180.5,
    "hosts_total": 100,
    "hosts_success": 100,
    "hosts_failed": 0,
    "hosts_unreachable": 0,
    "tasks_total": 1500,
    "tasks_success": 1500,
    "tasks_failed": 0,
    "tasks_changed": 1200,
    "tasks_skipped": 100
  },

  "metrics": {
    "time_per_host_seconds": 1.8,
    "hosts_per_minute": 33.3,
    "peak_memory_mb": 1800,
    "avg_cpu_percent": 45,
    "peak_connections": 100,
    "latency_p50_ms": 50,
    "latency_p95_ms": 200,
    "latency_p99_ms": 500
  },

  "slo_compliance": {
    "execution_time": { "target": 300, "actual": 180.5, "pass": true },
    "success_rate": { "target": 0.999, "actual": 1.0, "pass": true },
    "memory_usage": { "target": 2048, "actual": 1800, "pass": true }
  },

  "issues": [],

  "notes": "Clean run, all nodes provisioned successfully."
}
```

### 8.3 Aggregated Results Template

```markdown
# Small/Medium Scale Validation Results

## Test Summary

| Test Category | Tests Run | Passed | Failed | Pass Rate |
|---------------|-----------|--------|--------|-----------|
| Provisioning | X | X | X | XX% |
| Reconfiguration | X | X | X | XX% |
| Rollback | X | X | X | XX% |
| Drift Detection | X | X | X | XX% |
| Partial Failure | X | X | X | XX% |
| **Total** | X | X | X | XX% |

## Performance Summary

### Small Scale (10-100 nodes)

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Provisioning time (100 nodes) | <5 min | X min | ✓/✗ |
| Configuration update | <2 min | X min | ✓/✗ |
| Rollback time | <30 sec | X sec | ✓/✗ |
| Drift detection | <1 min | X sec | ✓/✗ |
| Memory usage | <2 GB | X GB | ✓/✗ |

### Medium Scale (100-1,000 nodes)

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Provisioning time (1000 nodes) | <15 min | X min | ✓/✗ |
| Configuration update | <10 min | X min | ✓/✗ |
| Rollback time | <2 min | X min | ✓/✗ |
| Drift detection | <5 min | X min | ✓/✗ |
| Memory usage | <8 GB | X GB | ✓/✗ |

## Comparison with Ansible

| Operation | Scale | Ansible | Rustible | Speedup |
|-----------|-------|---------|----------|---------|
| Provisioning | 100 | X min | X min | X.Xx |
| Provisioning | 1000 | X min | X min | X.Xx |
| Config update | 100 | X min | X min | X.Xx |
| Config update | 1000 | X min | X min | X.Xx |

## Issues Discovered

| ID | Severity | Summary | Status |
|----|----------|---------|--------|
| #XXX | High | Description | Open/Closed |

## Recommendations

1. ...
2. ...
```

---

## 9. Issue Tracking Template

### 9.1 Issue Template for Failures

```markdown
## Issue: [Brief Description]

**Test ID**: PROV-001 / RECONF-002 / etc.
**Severity**: Critical / High / Medium / Low
**Scale**: 10 / 100 / 1000 nodes
**Environment**: bare-metal / AWS / etc.

### Reproduction Steps

1. Step one
2. Step two
3. Step three

### Expected Behavior

What should have happened.

### Actual Behavior

What actually happened.

### Error Output

```
Error messages, logs, etc.
```

### Impact

- Affects X% of nodes
- Blocks Y functionality
- Workaround: Z

### Root Cause Analysis

Initial analysis of what caused the issue.

### Suggested Fix

Proposed solution or investigation areas.

### Related

- Related issues: #XXX
- Related documentation: docs/hpc/XXX.md
```

### 9.2 Issue Severity Guidelines

| Severity | Criteria | Response Time |
|----------|----------|---------------|
| **Critical** | Data loss, cluster down, no workaround | Immediate |
| **High** | Major feature broken, partial workaround | 24 hours |
| **Medium** | Feature degraded, workaround available | 1 week |
| **Low** | Minor issue, cosmetic, enhancement | Backlog |

---

## 10. Execution Runbook

### 10.1 Pre-Execution Checklist

```markdown
## Pre-Validation Checklist

### Environment
- [ ] Controller node provisioned and accessible
- [ ] Compute nodes provisioned (target count)
- [ ] Network connectivity verified (all nodes pingable)
- [ ] SSH key authentication working
- [ ] Python installed on all targets
- [ ] Sudo access configured

### Tools
- [ ] Rustible installed (version X.Y.Z)
- [ ] Ansible installed for comparison (version X.Y.Z)
- [ ] Monitoring tools configured
- [ ] Result collection scripts ready

### Configuration
- [ ] Inventory files generated
- [ ] Playbooks tested on single node
- [ ] Checkpoint directory exists and writable
- [ ] Log collection configured

### Documentation
- [ ] Test plan reviewed
- [ ] Success criteria confirmed
- [ ] Issue tracker ready
```

### 10.2 Execution Order

```markdown
## Recommended Execution Order

### Day 1: Small Scale (10-100 nodes)

1. **Morning**: Provisioning tests
   - PROV-001 at 10, 50, 100 nodes
   - PROV-002 at 10, 100 nodes
   - PROV-003 at 10 nodes (if GPU available)

2. **Afternoon**: Reconfiguration tests
   - RECONF-001 at 10, 100 nodes
   - RECONF-002 at 100 nodes
   - RECONF-003 at 100 nodes

3. **End of day**: Initial results review

### Day 2: Small Scale Continued

1. **Morning**: Rollback tests
   - ROLL-001 at 10, 100 nodes
   - ROLL-002 at 100 nodes
   - ROLL-003 at 100 nodes

2. **Afternoon**: Drift and failure tests
   - DRIFT-001 at 100 nodes
   - DRIFT-002 at 100 nodes
   - FAIL-001 at 100 nodes
   - FAIL-002 at 100 nodes

### Day 3-4: Medium Scale (100-1000 nodes)

1. Provisioning tests at 500, 1000 nodes
2. Reconfiguration tests at 500, 1000 nodes
3. Rollback tests at 500, 1000 nodes
4. Drift detection at 1000 nodes (sampling)
5. Failure handling at 500, 1000 nodes

### Day 5: Comparison and Reporting

1. Run Ansible comparison tests
2. Generate comparison reports
3. Document issues found
4. Compile final report
```

### 10.3 Execution Commands

```bash
# Run single test
./scripts/run_validation.sh PROV-001 100 rustible

# Run test category
./scripts/run_category.sh provisioning small

# Run full validation suite
./scripts/run_full_validation.sh small
./scripts/run_full_validation.sh medium

# Generate report
./scripts/generate_report.sh results/small-scale/

# Run comparison
./scripts/run_comparison.sh 100 provisioning
./scripts/run_comparison.sh 1000 provisioning
```

### 10.4 Post-Execution

```markdown
## Post-Validation Checklist

- [ ] All test results collected
- [ ] Failed tests documented with issues
- [ ] Comparison data complete
- [ ] Summary report generated
- [ ] Issues filed in tracker
- [ ] Environment cleaned up (if cloud)
- [ ] Results archived
- [ ] Stakeholders notified
```

---

## Appendix: Quick Reference

### A.1 Test ID Reference

| ID | Category | Name |
|----|----------|------|
| PROV-001 | Provisioning | Initial Node Provisioning |
| PROV-002 | Provisioning | Slurm Node Registration |
| PROV-003 | Provisioning | GPU Node Provisioning |
| RECONF-001 | Reconfiguration | Configuration Update |
| RECONF-002 | Reconfiguration | Service Restart Coordination |
| RECONF-003 | Reconfiguration | Package Updates |
| ROLL-001 | Rollback | Configuration Rollback |
| ROLL-002 | Rollback | Partial Rollback |
| ROLL-003 | Rollback | Transaction Rollback |
| DRIFT-001 | Drift | Configuration Drift Detection |
| DRIFT-002 | Drift | Drift Remediation |
| DRIFT-003 | Drift | Continuous Drift Monitoring |
| FAIL-001 | Failure | Node Unreachable Handling |
| FAIL-002 | Failure | Mid-Execution Recovery |
| FAIL-003 | Failure | Cascading Failure Prevention |

### A.2 SLO Quick Reference (from Phase 2D)

| Scale | Provisioning | Uptime | Recovery |
|-------|--------------|--------|----------|
| 10-100 | <5 min | 99.5% | <5 min |
| 100-1K | <15 min | 99.9% | <15 min |
