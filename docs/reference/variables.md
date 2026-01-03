---
summary: Complete variable system reference including 20-level precedence order, special variables, scoping rules, and cross-host access patterns.
read_when: You need to understand variable precedence, access other hosts' variables, or debug unexpected variable values.
---

# Variables in Rustible

This document describes the variable system in Rustible, including precedence rules, special variables, and scoping behavior.

## Variable Precedence

Rustible follows Ansible's variable precedence model. Variables are evaluated from lowest to highest precedence, with higher precedence values overriding lower ones.

### Precedence Order (Lowest to Highest)

| Level | Source | Description |
|-------|--------|-------------|
| 1 | Role defaults | `roles/x/defaults/main.yml` - Lowest priority |
| 2 | Inventory group vars | Dynamic inventory group variables |
| 3 | Inventory file group vars | Group vars defined in inventory file |
| 4 | Playbook group_vars/all | `group_vars/all.yml` in playbook directory |
| 5 | Playbook group_vars/* | `group_vars/<groupname>.yml` |
| 6 | Inventory host vars | Dynamic inventory host variables |
| 7 | Inventory file host vars | Host vars defined in inventory file |
| 8 | Playbook host_vars/* | `host_vars/<hostname>.yml` |
| 9 | Host facts | Facts gathered from hosts / cached set_facts |
| 10 | Play vars | Variables defined in play with `vars:` |
| 11 | Play vars_prompt | Variables from `vars_prompt:` |
| 12 | Play vars_files | Variables from `vars_files:` |
| 13 | Role vars | `roles/x/vars/main.yml` |
| 14 | Block vars | Variables defined in a block |
| 15 | Task vars | Variables defined on a task |
| 16 | Include vars | Variables from `include_vars` module |
| 17 | Set facts | `set_fact` and registered variables |
| 18 | Role params | Parameters passed when including a role |
| 19 | Include params | Parameters passed to includes |
| 20 | Extra vars | `-e` / `--extra-vars` - Highest priority |

### Key Precedence Rules

1. **Extra vars always win**: Command-line extra vars (`-e`) have the highest precedence and cannot be overridden.

2. **Role defaults are fallbacks**: Role defaults are intended as fallback values and can be overridden by almost anything.

3. **Host vars override group vars**: Host-specific variables always take precedence over group variables.

4. **More specific groups win**: In group variable resolution, child groups override parent groups.

5. **Task scope is temporary**: Task-level variables only apply to that specific task.

## Special Variables

Rustible provides several special (magic) variables that are automatically available during playbook execution.

### Host-Related Variables

| Variable | Description |
|----------|-------------|
| `inventory_hostname` | Name of the current host as defined in inventory |
| `inventory_hostname_short` | First component of hostname (before first `.`) |
| `ansible_host` | Actual connection address for the host |
| `group_names` | List of groups the current host belongs to |

### Group-Related Variables

| Variable | Description |
|----------|-------------|
| `groups` | Dictionary of all groups with their member hosts |
| `groups['all']` | List of all hosts in the inventory |
| `groups['groupname']` | List of hosts in a specific group |

### Cross-Host Access

| Variable | Description |
|----------|-------------|
| `hostvars` | Dictionary containing variables for all hosts |
| `hostvars['hostname']['varname']` | Access another host's variable |

### Play-Related Variables

| Variable | Description |
|----------|-------------|
| `ansible_play_hosts` | All hosts in the current play |
| `ansible_play_hosts_all` | All hosts in the current play (including failed) |
| `ansible_play_batch` | Current batch when using `serial` |
| `playbook_dir` | Directory containing the playbook |
| `role_path` | Path to the current role (when in a role) |

### Connection Variables

| Variable | Description |
|----------|-------------|
| `ansible_connection` | Connection type (ssh, local, docker, etc.) |
| `ansible_user` | User for SSH connections |
| `ansible_port` | Port for SSH connections (default: 22) |
| `ansible_become` | Whether to use privilege escalation |
| `ansible_become_method` | Method for privilege escalation (sudo, su) |
| `ansible_become_user` | Target user for privilege escalation |

### Facts Variables

| Variable | Description |
|----------|-------------|
| `ansible_facts` | Dictionary of gathered facts |
| `ansible_facts.distribution` | OS distribution name |
| `ansible_facts.os_family` | OS family (Debian, RedHat, etc.) |
| `ansible_facts.architecture` | System architecture |

## Variable Scoping

### Play Scope

Variables defined at the play level are available to all tasks in that play:

```yaml
- hosts: webservers
  vars:
    http_port: 80
  tasks:
    - debug:
        msg: "Port is {{ http_port }}"
```

### Block Scope

Variables defined in a block are only available within that block:

```yaml
tasks:
  - block:
      - debug:
          msg: "Block var is {{ block_var }}"
    vars:
      block_var: "only in block"
```

### Task Scope

Variables defined on a task are only available to that task:

```yaml
tasks:
  - debug:
      msg: "Task var is {{ task_var }}"
    vars:
      task_var: "only this task"
```

### Loop Variables

When using loops, the current item is available as `item` (or a custom name):

```yaml
tasks:
  - debug:
      msg: "Item is {{ item }}"
    loop:
      - one
      - two
      - three
```

With custom loop variable:

```yaml
tasks:
  - debug:
      msg: "Package is {{ pkg }}"
    loop:
      - nginx
      - apache
    loop_control:
      loop_var: pkg
```

## Accessing Other Hosts' Variables

Use `hostvars` to access variables from other hosts:

```yaml
tasks:
  - debug:
      msg: "DB host is {{ hostvars['db-server']['ansible_host'] }}"
```

This is useful for:
- Configuring services that need to know about other hosts
- Building configuration files with addresses of related services
- Conditional logic based on other hosts' states

## Registered Variables

Task results can be captured with `register`:

```yaml
tasks:
  - command: hostname
    register: hostname_result

  - debug:
      msg: "Hostname is {{ hostname_result.stdout }}"
```

Registered variables contain:
- `changed`: Whether the task made changes
- `failed`: Whether the task failed
- `skipped`: Whether the task was skipped
- `rc`: Return code (for command modules)
- `stdout`: Standard output
- `stdout_lines`: Standard output as a list of lines
- `stderr`: Standard error
- `msg`: Module message

## Set_fact Module

Use `set_fact` to create new variables during execution:

```yaml
tasks:
  - set_fact:
      my_var: "computed value"
      another_var: "{{ some_var | upper }}"
```

Set_fact variables have high precedence and persist for the rest of the play.

## Variable Merging

### Hash Behavior

By default, when the same variable is defined at multiple precedence levels, the higher precedence value completely replaces the lower one.

For dictionaries (hashes), you can enable recursive merging:

```yaml
# With hash_behaviour: replace (default)
base_config:
  setting1: value1
  setting2: value2

# Higher precedence definition
base_config:
  setting2: new_value
  setting3: value3

# Result: only setting2 and setting3 are present
```

### Deep Merging

When merging is enabled, dictionaries are merged recursively:

```yaml
# With hash_behaviour: merge
base_config:
  setting1: value1
  setting2: value2

# Higher precedence definition
base_config:
  setting2: new_value
  setting3: value3

# Result: setting1, setting2 (new), and setting3 are all present
```

## Variable Files

### vars_files

Load variables from external files:

```yaml
- hosts: all
  vars_files:
    - vars/common.yml
    - vars/{{ ansible_os_family }}.yml
```

### include_vars Module

Dynamically load variables during execution:

```yaml
tasks:
  - include_vars:
      file: vars/secrets.yml
      name: secrets
```

## Best Practices

1. **Use role defaults for truly optional values**: Put default values in `defaults/main.yml` so they can be easily overridden.

2. **Use group_vars for environment-specific settings**: Define environment differences (dev, staging, prod) in group_vars.

3. **Use host_vars sparingly**: Only use host_vars for truly host-specific settings.

4. **Avoid extra vars in automation**: Extra vars make playbooks less predictable.

5. **Document your variables**: Use comments to explain what each variable does.

6. **Use meaningful names**: Variable names should clearly indicate their purpose.

7. **Namespace role variables**: Prefix role variables with the role name to avoid conflicts.

## Examples

### Complete Precedence Example

```yaml
# Role defaults (roles/webserver/defaults/main.yml)
http_port: 80

# Group vars (group_vars/webservers.yml)
http_port: 8080

# Host vars (host_vars/web1.yml)
http_port: 8888

# Play vars
- hosts: webservers
  vars:
    http_port: 9000
  tasks:
    - debug:
        msg: "Port is {{ http_port }}"  # Shows 9000
```

### Cross-Host Variable Access

```yaml
- hosts: webservers
  tasks:
    - name: Configure app to use database
      template:
        src: app.conf.j2
        dest: /etc/myapp/app.conf
      vars:
        db_host: "{{ hostvars[groups['databases'][0]]['ansible_host'] }}"
```

### Conditional with Group Membership

```yaml
- hosts: all
  tasks:
    - name: Install web packages
      package:
        name: nginx
      when: "'webservers' in group_names"
```

## See Also

- [Inventory Documentation](./inventory.md)
- [Playbook Documentation](./playbooks.md)
- [Module Documentation](./modules/README.md)
