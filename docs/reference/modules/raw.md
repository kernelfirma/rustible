---
summary: Reference for the raw module that executes commands directly on remote hosts without shell processing.
read_when: You need to run low-level commands on hosts where Python or the module subsystem is unavailable.
---

# raw - Execute Raw Commands

## Synopsis
Executes commands directly on the remote system over SSH without any additional processing, escaping, or shell wrapper. Designed for bootstrapping systems before full module support is available, or for managing network devices and non-standard systems.

## Classification
**RemoteCommand** - sends commands directly to the remote host. Fully parallelizable across hosts.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| free_form | yes | - | string | The command to execute on the remote host |
| executable | no | - | string | Path to the shell/interpreter to use for the command |

The command can also be specified via the `raw`, `cmd`, or `_raw_params` keys.

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| stdout | string | Standard output from the command |
| stderr | string | Standard error from the command |
| rc | integer | Return code from the command |
| stdout_lines | list | Standard output split into lines |
| stderr_lines | list | Standard error split into lines |
| cmd | string | The command that was executed |

## Examples
```yaml
- name: Bootstrap Python on a minimal system
  raw: apt-get update && apt-get install -y python3

- name: Run command on a network device
  raw: show running-config

- name: Use a specific shell
  raw: echo $SHELL
  args:
    executable: /bin/bash

- name: Install package on AIX
  raw: /usr/bin/rpm -ivh python3.rpm
```

## Notes
- The raw module always reports `changed: true` on success since it cannot determine idempotency.
- In check mode, the command is not executed; the module reports what would run.
- Unlike the `command` or `shell` modules, raw does not use a module wrapper on the remote side.
- When `executable` is set, the command is wrapped as `<executable> -c '<command>'`.
- A connection to the remote host is required; the module fails without one.
