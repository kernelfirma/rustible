---
summary: Reference for the ios_config module that manages Cisco IOS device configurations.
read_when: You need to configure Cisco IOS or IOS-XE devices from playbooks.
---

# ios_config - Manage Cisco IOS Configuration

## Synopsis

Manages configuration on Cisco IOS, IOS-XE, and similar platforms. Provides
configuration templating with Jinja2 support, accurate diff generation,
automatic backup before changes, idempotent application with smart matching,
and parent/child hierarchy handling for nested configuration blocks.

## Classification

**Network Devices** - RemoteCommand tier. HostExclusive parallelization (one
session per device at a time).

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| lines | no* | `[]` | list(str) | Configuration lines to apply. Alias: `line` (single string). |
| parents | no | `[]` | list(str) | Parent configuration context for hierarchical config. Supports multi-level (e.g., `["router bgp 65000", "neighbor 1.1.1.1"]`). Alias: `parent`. |
| src | no* | - | str | Path to a Jinja2 configuration template file. |
| config | no* | - | str | Configuration text to apply directly (alternative to lines/src). |
| before | no | - | list(str) | Lines to apply before the main configuration lines. |
| after | no | - | list(str) | Lines to append after the main configuration lines. |
| match | no | `line` | str | How to match existing config. Choices: `line`, `strict`, `exact`, `none`. |
| replace | no | `merge` | str/bool | How to apply changes. Choices: `merge`, `block`, `config`, `override`. Also accepts `true` (config) / `false` (merge). |
| backup | no | `false` | bool | Create a backup of running-config before changes. |
| backup_dir | no | `./backups` | str | Directory to store backup files. |
| save_when | no | `never` | str | When to write running-config to startup-config. Choices: `always`, `never`, `modified`, `changed`. |
| diff_against | no | `running` | str | Configuration to compare against. Choices: `running`, `startup`, `intended`. |
| intended_config | no | - | str | Intended configuration text (required when `diff_against: intended`). |
| diff_ignore_lines | no | `[]` | list(str) | Regex patterns for lines to ignore during diff. |
| defaults | no | - | list(str) | Default configuration lines applied only if not already present. |
| transport | no | `ssh` | str | Transport method. Choices: `ssh`, `netconf`. |
| comment | no | - | str | Comment to add to the configuration change. |
| check_only | no | `false` | bool | Run in check mode regardless of global setting. |
| create_checkpoint | no | `false` | bool | Create a configuration checkpoint before changes. |
| rollback_on_failure | no | `false` | bool | Roll back to checkpoint on failure. |
| checkpoint_name | no | `rustible_checkpoint` | str | Name for the checkpoint. |
| timeout | no | `30` | int | Command timeout in seconds. |

*At least one of `lines`, `src`, or `config` is required.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | bool | Whether configuration was modified. |
| commands | list(str) | The commands that were applied to the device. |
| backup_path | str | File path of the configuration backup (when `backup: true`). |
| backup_checksum | str | Checksum of the backup file. |
| saved | bool | Whether the configuration was saved to startup-config. |
| diff | object | Diff details showing before/after line counts and unified diff output. |

## Examples

```yaml
- name: Configure interface with backup
  ios_config:
    lines:
      - ip address 10.0.0.1 255.255.255.0
      - no shutdown
    parents:
      - interface GigabitEthernet0/0
    backup: true
    save_when: modified

- name: Replace an ACL section
  ios_config:
    lines:
      - 10 permit ip 10.0.0.0 0.255.255.255 any
      - 20 deny ip any any log
    parents:
      - ip access-list extended MGMT
    match: exact
    replace: block

- name: Apply configuration from template
  ios_config:
    src: templates/router.j2
    backup: true
    diff_against: running
```

## Notes

- `replace: block` and `replace: override` require `parents` to be set.
- When `diff_against` is `intended`, `intended_config` must also be provided.
- NETCONF transport falls back to SSH in the current implementation.
- Multi-level parent hierarchies are fully supported (e.g., BGP neighbor context).
- Lines are normalized (whitespace collapsed) before comparison to avoid false diffs.
