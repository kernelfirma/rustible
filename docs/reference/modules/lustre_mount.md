---
summary: Reference for the lustre_mount module that manages LNet-aware Lustre filesystem mounts with fstab persistence.
read_when: You need to mount, unmount, or remove Lustre filesystems from playbooks.
---

# lustre_mount - Manage Lustre Filesystem Mounts

## Synopsis

Manages Lustre filesystem mounts with LNet NID configuration, mount options, and fstab persistence. This module focuses specifically on mount lifecycle management, complementing `lustre_client` which handles package installation and basic mounts.

## Classification

**Default** - HPC module. Requires `hpc` and `parallel_fs` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| nid | yes | - | string | LNet NID address (e.g., "10.0.0.1@tcp") |
| fs_name | yes | - | string | Lustre filesystem name |
| mount_point | yes | - | string | Target mount point path |
| mount_options | no | "defaults" | string | Mount options passed to the mount command |
| fstab | no | true | boolean | Whether to manage the /etc/fstab entry |
| state | no | "mounted" | string | Desired state: "mounted", "unmounted", or "absent" |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of individual changes applied |
| data.mount_point | string | The mount point path (when state is "mounted") |
| data.source | string | The Lustre source string in NID:/fsname format (when state is "mounted") |

## Examples

```yaml
- name: Mount Lustre filesystem
  lustre_mount:
    nid: "10.0.0.1@tcp"
    fs_name: "scratch"
    mount_point: "/mnt/lustre/scratch"
    mount_options: "flock,noatime"
    fstab: true
    state: mounted

- name: Unmount but keep fstab entry
  lustre_mount:
    nid: "10.0.0.1@tcp"
    fs_name: "scratch"
    mount_point: "/mnt/lustre/scratch"
    state: unmounted

- name: Completely remove Lustre mount and fstab entry
  lustre_mount:
    nid: "10.0.0.1@tcp"
    fs_name: "scratch"
    mount_point: "/mnt/lustre/scratch"
    state: absent
```

## Notes

- Requires building with `--features hpc,parallel_fs` or `--features full-hpc`.
- The module automatically loads the `lustre` kernel module if it is not already loaded (when state is "mounted").
- The mount point directory is created automatically if it does not exist.
- Fstab entries are matched by NID and filesystem name for idempotent updates.
- This module uses `HostExclusive` parallelization, meaning only one instance runs per host at a time.
