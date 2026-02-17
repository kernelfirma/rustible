---
summary: Reference for the ipmi_power module that manages server power state via IPMI using ipmitool.
read_when: You need to power on, off, reset, cycle, or query the power status of servers via IPMI from playbooks.
---

# ipmi_power - Manage Server Power State via IPMI

## Synopsis

Manages server power state via IPMI using `ipmitool chassis power`. Supports
power on, off, reset, cycle, and status query operations with idempotency
checks against the current power state.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter | Required | Default    | Type   | Description                                                          |
|-----------|----------|------------|--------|----------------------------------------------------------------------|
| host      | yes      | -          | string | BMC/IPMI hostname or IP address.                                     |
| action    | yes      | -          | string | Power action: `on`, `off`, `reset`, `cycle`, or `status`.           |
| user      | no       | `admin`    | string | IPMI username.                                                       |
| password  | no       | `""`       | string | IPMI password.                                                       |
| interface | no       | `lanplus`  | string | IPMI interface type (e.g. `lanplus`, `lan`).                         |

## Return Values

| Key     | Type    | Description                                            |
|---------|---------|--------------------------------------------------------|
| changed | boolean | Whether the power state was changed.                   |
| msg     | string  | Status message.                                        |
| data    | object  | Contains `host`, `power_state`, `action`, and `previous_state` fields. |

## Examples

```yaml
- name: Power on a server
  ipmi_power:
    host: 10.0.0.101
    action: "on"
    user: admin
    password: "{{ ipmi_password }}"

- name: Power off a server
  ipmi_power:
    host: 10.0.0.101
    action: "off"
    user: admin
    password: "{{ ipmi_password }}"

- name: Reset a server
  ipmi_power:
    host: 10.0.0.101
    action: reset
    user: admin
    password: "{{ ipmi_password }}"

- name: Check power status
  ipmi_power:
    host: 10.0.0.101
    action: status
    user: admin
    password: "{{ ipmi_password }}"
```

## Notes

- Requires building with `--features hpc`.
- Requires `ipmitool` to be installed on the managed host.
- The `on` and `off` actions are idempotent: if the server is already in the desired power state, no change is made.
- The `reset` and `cycle` actions always report `changed: true` since they are inherently non-idempotent.
- The `status` action is read-only and reports the current power state (`on`, `off`, or `unknown`).
- Passwords containing single quotes are automatically escaped.
