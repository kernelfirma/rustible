---
summary: Reference for the slurm_ops module that performs Slurm operational tasks.
read_when: You need to drain or resume nodes, submit or cancel jobs, or reconfigure Slurm from playbooks.
---

# slurm_ops - Slurm Operational Tasks

## Synopsis

Performs operational actions against a running Slurm cluster including draining and
resuming nodes, submitting and cancelling jobs, and triggering cluster reconfiguration.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter  | Required | Default | Type   | Description                                                               |
|------------|----------|---------|--------|---------------------------------------------------------------------------|
| action     | yes      | -       | string | Operation to perform: `drain`, `resume`, `reconfigure`, `submit`, `cancel`. |
| node       | no       | -       | string | Target node or node range (e.g. `node[001-010]`).                         |
| job_id     | no       | -       | string | Job ID for cancel operations.                                             |
| partition  | no       | -       | string | Target partition for submit operations.                                   |
| reason     | no       | -       | string | Reason string when draining a node.                                       |
| script     | no       | -       | string | Path to job script for submit operations.                                 |

## Return Values

| Key    | Type   | Description                                         |
|--------|--------|-----------------------------------------------------|
| status | string | Current implementation status (`stub`).              |
| action | string | The action that was requested.                        |

## Examples

```yaml
- name: Drain a node for maintenance
  slurm_ops:
    action: drain
    node: node042
    reason: "Scheduled hardware maintenance"

- name: Resume a node after maintenance
  slurm_ops:
    action: resume
    node: node042

- name: Reconfigure the cluster after config changes
  slurm_ops:
    action: reconfigure

- name: Submit a batch job
  slurm_ops:
    action: submit
    script: /home/user/jobs/benchmark.sh
    partition: compute

- name: Cancel a running job
  slurm_ops:
    action: cancel
    job_id: "12345"
```

## Notes

- Requires building with `--features hpc,slurm`.
- Currently a stub implementation; scontrol/sbatch integration is planned.
- The `drain` action requires a `node` and optionally a `reason`.
- The `submit` action requires a `script` path.
- The `cancel` action requires a `job_id`.
