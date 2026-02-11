---
summary: Reference for the script module that transfers and executes local scripts on remote hosts.
read_when: You need to run a local script on remote machines from playbooks.
---

# script - Transfer and Execute Local Scripts

## Synopsis
Copies a script from the control machine to the target host, makes it executable, runs it, and then removes it. Useful for running complex scripts that exist locally on remote hosts.

## Classification
**NativeTransport** - uploads then executes via the connection. Fully parallelizable across hosts.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| free_form | yes | - | string | Path to the local script, optionally followed by arguments |
| chdir | no | - | string | Change to this directory on the remote host before executing |
| creates | no | - | string | Remote path; if it exists, skip execution |
| removes | no | - | string | Remote path; if it does not exist, skip execution |
| executable | no | - | string | Override the interpreter (e.g., `/usr/bin/python3`) |

The script path can also be specified via the `script`, `src`, or `_raw_params` keys.

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| stdout | string | Standard output from the script |
| stderr | string | Standard error from the script |
| rc | integer | Return code from the script |
| stdout_lines | list | Standard output split into lines |
| stderr_lines | list | Standard error split into lines |
| script | string | Path to the local script |
| args | list | Arguments passed to the script |

## Examples
```yaml
- name: Run a local shell script on remote hosts
  script: /opt/scripts/setup.sh

- name: Run a script with arguments
  script: /opt/scripts/deploy.sh --env production --verbose

- name: Run a Python script with a specific interpreter
  script: /opt/scripts/check.py
  args:
    executable: /usr/bin/python3

- name: Run script only if marker file is absent
  script: /opt/scripts/initialize.sh
  args:
    creates: /var/lib/app/.initialized

- name: Run script from a specific working directory
  script: /opt/scripts/build.sh
  args:
    chdir: /srv/app
```

## Notes
- The script is uploaded to a temporary location on the remote host and removed after execution.
- Arguments in the free-form string are split by whitespace; the first token is the script path.
- The `creates` and `removes` conditions are checked before uploading the script.
- The `executable` parameter is validated against shell injection patterns.
- In check mode, the script is not executed; the module reports what would happen.
