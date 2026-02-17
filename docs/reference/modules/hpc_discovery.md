---
summary: Reference for the hpc_discovery module that discovers and inventories bare-metal hardware on HPC nodes.
read_when: You need to collect hardware inventory (CPU, GPU, NIC, storage, memory, BMC) from HPC nodes in playbooks.
---

# hpc_discovery - Hardware Discovery and Inventory

## Synopsis

Discovers and inventories bare-metal hardware on HPC nodes. Parses `/proc/cpuinfo`, `lspci`, `ip link`, `lsblk`, `/proc/meminfo`, and `ipmitool lan print` to build a structured `HardwareInventory` with CPU, GPU, network interface, storage device, memory, and BMC information.

## Classification

**Default** - HPC module. Requires `hpc` feature flag. Classification: `RemoteCommand`.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| gather | no | all categories | list(string) | List of hardware categories to gather. Values: `"cpu"`, `"gpu"`, `"nic"`, `"storage"`, `"memory"`, `"bmc"`. When omitted, all categories are collected. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Always `false` (read-only module) |
| msg | string | Status message |
| data.inventory | object | Structured hardware inventory |
| data.inventory.cpu_count | integer | Number of logical CPUs |
| data.inventory.cpu_model | string | CPU model name |
| data.inventory.gpu_count | integer | Number of GPUs detected |
| data.inventory.gpu_models | array | List of GPU model name strings |
| data.inventory.nic_count | integer | Number of network interfaces (excluding loopback) |
| data.inventory.nics | array | List of NIC objects with `name`, `mac`, and `state` fields |
| data.inventory.storage_devices | array | List of storage device objects with `name`, `size`, `device_type`, and `model` fields |
| data.inventory.total_memory_mb | integer | Total system memory in megabytes |
| data.inventory.bmc_address | string or null | BMC/IPMI IP address if detected |

## Examples

```yaml
- name: Discover all hardware
  hpc_discovery:

- name: Discover only CPU and GPU
  hpc_discovery:
    gather:
      - cpu
      - gpu

- name: Discover network and BMC for inventory
  hpc_discovery:
    gather:
      - nic
      - bmc
      - memory
```

## Notes

- Requires building with `--features hpc`.
- GPU discovery prefers `nvidia-smi` for NVIDIA GPUs; falls back to `lspci` for VGA/3D controller detection.
- NIC discovery uses `ip -o link show` and filters out the loopback interface.
- Storage discovery uses `lsblk -d -n -o NAME,SIZE,TYPE,MODEL`.
- BMC address discovery requires `ipmitool` to be installed on the target.
- This module is read-only and never reports `changed: true`.
- In check mode, no remote commands are executed.
