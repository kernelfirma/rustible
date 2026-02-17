---
summary: Reference for the hpc_toolchain module that installs curated sets of HPC development and diagnostic tools.
read_when: You need to install HPC build tools, performance tools, debug tools, or RDMA userland packages from playbooks.
---

# hpc_toolchain - Install HPC Toolchain Sets

## Synopsis

Installs curated sets of HPC development and diagnostic tools. Each set maps to OS-specific packages for RHEL-family and Debian-family distributions. Multiple sets can be installed in a single invocation.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| sets | **yes** | - | list(string) | List of toolchain set names to install. Must contain at least one entry. |

### Available Toolchain Sets

| Set Name | RHEL Packages | Debian Packages |
|----------|---------------|-----------------|
| `build_essentials` | gcc, gcc-c++, make, cmake, autoconf, automake, libtool | build-essential, cmake, autoconf, automake, libtool |
| `perf_tools` | perf, strace, ltrace, sysstat, htop | linux-tools-generic, strace, ltrace, sysstat, htop |
| `debug_tools` | gdb, valgrind, elfutils | gdb, valgrind, elfutils |
| `rdma_userland` | rdma-core, libibverbs-utils, librdmacm-utils | rdma-core, ibverbs-utils, rdmacm-utils |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether any packages were installed |
| msg | string | Status message |
| data.changes | array | List of changes applied per set |
| data.installed_sets | array | Names of all sets that are now installed |
| data.os_family | string | Detected OS family (`"rhel"` or `"debian"`) |

## Examples

```yaml
- name: Install build essentials and performance tools
  hpc_toolchain:
    sets:
      - build_essentials
      - perf_tools

- name: Install all HPC toolchain sets
  hpc_toolchain:
    sets:
      - build_essentials
      - perf_tools
      - debug_tools
      - rdma_userland

- name: Install RDMA userland only
  hpc_toolchain:
    sets:
      - rdma_userland
```

## Notes

- Requires building with `--features hpc`.
- Supports RHEL-family (dnf) and Debian-family (apt) distributions.
- Set names are validated before any installation; an invalid name causes the module to fail immediately.
- Idempotent: sets already fully installed are skipped without reporting a change.
- Parallelization hint: `HostExclusive` (one invocation per host at a time).
