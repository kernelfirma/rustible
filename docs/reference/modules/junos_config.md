---
summary: Reference for the junos_config module that manages Juniper JunOS device configurations.
read_when: You need to configure Juniper JunOS devices from playbooks.
---

# junos_config - Manage Juniper JunOS Configuration

## Synopsis

Manages configuration on Juniper devices running JunOS via NETCONF over SSH.
Supports candidate-based configuration with commit confirm for safe rollback,
multiple configuration formats (text, set, XML, JSON), configuration
validation, and rollback to previous or rescue configurations.

## Classification

**Network Devices** - RemoteCommand tier.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| config | no* | - | str | Configuration content to load (inline text). |
| src | no* | - | str | Path to a configuration file to load. |
| config_format | no | `text` | str | Format of the configuration. Choices: `text`, `set`, `xml`, `json`. Alias: `format`. |
| load_operation | no | `merge` | str | How to load config into candidate. Choices: `merge`, `replace`, `override`, `update`. Alias: `operation`. |
| commit | no | `true` | bool | Commit the candidate configuration after loading. |
| commit_confirm | no | - | int | Minutes until auto-rollback if not confirmed (1-65535). |
| confirm | no | `false` | bool | Confirm a previously pending commit-confirm. |
| rollback | no | - | int/str | Rollback target: integer 0-49 or `"rescue"`. |
| compare | no | `false` | bool | Compare candidate configuration against running (show diff only). |
| validate | no | `false` | bool | Validate the candidate configuration without committing. |
| comment | no | - | str | Comment for the commit log entry. |
| synchronize | no | `false` | bool | Synchronize commit across dual routing engines. |
| lock | no | `true` | bool | Lock the candidate configuration during the operation. |

*At least one of `config`, `src`, `confirm`, `rollback`, `compare`, or `validate` is required.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | bool | Whether configuration was modified and committed. |
| diff | object | Configuration diff between candidate and running (before commit). |
| output | str | CLI output from the commit, confirm, or validate operation. |
| config_format | str | The format used for the loaded configuration. |
| operation | str | The load operation that was performed. |

## Examples

```yaml
- name: Load set commands and commit
  junos_config:
    config: |
      set system host-name router01
      set interfaces ge-0/0/0 unit 0 family inet address 10.0.0.1/24
    config_format: set

- name: Commit with 5-minute confirm timeout
  junos_config:
    config: "{{ lookup('file', 'new_config.conf') }}"
    commit_confirm: 5

- name: Confirm a pending commit
  junos_config:
    confirm: true

- name: Rollback to previous configuration
  junos_config:
    rollback: 1

- name: Validate candidate without committing
  junos_config:
    config: |
      interfaces {
        ge-0/0/0 { unit 0 { family inet { address 10.0.0.1/24; } } }
      }
    validate: true
    commit: false
```

## Notes

- When `commit: false`, loaded changes are discarded after the operation (dry-run behavior).
- `confirm` cannot be combined with `config` or `rollback` in the same task.
- The `rollback` parameter accepts 0-49 for numbered rollback points or `"rescue"` for rescue configuration.
- `commit_confirm` triggers auto-rollback if a follow-up `confirm: true` task is not run within the specified minutes.
- Configuration is loaded via CLI commands internally; full NETCONF subsystem support is planned.
- The `update` load operation is only valid with `set` format commands.
