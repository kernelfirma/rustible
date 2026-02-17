---
summary: Reference for the hpc_power module that manages bare-metal server power state via IPMI or Redfish.
read_when: You need to power on, off, reset, cycle, or query server power state from playbooks.
---

# hpc_power - Bare-Metal Power Management

## Synopsis

Manages server power state via IPMI (`ipmitool`) or Redfish REST API. Supports power on, off, reset, cycle, and status queries. The module is idempotent: it checks the current power state before issuing commands and skips actions that would not change state.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| action | **yes** | - | string | Power action: `"on"`, `"off"`, `"reset"`, `"cycle"`, or `"status"`. |
| host | **yes** | - | string | BMC/IPMI host address or hostname. |
| user | no | `"admin"` | string | IPMI or Redfish username. |
| password | no | `""` | string | IPMI or Redfish password. |
| interface | no | `"lanplus"` | string | IPMI interface type: `"lanplus"`, `"lan"`, or `"open"`. Only used with the `ipmi` provider. |
| provider | no | `"ipmi"` | string | Power management provider: `"ipmi"` or `"redfish"`. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether a power state change was executed |
| msg | string | Status message describing the action taken |
| data.power_state | string | Current power state (`"on"`, `"off"`, or `"unknown"`) |
| data.host | string | BMC host address |
| data.action | string | The requested power action |
| data.provider | string | The provider used (`"ipmi"` or `"redfish"`) |
| data.previous_state | string | Power state before the action (only on change) |
| data.reset_type | string | Redfish reset type used (only for Redfish provider) |

## Examples

```yaml
- name: Power on a compute node via IPMI
  hpc_power:
    action: "on"
    host: 10.0.1.100
    user: admin
    password: "{{ ipmi_password }}"

- name: Check power status
  hpc_power:
    action: status
    host: 10.0.1.100
    user: admin
    password: "{{ ipmi_password }}"

- name: Power cycle via Redfish
  hpc_power:
    action: cycle
    host: bmc-node01.cluster.local
    user: admin
    password: "{{ redfish_password }}"
    provider: redfish

- name: Graceful reset via IPMI with open interface
  hpc_power:
    action: reset
    host: 10.0.1.100
    user: admin
    password: "{{ ipmi_password }}"
    interface: open
```

## Notes

- Requires building with `--features hpc`.
- IPMI provider requires `ipmitool` to be installed on the control node.
- Redfish provider uses `curl` to communicate with the BMC REST API over HTTPS.
- Redfish reset type mapping: `on` -> `On`, `off` -> `ForceOff`, `reset` -> `ForceRestart`, `cycle` -> `PowerCycle`.
- Idempotent: `on` on an already-on server and `off` on an already-off server are no-ops.
- `reset` and `cycle` actions always report `changed: true` since they are inherently state-changing.
- `status` never reports `changed: true`.
- Parallelization hint: `FullyParallel` (safe to run on multiple hosts simultaneously).
