---
summary: Reference for the pxe_host module that associates hosts by MAC address with PXE boot profiles.
read_when: You need to link a host MAC address to a PXE boot profile from playbooks.
---

# pxe_host - Associate Hosts with PXE Boot Profiles

## Synopsis

Associate hosts (identified by MAC address) with PXE boot profiles by creating symbolic links in the PXELinux configuration directory. The MAC address is converted to PXELinux format (`01-aa-bb-cc-dd-ee-ff`) automatically.

## Classification

**Default** - HPC module. Requires `hpc` and `bare_metal` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| mac | yes | - | string | Host MAC address in colon-separated format (e.g., "aa:bb:cc:dd:ee:ff") |
| profile | yes | - | string | Name of the PXE profile to link to (must exist under pxelinux.cfg/) |
| state | no | "present" | string | Desired state: "present" or "absent" |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.mac | string | The MAC address |
| data.profile | string | The profile name linked to |

## Examples

```yaml
- name: Link compute node to PXE profile
  pxe_host:
    mac: "aa:bb:cc:dd:ee:01"
    profile: centos8-compute
    state: present

- name: Link GPU node to diskless profile
  pxe_host:
    mac: "aa:bb:cc:dd:ee:02"
    profile: diskless-node

- name: Remove PXE host association
  pxe_host:
    mac: "aa:bb:cc:dd:ee:01"
    profile: centos8-compute
    state: absent
```

## Notes

- Requires building with `--features hpc,bare_metal` or `--features full-hpc`.
- MAC addresses are converted to PXELinux format: `aa:bb:cc:dd:ee:ff` becomes `01-aa-bb-cc-dd-ee-ff`.
- Host entries are created as symbolic links pointing to the profile file.
- If the symlink already points to the correct profile, no change is made (idempotent).
- The PXELinux configuration directory (`/var/lib/tftpboot/pxelinux.cfg/`) is created if it does not exist.
- This module uses `FullyParallel` parallelization, meaning multiple instances can run concurrently.
