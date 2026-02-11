---
summary: Reference for the rdma_stack module that manages the RDMA/InfiniBand/OFED stack.
read_when: You need to install or configure InfiniBand and RDMA networking on HPC nodes from playbooks.
---

# rdma_stack - RDMA / InfiniBand / OFED Stack

## Synopsis

Manages RDMA userland packages and kernel module configuration for InfiniBand
and RoCE fabrics. Handles OFED stack installation, subnet manager configuration,
and partition key (PKey) setup.

## Classification

**Default** - HPC module. Requires `hpc` and `ofed` feature flags.

## Parameters

| Parameter       | Required | Default    | Type   | Description                                                       |
|-----------------|----------|------------|--------|-------------------------------------------------------------------|
| state           | no       | `present`  | string | Desired state: `present` to install/configure, `absent` to remove.|
| version         | no       | -          | string | OFED stack version to install.                                    |
| packages        | no       | -          | list   | Specific RDMA packages to install (e.g. `rdma-core`, `libibverbs`). |
| subnet_manager  | no       | -          | string | Subnet manager to configure: `opensm`, `none`.                   |
| port            | no       | -          | string | InfiniBand port identifier (e.g. `mlx5_0/1`).                    |
| pkey            | no       | -          | string | Partition key value (e.g. `0x8001`).                              |

## Return Values

| Key    | Type   | Description                                 |
|--------|--------|---------------------------------------------|
| status | string | Current implementation status (`stub`).      |

## Examples

```yaml
- name: Install RDMA/OFED stack
  rdma_stack:
    state: present

- name: Install specific OFED version with subnet manager
  rdma_stack:
    state: present
    version: "5.9-0.5.6.0"
    subnet_manager: opensm

- name: Configure a partition key on a specific port
  rdma_stack:
    port: mlx5_0/1
    pkey: "0x8001"

- name: Remove OFED stack
  rdma_stack:
    state: absent
```

## Notes

- Requires building with `--features hpc,ofed`.
- Currently a stub implementation; OFED install and opensm integration is planned.
- The subnet manager should typically run on only one or two nodes in the fabric.
- PKey configuration requires the OpenSM partition configuration file to be updated.
