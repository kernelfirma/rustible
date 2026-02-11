---
summary: Variable types, scoping rules, and the 20-level precedence hierarchy that determines which value wins.
read_when: You need to understand how variables are resolved, how to override them, or how to organize variable files.
---

# Chapter 4: Variables - Precedence and Scoping

Variables are the mechanism for making playbooks flexible and reusable. Rustible implements a 20-level precedence hierarchy (based on Ansible's model) that determines which value wins when the same variable is defined in multiple places.

## Variable Precedence

The following list shows all precedence levels from lowest (most easily overridden) to highest (wins over everything else). This ordering comes directly from the `VarPrecedence` enum in the Rustible source:

| Level | Source | Priority |
|-------|--------|----------|
| 1 | Role defaults (`defaults/main.yml`) | Lowest |
| 2 | Dynamic inventory group vars | |
| 3 | Inventory file group vars | |
| 4 | Playbook `group_vars/all` | |
| 5 | Playbook `group_vars/*` (specific group) | |
| 6 | Dynamic inventory host vars | |
| 7 | Inventory file host vars | |
| 8 | Playbook `host_vars/*` | |
| 9 | Host facts / cached `set_facts` | |
| 10 | Play `vars:` | |
| 11 | Play `vars_prompt:` | |
| 12 | Play `vars_files:` | |
| 13 | Role vars (`vars/main.yml`) | |
| 14 | Block vars | |
| 15 | Task vars | |
| 16 | `include_vars` | |
| 17 | `set_facts` / registered vars | |
| 18 | Role params (when including a role) | |
| 19 | Include params | |
| 20 | Extra vars (`-e` / `--extra-vars`) | Highest |

The key takeaway: **extra vars always win**, and **role defaults are always overridable**. Everything else falls on a gradient between those two extremes.

## Variable Types

Rustible variables support the standard YAML data types:

```yaml
vars:
  # Strings
  app_name: "myapp"
  greeting: Hello World

  # Numbers
  http_port: 80
  threshold: 0.95

  # Booleans
  ssl_enabled: true
  debug_mode: false

  # Lists
  packages:
    - nginx
    - curl
    - jq

  # Dictionaries (maps)
  database:
    host: db.example.com
    port: 5432
    name: appdb
    ssl: true

  # Nested structures
  app:
    frontend:
      port: 3000
      workers: 4
    backend:
      port: 8080
      workers: 8
```

Access nested values with dot notation or bracket notation in templates:

```yaml
- debug:
    msg: "DB host: {{ database.host }}, port: {{ database['port'] }}"
```

## Variable Scoping

Variables exist in one of three scopes:

### Play Scope

Variables defined in the play's `vars:`, `vars_files:`, or `vars_prompt:` sections are available to all tasks within that play, but not to other plays in the same playbook.

```yaml
- hosts: webservers
  vars:
    app_port: 8080    # Available to all tasks in this play
  tasks:
    - debug:
        msg: "Port: {{ app_port }}"

- hosts: databases
  tasks:
    - debug:
        msg: "Port: {{ app_port }}"  # UNDEFINED - different play
```

### Host Scope

Variables tied to a specific host (inventory host vars, facts, registered vars, `set_facts`) persist across plays for that host within a single playbook run.

```yaml
- hosts: web1
  tasks:
    - set_fact:
        deployment_id: "deploy-123"  # Persists for web1

- hosts: web1
  tasks:
    - debug:
        msg: "{{ deployment_id }}"   # Still available for web1
```

### Task Scope

Variables defined inline on a task apply only to that task:

```yaml
tasks:
  - debug:
      msg: "{{ temp_var }}"
    vars:
      temp_var: "only here"   # Task-scoped

  - debug:
      msg: "{{ temp_var }}"   # UNDEFINED - different task
```

## Registered Variables

Capture the output of any task using the `register` keyword:

```yaml
tasks:
  - name: Check disk space
    command: df -h /
    register: disk_result

  - name: Show disk usage
    debug:
      msg: "{{ disk_result.stdout }}"

  - name: Warn if low space
    debug:
      msg: "Low disk space detected"
    when: "'90%' in disk_result.stdout"
```

Registered variables contain these standard fields:

| Field | Description |
|-------|-------------|
| `changed` | Whether the task reported a change |
| `failed` | Whether the task failed |
| `skipped` | Whether the task was skipped |
| `rc` | Return code (command modules) |
| `stdout` | Standard output as a string |
| `stdout_lines` | Standard output as a list of lines |
| `stderr` | Standard error output |
| `msg` | Module message |
| `data` | Additional module-specific data |

## Facts

Facts are variables automatically gathered from each host at the start of a play (unless `gather_facts: false` is set). They describe the target system's hardware, OS, network, and more.

```yaml
- hosts: all
  gather_facts: true
  tasks:
    - debug:
        msg: >
          OS: {{ ansible_os_family }}
          CPU count: {{ ansible_processor_count }}
          Memory: {{ ansible_memtotal_mb }} MB
          IP: {{ ansible_default_ipv4.address }}
```

Disable fact gathering for speed when facts are not needed:

```yaml
- hosts: all
  gather_facts: false
  tasks:
    - debug:
        msg: "No facts gathered"
```

## Magic Variables

Rustible provides several special variables that are always available:

| Variable | Description |
|----------|-------------|
| `inventory_hostname` | The name of the current host as defined in inventory |
| `ansible_host` | The actual address to connect to |
| `groups` | Dictionary of all groups and their host lists |
| `group_names` | List of groups the current host belongs to |
| `hostvars` | Dictionary of all host variables, keyed by hostname |
| `ansible_play_hosts` | List of hosts in the current play |
| `ansible_play_batch` | Hosts in the current serial batch |
| `play_hosts` | Alias for `ansible_play_hosts` |
| `ansible_check_mode` | `true` when running with `--check` |
| `ansible_diff_mode` | `true` when running with `--diff` |
| `role_name` | Name of the current role (when inside a role) |
| `role_path` | Path to the current role directory |

Access another host's variables through `hostvars`:

```yaml
- debug:
    msg: "DB is at {{ hostvars['db1'].ansible_host }}"
```

## Variable Files

### vars_files Directive

Load variables from external YAML files:

```yaml
- hosts: webservers
  vars_files:
    - vars/common.yml
    - vars/secrets.yml
    - "vars/{{ env }}.yml"    # Dynamic file based on variable
```

### include_vars Module

Load variables dynamically during task execution:

```yaml
tasks:
  - name: Load OS-specific variables
    include_vars:
      file: "vars/{{ ansible_os_family }}.yml"

  - name: Load all variable files from directory
    include_vars:
      dir: vars/overrides
      extensions:
        - yml
        - yaml
```

## Prompting for Variables

Use `vars_prompt` to interactively request values at runtime:

```yaml
- hosts: webservers
  vars_prompt:
    - name: deploy_version
      prompt: "Which version to deploy?"
      default: "latest"
      private: false

    - name: db_password
      prompt: "Database password"
      private: true       # Input is hidden
```

## Extra Variables from the Command Line

Extra variables have the highest precedence and override everything:

```bash
# Key=value format
rustible run playbook.yml -e "env=production version=2.0.0"

# YAML/JSON format
rustible run playbook.yml -e '{"env": "production", "version": "2.0.0"}'

# Load from file (prefix with @)
rustible run playbook.yml -e @vars/deploy.yml

# Multiple -e flags
rustible run playbook.yml -e "env=production" -e "debug=true"
```

## Best Practices

1. **Use role defaults** for values that should be easily overridable. Put truly fixed values in `vars/main.yml`.
2. **Organize by scope**: keep inventory-level variables in `group_vars/` and `host_vars/`, keep play-level variables in `vars_files`.
3. **Never rely on precedence tricks**. If you find yourself depending on subtle precedence ordering, simplify your variable structure instead.
4. **Use `default` filter** for optional variables: `{{ optional_var | default('fallback') }}`.
5. **Encrypt secrets** with Vault rather than storing them in plain-text variable files.
6. **Document your variables** in role `README` files or in `defaults/main.yml` with comments describing each variable's purpose and allowed values.
7. **Use extra vars sparingly** -- they are impossible to override, which can make debugging difficult.

## Next Steps

- Explore [Built-in Modules](05-modules.md)
- Learn about [Roles](06-roles.md)
- Understand [Security and Vault](08-security.md)
