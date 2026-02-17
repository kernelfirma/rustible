---
summary: Reference for the hpc_job module that provides unified job management across Slurm and PBS Pro schedulers.
read_when: You need to submit, cancel, hold, release, or check status of HPC jobs from playbooks.
---

# hpc_job - Unified HPC Job Management

## Synopsis

Scheduler-agnostic job operations that work with Slurm or PBS Pro. Supports submitting jobs, cancelling them, querying status, and placing/releasing holds. The scheduler backend is selected via the `scheduler` parameter or auto-detected.

## Classification

**Default** - HPC module. Requires `hpc` feature flag. Requires `slurm` or `pbs` feature flag for the respective backend.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | **yes** | - | string | Job action: `"submit"`, `"cancel"`, `"status"`, `"hold"`, or `"release"`. |
| scheduler | no | `"auto"` | string | Scheduler backend: `"slurm"`, `"pbs"`, or `"auto"`. |
| job_id | conditional | `null` | string | Job ID. Required for `cancel`, `status`, `hold`, and `release` actions. |
| script | no | `null` | string | Inline job script content (for `submit`). |
| script_path | no | `null` | string | Path to a job script file on the target (for `submit`). |
| job_name | no | `null` | string | Job name. Used for idempotency on submit. |
| queue | no | `null` | string | Target queue or partition name. |
| nodes | no | `null` | integer | Number of nodes to request. |
| cpus | no | `null` | integer | Number of CPUs or tasks to request. |
| walltime | no | `null` | string | Wall time limit (e.g. `"24:00:00"`). |
| output_path | no | `null` | string | Path for stdout output file. |
| error_path | no | `null` | string | Path for stderr output file. |
| extra_args | no | `null` | string | Additional scheduler-specific arguments. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether a state change occurred |
| msg | string | Status message |
| data.scheduler | string | Scheduler backend used (`"slurm"` or `"pbs"`) |
| data.job | object | Job information object (for `status` action) with `id`, `name`, `state`, `queue`, `owner`, `nodes`, `cpus`, and walltime fields |

## Examples

```yaml
- name: Submit a job from a script file
  hpc_job:
    action: submit
    script_path: /home/user/my_simulation.sh
    job_name: sim_run_001
    queue: batch
    nodes: 4
    walltime: "12:00:00"

- name: Check job status
  hpc_job:
    action: status
    job_id: "12345"

- name: Cancel a job
  hpc_job:
    action: cancel
    job_id: "12345"

- name: Hold a job
  hpc_job:
    action: hold
    job_id: "12345"

- name: Release a held job
  hpc_job:
    action: release
    job_id: "12345"

- name: Submit with explicit Slurm backend
  hpc_job:
    action: submit
    scheduler: slurm
    script_path: /home/user/job.sh
    queue: gpu
    nodes: 2
    cpus: 64
```

## Notes

- Requires building with `--features hpc` and either `--features slurm` or `--features pbs`.
- When `scheduler` is `"auto"`, the module probes for `scontrol` (Slurm) then `qstat` (PBS) on the target.
- Parallelization hint: `FullyParallel` (safe to run on multiple hosts simultaneously).
