---
summary: Reference for the pbs_job module that manages PBS Pro job lifecycle operations.
read_when: You need to submit, cancel, hold, release, or query PBS Pro jobs from playbooks.
---

# pbs_job - PBS Pro Job Management

## Synopsis

Submit, cancel, hold, release, and query PBS Pro jobs via qsub, qdel, qhold, qrls, and qstat. Supports idempotent job submission by checking for active jobs with the same name before submitting.

## Classification

**Default** - HPC module. Requires `hpc` and `pbs` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | yes | - | string | Operation to perform: `submit`, `cancel`, `status`, `hold`, or `release` |
| script | no | null | string | Inline job script content (for `submit` action) |
| script_path | no | null | string | Path to job script file (for `submit` action) |
| job_name | no | null | string | Job name (`-N` flag for qsub, also used for idempotency checks) |
| queue | no | null | string | Target queue (`-q` flag) |
| nodes | no | null | string | Number of nodes (`-l nodes=N`) |
| ncpus | no | null | string | Number of CPUs per node (`-l ncpus=N`) |
| walltime | no | null | string | Wall time limit (`-l walltime=HH:MM:SS`) |
| output_path | no | null | string | stdout file path (`-o` flag) |
| error_path | no | null | string | stderr file path (`-e` flag) |
| extra_args | no | null | string | Additional qsub arguments as a raw string |
| job_id | conditional | null | string | Job ID to operate on (required for `cancel`, `status`, `hold`, `release`) |
| resource_list | no | null | string | Additional `-l` resource specifications |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.job_id | string | PBS job ID (from submit, cancel, status, hold, release) |
| data.job_name | string | Job name (from submit) |
| data.already_active | boolean | Whether the job was already running (idempotent submit) |
| data.jobs | object | Full job status JSON from qstat (from status action) |
| data.state | string | Terminal job state if already completed (from cancel) |
| data.hold_types | string | Current hold type value (from hold/release) |

## Examples

```yaml
- name: Submit a job from a script file
  pbs_job:
    action: submit
    script_path: /home/user/jobs/simulation.sh
    job_name: my_simulation
    queue: batch
    nodes: "4"
    ncpus: "32"
    walltime: "04:00:00"
    output_path: /logs/sim.out
    error_path: /logs/sim.err

- name: Submit an inline job script
  pbs_job:
    action: submit
    script: |
      #!/bin/bash
      echo "Hello from PBS"
      hostname
    job_name: hello_job
    queue: batch
    walltime: "00:05:00"

- name: Check job status
  pbs_job:
    action: status
    job_id: "12345.pbs-server"

- name: Hold a queued job
  pbs_job:
    action: hold
    job_id: "12345.pbs-server"

- name: Release a held job
  pbs_job:
    action: release
    job_id: "12345.pbs-server"

- name: Cancel a running job
  pbs_job:
    action: cancel
    job_id: "12345.pbs-server"
```

## Notes

- Requires building with `--features hpc,pbs` (or `full-hpc`).
- Either `script` or `script_path` must be provided for the `submit` action.
- The `job_id` parameter is required for `cancel`, `status`, `hold`, and `release` actions.
- Submission is idempotent when `job_name` is provided: if an active job with that name exists, the module returns without submitting a duplicate.
- Cancel is idempotent: cancelling a job that is already in a terminal state (`F` or `X`) or not found returns without error.
- Hold is idempotent: holding an already-held job returns without change.
- Release is idempotent: releasing a job that is not held returns without change.
- Supports check mode for all actions.
