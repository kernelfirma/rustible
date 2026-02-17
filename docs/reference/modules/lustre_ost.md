---
summary: Reference for the lustre_ost module that manages Lustre OST lifecycle operations via lctl.
read_when: You need to activate, deactivate, add, or remove Lustre Object Storage Targets from playbooks.
---

# lustre_ost - Manage Lustre OST Lifecycle Operations

## Synopsis

Manage Lustre OST (Object Storage Target) lifecycle operations via the `lctl` command-line utility. Supports activating, deactivating, adding, and removing OSTs with idempotent state checks.

## Classification

**Default** - HPC module. Requires `hpc` and `parallel_fs` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| ost_index | yes | - | string | OST index number |
| target | yes | - | string | Target device or filesystem name |
| action | yes | - | string | Operation to perform: "activate", "deactivate", "add", or "remove" |
| mdt_index | no | null | string | MDT index for coordinated add/remove operations |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.ost | string | The OST name in TARGET-OSTxxxx format |
| data.action | string | The action that was executed |
| data.previous_state | string | The state of the OST before the action was applied |

## Examples

```yaml
- name: Activate an OST
  lustre_ost:
    ost_index: "0"
    target: "lustre-fs"
    action: activate

- name: Deactivate an OST for maintenance
  lustre_ost:
    ost_index: "3"
    target: "lustre-fs"
    action: deactivate

- name: Add an OST with MDT coordination
  lustre_ost:
    ost_index: "5"
    target: "lustre-fs"
    action: add
    mdt_index: "0"

- name: Remove an OST
  lustre_ost:
    ost_index: "2"
    target: "lustre-fs"
    action: remove
```

## Notes

- Requires building with `--features hpc,parallel_fs` or `--features full-hpc`.
- The `lctl` command must be available on the target host (Lustre client utilities must be installed).
- Activate and deactivate actions are idempotent; if the OST is already in the desired state, no change is made.
- Add and remove actions always execute regardless of current state.
- The OST name is derived as `TARGET-OSTxxxx` where `xxxx` is the zero-padded hex representation of the index.
- This module uses `HostExclusive` parallelization, meaning only one instance runs per host at a time.
