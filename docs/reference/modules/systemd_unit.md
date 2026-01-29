---
summary: Reference for the systemd_unit module that manages systemd unit files and service state.
read_when: You need to manage systemd services, timers, or unit files.
---

# systemd_unit - Manage systemd Unit Files

## Synopsis

The `systemd_unit` module manages systemd unit files and service state on Linux hosts.
It can start, stop, enable, and reload units.

## Classification

**RemoteCommand** - Executes systemd operations on the remote host.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Name of the systemd unit (e.g., `nginx.service`). |
| `state` | no | started | string | Desired state: started, stopped, restarted, reloaded. |
| `enabled` | no | - | boolean | Whether the unit should start on boot. |
| `daemon_reload` | no | false | boolean | Reload the systemd manager configuration. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `changed` | boolean | Whether the unit state changed. |
| `status` | string | Final unit status. |

## Examples

### Start and enable a service

```yaml
- name: Ensure nginx is running
  systemd_unit:
    name: nginx.service
    state: started
    enabled: true
```

### Reload systemd daemon after unit changes

```yaml
- name: Reload systemd
  systemd_unit:
    name: myapp.service
    daemon_reload: true
```

## Notes

- Use `daemon_reload` after updating unit files.
- On non-systemd systems, this module is not applicable.
