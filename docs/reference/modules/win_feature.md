---
summary: Reference for the win_feature module that manages Windows Server features and roles.
read_when: You need to install or remove Windows features or optional components from playbooks.
---

# win_feature - Manage Windows Server Features and Roles

## Synopsis

Installs or removes Windows features using `Install-WindowsFeature` on Server editions
and `Enable-WindowsOptionalFeature` (DISM) on client editions. Supports sub-feature
installation, management tool inclusion, and offline source paths.

## Classification

**RemoteCommand** - Windows module (experimental). Requires `winrm` feature flag.

## Parameters

| Parameter                | Required | Default    | Type         | Description                                                  |
|--------------------------|----------|------------|--------------|--------------------------------------------------------------|
| name                     | yes      | -          | string/list  | Feature name or list of feature names to manage.             |
| state                    | no       | `present`  | string       | Desired state: `present`, `absent`. Aliases: `installed`/`enabled`, `removed`/`disabled`. |
| include_sub_features     | no       | `false`    | bool         | Install all sub-features of the specified features.          |
| include_management_tools | no       | `false`    | bool         | Install associated management tools (Server only).           |
| source                   | no       | -          | string       | Path to feature source files for offline installation.       |
| restart                  | no       | `false`    | bool         | Allow automatic restart if the feature requires it.          |

## Return Values

| Key             | Type   | Description                                       |
|-----------------|--------|---------------------------------------------------|
| features        | object | Per-feature result details from the installation.  |
| restart_needed  | bool   | Whether a restart is required to complete changes.  |

## Examples

```yaml
- name: Install IIS web server with management tools
  win_feature:
    name: IIS-WebServerRole
    state: present
    include_sub_features: true
    include_management_tools: true

- name: Install multiple features
  win_feature:
    name:
      - NET-Framework-45-Core
      - NET-Framework-45-ASPNET
    state: present

- name: Remove a feature
  win_feature:
    name: Telnet-Client
    state: absent
```

## Notes

- Requires building with `--features winrm`.
- On Windows Server, uses `Install-WindowsFeature` / `Remove-WindowsFeature`.
- On Windows client, falls back to DISM (`Enable-WindowsOptionalFeature` / `Disable-WindowsOptionalFeature`).
- The `include_sub_features` and `include_management_tools` options apply only to Server editions.
- Features already in the desired state are skipped without changes.
