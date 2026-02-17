---
summary: Reference for the hpc_facts module that gathers HPC-specific system facts including CPU, NUMA, hugepages, GPU, and InfiniBand.
read_when: You need to collect hardware and system facts from HPC nodes in playbooks.
---

# hpc_facts - Gather HPC System Facts

## Synopsis

Gathers HPC-specific system facts and returns them as structured data. Collects information about CPU features, NUMA topology, hugepages configuration, GPU inventory (via `nvidia-smi`), and InfiniBand devices (via `ibstat` or `lspci`). Facts are returned under the `hpc_facts` data key.

## Classification

**Default** - HPC module. Requires `hpc` feature flag. Classification: `RemoteCommand`.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| gather | no | all categories | list(string) | List of fact categories to collect. Values: `"cpu"`, `"numa"`, `"hugepages"`, `"gpu"`, `"infiniband"`. When omitted, all available categories are gathered. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Always `false` (read-only module) |
| msg | string | Status message |
| data.hpc_facts | object | Collected facts object containing category sub-objects |
| data.hpc_facts.cpu | object | CPU model, count, flags, and feature booleans (`has_avx`, `has_avx2`, `has_avx512f`, `has_sse4_2`) |
| data.hpc_facts.numa | object | NUMA node count and per-node cpulist/memory |
| data.hpc_facts.hugepages | object | Hugepages total, free, and size in KB |
| data.hpc_facts.gpu | object | GPU count, vendor, and device details (index, name, memory, driver, bus ID) |
| data.hpc_facts.infiniband | object | InfiniBand presence, device count, and device names |

## Examples

```yaml
- name: Gather all HPC facts
  hpc_facts:

- name: Gather only CPU and GPU facts
  hpc_facts:
    gather:
      - cpu
      - gpu

- name: Check InfiniBand and NUMA topology
  hpc_facts:
    gather:
      - infiniband
      - numa
```

## Notes

- Requires building with `--features hpc`.
- GPU facts require `nvidia-smi` to be installed on the target for NVIDIA GPU detection.
- InfiniBand facts prefer `ibstat` when available; falls back to `lspci` for Mellanox/InfiniBand device detection.
- This module is read-only and never reports `changed: true`.
- In check mode, no remote commands are executed.
