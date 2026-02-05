# Ansible Baseline for HPC Operations

Phase 3A of the HPC Initiative - Establishing the Ansible baseline used in real HPC operations, including capability mapping, patterns, and limitations.

## Table of Contents

1. [Ansible HPC Ecosystem Overview](#1-ansible-hpc-ecosystem-overview)
2. [Capability Map by HPC Domain](#2-capability-map-by-hpc-domain)
3. [Common Roles and Collections](#3-common-roles-and-collections)
4. [Configuration Patterns](#4-configuration-patterns)
5. [Failure Handling and Rollback](#5-failure-handling-and-rollback)
6. [Strengths and Best Practices](#6-strengths-and-best-practices)
7. [Limitations and Pain Points](#7-limitations-and-pain-points)
8. [Rustible Improvement Opportunities](#8-rustible-improvement-opportunities)

---

## 1. Ansible HPC Ecosystem Overview

### 1.1 Key Projects and Collections

| Project | Maintainer | Focus Area |
|---------|------------|------------|
| **stackhpc.openhpc** | StackHPC | OpenHPC/Slurm deployment |
| **ansible-slurm-appliance** | StackHPC | Complete Slurm environment |
| **galaxyproject.slurm** | Galaxy Project | Slurm role |
| **ansible-playbook-for-ohpc** | Linaro | OpenHPC on ARM |
| **ElastiCluster** | GC3/UZH | Cloud HPC clusters |
| **osc.pbspro** | OSC | PBS Pro deployment |
| **NVIDIA.nvidia_driver** | NVIDIA | GPU drivers |

### 1.2 Typical HPC Ansible Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Ansible Control Architecture                      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Control Node (Admin workstation or CI/CD)                         │
│  ├── Inventory (static YAML or dynamic from CMDB)                  │
│  ├── Playbooks (site.yml, cluster.yml, etc.)                       │
│  ├── Roles (stackhpc.openhpc, custom roles)                        │
│  ├── Group/Host vars (per-cluster configuration)                   │
│  └── Vault (encrypted secrets)                                     │
│                                                                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                │
│  │ Head Nodes  │  │ Compute     │  │ Storage     │                │
│  │ (login,     │  │ Nodes       │  │ Servers     │                │
│  │  scheduler) │  │ (100s-1000s)│  │ (MDS, OSS)  │                │
│  └─────────────┘  └─────────────┘  └─────────────┘                │
│        ↑               ↑                ↑                          │
│        └───────────────┴────────────────┘                          │
│                    SSH connections                                  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.3 Inventory Organization

```yaml
# Typical HPC inventory structure
all:
  children:
    cluster:
      children:
        login:
          hosts:
            login01:
            login02:
        control:
          hosts:
            slurmctld01:
            slurmctld02:
        compute:
          children:
            compute_standard:
              hosts:
                node[001:100]:
            compute_gpu:
              hosts:
                gpu[001:020]:
            compute_himem:
              hosts:
                himem[001:010]:
        storage:
          children:
            mds:
              hosts:
                mds[01:02]:
            oss:
              hosts:
                oss[01:10]:
```

---

## 2. Capability Map by HPC Domain

### 2.1 Scheduler Management

| Capability | Ansible Support | Typical Approach |
|------------|-----------------|------------------|
| **Slurm installation** | ✅ Strong | OpenHPC packages, stackhpc.openhpc |
| **slurm.conf management** | ✅ Strong | Template with node/partition vars |
| **Accounting setup** | ✅ Good | MySQL + slurmdbd configuration |
| **Node configuration** | ✅ Strong | Dynamic inventory + templates |
| **Partition management** | ✅ Good | Variable-driven templates |
| **GRES (GPU) config** | ✅ Good | Auto-detection or manual vars |
| **Cgroup configuration** | ⚠️ Moderate | Template-based, complex |
| **Federation** | ⚠️ Limited | Manual configuration needed |

**Example Slurm playbook pattern:**
```yaml
- name: Configure Slurm cluster
  hosts: cluster
  roles:
    - role: stackhpc.openhpc
      vars:
        openhpc_slurm_configless: true
        openhpc_slurm_control_host: "{{ groups['control'][0] }}"
        openhpc_slurm_partitions:
          - name: batch
            default: yes
            nodes: "node[001-100]"
```

### 2.2 High-Performance Fabric

| Capability | Ansible Support | Typical Approach |
|------------|-----------------|------------------|
| **IB driver installation** | ✅ Good | MLNX_OFED packages |
| **IPoIB configuration** | ✅ Good | Network role + templates |
| **OpenSM configuration** | ⚠️ Moderate | Template for opensm.conf |
| **Partition configuration** | ⚠️ Limited | Manual or custom role |
| **Fabric diagnostics** | ❌ Weak | Ad-hoc command modules |
| **Firmware updates** | ⚠️ Limited | Vendor-specific, risky |

**Example InfiniBand pattern:**
```yaml
- name: Configure InfiniBand
  hosts: compute
  tasks:
    - name: Install MLNX_OFED
      ansible.builtin.package:
        name: mlnx-ofed-all
        state: present

    - name: Configure IPoIB interface
      ansible.builtin.template:
        src: ifcfg-ib0.j2
        dest: /etc/sysconfig/network-scripts/ifcfg-ib0
      notify: restart network
```

### 2.3 Parallel Filesystems

| Capability | Ansible Support | Typical Approach |
|------------|-----------------|------------------|
| **Lustre client mount** | ✅ Good | Package + fstab/mount |
| **Lustre server setup** | ⚠️ Moderate | Complex, multi-step |
| **BeeGFS client** | ✅ Good | Official packages |
| **BeeGFS server** | ⚠️ Moderate | Template-driven |
| **GPFS client** | ⚠️ Limited | IBM-specific procedures |
| **GPFS cluster** | ❌ Weak | Manual/IBM tools |
| **Quota management** | ⚠️ Limited | Command modules |
| **Filesystem tuning** | ⚠️ Limited | Ad-hoc commands |

**Example Lustre client pattern:**
```yaml
- name: Configure Lustre client
  hosts: compute
  tasks:
    - name: Install Lustre client packages
      ansible.builtin.package:
        name:
          - lustre-client
          - lustre-client-dkms
        state: present

    - name: Mount Lustre filesystem
      ansible.posix.mount:
        path: /scratch
        src: "mds01@o2ib:/scratch"
        fstype: lustre
        opts: defaults,_netdev,flock
        state: mounted
```

### 2.4 GPU and Accelerator Stack

| Capability | Ansible Support | Typical Approach |
|------------|-----------------|------------------|
| **NVIDIA driver** | ✅ Good | NVIDIA role or packages |
| **CUDA toolkit** | ✅ Good | Package installation |
| **cuDNN/NCCL** | ✅ Good | Package installation |
| **nvidia-persistenced** | ✅ Good | Service management |
| **DCGM setup** | ⚠️ Moderate | Package + service |
| **GPU health checks** | ⚠️ Limited | nvidia-smi commands |
| **Multi-version CUDA** | ⚠️ Moderate | Path management |
| **AMD ROCm** | ⚠️ Limited | Less community support |

**Example GPU configuration pattern:**
```yaml
- name: Configure NVIDIA GPUs
  hosts: compute_gpu
  roles:
    - role: nvidia.nvidia_driver
      vars:
        nvidia_driver_branch: "550"
        nvidia_driver_persistence_mode_on: yes

  tasks:
    - name: Install CUDA toolkit
      ansible.builtin.package:
        name: cuda-toolkit-12-4
        state: present

    - name: Set CUDA environment
      ansible.builtin.template:
        src: cuda.sh.j2
        dest: /etc/profile.d/cuda.sh
```

### 2.5 MPI Stacks

| Capability | Ansible Support | Typical Approach |
|------------|-----------------|------------------|
| **OpenMPI installation** | ✅ Good | OpenHPC or packages |
| **MPICH installation** | ✅ Good | Packages |
| **Intel MPI** | ⚠️ Moderate | License handling needed |
| **MPI environment** | ✅ Good | Module files |
| **UCX configuration** | ⚠️ Limited | Environment variables |
| **MPI testing** | ⚠️ Limited | Ad-hoc verification |

### 2.6 Environment Modules

| Capability | Ansible Support | Typical Approach |
|------------|-----------------|------------------|
| **Lmod installation** | ✅ Good | Package installation |
| **Module paths** | ✅ Good | Profile.d scripts |
| **Spack installation** | ⚠️ Moderate | Git clone + config |
| **Spack packages** | ⚠️ Limited | Long-running, complex |
| **EasyBuild** | ⚠️ Moderate | Similar to Spack |
| **Module defaults** | ✅ Good | Lmod configuration |

### 2.7 Identity and Access

| Capability | Ansible Support | Typical Approach |
|------------|-----------------|------------------|
| **SSSD configuration** | ✅ Strong | RHEL system roles |
| **LDAP client** | ✅ Strong | RHEL system roles |
| **Kerberos client** | ✅ Good | krb5.conf template |
| **SSH key distribution** | ✅ Good | authorized_key module |
| **PAM configuration** | ⚠️ Moderate | Template + pamd module |
| **Sudo rules** | ✅ Good | Template or LDAP |

### 2.8 Secrets and Compliance

| Capability | Ansible Support | Typical Approach |
|------------|-----------------|------------------|
| **Ansible Vault** | ✅ Strong | Built-in encryption |
| **HashiCorp Vault** | ✅ Good | Lookup plugins |
| **AWS Secrets Manager** | ✅ Good | AWS collection |
| **Compliance scanning** | ⚠️ Moderate | Custom tasks |
| **Audit logging** | ⚠️ Limited | Callback plugins |

---

## 3. Common Roles and Collections

### 3.1 StackHPC Collections

| Role/Collection | Purpose | Maturity |
|-----------------|---------|----------|
| **stackhpc.openhpc** | OpenHPC/Slurm deployment | Production |
| **stackhpc.linux** | Linux system configuration | Production |
| **stackhpc.infiniband** | InfiniBand setup | Production |
| **stackhpc.cluster_infra** | OpenStack infrastructure | Production |

### 3.2 Community Roles

| Role | Purpose | Notes |
|------|---------|-------|
| **geerlingguy.docker** | Docker installation | Well-maintained |
| **geerlingguy.nfs** | NFS server/client | Simple use cases |
| **ansible.posix** | POSIX modules | Official collection |
| **community.general** | General modules | Broad utility |

### 3.3 Vendor Roles

| Role | Purpose | Source |
|------|---------|--------|
| **nvidia.nvidia_driver** | GPU drivers | NVIDIA |
| **redhat.rhel_system_roles** | RHEL configuration | Red Hat |
| **amazon.aws** | AWS integration | AWS |

---

## 4. Configuration Patterns

### 4.1 Site-Wide Configuration

```yaml
# group_vars/all/cluster.yml
---
cluster_name: "hpc-prod"
cluster_domain: "hpc.example.com"
ntp_servers:
  - ntp1.example.com
  - ntp2.example.com
dns_servers:
  - 10.0.0.1
  - 10.0.0.2

# Slurm configuration
slurm_cluster_name: "{{ cluster_name }}"
slurm_control_host: slurmctld01
slurm_backup_control: slurmctld02
slurm_accounting_host: slurmdbd01
```

### 4.2 Node-Type Configuration

```yaml
# group_vars/compute_gpu/gpu.yml
---
nvidia_driver_version: "550.54.14"
cuda_version: "12.4"

slurm_gres_config:
  - "NodeName=gpu[001-020] Name=gpu Type=a100 File=/dev/nvidia[0-3]"

gpu_resource_def:
  - name: gpu
    count: 4
    type: a100
```

### 4.3 Secrets Management

```yaml
# group_vars/all/vault.yml (encrypted)
---
slurm_db_password: "{{ vault_slurm_db_password }}"
ldap_bind_password: "{{ vault_ldap_bind_password }}"
munge_key: "{{ vault_munge_key }}"

# Vault lookup example
vault_slurm_db_password: !vault |
  $ANSIBLE_VAULT;1.1;AES256
  ...
```

### 4.4 Dynamic Inventory Pattern

```python
#!/usr/bin/env python3
# inventory/dynamic_inventory.py
"""Dynamic inventory from CMDB/Netbox."""

import json
import requests

def get_hosts():
    response = requests.get("https://cmdb.example.com/api/nodes")
    nodes = response.json()

    inventory = {
        "compute": {"hosts": []},
        "compute_gpu": {"hosts": []},
        "_meta": {"hostvars": {}}
    }

    for node in nodes:
        group = "compute_gpu" if node["has_gpu"] else "compute"
        inventory[group]["hosts"].append(node["hostname"])
        inventory["_meta"]["hostvars"][node["hostname"]] = {
            "ansible_host": node["ip"],
            "gpu_count": node.get("gpu_count", 0)
        }

    return inventory

if __name__ == "__main__":
    print(json.dumps(get_hosts()))
```

---

## 5. Failure Handling and Rollback

### 5.1 Error Handling Patterns

```yaml
# Block/rescue/always pattern
- name: Update critical service
  block:
    - name: Stop service
      ansible.builtin.service:
        name: slurmd
        state: stopped

    - name: Update configuration
      ansible.builtin.template:
        src: slurm.conf.j2
        dest: /etc/slurm/slurm.conf
      register: config_result

    - name: Start service
      ansible.builtin.service:
        name: slurmd
        state: started

  rescue:
    - name: Restore previous configuration
      ansible.builtin.copy:
        src: /etc/slurm/slurm.conf.bak
        dest: /etc/slurm/slurm.conf
        remote_src: yes

    - name: Start service with old config
      ansible.builtin.service:
        name: slurmd
        state: started

    - name: Notify failure
      ansible.builtin.debug:
        msg: "Configuration update failed, rolled back"

  always:
    - name: Verify service status
      ansible.builtin.service:
        name: slurmd
        state: started
      register: service_status
      failed_when: false
```

### 5.2 Pre-Change Backup Pattern

```yaml
- name: Backup before changes
  hosts: all
  tasks:
    - name: Create backup directory
      ansible.builtin.file:
        path: /var/backup/ansible/{{ ansible_date_time.date }}
        state: directory

    - name: Backup critical configs
      ansible.builtin.copy:
        src: "{{ item }}"
        dest: "/var/backup/ansible/{{ ansible_date_time.date }}/"
        remote_src: yes
      loop:
        - /etc/slurm/slurm.conf
        - /etc/sssd/sssd.conf
        - /etc/fstab
      ignore_errors: yes
```

### 5.3 Serial Execution for Safety

```yaml
- name: Rolling kernel update
  hosts: compute
  serial: "10%"  # 10% of hosts at a time
  max_fail_percentage: 5

  tasks:
    - name: Drain node from Slurm
      ansible.builtin.command:
        cmd: scontrol update nodename={{ inventory_hostname }} state=drain reason="maintenance"
      delegate_to: "{{ slurm_control_host }}"

    - name: Update kernel
      ansible.builtin.package:
        name: kernel
        state: latest
      register: kernel_update

    - name: Reboot if needed
      ansible.builtin.reboot:
        reboot_timeout: 600
      when: kernel_update.changed

    - name: Resume node
      ansible.builtin.command:
        cmd: scontrol update nodename={{ inventory_hostname }} state=resume
      delegate_to: "{{ slurm_control_host }}"
```

### 5.4 Validation Tasks

```yaml
- name: Post-change validation
  hosts: compute
  tasks:
    - name: Verify Slurm connectivity
      ansible.builtin.command:
        cmd: sinfo -N -n {{ inventory_hostname }}
      register: sinfo_result
      failed_when: "'idle' not in sinfo_result.stdout and 'alloc' not in sinfo_result.stdout"

    - name: Verify filesystem mounts
      ansible.builtin.command:
        cmd: mountpoint /scratch
      changed_when: false

    - name: Verify GPU detection
      ansible.builtin.command:
        cmd: nvidia-smi -L
      register: gpu_result
      failed_when: gpu_result.rc != 0
      when: "'compute_gpu' in group_names"
```

---

## 6. Strengths and Best Practices

### 6.1 Ansible Strengths for HPC

| Strength | Description | HPC Benefit |
|----------|-------------|-------------|
| **Agentless** | No daemon on nodes | Minimal footprint, easy bootstrap |
| **Idempotent** | Safe to re-run | Drift correction, consistency |
| **YAML syntax** | Human-readable | Accessible to HPC admins |
| **Extensible** | Custom modules | HPC-specific tooling |
| **Ecosystem** | Large community | Many HPC roles available |
| **Vault** | Built-in secrets | Secure credential handling |

### 6.2 Best Practices

**Inventory Management:**
- Use dynamic inventory from CMDB/Netbox
- Group hosts by function and hardware type
- Use group_vars hierarchy for configuration

**Playbook Organization:**
```
site/
├── inventory/
│   ├── production/
│   │   ├── hosts.yml
│   │   ├── group_vars/
│   │   └── host_vars/
│   └── staging/
├── playbooks/
│   ├── site.yml           # Full cluster
│   ├── scheduler.yml      # Slurm only
│   ├── compute.yml        # Compute nodes
│   └── storage.yml        # Storage servers
├── roles/
│   ├── common/
│   ├── slurm/
│   └── lustre_client/
└── collections/
    └── requirements.yml
```

**Performance Optimization:**
- Use `strategy: free` for independent tasks
- Set appropriate `forks` (50-100 for HPC)
- Enable pipelining in ansible.cfg
- Use `async` for long-running tasks

---

## 7. Limitations and Pain Points

### 7.1 Scale Limitations

| Issue | Impact | Workaround |
|-------|--------|------------|
| **SSH overhead** | Slow at 1000+ nodes | Mitogen, pull mode |
| **Serial execution** | 6+ hour runs | `strategy: free`, increase forks |
| **Controller bottleneck** | Single point of failure | AWX/Tower, multiple controllers |
| **Memory consumption** | OOM on large inventories | Limit batch size |
| **Fact gathering** | Slow startup | Cache facts, `gather_facts: no` |

### 7.2 HPC-Specific Pain Points

| Pain Point | Description | Impact |
|------------|-------------|--------|
| **Heterogeneous nodes** | Different hardware configurations | Complex group_vars, conditionals |
| **Long-running tasks** | Spack builds, firmware updates | Timeouts, orphaned processes |
| **Fabric operations** | IB diagnostics, SM config | Poor module support |
| **Scheduler integration** | Job-aware maintenance | Manual drain/resume |
| **Parallel filesystem** | Complex server operations | Limited modules |
| **Firmware updates** | BMC, HCA, GPU firmware | Risky, vendor-specific |

### 7.3 Operational Challenges

| Challenge | Description | Mitigation |
|-----------|-------------|------------|
| **Partial failures** | Some nodes fail, others succeed | `--limit @failed_hosts.retry` |
| **Rollback complexity** | No built-in state tracking | Manual backup/restore |
| **Drift detection** | No native drift reporting | Custom check mode analysis |
| **Testing** | Difficult to test at scale | Molecule, staging clusters |
| **Secrets rotation** | Vault re-encryption needed | External secret managers |

### 7.4 Missing Capabilities

| Capability | Current State | Need |
|------------|---------------|------|
| **Native Slurm module** | Command-based | Proper state management |
| **IB fabric management** | Ad-hoc commands | OpenSM configuration module |
| **Lustre management** | Basic mount only | OST/MDT lifecycle |
| **GPU GRES auto-config** | Template-based | Dynamic detection |
| **License-aware execution** | Not available | FlexLM integration |
| **Checkpoint/resume** | Not available | Long playbook recovery |

---

## 8. Rustible Improvement Opportunities

### 8.1 Performance Improvements

| Opportunity | Ansible Limitation | Rustible Approach |
|-------------|-------------------|-------------------|
| **Parallel execution** | Python GIL, SSH overhead | Native async, connection pooling |
| **Large inventories** | Memory-heavy | Streaming, lazy evaluation |
| **Fact caching** | File-based, slow | In-memory, distributed |
| **Module execution** | Python interpreter startup | Native compiled modules |

### 8.2 HPC-Specific Modules

| Module | Purpose | Ansible Gap |
|--------|---------|-------------|
| **slurm_node** | Node state management | No native module |
| **slurm_partition** | Partition configuration | Template-only |
| **opensm_config** | Subnet manager | Not available |
| **lustre_ost** | OST lifecycle | Not available |
| **beegfs_target** | Storage target management | Not available |
| **flexlm_license** | License checking | Not available |
| **ipmi_boot** | Boot device control | Basic only |
| **redfish_firmware** | Firmware updates | Limited |

### 8.3 Operational Features

| Feature | Ansible Gap | Rustible Opportunity |
|---------|-------------|---------------------|
| **Checkpoint/resume** | None | Native support |
| **Automatic rollback** | Manual | State-tracked undo |
| **Drift detection** | Check mode only | Continuous monitoring |
| **Job-aware execution** | Manual integration | Slurm API integration |
| **Scale optimization** | Tuning required | Adaptive parallelism |

### 8.4 Priority Implementation

| Priority | Module/Feature | Rationale |
|----------|----------------|-----------|
| **P0** | Ansible compatibility | Migration path |
| **P1** | slurm_node, slurm_partition | Most common HPC need |
| **P1** | Parallel execution at scale | Key differentiator |
| **P2** | opensm_config | Fabric management |
| **P2** | lustre_mount, lustre_ost | Storage operations |
| **P3** | Checkpoint/resume | Long playbook recovery |
| **P3** | Automatic rollback | Operational safety |

---

## References

- [StackHPC Ansible Slurm Appliance](https://github.com/stackhpc/ansible-slurm-appliance)
- [StackHPC OpenHPC Role](https://galaxy.ansible.com/stackhpc/openhpc)
- [Galaxy Project Slurm Role](https://github.com/galaxyproject/ansible-slurm)
- [NVIDIA Ansible Role](https://github.com/NVIDIA/ansible-role-nvidia-driver)
- [Red Hat HPC on Azure](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/9/html/deploying_rhel_9_on_microsoft_azure/deploying-an-hpc-cluster-on-azure-by-using-rhel-system-roles)
- [Scaling Ansible](https://medium.com/@devonfinninger/scaling-ansible-b6dcc310cd7c)
- [Ansible Performance Optimization](https://www.redhat.com/sysadmin/optimize-ansible-automation-platform)
