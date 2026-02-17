---
summary: Reference for the ib_partition module that manages InfiniBand partition keys in OpenSM.
read_when: You need to add, remove, or configure InfiniBand partition keys from playbooks.
---

# ib_partition - InfiniBand Partition Key Management

## Synopsis

Manage InfiniBand partition keys (P-Keys) via the OpenSM `partitions.conf` file. Supports adding and removing partition entries with member lists and IPoIB enablement.

## Classification

**Default** - HPC module. Requires `hpc` and `ofed` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| pkey | yes | - | string | Partition key in hex format (e.g., `0x7fff`) |
| members | no | [] | list | List of node GUIDs or names to include in the partition. Defaults to `ALL` if empty. |
| state | no | "present" | string | Desired state: `present` (add partition) or `absent` (remove partition) |
| ipoib | no | false | boolean | Enable IPoIB for this partition |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.pkey | string | Partition key value |
| data.members | array | List of members assigned to the partition |

## Examples

```yaml
- name: Add a default partition with all nodes
  ib_partition:
    pkey: "0x7fff"
    state: present

- name: Add a partition with specific members and IPoIB
  ib_partition:
    pkey: "0x0001"
    members:
      - node1
      - node2
      - node3
    ipoib: true
    state: present

- name: Remove a partition key
  ib_partition:
    pkey: "0x0001"
    state: absent
```

## Notes

- Requires building with `--features hpc,ofed` (or `full-hpc`).
- Manages the `/etc/opensm/partitions.conf` file. Creates the file and directory if they do not exist.
- Partition entries are written in the format `pkey[,ipoib]=member1,member2` (or `pkey=ALL` if no members are specified).
- Adding a partition is idempotent: if a line starting with `pkey:` already exists, no changes are made.
- Removing a partition deletes the matching line from `partitions.conf` using sed.
- OpenSM must be restarted or reloaded for partition configuration changes to take effect.
- Supports check mode for all operations.
