---
summary: Reference for the docker_compose module that manages Docker Compose projects.
read_when: You need to deploy or manage Docker Compose stacks from playbooks.
---

# docker_compose - Manage Docker Compose Projects

## Synopsis

The `docker_compose` module manages Docker Compose projects by executing the
`docker compose` (V2) or `docker-compose` (V1) CLI. It supports deploying,
stopping, restarting, and removing Compose stacks. It is feature-gated and
requires building with `--features docker`.

## Classification

**RemoteCommand** - Host-exclusive operations (one Compose operation per host at a time).

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `project_src` | yes* | - | string | Path to docker-compose.yml directory. Required unless `definition` is provided. |
| `project_name` | no | - | string | Compose project name (defaults to directory name). |
| `state` | no | present | string | Desired state: present (up), absent (down), stopped, restarted. |
| `files` | no | ["docker-compose.yml"] | list | Compose files to use. |
| `services` | no | [] | list | Specific services to operate on (default: all). |
| `definition` | no | - | string | Inline docker-compose YAML definition. |
| `build` | no | false | bool | Build images before starting. |
| `pull` | no | missing | string | Pull policy: always, missing, never. |
| `recreate` | no | smart | string | Recreate policy: always, never, smart. |
| `remove_orphans` | no | true | bool | Remove containers not defined in the compose file. |
| `remove_images` | no | - | string | Remove images when stopping: all, local. |
| `remove_volumes` | no | false | bool | Remove volumes when stopping. |
| `timeout` | no | - | integer | Timeout in seconds for container operations. |
| `scale` | no | - | map | Service scaling as service_name: count. |
| `env_file` | no | - | string | Path to environment file. |
| `profiles` | no | [] | list | Compose profiles to enable. |
| `detach` | no | true | bool | Detach after starting services. |
| `wait` | no | false | bool | Wait for services to be healthy. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `project.services` | list | List of service status objects. |
| `project.running` | bool | Whether the project has running services. |

## Examples

### Deploy a Compose project

```yaml
- name: Start application stack
  docker_compose:
    project_src: /opt/myapp
    state: present
    build: true
    pull: always
```

### Scale a service

```yaml
- name: Scale web service
  docker_compose:
    project_src: /opt/myapp
    state: present
    scale:
      web: 3
```

### Tear down a project

```yaml
- name: Remove application stack
  docker_compose:
    project_src: /opt/myapp
    state: absent
    remove_volumes: true
    remove_images: all
```

## Notes

- Build with `--features docker` to enable this module.
- Requires `docker compose` (V2) or `docker-compose` (V1) to be installed.
- Either `project_src` or `definition` must be provided.
- The `present` state also accepts `started` and `up` as aliases; `absent` accepts `removed` and `down`.
