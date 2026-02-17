---
summary: Reference for the slurmrestd module that provides a native REST API client for the Slurm slurmrestd HTTP daemon.
read_when: You need to interact with Slurm via the slurmrestd REST API for jobs, nodes, partitions, or diagnostics from playbooks.
---

# slurmrestd - Slurm REST API Client

## Synopsis

Native REST client for `slurmrestd` -- Slurm's HTTP daemon -- enabling structured
JSON interaction for jobs, nodes, partitions, and diagnostics. Falls back to CLI
commands (`sacct`, `sacctmgr`) for accounting endpoints not covered by the REST API.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter      | Required | Default    | Type    | Description                                                         |
|----------------|----------|------------|---------|---------------------------------------------------------------------|
| api_url        | yes      | -          | string  | slurmrestd base URL (e.g. `http://slurmctld:6820`).                |
| api_user       | yes      | -          | string  | Slurm username for `X-SLURM-USER-NAME` header.                     |
| api_token      | yes      | -          | string  | JWT token for `X-SLURM-USER-TOKEN` header.                         |
| action         | yes      | -          | string  | Action to perform (see Actions table below).                        |
| api_version    | no       | `v0.0.44`  | string  | API version string.                                                 |
| timeout        | no       | `30`       | integer | HTTP timeout in seconds.                                            |
| validate_certs | no       | `true`     | boolean | Whether to verify TLS certificates.                                 |
| job_id         | no       | -          | string  | Job ID (for `cancel_job`, `get_job`, `job_history`).                |
| job_name       | no       | -          | string  | Job name (for `submit_job` idempotency).                            |
| script         | no       | -          | string  | Job script content (required for `submit_job`).                     |
| partition      | no       | -          | string  | Partition name (for `submit_job`, `get_partition`).                  |
| nodes          | no       | -          | string  | Number of nodes (for `submit_job`).                                 |
| ntasks         | no       | -          | string  | Number of tasks (for `submit_job`).                                 |
| time_limit     | no       | -          | string  | Wall time limit (for `submit_job`).                                 |
| signal         | no       | -          | string  | Signal to send on cancel (for `cancel_job`).                        |
| node_name      | no       | -          | string  | Node name (for `get_node`, `update_node`).                          |
| state          | no       | -          | string  | Desired node state (for `update_node`).                             |
| reason         | no       | -          | string  | Reason string (for `update_node`).                                  |
| account        | no       | -          | string  | Account name (for `job_history`, `list_accounts` CLI fallback).     |
| user           | no       | -          | string  | User name (for `job_history` CLI fallback).                         |

## Actions

| Action            | Method  | REST Endpoint                   | Description                           |
|-------------------|---------|---------------------------------|---------------------------------------|
| `submit_job`      | POST    | `/slurm/{ver}/job/submit`       | Submit a new job.                     |
| `cancel_job`      | DELETE  | `/slurm/{ver}/job/{id}`         | Cancel an active job.                 |
| `get_job`         | GET     | `/slurm/{ver}/job/{id}`         | Get details for a specific job.       |
| `list_jobs`       | GET     | `/slurm/{ver}/jobs/`            | List all jobs.                        |
| `get_node`        | GET     | `/slurm/{ver}/node/{name}`      | Get details for a specific node.      |
| `list_nodes`      | GET     | `/slurm/{ver}/nodes/`           | List all nodes.                       |
| `update_node`     | POST    | `/slurm/{ver}/node/{name}`      | Update node state.                    |
| `get_partition`   | GET     | `/slurm/{ver}/partition/{name}` | Get details for a specific partition. |
| `list_partitions` | GET     | `/slurm/{ver}/partitions/`      | List all partitions.                  |
| `ping`            | GET     | `/slurm/{ver}/ping/`            | Check slurmrestd connectivity.        |
| `diag`            | GET     | `/slurm/{ver}/diag/`            | Retrieve diagnostics.                 |
| `reconfigure`     | GET     | `/slurm/{ver}/reconfigure/`     | Trigger Slurm reconfiguration.        |
| `job_history`     | CLI     | `sacct` (fallback)              | Query job history via sacct.          |
| `list_accounts`   | CLI     | `sacctmgr` (fallback)           | List accounts via sacctmgr.           |

## Return Values

| Key     | Type    | Description                                                    |
|---------|---------|----------------------------------------------------------------|
| changed | boolean | Whether changes were made.                                     |
| msg     | string  | Status message.                                                |
| data    | object  | Action-specific response data from slurmrestd or CLI fallback. |

## Examples

```yaml
- name: Ping slurmrestd
  slurmrestd:
    api_url: "http://slurmctld:6820"
    api_user: admin
    api_token: "{{ slurm_jwt_token }}"
    action: ping

- name: Submit a job via REST API
  slurmrestd:
    api_url: "http://slurmctld:6820"
    api_user: admin
    api_token: "{{ slurm_jwt_token }}"
    action: submit_job
    script: |
      #!/bin/bash
      echo "Hello from REST"
    job_name: rest_job
    partition: compute
    nodes: "2"
    ntasks: "8"
    time_limit: "60"

- name: List all jobs
  slurmrestd:
    api_url: "http://slurmctld:6820"
    api_user: admin
    api_token: "{{ slurm_jwt_token }}"
    action: list_jobs

- name: Cancel a job
  slurmrestd:
    api_url: "http://slurmctld:6820"
    api_user: admin
    api_token: "{{ slurm_jwt_token }}"
    action: cancel_job
    job_id: "12345"

- name: Get job history via CLI fallback
  slurmrestd:
    api_url: "http://slurmctld:6820"
    api_user: admin
    api_token: "{{ slurm_jwt_token }}"
    action: job_history
    user: jdoe
    account: research

- name: Update node state via REST
  slurmrestd:
    api_url: "http://slurmctld:6820"
    api_user: admin
    api_token: "{{ slurm_jwt_token }}"
    action: update_node
    node_name: node01
    state: drain
    reason: "Scheduled maintenance"
```

## Notes

- Requires building with `--features hpc,slurm`.
- The `job_history` and `list_accounts` actions use CLI fallback (`sacct`/`sacctmgr`) and require a connection to the target host.
- The `submit_job` action is idempotent when `job_name` is set: if a RUNNING, PENDING, or CONFIGURING job with the same name exists, the submission is skipped.
- The `cancel_job` action is idempotent: cancelling a job that is already in a terminal state (COMPLETED, CANCELLED, FAILED, TIMEOUT) reports no change.
- slurmrestd error responses are checked for non-zero `error_number` entries and reported as failures.
- Set `validate_certs: false` to skip TLS certificate verification for self-signed certificates.
