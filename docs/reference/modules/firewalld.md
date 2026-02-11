---
summary: Reference for the firewalld module that manages firewall rules using firewalld.
read_when: You need to configure firewalld zones, services, ports, or rich rules from playbooks.
---

# firewalld - Manage Firewalld Rules

## Synopsis
Manages firewall rules on systems using firewalld (Red Hat, Fedora, CentOS). Supports zone management, service and port configuration, source/interface binding, masquerading, rich rules, ICMP blocks, and zone targets.

## Classification
**RemoteCommand** - executes firewall-cmd on the remote host. Host-exclusive parallelization to avoid conflicts.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| service | no | - | string | Service name to add/remove (e.g., `http`, `ssh`) |
| port | no | - | string | Port/protocol spec (e.g., `8080/tcp`, `53/udp`, `8000-9000/tcp`) |
| zone | no | public | string | Firewalld zone to operate on |
| state | no | enabled | string | Desired state: `enabled`, `disabled`, `present`, `absent` |
| permanent | no | true | bool | Make changes permanent across reboots |
| immediate | no | true | bool | Apply changes to running configuration immediately |
| rich_rule | no | - | string | Rich rule specification |
| source | no | - | string | Source IP/network to add to zone |
| interface | no | - | string | Network interface to bind to zone |
| masquerade | no | - | bool | Enable or disable masquerading |
| icmp_block | no | - | string | ICMP type to block |
| icmp_block_inversion | no | - | bool | Invert ICMP block behavior |
| target | no | - | string | Zone default target: `default`, `ACCEPT`, `DROP`, `REJECT` |
| offline | no | false | bool | Run in offline mode without firewalld daemon |
| timeout | no | - | integer | Timeout in seconds for temporary rules |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| zone | string | The zone that was modified |
| changed | bool | Whether any firewall rules were changed |
| msg | string | Description of actions taken |

## Examples
```yaml
- name: Allow HTTP service in public zone
  firewalld:
    service: http
    zone: public
    state: enabled
    permanent: true
    immediate: true

- name: Open custom port
  firewalld:
    port: 8443/tcp
    zone: public
    state: enabled

- name: Add rich rule for rate limiting
  firewalld:
    rich_rule: 'rule family="ipv4" source address="10.0.0.0/8" service name="ssh" accept'
    zone: public
    state: enabled

- name: Remove a service
  firewalld:
    service: telnet
    zone: public
    state: disabled
```

## Notes
- At least one of service, port, source, interface, masquerade, rich_rule, icmp_block, icmp_block_inversion, or target must be specified.
- When both `permanent` and `immediate` are true, the module reloads firewalld after making permanent changes.
- The module verifies that firewalld is installed and running before making changes.
- Zone names must start with a letter and contain only alphanumeric characters, underscores, and hyphens.
