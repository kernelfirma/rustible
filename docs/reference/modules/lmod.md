---
summary: Reference for the lmod module that manages Lmod environment modules.
read_when: You need to install or configure Lmod environment modules on HPC nodes from playbooks.
---

# lmod - Manage Lmod Environment Modules

## Synopsis

Manages Lmod installation and module path configuration on HPC nodes. Lmod is the
standard environment module system used on most HPC clusters to let users load and
switch between software stacks.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter    | Required | Default    | Type   | Description                                                     |
|--------------|----------|------------|--------|-----------------------------------------------------------------|
| name         | no       | -          | string | Module name to manage (e.g. `gcc`, `openmpi`).                  |
| state        | no       | `present`  | string | Desired state: `present`, `absent`, `loaded`, `unloaded`.       |
| version      | no       | -          | string | Specific module version to target.                              |
| module_path  | no       | -          | string | Additional module path to add to `MODULEPATH`.                  |

## Return Values

| Key    | Type   | Description                                 |
|--------|--------|---------------------------------------------|
| status | string | Current implementation status (`stub`).      |

## Examples

```yaml
- name: Ensure Lmod is installed and configured
  lmod:
    state: present

- name: Add a custom module path
  lmod:
    module_path: /opt/custom/modules
    state: present

- name: Ensure a module is loaded
  lmod:
    name: gcc
    version: "12.3.0"
    state: loaded
```

## Notes

- Requires building with `--features hpc`.
- Currently a stub implementation; Lmod install and load logic is planned.
- The `loaded`/`unloaded` states affect the runtime environment, not installation.
- Module paths are typically configured in `/etc/lmod/modulespath`.
