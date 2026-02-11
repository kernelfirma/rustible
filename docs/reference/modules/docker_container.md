---
summary: Reference for the docker_container module that manages Docker container lifecycle.
read_when: You need to create, start, stop, or remove Docker containers from playbooks.
---

# docker_container - Manage Docker Containers

## Synopsis

The `docker_container` module manages Docker container lifecycle including creating,
starting, stopping, restarting, and removing containers. It is feature-gated and
requires building with `--features docker`.

## Classification

**RemoteCommand** - Fully parallelizable container operations via the Docker API.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Container name. |
| `image` | yes* | - | string | Docker image to use. Required for state=present/started. |
| `state` | no | started | string | Desired state: present, absent, started, stopped, restarted. |
| `command` | no | - | string/list | Command to run in the container. |
| `entrypoint` | no | - | string/list | Override the default entrypoint. |
| `env` | no | - | map/list | Environment variables (key=value pairs or map). |
| `ports` | no | - | map/list | Port mappings (host:container format). |
| `volumes` | no | - | list | Volume mounts (host:container format). |
| `network` | no | - | string | Network to connect to. |
| `restart_policy` | no | - | string | Restart policy: no, always, on-failure, unless-stopped. |
| `pull` | no | missing | string | Image pull policy: always, missing, never. |
| `recreate` | no | false | bool | Recreate container if config changed. |
| `remove_volumes` | no | false | bool | Remove volumes when removing container. |
| `force_kill` | no | false | bool | Use SIGKILL instead of SIGTERM. |
| `stop_timeout` | no | 10 | integer | Timeout in seconds before SIGKILL. |
| `labels` | no | - | map | Container labels. |
| `hostname` | no | - | string | Container hostname. |
| `user` | no | - | string | User to run as inside the container. |
| `working_dir` | no | - | string | Working directory inside the container. |
| `memory` | no | - | string | Memory limit (e.g., "512m", "1g"). |
| `cpus` | no | - | float | CPU limit (e.g., 0.5, 2.0). |
| `privileged` | no | false | bool | Run container in privileged mode. |
| `read_only` | no | false | bool | Mount root filesystem as read-only. |
| `capabilities_add` | no | - | list | Linux capabilities to add. |
| `capabilities_drop` | no | - | list | Linux capabilities to drop. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `container.id` | string | Container ID. |
| `container.name` | string | Container name. |
| `container.running` | bool | Whether the container is running. |
| `container.image` | string | Image used by the container. |

## Examples

### Start a container

```yaml
- name: Start nginx container
  docker_container:
    name: web
    image: nginx:latest
    state: started
    ports:
      - "8080:80"
    env:
      NGINX_HOST: example.com
```

### Stop and remove a container

```yaml
- name: Remove old container
  docker_container:
    name: web
    state: absent
    remove_volumes: true
```

## Notes

- Build with `--features docker` to enable this module.
- Uses the bollard crate to communicate with the Docker daemon.
- The `running` alias is accepted for the `started` state.
