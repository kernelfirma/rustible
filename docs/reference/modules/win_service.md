---
summary: Reference for the win_service module that manages Windows services.
read_when: You need to start, stop, configure, create, or remove Windows services from playbooks.
---

# win_service - Manage Windows Services

## Synopsis

Manages Windows services including starting, stopping, restarting, pausing, and removal.
Supports configuring the startup type, service account credentials, display name,
description, executable path, and dependent service handling.

## Classification

**RemoteCommand** - Windows module (experimental). Requires `winrm` feature flag.

## Parameters

| Parameter                 | Required | Default   | Type   | Description                                                         |
|---------------------------|----------|-----------|--------|---------------------------------------------------------------------|
| name                      | yes      | -         | string | The internal service name (e.g. `wuauserv`).                        |
| state                     | no       | -         | string | Desired state: `started`, `stopped`, `restarted`, `paused`, `absent`. |
| start_mode                | no       | -         | string | Startup type: `auto`, `delayed`, `manual`, `disabled`.              |
| display_name              | no       | -         | string | Human-readable display name for the service.                        |
| description               | no       | -         | string | Description of the service.                                         |
| path                      | no       | -         | string | Path to service executable. Required when creating a new service.   |
| username                  | no       | -         | string | Account under which the service runs (e.g. `.\LocalService`).      |
| password                  | no       | -         | string | Password for the service account.                                   |
| dependencies              | no       | -         | list   | List of services this service depends on.                           |
| force_dependent_services  | no       | `false`   | bool   | Force-stop dependent services when stopping this service.           |
| timeout                   | no       | `30`      | u32    | Timeout in seconds to wait for state transitions.                   |

## Return Values

| Key     | Type   | Description                                    |
|---------|--------|------------------------------------------------|
| service | object | Full service status after the operation.        |

## Examples

```yaml
- name: Ensure Windows Update service is running
  win_service:
    name: wuauserv
    state: started
    start_mode: auto

- name: Create and configure a custom service
  win_service:
    name: MyAppService
    path: C:\MyApp\service.exe
    display_name: My Application Service
    description: Provides important functionality
    start_mode: delayed
    username: .\ServiceAccount
    password: "{{ service_password }}"

- name: Remove a service
  win_service:
    name: OldService
    state: absent
```

## Notes

- Requires building with `--features winrm`.
- If the service does not exist and `path` is provided, it will be created.
- The `restarted` state always triggers a stop then start cycle.
- `delayed` start mode maps to Windows `AutomaticDelayedStart`.
- Service account changes use WMI to apply the new credentials.
