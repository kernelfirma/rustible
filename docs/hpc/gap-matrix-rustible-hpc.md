# Gap Matrix: Rustible vs HPC Requirements

Phase 4A of the HPC Initiative - Comprehensive gap analysis mapping HPC requirements to Rustible capabilities with severity, impact, and confidence ratings.

## Table of Contents

1. [Gap Matrix Summary](#1-gap-matrix-summary)
2. [Scheduler Management Gaps](#2-scheduler-management-gaps)
3. [Bare-Metal and Provisioning Gaps](#3-bare-metal-and-provisioning-gaps)
4. [High-Performance Fabric Gaps](#4-high-performance-fabric-gaps)
5. [Parallel Filesystem Gaps](#5-parallel-filesystem-gaps)
6. [GPU and Accelerator Gaps](#6-gpu-and-accelerator-gaps)
7. [Identity and Access Gaps](#7-identity-and-access-gaps)
8. [Software Stack Gaps](#8-software-stack-gaps)
9. [Scale and Performance Gaps](#9-scale-and-performance-gaps)
10. [Top Gaps by Severity](#10-top-gaps-by-severity)

---

## 1. Gap Matrix Summary

### 1.1 Severity Legend

| Severity | Definition | Action Required |
|----------|------------|-----------------|
| 🔴 **Critical** | Blocks HPC adoption; no workaround | Must implement |
| 🟠 **High** | Major limitation; workaround exists but poor | Should implement |
| 🟡 **Medium** | Notable gap; reasonable workaround | Consider implementing |
| 🟢 **Low** | Minor inconvenience; good workaround | Nice to have |

### 1.2 Impact Legend

| Impact | Definition |
|--------|------------|
| **Operational** | Affects day-to-day cluster operations |
| **Scale** | Limits cluster size or performance |
| **Reliability** | Affects uptime or recovery |
| **Security** | Affects security posture |
| **Usability** | Affects operator experience |

### 1.3 Overall Gap Summary

| Category | Critical | High | Medium | Low | Total |
|----------|----------|------|--------|-----|-------|
| Scheduler Management | 2 | 3 | 2 | 1 | 8 |
| Bare-Metal/Provisioning | 3 | 2 | 2 | 1 | 8 |
| High-Performance Fabric | 2 | 3 | 2 | 0 | 7 |
| Parallel Filesystems | 2 | 2 | 2 | 1 | 7 |
| GPU/Accelerator | 1 | 2 | 2 | 1 | 6 |
| Identity/Access | 0 | 2 | 2 | 1 | 5 |
| Software Stack | 0 | 2 | 3 | 2 | 7 |
| Scale/Performance | 1 | 2 | 2 | 1 | 6 |
| **Total** | **11** | **18** | **17** | **8** | **54** |

---

## 2. Scheduler Management Gaps

### 2.1 Gap Matrix

| ID | Requirement | Rustible Capability | Ansible Baseline | Gap | Severity | Impact | Confidence |
|----|-------------|---------------------|------------------|-----|----------|--------|------------|
| SCH-01 | Slurm node state management | ❌ None | ⚠️ Command-based | No native slurm_node module | 🔴 Critical | Operational | High |
| SCH-02 | Slurm partition configuration | ❌ None | ⚠️ Template only | No native slurm_partition module | 🔴 Critical | Operational | High |
| SCH-03 | Slurm accounting setup | ❌ None | ⚠️ Manual | No slurmdbd configuration module | 🟠 High | Operational | High |
| SCH-04 | PBS Pro queue management | ❌ None | ⚠️ Command-based | No PBS modules | 🟠 High | Operational | Medium |
| SCH-05 | LSF configuration | ❌ None | ⚠️ Template only | No LSF modules | 🟠 High | Operational | Medium |
| SCH-06 | Job-aware maintenance | ❌ None | ❌ None | No scheduler API integration | 🟡 Medium | Operational | High |
| SCH-07 | GRES auto-detection | ❌ None | ⚠️ Limited | No GPU resource detection for Slurm | 🟡 Medium | Usability | Medium |
| SCH-08 | Federation configuration | ❌ None | ⚠️ Manual | No multi-cluster federation support | 🟢 Low | Scale | Low |

### 2.2 Evidence References

| Gap ID | HPC Requirement Doc | Rustible Capability Doc | Baseline Doc |
|--------|---------------------|------------------------|--------------|
| SCH-01 | [scheduler-requirements-matrix.md](./scheduler-requirements-matrix.md) §1.2 | [modules-integrations-capabilities.md](../compatibility/modules-integrations-capabilities.md) - Not listed | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.1 |
| SCH-02 | [scheduler-requirements-matrix.md](./scheduler-requirements-matrix.md) §1.3 | Not implemented | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.1 |
| SCH-03 | [scheduler-requirements-matrix.md](./scheduler-requirements-matrix.md) §5 | Not implemented | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.1 |

### 2.3 Proposed Modules

| Module | Priority | Description |
|--------|----------|-------------|
| `slurm_node` | P0 | Manage node state (drain, resume, idle) |
| `slurm_partition` | P0 | Create/modify partitions |
| `slurm_account` | P1 | Accounting and fairshare |
| `slurm_qos` | P1 | Quality of Service policies |
| `pbs_queue` | P2 | PBS Pro queue management |
| `lsf_queue` | P2 | LSF queue configuration |

---

## 3. Bare-Metal and Provisioning Gaps

### 3.1 Gap Matrix

| ID | Requirement | Rustible Capability | Ansible Baseline | Gap | Severity | Impact | Confidence |
|----|-------------|---------------------|------------------|-----|----------|--------|------------|
| BM-01 | IPMI power control | ❌ None | ⚠️ Command module | No native IPMI module | 🔴 Critical | Operational | High |
| BM-02 | Redfish power/firmware | ❌ None | ✅ community.general.redfish_* | No Redfish modules | 🔴 Critical | Operational | High |
| BM-03 | PXE boot configuration | ⚠️ Template only | ⚠️ Template only | No PXE-specific modules | 🔴 Critical | Operational | High |
| BM-04 | Warewulf integration | ❌ None | ❌ None | No provisioning tool integration | 🟠 High | Operational | High |
| BM-05 | Node discovery | ❌ None | ⚠️ Limited | No hardware discovery module | 🟠 High | Usability | Medium |
| BM-06 | BMC user management | ❌ None | ⚠️ Command-based | No BMC configuration module | 🟡 Medium | Security | Medium |
| BM-07 | Firmware orchestration | ❌ None | ⚠️ Vendor-specific | No coordinated firmware updates | 🟡 Medium | Reliability | Medium |
| BM-08 | Hardware inventory | ⚠️ Facts only | ⚠️ Facts only | Limited HPC hardware details | 🟢 Low | Usability | Low |

### 3.2 Evidence References

| Gap ID | HPC Requirement Doc | Rustible Capability Doc | Baseline Doc |
|--------|---------------------|------------------------|--------------|
| BM-01 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §2.2 | Not implemented | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.2 |
| BM-02 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §2.1 | Not implemented | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.2 |
| BM-03 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §1.2 | Template module exists | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.1 |

### 3.3 Proposed Modules

| Module | Priority | Description |
|--------|----------|-------------|
| `ipmi_power` | P0 | Power on/off/cycle/status |
| `ipmi_boot` | P0 | Boot device selection |
| `redfish_power` | P0 | Redfish power management |
| `redfish_info` | P0 | Redfish inventory/status |
| `redfish_firmware` | P1 | Firmware updates via Redfish |
| `bmc_user` | P1 | BMC user management |
| `pxe_host` | P1 | PXE boot configuration |
| `warewulf_node` | P2 | Warewulf provisioning integration |

---

## 4. High-Performance Fabric Gaps

### 4.1 Gap Matrix

| ID | Requirement | Rustible Capability | Ansible Baseline | Gap | Severity | Impact | Confidence |
|----|-------------|---------------------|------------------|-----|----------|--------|------------|
| IB-01 | OpenSM configuration | ❌ None | ⚠️ Template only | No subnet manager module | 🔴 Critical | Operational | High |
| IB-02 | IB partition configuration | ❌ None | ⚠️ Template only | No partition key management | 🔴 Critical | Security | High |
| IB-03 | IPoIB interface setup | ⚠️ Generic network | ⚠️ Template | No IPoIB-specific module | 🟠 High | Operational | High |
| IB-04 | InfiniBand driver install | ⚠️ Package module | ⚠️ Package module | No OFED-specific handling | 🟠 High | Operational | Medium |
| IB-05 | Fabric diagnostics | ❌ None | ⚠️ Command-based | No ibdiagnet/iblinkinfo integration | 🟠 High | Reliability | High |
| IB-06 | HCA firmware updates | ❌ None | ⚠️ Command-based | No firmware orchestration | 🟡 Medium | Reliability | Medium |
| IB-07 | RDMA memory configuration | ⚠️ sysctl module | ⚠️ sysctl module | No RDMA-specific validation | 🟡 Medium | Operational | Low |

### 4.2 Evidence References

| Gap ID | HPC Requirement Doc | Rustible Capability Doc | Baseline Doc |
|--------|---------------------|------------------------|--------------|
| IB-01 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §3.2-3.3 | Not implemented | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.2 |
| IB-02 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §3.3 | Not implemented | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.2 |
| IB-03 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §3.5 | Generic network only | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.2 |

### 4.3 Proposed Modules

| Module | Priority | Description |
|--------|----------|-------------|
| `opensm_config` | P0 | OpenSM subnet manager configuration |
| `ib_partition` | P0 | InfiniBand partition management |
| `ipoib` | P1 | IPoIB interface configuration |
| `ib_info` | P1 | Fabric information and diagnostics |
| `mlnx_ofed` | P2 | MLNX_OFED driver management |
| `ib_health` | P2 | Fabric health checks |

---

## 5. Parallel Filesystem Gaps

### 5.1 Gap Matrix

| ID | Requirement | Rustible Capability | Ansible Baseline | Gap | Severity | Impact | Confidence |
|----|-------------|---------------------|------------------|-----|----------|--------|------------|
| FS-01 | Lustre client mount | ⚠️ Generic mount | ⚠️ Generic mount | No Lustre-specific handling | 🔴 Critical | Operational | High |
| FS-02 | Lustre OST management | ❌ None | ⚠️ Command-based | No OST lifecycle module | 🔴 Critical | Operational | High |
| FS-03 | BeeGFS client setup | ⚠️ Generic mount | ⚠️ Package + mount | No BeeGFS-specific module | 🟠 High | Operational | High |
| FS-04 | BeeGFS target management | ❌ None | ⚠️ Command-based | No storage target module | 🟠 High | Operational | Medium |
| FS-05 | Lustre quota management | ❌ None | ⚠️ Command-based | No quota module | 🟡 Medium | Operational | Medium |
| FS-06 | GPFS client setup | ⚠️ Generic mount | ⚠️ Limited | No GPFS-specific module | 🟡 Medium | Operational | Medium |
| FS-07 | Filesystem health checks | ❌ None | ⚠️ Command-based | No parallel FS health module | 🟢 Low | Reliability | Medium |

### 5.2 Evidence References

| Gap ID | HPC Requirement Doc | Rustible Capability Doc | Baseline Doc |
|--------|---------------------|------------------------|--------------|
| FS-01 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §4.2-4.4 | [modules-integrations-capabilities.md](../compatibility/modules-integrations-capabilities.md) §1.5 mount | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.3 |
| FS-02 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §4.4 | Not implemented | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.3 |
| FS-03 | [bare-metal-fabric-storage-requirements.md](./bare-metal-fabric-storage-requirements.md) §4.5-4.6 | Generic mount | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.3 |

### 5.3 Proposed Modules

| Module | Priority | Description |
|--------|----------|-------------|
| `lustre_mount` | P0 | Lustre client mount with LNet config |
| `lustre_ost` | P1 | OST lifecycle management |
| `lustre_quota` | P2 | User/group quotas |
| `beegfs_mount` | P1 | BeeGFS client configuration |
| `beegfs_target` | P2 | Storage target management |
| `gpfs_mount` | P2 | GPFS/Spectrum Scale client |

---

## 6. GPU and Accelerator Gaps

### 6.1 Gap Matrix

| ID | Requirement | Rustible Capability | Ansible Baseline | Gap | Severity | Impact | Confidence |
|----|-------------|---------------------|------------------|-----|----------|--------|------------|
| GPU-01 | NVIDIA driver installation | ⚠️ Package module | ✅ NVIDIA role | No driver-specific module | 🔴 Critical | Operational | High |
| GPU-02 | CUDA toolkit multi-version | ⚠️ Package module | ⚠️ Manual | No CUDA version management | 🟠 High | Operational | High |
| GPU-03 | nvidia-persistenced | ⚠️ Service module | ⚠️ Service module | No GPU-specific validation | 🟠 High | Reliability | Medium |
| GPU-04 | DCGM monitoring setup | ⚠️ Package + service | ⚠️ Package + service | No DCGM-specific module | 🟡 Medium | Operational | Medium |
| GPU-05 | GPU health validation | ❌ None | ⚠️ Command-based | No nvidia-smi integration | 🟡 Medium | Reliability | High |
| GPU-06 | ROCm stack (AMD) | ⚠️ Package module | ⚠️ Limited | No AMD GPU support | 🟢 Low | Operational | Low |

### 6.2 Evidence References

| Gap ID | HPC Requirement Doc | Rustible Capability Doc | Baseline Doc |
|--------|---------------------|------------------------|--------------|
| GPU-01 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §1.2-1.3 | [modules-integrations-capabilities.md](../compatibility/modules-integrations-capabilities.md) §1.2 package | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.4 |
| GPU-02 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §1.3-1.4 | Generic package | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.4 |
| GPU-03 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §1.6 | Service module | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.4 |

### 6.3 Proposed Modules

| Module | Priority | Description |
|--------|----------|-------------|
| `nvidia_driver` | P0 | Driver installation with version control |
| `cuda_toolkit` | P1 | CUDA toolkit with multi-version support |
| `nvidia_persistenced` | P1 | Persistence daemon configuration |
| `nvidia_info` | P1 | GPU detection and health checks |
| `dcgm` | P2 | Data Center GPU Manager setup |
| `rocm` | P3 | AMD ROCm stack |

---

## 7. Identity and Access Gaps

### 7.1 Gap Matrix

| ID | Requirement | Rustible Capability | Ansible Baseline | Gap | Severity | Impact | Confidence |
|----|-------------|---------------------|------------------|-----|----------|--------|------------|
| ID-01 | SSSD configuration | ⚠️ Template only | ✅ RHEL system roles | No native SSSD module | 🟠 High | Security | High |
| ID-02 | Kerberos client setup | ⚠️ Template only | ✅ Good | No krb5.conf module | 🟠 High | Security | High |
| ID-03 | LDAP client | ⚠️ Template only | ✅ Good | No LDAP-specific module | 🟡 Medium | Security | Medium |
| ID-04 | PAM configuration | ⚠️ Template only | ⚠️ Moderate | No PAM module | 🟡 Medium | Security | Medium |
| ID-05 | SSH key distribution | ✅ authorized_key | ✅ authorized_key | None - parity | 🟢 Low | Security | High |

### 7.2 Evidence References

| Gap ID | HPC Requirement Doc | Rustible Capability Doc | Baseline Doc |
|--------|---------------------|------------------------|--------------|
| ID-01 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §4.4-4.5 | Template only | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.7 |
| ID-02 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §4.3 | Template only | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.7 |
| ID-03 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §4.2 | Template only | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.7 |

### 7.3 Proposed Modules

| Module | Priority | Description |
|--------|----------|-------------|
| `sssd_config` | P1 | SSSD configuration management |
| `sssd_domain` | P1 | SSSD domain configuration |
| `krb5_config` | P1 | Kerberos client configuration |
| `ldap_config` | P2 | LDAP client setup |
| `pam_config` | P2 | PAM module configuration |

---

## 8. Software Stack Gaps

### 8.1 Gap Matrix

| ID | Requirement | Rustible Capability | Ansible Baseline | Gap | Severity | Impact | Confidence |
|----|-------------|---------------------|------------------|-----|----------|--------|------------|
| SW-01 | Lmod installation | ⚠️ Package module | ⚠️ Package module | No Lmod-specific module | 🟠 High | Usability | Medium |
| SW-02 | Module path management | ⚠️ Template | ⚠️ Template | No module hierarchy support | 🟠 High | Usability | Medium |
| SW-03 | Spack configuration | ❌ None | ⚠️ Moderate | No Spack integration | 🟡 Medium | Usability | Medium |
| SW-04 | EasyBuild setup | ❌ None | ⚠️ Moderate | No EasyBuild integration | 🟡 Medium | Usability | Low |
| SW-05 | MPI stack installation | ⚠️ Package module | ⚠️ Package module | No MPI-specific module | 🟡 Medium | Operational | Medium |
| SW-06 | FlexLM server setup | ❌ None | ⚠️ Command-based | No license server module | 🟢 Low | Operational | Medium |
| SW-07 | License resource tracking | ❌ None | ❌ None | No scheduler license integration | 🟢 Low | Operational | Low |

### 8.2 Evidence References

| Gap ID | HPC Requirement Doc | Rustible Capability Doc | Baseline Doc |
|--------|---------------------|------------------------|--------------|
| SW-01 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §3.2 | Generic package | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.6 |
| SW-02 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §3.3 | Template only | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.6 |
| SW-03 | [software-stack-identity-requirements.md](./software-stack-identity-requirements.md) §3.4 | Not implemented | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §2.6 |

### 8.3 Proposed Modules

| Module | Priority | Description |
|--------|----------|-------------|
| `lmod` | P2 | Lmod installation and configuration |
| `modulepath` | P2 | Module path hierarchy management |
| `spack_config` | P3 | Spack installation and setup |
| `easybuild_config` | P3 | EasyBuild configuration |
| `mpi_stack` | P2 | MPI installation with fabric selection |
| `flexlm_server` | P3 | FlexLM license server |

---

## 9. Scale and Performance Gaps

### 9.1 Gap Matrix

| ID | Requirement | Rustible Capability | Ansible Baseline | Gap | Severity | Impact | Confidence |
|----|-------------|---------------------|------------------|-----|----------|--------|------------|
| SC-01 | 10,000+ node execution | ⚠️ Untested | ❌ Poor (6+ hours) | No validated large-scale testing | 🔴 Critical | Scale | Medium |
| SC-02 | Adaptive parallelism | ✅ Work-stealing | ❌ Fixed forks | None - better than Ansible | 🟢 Low | Scale | High |
| SC-03 | Connection pooling | ✅ HostPinned strategy | ⚠️ Limited | None - parity or better | 🟢 Low | Scale | High |
| SC-04 | Checkpoint at scale | ✅ Checkpoint system | ❌ None | None - better than Ansible | 🟢 Low | Reliability | High |
| SC-05 | State file performance | ⚠️ Untested | N/A | Unknown performance at scale | 🟡 Medium | Scale | Low |
| SC-06 | Memory efficiency | ⚠️ Untested | ❌ Poor | Unknown memory profile at scale | 🟡 Medium | Scale | Low |
| SC-07 | Inventory loading | ⚠️ Untested | ⚠️ Slow at scale | Unknown large inventory perf | 🟠 High | Scale | Medium |
| SC-08 | Terraform state reading | ✅ Implemented | N/A | None - unique capability | 🟢 Low | Usability | High |

### 9.2 Evidence References

| Gap ID | HPC Requirement Doc | Rustible Capability Doc | Baseline Doc |
|--------|---------------------|------------------------|--------------|
| SC-01 | [scale-bands-slo-requirements.md](./scale-bands-slo-requirements.md) §1.1 | [execution-reliability-capabilities.md](../compatibility/execution-reliability-capabilities.md) §2 | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §7.1 |
| SC-02 | [scale-bands-slo-requirements.md](./scale-bands-slo-requirements.md) §2 | [execution-reliability-capabilities.md](../compatibility/execution-reliability-capabilities.md) §2.2 | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §7.1 |
| SC-07 | [scale-bands-slo-requirements.md](./scale-bands-slo-requirements.md) §2 | Untested | [ansible-baseline-hpc-operations.md](./ansible-baseline-hpc-operations.md) §7.1 |

### 9.3 Required Validation

| Test | Scale | Target | Current Status |
|------|-------|--------|----------------|
| Basic playbook | 100 nodes | < 5 min | Untested |
| Complex playbook | 1,000 nodes | < 30 min | Untested |
| Full deployment | 10,000 nodes | < 4 hours | Untested |
| Memory usage | 10,000 nodes | < 8 GB | Untested |
| Checkpoint restore | 1,000 nodes | < 2 min | Untested |

---

## 10. Top Gaps by Severity

### 10.1 Critical Gaps (Must Implement)

| Rank | Gap ID | Description | Impact | Effort |
|------|--------|-------------|--------|--------|
| 1 | SCH-01 | Slurm node state management | Operational | Medium |
| 2 | SCH-02 | Slurm partition configuration | Operational | Medium |
| 3 | BM-01 | IPMI power control | Operational | Low |
| 4 | BM-02 | Redfish power/firmware | Operational | Medium |
| 5 | BM-03 | PXE boot configuration | Operational | Medium |
| 6 | IB-01 | OpenSM configuration | Operational | Medium |
| 7 | IB-02 | IB partition configuration | Security | Medium |
| 8 | FS-01 | Lustre client mount | Operational | Low |
| 9 | FS-02 | Lustre OST management | Operational | High |
| 10 | GPU-01 | NVIDIA driver installation | Operational | Medium |
| 11 | SC-01 | Large-scale validation | Scale | High |

### 10.2 High Priority Gaps (Should Implement)

| Rank | Gap ID | Description | Impact | Effort |
|------|--------|-------------|--------|--------|
| 1 | SCH-03 | Slurm accounting setup | Operational | Medium |
| 2 | SCH-04 | PBS Pro queue management | Operational | Medium |
| 3 | SCH-05 | LSF configuration | Operational | Medium |
| 4 | BM-04 | Warewulf integration | Operational | High |
| 5 | BM-05 | Node discovery | Usability | Medium |
| 6 | IB-03 | IPoIB interface setup | Operational | Low |
| 7 | IB-04 | InfiniBand driver install | Operational | Low |
| 8 | IB-05 | Fabric diagnostics | Reliability | Medium |
| 9 | FS-03 | BeeGFS client setup | Operational | Low |
| 10 | FS-04 | BeeGFS target management | Operational | Medium |
| 11 | GPU-02 | CUDA toolkit multi-version | Operational | Medium |
| 12 | GPU-03 | nvidia-persistenced | Reliability | Low |
| 13 | ID-01 | SSSD configuration | Security | Medium |
| 14 | ID-02 | Kerberos client setup | Security | Medium |
| 15 | SW-01 | Lmod installation | Usability | Low |
| 16 | SW-02 | Module path management | Usability | Low |
| 17 | SC-07 | Inventory loading at scale | Scale | Medium |

### 10.3 Implementation Roadmap Summary

| Phase | Focus | Gaps Addressed | Effort |
|-------|-------|----------------|--------|
| **Phase 1** | Scheduler basics | SCH-01, SCH-02 | 2-3 weeks |
| **Phase 2** | Bare-metal control | BM-01, BM-02, BM-03 | 2-3 weeks |
| **Phase 3** | InfiniBand fabric | IB-01, IB-02, IB-03 | 2-3 weeks |
| **Phase 4** | Storage | FS-01, FS-02, FS-03 | 2-3 weeks |
| **Phase 5** | GPU stack | GPU-01, GPU-02, GPU-03 | 2 weeks |
| **Phase 6** | Identity | ID-01, ID-02 | 2 weeks |
| **Phase 7** | Software stack | SW-01, SW-02 | 1-2 weeks |
| **Phase 8** | Scale validation | SC-01, SC-07 | 2-4 weeks |

---

## Appendix: Gap Evidence Index

| Gap ID | Requirement Source | Capability Source | Baseline Source |
|--------|-------------------|-------------------|-----------------|
| SCH-* | scheduler-requirements-matrix.md | modules-integrations-capabilities.md | ansible-baseline-hpc-operations.md |
| BM-* | bare-metal-fabric-storage-requirements.md | modules-integrations-capabilities.md | ansible-baseline-hpc-operations.md |
| IB-* | bare-metal-fabric-storage-requirements.md | modules-integrations-capabilities.md | ansible-baseline-hpc-operations.md |
| FS-* | bare-metal-fabric-storage-requirements.md | modules-integrations-capabilities.md | ansible-baseline-hpc-operations.md |
| GPU-* | software-stack-identity-requirements.md | modules-integrations-capabilities.md | ansible-baseline-hpc-operations.md |
| ID-* | software-stack-identity-requirements.md | modules-integrations-capabilities.md | ansible-baseline-hpc-operations.md |
| SW-* | software-stack-identity-requirements.md | modules-integrations-capabilities.md | ansible-baseline-hpc-operations.md |
| SC-* | scale-bands-slo-requirements.md | execution-reliability-capabilities.md | ansible-baseline-hpc-operations.md |
