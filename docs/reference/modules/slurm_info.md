---
summary: Reference for the slurm_info module that gathers Slurm cluster state as structured facts.
read_when: You need to query Slurm cluster information (nodes, jobs, partitions, accounts) from playbooks.
---

# slurm_info - Gather Slurm Cluster Facts

## Synopsis

Gathers Slurm cluster state as structured facts from Slurm CLI commands (`sinfo`,
`squeue`, `sacctmgr`). Supports querying nodes, jobs, partitions, accounts, and
a cluster-wide summary. Output is returned as structured JSON data suitable for
use in subsequent playbook tasks.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter | Required | Default | Type   | Description                                                                          |
|-----------|----------|---------|--------|--------------------------------------------------------------------------------------|
| gather    | yes      | -       | string | What to gather: `nodes`, `jobs`, `partitions`, `accounts`, or `cluster`.             |
| partition | no       | -       | string | Filter by partition name. Applies to `nodes`, `jobs`, and `partitions` gather types. |
| node      | no       | -       | string | Filter by node name. Applies to `nodes` gather type.                                 |
| user      | no       | -       | string | Filter by user name. Applies to `jobs` gather type.                                  |
| state     | no       | -       | string | Filter by state (e.g. `idle`, `running`). Applies to `jobs` gather type.             |

## Return Values

| Key     | Type    | Description                                      |
|---------|---------|--------------------------------------------------|
| changed | boolean | Always `false` (read-only module).               |
| msg     | string  | Status message with count of gathered items.     |
| data    | object  | Structured data depending on gather type.        |

### Data by gather type

**nodes**: `{ nodes: [...], count: N }` - Each node has `name`, `state`, `cpus`, `memory`, `partition`, `reason`, `load`, `free_mem`.

**jobs**: `{ jobs: [...], count: N }` - Each job has `job_id`, `name`, `user`, `state`, `partition`, `nodes`, `cpus`, `time_limit`, `time_used`, `reason`.

**partitions**: `{ partitions: [...], count: N }` - Each partition has `name`, `avail`, `nodes_aiot`, `cpus`, `memory`, `time_limit`, `gres`.

**accounts**: `{ accounts: [...], count: N }` - Each account has `account`, `description`, `organization`.

**cluster**: `{ node_states: {...}, job_states: {...}, partitions: [...], partition_count: N }` - Aggregated cluster summary.

## Examples

```yaml
- name: Gather all node information
  slurm_info:
    gather: nodes

- name: Gather nodes in a specific partition
  slurm_info:
    gather: nodes
    partition: gpu

- name: Gather running jobs for a user
  slurm_info:
    gather: jobs
    user: jdoe
    state: running

- name: Gather partition information
  slurm_info:
    gather: partitions

- name: Gather cluster summary
  slurm_info:
    gather: cluster
```

## Notes

- Requires building with `--features hpc,slurm`.
- This is a read-only module; it never reports `changed: true`.
- The `cluster` gather type runs multiple commands to produce an aggregated summary including node state counts, job state counts, and partition list.
- Filters are additive: specifying `partition` and `state` together narrows the results.
