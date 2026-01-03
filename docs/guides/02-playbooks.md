---
summary: Complete guide to playbook structure including plays, tasks, handlers, blocks, variables, conditionals, loops, and tags.
read_when: You need to understand how to write and structure playbooks for automation tasks.
---

# Chapter 2: Playbooks

Playbooks are the core of Rustible automation. They define what tasks to execute on which hosts and in what order.

## Playbook Structure

A playbook is a YAML file containing one or more plays:

```yaml
---
# A playbook can contain multiple plays
- name: First play
  hosts: webservers
  tasks:
    - name: Task 1
      debug:
        msg: "Hello from play 1"

- name: Second play
  hosts: databases
  tasks:
    - name: Task 2
      debug:
        msg: "Hello from play 2"
```

## Play Anatomy

Each play has several components:

```yaml
- name: Configure web servers          # Play name (for logging)
  hosts: webservers                     # Target hosts/groups
  become: true                          # Privilege escalation
  become_user: root                     # User to become
  gather_facts: true                    # Collect host facts

  vars:                                 # Play-level variables
    http_port: 80
    app_name: myapp

  vars_files:                           # External variable files
    - vars/common.yml
    - vars/secrets.yml

  pre_tasks:                            # Tasks before roles
    - name: Update apt cache
      apt:
        update_cache: yes

  roles:                                # Roles to apply
    - common
    - webserver

  tasks:                                # Main tasks
    - name: Ensure nginx is running
      service:
        name: nginx
        state: started

  post_tasks:                           # Tasks after main tasks
    - name: Verify service
      uri:
        url: http://localhost
        status_code: 200

  handlers:                             # Handlers for notifications
    - name: Restart nginx
      service:
        name: nginx
        state: restarted
```

### Execution Order

Within a play, components execute in this order:

1. `pre_tasks`
2. Handlers notified by pre_tasks
3. `roles`
4. `tasks`
5. Handlers notified by roles and tasks
6. `post_tasks`
7. Handlers notified by post_tasks

## Tasks

Tasks are the basic unit of work in a playbook.

### Basic Task Structure

```yaml
tasks:
  - name: Install nginx                 # Task name (required for clarity)
    package:                            # Module to use
      name: nginx                       # Module parameters
      state: present
```

### Task Attributes

```yaml
tasks:
  - name: Install packages
    package:
      name: "{{ item }}"
      state: present
    loop:                               # Iterate over list
      - nginx
      - curl
      - htop
    when: ansible_os_family == "Debian" # Conditional
    become: true                        # Override play-level become
    become_user: root
    register: install_result            # Capture result
    notify: Restart nginx               # Trigger handler
    tags:                               # Tags for filtering
      - packages
      - install
    ignore_errors: true                 # Continue on failure
    changed_when: install_result.rc == 0
    failed_when: install_result.rc > 1
```

### Common Task Patterns

#### Execute Commands

```yaml
- name: Run a simple command
  command: hostname

- name: Run shell command with pipes
  shell: cat /etc/passwd | grep deploy

- name: Run command with error handling
  command: /opt/app/healthcheck.sh
  register: health
  failed_when: health.rc not in [0, 1]
```

#### File Operations

```yaml
- name: Create directory
  file:
    path: /opt/myapp
    state: directory
    owner: deploy
    group: deploy
    mode: '0755'

- name: Copy file
  copy:
    src: files/config.yml
    dest: /opt/myapp/config.yml
    mode: '0644'

- name: Template file
  template:
    src: templates/app.conf.j2
    dest: /etc/myapp/app.conf
    owner: root
    mode: '0600'
```

#### Service Management

```yaml
- name: Ensure service is running
  service:
    name: nginx
    state: started
    enabled: true
```

## Variables

### Defining Variables

```yaml
- hosts: webservers
  vars:
    # Simple variables
    app_name: myapp
    app_version: "2.0.0"

    # Lists
    packages:
      - nginx
      - curl
      - jq

    # Dictionaries
    database:
      host: db.example.com
      port: 5432
      name: appdb

    # Multiline strings
    welcome_message: |
      Welcome to {{ app_name }}
      Version: {{ app_version }}
```

### Using Variables

```yaml
tasks:
  - name: Use simple variable
    debug:
      msg: "Deploying {{ app_name }} version {{ app_version }}"

  - name: Use list variable
    package:
      name: "{{ packages }}"
      state: present

  - name: Access dictionary
    debug:
      msg: "Database: {{ database.host }}:{{ database.port }}"

  - name: Use default value
    debug:
      msg: "Environment: {{ env | default('development') }}"
```

### Variable Files

```yaml
- hosts: all
  vars_files:
    - vars/common.yml
    - "vars/{{ env }}.yml"  # Dynamic file
```

## Conditionals

### When Clause

```yaml
tasks:
  # Simple condition
  - name: Install on Debian
    apt:
      name: nginx
    when: ansible_os_family == "Debian"

  # Multiple conditions (AND)
  - name: Production Debian only
    debug:
      msg: "Production Debian"
    when:
      - ansible_os_family == "Debian"
      - env == "production"

  # OR condition
  - name: RedHat family
    debug:
      msg: "RedHat-based"
    when: ansible_os_family == "RedHat" or ansible_os_family == "Rocky"

  # Variable defined check
  - name: Use custom port
    debug:
      msg: "Port: {{ custom_port }}"
    when: custom_port is defined

  # Boolean check
  - name: Debug mode
    debug:
      msg: "Debug enabled"
    when: debug_mode | bool
```

