---
summary: Reference for the redfish_power module that manages server power state via Redfish REST API.
read_when: You need to power on, power off, reset, cycle, or query server power status from playbooks.
---

# redfish_power - Manage Server Power State via Redfish

## Synopsis

Manage server power state through Redfish-compliant BMC REST API endpoints. Supports power on, power off, force restart, power cycle, and status queries with idempotent state checks. Uses curl for REST API calls.

## Classification

**Default** - HPC module. Requires `hpc` and `redfish` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| host | yes | - | string | BMC hostname or IP address |
| action | yes | - | string | Power action: "on", "off", "reset", "cycle", or "status" |
| user | no | "admin" | string | Redfish authentication username |
| password | no | "" | string | Redfish authentication password |
| verify_ssl | no | false | boolean | Whether to verify SSL certificates |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made (always false for "status") |
| msg | string | Status message |
| data.host | string | The BMC host address |
| data.power_state | string | Current power state ("on", "off", or "unknown") |
| data.action | string | The action that was requested |
| data.reset_type | string | The Redfish ResetType used (e.g., "On", "ForceOff", "ForceRestart", "PowerCycle") |
| data.previous_state | string | Power state before the action was applied |

## Examples

```yaml
- name: Power on a server
  redfish_power:
    host: "bmc-compute-001.hpc.local"
    action: "on"
    user: "admin"
    password: "secret"

- name: Force restart a server
  redfish_power:
    host: "10.0.1.100"
    action: reset
    user: admin
    password: "{{ bmc_password }}"
    verify_ssl: false

- name: Query power status
  redfish_power:
    host: "bmc-compute-001.hpc.local"
    action: status
    user: admin
    password: "{{ bmc_password }}"

- name: Power off a server
  redfish_power:
    host: "bmc-compute-001.hpc.local"
    action: "off"
    user: admin
    password: "{{ bmc_password }}"
```

## Notes

- Requires building with `--features hpc,redfish` or `--features full-hpc`.
- The `curl` command must be available on the target host.
- Power actions map to Redfish ResetType values: "on" -> "On", "off" -> "ForceOff", "reset" -> "ForceRestart", "cycle" -> "PowerCycle".
- The "status" action queries the current power state without making changes.
- Power on/off actions are idempotent; if the server is already in the desired state, no action is taken.
- Reset and cycle actions always execute regardless of current state.
- SSL verification is disabled by default for self-signed BMC certificates.
- This module uses `FullyParallel` parallelization, meaning multiple BMC operations can run concurrently.
