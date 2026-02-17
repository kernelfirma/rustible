---
summary: Reference for the hpc_healthcheck module that runs configurable health checks against HPC nodes.
read_when: You need to validate HPC node health (munge, NFS, services, GPU, InfiniBand) from playbooks.
---

# hpc_healthcheck - HPC Node Health Checks

## Synopsis

Runs a configurable set of health checks against an HPC node and returns structured pass/fail results. Checks include MUNGE authentication round-trip, NFS mount availability, systemd service status, GPU validation via `nvidia-smi`, and InfiniBand validation via `ibstat`. Optionally fails the module when any check does not pass.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| checks | no | all applicable | list(string) | List of checks to run. Values: `"munge"`, `"nfs"`, `"services"`, `"gpu"`, `"infiniband"`. When omitted, all applicable checks run. |
| nfs_mounts | no | `[]` | list(string) | List of NFS mount point paths to verify (used by the `nfs` check). |
| services | no | `[]` | list(string) | List of systemd service names to verify (used by the `services` check). |
| fail_on_error | no | `false` | boolean | When `true`, the module returns an error if any check fails. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Always `false` (read-only module) |
| msg | string | Summary message (e.g. "HPC health check: 5/5 passed") |
| data.healthcheck | object | Structured results object |
| data.healthcheck.checks | array | Per-check results with `check`, `passed`, `detail`, and `stderr` fields |
| data.healthcheck.passed | integer | Number of checks that passed |
| data.healthcheck.failed | integer | Number of checks that failed |
| data.healthcheck.total | integer | Total number of checks executed |

## Examples

```yaml
- name: Run all HPC health checks
  hpc_healthcheck:
    nfs_mounts:
      - /home
      - /scratch
    services:
      - slurmctld
      - munge

- name: Check only munge and GPU, fail on error
  hpc_healthcheck:
    checks:
      - munge
      - gpu
    fail_on_error: true

- name: Verify NFS mounts only
  hpc_healthcheck:
    checks:
      - nfs
    nfs_mounts:
      - /home
      - /scratch
      - /apps
```

## Notes

- Requires building with `--features hpc`.
- The `munge` check runs `munge -n | unmunge` to verify a round-trip authentication.
- The `gpu` check only runs if `nvidia-smi` is found on the target.
- The `infiniband` check only runs if `ibstat` is found on the target.
- When `fail_on_error` is `false` (the default), failed checks are reported but the module itself succeeds.
- In check mode, no remote commands are executed.
