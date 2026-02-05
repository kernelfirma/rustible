# HPC Software Stack and Identity Requirements Matrix

Phase 2C of the HPC Initiative - Requirements for GPU/accelerator software, environment modules, identity management, and license servers.

## Table of Contents

1. [GPU and Accelerator Software](#1-gpu-and-accelerator-software)
2. [MPI Stacks](#2-mpi-stacks)
3. [Environment Modules](#3-environment-modules-lmod-spack-easybuild)
4. [Identity and Access Management](#4-identity-and-access-management)
5. [License Servers](#5-license-servers)
6. [Operational Workflows](#6-operational-workflows)
7. [Implementation Priorities](#7-implementation-priorities)

---

## 1. GPU and Accelerator Software

### 1.1 NVIDIA GPU Stack Components

| Component | Description | Version Coupling |
|-----------|-------------|------------------|
| **GPU Driver** | Kernel module for GPU access | Must match GPU hardware |
| **CUDA Toolkit** | Compiler, libraries, tools | Requires compatible driver |
| **cuDNN** | Deep learning primitives | CUDA version specific |
| **NCCL** | Multi-GPU communication | CUDA version specific |
| **TensorRT** | Inference optimization | CUDA/cuDNN dependent |
| **HPC SDK** | Compilers (nvcc, nvc, nvfortran) | Bundles CUDA components |

### 1.2 Driver Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Kernel Compatibility** | Driver matches kernel version | Module loads without error |
| **GPU Detection** | All GPUs visible | `nvidia-smi` shows all cards |
| **Persistence Mode** | Driver stays loaded | No first-job latency |
| **ECC Memory** | Error correction enabled | ECC status active |
| **Power/Thermal Limits** | Appropriate for cooling | Within safe thresholds |

### 1.3 CUDA Toolkit Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Multiple Versions** | Side-by-side CUDA installs | `/usr/local/cuda-{11.8,12.1,12.4}` |
| **Default Selection** | Symlink or module default | `/usr/local/cuda` points to preferred |
| **Library Paths** | Runtime library discovery | `LD_LIBRARY_PATH` or ldconfig |
| **Compiler Access** | nvcc in PATH | `nvcc --version` works |
| **Samples/Tests** | Validation utilities | `deviceQuery` passes |

### 1.4 CUDA Version Matrix

| CUDA Version | Min Driver | Max GCC | Key Features |
|--------------|------------|---------|--------------|
| **11.8** | 520.61 | 11 | Last CUDA 11.x |
| **12.0** | 525.60 | 12 | New PTX features |
| **12.1** | 530.30 | 12 | Hopper support |
| **12.4** | 550.54 | 13 | Current stable |
| **12.6** | 560.35 | 13 | Latest |

### 1.5 NCCL and Multi-GPU Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **NCCL Version** | Collective communication library | Compatible with CUDA version |
| **NVLink** | Direct GPU-GPU connection | Topology visible in `nvidia-smi topo` |
| **GPUDirect RDMA** | IB-GPU direct transfer | `nvidia_peermem` module loaded |
| **CUDA IPC** | Inter-process GPU memory | P2P access enabled |

### 1.6 GPU Configuration Files

| File | Purpose | Location |
|------|---------|----------|
| **nvidia.conf** | Kernel module options | `/etc/modprobe.d/` |
| **nvidia-persistenced** | Persistence daemon | systemd service |
| **fabricmanager** | NVSwitch management | systemd service (DGX) |
| **dcgm** | Data Center GPU Manager | systemd service |
| **cuda.sh** | Environment setup | `/etc/profile.d/` |

### 1.7 AMD ROCm Stack (Alternative)

| Component | Description | Equivalent |
|-----------|-------------|------------|
| **amdgpu** | Kernel driver | nvidia driver |
| **ROCm** | Runtime and tools | CUDA toolkit |
| **rocBLAS** | BLAS library | cuBLAS |
| **RCCL** | Collective communication | NCCL |
| **MIOpen** | Deep learning | cuDNN |

---

## 2. MPI Stacks

### 2.1 MPI Implementation Comparison

| Implementation | Maintainer | InfiniBand | Best For |
|----------------|------------|------------|----------|
| **OpenMPI** | Open community | UCX, native | General HPC, flexibility |
| **MPICH** | Argonne | UCX, OFI | Reference, compatibility |
| **Intel MPI** | Intel | OFI | Intel hardware optimization |
| **MVAPICH2** | Ohio State | Native IB | InfiniBand performance |

### 2.2 MPI Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Network Transport** | High-speed fabric support | UCX/OFI configured for IB |
| **Process Launch** | Job launcher integration | srun, mpiexec work |
| **Shared Filesystem** | Executable/library access | All nodes see same paths |
| **SSH or PMI** | Process management | Key-based SSH or PMI2/PMIx |
| **ABI Compatibility** | Library interop | Module-managed versions |

### 2.3 MPI Configuration

**OpenMPI:**
```bash
# Key environment variables
OMPI_MCA_btl=^tcp          # Disable TCP for IB clusters
OMPI_MCA_pml=ucx           # Use UCX for point-to-point
OMPI_MCA_osc=ucx           # Use UCX for one-sided
UCX_TLS=rc,sm,self         # Transport selection
```

**Intel MPI:**
```bash
# Key environment variables
I_MPI_FABRICS=shm:ofi      # Fabric selection
I_MPI_OFI_PROVIDER=mlx     # Mellanox provider
FI_PROVIDER=mlx            # libfabric provider
```

**MPICH:**
```bash
# Key environment variables
MPICH_CH4_OFI_CAPABILITY_SETS_DEBUG=1
MPICH_OFI_NIC_POLICY=GPU   # GPU-aware selection
```

### 2.4 MPI-GPU Integration (CUDA-aware MPI)

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **GPUDirect RDMA** | Direct GPU-IB transfer | `nvidia_peermem` loaded |
| **CUDA-aware Build** | MPI compiled with CUDA | `ompi_info | grep cuda` shows yes |
| **GDRCopy** | Fast GPU memory copy | `gdrcopy` module available |
| **Memory Registration** | Pin GPU buffers | `UCX_CUDA_COPY_DMABUF=y` |

---

## 3. Environment Modules (Lmod, Spack, EasyBuild)

### 3.1 Module System Comparison

| System | Language | Hierarchy | Build Tool |
|--------|----------|-----------|------------|
| **Environment Modules** | TCL | Flat | None |
| **Lmod** | Lua | Hierarchical | None |
| **Spack** | Python | Both | Yes (source) |
| **EasyBuild** | Python | Both | Yes (source) |

### 3.2 Lmod Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Installation** | Lmod package or source | `module --version` works |
| **Module Path** | Search directories | `MODULEPATH` set correctly |
| **Hierarchy** | Core/Compiler/MPI levels | Dependent modules hide/show |
| **Default Modules** | Initial environment | `/etc/profile.d/lmod.sh` |
| **Spider Cache** | Module database | `module spider <name>` fast |
| **User Modules** | Personal modulefiles | `~/modulefiles` in path |

### 3.3 Lmod Configuration Files

| File | Purpose | Location |
|------|---------|----------|
| **lmodrc.lua** | Lmod configuration | `/etc/lmod/` or `$LMOD_ROOT/etc/` |
| **.modulespath** | Default MODULEPATH | `/etc/` |
| **SitePackage.lua** | Site customizations | `$LMOD_ROOT/libexec/` |
| **admin.list** | Admin module visibility | `$LMOD_ROOT/etc/` |

### 3.4 Spack Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Installation** | Spack clone or package | `spack --version` works |
| **Compiler Detection** | System compilers found | `spack compiler find` populates |
| **Build Cache** | Pre-built binaries | Mirror configured if available |
| **Concretization** | Dependency resolution | Packages install correctly |
| **Lmod Integration** | Module generation | `spack module lmod refresh` |
| **Environments** | Reproducible sets | `spack.yaml` for environments |

### 3.5 Spack Configuration Files

| File | Purpose | Location |
|------|---------|----------|
| **config.yaml** | General settings | `~/.spack/` or `$SPACK_ROOT/etc/` |
| **compilers.yaml** | Compiler definitions | `~/.spack/` |
| **packages.yaml** | Package preferences | `~/.spack/` |
| **modules.yaml** | Module generation | `~/.spack/` |
| **mirrors.yaml** | Binary cache mirrors | `~/.spack/` |

### 3.6 EasyBuild Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Installation** | EasyBuild package | `eb --version` works |
| **Robot Paths** | Easyconfig search | `--robot-paths` configured |
| **Toolchains** | Compiler/MPI bundles | foss, intel, GCC toolchains |
| **Naming Scheme** | Module naming | EasyBuildMNS or custom |
| **Software Path** | Installation prefix | Writable shared location |

### 3.7 EasyBuild Configuration

| File | Purpose | Location |
|------|---------|----------|
| **config.cfg** | EasyBuild settings | `~/.config/easybuild/` |
| **easyconfigs/** | Build recipes | `$EASYBUILD_ROBOT_PATHS` |
| **toolchains/** | Toolchain definitions | Part of EasyBuild install |

### 3.8 Module Hierarchy Example

```
/opt/modules/
├── Core/                      # No dependencies
│   ├── gcc/13.2.0.lua
│   ├── intel/2024.lua
│   └── cuda/12.4.lua
├── Compiler/
│   └── gcc/13.2.0/           # Depends on gcc/13.2.0
│       ├── openmpi/5.0.lua
│       └── mpich/4.2.lua
└── MPI/
    └── gcc/13.2.0/
        └── openmpi/5.0/      # Depends on gcc + openmpi
            ├── hdf5/1.14.lua
            └── netcdf/4.9.lua
```

---

## 4. Identity and Access Management

### 4.1 Identity Stack Overview

| Component | Purpose | Protocol |
|-----------|---------|----------|
| **LDAP Server** | User/group directory | LDAP/LDAPS |
| **Kerberos KDC** | Authentication | Kerberos v5 |
| **SSSD** | Client-side caching | Local daemon |
| **PAM** | Authentication modules | System integration |
| **NSS** | Name service switch | System integration |

### 4.2 LDAP Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Server Connectivity** | Reach LDAP server | `ldapsearch` works |
| **TLS/SSL** | Encrypted connection | LDAPS (636) or StartTLS |
| **Base DN** | Search base | Correct for organization |
| **Bind Credentials** | Service account | Read access to user tree |
| **Schema** | User/group attributes | POSIX attributes available |
| **Replication** | HA LDAP servers | Multiple servers configured |

### 4.3 Kerberos Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **KDC Connectivity** | Reach Kerberos KDC | `kinit` obtains ticket |
| **DNS SRV Records** | KDC discovery | `_kerberos._tcp` records |
| **NTP Synchronization** | Time sync (<5 min skew) | `ntpstat` shows sync |
| **Keytab** | Host authentication | `/etc/krb5.keytab` present |
| **Realm** | Kerberos domain | Matches organization |
| **Cross-realm Trust** | Multi-domain | Trust relationships if needed |

### 4.4 SSSD Configuration

**Main config: `/etc/sssd/sssd.conf`**

| Section | Key Parameters |
|---------|----------------|
| **[sssd]** | `services = nss, pam, ssh`, `domains` |
| **[domain/EXAMPLE]** | `id_provider`, `auth_provider`, `ldap_uri`, `krb5_realm` |
| **[nss]** | `filter_groups`, `filter_users` |
| **[pam]** | `offline_credentials_expiration` |

### 4.5 SSSD Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Installation** | sssd packages | `sssd` service runs |
| **Cache Directory** | Local credential cache | `/var/lib/sss/` exists |
| **NSS Integration** | Name resolution | `getent passwd <user>` works |
| **PAM Integration** | Authentication | Login works for LDAP users |
| **Enumeration** | User/group listing | Optional, impacts performance |
| **Offline Auth** | Cached credentials | Login works without network |

### 4.6 Identity Configuration Files

| File | Purpose | Service |
|------|---------|---------|
| **/etc/sssd/sssd.conf** | SSSD configuration | sssd |
| **/etc/krb5.conf** | Kerberos client config | System-wide |
| **/etc/ldap.conf** | LDAP client config | nss_ldap (legacy) |
| **/etc/nsswitch.conf** | Name service order | System |
| **/etc/pam.d/** | PAM module config | Authentication |

### 4.7 Access Control Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Group Membership** | Project/group based | Users in correct groups |
| **Sudo Rules** | Privilege escalation | LDAP-based sudo or local |
| **SSH Keys** | Key distribution | LDAP sshPublicKey or file |
| **Home Directories** | Creation/mounting | Auto-created or NFS mounted |
| **Shell Access** | Login shell | Valid shell for users |

---

## 5. License Servers

### 5.1 License Server Types

| Server | Vendor | Protocol | Use Case |
|--------|--------|----------|----------|
| **FlexLM/FlexNet** | Flexera | TCP | Most commercial HPC software |
| **LM-X** | X-Formation | TCP | Alternative to FlexLM |
| **RLM** | Reprise | TCP | ISV-specific |
| **LS-DYNA** | Ansys | UDP | LS-DYNA specific |
| **Token-based** | Various | HTTP | Cloud-native licensing |

### 5.2 FlexLM Requirements

| Requirement | Description | Acceptance Criteria |
|-------------|-------------|---------------------|
| **Server Hardware** | Adequate resources | 2+ cores, 4+ GB RAM |
| **Network Ports** | SERVER and DAEMON ports | Firewall allows traffic |
| **License File** | Valid license.dat | Hostname/MAC matches |
| **Daemon Binaries** | lmgrd, vendor daemons | Correct architecture |
| **Redundancy** | Three-server triad | At least 2/3 running |

### 5.3 FlexLM Configuration

| File | Purpose | Location |
|------|---------|----------|
| **license.dat** | License definitions | Vendor-specific |
| **lmgrd** | License manager daemon | `/opt/flexlm/` typically |
| **vendor daemon** | Application-specific | Same as lmgrd |
| **options file** | Access control | Per-daemon settings |

**License file structure:**
```
SERVER hostname hostid port
DAEMON vendor_daemon [path] [options]
FEATURE feature_name vendor version exp_date num_lic ...
```

### 5.4 Scheduler License Integration

| Scheduler | Method | Configuration |
|-----------|--------|---------------|
| **Slurm** | `ResvEpilog`, licenses resource | `slurm.conf`: Licenses= |
| **PBS Pro** | Resource-based | `qmgr`: license resources |
| **LSF** | elim (External LIM) | `lsf.shared`: external resources |
| **SGE** | Consumable resources | `complex_values` |

### 5.5 License Operations

| Operation | FlexLM Command | Purpose |
|-----------|----------------|---------|
| **Status** | `lmstat -a` | Show license usage |
| **Users** | `lmstat -f feature` | Who has licenses |
| **Restart** | `lmdown; lmgrd` | Restart daemons |
| **Reread** | `lmreread` | Reload license file |
| **Log** | `lmdiag` | Diagnostics |

---

## 6. Operational Workflows

### 6.1 GPU Driver Update Workflow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    GPU Driver Update Procedure                       │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. Pre-Update Checks                                               │
│     ├── Verify CUDA compatibility matrix                           │
│     ├── Check application requirements                              │
│     └── Schedule maintenance window                                │
│                                                                     │
│  2. Drain Compute Nodes                                            │
│     ├── Slurm: scontrol update nodename=node state=drain           │
│     ├── Wait for jobs to complete                                  │
│     └── Verify nodes are idle                                      │
│                                                                     │
│  3. Update Driver                                                  │
│     ├── Stop GPU services (dcgm, persistence)                      │
│     ├── Unload nvidia modules                                      │
│     ├── Install new driver package                                 │
│     ├── Rebuild DKMS modules if needed                             │
│     └── Reboot node                                                │
│                                                                     │
│  4. Validation                                                     │
│     ├── Check nvidia-smi                                           │
│     ├── Run GPU diagnostic (cuda-samples)                          │
│     ├── Verify NCCL communication                                  │
│     └── Run application smoke test                                 │
│                                                                     │
│  5. Return to Service                                              │
│     ├── Re-enable node: scontrol update nodename=node state=resume │
│     └── Monitor for issues                                         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 6.2 Software Module Update Workflow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Module Update with Version Pinning                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. Build New Version                                              │
│     ├── Spack: spack install package@new_version                   │
│     ├── EasyBuild: eb package-new-version.eb                       │
│     └── Verify build success                                       │
│                                                                     │
│  2. Generate Module File                                           │
│     ├── Spack: spack module lmod refresh package@new               │
│     ├── EasyBuild: automatic                                       │
│     └── Review module content                                      │
│                                                                     │
│  3. Testing Phase                                                  │
│     ├── Add to hidden module path (dot prefix)                     │
│     ├── Test with select users                                     │
│     └── Run regression tests                                       │
│                                                                     │
│  4. Promotion                                                      │
│     ├── Make module visible (remove dot)                           │
│     ├── Update default symlink if appropriate                      │
│     └── Announce to users                                          │
│                                                                     │
│  5. Rollback Capability                                            │
│     ├── Keep old version installed                                 │
│     ├── Document rollback procedure                                │
│     └── Set deprecation timeline                                   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 6.3 Identity Service Update Workflow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    SSSD/Kerberos Update Procedure                    │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. Pre-Update                                                     │
│     ├── Backup sssd.conf, krb5.conf                                │
│     ├── Verify LDAP/KDC connectivity                               │
│     └── Test authentication flow                                   │
│                                                                     │
│  2. Update Package                                                 │
│     ├── Install new sssd version                                   │
│     ├── Update configuration if schema changed                     │
│     └── Restart sssd service                                       │
│                                                                     │
│  3. Clear Caches                                                   │
│     ├── sss_cache -E (clear all)                                   │
│     ├── Or: rm -rf /var/lib/sss/db/*                               │
│     └── Restart sssd                                               │
│                                                                     │
│  4. Validation                                                     │
│     ├── getent passwd <user>                                       │
│     ├── id <user>                                                  │
│     ├── kinit <user>                                               │
│     └── SSH login test                                             │
│                                                                     │
│  5. Rollback Trigger                                               │
│     ├── Authentication failures > threshold                        │
│     ├── Restore backed-up configs                                  │
│     └── Downgrade package                                          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 6.4 Node Type Requirements Matrix

| Component | Login Nodes | Compute Nodes | GPU Nodes |
|-----------|-------------|---------------|-----------|
| **GPU Driver** | Optional | No | Required |
| **CUDA Toolkit** | Development | Runtime only | Full |
| **MPI** | Build tools | Runtime | Runtime + GPU |
| **Lmod** | Full | Full | Full |
| **LDAP/Kerberos** | Full | Full | Full |
| **License Client** | Interactive | Job-based | Job-based |
| **Compilers** | Full | Minimal | Minimal |

---

## 7. Implementation Priorities

### 7.1 Phase 1: Core Identity (Required)

| Priority | Component | Rustible Module(s) |
|----------|-----------|-------------------|
| **P0** | SSSD configuration | `sssd_config`, `sssd_domain` |
| **P0** | Kerberos client | `krb5_config`, `keytab` |
| **P0** | LDAP client | `ldap_config` |
| **P1** | PAM configuration | `pam_module`, `pam_config` |
| **P1** | NSS configuration | `nsswitch` |

### 7.2 Phase 2: GPU Stack (High Priority)

| Priority | Component | Rustible Module(s) |
|----------|-----------|-------------------|
| **P1** | NVIDIA driver | `nvidia_driver` |
| **P1** | CUDA toolkit | `cuda_toolkit`, `cuda_env` |
| **P2** | NCCL library | `nccl` |
| **P2** | GPU persistence | `nvidia_persistenced` |
| **P2** | DCGM monitoring | `dcgm` |

### 7.3 Phase 3: Environment Modules (Medium Priority)

| Priority | Component | Rustible Module(s) |
|----------|-----------|-------------------|
| **P2** | Lmod installation | `lmod` |
| **P2** | Module paths | `modulepath` |
| **P2** | Spack configuration | `spack_config` |
| **P3** | EasyBuild setup | `easybuild_config` |
| **P3** | Module defaults | `lmod_default` |

### 7.4 Phase 4: MPI and Licensing (Medium Priority)

| Priority | Component | Rustible Module(s) |
|----------|-----------|-------------------|
| **P2** | MPI installation | `openmpi`, `mpich`, `intelmpi` |
| **P2** | MPI environment | `mpi_env` |
| **P3** | FlexLM server | `flexlm_server` |
| **P3** | License resources | `slurm_license`, `pbs_license` |

### 7.5 Gap Analysis vs Current Rustible

| Capability | Current Status | Gap |
|------------|---------------|-----|
| SSSD/LDAP | Not implemented | Need `sssd` module family |
| Kerberos | Not implemented | Need `krb5` module family |
| NVIDIA driver | Not implemented | Need `nvidia` module family |
| CUDA toolkit | Not implemented | Need `cuda` module family |
| Lmod | Not implemented | Need `lmod` module |
| Spack | Not implemented | Need `spack` module |
| MPI stacks | Not implemented | Need MPI modules |
| FlexLM | Not implemented | Need `flexlm` module |

---

## References

- [NVIDIA HPC SDK Installation Guide](https://docs.nvidia.com/hpc-sdk/hpc-sdk-install-guide/index.html)
- [CUDA Toolkit Documentation](https://docs.nvidia.com/cuda/)
- [Lmod Documentation](https://lmod.readthedocs.io/)
- [Spack Documentation](https://spack.readthedocs.io/)
- [EasyBuild Documentation](https://docs.easybuild.io/)
- [SSSD Documentation](https://sssd.io/)
- [Red Hat SSSD Configuration](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/7/html/system-level_authentication_guide/configuring_domains)
- [FlexNet Publisher Documentation](https://docs.flexera.com/)
- [OpenMPI FAQ](https://www.open-mpi.org/faq/)
- [Intel MPI Library](https://www.intel.com/content/www/us/en/developer/tools/oneapi/mpi-library.html)
