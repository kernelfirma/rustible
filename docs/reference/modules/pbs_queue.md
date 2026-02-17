---
summary: Reference for the pbs_queue module that manages PBS Pro queue configuration.
read_when: You need to create, delete, enable, disable, start, stop, or configure PBS Pro queues from playbooks.
---

# pbs_queue - PBS Pro Queue Management

## Synopsis

Manage PBS Pro queues via qstat and qmgr. Supports creating, deleting, enabling, disabling, starting, stopping, and setting attributes on PBS queues with full idempotency.

## Classification

**Default** - HPC module. Requires `hpc` and `pbs` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | yes | - | string | Operation to perform: `list`, `create`, `delete`, `enable`, `disable`, `start`, `stop`, or `set_attributes` |
| name | yes | - | string | Queue name |
| queue_type | no | "execution" | string | Queue type: `execution` or `route` (used during `create`) |
| enabled | no | null | string | Whether the queue accepts jobs: `True` or `False` |
| started | no | null | string | Whether the queue routes/runs jobs: `True` or `False` |
| max_run | no | null | string | Maximum number of running jobs in the queue |
| max_queued | no | null | string | Maximum number of queued jobs in the queue |
| resources_max_walltime | no | null | string | Maximum walltime for jobs (e.g., `168:00:00`) |
| resources_max_ncpus | no | null | string | Maximum CPUs per job |
| resources_max_mem | no | null | string | Maximum memory per job (e.g., `256gb`) |
| resources_default_walltime | no | null | string | Default walltime for jobs in this queue |
| priority | no | null | string | Queue priority value |
| acl_groups | no | null | string | Comma-separated ACL group names |
| attributes | no | null | object | JSON object of arbitrary queue attributes to set |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.name | string | Queue name |
| data.queue | object | Current queue configuration (from create, set_attributes) |
| data.queues | object | All queue configurations (from list) |
| data.count | integer | Number of queues listed (from list) |
| data.changes | object | Map of attribute changes applied (from set_attributes) |

## Examples

```yaml
- name: List all PBS queues
  pbs_queue:
    action: list
    name: any

- name: Create a batch queue
  pbs_queue:
    action: create
    name: batch
    queue_type: execution
    enabled: "True"
    started: "True"
    resources_max_walltime: "168:00:00"
    resources_max_ncpus: "128"
    resources_max_mem: "256gb"
    priority: "100"

- name: Create a GPU queue with ACL
  pbs_queue:
    action: create
    name: gpu
    queue_type: execution
    enabled: "True"
    started: "True"
    max_run: "50"
    acl_groups: "gpu_users,admin"

- name: Disable a queue for maintenance
  pbs_queue:
    action: disable
    name: batch

- name: Re-enable a queue
  pbs_queue:
    action: enable
    name: batch

- name: Stop a queue from running jobs
  pbs_queue:
    action: stop
    name: batch

- name: Update queue attributes
  pbs_queue:
    action: set_attributes
    name: batch
    max_run: "100"
    resources_max_walltime: "72:00:00"
    attributes:
      max_array_size: "10000"

- name: Delete a queue
  pbs_queue:
    action: delete
    name: old_queue
```

## Notes

- Requires building with `--features hpc,pbs` (or `full-hpc`).
- Both `action` and `name` are required parameters.
- Create is idempotent: if the queue already exists, no changes are made.
- Delete is idempotent: deleting a non-existent queue returns without error.
- Enable, disable, start, and stop check current state before making changes.
- The `set_attributes` action computes a diff against the current queue configuration and only applies changes where values differ.
- The `attributes` parameter accepts a JSON object for setting arbitrary PBS queue attributes not covered by the named parameters.
- Supports check mode for all actions.
