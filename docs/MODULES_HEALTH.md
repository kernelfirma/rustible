# Rustible Module Health Dashboard

> **Generated:** 2026-02-17
> **Total Modules:** 125
> **Average Health Score:** 68/100

## Summary Statistics

| Metric | Value |
|--------|-------|
| **Total Modules** | 125 |
| **Core Modules** | 76 |
| **HPC Modules** | 49 |
| **With Tests** | 23 (18%) |
| **Excellent Docs** | 12 (10%) |
| **Good Docs** | 28 (22%) |
| **Partial Docs** | 73 (58%) |
| **Missing Docs** | 12 (10%) |
| **Recently Updated (7d)** | 49 |

## Health Score Legend

| Score | Status | Meaning |
|-------|--------|---------|
| 90-100 | Excellent | Well-tested, documented, actively maintained |
| 70-89 | Good | Adequate coverage, minor improvements needed |
| 50-69 | Fair | Needs attention, lacking tests or docs |
| 0-49 | Needs Work | Critical gaps in tests, docs, or maintenance |

## Documentation Status Legend

| Status | Meaning |
|--------|---------|
| **Excellent** | Module-level docs + all public items documented (50+ doc comments) |
| **Good** | Module-level docs + most items documented (20-49 doc comments) |
| **Partial** | Some documentation present (5-19 doc comments) |
| **Missing** | Minimal or no documentation (<5 doc comments) |

---

## Core Modules (39)

### Package Management

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| apt | 92 | 1563 | Yes | 30 | Excellent | 2026-01-01 |
| dnf | 88 | 886 | Yes | 27 | Good | 2025-12-30 |
| yum | 85 | 864 | Yes | 30 | Good | 2025-12-30 |
| pip | 82 | 960 | Yes | 34 | Partial | 2025-12-27 |
| package | 78 | 531 | Yes | 36 | Missing | 2025-12-27 |

### Command Execution

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| command | 85 | 502 | Yes | 31 | Partial | 2025-12-30 |
| shell | 82 | 556 | Yes | 22 | Partial | 2026-01-01 |

### File Operations

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| copy | 65 | 976 | No | 0 | Partial | 2025-12-25 |
| file | 62 | 1029 | No | 0 | Good | 2025-12-27 |
| lineinfile | 58 | 948 | No | 0 | Partial | 2026-01-01 |
| blockinfile | 55 | 608 | No | 0 | Partial | 2025-12-27 |
| template | 52 | 763 | No | 0 | Missing | 2025-12-30 |
| archive | 88 | 847 | Yes | 17 | Good | 2025-12-27 |
| unarchive | 68 | 1111 | No | 0 | Excellent | 2025-12-28 |
| stat | 85 | 383 | Yes | 19 | Partial | 2025-12-27 |

### System Administration

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| service | 90 | 1499 | Yes | 27 | Excellent | 2025-12-27 |
| systemd_unit | 92 | 1551 | Yes | 41 | Excellent | 2025-12-27 |
| user | 88 | 828 | Yes | 35 | Partial | 2025-12-30 |
| group | 85 | 444 | Yes | 28 | Partial | 2025-12-30 |
| cron | 60 | 683 | No | 0 | Good | 2026-01-01 |
| mount | 58 | 817 | No | 0 | Good | 2026-01-01 |
| sysctl | 55 | 560 | No | 0 | Partial | 2026-01-01 |
| hostname | 58 | 492 | No | 0 | Good | 2026-01-01 |
| timezone | 85 | 813 | Yes | 23 | Excellent | 2026-01-01 |

### Security & Firewall

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| firewalld | 92 | 1291 | Yes | 60 | Excellent | 2026-01-01 |
| ufw | 92 | 1207 | Yes | 75 | Excellent | 2026-01-01 |
| selinux | 90 | 1613 | Yes | 27 | Excellent | 2026-01-01 |
| authorized_key | 92 | 1162 | Yes | 87 | Excellent | 2025-12-30 |
| known_hosts | 92 | 1097 | Yes | 78 | Excellent | 2025-12-27 |

### Source Control

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| git | 85 | 1086 | Yes | 23 | Good | 2025-12-25 |

### Network & HTTP

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| uri | 88 | 1162 | Yes | 25 | Excellent | 2025-12-27 |
| wait_for | 90 | 1018 | Yes | 37 | Excellent | 2026-01-01 |

### Utility & Logic

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| debug | 55 | 378 | No | 0 | Partial | 2025-12-30 |
| assert | 52 | 511 | No | 0 | Excellent | 2025-12-26 |
| set_fact | 45 | 268 | No | 0 | Missing | 2025-12-23 |
| include_vars | 50 | 572 | No | 0 | Partial | 2025-12-25 |
| pause | 82 | 696 | Yes | 31 | Partial | 2025-12-27 |
| facts | 60 | 1155 | No | 0 | Good | 2026-01-01 |
| python | 55 | 632 | No | 0 | Good | 2025-12-27 |

