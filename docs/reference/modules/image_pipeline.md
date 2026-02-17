---
summary: Reference for the hpc_image_pipeline module that manages OS image lifecycle for bare-metal provisioning.
read_when: You need to build, promote, rollback, list, or check status of OS images from playbooks.
---

# hpc_image_pipeline - OS Image Lifecycle Pipeline

## Synopsis

Manages the lifecycle of OS images for bare-metal provisioning. Images progress through status stages: `Building` -> `Ready` -> `Active` -> `Deprecated`. Supports building new images, promoting ready images to active, rolling back to previously deprecated images, listing versions, and querying status.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | **yes** | - | string | Action to perform: `"build"`, `"promote"`, `"rollback"`, `"list"`, or `"status"`. |
| name | **yes** | - | string | Image name (e.g. `"rocky9-hpc"`). |
| version | conditional | `null` | string | Image version string. Required for `promote` and `rollback` actions. Optional for `status` (defaults to active image). |
| build_script | no | `null` | string | Path to an image build script on the target. Executed during the `build` action. |
| image_dir | no | `"/var/lib/rustible/images"` | string | Directory for image metadata storage. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether an image was created or its status changed |
| msg | string | Status message |
| data.image | object | Image metadata with `name`, `version`, `build_id`, `status`, and `created_at` fields |
| data.path | string | Filesystem path to the image metadata JSON file |
| data.images | array | List of image version objects (for `list` action) |
| data.name | string | Image name (for `list` and `status` actions when no active image found) |

## Image Lifecycle

```
Building  -->  Ready  -->  Active  -->  Deprecated
                  |                         |
                  +--- Deprecated           |
                                            |
                  Active  <--- (rollback) --+
```

Valid transitions:
- `Building` -> `Ready` (build completes)
- `Ready` -> `Active` (promote)
- `Active` -> `Deprecated` (superseded by promotion)
- `Ready` -> `Deprecated` (skip active)
- `Deprecated` -> `Active` (rollback)

## Examples

```yaml
- name: Build a new OS image
  hpc_image_pipeline:
    action: build
    name: rocky9-hpc
    build_script: /opt/imaging/build-rocky9.sh

- name: Build image without a build script
  hpc_image_pipeline:
    action: build
    name: rocky9-hpc

- name: Promote an image to active
  hpc_image_pipeline:
    action: promote
    name: rocky9-hpc
    version: "rocky9-hpc-20250101120000"

- name: Rollback to a previous image
  hpc_image_pipeline:
    action: rollback
    name: rocky9-hpc
    version: "rocky9-hpc-20241215090000"

- name: List all versions of an image
  hpc_image_pipeline:
    action: list
    name: rocky9-hpc

- name: Check status of the active image
  hpc_image_pipeline:
    action: status
    name: rocky9-hpc
```

## Notes

- Requires building with `--features hpc`.
- Image metadata is stored as JSON files under `<image_dir>/<name>/<build_id>.json`.
- An `active.json` symlink is maintained pointing to the currently active image.
- Build IDs are generated from UTC timestamps (`YYYYMMDDHHmmss`).
- Version strings follow the format `<name>-<build_id>`.
- Promoting an image automatically deprecates any currently active image for the same name.
- If a build script fails, the module returns an error and the image remains in `Building` state.
- Parallelization hint: `HostExclusive` (one invocation per host at a time).
