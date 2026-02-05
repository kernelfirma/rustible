# HPC Scheduler Requirements Matrix

> **Last Updated:** 2026-02-05
> **Rustible Version:** 0.1.x
> **HPC Initiative Phase:** 2A - Scheduler-Specific Requirements

This document defines automation requirements for major HPC job schedulers: Slurm, PBS Pro/OpenPBS, IBM Spectrum LSF, and Grid Engine (SGE).

---

## Quick Reference

| Scheduler | Market Share | Open Source | Current Version | Primary Use |
|-----------|-------------|-------------|-----------------|-------------|
| **Slurm** | ~60% | Yes (GPL) | 24.x | Large-scale HPC, Cloud |
| **PBS Pro/OpenPBS** | ~15% | Yes (AGPLv3) | 23.x | Enterprise HPC, Cloud |
| **IBM Spectrum LSF** | ~15% | No (Commercial) | 10.x | Enterprise, Finance |
| **Grid Engine (SGE)** | ~10% | Yes (Various) | 8.x | Legacy, Academic |

---

## 1. Slurm Workload Manager

### 1.1 Overview

Slurm (Simple Linux Utility for Resource Management) is the dominant open-source HPC scheduler, used by 6 of the top 10 supercomputers worldwide.

**Key Resources:**
- [SchedMD Official Documentation](https://slurm.schedmd.com/)
- [stackhpc/ansible-slurm-appliance](https://github.com/stackhpc/ansible-slurm-appliance)
- [galaxyproject/ansible-slurm](https://github.com/galaxyproject/ansible-slurm)

### 1.2 Configuration Files

| File | Purpose | Automation Priority |
|------|---------|---------------------|
| `slurm.conf` | Main configuration | **Critical** |
| `slurmdbd.conf` | Database daemon config | **Critical** |
| `gres.conf` | Generic resources (GPUs, etc.) | **High** |
| `cgroup.conf` | Linux cgroups settings | **High** |
| `topology.conf` | Network topology | Medium |
| `job_container.conf` | Job isolation | Medium |
| `acct_gather.conf` | Accounting/profiling | Medium |
| `helpers.conf` | Node feature helpers | Low |
| `oci.conf` | Container runtime | Low |

### 1.3 Requirements Matrix

#### 1.3.1 Installation & Deployment

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SLURM-INST-01** | Install slurmctld (controller) | Package installed, service enabled |
| **SLURM-INST-02** | Install slurmd (compute daemon) | Package installed, service enabled |
| **SLURM-INST-03** | Install slurmdbd (database daemon) | Package installed, service enabled |
| **SLURM-INST-04** | Install client tools | srun, sbatch, squeue available |
| **SLURM-INST-05** | Install from OpenHPC repos | OpenHPC repos configured, packages installed |
| **SLURM-INST-06** | Build from source | Custom build with specified options |
| **SLURM-INST-07** | Configure Munge authentication | Munge key distributed, service running |

#### 1.3.2 Configuration Management

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SLURM-CONF-01** | Generate slurm.conf | Valid config, passes `slurmd -C` check |
| **SLURM-CONF-02** | Configure partitions | Partitions defined with correct nodes |
| **SLURM-CONF-03** | Configure QOS | Quality of Service levels defined |
| **SLURM-CONF-04** | Configure GRES (GPUs) | GPU resources detected and allocated |
| **SLURM-CONF-05** | Configure cgroups | Resource isolation working |
| **SLURM-CONF-06** | Configure topology | Network topology for job placement |
| **SLURM-CONF-07** | Configure accounting | slurmdbd connected, data collecting |
| **SLURM-CONF-08** | Configure fair-share | Fair-share scheduling active |
| **SLURM-CONF-09** | Configure preemption | Preemption policies enforced |
| **SLURM-CONF-10** | Configure node features | Dynamic/static features assigned |

#### 1.3.3 High Availability

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SLURM-HA-01** | Controller failover | Backup slurmctld takes over <30s |
| **SLURM-HA-02** | Database replication | slurmdbd master/slave working |
| **SLURM-HA-03** | State preservation | Jobs survive controller restart |
| **SLURM-HA-04** | Shared state directory | StateSaveLocation on shared storage |

#### 1.3.4 Accounting & Quotas

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SLURM-ACCT-01** | Create accounts | sacctmgr add account works |
| **SLURM-ACCT-02** | Create users | sacctmgr add user works |
| **SLURM-ACCT-03** | Set account limits | GrpTRES, MaxJobs, etc. enforced |
| **SLURM-ACCT-04** | Configure fair-share | FairShare values calculated |
| **SLURM-ACCT-05** | Generate reports | sacct queries return data |
| **SLURM-ACCT-06** | Archive accounting data | Data archived per policy |

#### 1.3.5 Node Lifecycle

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SLURM-NODE-01** | Add compute node | Node appears in sinfo |
| **SLURM-NODE-02** | Remove compute node | Node gracefully drained, removed |
| **SLURM-NODE-03** | Drain node | Jobs complete, no new jobs |
| **SLURM-NODE-04** | Resume node | Node accepts jobs again |
| **SLURM-NODE-05** | Set node state | idle, down, drain states work |
| **SLURM-NODE-06** | Update node features | Features updated without restart |
| **SLURM-NODE-07** | Health check integration | Unhealthy nodes auto-drained |

#### 1.3.6 Upgrades & Maintenance

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SLURM-UPG-01** | Rolling upgrade (minor) | No job loss during upgrade |
| **SLURM-UPG-02** | Rolling upgrade (major) | Documented downtime, state migrated |
| **SLURM-UPG-03** | Database schema migration | slurmdbd upgrades schema |
| **SLURM-UPG-04** | Configuration reload | scontrol reconfigure works |
| **SLURM-UPG-05** | Backup configuration | Configs backed up before changes |

---

## 2. PBS Pro / OpenPBS

### 2.1 Overview

PBS Professional (and its open-source variant OpenPBS) is an enterprise-grade HPC workload manager with extensive customization through hooks.

**Key Resources:**
- [OpenPBS Official Site](https://www.openpbs.org/)
- [Altair PBS Professional](https://altair.com/pbs-professional)
- [Azure CycleCloud OpenPBS Integration](https://learn.microsoft.com/en-us/azure/cyclecloud/openpbs)

### 2.2 Configuration Files

| File | Purpose | Automation Priority |
|------|---------|---------------------|
| `pbs.conf` | Main PBS configuration | **Critical** |
| `resourcedef` | Custom resource definitions | **High** |
| `sched_config` | Scheduler configuration | **High** |
| `mom_priv/config` | Compute node config | **High** |
| `holidays` | Holiday calendar | Medium |
| `resource_group` | Resource groups | Medium |
| Hooks (Python) | Custom logic | Medium |

### 2.3 Requirements Matrix

#### 2.3.1 Installation & Deployment

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **PBS-INST-01** | Install pbs_server | Server daemon running |
| **PBS-INST-02** | Install pbs_mom (compute) | MOM daemon on all nodes |
| **PBS-INST-03** | Install pbs_sched | Scheduler daemon running |
| **PBS-INST-04** | Install pbs_comm | Communication daemon running |
| **PBS-INST-05** | Install client tools | qsub, qstat, qdel available |
| **PBS-INST-06** | Configure PBS datastore | PostgreSQL backend configured |

#### 2.3.2 Configuration Management

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **PBS-CONF-01** | Configure queues | Queues created and configured |
| **PBS-CONF-02** | Configure resources | Custom resources defined |
| **PBS-CONF-03** | Configure nodes (vnodes) | Vnodes defined with resources |
| **PBS-CONF-04** | Configure hooks | Event hooks installed and active |
| **PBS-CONF-05** | Configure limits | Queue/server limits enforced |
| **PBS-CONF-06** | Configure scheduling policy | Scheduling parameters set |
| **PBS-CONF-07** | Configure node grouping | Placement sets configured |
| **PBS-CONF-08** | Configure reservations | Advance reservations working |

#### 2.3.3 High Availability

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **PBS-HA-01** | Failover configuration | Secondary server ready |
| **PBS-HA-02** | Database replication | PostgreSQL HA configured |
| **PBS-HA-03** | Shared home directory | Jobs survive failover |

#### 2.3.4 Accounting & Quotas

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **PBS-ACCT-01** | Enable accounting | Accounting records generated |
| **PBS-ACCT-02** | Configure fairshare | Fairshare tree configured |
| **PBS-ACCT-03** | Set user/group limits | Limits enforced per entity |
| **PBS-ACCT-04** | Generate reports | pbsnodes, qstat queries work |
| **PBS-ACCT-05** | Archive accounting | Data retention policy enforced |

#### 2.3.5 Node Lifecycle

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **PBS-NODE-01** | Add compute node | Node visible in pbsnodes |
| **PBS-NODE-02** | Remove compute node | Node cleanly removed |
| **PBS-NODE-03** | Offline node | Node marked offline |
| **PBS-NODE-04** | Online node | Node accepts jobs |
| **PBS-NODE-05** | Update node resources | Resources updated dynamically |

---

## 3. IBM Spectrum LSF

### 3.1 Overview

IBM Spectrum LSF is a commercial enterprise HPC workload manager with advanced scheduling capabilities and cloud integration.

**Key Resources:**
- [IBM Spectrum LSF](https://www.ibm.com/products/hpc-workload-management)
- [IBM Cloud LSF Deployment](https://cloud.ibm.com/catalog/content/ibm-spectrum-lsf)
- [AWS LSF Deployment Guide](https://aws.amazon.com/blogs/apn/scheduling-on-the-aws-cloud-with-ibm-spectrum-lsf-and-ibm-spectrum-symphony/)

### 3.2 Configuration Files

| File | Purpose | Automation Priority |
|------|---------|---------------------|
| `lsf.conf` | Main LSF configuration | **Critical** |
| `lsf.cluster.*` | Cluster configuration | **Critical** |
| `lsb.queues` | Queue definitions | **High** |
| `lsb.hosts` | Host definitions | **High** |
| `lsb.users` | User/group config | **High** |
| `lsb.resources` | Resource definitions | Medium |
| `lsb.params` | Scheduling parameters | Medium |
| `lsf.shared` | Shared configuration | Medium |

### 3.3 Requirements Matrix

#### 3.3.1 Installation & Deployment

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **LSF-INST-01** | Install LSF master | Master host configured |
| **LSF-INST-02** | Install LSF server hosts | Server hosts registered |
| **LSF-INST-03** | Install LSF clients | Client commands available |
| **LSF-INST-04** | License configuration | License server connected |
| **LSF-INST-05** | Cluster registration | Cluster visible in lsclusters |

#### 3.3.2 Configuration Management

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **LSF-CONF-01** | Configure queues | bqueues shows configured queues |
| **LSF-CONF-02** | Configure hosts | bhosts shows all hosts |
| **LSF-CONF-03** | Configure users/groups | busers shows configured entities |
| **LSF-CONF-04** | Configure resources | lsinfo shows resources |
| **LSF-CONF-05** | Configure limits | Limits enforced per queue/user |
| **LSF-CONF-06** | Configure preemption | Preemption policies work |
| **LSF-CONF-07** | Configure GPU resources | GPU allocation working |
| **LSF-CONF-08** | Configure reservations | Advance reservations work |

#### 3.3.3 High Availability

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **LSF-HA-01** | Master failover | Failover <60s to backup |
| **LSF-HA-02** | EGO service HA | EGO services highly available |
| **LSF-HA-03** | Shared file system | LSF_SHAREDIR on shared storage |

#### 3.3.4 Accounting & Quotas

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **LSF-ACCT-01** | Enable accounting | lsb.acct records generated |
| **LSF-ACCT-02** | Configure fairshare | Fairshare policies active |
| **LSF-ACCT-03** | Set user limits | Limits per user/group enforced |
| **LSF-ACCT-04** | Configure chargebacks | Resource usage tracked |
| **LSF-ACCT-05** | Generate reports | bacct reports work |

#### 3.3.5 Cloud Integration

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **LSF-CLOUD-01** | AWS resource connector | EC2 instances auto-provisioned |
| **LSF-CLOUD-02** | Azure resource connector | Azure VMs auto-provisioned |
| **LSF-CLOUD-03** | GCP resource connector | GCE instances auto-provisioned |
| **LSF-CLOUD-04** | Auto-scaling policies | Scale up/down based on demand |
| **LSF-CLOUD-05** | Data staging | Data staged to/from cloud |

---

## 4. Grid Engine (SGE/OGS/UGE)

### 4.1 Overview

Grid Engine encompasses several variants: Sun Grid Engine (SGE), Open Grid Scheduler (OGS), Univa Grid Engine (UGE), and the newer Gridware Cluster Scheduler (GCS).

**Key Resources:**
- [Open Grid Scheduler](https://gridscheduler.sourceforge.net/)
- [Gridware Cluster Scheduler](https://github.com/hpc-gridware/clusterscheduler)
- [SGE Documentation](https://gridscheduler.sourceforge.net/howto/GridEngineHowto.html)

### 4.2 Configuration Files

| File | Purpose | Automation Priority |
|------|---------|---------------------|
| `sge_conf` | Global configuration | **Critical** |
| `sge_host` | Host configuration | **High** |
| `sge_queue` | Queue configuration | **High** |
| `sge_pe` | Parallel environments | **High** |
| `sge_complex` | Complex attributes | Medium |
| `sge_ckpt` | Checkpointing config | Medium |
| `sge_user` | User configuration | Medium |

### 4.3 Requirements Matrix

#### 4.3.1 Installation & Deployment

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SGE-INST-01** | Install qmaster | Master daemon running |
| **SGE-INST-02** | Install execd | Execution daemons on nodes |
| **SGE-INST-03** | Install shadow master | HA shadow configured |
| **SGE-INST-04** | Install submit hosts | Client commands available |
| **SGE-INST-05** | Configure cells | Multi-cell if needed |

#### 4.3.2 Configuration Management

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SGE-CONF-01** | Configure queues | qconf -sql shows queues |
| **SGE-CONF-02** | Configure hosts | qconf -sel shows exec hosts |
| **SGE-CONF-03** | Configure parallel env | PEs defined for MPI |
| **SGE-CONF-04** | Configure complexes | Resources defined |
| **SGE-CONF-05** | Configure user mapping | User/project quotas set |
| **SGE-CONF-06** | Configure checkpointing | CKPT environments defined |
| **SGE-CONF-07** | Configure scheduler | Scheduling algorithm tuned |

#### 4.3.3 High Availability

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SGE-HA-01** | Shadow master | Automatic failover works |
| **SGE-HA-02** | Shared spool directory | State preserved across restarts |

#### 4.3.4 Accounting & Quotas

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **SGE-ACCT-01** | Enable accounting | qacct returns data |
| **SGE-ACCT-02** | Configure resource quotas | RQS enforced |
| **SGE-ACCT-03** | Configure projects | Project limits work |
| **SGE-ACCT-04** | Configure share tree | Fairshare scheduling active |

---

## 5. Cross-Scheduler Requirements

### 5.1 Common Automation Workflows

| Workflow | Slurm | PBS | LSF | SGE |
|----------|-------|-----|-----|-----|
| Install controller | ✓ | ✓ | ✓ | ✓ |
| Install compute agent | ✓ | ✓ | ✓ | ✓ |
| Configure queues/partitions | ✓ | ✓ | ✓ | ✓ |
| Add/remove nodes | ✓ | ✓ | ✓ | ✓ |
| Configure GPU resources | ✓ | ✓ | ✓ | ✓ |
| Set user/group quotas | ✓ | ✓ | ✓ | ✓ |
| Configure fairshare | ✓ | ✓ | ✓ | ✓ |
| High availability | ✓ | ✓ | ✓ | ✓ |
| Rolling upgrades | ✓ | ✓ | ✓ | ✓ |
| Accounting/reporting | ✓ | ✓ | ✓ | ✓ |

### 5.2 Scheduler-Agnostic Module Requirements

| Module | Purpose | Priority |
|--------|---------|----------|
| `hpc_scheduler_install` | Install any scheduler | **Critical** |
| `hpc_queue` | Manage queues/partitions | **Critical** |
| `hpc_node` | Manage compute nodes | **Critical** |
| `hpc_user` | Manage scheduler users | **High** |
| `hpc_account` | Manage accounts/projects | **High** |
| `hpc_limits` | Configure quotas/limits | **High** |
| `hpc_fairshare` | Configure fairshare | Medium |
| `hpc_reservation` | Manage reservations | Medium |
| `hpc_job` | Submit/manage jobs | Medium |

---

## 6. Rustible Implementation Priorities

### 6.1 Phase 1: Core Scheduler Support

| Priority | Scheduler | Modules Needed |
|----------|-----------|----------------|
| 1 | Slurm | slurm_config, slurm_partition, slurm_node, slurm_account |
| 2 | PBS Pro | pbs_queue, pbs_node, pbs_hook, pbs_resource |
| 3 | LSF | lsf_queue, lsf_host, lsf_user |
| 4 | SGE | sge_queue, sge_host, sge_pe |

### 6.2 Phase 2: Advanced Features

| Priority | Feature | Schedulers |
|----------|---------|------------|
| 1 | GPU (GRES) configuration | All |
| 2 | High availability setup | All |
| 3 | Accounting database setup | Slurm, PBS |
| 4 | Cloud auto-scaling | LSF, Slurm |
| 5 | Node health integration | All |

### 6.3 Recommended Implementation Approach

1. **Slurm First**: Largest market share, best-documented APIs
2. **Generic Abstractions**: Design scheduler-agnostic interfaces where possible
3. **Python Fallback**: Use existing Ansible modules during transition
4. **Community Engagement**: Leverage existing projects (stackhpc, galaxyproject)

---

## 7. Existing Ansible Resources

### 7.1 Slurm

| Project | URL | Status |
|---------|-----|--------|
| stackhpc/ansible-slurm-appliance | [GitHub](https://github.com/stackhpc/ansible-slurm-appliance) | Active |
| galaxyproject/ansible-slurm | [GitHub](https://github.com/galaxyproject/ansible-slurm) | Active |
| OpenHPC Ansible | [OpenHPC](https://openhpc.community/) | Active |

### 7.2 PBS Pro

| Project | URL | Status |
|---------|-----|--------|
| Azure CycleCloud | [Microsoft Learn](https://learn.microsoft.com/en-us/azure/cyclecloud/openpbs) | Active |
| OpenPBS Hooks | [OpenPBS](https://www.openpbs.org/) | Active |

### 7.3 LSF

| Project | URL | Status |
|---------|-----|--------|
| IBM LSF Ansible Playbooks | [IBM GitHub](https://github.com/IBM) | Active |
| AWS LSF Deployment | [AWS Blog](https://aws.amazon.com/blogs/industries/optimizing-hpc-deployments-with-ec2-fleet-and-ibm-spectrum-lsf-2/) | Active |

---

## 8. Acceptance Criteria Summary

For each scheduler, the following baseline workflows must be automated:

1. **Installation**: All components installed from packages or source
2. **Configuration**: All config files managed with idempotent updates
3. **Authentication**: Munge (Slurm) or equivalent configured
4. **Database**: Accounting database configured and connected
5. **Nodes**: Compute nodes can be added/removed dynamically
6. **Queues**: Partitions/queues can be created/modified
7. **Users**: User accounts and quotas manageable
8. **HA**: Failover tested and documented
9. **Upgrades**: Rolling upgrade procedure documented
10. **Monitoring**: Health checks integrated with automation

---

*This document is part of the Rustible HPC Initiative. For related documentation, see:*
- [Modules & Integrations](../compatibility/modules-integrations-capabilities.md)
- [Execution & Reliability](../compatibility/execution-reliability-capabilities.md)
- [Provisioning & State](../compatibility/provisioning-state-capabilities.md)
