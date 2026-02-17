---
summary: Reference for the pxe_profile module that manages PXE boot profiles with kernel, initrd, and append parameters.
read_when: You need to create, update, or remove PXE boot profiles from playbooks.
---

# pxe_profile - Manage PXE Boot Profiles

## Synopsis

Manage PXE boot profiles stored under `/var/lib/tftpboot/pxelinux.cfg/`. Each profile defines a kernel, initrd, and optional append parameters used during PXE network boot.

## Classification

**Default** - HPC module. Requires `hpc` and `bare_metal` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | - | string | Profile name (used as the filename under pxelinux.cfg/) |
| kernel | no | null | string | Path to the kernel image (e.g., "images/centos8/vmlinuz") |
| initrd | no | null | string | Path to the initrd image (e.g., "images/centos8/initrd.img") |
| append | no | null | string | Kernel append parameters (e.g., "ks=http://server/ks.cfg") |
| state | no | "present" | string | Desired state: "present" or "absent" |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.profile | string | The profile name |

## Examples

```yaml
- name: Create a CentOS PXE boot profile
  pxe_profile:
    name: centos8-compute
    kernel: "images/centos8/vmlinuz"
    initrd: "images/centos8/initrd.img"
    append: "ks=http://kickstart.hpc.local/centos8-compute.cfg"
    state: present

- name: Create a minimal diskless boot profile
  pxe_profile:
    name: diskless-node
    kernel: "images/diskless/vmlinuz"
    initrd: "images/diskless/initrd.img"
    append: "root=nfs:10.0.0.1:/nfsroot"

- name: Remove a PXE profile
  pxe_profile:
    name: old-profile
    state: absent
```

## Notes

- Requires building with `--features hpc,bare_metal` or `--features full-hpc`.
- Profile files are written to `/var/lib/tftpboot/pxelinux.cfg/`.
- The profile directory is created automatically if it does not exist.
- Profile content is compared for idempotency; unchanged profiles are not rewritten.
- The generated profile uses PXELinux `DEFAULT linux` / `LABEL linux` format.
- This module uses `HostExclusive` parallelization, meaning only one instance runs per host at a time.
