---
summary: Reference for the nvidia_driver module that manages NVIDIA GPU driver installation and configuration.
read_when: You need to install, update, or remove NVIDIA GPU drivers with DKMS and nouveau blacklisting from playbooks.
---

# nvidia_driver - NVIDIA GPU Driver Management

## Synopsis

Install and manage NVIDIA GPU drivers with version pinning, DKMS kernel module support, nouveau driver blacklisting, and repository management. Supports both RHEL-family and Debian-family Linux distributions.

## Classification

**Default** - HPC module. Requires `hpc` and `gpu` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| version | no | null | string | Specific driver version branch to install (e.g., `535`, `550`). Uses prefix matching. |
| state | no | "present" | string | Desired state: `present` (install/ensure) or `absent` (remove) |
| dkms | no | true | boolean | Enable DKMS support for automatic kernel module rebuilds |
| blacklist_nouveau | no | true | boolean | Blacklist the open-source nouveau driver and rebuild initramfs |
| repo_url | no | null | string | Custom repository URL for driver packages (overrides the default NVIDIA CUDA repository) |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of changes applied (e.g., repository added, driver installed, nouveau blacklisted) |
| data.driver_info | object | GPU information from nvidia-smi after installation |
| data.driver_info.gpu_name | string | GPU model name |
| data.driver_info.driver_version | string | Installed driver version |
| data.driver_info.compute_capability | string | GPU compute capability |
| data.driver_info.gpu_count | integer | Number of GPUs detected |

## Examples

```yaml
- name: Install latest NVIDIA driver with DKMS
  nvidia_driver:
    state: present

- name: Install specific driver version
  nvidia_driver:
    version: "535"
    state: present
    dkms: true
    blacklist_nouveau: true

- name: Install driver from custom repository
  nvidia_driver:
    state: present
    repo_url: "https://internal-mirror.example.com/nvidia/repo"

- name: Install driver without DKMS
  nvidia_driver:
    version: "550"
    state: present
    dkms: false

- name: Remove NVIDIA driver and restore nouveau
  nvidia_driver:
    state: absent
```

## Notes

- Requires building with `--features hpc,gpu` (or `full-hpc`).
- Supports RHEL-family (Rocky, AlmaLinux, CentOS, Fedora) and Debian-family (Ubuntu, Debian) distributions.
- When `state: present`, the module performs these steps in order: add NVIDIA repository, install driver package, blacklist nouveau, load the nvidia kernel module, and verify via nvidia-smi.
- When `state: absent`, the module removes driver packages and optionally removes the nouveau blacklist (restoring initramfs).
- Version matching uses prefix comparison: specifying `535` matches any installed version starting with `535` (e.g., `535.183.01`).
- If the nvidia kernel module cannot be loaded (e.g., nouveau is still active), a note is added to the output rather than failing -- a reboot may be required.
- Supports check mode for all operations.
