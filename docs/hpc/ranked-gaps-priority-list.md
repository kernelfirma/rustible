# Ranked HPC Gaps: Priority List with Impact/Effort/Confidence

Phase 4B of the HPC Initiative - Scoring and ranking the most important gaps for HPC adoption with explicit rationale and sequencing.

## Table of Contents

1. [Scoring Rubric](#1-scoring-rubric)
2. [Ranked Gap List](#2-ranked-gap-list)
3. [Quick Wins](#3-quick-wins)
4. [Strategic Bets](#4-strategic-bets)
5. [Dependencies and Sequencing](#5-dependencies-and-sequencing)
6. [Implementation Phases](#6-implementation-phases)

---

## 1. Scoring Rubric

### 1.1 Impact Score (1-5)

| Score | Definition | Examples |
|-------|------------|----------|
| **5** | Blocks HPC adoption entirely | No scheduler control, no power management |
| **4** | Severely limits HPC use cases | Missing fabric support, no GPU management |
| **3** | Significant operational burden | Manual workarounds required daily |
| **2** | Moderate inconvenience | Occasional manual intervention |
| **1** | Minor enhancement | Nice-to-have improvements |

### 1.2 Effort Score (1-5)

| Score | Definition | Estimated Time |
|-------|------------|----------------|
| **1** | Trivial | < 1 week |
| **2** | Small | 1-2 weeks |
| **3** | Medium | 2-4 weeks |
| **4** | Large | 1-2 months |
| **5** | Very Large | 2+ months |

### 1.3 Confidence Score (1-5)

| Score | Definition | Evidence Level |
|-------|------------|----------------|
| **5** | Certain | Direct testing, proven patterns |
| **4** | High | Strong precedent, clear requirements |
| **3** | Medium | Some unknowns, reasonable assumptions |
| **2** | Low | Significant unknowns |
| **1** | Speculative | Novel territory, research needed |

### 1.4 Priority Formula

```
Priority Score = (Impact × 2) + (6 - Effort) + Confidence
                 ─────────────────────────────────────────
                              Max possible (20)
```

**Rationale:**
- Impact weighted 2x (most important factor)
- Effort inverted (lower effort = higher score)
- Confidence adds certainty bonus
- Normalized to 0-1 scale for comparison

---

## 2. Ranked Gap List

### 2.1 Complete Ranking (Top 20)

| Rank | Gap ID | Description | Impact | Effort | Conf. | Score | Category |
|------|--------|-------------|--------|--------|-------|-------|----------|
| 1 | BM-01 | IPMI power control | 5 | 2 | 5 | 0.90 | Quick Win |
| 2 | SCH-01 | Slurm node state management | 5 | 2 | 5 | 0.90 | Quick Win |
| 3 | FS-01 | Lustre client mount | 5 | 1 | 5 | 0.95 | Quick Win |
| 4 | SCH-02 | Slurm partition configuration | 5 | 2 | 5 | 0.90 | Quick Win |
| 5 | BM-02 | Redfish power/firmware | 5 | 3 | 4 | 0.80 | Foundation |
| 6 | IB-01 | OpenSM configuration | 5 | 3 | 4 | 0.80 | Foundation |
| 7 | GPU-01 | NVIDIA driver installation | 5 | 2 | 4 | 0.85 | Quick Win |
| 8 | IB-02 | IB partition configuration | 4 | 3 | 4 | 0.70 | Foundation |
| 9 | BM-03 | PXE boot configuration | 4 | 3 | 4 | 0.70 | Foundation |
| 10 | IB-03 | IPoIB interface setup | 4 | 2 | 4 | 0.75 | Quick Win |
| 11 | FS-03 | BeeGFS client setup | 4 | 2 | 4 | 0.75 | Quick Win |
| 12 | GPU-02 | CUDA toolkit multi-version | 4 | 3 | 4 | 0.70 | Foundation |
| 13 | ID-01 | SSSD configuration | 4 | 3 | 4 | 0.70 | Foundation |
| 14 | SCH-03 | Slurm accounting setup | 4 | 3 | 4 | 0.70 | Foundation |
| 15 | ID-02 | Kerberos client setup | 4 | 2 | 4 | 0.75 | Quick Win |
| 16 | FS-02 | Lustre OST management | 5 | 4 | 3 | 0.65 | Strategic |
| 17 | IB-05 | Fabric diagnostics | 3 | 3 | 4 | 0.60 | Foundation |
| 18 | SC-01 | Large-scale validation | 5 | 5 | 2 | 0.55 | Strategic |
| 19 | BM-04 | Warewulf integration | 4 | 4 | 3 | 0.55 | Strategic |
| 20 | SW-01 | Lmod installation | 3 | 2 | 4 | 0.65 | Quick Win |

### 2.2 Score Calculation Examples

**Rank #1: BM-01 (IPMI power control)**
```
Impact:     5 (Blocks bare-metal operations)
Effort:     2 (Well-defined IPMI spec, existing libraries)
Confidence: 5 (Proven pattern, ipmitool reference)

Score = ((5×2) + (6-2) + 5) / 20 = 19/20 = 0.95
```

**Rank #16: FS-02 (Lustre OST management)**
```
Impact:     5 (Critical for storage operations)
Effort:     4 (Complex Lustre internals, multiple operations)
Confidence: 3 (Some unknowns in server-side operations)

Score = ((5×2) + (6-4) + 3) / 20 = 15/20 = 0.75
```

**Rank #18: SC-01 (Large-scale validation)**
```
Impact:     5 (Essential for HPC credibility)
Effort:     5 (Requires test infrastructure, time)
Confidence: 2 (Unknown performance characteristics)

Score = ((5×2) + (6-5) + 2) / 20 = 13/20 = 0.65
```

---

## 3. Quick Wins

### 3.1 Definition

Quick wins are gaps with:
- **Impact ≥ 4**: Meaningful HPC value
- **Effort ≤ 2**: Implementable in 1-2 weeks
- **Confidence ≥ 4**: Clear path to implementation

### 3.2 Quick Win List

| Priority | Gap ID | Description | Impact | Effort | Time Est. |
|----------|--------|-------------|--------|--------|-----------|
| 1 | FS-01 | Lustre client mount | 5 | 1 | 3-5 days |
| 2 | BM-01 | IPMI power control | 5 | 2 | 1-2 weeks |
| 3 | SCH-01 | Slurm node state | 5 | 2 | 1-2 weeks |
| 4 | SCH-02 | Slurm partition config | 5 | 2 | 1-2 weeks |
| 5 | GPU-01 | NVIDIA driver module | 5 | 2 | 1-2 weeks |
| 6 | IB-03 | IPoIB interface | 4 | 2 | 1 week |
| 7 | FS-03 | BeeGFS client | 4 | 2 | 1 week |
| 8 | ID-02 | Kerberos client | 4 | 2 | 1 week |
| 9 | SW-01 | Lmod module | 3 | 2 | 1 week |

### 3.3 Quick Win Rationale

**FS-01: Lustre client mount**
- Extends existing `mount` module with Lustre-specific options
- LNet configuration is well-documented
- Immediate value for any Lustre-using HPC site
- Reference: [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §4.3

**BM-01: IPMI power control**
- Standard IPMI 2.0 protocol, well-documented
- Existing Rust crates for IPMI (e.g., `ipmi-rs`)
- Every bare-metal HPC site needs this
- Reference: [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §2.2

**SCH-01: Slurm node state**
- Wraps `scontrol` commands with state management
- Clear API: drain, resume, idle states
- Most HPC sites use Slurm
- Reference: [scheduler-requirements-matrix.md](./scheduler-requirements-matrix.md) §1.2

**GPU-01: NVIDIA driver module**
- Package installation with DKMS handling
- Version management and persistence configuration
- Growing GPU HPC market
- Reference: [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §1.2

---

## 4. Strategic Bets

### 4.1 Definition

Strategic bets are gaps with:
- **Impact = 5**: Critical for HPC adoption
- **Effort ≥ 4**: Significant investment required
- Potential for differentiation or long-term value

### 4.2 Strategic Bet List

| Priority | Gap ID | Description | Impact | Effort | Risk | Payoff |
|----------|--------|-------------|--------|--------|------|--------|
| 1 | SC-01 | Large-scale validation | 5 | 5 | Medium | High |
| 2 | FS-02 | Lustre OST management | 5 | 4 | Medium | High |
| 3 | BM-04 | Warewulf integration | 4 | 4 | Low | Medium |
| 4 | IB-01 | OpenSM configuration | 5 | 3 | Medium | High |

### 4.3 Strategic Bet Rationale

**SC-01: Large-scale validation (10,000+ nodes)**
- **Risk**: Requires test infrastructure, may reveal unknown issues
- **Payoff**: Validates HPC credibility, differentiates from Ansible
- **Approach**: Partner with HPC site, incremental scale testing
- **Timeline**: 2-4 weeks testing once infrastructure available
- **Reference**: [scale-bands-slo-requirements.md](./scale-bands-slo-requirements.md) §1.1

**FS-02: Lustre OST management**
- **Risk**: Complex Lustre internals, potential for data issues
- **Payoff**: Full storage lifecycle, unique capability
- **Approach**: Start with read-only operations, add mutations carefully
- **Timeline**: 4-6 weeks with careful testing
- **Reference**: [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §4.4

**BM-04: Warewulf integration**
- **Risk**: External tool dependency, version compatibility
- **Payoff**: Complete bare-metal provisioning story
- **Approach**: API integration, not replacement
- **Timeline**: 4-6 weeks
- **Reference**: [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §1.4

**IB-01: OpenSM configuration**
- **Risk**: Fabric disruption potential, complex state
- **Payoff**: InfiniBand management differentiator
- **Approach**: Template-based initially, then native module
- **Timeline**: 3-4 weeks
- **Reference**: [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §3.2

---

## 5. Dependencies and Sequencing

### 5.1 Dependency Graph

```
                    ┌─────────────────────────────────────────────────┐
                    │              Foundation Layer                    │
                    │  (Must complete before dependent gaps)           │
                    └─────────────────────────────────────────────────┘
                                          │
           ┌──────────────────────────────┼──────────────────────────────┐
           │                              │                              │
           ▼                              ▼                              ▼
    ┌─────────────┐              ┌─────────────┐              ┌─────────────┐
    │   BM-01     │              │   SCH-01    │              │   FS-01     │
    │ IPMI power  │              │ Slurm node  │              │ Lustre mount│
    └──────┬──────┘              └──────┬──────┘              └──────┬──────┘
           │                            │                            │
           ▼                            ▼                            ▼
    ┌─────────────┐              ┌─────────────┐              ┌─────────────┐
    │   BM-02     │              │   SCH-02    │              │   FS-02     │
    │  Redfish    │              │ Slurm part  │              │ Lustre OST  │
    └──────┬──────┘              └──────┬──────┘              └─────────────┘
           │                            │
           ▼                            ▼
    ┌─────────────┐              ┌─────────────┐
    │   BM-03     │              │   SCH-03    │
    │  PXE boot   │              │ Slurm acct  │
    └──────┬──────┘              └─────────────┘
           │
           ▼
    ┌─────────────┐
    │   BM-04     │
    │  Warewulf   │
    └─────────────┘


    ┌─────────────────────────────────────────────────────────────────┐
    │                     Parallel Tracks                              │
    │  (Can be implemented independently)                              │
    └─────────────────────────────────────────────────────────────────┘

    Track A: Fabric          Track B: GPU           Track C: Identity
    ┌─────────────┐         ┌─────────────┐        ┌─────────────┐
    │   IB-01     │         │   GPU-01    │        │   ID-01     │
    │  OpenSM     │         │ NVIDIA drv  │        │   SSSD      │
    └──────┬──────┘         └──────┬──────┘        └──────┬──────┘
           │                       │                      │
           ▼                       ▼                      ▼
    ┌─────────────┐         ┌─────────────┐        ┌─────────────┐
    │   IB-02     │         │   GPU-02    │        │   ID-02     │
    │ IB partition│         │   CUDA      │        │  Kerberos   │
    └──────┬──────┘         └─────────────┘        └─────────────┘
           │
           ▼
    ┌─────────────┐
    │   IB-03     │
    │   IPoIB     │
    └─────────────┘
```

### 5.2 Dependency Matrix

| Gap ID | Depends On | Blocks | Parallel With |
|--------|------------|--------|---------------|
| BM-01 | None | BM-02, BM-03 | SCH-01, FS-01 |
| BM-02 | BM-01 | BM-03 | SCH-02 |
| BM-03 | BM-02 | BM-04 | IB-01 |
| BM-04 | BM-03 | None | SC-01 |
| SCH-01 | None | SCH-02, SCH-03 | BM-01, FS-01 |
| SCH-02 | SCH-01 | None | BM-02, IB-01 |
| SCH-03 | SCH-02 | None | GPU-01 |
| FS-01 | None | FS-02 | BM-01, SCH-01 |
| FS-02 | FS-01 | None | SCH-03 |
| FS-03 | None | None | FS-01 |
| IB-01 | None | IB-02 | BM-02, GPU-01 |
| IB-02 | IB-01 | IB-03 | SCH-02 |
| IB-03 | IB-02 | None | GPU-02 |
| GPU-01 | None | GPU-02 | IB-01, ID-01 |
| GPU-02 | GPU-01 | None | IB-02, ID-02 |
| ID-01 | None | ID-02 | GPU-01 |
| ID-02 | ID-01 | None | GPU-02 |
| SC-01 | BM-04, SCH-03, FS-02 | None | None |

### 5.3 Critical Path

```
BM-01 → BM-02 → BM-03 → BM-04 ─┐
                                │
SCH-01 → SCH-02 → SCH-03 ───────┼───→ SC-01 (Large-scale validation)
                                │
FS-01 → FS-02 ──────────────────┘
```

**Critical path duration**: ~10-12 weeks

---

## 6. Implementation Phases

### 6.1 Phase Overview

| Phase | Name | Duration | Gaps | Outcome |
|-------|------|----------|------|---------|
| **1** | Core Quick Wins | 3 weeks | BM-01, SCH-01, FS-01, SCH-02 | Basic HPC operations |
| **2** | Extended Control | 3 weeks | BM-02, GPU-01, IB-03, FS-03 | Bare-metal + GPU |
| **3** | Fabric & Identity | 3 weeks | IB-01, IB-02, ID-01, ID-02 | Full fabric + auth |
| **4** | Advanced Storage | 2 weeks | GPU-02, SCH-03, FS-02 | Complete stack |
| **5** | Integration | 2 weeks | BM-03, BM-04, IB-05 | Provisioning workflow |
| **6** | Validation | 3 weeks | SC-01, SW-01 | Scale testing |

### 6.2 Phase 1: Core Quick Wins (Weeks 1-3)

| Week | Gap | Deliverable | Dependencies |
|------|-----|-------------|--------------|
| 1 | FS-01 | `lustre_mount` module | None |
| 1-2 | BM-01 | `ipmi_power`, `ipmi_boot` modules | None |
| 2 | SCH-01 | `slurm_node` module | None |
| 3 | SCH-02 | `slurm_partition` module | SCH-01 |

**Milestone**: Basic Slurm cluster management with Lustre storage

### 6.3 Phase 2: Extended Control (Weeks 4-6)

| Week | Gap | Deliverable | Dependencies |
|------|-----|-------------|--------------|
| 4 | BM-02 | `redfish_power`, `redfish_info` modules | BM-01 |
| 4-5 | GPU-01 | `nvidia_driver` module | None |
| 5 | IB-03 | `ipoib` module | None |
| 6 | FS-03 | `beegfs_mount` module | None |

**Milestone**: Modern server management, GPU support, network options

### 6.4 Phase 3: Fabric & Identity (Weeks 7-9)

| Week | Gap | Deliverable | Dependencies |
|------|-----|-------------|--------------|
| 7-8 | IB-01 | `opensm_config` module | None |
| 8 | IB-02 | `ib_partition` module | IB-01 |
| 8-9 | ID-01 | `sssd_config`, `sssd_domain` modules | None |
| 9 | ID-02 | `krb5_config` module | ID-01 |

**Milestone**: Full InfiniBand fabric control, enterprise identity

### 6.5 Phase 4: Advanced Storage (Weeks 10-11)

| Week | Gap | Deliverable | Dependencies |
|------|-----|-------------|--------------|
| 10 | GPU-02 | `cuda_toolkit` module | GPU-01 |
| 10 | SCH-03 | `slurm_account`, `slurm_qos` modules | SCH-02 |
| 11 | FS-02 | `lustre_ost` module | FS-01 |

**Milestone**: Complete software stack, storage lifecycle

### 6.6 Phase 5: Integration (Weeks 12-13)

| Week | Gap | Deliverable | Dependencies |
|------|-----|-------------|--------------|
| 12 | BM-03 | `pxe_host` module | BM-02 |
| 12-13 | BM-04 | `warewulf_node` module | BM-03 |
| 13 | IB-05 | `ib_info`, `ib_health` modules | IB-02 |

**Milestone**: End-to-end provisioning workflow

### 6.7 Phase 6: Validation (Weeks 14-16)

| Week | Gap | Deliverable | Dependencies |
|------|-----|-------------|--------------|
| 14 | SW-01 | `lmod` module | None |
| 14-16 | SC-01 | Scale testing (100→1000→10000) | All above |

**Milestone**: Validated at HPC scale

### 6.8 Resource Allocation

| Phase | Developer Weeks | Testing Weeks | Total |
|-------|-----------------|---------------|-------|
| 1 | 3 | 1 | 4 |
| 2 | 3 | 1 | 4 |
| 3 | 3 | 1 | 4 |
| 4 | 2 | 1 | 3 |
| 5 | 2 | 1 | 3 |
| 6 | 1 | 3 | 4 |
| **Total** | **14** | **8** | **22** |

---

## Appendix: Scoring Audit Trail

### A.1 All Gaps with Full Scoring

| Gap ID | Impact | Impact Rationale | Effort | Effort Rationale | Conf. | Conf. Rationale | Final |
|--------|--------|------------------|--------|------------------|-------|-----------------|-------|
| BM-01 | 5 | Blocks all bare-metal ops | 2 | Standard IPMI protocol | 5 | Proven pattern | 0.90 |
| BM-02 | 5 | Modern server requirement | 3 | RESTful but complex | 4 | Good docs | 0.80 |
| BM-03 | 4 | Provisioning enabler | 3 | DHCP/TFTP integration | 4 | Known patterns | 0.70 |
| BM-04 | 4 | Complete provisioning | 4 | External tool integration | 3 | API stability unknown | 0.55 |
| SCH-01 | 5 | Core scheduler function | 2 | Simple state machine | 5 | Clear scontrol API | 0.90 |
| SCH-02 | 5 | Partition management | 2 | Config file generation | 5 | Well documented | 0.90 |
| SCH-03 | 4 | Accounting needed | 3 | Database integration | 4 | Good examples | 0.70 |
| FS-01 | 5 | Storage access critical | 1 | Extend mount module | 5 | Simple addition | 0.95 |
| FS-02 | 5 | Storage lifecycle | 4 | Complex Lustre ops | 3 | Server-side unknowns | 0.65 |
| FS-03 | 4 | Alternative storage | 2 | Similar to Lustre | 4 | Good BeeGFS docs | 0.75 |
| IB-01 | 5 | Fabric control | 3 | OpenSM complexity | 4 | Documented config | 0.80 |
| IB-02 | 4 | Security isolation | 3 | Partition keys | 4 | Standard patterns | 0.70 |
| IB-03 | 4 | IP over IB | 2 | Network module extend | 4 | Clear requirements | 0.75 |
| IB-05 | 3 | Diagnostics | 3 | Tool integration | 4 | Known tools | 0.60 |
| GPU-01 | 5 | GPU critical | 2 | Package + DKMS | 4 | NVIDIA docs | 0.85 |
| GPU-02 | 4 | Multi-version | 3 | Path management | 4 | Common pattern | 0.70 |
| ID-01 | 4 | Enterprise auth | 3 | Complex config | 4 | RHEL examples | 0.70 |
| ID-02 | 4 | Kerberos needed | 2 | Config templating | 4 | Standard setup | 0.75 |
| SW-01 | 3 | User convenience | 2 | Package + config | 4 | Well documented | 0.65 |
| SC-01 | 5 | HPC credibility | 5 | Test infrastructure | 2 | Unknown perf | 0.55 |

### A.2 Score Distribution

| Score Range | Count | Category |
|-------------|-------|----------|
| 0.90-1.00 | 4 | Top priority quick wins |
| 0.80-0.89 | 2 | High priority |
| 0.70-0.79 | 8 | Medium-high priority |
| 0.60-0.69 | 4 | Medium priority |
| 0.50-0.59 | 2 | Strategic bets |
