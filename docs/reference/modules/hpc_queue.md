---
summary: Reference for the hpc_queue module that provides unified queue/partition management across Slurm and PBS Pro schedulers.
read_when: You need to list, create, or delete HPC queues/partitions from playbooks.
---

# hpc_queue - Unified HPC Queue Management

## Synopsis

Scheduler-agnostic queue and partition operations that work with Slurm or PBS Pro. Supports listing existing queues, creating new queues, and deleting queues. The scheduler backend is selected via the `scheduler` parameter or auto-detected.

## Classification

**Default** - HPC module. Requires `hpc` feature flag. Requires `slurm` or `pbs` feature flag for the respective backend.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | **yes** | - | string | Queue action: `"list"`, `"create"`, or `"delete"`. |
| name | conditional | `null` | string | Queue or partition name. Required for `create` and `delete` actions. |
| scheduler | no | `"auto"` | string | Scheduler backend: `"slurm"`, `"pbs"`, or `"auto"`. |
| queue_type | no | `null` | string | Queue type (PBS-specific: `"execution"` or `"route"`). |
| enabled | no | `null` | boolean | Whether the queue accepts jobs. |
| started | no | `null` | boolean | Whether the queue routes or runs jobs. |
| max_run | no | `null` | integer | Maximum number of running jobs. |
| max_queued | no | `null` | integer | Maximum number of queued jobs. |
| attributes | no | `null` | object | JSON object of additional scheduler-specific attributes. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether a queue was created or deleted |
| msg | string | Status message |
| data.scheduler | string | Scheduler backend used (`"slurm"` or `"pbs"`) |
| data.queues | array | List of queue info objects (for `list` action) with `name`, `state`, and `total_jobs` fields |
| data.count | integer | Number of queues listed (for `list` action) |

## Examples

```yaml
- name: List all queues
  hpc_queue:
    action: list

- name: Create a new batch queue
  hpc_queue:
    action: create
    name: batch
    enabled: true
    started: true

- name: Create a PBS execution queue
  hpc_queue:
    action: create
    name: gpu
    scheduler: pbs
    queue_type: execution

- name: Delete a queue
  hpc_queue:
    action: delete
    name: old_queue
```

## Notes

- Requires building with `--features hpc` and either `--features slurm` or `--features pbs`.
- When `scheduler` is `"auto"`, the module probes for `scontrol` (Slurm) then `qstat` (PBS) on the target.
- Parallelization hint: `GlobalExclusive` (only one invocation at a time cluster-wide, since queue operations affect the entire scheduler).
