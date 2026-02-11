---
summary: Reference for the nxos_config module that manages Cisco NX-OS device configurations.
read_when: You need to configure Cisco Nexus switches from playbooks.
---

# nxos_config - Manage Cisco NX-OS Configuration

## Synopsis

Manages configuration on Cisco NX-OS devices (Nexus switches). Supports both
SSH and NX-API (HTTP/HTTPS) transports with checkpoint/rollback, configuration
replace, hierarchical parent context, and automatic backup.

## Classification

**Network Devices** - RemoteCommand tier. RateLimited parallelization (5
requests per second).

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| lines | no* | - | list(str) | Configuration lines to apply. |
| parents | no | - | list(str) | Parent configuration sections for hierarchical config. |
| src | no* | - | str | Path to a configuration file. Required when `replace: config`. |
| replace | no | `line` | str | Replace mode. Choices: `line`, `block`, `config`. |
| match | no | `line` | str | Match mode for comparison. Choices: `line`, `strict`, `exact`, `none`. |
| backup | no | `false` | bool | Backup running-config before changes. |
| backup_options | no | - | object | Backup destination: `{ dir_path: str, filename: str }`. |
| running_config | no | - | str | Pre-fetched running-config for diff comparison. |
| save_when | no | `never` | str | When to copy running-config to startup-config. Choices: `always`, `never`, `modified`, `changed`. |
| diff_against | no | - | str | What to diff against. Choices: `startup`, `intended`, `running`. |
| diff_ignore_lines | no | `[]` | list(str) | Patterns for lines to ignore during diff. |
| defaults | no | `false` | bool | Include default values in config output. |
| transport | no | `ssh` | str | Transport method. Choices: `ssh`, `nxapi`. |
| checkpoint | no | - | str | Name of a checkpoint to create before changes. |
| rollback_to | no | - | str | Name of an existing checkpoint to rollback to. |
| checkpoint_file | no | - | str | File path to save or load a checkpoint. |
| timeout | no | `30` | int | Command/request timeout in seconds. |
| nxapi_host | no** | - | str | NX-API host address. |
| nxapi_port | no | `443`/`80` | int | NX-API port (defaults based on SSL setting). |
| nxapi_use_ssl | no | `true` | bool | Use HTTPS for NX-API. |
| nxapi_validate_certs | no | `true` | bool | Validate SSL certificates. |
| nxapi_username | no** | - | str | NX-API authentication username. |
| nxapi_password | no** | - | str | NX-API authentication password. |

*At least one of `lines`, `src`, `checkpoint`, or `rollback_to` is required.
**Required when `transport: nxapi`.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | bool | Whether configuration was modified. |
| commands | list(str) | Configuration commands that were applied. |
| backup_path | str | File path of the backup (when `backup: true`). |
| diff | object | Unified diff of running-config before and after changes. |

## Examples

```yaml
- name: Configure VLANs via SSH
  nxos_config:
    lines:
      - vlan 100
      - name Production

- name: Configure interface with parent context
  nxos_config:
    parents:
      - interface Ethernet1/1
    lines:
      - description Uplink to Core
      - switchport mode trunk
      - no shutdown

- name: Create checkpoint before risky change
  nxos_config:
    checkpoint: pre_change

- name: Rollback to checkpoint on failure
  nxos_config:
    rollback_to: pre_change

- name: Replace full configuration from file
  nxos_config:
    src: /path/to/new_config.txt
    replace: config

- name: Configure via NX-API
  nxos_config:
    lines:
      - feature bgp
    transport: nxapi
    nxapi_host: 192.168.1.1
    nxapi_username: admin
    nxapi_password: secret
```

## Notes

- The default transport is `ssh`; set `transport: nxapi` for HTTP/HTTPS API access.
- Checkpoint names must contain only alphanumeric characters, underscores, and hyphens.
- NX-OS supports up to 64 named checkpoints.
- `replace: config` requires the `src` parameter and creates a temporary checkpoint for automatic rollback on failure.
- NX-API credentials fall back to `ansible_user` / `ansible_password` variables if NX-API-specific parameters are not set.
- Checkpoint and configuration operations can be combined in a single task (e.g., create checkpoint then apply lines).
