---
summary: Reference for the slurm_qos module that manages Slurm Quality of Service definitions via sacctmgr.
read_when: You need to create, update, or delete Slurm QoS definitions from playbooks.
---

# slurm_qos - Manage Slurm QoS Definitions

## Synopsis

Manages Slurm Quality of Service (QoS) definitions via `sacctmgr`. Supports
creating, updating, and deleting QoS entries with fine-grained control over
job limits, priority, preemption, and resource quotas. All operations are
idempotent.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter           | Required | Default   | Type   | Description                                                        |
|---------------------|----------|-----------|--------|--------------------------------------------------------------------|
| name                | yes      | -         | string | QoS name.                                                          |
| state               | no       | `present` | string | Desired state: `present` or `absent`.                              |
| priority            | no       | -         | string | QoS priority value.                                                |
| max_jobs_per_user   | no       | -         | string | Maximum concurrent jobs per user.                                  |
| max_submit_per_user | no       | -         | string | Maximum submitted jobs per user.                                   |
| max_wall            | no       | -         | string | Maximum wall time (e.g. `7-00:00:00`).                             |
| max_tres_per_user   | no       | -         | string | Maximum TRES per user (e.g. `cpu=100,mem=500G`).                   |
| preempt             | no       | -         | string | Comma-separated QoS names that this QoS can preempt.              |
| preempt_mode        | no       | -         | string | Preempt mode: `cancel`, `requeue`, or `suspend`.                  |
| grace_time          | no       | -         | string | Grace time in seconds before preemption takes effect.              |

## Return Values

| Key     | Type    | Description                                     |
|---------|---------|-------------------------------------------------|
| changed | boolean | Whether changes were made.                      |
| msg     | string  | Status message.                                 |
| data    | object  | Contains `name` and optional `properties` fields. |

## Examples

```yaml
- name: Create a high-priority QoS
  slurm_qos:
    name: high_priority
    priority: "1000"
    max_jobs_per_user: "20"
    max_wall: "14-00:00:00"
    max_tres_per_user: "cpu=256,mem=1T"

- name: Create a preemptible QoS
  slurm_qos:
    name: preemptible
    priority: "10"
    preempt: "normal,low"
    preempt_mode: cancel
    grace_time: "120"

- name: Update QoS limits
  slurm_qos:
    name: high_priority
    max_jobs_per_user: "50"
    max_submit_per_user: "100"

- name: Remove a QoS definition
  slurm_qos:
    name: preemptible
    state: absent
```

## Notes

- Requires building with `--features hpc,slurm`.
- All operations are idempotent: creating an existing QoS with no property changes reports no change.
- When `state: present` (the default), the module creates the QoS if it does not exist or updates it if properties differ.
- Uses `sacctmgr --immediate` to apply changes without confirmation prompts.
- The `preempt` parameter accepts a comma-separated list of QoS names (e.g. `normal,low`).
