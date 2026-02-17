---
summary: Reference for the slurm_node module that manages Slurm compute node states via scontrol.
read_when: You need to drain, resume, or change the state of Slurm compute nodes from playbooks.
---

# slurm_node - Manage Slurm Node State

## Synopsis

Manages Slurm compute node states via `scontrol`. Supports draining, resuming,
setting down, and idling nodes with idempotency checks against the current node
state. Requires a reason string for drain and down operations.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter | Required | Default | Type   | Description                                                                     |
|-----------|----------|---------|--------|---------------------------------------------------------------------------------|
| name      | yes      | -       | string | Node name.                                                                      |
| state     | yes      | -       | string | Desired state: `drain`, `resume`, `down`, `idle`, or `undrain`.                 |
| reason    | no       | -       | string | Reason for the state change. Required for `drain` and `down` actions.           |
| weight    | no       | -       | string | Node scheduling weight.                                                         |
| features  | no       | -       | string | Node features/attributes.                                                       |

## Return Values

| Key     | Type    | Description                                                         |
|---------|---------|---------------------------------------------------------------------|
| changed | boolean | Whether the node state was changed.                                 |
| msg     | string  | Status message.                                                     |
| data    | object  | Contains `node`, `new_state`/`state`, and `previous_state` fields.  |

## Examples

```yaml
- name: Drain a node for maintenance
  slurm_node:
    name: node01
    state: drain
    reason: "Firmware update"

- name: Resume a drained node
  slurm_node:
    name: node01
    state: resume

- name: Mark a node as down
  slurm_node:
    name: node02
    state: down
    reason: "Hardware failure"

- name: Set node features and weight
  slurm_node:
    name: gpu-node01
    state: idle
    weight: "100"
    features: "gpu,nvlink"
```

## Notes

- Requires building with `--features hpc,slurm`.
- The `reason` parameter is required when setting state to `drain` or `down`; the module will return an error if omitted.
- All state transitions are idempotent: if the node is already in the desired state, no change is made.
- The module queries the current node state via `scontrol show node` before performing any action.
- Slurm compound states (e.g. `IDLE+DRAIN`, `ALLOCATED+DRAIN`) are parsed correctly.
