---
summary: Reference for the nvidia_gpu module that manages NVIDIA GPU driver installation and configuration.
read_when: You need to install GPU drivers or configure NVIDIA GPUs on HPC nodes from playbooks.
---

# nvidia_gpu - NVIDIA GPU Configuration

## Synopsis

Manages NVIDIA GPU driver installation and runtime configuration. Supports setting
persistence mode, compute mode, ECC mode, and power limits via `nvidia-smi`.

## Classification

**Default** - HPC module. Requires `hpc` and `gpu` feature flags.

## Parameters

| Parameter        | Required | Default    | Type   | Description                                                        |
|------------------|----------|------------|--------|--------------------------------------------------------------------|
| state            | no       | `present`  | string | Desired state: `present` to configure, `absent` to remove driver.  |
| driver_version   | no       | -          | string | Specific driver version to install (e.g. `535.129.03`).            |
| persistence_mode | no       | -          | bool   | Enable or disable GPU persistence mode.                            |
| compute_mode     | no       | -          | string | GPU compute mode: `default`, `exclusive_thread`, `exclusive_process`, `prohibited`. |
| gpu_id           | no       | -          | string | Target a specific GPU by index or UUID.                            |
| ecc_mode         | no       | -          | bool   | Enable or disable ECC memory (requires reboot).                    |
| power_limit      | no       | -          | u32    | Power limit in watts.                                              |
| gres_config      | no       | `false`    | bool   | Generate Slurm GRES entries from detected GPUs.                    |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of applied changes |
| data.gpu_count | integer | Number of GPUs detected |
| data.gpus | array | GPU inventory (index, name, uuid, pci bus, memory) |
| data.gres_config | array | Slurm GRES config entries (when `gres_config: true`) |

## Examples

```yaml
- name: Install NVIDIA drivers
  nvidia_gpu:
    state: present
    driver_version: "535.129.03"

- name: Enable persistence mode on all GPUs
  nvidia_gpu:
    persistence_mode: true

- name: Set exclusive process compute mode on GPU 0
  nvidia_gpu:
    gpu_id: "0"
    compute_mode: exclusive_process

- name: Set power limit to 300W
  nvidia_gpu:
    gpu_id: "0"
    power_limit: 300
```

## Notes

- Requires building with `--features hpc,gpu`.
- Uses `nvidia-smi` for configuration and inventory; ensure it is on PATH.
- ECC mode changes require a GPU reset or node reboot to take effect.
- Persistence mode prevents the driver from unloading when no clients are active.
- Power limit values must be within the GPU's supported range.
