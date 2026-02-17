---
summary: Reference for the slurm_job module that submits, cancels, and queries Slurm jobs via sbatch/scancel/squeue/sacct.
read_when: You need to submit, cancel, or check the status of Slurm jobs from playbooks.
---

# slurm_job - Manage Slurm Jobs

## Synopsis

Submits, cancels, and queries Slurm jobs using the standard CLI tools `sbatch`,
`scancel`, `squeue`, and `sacct`. The submit action supports both inline scripts
and script file paths, with idempotency based on job name.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter   | Required | Default | Type   | Description                                                                     |
|-------------|----------|---------|--------|---------------------------------------------------------------------------------|
| action      | yes      | -       | string | Action to perform: `submit`, `cancel`, or `status`.                             |
| script      | no       | -       | string | Inline job script content. Required for `submit` if `script_path` is not set.   |
| script_path | no       | -       | string | Path to a job script file. Required for `submit` if `script` is not set.        |
| job_name    | no       | -       | string | Job name (`--job-name` for sbatch). Used for idempotency on submit.             |
| partition   | no       | -       | string | Target partition (`--partition`).                                               |
| nodes       | no       | -       | string | Number of nodes (`--nodes`).                                                    |
| ntasks      | no       | -       | string | Number of tasks (`--ntasks`).                                                   |
| time_limit  | no       | -       | string | Wall time limit (`--time`), e.g. `2:00:00`.                                     |
| output      | no       | -       | string | Stdout file path (`--output`).                                                  |
| error       | no       | -       | string | Stderr file path (`--error`).                                                   |
| extra_args  | no       | -       | string | Additional sbatch arguments as a raw string (e.g. `--mem=4G --gres=gpu:1`).     |
| job_id      | no       | -       | string | Job ID. Required for `cancel` and `status` actions.                             |
| signal      | no       | -       | string | Signal to send on cancel (e.g. `SIGTERM`).                                      |

## Return Values

| Key            | Type    | Description                                                     |
|----------------|---------|-----------------------------------------------------------------|
| changed        | boolean | Whether changes were made.                                      |
| msg            | string  | Status message.                                                 |
| data           | object  | Contains `job_id`, `job_name`, and action-specific fields.      |

For `submit`, `data` includes:
- `job_id` - The assigned job ID.
- `job_name` - The job name (if provided).
- `already_active` - Set to `true` if an existing job with the same name is active.

For `status`, `data` includes:
- `job` - Structured job information (state, partition, nodes, etc.).
- `source` - Either `squeue` (active) or `sacct` (completed).

## Examples

```yaml
- name: Submit a job from a script file
  slurm_job:
    action: submit
    script_path: /shared/jobs/simulation.sh
    job_name: sim_run_01
    partition: compute
    nodes: "4"
    ntasks: "128"
    time_limit: "8:00:00"
    output: /logs/sim_%j.out
    error: /logs/sim_%j.err

- name: Submit an inline job
  slurm_job:
    action: submit
    script: |
      #!/bin/bash
      echo "Hello from $(hostname)"
      sleep 60
    job_name: hello_job
    partition: debug

- name: Check job status
  slurm_job:
    action: status
    job_id: "12345"

- name: Cancel a job with SIGTERM
  slurm_job:
    action: cancel
    job_id: "12345"
    signal: SIGTERM
```

## Notes

- Requires building with `--features hpc,slurm`.
- Either `script` or `script_path` must be provided for the `submit` action.
- Submit is idempotent when `job_name` is set: if a PENDING or RUNNING job with the same name exists, the module reports no change.
- The `status` action first queries `squeue` for active jobs, then falls back to `sacct` for completed jobs.
- Inline scripts are submitted using `sbatch --wrap`.