## Loops

### Basic Loops

```yaml
tasks:
  # Simple list loop
  - name: Install packages
    package:
      name: "{{ item }}"
      state: present
    loop:
      - nginx
      - curl
      - htop

  # Loop with variable
  - name: Install from variable
    package:
      name: "{{ item }}"
    loop: "{{ packages }}"

  # Loop over dictionary
  - name: Create users
    user:
      name: "{{ item.name }}"
      groups: "{{ item.groups }}"
    loop:
      - { name: 'alice', groups: 'admin' }
      - { name: 'bob', groups: 'users' }
```

### Loop Control

```yaml
tasks:
  - name: Loop with index
    debug:
      msg: "Item {{ index }}: {{ item }}"
    loop:
      - apple
      - banana
      - cherry
    loop_control:
      index_var: index

  - name: Custom loop variable
    debug:
      msg: "Package: {{ pkg }}"
    loop: "{{ packages }}"
    loop_control:
      loop_var: pkg
```

## Handlers

Handlers are tasks that run only when notified:

```yaml
- hosts: webservers
  tasks:
    - name: Update nginx config
      template:
        src: nginx.conf.j2
        dest: /etc/nginx/nginx.conf
      notify: Restart nginx          # Notify handler

    - name: Update site config
      template:
        src: site.conf.j2
        dest: /etc/nginx/sites-enabled/default
      notify:                          # Multiple handlers
        - Reload nginx
        - Clear cache

  handlers:
    - name: Restart nginx
      service:
        name: nginx
        state: restarted

    - name: Reload nginx
      service:
        name: nginx
        state: reloaded

    - name: Clear cache
      command: /opt/scripts/clear-cache.sh
```

**Handler Behavior:**
- Handlers run once at the end of the play
- Multiple notifications result in single handler execution
- Handlers run in definition order, not notification order
- Use `meta: flush_handlers` to run handlers immediately

## Blocks

Group tasks with shared attributes:

```yaml
tasks:
  - name: Web server setup
    block:
      - name: Install nginx
        package:
          name: nginx
          state: present

      - name: Start nginx
        service:
          name: nginx
          state: started
    when: "'webservers' in group_names"
    become: true
```

### Error Handling with Blocks

```yaml
tasks:
  - name: Handle errors gracefully
    block:
      - name: Try risky operation
        command: /opt/risky-script.sh

      - name: Continue if successful
        debug:
          msg: "Operation succeeded"

    rescue:
      - name: Handle failure
        debug:
          msg: "Operation failed, running recovery..."

      - name: Recovery action
        command: /opt/recovery.sh

    always:
      - name: Always cleanup
        file:
          path: /tmp/temp-file
          state: absent
```

## Tags

Filter which tasks to run:

```yaml
tasks:
  - name: Install packages
    package:
      name: nginx
    tags:
      - install
      - packages

  - name: Configure nginx
    template:
      src: nginx.conf.j2
      dest: /etc/nginx/nginx.conf
    tags:
      - configure

  - name: Start service
    service:
      name: nginx
      state: started
    tags:
      - always  # Always runs
```

Run with tags:

```bash
# Run only install tasks
rustible run playbook.yml --tags install

# Run multiple tags
rustible run playbook.yml --tags install,configure

# Skip specific tags
rustible run playbook.yml --skip-tags slow
```

## Include and Import

### Include Tasks

Dynamic inclusion (evaluated at runtime):

```yaml
tasks:
  - name: Include OS-specific tasks
    include_tasks: "tasks/{{ ansible_os_family }}.yml"

  - name: Include with variables
    include_tasks: tasks/deploy.yml
    vars:
      version: "2.0.0"
```

### Import Tasks

Static import (evaluated at parse time):

```yaml
tasks:
  - name: Import common tasks
    import_tasks: tasks/common.yml
```

## Registered Variables

Capture task output:

```yaml
tasks:
  - name: Get hostname
    command: hostname
    register: hostname_result

  - name: Show result
    debug:
      msg: "Hostname: {{ hostname_result.stdout }}"

  - name: Check file exists
    stat:
      path: /etc/config.yml
    register: config_stat

  - name: Create if missing
    copy:
      content: "default: true"
      dest: /etc/config.yml
    when: not config_stat.stat.exists
```

### Registered Variable Attributes

| Attribute | Description |
|-----------|-------------|
| `changed` | Whether task made changes |
| `failed` | Whether task failed |
| `skipped` | Whether task was skipped |
| `rc` | Return code (command modules) |
| `stdout` | Standard output |
| `stdout_lines` | Output as list of lines |
| `stderr` | Standard error |
| `msg` | Module message |

## Best Practices

1. **Always name your tasks**: Makes output readable and debugging easier
2. **Use meaningful variable names**: Self-documenting playbooks
3. **Group related tasks**: Use blocks for logical grouping
4. **Handle errors**: Use `rescue` blocks for recovery
5. **Use tags**: Enable selective execution
6. **Keep playbooks focused**: One playbook per logical function
7. **Validate with check mode**: Run `--check` before production

## Next Steps

- Learn about [Inventory Management](03-inventory.md)
- Understand [Variables and Precedence](04-variables.md)
- Explore [Available Modules](05-modules.md)
