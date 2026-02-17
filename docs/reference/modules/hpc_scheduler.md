---
summary: Reference for the hpc_scheduler abstraction layer that provides a common interface for Slurm and PBS Pro backends.
read_when: You need to understand the scheduler abstraction used by hpc_job, hpc_queue, and hpc_server modules.
---

# hpc_scheduler - Unified Scheduler Abstraction

## Synopsis

Provides a common trait interface (`HpcScheduler`) and shared types so that playbooks can use `hpc_job`, `hpc_queue`, and `hpc_server` modules with either Slurm or PBS Pro without scheduler-specific parameters. This is not a standalone module but the underlying abstraction layer used by the scheduler-agnostic HPC modules.

## Classification

**Default** - HPC module. Requires `hpc` feature flag. Additional feature flags: `slurm` for Slurm backend, `pbs` for PBS Pro backend.

## Auto-Detection

When the `scheduler` parameter is set to `"auto"` (the default), the abstraction layer probes the remote host:

1. Checks for `scontrol` on `$PATH` (Slurm)
2. Checks for `qstat` on `$PATH` (PBS Pro)
3. Uses the first scheduler found

## Shared Types

| Type | Description |
|------|-------------|
| `JobState` | Common job states: `Queued`, `Running`, `Held`, `Suspended`, `Completed`, `Failed`, `Cancelled`, `Unknown` |
| `JobInfo` | Scheduler-agnostic job information (id, name, state, queue, owner, nodes, cpus, walltime) |
| `QueueInfo` | Scheduler-agnostic queue information (name, state, total_jobs) |
| `ServerInfo` | Scheduler-agnostic server/cluster information (scheduler name, attributes) |

## State Mapping

### Slurm States

| Slurm State | Mapped State |
|-------------|--------------|
| PENDING / PD | Queued |
| RUNNING / R | Running |
| SUSPENDED / S | Suspended |
| COMPLETED / CD | Completed |
| FAILED / F | Failed |
| CANCELLED / CA | Cancelled |
| TIMEOUT / TO | Failed |
| NODE_FAIL / NF | Failed |
| PREEMPTED / PR | Cancelled |
| HELD | Held |

### PBS States

| PBS State | Mapped State |
|-----------|--------------|
| Q / W | Queued |
| R / E / B | Running |
| H | Held |
| S / U / T | Suspended |
| F | Completed |
| X | Cancelled |

## Notes

- Requires building with `--features hpc`.
- Slurm backend requires `--features slurm`.
- PBS Pro backend requires `--features pbs`.
- This module is not invoked directly; use `hpc_job`, `hpc_queue`, or `hpc_server` instead.
- All three scheduler-agnostic modules accept a `scheduler` parameter: `"slurm"`, `"pbs"`, or `"auto"`.
