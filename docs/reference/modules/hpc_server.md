---
summary: Reference for the hpc_server module that provides unified scheduler server/cluster configuration for Slurm and PBS Pro.
read_when: You need to query or configure HPC scheduler server attributes from playbooks.
---

# hpc_server - Unified HPC Server Configuration

## Synopsis

Scheduler-agnostic server query and configuration that works with Slurm or PBS Pro. Supports querying server attributes and setting server-level configuration. The scheduler backend is selected via the `scheduler` parameter or auto-detected.

## Classification

**Default** - HPC module. Requires `hpc` feature flag. Requires `slurm` or `pbs` feature flag for the respective backend.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | **yes** | - | string | Server action: `"query"` or `"set_attributes"`. |
| scheduler | no | `"auto"` | string | Scheduler backend: `"slurm"`, `"pbs"`, or `"auto"`. |
| attributes | no | `null` | object | JSON object of server attributes to set (for `set_attributes` action). |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether server attributes were modified |
| msg | string | Status message |
| data.scheduler | string | Scheduler backend used (`"slurm"` or `"pbs"`) |
| data.server | object | Server information object with `scheduler`, `attributes`, and `raw` fields (for `query` action) |

## Examples

```yaml
- name: Query scheduler server attributes
  hpc_server:
    action: query

- name: Query with explicit PBS backend
  hpc_server:
    action: query
    scheduler: pbs

- name: Set server attributes
  hpc_server:
    action: set_attributes
    attributes:
      scheduling: "True"
      default_queue: batch
```

## Notes

- Requires building with `--features hpc` and either `--features slurm` or `--features pbs`.
- When `scheduler` is `"auto"`, the module probes for `scontrol` (Slurm) then `qstat` (PBS) on the target.
- Parallelization hint: `GlobalExclusive` (only one invocation at a time cluster-wide, since server operations affect the entire scheduler).
