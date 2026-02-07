# HPC Reference Blueprints

Reference cluster topologies and playbooks for deploying HPC clusters with Rustible.
These blueprints serve as acceptance-test targets for all HPC modules.

## Topologies

### Small On-Prem (`inventories/onprem/`)

A traditional on-premises HPC cluster:

| Role | Hostname | IP | Notes |
|------|----------|------|-------|
| Login | login01 | 10.0.10.10 | User access |
| Controller | controller01 | 10.0.10.11 | Slurm controller + NFS server |
| Compute | compute01-02 | 10.0.10.20-21 | CPU-only |
| Compute (GPU) | compute03-04 | 10.0.10.22-23 | NVIDIA GPU |

### Cloud-Burst (`inventories/cloud-burst/`)

Hybrid cluster with on-prem controller and AWS compute nodes:

| Role | Hostname | Location | Notes |
|------|----------|----------|-------|
| Controller | controller01 | On-prem (10.0.10.11) | Slurm controller |
| Compute | compute-aws-01-02 | AWS (c5.2xlarge) | CPU-only |
| Compute (GPU) | compute-aws-03-04 | AWS (p3.2xlarge) | NVIDIA GPU |

## Quick Start

```bash
# Deploy the on-prem cluster
rustible examples/hpc/playbooks/site.yml -i examples/hpc/inventories/onprem/hosts.yml

# Validate the cluster
rustible examples/hpc/playbooks/validate.yml -i examples/hpc/inventories/onprem/hosts.yml

# Run a quick health check
rustible examples/hpc/playbooks/healthcheck.yml -i examples/hpc/inventories/onprem/hosts.yml
```

## Playbooks

| Playbook | Purpose |
|----------|---------|
| `site.yml` | Full cluster deployment (baseline + munge + NFS + Slurm) |
| `validate.yml` | Acceptance tests (connectivity, auth, scheduler, MPI, GPU) |
| `healthcheck.yml` | Lightweight periodic health check |

## Roles

| Role | Description |
|------|-------------|
| `hpc_common` | System baseline: packages, sysctl, limits, time sync, directories |
| `munge` | Munge authentication (Slurm prerequisite) |
| `nfs_server` | NFS exports on controller |
| `nfs_client` | NFS mounts on compute/login nodes |
| `slurm_controller` | Slurm controller (slurmctld) configuration |
| `slurm_compute` | Slurm compute node (slurmd) configuration |

## Acceptance Criteria

A cluster deployment is considered successful when:

1. All nodes are reachable via SSH
2. Munge authentication works (encode/decode round-trip)
3. NFS shares are mounted on compute nodes
4. Slurm controller accepts job submissions
5. Slurm compute nodes register and report idle
6. (Optional) GPU nodes report GPUs via `nvidia-smi`
7. (Optional) MPI hello-world runs across multiple nodes

## Prerequisites

- Target nodes running Rocky/Alma Linux 9 or Ubuntu 22.04
- SSH access with sudo privileges
- Network connectivity between all nodes
- (Cloud-burst) AWS credentials configured
- (GPU nodes) NVIDIA GPU hardware present

## Customization

Override variables in `group_vars/` or pass extra vars:

```bash
rustible examples/hpc/playbooks/site.yml \
  -i examples/hpc/inventories/onprem/hosts.yml \
  -e slurm_version=24.05 \
  -e timezone=America/New_York
```
