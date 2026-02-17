---
summary: Reference for the hpc_boot_profile module that manages PXE/iPXE/UEFI boot profiles for bare-metal provisioning.
read_when: You need to create, retrieve, list, or generate iPXE scripts for boot profiles from playbooks.
---

# hpc_boot_profile - Boot Profile Management

## Synopsis

Manages boot profiles for bare-metal provisioning. Supports creating, updating, retrieving, and listing PXE/iPXE/UEFI boot profiles. Can generate iPXE scripts from stored profile definitions. Profiles are stored as JSON files in a configurable directory.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | **yes** | - | string | Action to perform: `"set"`, `"get"`, `"list"`, or `"generate_ipxe"`. |
| name | conditional | `null` | string | Profile name. Required for `set`, `get`, and `generate_ipxe` actions. |
| kernel_url | conditional | `null` | string | URL to the kernel image. Required for `set` action. |
| initrd_url | conditional | `null` | string | URL to the initrd image. Required for `set` action. |
| cmdline | no | `""` | string | Kernel command line arguments. |
| boot_mode | no | `"ipxe"` | string | Boot mode: `"pxe"`, `"ipxe"`, or `"uefi"`. |
| profile_dir | no | `"/etc/rustible/boot-profiles"` | string | Directory for storing boot profile JSON files. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether a profile was created or updated |
| msg | string | Status message |
| data.profile | object | Boot profile object with `name`, `kernel_url`, `initrd_url`, `cmdline`, and `boot_mode` fields |
| data.path | string | Filesystem path to the profile JSON file |
| data.profiles | array | List of profile names (for `list` action) |
| data.profile_dir | string | Profile storage directory (for `list` action) |
| data.ipxe_script | string | Generated iPXE script content (for `generate_ipxe` action) |

## Examples

```yaml
- name: Create a Rocky 9 boot profile
  hpc_boot_profile:
    action: set
    name: rocky9
    kernel_url: "http://pxe.cluster.local/rocky9/vmlinuz"
    initrd_url: "http://pxe.cluster.local/rocky9/initrd.img"
    cmdline: "console=tty0 console=ttyS0,115200n8 ip=dhcp"
    boot_mode: ipxe

- name: Create a UEFI boot profile
  hpc_boot_profile:
    action: set
    name: ubuntu2204-uefi
    kernel_url: "http://pxe.cluster.local/ubuntu/vmlinuz"
    initrd_url: "http://pxe.cluster.local/ubuntu/initrd"
    cmdline: "console=ttyS0 ip=dhcp"
    boot_mode: uefi

- name: Retrieve a boot profile
  hpc_boot_profile:
    action: get
    name: rocky9

- name: List all boot profiles
  hpc_boot_profile:
    action: list

- name: Generate iPXE script for a profile
  hpc_boot_profile:
    action: generate_ipxe
    name: rocky9
```

## Notes

- Requires building with `--features hpc`.
- Profiles are stored as `<profile_dir>/<name>.json`.
- The `set` action is idempotent: if the profile already exists with identical content, no change is reported.
- Generated iPXE scripts follow standard iPXE format with `#!ipxe` shebang, `kernel`, `initrd`, and `boot` directives.
- Parallelization hint: `HostExclusive` (one invocation per host at a time).
