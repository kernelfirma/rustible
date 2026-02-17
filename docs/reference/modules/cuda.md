---
summary: Reference for the cuda_toolkit module that manages CUDA Toolkit installations.
read_when: You need to install, configure, or remove CUDA Toolkit versions from playbooks.
---

# cuda_toolkit - CUDA Toolkit Management

## Synopsis

Manage multi-version CUDA Toolkit installations with alternatives-based version switching and environment setup. Handles installation directories, environment variables (`CUDA_HOME`, `PATH`, `LD_LIBRARY_PATH`), and the `update-alternatives` system for default version selection.

## Classification

**Default** - HPC module. Requires `hpc` and `gpu` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| version | yes | - | string | CUDA version to manage (e.g., `12.3`, `11.8`) |
| state | no | "present" | string | Desired state: `present` (install/ensure) or `absent` (remove) |
| install_path | no | "/usr/local/cuda-{version}" | string | Base installation path for the CUDA Toolkit |
| set_default | no | false | boolean | Set this version as the default via `update-alternatives` |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of changes applied (e.g., toolkit installed, environment file updated, alternatives configured) |
| data.version | string | CUDA version that was managed |

## Examples

```yaml
- name: Install CUDA 12.3
  cuda_toolkit:
    version: "12.3"
    state: present
    set_default: true

- name: Install CUDA 11.8 as secondary version
  cuda_toolkit:
    version: "11.8"
    state: present

- name: Install CUDA to custom path
  cuda_toolkit:
    version: "12.3"
    install_path: /opt/cuda-12.3
    set_default: true

- name: Remove CUDA 11.8
  cuda_toolkit:
    version: "11.8"
    state: absent
```

## Notes

- Requires building with `--features hpc,gpu` (or `full-hpc`).
- Supports RHEL-family and Debian-family distributions.
- When `state: present`, the module creates the installation directory structure, configures `update-alternatives` if `set_default` is true, and writes `/etc/profile.d/cuda.sh` with `CUDA_HOME`, `PATH`, and `LD_LIBRARY_PATH` exports.
- When `state: absent`, the module removes the installation directory and the environment profile file.
- Installation is idempotent: if the install path already contains a `bin/` directory, it is considered installed.
- The environment file is updated only if the content has changed.
- Supports check mode for all operations.
