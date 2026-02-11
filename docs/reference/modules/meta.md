---
summary: Reference for the meta module that controls playbook execution flow with internal actions.
read_when: You need to flush handlers, end plays, clear facts, or reset connections from playbooks.
---

# meta - Playbook Flow Control Actions

## Synopsis
Executes meta-level operations that control playbook execution rather than modifying target hosts. Actions run entirely on the control node and affect the playbook execution flow.

## Classification
**LocalLogic** - runs on the control node only, fully parallelizable.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| free_form | yes | - | string | The meta action to execute (see supported actions below) |

Supported actions for `free_form`:
- `flush_handlers` - Run all pending handlers immediately
- `end_host` - End play for the current host, continue on others
- `end_play` - End the current play entirely
- `end_batch` - End the current batch of hosts
- `clear_facts` - Clear all gathered facts for the current host
- `clear_host_errors` - Clear failure status for the current host
- `refresh_inventory` - Re-read inventory from sources
- `reset_connection` - Reset the SSH/connection to the host
- `noop` - No operation (placeholder)

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| meta_action | string | The action that was executed |
| flush_handlers | bool | True when handlers were flushed |
| end_host | bool | True when host play ended |
| end_play | bool | True when play ended |
| end_batch | bool | True when batch ended |
| clear_facts | bool | True when facts were cleared |
| clear_host_errors | bool | True when host errors were cleared |
| refresh_inventory | bool | True when inventory refresh was requested |
| reset_connection | bool | True when connection reset was requested |

## Examples
```yaml
- name: Flush handlers now
  meta: flush_handlers

- name: End play for this host if condition met
  meta: end_host
  when: skip_remaining_tasks

- name: Clear gathered facts
  meta: clear_facts

- name: Reset SSH connection after user changes
  meta: reset_connection
```

## Notes
- The action can be specified via the `meta`, `action`, or `free_form` parameter keys.
- Terminal actions (`end_host`, `end_play`, `end_batch`) stop further task execution.
- `clear_facts` is the only action that reports `changed: true`; all others report `ok`.
- Meta actions do not require a connection since they execute on the control node.
