---
summary: Reference for the warewulf_node module that manages Warewulf compute node definitions via wwctl.
read_when: You need to add, configure, or remove Warewulf compute node definitions from playbooks.
---

# warewulf_node - Manage Warewulf Compute Node Definitions

## Synopsis

Manage Warewulf compute node definitions using the `wwctl` command-line interface. Supports adding nodes with optional image and network assignments, and deleting existing node definitions.

## Classification

**Default** - HPC module. Requires `hpc` and `bare_metal` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | - | string | Node name (e.g., "compute-001") |
| image | no | null | string | Container/image name to assign to the node (passed as `--container` to wwctl) |
| network | no | null | string | Network name to assign to the node (passed as `--netname` to wwctl) |
| state | no | "present" | string | Desired state: "present" or "absent" |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.node | string | The node name |
| data.image | string | The image assigned (when creating) |
| data.network | string | The network assigned (when creating) |

## Examples

```yaml
- name: Add a compute node with image and network
  warewulf_node:
    name: "compute-001"
    image: "rocky8-compute"
    network: "cluster-net"
    state: present

- name: Add a basic node definition
  warewulf_node:
    name: "compute-002"

- name: Remove a node definition
  warewulf_node:
    name: "compute-001"
    state: absent
```

## Notes

- Requires building with `--features hpc,bare_metal` or `--features full-hpc`.
- The `wwctl` command must be available on the target host (Warewulf must be installed).
- If the node already exists, the module reports no change (idempotent for creation).
- The `image` parameter maps to the `--container` flag in `wwctl node add`.
- The `network` parameter maps to the `--netname` flag in `wwctl node add`.
- This module uses `GlobalExclusive` parallelization, meaning only one instance runs across the entire inventory at a time.
