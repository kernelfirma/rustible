---
summary: Reference for the warewulf_image module that manages Warewulf node images (containers/chroots) via wwctl.
read_when: You need to import, manage, or remove Warewulf node images from playbooks.
---

# warewulf_image - Manage Warewulf Node Images

## Synopsis

Manage Warewulf node images (containers/chroots) using the `wwctl` command-line interface. Supports importing images from chroot paths and deleting existing image definitions.

## Classification

**Default** - HPC module. Requires `hpc` and `bare_metal` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | - | string | Image name (e.g., "rocky8-compute") |
| chroot | no | null | string | Path to the chroot directory to import (required when creating a new image) |
| state | no | "present" | string | Desired state: "present" or "absent" |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.image | string | The image name |
| data.chroot | string | The chroot path used for import (when creating) |

## Examples

```yaml
- name: Import a Rocky Linux chroot as a Warewulf image
  warewulf_image:
    name: "rocky8-compute"
    chroot: "/var/warewulf/chroots/rocky8"
    state: present

- name: Import an Ubuntu chroot
  warewulf_image:
    name: "ubuntu22-gpu"
    chroot: "/var/warewulf/chroots/ubuntu22-gpu"

- name: Remove a Warewulf image
  warewulf_image:
    name: "old-image"
    state: absent
```

## Notes

- Requires building with `--features hpc,bare_metal` or `--features full-hpc`.
- The `wwctl` command must be available on the target host (Warewulf must be installed).
- If the image already exists, the module reports no change (idempotent for creation).
- The `chroot` parameter is required when creating a new image; omitting it for a new image results in an error.
- Internally uses `wwctl container import CHROOT NAME` to import and `wwctl container delete NAME` to remove.
- This module uses `GlobalExclusive` parallelization, meaning only one instance runs across the entire inventory at a time.
