---
summary: Reference for the docker_image module that manages Docker images.
read_when: You need to pull, build, tag, or remove Docker images from playbooks.
---

# docker_image - Manage Docker Images

## Synopsis

The `docker_image` module manages Docker images including pulling from registries,
building from Dockerfiles, loading from archives, tagging, pushing, and removing.
It is feature-gated and requires building with `--features docker`.

## Classification

**RemoteCommand** - Rate-limited image operations (5 requests/second) via the Docker API.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Image name. |
| `tag` | no | latest | string | Image tag. |
| `state` | no | present | string | Desired state: present, absent, build. |
| `source` | no | pull | string | Image source: pull, build, load, local. |
| `build` | no | - | map | Build configuration (path, dockerfile, args, nocache, pull, target, rm, forcerm, labels). |
| `build_path` | no | - | string | Shorthand for build.path when not using the build map. |
| `dockerfile` | no | Dockerfile | string | Shorthand for build.dockerfile. |
| `push` | no | false | bool | Push image to registry after pull/build. |
| `force` | no | false | bool | Force removal of image. |
| `archive_path` | no | - | string | Path to tar archive for source=load. |
| `repository` | no | - | string | Registry repository for push. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `image.id` | string | Image ID. |
| `image.created` | string | Image creation timestamp. |
| `image.size` | integer | Image size in bytes. |
| `image.architecture` | string | Image architecture. |
| `image.os` | string | Image operating system. |
| `image.tags` | list | Image tags. |

## Examples

### Pull an image

```yaml
- name: Pull nginx image
  docker_image:
    name: nginx
    tag: "1.25"
    source: pull
    state: present
```

### Build an image from Dockerfile

```yaml
- name: Build application image
  docker_image:
    name: myapp
    tag: latest
    state: build
    build:
      path: /opt/myapp
      dockerfile: Dockerfile
      args:
        VERSION: "1.0"
      nocache: false
```

### Remove an image

```yaml
- name: Remove old image
  docker_image:
    name: myapp
    tag: old
    state: absent
    force: true
```

## Notes

- Build with `--features docker` to enable this module.
- Uses the bollard crate to communicate with the Docker daemon.
- Image operations are rate-limited to 5 requests per second to avoid registry throttling.
