---
summary: Reference for the partition_policy module that defines rolling update strategies for Slurm partitions.
read_when: You need to plan rolling updates across Slurm partitions while maintaining minimum cluster availability.
---

# partition_policy - Partition Rolling Update Policy

## Synopsis

Defines batch strategies for updating nodes within Slurm partitions without
fully draining cluster capacity. Calculates safe batch sizes based on partition
size, minimum availability requirements, and parallelism limits. This is a
planning/library module that produces batch schedules for use by orchestration
modules.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Configuration

### PartitionPolicy

| Option         | Default | Type   | Description                                          |
|----------------|---------|--------|------------------------------------------------------|
| partition_name | -       | string | Partition name in Slurm.                             |
| total_nodes    | -       | usize  | Total number of nodes in the partition.              |
| batch          | see below | object | Rolling batch configuration.                       |

### RollingBatch

| Option               | Default | Type    | Description                                              |
|----------------------|---------|---------|----------------------------------------------------------|
| max_parallel         | `5`     | usize   | Maximum number of nodes to update simultaneously.        |
| min_available_pct    | `75.0`  | float   | Minimum percentage of partition that must remain online.  |
| respect_reservations | `true`  | boolean | Whether to respect job reservations during updates.      |

## Batch Calculation

The maximum number of nodes that can be taken offline simultaneously is:

```
max_offline = min(max_parallel, total_nodes - ceil(total_nodes * min_available_pct / 100))
```

For example, with 100 nodes, `min_available_pct=75.0`, and `max_parallel=5`:
- Minimum online: ceil(100 * 0.75) = 75
- Max offline: 100 - 75 = 25, capped at max_parallel = 5
- Batch size: 5

## Return Values

| Key     | Type  | Description                                           |
|---------|-------|-------------------------------------------------------|
| batches | list  | List of node batches, each a list of node names.      |

## Examples

```yaml
# Conceptual usage - plan rolling update batches
- name: Plan rolling update for compute partition
  partition_policy:
    partition_name: compute
    total_nodes: 100
    batch:
      max_parallel: 5
      min_available_pct: 75.0
      respect_reservations: true
    nodes:
      - node01
      - node02
      - node03
      - node04
      - node05
      - node06
      - node07
      - node08
      - node09
      - node10
    # Result: [[node01..05], [node06..10]]
```

## Policy Validation

The `within_policy` method checks whether a given number of offline nodes is
acceptable:

| Cluster Size | min_available_pct | max_parallel | Effective Max Offline |
|--------------|-------------------|--------------|-----------------------|
| 100          | 75.0              | 5            | 5                     |
| 100          | 50.0              | 50           | 50                    |
| 20           | 75.0              | 3            | 3                     |
| 4            | 75.0              | 5            | 1                     |
| 1            | 75.0              | 5            | 0                     |

## Notes

- Requires building with `--features hpc,slurm`.
- This is a planning module; it computes batch schedules but does not execute any commands.
- Use in conjunction with `scheduler_orchestration` to execute the drain-operate-resume cycle for each batch.
- The `PartitionPolicyModule` can manage policies for multiple partitions simultaneously.
- A single-node partition with `min_available_pct=75.0` will have `max_offline=0`, effectively preventing any rolling update without adjusting the policy.
