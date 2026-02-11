---
summary: Reference for the selinux module that manages SELinux mode, booleans, file contexts, and port types.
read_when: You need to configure SELinux enforcement, booleans, or security contexts from playbooks.
---

# selinux - Manage SELinux Configuration

## Synopsis
Provides comprehensive SELinux management including mode control (enforcing/permissive/disabled), boolean management, file context configuration, and port type definitions. The operation type is determined by which parameters are supplied.

## Classification
**RemoteCommand** - executes SELinux tools (getenforce, setenforce, setsebool, chcon, semanage) on the remote host.

## Parameters

### Mode Management
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| state | yes | - | string | SELinux mode: `enforcing`, `permissive`, `disabled` |
| policy | no | - | string | SELinux policy name (e.g., `targeted`, `mls`) |
| configfile | no | /etc/selinux/config | string | Path to the SELinux configuration file |

### Boolean Management
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| boolean | yes | - | string | Boolean name to manage (e.g., `httpd_can_network_connect`) |
| boolean_state | yes | - | string | Boolean value: `on`, `off`, `true`, `false`, `1`, `0` |
| persistent | no | true | bool | Make the boolean change persistent across reboots |

### Context Management
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| target | yes | - | string | Target file or directory path |
| setype | no | - | string | SELinux type (e.g., `httpd_sys_content_t`) |
| seuser | no | - | string | SELinux user (e.g., `system_u`) |
| selevel | no | - | string | SELinux level/range (e.g., `s0`) |
| serole | no | - | string | SELinux role (e.g., `object_r`) |
| ftype | no | a | string | File type for fcontext: `a`, `f`, `d`, `c`, `b`, `s`, `l`, `p` |
| recursive | no | false | bool | Apply context change recursively |
| reload | no | true | bool | Run restorecon after fcontext change |

### Port Management
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| ports | yes | - | string | Port(s) to manage (single, range `1000-2000`, or comma-separated) |
| proto | yes | - | string | Protocol: `tcp`, `udp`, `dccp`, `sctp` |
| port_type | yes | - | string | SELinux port type (e.g., `http_port_t`) |
| port_state | no | present | string | `present` or `absent` |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| mode | string | Current/new SELinux mode |
| previous_mode | string | Mode before the change |
| reboot_required | bool | Whether a reboot is needed (disabled/enabled transitions) |
| name | string | Boolean name (boolean operations) |
| value | bool | Boolean value (boolean operations) |
| target | string | Target path (context operations) |
| ports | list | Changed ports (port operations) |

## Examples
```yaml
- name: Set SELinux to enforcing
  selinux:
    state: enforcing

- name: Enable HTTPD network connections
  selinux:
    boolean: httpd_can_network_connect
    boolean_state: "on"
    persistent: true

- name: Set web content context
  selinux:
    target: /srv/www
    setype: httpd_sys_content_t
    recursive: true

- name: Allow custom port for HTTP
  selinux:
    ports: "8080"
    proto: tcp
    port_type: http_port_t
```

## Notes
- The module auto-detects the operation type based on which parameters are present.
- Switching to/from `disabled` mode requires a reboot to take effect.
- SELinux type names must end with `_t`, user names with `_u`, role names with `_r`.
- The module verifies that SELinux tools (getenforce) are available before proceeding.
- Port numbers must be in the range 1-65535.
