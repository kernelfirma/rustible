---
summary: Reference for the ufw module that manages firewall rules using UFW (Uncomplicated Firewall).
read_when: You need to configure UFW firewall rules, default policies, or state from playbooks.
---

# ufw - Manage UFW Firewall

## Synopsis
Manages firewall rules on Ubuntu/Debian systems using UFW. Supports enabling/disabling the firewall, adding and removing rules, setting default policies, application profiles, and logging configuration.

## Classification
**RemoteCommand** - executes ufw commands on the remote host. Host-exclusive parallelization to avoid conflicts.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| rule | no | - | string | Rule action: `allow`, `deny`, `reject`, `limit` |
| port | no | - | string | Port number or range (e.g., `22`, `8000:9000`) |
| proto | no | - | string | Protocol: `tcp`, `udp`, `any` |
| from_ip | no | - | string | Source IP address or subnet (e.g., `192.168.1.0/24`, `any`) |
| to_ip | no | - | string | Destination IP address or subnet |
| direction | no | - | string | Traffic direction: `in`, `out`, `routed` |
| state | no | - | string | Firewall state: `enabled`, `disabled`, `reset`, `reloaded` |
| delete | no | false | bool | Delete the specified rule instead of adding it |
| interface | no | - | string | Network interface (e.g., `eth0`) |
| interface_in | no | - | string | Incoming interface for routed rules |
| interface_out | no | - | string | Outgoing interface for routed rules |
| route | no | false | bool | Enable routing mode for the rule |
| app | no | - | string | UFW application profile name (e.g., `OpenSSH`, `Apache Full`) |
| comment | no | - | string | Comment for the rule |
| log | no | - | bool | Enable logging for this rule |
| log_level | no | - | string | Logging level: `off`, `low`, `medium`, `high`, `full` |
| default | no | - | string | Default policy: `allow`, `deny`, `reject` (used with direction) |
| insert | no | - | integer | Insert rule at a specific position number |
| from_port | no | - | string | Source port |
| to_port | no | - | string | Destination port |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| changed | bool | Whether firewall configuration was modified |
| msg | string | Description of actions taken |

## Examples
```yaml
- name: Allow SSH
  ufw:
    rule: allow
    port: "22"
    proto: tcp

- name: Enable UFW
  ufw:
    state: enabled

- name: Set default incoming policy to deny
  ufw:
    default: deny
    direction: in

- name: Allow from specific subnet
  ufw:
    rule: allow
    from_ip: 192.168.1.0/24
    port: "443"
    proto: tcp

- name: Rate limit SSH connections
  ufw:
    rule: limit
    port: "22"
    proto: tcp

- name: Delete a rule
  ufw:
    rule: allow
    port: "8080"
    delete: true

- name: Allow application profile
  ufw:
    rule: allow
    app: "Nginx Full"
```

## Notes
- At least one of `state`, `default`, or `rule`/`app` must be specified.
- The module checks if UFW is installed before executing any commands.
- Port ranges use colon syntax (`8000:9000`) unlike firewalld which uses dash syntax.
- Rule existence is checked before adding/deleting to ensure idempotency.
- The `--force` flag is used for enable/disable/reset to avoid interactive prompts.
- Application profiles must start with a letter and contain only alphanumeric characters, spaces, underscores, and hyphens.