---

## Docker Modules (6)

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| docker/docker_container | 72 | 901 | No | 0 | Excellent | 2025-12-27 |
| docker/docker_compose | 72 | 873 | No | 0 | Excellent | 2025-12-27 |
| docker/docker_image | 70 | 692 | No | 0 | Good | 2025-12-27 |
| docker/docker_network | 68 | 605 | No | 0 | Good | 2025-12-27 |
| docker/docker_volume | 65 | 411 | No | 0 | Good | 2025-12-27 |
| docker/mod | 50 | 74 | No | 0 | Missing | 2025-12-27 |

---

## Kubernetes Modules (11)

### Native k8s Modules

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| k8s/k8s_deployment | 65 | 690 | No | 0 | Partial | 2025-12-27 |
| k8s/k8s_service | 65 | 671 | No | 0 | Partial | 2025-12-27 |
| k8s/k8s_namespace | 65 | 625 | No | 0 | Partial | 2025-12-27 |
| k8s/k8s_secret | 62 | 542 | No | 0 | Partial | 2025-12-27 |
| k8s/k8s_configmap | 62 | 532 | No | 0 | Partial | 2025-12-27 |
| k8s/mod | 40 | 33 | No | 0 | Missing | 2025-12-27 |

### Cloud Kubernetes Modules

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| cloud/kubernetes/deployment | 72 | 1020 | No | 0 | Partial | 2026-01-02 |
| cloud/kubernetes/mod | 75 | 714 | No | 0 | Excellent | 2026-01-02 |
| cloud/kubernetes/service | 70 | 738 | No | 0 | Partial | 2025-12-27 |
| cloud/kubernetes/configmap | 68 | 650 | No | 0 | Partial | 2025-12-27 |
| cloud/kubernetes/secret | 58 | 326 | No | 0 | Missing | 2026-01-02 |

---

## Cloud Provider Modules (7)

### AWS

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| cloud/aws/ec2 | 78 | 2432 | No | 0 | Excellent | 2025-12-28 |
| cloud/aws/s3 | 80 | 1857 | No | 0 | Excellent | 2025-12-28 |
| cloud/aws/mod | 45 | 31 | No | 0 | Missing | 2025-12-27 |

### GCP

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| cloud/gcp/compute | 78 | 2012 | No | 0 | Excellent | 2025-12-27 |
| cloud/gcp/mod | 45 | 63 | No | 0 | Missing | 2025-12-27 |

### Azure

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| cloud/azure/vm | 75 | 1926 | No | 0 | Excellent | 2025-12-27 |
| cloud/azure/mod | 45 | 56 | No | 0 | Missing | 2025-12-27 |

---

## Network Device Modules (5)

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| network/junos_config | 82 | 1584 | No | 0 | Excellent | 2025-12-27 |
| network/eos_config | 80 | 1924 | No | 0 | Excellent | 2025-12-28 |
| network/ios_config | 80 | 1680 | No | 0 | Excellent | 2026-01-01 |
| network/nxos_config | 78 | 1769 | No | 0 | Excellent | 2025-12-27 |
| network/common | 75 | 861 | No | 0 | Excellent | 2025-12-27 |

---

## Windows Modules (5)

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| windows/win_package | 62 | 1067 | No | 0 | Good | 2025-12-27 |
| windows/win_service | 60 | 840 | No | 0 | Partial | 2025-12-27 |
| windows/win_user | 58 | 835 | No | 0 | Partial | 2025-12-27 |
| windows/win_feature | 55 | 560 | No | 0 | Partial | 2025-12-27 |
| windows/win_copy | 52 | 528 | No | 0 | Partial | 2025-12-27 |

---

## Database Modules (8) [DISABLED]

> **Note:** Database modules are currently disabled pending sqlx integration. See `src/modules/mod.rs:16-17`.

| Module | Health | Lines | Tests | Test Count | Docs Status | Last Updated |
|--------|--------|-------|-------|------------|-------------|--------------|
| database/postgresql_db | 68 | 1032 | No | 0 | Good | 2025-12-27 |
| database/postgresql_user | 65 | 857 | No | 0 | Good | 2025-12-27 |
| database/postgresql_query | 62 | 582 | No | 0 | Partial | 2025-12-27 |
| database/mysql_user | 62 | 797 | No | 0 | Good | 2025-12-27 |
| database/mysql_db | 60 | 582 | No | 0 | Partial | 2025-12-27 |
| database/mysql_query | 58 | 535 | No | 0 | Partial | 2025-12-27 |
| database/pool | 72 | 389 | No | 0 | Excellent | 2025-12-27 |
| database/mod | 60 | 331 | No | 0 | Good | 2025-12-27 |

