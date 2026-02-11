---
summary: Reference for the hpc_baseline module that validates and applies HPC cluster baseline configuration.
read_when: You need to validate or enforce baseline tuning on HPC cluster nodes from playbooks.
---

# hpc_baseline - HPC Cluster Baseline Configuration

## Synopsis

Validates and reports HPC cluster baseline configuration including system limits,
sysctl parameters, required directories, and time synchronization. Designed as the
first step in provisioning compute or login nodes for an HPC environment.

## Classification

**LocalLogic** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter       | Required | Default      | Type   | Description                                                        |
|-----------------|----------|--------------|--------|--------------------------------------------------------------------|
| state           | no       | `present`    | string | Desired state: `present` to apply, `absent` to remove tuning.     |
| tuning_profile  | no       | -            | string | Named tuning profile (e.g. `throughput`, `latency`).               |
| huge_pages      | no       | -            | string | HugePages configuration value (e.g. `1024`).                      |
| numa_balancing  | no       | -            | bool   | Enable or disable automatic NUMA balancing.                        |
| cpu_governor    | no       | -            | string | CPU frequency governor (e.g. `performance`, `powersave`).          |

## Return Values

| Key               | Type   | Description                                        |
|-------------------|--------|----------------------------------------------------|
| status            | string | Current implementation status (`stub`).             |
| supported_distros | list   | List of supported Linux distributions.              |

## Examples

```yaml
- name: Validate HPC baseline configuration
  hpc_baseline:

- name: Apply performance tuning profile
  hpc_baseline:
    state: present
    tuning_profile: throughput
    huge_pages: "1024"
    numa_balancing: false
    cpu_governor: performance
```

## Notes

- Requires building with `--features hpc`.
- Currently a stub implementation; validation logic is planned.
- Supported distributions: Rocky 9, AlmaLinux 9, Ubuntu 22.04.
- Intended to be run early in a provisioning playbook before scheduler or filesystem setup.
