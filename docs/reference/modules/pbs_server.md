---
summary: Reference for the pbs_server module that manages PBS Pro server-level configuration.
read_when: You need to query or set PBS Pro server attributes or manage custom resources from playbooks.
---

# pbs_server - PBS Pro Server Configuration

## Synopsis

Query and set PBS Pro server attributes via qmgr, and manage custom server resources. Provides idempotent configuration of server-level settings such as scheduling, default queue, job limits, and custom resource definitions.

## Classification

**Default** - HPC module. Requires `hpc` and `pbs` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | yes | - | string | Operation to perform: `query`, `set_attributes`, or `manage_resources` |
| attributes | no | null | object | JSON object of server attributes or resource definitions (name -> type) |
| default_queue | no | null | string | Default queue name for the server |
| scheduling | no | null | string | Enable or disable scheduling: `True` or `False` |
| node_fail_requeue | no | null | string | Requeue jobs on node failure: `True` or `False` |
| max_run | no | null | string | Maximum running jobs across the server |
| max_queued | no | null | string | Maximum queued jobs across the server |
| query_other_jobs | no | null | string | Allow users to query other users' jobs: `True` or `False` |
| resources_default_walltime | no | null | string | Default walltime for all jobs on the server |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.server | object | Current server attributes as key-value pairs (from query, set_attributes) |
| data.changes | object | Map of attribute changes applied (from set_attributes) |
| data.created | array | List of resource names created (from manage_resources) |
| data.skipped | array | List of resource names that already existed (from manage_resources) |

## Examples

```yaml
- name: Query current server configuration
  pbs_server:
    action: query

- name: Set server attributes
  pbs_server:
    action: set_attributes
    default_queue: batch
    scheduling: "True"
    node_fail_requeue: "True"
    max_run: "500"
    resources_default_walltime: "01:00:00"

- name: Set arbitrary server attributes
  pbs_server:
    action: set_attributes
    attributes:
      log_events: "511"
      mail_from: "pbs@cluster.example.com"

- name: Create custom server resources
  pbs_server:
    action: manage_resources
    attributes:
      ngpus: "long"
      scratch: "size"
      software: "string_array"
```

## Notes

- Requires building with `--features hpc,pbs` (or `full-hpc`).
- The `query` action reads from `qmgr -c "print server"` and parses all `set server` lines into key-value pairs.
- The `set_attributes` action computes a diff against current server state and only applies attributes that differ.
- The `manage_resources` action uses the `attributes` parameter as a mapping of resource name to PBS resource type (e.g., `long`, `size`, `string`, `string_array`). It skips resources that already exist.
- Named parameters (e.g., `default_queue`, `scheduling`) map to their PBS server attribute names. The `attributes` object can also be used for arbitrary attributes.
- Supports check mode for all actions.
