---
summary: Reference for the aws_ebs_volume module that manages AWS Elastic Block Store volumes.
read_when: You need to create, resize, retag, or delete EBS volumes from playbooks.
---

# aws_ebs_volume - Manage AWS EBS Volumes

## Synopsis

The `aws_ebs_volume` module manages AWS Elastic Block Store volumes from the
control node. It supports idempotent lookup by `volume_id` or `name`, create,
safe in-place updates where AWS allows them, tag synchronization, check mode,
and explicit validation for immutable settings.

## Classification

**Cloud** - Uses the AWS SDK to manage EC2 EBS volumes.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `volume_id` | no* | - | string | Existing EBS volume id. `id` is accepted as an alias. |
| `name` | no* | - | string | Logical identifier backed by the `Name` tag. Used for idempotent lookup and added to tags when absent. |
| `state` | no | present | string | Desired state: `present`, `absent`. |
| `availability_zone` | no** | - | string | Availability zone for new volumes or for narrowing `name`-based lookup when multiple regions/AZs are in play. |
| `size` | no*** | - | integer | Volume size in GiB. Required unless `snapshot_id` is used for creation. |
| `type` | no | gp3 | string | Volume type. `volume_type` is accepted as an alias. |
| `iops` | no | - | integer | IOPS for `gp3`, `io1`, or `io2` volumes. |
| `throughput` | no | - | integer | Throughput in MiB/s for `gp3` volumes. |
| `encrypted` | no | false on create | boolean | Whether to encrypt the volume. Immutable after creation. |
| `kms_key_id` | no | - | string | KMS key id for encrypted volumes. Immutable after creation. |
| `snapshot_id` | no | - | string | Snapshot id to restore from. Immutable after creation. |
| `tags` | no | {} | map | Resource tags synchronized to the volume. |
| `region` | no | - | string | AWS region override. |

\* Provide at least one of `volume_id`, `name`, or `tags.Name`.

\** Required when creating a volume. It is also used to narrow name-based lookup when provided.

\*** Required for creation when `snapshot_id` is not provided.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `action` | string | Action taken or planned: create, update, or delete. Present only when a change is required. |
| `volume_id` | string | The AWS EBS volume id. |
| `volume` | object | Resolved volume metadata including size, type, encryption, tags, state, and attachment count. |

## Examples

### Create a gp3 data volume

```yaml
- name: Create a database data volume
  aws_ebs_volume:
    name: db-data
    availability_zone: us-east-1a
    size: 100
    type: gp3
    iops: 3000
    throughput: 125
    encrypted: true
    tags:
      Environment: production
      Role: database
```

### Resize and retag an existing volume

```yaml
- name: Expand the application volume
  aws_ebs_volume:
    volume_id: vol-0123456789abcdef0
    size: 200
    type: gp3
    iops: 6000
    throughput: 250
    encrypted: true
    tags:
      Name: app-data
      Environment: production
      Owner: platform
```

### Delete a detached volume

```yaml
- name: Remove an unused volume
  aws_ebs_volume:
    volume_id: vol-0123456789abcdef0
    state: absent
```

## Notes

- Requires AWS credentials in the environment or config files.
- Build with `--features aws` to enable this module.
- The module refuses unsupported immutable updates such as changing `availability_zone`, `encrypted`, `kms_key_id`, or `snapshot_id` on an existing volume.
- Size changes are grow-only; shrinking a volume is rejected.
- Creating a new volume requires a stable `name` or `tags.Name`; `volume_id` alone can only target an existing volume.
- When identifying a volume by `name`, the match must be unique within the selected availability zone when one is provided.
