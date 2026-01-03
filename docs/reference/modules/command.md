---
summary: Reference for the command module that executes commands on remote hosts without shell processing.
read_when: You need to run simple commands on remote hosts without shell features like pipes or redirects.
---

# command - Execute Commands

## Synopsis

The `command` module executes commands on remote hosts. Unlike the `shell` module, it does not process commands through a shell, so variables like `$HOME` and operations like `<`, `>`, `|`, `;` and `&` will not work.

Use the `shell` module if you need those features.

## Classification

**RemoteCommand** - This module executes commands on remote hosts via SSH.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `cmd` | yes* | - | string | The command to run. Either this or `argv` is required. |
| `argv` | yes* | - | list | Pass the command as a list rather than a string. |
| `chdir` | no | - | string | Change into this directory before running the command. |
| `creates` | no | - | string | A filename or glob pattern. If it exists, this step will not run. |
| `removes` | no | - | string | A filename or glob pattern. If it does NOT exist, this step will not run. |
| `stdin` | no | - | string | Set stdin of the command directly to the specified value. |

*Either `cmd` or `argv` must be provided.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `cmd` | string | The command that was executed |
| `stdout` | string | The standard output of the command |
| `stderr` | string | The standard error output of the command |
| `rc` | integer | The return code of the command |
| `start` | string | The timestamp when the command started |
| `end` | string | The timestamp when the command ended |
| `delta` | string | The time elapsed during command execution |

## Examples

### Run a simple command

```yaml
- name: Get uptime
  command:
    cmd: uptime
```

### Run a command with chdir

```yaml
- name: Run a command in a specific directory
  command:
    cmd: ls -la
    chdir: /var/log
```

### Run a command only if a file does not exist

```yaml
- name: Initialize database only if not already done
  command:
    cmd: /usr/local/bin/init-db.sh
    creates: /var/lib/myapp/db_initialized
```

### Run a command only if a file exists

```yaml
- name: Clean up old logs if they exist
  command:
    cmd: rm -f /var/log/myapp/*.old
    removes: /var/log/myapp/*.old
```

### Use argv for commands with special characters

```yaml
- name: Echo a message with special characters
  command:
    argv:
      - echo
      - "Hello, World!"
```

## Notes

- The `command` module does not use a shell, so shell-specific syntax will not work
- For shell features like pipes, redirects, or environment variables, use the `shell` module
- The module is idempotent when using `creates` or `removes` parameters
- Return code 0 indicates success; any other code indicates failure
- The command is marked as `changed` when it runs successfully

## Real-World Use Cases

### Database Initialization

```yaml
- name: Initialize PostgreSQL database
  command:
    cmd: /usr/pgsql-14/bin/initdb -D /var/lib/pgsql/14/data
    creates: /var/lib/pgsql/14/data/PG_VERSION
  become: yes
  become_user: postgres
```

### Application Health Check

```yaml
- name: Check application health
  command:
    cmd: /opt/myapp/bin/healthcheck
    chdir: /opt/myapp
  register: health_result
  failed_when: health_result.rc != 0
```

### Certificate Generation

```yaml
- name: Generate self-signed certificate
  command:
    argv:
      - openssl
      - req
      - -x509
      - -nodes
      - -newkey
      - rsa:4096
      - -keyout
      - /etc/ssl/private/server.key
      - -out
      - /etc/ssl/certs/server.crt
      - -days
      - "365"
      - -subj
      - "/CN={{ ansible_fqdn }}"
    creates: /etc/ssl/certs/server.crt
```

### Service Pre-flight Check

```yaml
- name: Verify service configuration before restart
  command:
    cmd: nginx -t
  register: nginx_test
  changed_when: false
```

## Troubleshooting

### Command not found

The command must be available in the system PATH or specified with full path:

```bash
# Check if command exists
which mycommand
type -a mycommand
```

Solution: Use the full path to the executable:

```yaml
- name: Run with full path
  command:
    cmd: /usr/local/bin/mycommand
```

### Exit code indicates failure

Non-zero exit codes cause the task to fail. Handle expected non-zero codes:

```yaml
- name: Command that may return non-zero
  command:
    cmd: grep pattern /etc/file
  register: result
  failed_when: result.rc > 1  # Only fail if rc > 1
  changed_when: false
```

### Shell features not working

The command module does NOT support shell features. If you need pipes, redirects, or environment variables:

```yaml
# WRONG - will not work
- command:
    cmd: echo $HOME | grep user

# CORRECT - use shell module instead
- shell:
    cmd: echo $HOME | grep user
```

### Command runs but task shows changed every time

Commands always report changed when they run. Use `changed_when` to control this:

```yaml
- name: Check version (read-only operation)
  command:
    cmd: myapp --version
  register: version_result
  changed_when: false
```

### Permission denied

Ensure the command is executable and you have proper permissions:

```yaml
- name: Run with privilege escalation
  command:
    cmd: /usr/local/bin/admin-command
  become: yes
```

### Working directory issues

The `chdir` parameter changes directory before running the command:

```yaml
- name: Run in specific directory
  command:
    cmd: ./relative-script.sh
    chdir: /opt/myapp
```

## See Also

- [shell](shell.md) - Execute shell commands with full shell features
- [script](script.md) - Run local scripts on remote hosts
- [service](service.md) - Manage services
- [assert](assert.md) - Assert conditions based on command output
