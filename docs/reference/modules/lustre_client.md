---
summary: Reference for the lustre_client module that manages Lustre parallel filesystem client configuration.
read_when: You need to install the Lustre client or mount Lustre filesystems on HPC nodes from playbooks.
---

# lustre_client - Lustre Parallel Filesystem Client

## Synopsis

Manages Lustre filesystem client installation and mount configuration. Handles
kernel module loading, client package installation, and filesystem mount entries
for connecting compute nodes to Lustre storage targets.

## Classification

**Default** - HPC module. Requires `hpc` and `parallel_fs` feature flags.

## Parameters

| Parameter    | Required | Default    | Type   | Description                                                      |
|--------------|----------|------------|--------|------------------------------------------------------------------|
| state        | no       | `present`  | string | Desired state: `present` to install/mount, `absent` to unmount.  |
| fsname       | no       | -          | string | Lustre filesystem name.                                          |
| mgs_nids     | no       | -          | string | Management server NID(s) (e.g. `192.168.1.1@tcp`).              |
| mount_point  | no       | -          | string | Local mount point path (e.g. `/lustre`).                        |
| mount_opts   | no       | -          | string | Additional mount options (e.g. `flock,lazystatfs`).              |

## Return Values

| Key    | Type   | Description                                 |
|--------|--------|---------------------------------------------|
| status | string | Current implementation status (`stub`).      |

## Examples

```yaml
- name: Install Lustre client and mount filesystem
  lustre_client:
    state: present
    fsname: scratch
    mgs_nids: "10.0.0.1@o2ib"
    mount_point: /lustre/scratch

- name: Mount with custom options
  lustre_client:
    state: present
    fsname: home
    mgs_nids: "10.0.0.1@tcp,10.0.0.2@tcp"
    mount_point: /lustre/home
    mount_opts: "flock,lazystatfs"

- name: Unmount Lustre filesystem
  lustre_client:
    state: absent
    mount_point: /lustre/scratch
```

## Notes

- Requires building with `--features hpc,parallel_fs`.
- Currently a stub implementation; kernel module and mount logic is planned.
- The MGS NID format depends on the network type (`@tcp`, `@o2ib`, etc.).
- Multiple MGS NIDs can be separated with commas for failover.
- Ensure the Lustre client kernel module is compatible with the running kernel version.
