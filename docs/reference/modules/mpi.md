---
summary: Reference for the mpi module that manages MPI library installations.
read_when: You need to install or configure MPI libraries on HPC nodes from playbooks.
---

# mpi - Manage MPI Installations

## Synopsis

Manages MPI library installation and configuration for HPC environments. Supports
OpenMPI, MPICH, and Intel MPI implementations. Configures MPI prefix paths and
optional compile-time settings.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter       | Required | Default    | Type   | Description                                                        |
|-----------------|----------|------------|--------|--------------------------------------------------------------------|
| implementation  | no       | `openmpi`  | string | MPI implementation: `openmpi`, `mpich`, `intel-mpi`.               |
| version         | no       | -          | string | Specific version to install.                                       |
| state           | no       | `present`  | string | Desired state: `present`, `absent`.                                |
| prefix          | no       | -          | string | Installation prefix path (e.g. `/opt/openmpi`).                   |
| configure_opts  | no       | -          | string | Additional configure/build options.                                |

## Return Values

| Key    | Type   | Description                                             |
|--------|--------|---------------------------------------------------------|
| status | string | Current implementation status (`stub`).                  |
| flavor | string | The MPI implementation that was configured.               |

## Examples

```yaml
- name: Install OpenMPI
  mpi:
    implementation: openmpi
    state: present

- name: Install Intel MPI with a custom prefix
  mpi:
    implementation: intel-mpi
    version: "2021.9"
    prefix: /opt/intel/mpi
    state: present

- name: Remove MPICH installation
  mpi:
    implementation: mpich
    state: absent
```

## Notes

- Requires building with `--features hpc`.
- Currently a stub implementation; build-from-source logic is planned.
- The `flavor` parameter in source code is mapped from `implementation` in playbooks.
- Typically used alongside the `lmod` module to expose MPI as an environment module.
