---
summary: Reference for the scheduler_orchestration module that implements the drain-operate-resume pattern for Slurm node maintenance.
read_when: You need to perform rolling maintenance on Slurm nodes using the drain-operate-resume pattern.
---

# scheduler_orchestration - Slurm Maintenance Orchestration

## Synopsis

Implements the drain-operate-resume pattern for Slurm node maintenance windows.
Generates maintenance plans that drain nodes (stop accepting new jobs, wait for
running jobs to complete), perform maintenance operations, and then resume nodes.
This is a library/planning module that produces command sequences rather than
executing them directly.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Configuration

The module is configured via an `OrchestrationConfig` struct rather than playbook
parameters. The following configuration options are available:

| Option          | Default       | Type     | Description                                                  |
|-----------------|---------------|----------|--------------------------------------------------------------|
| drain_timeout   | `1h`          | duration | Maximum time to wait for drain to complete.                  |
| drain_reason    | `maintenance` | string   | Reason string passed to `scontrol drain`.                    |
| force_drain     | `false`       | boolean  | Whether to force-drain (cancel running jobs).                |
| poll_interval   | `10s`         | duration | Poll interval for checking drain status.                     |

## Maintenance Steps

Each node in a maintenance plan goes through four steps:

| Step         | Description                                               |
|--------------|-----------------------------------------------------------|
| Drain        | Issue `scontrol drain` to stop new job scheduling.        |
| WaitDrained  | Poll `scontrol show node` until node is fully drained.    |
| Operate      | Execute user-supplied maintenance commands (empty placeholder). |
| Resume       | Issue `scontrol resume` to make the node available again. |

## Return Values

| Key      | Type    | Description                                            |
|----------|---------|--------------------------------------------------------|
| node     | string  | Node name.                                             |
| state    | string  | Final drain state: `Active`, `Draining`, `Drained`, `Maintenance`, `Resumed`, or `Failed`. |
| success  | boolean | Whether the full drain-operate-resume cycle succeeded. |
| messages | list    | Any messages or warnings generated during the cycle.   |

## Examples

```yaml
# Conceptual usage - the module generates commands for orchestration
- name: Plan maintenance for compute nodes
  scheduler_orchestration:
    nodes:
      - node01
      - node02
      - node03
    drain_timeout: "2h"
    drain_reason: "Firmware update"
    poll_interval: "15s"
```

## Node State Parsing

The module parses `scontrol show node` output to determine drain state:

| scontrol State              | Parsed As    |
|-----------------------------|--------------|
| `IDLE`, `MIXED`, `ALLOCATED`| Active       |
| `DRAINING`                  | Draining     |
| `DRAINED`, `IDLE+DRAIN`    | Drained      |
| `DOWN`                      | Maintenance  |
| Other                       | Failed       |

## Notes

- Requires building with `--features hpc,slurm`.
- This module generates maintenance plans (command sequences) rather than executing commands directly. Integration with a connection/executor is required for actual execution.
- The drain-operate-resume pattern ensures minimal disruption: nodes stop accepting new jobs but allow running jobs to complete before maintenance begins.
- For multi-node maintenance, the plan processes nodes sequentially (4 steps per node).
- Use in conjunction with the `partition_policy` module for rolling updates that respect cluster capacity constraints.