---

## HPC Modules (49)

> **Note:** HPC modules were added in PRs #587-#635. Health scores are listed as **N/A** pending formal assessment. All modules include `//!` doc comments and support check mode.

### Core HPC (17 modules, feature: `hpc`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/common (hpc_baseline) | N/A | ~500 | No | Partial | 2026-01 |
| hpc/munge | N/A | ~400 | No | Partial | 2026-01 |
| hpc/nfs | N/A | ~600 | No | Partial | 2026-01 |
| hpc/healthcheck | N/A | ~450 | No | Partial | 2026-01 |
| hpc/facts | N/A | ~500 | No | Partial | 2026-01 |
| hpc/lmod | N/A | ~700 | No | Partial | 2026-02 |
| hpc/mpi | N/A | ~500 | No | Partial | 2026-01 |
| hpc/ipmi | N/A | ~500 | No | Partial | 2026-01 |
| hpc/power | N/A | ~400 | No | Partial | 2026-01 |
| hpc/toolchain | N/A | ~400 | No | Partial | 2026-01 |
| hpc/discovery | N/A | ~450 | No | Partial | 2026-01 |
| hpc/boot_profile | N/A | ~400 | No | Partial | 2026-01 |
| hpc/image_pipeline | N/A | ~450 | No | Partial | 2026-01 |
| hpc/scheduler | N/A | ~300 | No | Partial | 2026-01 |
| hpc/hpc_job | N/A | ~350 | No | Partial | 2026-01 |
| hpc/hpc_queue | N/A | ~350 | No | Partial | 2026-01 |
| hpc/hpc_server | N/A | ~350 | No | Partial | 2026-01 |

### Slurm Modules (12 modules, feature: `slurm`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/slurm (config+ops) | N/A | ~800 | No | Partial | 2026-01 |
| hpc/slurm_node | N/A | ~500 | No | Partial | 2026-01 |
| hpc/slurm_partition | N/A | ~500 | No | Partial | 2026-01 |
| hpc/slurm_account | N/A | ~600 | No | Partial | 2026-01 |
| hpc/slurm_job | N/A | ~500 | No | Partial | 2026-01 |
| hpc/slurm_queue | N/A | ~400 | No | Partial | 2026-01 |
| hpc/slurm_info | N/A | ~450 | No | Partial | 2026-01 |
| hpc/slurmrestd | N/A | ~500 | No | Partial | 2026-01 |
| hpc/scheduler_slurm | N/A | ~400 | No | Partial | 2026-01 |
| hpc/scheduler_orchestration | N/A | ~500 | No | Partial | 2026-01 |
| hpc/partition_policy | N/A | ~450 | No | Partial | 2026-01 |

### PBS Modules (4 modules, feature: `pbs`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/pbs_job | N/A | ~450 | No | Partial | 2026-01 |
| hpc/pbs_queue | N/A | ~450 | No | Partial | 2026-01 |
| hpc/pbs_server | N/A | ~400 | No | Partial | 2026-01 |
| hpc/scheduler_pbs | N/A | ~350 | No | Partial | 2026-01 |

### GPU Modules (3 modules, feature: `gpu`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/gpu (nvidia_gpu) | N/A | ~600 | No | Partial | 2026-01 |
| hpc/nvidia_driver | N/A | ~500 | No | Partial | 2026-01 |
| hpc/cuda | N/A | ~500 | No | Partial | 2026-02 |

### OFED / InfiniBand Modules (6 modules, feature: `ofed`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/ofed (rdma_stack) | N/A | ~600 | No | Partial | 2026-01 |
| hpc/opensm | N/A | ~500 | No | Partial | 2026-02 |
| hpc/ib_partition | N/A | ~500 | No | Partial | 2026-02 |
| hpc/ib_diagnostics | N/A | ~500 | No | Partial | 2026-02 |
| hpc/ib_validate | N/A | ~400 | No | Partial | 2026-01 |
| hpc/ipoib | N/A | ~450 | No | Partial | 2026-02 |

### Parallel Filesystem Modules (3 modules, feature: `parallel_fs`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/fs (lustre_client, beegfs_client) | N/A | ~800 | No | Partial | 2026-02 |
| hpc/lustre_mount | N/A | ~500 | No | Partial | 2026-01 |
| hpc/lustre_ost | N/A | ~500 | No | Partial | 2026-02 |

### Identity Modules (2 modules, feature: `identity`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/kerberos | N/A | ~500 | No | Partial | 2026-02 |
| hpc/sssd | N/A | ~600 | No | Partial | 2026-02 |

### Bare-Metal Provisioning Modules (2 modules, feature: `bare_metal`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/pxe | N/A | ~600 | No | Partial | 2026-02 |
| hpc/warewulf | N/A | ~600 | No | Partial | 2026-02 |

