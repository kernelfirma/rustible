---
summary: Reference for the slurm_partition module that manages Slurm partitions via scontrol.
read_when: You need to create, update, or delete Slurm partitions from playbooks.
---

# slurm_partition - Manage Slurm Partitions

## Synopsis

Manages Slurm partitions via `scontrol`. Supports creating new partitions,
updating existing partition properties, and deleting partitions. Uses a
declarative `state` parameter for present/absent management.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter     | Required | Default   | Type    | Description                                                      |
|---------------|----------|-----------|---------|------------------------------------------------------------------|
| name          | yes      | -         | string  | Partition name.                                                  |
| state         | no       | `present` | string  | Desired state: `present` or `absent`.                            |
| nodes         | no       | -         | string  | Comma-separated list of nodes (e.g. `node[01-10]`).             |
| max_time      | no       | -         | string  | Maximum wall time (e.g. `7-00:00:00`).                           |
| default       | no       | `false`   | boolean | Whether this is the default partition.                           |
| priority_tier | no       | -         | string  | Priority tier value.                                             |
| properties    | no       | -         | object  | Map of additional key=value properties to pass to scontrol.      |

## Return Values

| Key     | Type    | Description                                      |
|---------|---------|--------------------------------------------------|
| changed | boolean | Whether changes were made.                       |
| msg     | string  | Status message.                                  |
| data    | object  | Contains `partition` name and optional `properties` fields. |

## Examples

```yaml
- name: Create a compute partition
  slurm_partition:
    name: compute
    nodes: "node[01-10]"
    max_time: "7-00:00:00"
    default: true
    priority_tier: "10"

- name: Update partition with additional properties
  slurm_partition:
    name: gpu
    nodes: "gpu[01-08]"
    max_time: "24:00:00"
    properties:
      State: UP
      AllowGroups: physics
      PreemptMode: CANCEL
      MaxNodes: 4

- name: Remove a partition
  slurm_partition:
    name: debug
    state: absent
```

## Notes

- Requires building with `--features hpc,slurm`.
- When `state: present` (the default), the module creates the partition if it does not exist, or updates it if properties differ.
- The `properties` parameter accepts a JSON object of arbitrary Slurm partition properties (e.g. `State`, `AllowGroups`, `PreemptMode`).
- Uses `scontrol create`/`scontrol update`/`scontrol delete` for partition management.
- All operations are idempotent.
