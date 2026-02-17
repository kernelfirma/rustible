---
summary: Reference for the slurm_queue module that manages Slurm partitions at runtime via scontrol.
read_when: You need to create, update, delete, or change the state of Slurm partitions at runtime from playbooks.
---

# slurm_queue - Manage Slurm Partitions at Runtime

## Synopsis

Manages Slurm partitions (queues) at runtime via `scontrol`. Unlike
`slurm_partition` which uses a declarative state model, this module provides
explicit action-based control including diff-aware updates and partition
state management (UP/DOWN).

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter     | Required | Default | Type   | Description                                                         |
|---------------|----------|---------|--------|---------------------------------------------------------------------|
| action        | yes      | -       | string | Action to perform: `create`, `update`, `delete`, or `state`.        |
| name          | yes      | -       | string | Partition name.                                                     |
| nodes         | no       | -       | string | Node list for the partition (e.g. `node[01-10]`).                   |
| default       | no       | -       | string | Whether this is the default partition: `yes` or `no`.               |
| max_time      | no       | -       | string | Maximum time limit (e.g. `7-00:00:00`).                             |
| max_nodes     | no       | -       | string | Maximum nodes per job.                                              |
| state         | no       | -       | string | Partition state: `UP` or `DOWN`. Required for the `state` action.   |
| priority_tier | no       | -       | string | Priority tier value.                                                |
| allow_groups  | no       | -       | string | Allowed groups (comma-separated).                                   |

## Return Values

| Key     | Type    | Description                                                          |
|---------|---------|----------------------------------------------------------------------|
| changed | boolean | Whether changes were made.                                           |
| msg     | string  | Status message.                                                      |
| data    | object  | Contains `name`, `partition` (current properties), `changes`, and `state` fields. |

## Examples

```yaml
- name: Create a new partition
  slurm_queue:
    action: create
    name: gpu
    nodes: "gpu[01-08]"
    default: "no"
    max_time: "24:00:00"
    priority_tier: "100"
    allow_groups: "admin,research"

- name: Update partition properties (diff-aware)
  slurm_queue:
    action: update
    name: compute
    max_time: "48:00:00"
    max_nodes: "16"

- name: Bring a partition down
  slurm_queue:
    action: state
    name: maintenance
    state: DOWN

- name: Bring a partition back up
  slurm_queue:
    action: state
    name: maintenance
    state: UP

- name: Delete a partition
  slurm_queue:
    action: delete
    name: debug
```

## Notes

- Requires building with `--features hpc,slurm`.
- The `update` action compares current partition properties against desired values and only applies changes when differences are detected.
- The `state` action requires the `state` parameter and only accepts `UP` or `DOWN`.
- All actions are idempotent: creating an existing partition or deleting a non-existent one reports no change.
- Uses `scontrol show partition` with one-liner output for property comparison.
