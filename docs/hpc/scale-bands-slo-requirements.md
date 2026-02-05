# HPC Scale Bands and Operational SLO Requirements

Phase 2D of the HPC Initiative - Defining scale band expectations, operational SLOs, and risk constraints for HPC clusters.

## Table of Contents

1. [Scale Band Definitions](#1-scale-band-definitions)
2. [Provisioning and Reconfiguration SLOs](#2-provisioning-and-reconfiguration-slos)
3. [Availability and Uptime Targets](#3-availability-and-uptime-targets)
4. [Failure Recovery SLOs](#4-failure-recovery-slos)
5. [Change Control and Auditability](#5-change-control-and-auditability)
6. [Risk and Constraint Analysis](#6-risk-and-constraint-analysis)
7. [Measurement and Testing](#7-measurement-and-testing)

---

## 1. Scale Band Definitions

### 1.1 Scale Band Overview

| Band | Node Count | Typical Use Case | Complexity |
|------|------------|------------------|------------|
| **Small** | 10-100 | Departmental, pilot clusters | Low |
| **Medium** | 100-1,000 | Institutional, production HPC | Moderate |
| **Large** | 1,000-10,000 | Enterprise, multi-site | High |
| **Very Large** | 10,000+ | National facilities, TOP500 | Very High |

### 1.2 Characteristics by Scale Band

#### Small (10-100 nodes)

| Characteristic | Typical Value |
|----------------|---------------|
| **Operators** | 1-2 FTE |
| **Network** | Single switch tier |
| **Storage** | 100 TB - 1 PB |
| **Power** | < 500 kW |
| **Management** | Single head node |
| **Scheduler** | Basic Slurm/PBS |

#### Medium (100-1,000 nodes)

| Characteristic | Typical Value |
|----------------|---------------|
| **Operators** | 2-5 FTE |
| **Network** | Two-tier (leaf-spine) |
| **Storage** | 1-10 PB |
| **Power** | 500 kW - 5 MW |
| **Management** | HA head nodes |
| **Scheduler** | Full features, accounting |

#### Large (1,000-10,000 nodes)

| Characteristic | Typical Value |
|----------------|---------------|
| **Operators** | 5-15 FTE |
| **Network** | Multi-tier, fat-tree |
| **Storage** | 10-100 PB |
| **Power** | 5-50 MW |
| **Management** | Distributed, HA |
| **Scheduler** | Multi-cluster, federation |

#### Very Large (10,000+ nodes)

| Characteristic | Typical Value |
|----------------|---------------|
| **Operators** | 15-50+ FTE |
| **Network** | Custom topologies |
| **Storage** | 100+ PB |
| **Power** | 50+ MW |
| **Management** | Hierarchical, automated |
| **Scheduler** | Custom/modified |

---

## 2. Provisioning and Reconfiguration SLOs

### 2.1 Initial Node Provisioning

| Scale Band | Target Time | Max Time | Notes |
|------------|-------------|----------|-------|
| **Small** | 15 min/node | 30 min/node | Serial acceptable |
| **Medium** | 5 min/node | 15 min/node | Parallel required |
| **Large** | 2 min/node | 5 min/node | Highly parallel |
| **Very Large** | 1 min/node | 3 min/node | Orchestrated waves |

### 2.2 Batch Provisioning (Full Cluster)

| Scale Band | Node Count | Target Time | Max Time |
|------------|------------|-------------|----------|
| **Small** | 100 | 2 hours | 4 hours |
| **Medium** | 1,000 | 4 hours | 8 hours |
| **Large** | 10,000 | 8 hours | 16 hours |
| **Very Large** | 50,000 | 24 hours | 48 hours |

### 2.3 Configuration Updates

| Operation | Small | Medium | Large | Very Large |
|-----------|-------|--------|-------|------------|
| **Single file push** | 1 min | 2 min | 5 min | 10 min |
| **Package install** | 5 min | 10 min | 20 min | 45 min |
| **Kernel update** | 30 min | 1 hour | 2 hours | 4 hours |
| **OS reimage** | 1 hour | 2 hours | 6 hours | 12 hours |

### 2.4 Rolling Update SLOs

| Scale Band | Concurrent Nodes | Impact Threshold | Completion Target |
|------------|------------------|------------------|-------------------|
| **Small** | 10% | 10% capacity loss | 2 hours |
| **Medium** | 5% | 5% capacity loss | 4 hours |
| **Large** | 2% | 2% capacity loss | 8 hours |
| **Very Large** | 1% | 1% capacity loss | 24 hours |

---

## 3. Availability and Uptime Targets

### 3.1 Overall Cluster Availability

| Scale Band | Target Uptime | Max Downtime/Year | Maintenance Window |
|------------|---------------|-------------------|-------------------|
| **Small** | 99.5% | 43.8 hours | Weekly allowed |
| **Medium** | 99.9% | 8.76 hours | Monthly preferred |
| **Large** | 99.95% | 4.38 hours | Quarterly preferred |
| **Very Large** | 99.99% | 52 minutes | Rolling maintenance |

### 3.2 Component Availability Targets

| Component | Small | Medium | Large | Very Large |
|-----------|-------|--------|-------|------------|
| **Scheduler** | 99.9% | 99.95% | 99.99% | 99.99% |
| **Login nodes** | 99.5% | 99.9% | 99.95% | 99.99% |
| **Parallel FS** | 99.9% | 99.95% | 99.99% | 99.99% |
| **Network fabric** | 99.9% | 99.95% | 99.99% | 99.99% |
| **Individual node** | 95% | 97% | 98% | 99% |

### 3.3 Uptime Calculation

| Metric | Description | Formula |
|--------|-------------|---------|
| **Scheduled Uptime** | Planned operational hours | Total - Maintenance |
| **Actual Uptime** | Realized operational hours | Scheduled - Unplanned |
| **Availability %** | Uptime percentage | (Actual / Scheduled) × 100 |
| **MTBF** | Mean time between failures | Total Uptime / Failure Count |
| **MTTR** | Mean time to repair | Total Downtime / Failure Count |

---

## 4. Failure Recovery SLOs

### 4.1 Failure Detection Time

| Failure Type | Small | Medium | Large | Very Large |
|--------------|-------|--------|-------|------------|
| **Node down** | 5 min | 2 min | 1 min | 30 sec |
| **Service failure** | 2 min | 1 min | 30 sec | 15 sec |
| **Network partition** | 2 min | 1 min | 30 sec | 15 sec |
| **Storage failure** | 1 min | 30 sec | 15 sec | 10 sec |

### 4.2 Failure Response Time (Human)

| Priority | Small | Medium | Large | Very Large |
|----------|-------|--------|-------|------------|
| **Critical** | 15 min | 10 min | 5 min | Immediate |
| **High** | 1 hour | 30 min | 15 min | 10 min |
| **Medium** | 4 hours | 2 hours | 1 hour | 30 min |
| **Low** | Next day | 8 hours | 4 hours | 2 hours |

### 4.3 Recovery Time Objectives (RTO)

| Failure Scenario | Small | Medium | Large | Very Large |
|------------------|-------|--------|-------|------------|
| **Single node failure** | 30 min | 15 min | 10 min | 5 min |
| **Rack failure (40 nodes)** | 2 hours | 1 hour | 30 min | 15 min |
| **Network switch failure** | 1 hour | 30 min | 15 min | 10 min |
| **Storage server failure** | 1 hour | 30 min | 15 min | 10 min |
| **Scheduler failover** | 5 min | 2 min | 1 min | 30 sec |

### 4.4 Partial Failure Tolerance

| Scale Band | Acceptable Node Loss | Impact to Users |
|------------|---------------------|-----------------|
| **Small** | Up to 5% | Jobs may be killed |
| **Medium** | Up to 2% | Automatic requeue |
| **Large** | Up to 1% | Transparent to users |
| **Very Large** | Up to 0.5% | Full transparency |

### 4.5 Data Recovery Objectives (RPO)

| Data Type | Small | Medium | Large | Very Large |
|-----------|-------|--------|-------|------------|
| **Job checkpoints** | 1 hour | 30 min | 15 min | 5 min |
| **Accounting data** | 1 day | 4 hours | 1 hour | 15 min |
| **Configuration** | 1 day | 4 hours | 1 hour | Real-time |
| **User data** | Per user | 1 day | 4 hours | 1 hour |

---

## 5. Change Control and Auditability

### 5.1 Change Management Requirements

| Scale Band | Pre-Approval | Testing | Rollback Plan | Documentation |
|------------|--------------|---------|---------------|---------------|
| **Small** | Recommended | Optional | Recommended | Basic |
| **Medium** | Required | Required | Required | Standard |
| **Large** | CAB review | Staged | Automated | Comprehensive |
| **Very Large** | Multi-level | Full CI/CD | Mandatory auto | Full audit |

### 5.2 Change Categories

| Category | Risk Level | Approval | Testing | Examples |
|----------|------------|----------|---------|----------|
| **Standard** | Low | Pre-approved | Minimal | Config updates, patches |
| **Normal** | Medium | Single approver | Standard | Package updates, new modules |
| **Major** | High | CAB approval | Full staging | Kernel, driver updates |
| **Emergency** | Critical | Post-hoc review | Minimal | Security patches, outage fix |

### 5.3 Audit Trail Requirements

| Scale Band | Retention | Granularity | Access |
|------------|-----------|-------------|--------|
| **Small** | 90 days | Daily summary | Admin only |
| **Medium** | 1 year | Change-level | SOC access |
| **Large** | 3 years | Command-level | Compliance review |
| **Very Large** | 7 years | Full replay | Regulatory audit |

### 5.4 Audit Data Points

| Category | Data Captured | Format |
|----------|---------------|--------|
| **Who** | User/operator identity | Username, UID |
| **What** | Commands/changes | Full command, diff |
| **When** | Timestamp | UTC, NTP synced |
| **Where** | Source/target systems | Hostname, IP |
| **Why** | Change ticket/reason | Ticket ID, comment |
| **Result** | Outcome | Success/failure, exit code |

---

## 6. Risk and Constraint Analysis

### 6.1 Small Scale (10-100 nodes)

**Constraints:**
- Limited redundancy budget
- Single points of failure acceptable
- Operator expertise may be limited

**Risks:**
| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Single node failure | Medium | Low | Basic monitoring |
| Head node failure | Low | High | Regular backups |
| Network failure | Low | High | Dual uplinks |
| Operator error | Medium | Medium | Documentation |

**Recommendations:**
- Focus on simplicity over redundancy
- Implement basic monitoring (Nagios/Prometheus)
- Weekly configuration backups
- Document all procedures

### 6.2 Medium Scale (100-1,000 nodes)

**Constraints:**
- Budget for partial redundancy
- Need for standardization
- Multiple operator skillsets

**Risks:**
| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Batch node failures | Medium | Medium | Automated replacement |
| Scheduler overload | Medium | High | HA scheduler |
| Storage bottleneck | Medium | High | Monitoring, expansion |
| Config drift | High | Medium | Automation (Ansible/Rustible) |

**Recommendations:**
- HA for head nodes and scheduler
- Configuration management mandatory
- Staged rolling updates
- Capacity planning quarterly

### 6.3 Large Scale (1,000-10,000 nodes)

**Constraints:**
- Complex failure domains
- Change coordination critical
- Blast radius concerns

**Risks:**
| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Correlated failures | Medium | Very High | Fault domains |
| Change-induced outage | Medium | Very High | Canary deployments |
| Scale bottlenecks | High | High | Distributed architecture |
| Human error cascade | Low | Very High | Automation, RBAC |

**Recommendations:**
- Fault domain design mandatory
- Canary/staged deployments required
- Comprehensive monitoring and alerting
- Full configuration as code
- Automated rollback capability

### 6.4 Very Large Scale (10,000+ nodes)

**Constraints:**
- Unique operational challenges
- Global coordination required
- Vendor-level support dependencies

**Risks:**
| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Systemic failures | Low | Catastrophic | Multi-level redundancy |
| Firmware bugs | Medium | Very High | Staged firmware rollout |
| Network storms | Low | Catastrophic | Traffic isolation |
| Automation failures | Medium | Very High | Human oversight, kill switches |

**Recommendations:**
- Custom tooling and automation
- Dedicated operations team
- Multi-datacenter considerations
- Vendor partnerships
- Continuous improvement program

---

## 7. Measurement and Testing

### 7.1 SLO Measurement Methods

| Metric | Measurement Method | Frequency |
|--------|-------------------|-----------|
| **Provisioning time** | Automated timing from PXE to ready | Per operation |
| **Uptime** | Synthetic probes + real monitoring | Continuous |
| **MTTR** | Incident tracking system | Per incident |
| **Change success rate** | Deployment pipeline metrics | Per change |
| **Compliance** | Configuration audits | Weekly/monthly |

### 7.2 Testing Framework

#### Provisioning Tests

| Test | Frequency | Acceptance |
|------|-----------|------------|
| Single node provision | Weekly | < Target time |
| Batch provision (10 nodes) | Monthly | < Target time |
| Full reimage validation | Quarterly | All services pass |
| Rollback test | Monthly | Successful restore |

#### Failure Recovery Tests

| Test | Frequency | Acceptance |
|------|-----------|------------|
| Node failure simulation | Weekly | Auto-recovery < RTO |
| Scheduler failover | Monthly | < 2 min failover |
| Network partition | Quarterly | Services survive |
| Storage failover | Quarterly | < RTO, no data loss |

#### Change Management Tests

| Test | Frequency | Acceptance |
|------|-----------|------------|
| Canary deployment | Per change | No errors |
| Rollback execution | Monthly | < 10 min restore |
| Audit log completeness | Monthly | 100% coverage |
| Compliance scan | Weekly | All nodes compliant |

### 7.3 Reporting Dashboard Metrics

| Metric Category | Metrics |
|-----------------|---------|
| **Availability** | Uptime %, incident count, MTTR |
| **Provisioning** | Avg time, success rate, queue depth |
| **Compliance** | Drift %, audit findings, remediation time |
| **Capacity** | Node utilization, storage usage, growth trend |
| **Performance** | Job throughput, wait time, efficiency |

### 7.4 SLO Compliance Reporting

| Report | Frequency | Audience |
|--------|-----------|----------|
| **Daily health** | Daily | Operations |
| **Weekly summary** | Weekly | Management |
| **Monthly SLO** | Monthly | Stakeholders |
| **Quarterly review** | Quarterly | Leadership |
| **Annual assessment** | Annually | Executive |

---

## Appendix: SLO Summary Table

| SLO Category | Small (10-100) | Medium (100-1K) | Large (1K-10K) | Very Large (10K+) |
|--------------|----------------|-----------------|----------------|-------------------|
| **Cluster Uptime** | 99.5% | 99.9% | 99.95% | 99.99% |
| **Node Provision** | 15 min | 5 min | 2 min | 1 min |
| **Config Push** | 1 min | 2 min | 5 min | 10 min |
| **Node Recovery** | 30 min | 15 min | 10 min | 5 min |
| **Scheduler Failover** | 5 min | 2 min | 1 min | 30 sec |
| **Detection Time** | 5 min | 2 min | 1 min | 30 sec |
| **Partial Loss Tolerance** | 5% | 2% | 1% | 0.5% |
| **Audit Retention** | 90 days | 1 year | 3 years | 7 years |
| **Change Approval** | Recommended | Required | CAB | Multi-level |

---

## References

- [Uptime Institute Tier Standards](https://uptimeinstitute.com/tiers)
- [Google SRE - Embracing Risk](https://sre.google/sre-book/embracing-risk/)
- [ITIL Change Management](https://www.axelos.com/certifications/itil-service-management)
- [HPC Cluster Management Best Practices](https://www.hpe.com/us/en/what-is/hpc-clusters.html)
- [Azure Well-Architected Framework - Reliability](https://learn.microsoft.com/en-us/azure/well-architected/reliability/metrics)
