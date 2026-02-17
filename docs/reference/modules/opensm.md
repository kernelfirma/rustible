---
summary: Reference for the opensm_config module that manages OpenSM InfiniBand subnet manager configuration.
read_when: You need to install, configure, or remove the OpenSM subnet manager from playbooks.
---

# opensm_config - OpenSM Subnet Manager Configuration

## Synopsis

Manage the OpenSM InfiniBand subnet manager, including package installation, `opensm.conf` configuration (subnet prefix, routing engine, log level), and systemd service state. Supports both RHEL-family and Debian-family distributions.

## Classification

**Default** - HPC module. Requires `hpc` and `ofed` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| subnet_prefix | no | null | string | IB subnet prefix (e.g., `0xfe80000000000000`) |
| routing_engine | no | null | string | Routing algorithm (e.g., `minhop`, `ftree`, `updn`, `dnup`, `lash`) |
| log_level | no | null | string | Log verbosity level (0-255, maps to `log_flags` in opensm.conf) |
| state | no | "present" | string | Desired state: `present` (install and configure) or `absent` (remove) |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of changes applied (e.g., package installed, config updated, service started) |

## Examples

```yaml
- name: Install and configure OpenSM with default settings
  opensm_config:
    state: present

- name: Configure OpenSM with fat-tree routing
  opensm_config:
    subnet_prefix: "0xfe80000000000000"
    routing_engine: ftree
    log_level: "7"
    state: present

- name: Configure OpenSM with minimum-hop routing
  opensm_config:
    routing_engine: minhop
    state: present

- name: Remove OpenSM
  opensm_config:
    state: absent
```

## Notes

- Requires building with `--features hpc,ofed` (or `full-hpc`).
- Supports RHEL-family (Rocky, AlmaLinux, CentOS, Fedora) and Debian-family (Ubuntu, Debian) distributions.
- When `state: present`, the module installs the `opensm` package if not present, writes `/etc/opensm/opensm.conf` with the specified parameters, and enables/starts the `opensm` systemd service.
- When `state: absent`, the module stops and disables the `opensm` service, then removes the package.
- Configuration parameters map to `opensm.conf` directives: `subnet_prefix` maps to `subnet_prefix`, `routing_engine` maps to `routing_engine`, and `log_level` maps to `log_flags`.
- The configuration file is only written if at least one configuration parameter is provided.
- Supports check mode for all operations.
