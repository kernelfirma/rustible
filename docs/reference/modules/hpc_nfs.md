---
summary: Reference for the nfs_server and nfs_client modules that manage NFS shared storage for HPC clusters.
read_when: You need to configure NFS server exports or client mounts from playbooks.
---

# nfs_server / nfs_client - Manage NFS Shared Storage

## Synopsis

Provides two modules for managing NFS shared storage on HPC clusters:

- **nfs_server**: Installs NFS server packages, manages `/etc/exports`, and controls the NFS server systemd service.
- **nfs_client**: Installs NFS client packages, manages fstab entries, and mounts/unmounts NFS exports.

---

## nfs_server

### Classification

**Default** - HPC module. Requires `hpc` feature flag.

### Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| state | no | `"present"` | string | Desired state: `"present"` to install and configure, `"absent"` to remove. |
| exports | no | `[]` | list(string) | List of NFS export lines to write to `/etc/exports`. Each entry is a full export line (e.g. `"/data 10.0.0.0/24(rw,sync,no_root_squash)"`). |

### Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of changes applied |

### Examples

```yaml
- name: Configure NFS server with exports
  nfs_server:
    state: present
    exports:
      - "/home 10.0.0.0/24(rw,sync,no_root_squash)"
      - "/scratch 10.0.0.0/24(rw,sync,no_subtree_check)"

- name: Remove NFS server
  nfs_server:
    state: absent
```

---

## nfs_client

### Classification

**Default** - HPC module. Requires `hpc` feature flag.

### Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| server | **yes** | - | string | NFS server hostname or IP address. |
| export | **yes** | - | string | Export path on the server (e.g. `/home`). |
| mount_point | **yes** | - | string | Local mount point directory. |
| state | no | `"mounted"` | string | Desired state: `"mounted"`, `"unmounted"`, or `"absent"`. |
| mount_options | no | `"defaults,hard,intr"` | string | Mount options for the fstab entry. |

### Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of changes applied |

### Examples

```yaml
- name: Mount shared home directories
  nfs_client:
    server: nfs-server.cluster.local
    export: /home
    mount_point: /home
    state: mounted

- name: Mount scratch space with custom options
  nfs_client:
    server: nfs-server.cluster.local
    export: /scratch
    mount_point: /scratch
    mount_options: "defaults,hard,intr,nfsvers=4.1"

- name: Unmount and remove fstab entry
  nfs_client:
    server: nfs-server.cluster.local
    export: /scratch
    mount_point: /scratch
    state: absent
```

---

## Notes

- Requires building with `--features hpc`.
- Both modules support RHEL-family (nfs-utils) and Debian-family (nfs-kernel-server / nfs-common) distributions.
- The server module runs `exportfs -ra` after updating `/etc/exports`.
- The client module creates the mount point directory if it does not exist.
- Parallelization hint: `HostExclusive` (one invocation per host at a time) for both modules.
