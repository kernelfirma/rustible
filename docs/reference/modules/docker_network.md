---
summary: Reference for the docker_network module that manages Docker networks.
read_when: You need to create, configure, or remove Docker networks from playbooks.
---

# docker_network - Manage Docker Networks

## Synopsis

The `docker_network` module manages Docker networks including creating, configuring
IPAM settings, connecting containers, and removing networks. It is feature-gated and
requires building with `--features docker`.

## Classification

**RemoteCommand** - Fully parallelizable network operations via the Docker API.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Network name. |
| `state` | no | present | string | Desired state: present, absent. |
| `driver` | no | bridge | string | Network driver: bridge, overlay, host, none, macvlan, or custom. |
| `driver_options` | no | - | map | Driver-specific options. |
| `ipam` | no | - | map | IPAM configuration with driver and config (subnet, gateway, ip_range). |
| `internal` | no | false | bool | Restrict external access to the network. |
| `attachable` | no | false | bool | Enable manual container attachment (for overlay networks). |
| `scope` | no | - | string | Network scope: local, global, swarm. |
| `labels` | no | - | map | Network labels. |
| `enable_ipv6` | no | false | bool | Enable IPv6 on the network. |
| `connected` | no | - | list | List of container names to connect to the network. |
| `force` | no | false | bool | Force removal even if containers are connected. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `network.id` | string | Network ID. |
| `network.name` | string | Network name. |
| `network.driver` | string | Network driver. |
| `network.scope` | string | Network scope. |
| `network.internal` | bool | Whether the network is internal. |
| `network.containers` | map | Connected containers. |

## Examples

### Create a bridge network with custom subnet

```yaml
- name: Create application network
  docker_network:
    name: app-net
    driver: bridge
    ipam:
      driver: default
      config:
        - subnet: "172.20.0.0/16"
          gateway: "172.20.0.1"
    state: present
```

### Remove a network

```yaml
- name: Remove old network
  docker_network:
    name: app-net
    state: absent
    force: true
```

## Notes

- Build with `--features docker` to enable this module.
- Uses the bollard crate to communicate with the Docker daemon.
- When `force` is set, all connected containers are disconnected before removal.