### Redfish/BMC Module (1 module, feature: `redfish`)

| Module | Health | Lines | Tests | Docs Status | Last Updated |
|--------|--------|-------|-------|-------------|--------------|
| hpc/redfish | N/A | ~500 | No | Partial | 2026-02 |

---

## Priority Improvements

### Critical (Score < 50)

| Module | Score | Issue |
|--------|-------|-------|
| set_fact | 45 | Missing tests, minimal docs, not recently updated |
| k8s/mod | 40 | Minimal stub file, no tests |
| cloud/aws/mod | 45 | Minimal stub file, no tests |
| cloud/gcp/mod | 45 | Minimal stub file, no tests |
| cloud/azure/mod | 45 | Minimal stub file, no tests |
| docker/mod | 50 | Minimal stub file, no tests |

### High Priority (Score 50-60)

| Module | Score | Improvement Needed |
|--------|-------|-------------------|
| template | 52 | Add tests, improve documentation |
| copy | 65 | Add tests (critical file module) |
| lineinfile | 58 | Add tests (frequently used) |
| blockinfile | 55 | Add tests |
| debug | 55 | Add tests |
| assert | 52 | Add tests |
| include_vars | 50 | Add tests |
| windows/win_copy | 52 | Add tests, improve docs |

### Medium Priority (Score 60-70)

| Module | Score | Improvement Needed |
|--------|-------|-------------------|
| file | 62 | Add tests |
| cron | 60 | Add tests |
| mount | 58 | Add tests |
| facts | 60 | Add tests |
| k8s/* modules | 62-65 | Add tests, improve docs |

---

## Test Coverage Summary

### Modules WITH Tests (23)

```
apt, archive, authorized_key, command, dnf, firewalld, git, group,
known_hosts, package, pause, pip, selinux, service, shell, stat,
systemd_unit, timezone, ufw, uri, user, wait_for, yum
```

### Modules WITHOUT Tests (102)

```
Core: assert, blockinfile, copy, cron, debug, facts, file, hostname,
      include_vars, lineinfile, mount, python, set_fact, sysctl, template,
      unarchive

Docker: docker_compose, docker_container, docker_image, docker_network,
        docker_volume, mod

Kubernetes: k8s_configmap, k8s_deployment, k8s_namespace, k8s_secret,
            k8s_service, cloud/kubernetes/*

Cloud: aws/*, gcp/*, azure/*

Network: common, eos_config, ios_config, junos_config, nxos_config

Windows: win_copy, win_feature, win_package, win_service, win_user

Database: mysql_*, postgresql_*, pool, mod

HPC: All 49 HPC modules (common, munge, nfs, healthcheck, facts, lmod, mpi,
     ipmi, power, toolchain, discovery, boot_profile, image_pipeline,
     scheduler, hpc_job, hpc_queue, hpc_server, slurm*, pbs_*, gpu,
     nvidia_driver, cuda, ofed, opensm, ib_partition, ib_diagnostics,
     ib_validate, ipoib, fs, lustre_mount, lustre_ost, kerberos, sssd,
     pxe, warewulf, redfish, partition_policy, scheduler_orchestration,
     slurmrestd)
```

---

## Health Score Calculation

Health scores are calculated based on:

| Factor | Weight | Criteria |
|--------|--------|----------|
| **Test Coverage** | 35% | Has dedicated test file with tests |
| **Documentation** | 25% | Doc comments on structs/functions |
| **Code Size** | 10% | Reasonable module size (<2000 lines) |
| **Recency** | 15% | Updated in last 30 days |
| **Complexity** | 15% | Appropriate for module type |

### Formula

```
score = (has_tests * 35) + (doc_status * 25) + (size_score * 10) +
        (recency_score * 15) + (complexity_score * 15)

where:
  has_tests: 1.0 if test file exists with tests, 0.0 otherwise
  doc_status: 1.0 (Excellent), 0.75 (Good), 0.5 (Partial), 0.25 (Missing)
  size_score: 1.0 if < 1500 lines, 0.8 if < 2000, 0.6 if >= 2000
  recency_score: 1.0 if updated in 7 days, 0.7 if 30 days, 0.5 otherwise
  complexity_score: Based on module type and implementation completeness
```

---

## Maintenance Guidelines

### Adding Tests
1. Create test file in `tests/modules/<module>_tests.rs`
2. Add module to `tests/modules/mod.rs`
3. Include success, failure, and edge case tests
4. Update this dashboard

### Improving Documentation
1. Add module-level `//!` documentation
2. Document all public structs with `///`
3. Include usage examples
4. Document all parameters

### Enabling Database Modules
1. Add `sqlx` dependency with appropriate feature flags
2. Uncomment `pub mod database` in `src/modules/mod.rs`
3. Add integration tests with test database
4. Update this dashboard

---

*Last updated: 2026-02-17*
