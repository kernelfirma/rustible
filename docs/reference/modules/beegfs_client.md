---
summary: Reference for the beegfs_client module that manages BeeGFS parallel filesystem client configuration.
read_when: You need to install the BeeGFS client or mount BeeGFS filesystems on HPC nodes from playbooks.
---

# beegfs_client - BeeGFS Parallel Filesystem Client

## Synopsis

Manages BeeGFS filesystem client installation and mount configuration. Handles
client package installation, management daemon connectivity, and client-side
tuning for optimal parallel I/O performance.

## Classification

**Default** - HPC module. Requires `hpc` and `parallel_fs` feature flags.

## Parameters

| Parameter      | Required | Default    | Type   | Description                                                       |
|----------------|----------|------------|--------|-------------------------------------------------------------------|
| state          | no       | `present`  | string | Desired state: `present` to install/mount, `absent` to unmount.   |
| mgmtd_host     | no       | -          | string | Hostname or IP of the BeeGFS management daemon.                   |
| mount_point    | no       | -          | string | Local mount point path (e.g. `/beegfs`).                         |
| client_config  | no       | -          | string | Path to a custom `beegfs-client.conf` file.                      |
| tuning         | no       | -          | object | Client tuning parameters (e.g. stripe settings, cache size).      |

## Return Values

| Key    | Type   | Description                                 |
|--------|--------|---------------------------------------------|
| status | string | Current implementation status (`stub`).      |

## Examples

```yaml
- name: Install BeeGFS client and mount filesystem
  beegfs_client:
    state: present
    mgmtd_host: beegfs-mgmt01
    mount_point: /beegfs

- name: Configure BeeGFS client with custom tuning
  beegfs_client:
    state: present
    mgmtd_host: beegfs-mgmt01
    mount_point: /beegfs/scratch
    tuning:
      connMaxInternodeNum: 32
      tuneFileCacheSize: 524288

- name: Use a custom client configuration file
  beegfs_client:
    state: present
    client_config: /etc/beegfs/beegfs-client-custom.conf
    mount_point: /beegfs

- name: Unmount BeeGFS filesystem
  beegfs_client:
    state: absent
    mount_point: /beegfs
```

## Notes

- Requires building with `--features hpc,parallel_fs`.
- Currently a stub implementation; client install and mount logic is planned.
- The management daemon (`mgmtd_host`) must be reachable from all client nodes.
- Tuning parameters are written to the BeeGFS client configuration file.
- BeeGFS uses a kernel module (`beegfs`) that must be compatible with the running kernel.
