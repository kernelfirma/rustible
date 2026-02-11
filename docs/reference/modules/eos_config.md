---
summary: Reference for the eos_config module that manages Arista EOS device configurations.
read_when: You need to configure Arista EOS switches from playbooks.
---

# eos_config - Manage Arista EOS Configuration

## Synopsis

Manages configuration on Arista EOS devices using either the eAPI (JSON-RPC
over HTTP/HTTPS) or SSH transport. Supports configuration sessions for atomic
changes with commit/abort, replace and merge modes, comprehensive diff output,
and automatic backup.

## Classification

**Network Devices** - RemoteCommand tier. RateLimited parallelization (5
requests per second).

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| lines | no* | - | list(str) | Configuration lines to apply. |
| parents | no | - | list(str) | Parent configuration sections for hierarchical config. |
| src | no* | - | str | Path to a configuration file to apply. Required when `replace: config`. |
| replace | no | `line` | str | Replace mode. Choices: `line` (merge), `block`, `config`. |
| match | no | `line` | str | Match mode for comparison. Choices: `line`, `strict`, `exact`, `none`. |
| backup | no | `false` | bool | Backup running-config before changes. |
| backup_options | no | - | object | Backup destination: `{ dir_path: str, filename: str }`. |
| running_config | no | - | str | Pre-fetched running-config for diff comparison. |
| save_when | no | `never` | str | When to save to startup-config. Choices: `always`, `never`, `modified`, `changed`. |
| diff_against | no | `running` | str | Diff target. Choices: `running`, `startup`, `intended`, `session`. |
| diff_ignore_lines | no | `[]` | list(str) | Patterns for lines to ignore during diff. |
| intended_config | no | - | str | Intended configuration text (required when `diff_against: intended`). |
| defaults | no | `false` | bool | Include default values in config output. |
| transport | no | `eapi` | str | Transport method. Choices: `eapi`, `ssh`. |
| session | no | - | str | Configuration session name for atomic changes. |
| commit | no | `true` | bool | Whether to commit session changes. |
| abort | no | `false` | bool | Abort an existing configuration session. |
| session_timeout | no | `300` | int | Session timeout in seconds. |
| timeout | no | `30` | int | Command/request timeout in seconds. |
| eapi_host | no** | - | str | eAPI host address. |
| eapi_port | no | `443`/`80` | int | eAPI port (defaults based on SSL setting). |
| eapi_use_ssl | no | `true` | bool | Use HTTPS for eAPI. |
| eapi_validate_certs | no | `true` | bool | Validate SSL certificates. |
| eapi_username | no** | - | str | eAPI authentication username. |
| eapi_password | no** | - | str | eAPI authentication password. |

*At least one of `lines`, `src`, or a session operation (`session` + `commit`/`abort`) is required.
**Required when `transport: eapi`.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | bool | Whether configuration was modified. |
| commands | list(str) | Configuration commands that were applied. |
| backup_path | str | File path of the backup (when `backup: true`). |
| session_diff | str | Session diff output (when using uncommitted sessions). |
| diff | object | Unified diff of running-config before and after changes. |

## Examples

```yaml
- name: Configure VLANs via eAPI
  eos_config:
    lines:
      - vlan 100
      - name Production
    eapi_host: 192.168.1.1
    eapi_username: admin
    eapi_password: secret

- name: Configure interface with parent context
  eos_config:
    parents:
      - interface Ethernet1
    lines:
      - description Uplink to Core
      - switchport mode trunk
      - no shutdown

- name: Atomic change with configuration session
  eos_config:
    lines:
      - router bgp 65001
      - neighbor 10.0.0.1 remote-as 65002
    session: my_session
    commit: true

- name: Replace entire configuration from file
  eos_config:
    src: /path/to/new_config.txt
    replace: config
```

## Notes

- The default transport is `eapi`; set `transport: ssh` for CLI-based access.
- Configuration sessions provide atomic commit/abort semantics.
- When `commit: false` with a session, changes stay in the session for later review or commit.
- Credentials fall back to `ansible_user` / `ansible_password` variables if eAPI-specific parameters are not set.
- `replace: config` requires the `src` parameter.
