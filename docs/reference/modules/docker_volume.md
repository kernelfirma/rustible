---
summary: Reference for the docker_volume module that manages Docker volumes.
read_when: You need to create, inspect, or remove Docker volumes from playbooks.
---

# docker_volume - Manage Docker Volumes

## Synopsis

The `docker_volume` module manages Docker volumes including creating, inspecting,
and removing named volumes. It is feature-gated and requires building with
`--features docker`.

## Classification

**RemoteCommand** - Fully parallelizable volume operations via the Docker API.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Volume name. |
| `state` | no | present | string | Desired state: present, absent. |
| `driver` | no | local | string | Volume driver. |
| `driver_options` | no | - | map | Driver-specific options. |
| `labels` | no | - | map | Volume labels. |
| `force` | no | false | bool | Force removal even if the volume is in use. |
| `recreate` | no | false | bool | Recreate volume if it already exists (removes and recreates). |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `volume.name` | string | Volume name. |
| `volume.driver` | string | Volume driver. |
| `volume.mountpoint` | string | Path where the volume is mounted on the host. |
| `volume.scope` | string | Volume scope. |
| `volume.labels` | map | Volume labels. |
| `volume.created_at` | string | Volume creation timestamp. |

## Examples

### Create a named volume

```yaml
- name: Create data volume
  docker_volume:
    name: postgres-data
    driver: local
    labels:
      app: database
      environment: production
    state: present
```

### Remove a volume

```yaml
- name: Remove old volume
  docker_volume:
    name: postgres-data
    state: absent
    force: true
```

## Notes

- Build with `--features docker` to enable this module.
- Uses the bollard crate to communicate with the Docker daemon.
- Using `recreate: true` will destroy all data in the volume.
