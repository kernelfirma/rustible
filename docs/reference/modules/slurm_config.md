---
summary: Reference for the slurm_config module that manages Slurm configuration files.
read_when: You need to generate or update slurm.conf and related files from playbooks.
---

# slurm_config - Manage Slurm Configuration

## Synopsis

Manages Slurm workload manager configuration files including `slurm.conf`,
`cgroup.conf`, and `gres.conf`. Handles cluster-wide settings such as partition
definitions, node lists, and default scheduling parameters.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter         | Required | Default               | Type   | Description                                                     |
|-------------------|----------|-----------------------|--------|-----------------------------------------------------------------|
| state             | no       | `present`             | string | Desired state: `present` to write config, `absent` to remove.  |
| cluster_name      | no       | -                     | string | Slurm cluster name.                                            |
| control_machine   | no       | -                     | string | Hostname of the Slurm controller node.                          |
| partitions        | no       | -                     | list   | List of partition definitions.                                  |
| nodes             | no       | -                     | list   | List of node definitions with resources.                        |
| default_partition | no       | -                     | string | Name of the default partition.                                  |
| slurm_conf_path   | no       | `/etc/slurm/slurm.conf` | string | Path to the slurm.conf file to manage.                       |

## Return Values

| Key    | Type   | Description                                 |
|--------|--------|---------------------------------------------|
| status | string | Current implementation status (`stub`).      |

## Examples

```yaml
- name: Configure Slurm cluster
  slurm_config:
    state: present
    cluster_name: mycluster
    control_machine: slurmctl01
    default_partition: compute
    partitions:
      - name: compute
        nodes: node[001-100]
        max_time: "48:00:00"
        default: true
      - name: gpu
        nodes: gpu[01-08]
        max_time: "24:00:00"
    nodes:
      - name: node[001-100]
        cpus: 64
        memory: 256000
      - name: gpu[01-08]
        cpus: 64
        memory: 512000
        gres: gpu:4

- name: Remove Slurm configuration
  slurm_config:
    state: absent
```

## Notes

- Requires building with `--features hpc,slurm`.
- Currently a stub implementation; config generation logic is planned.
- After changing configuration, use the `slurm_ops` module with `action: reconfigure`.
- The module does not manage the Slurm daemon itself; use a service module for that.
