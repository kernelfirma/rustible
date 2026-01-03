---
summary: Complete reference for all 21+ built-in modules covering package management, file operations, commands, services, users, groups, and git.
read_when: You need module parameters, examples, or return values for any built-in module.
---

# Rustible Module Reference

This document provides detailed documentation for all built-in modules in Rustible.

## Table of Contents

1. [Package Management](#package-management)
   - [apt](#apt-module)
   - [yum](#yum-module)
   - [dnf](#dnf-module)
   - [pip](#pip-module)
2. [File Management](#file-management)
   - [copy](#copy-module)
   - [file](#file-module)
   - [template](#template-module)
   - [lineinfile](#lineinfile-module)
   - [blockinfile](#blockinfile-module)
3. [Command Execution](#command-execution)
   - [command](#command-module)
   - [shell](#shell-module)
4. [System Administration](#system-administration)
   - [service](#service-module)
   - [user](#user-module)
   - [group](#group-module)
5. [Source Control](#source-control)
   - [git](#git-module)
6. [Utility Modules](#utility-modules)
   - [debug](#debug-module)
   - [assert](#assert-module)
   - [fail](#fail-module)
   - [set_fact](#set_fact-module)

---

## Package Management

### apt Module

Manages packages on Debian/Ubuntu systems using APT.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string/list | Yes | - | Package name(s) to manage |
| `state` | string | No | `present` | Desired state: `present`, `absent`, `latest`, `build-dep` |
| `update_cache` | bool | No | `false` | Update apt cache before operation |
| `cache_valid_time` | int | No | - | Seconds to consider cache valid |
| `force` | bool | No | `false` | Force package operations |
| `purge` | bool | No | `false` | Purge package configuration on removal |
| `autoremove` | bool | No | `false` | Remove unused dependencies |
| `install_recommends` | bool | No | `true` | Install recommended packages |

**Examples:**

```yaml
# Install a package
- name: Install nginx
  apt:
    name: nginx
    state: present

# Install multiple packages
- name: Install web stack
  apt:
    name:
      - nginx
      - php-fpm
      - mysql-server
    state: present
    update_cache: true
    cache_valid_time: 3600

# Ensure latest version
- name: Update nginx to latest
  apt:
    name: nginx
    state: latest

# Remove a package
- name: Remove apache2
  apt:
    name: apache2
    state: absent
    purge: true
    autoremove: true
```

**Return Values:**

```json
{
  "changed": true,
  "msg": "Package nginx installed",
  "packages": ["nginx=1.18.0-0ubuntu1"],
  "cache_updated": false
}
```

---

### yum Module

Manages packages on RHEL/CentOS 7 and earlier using YUM.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string/list | Yes | - | Package name(s) to manage |
| `state` | string | No | `present` | Desired state: `present`, `absent`, `latest`, `installed`, `removed` |
| `enablerepo` | string | No | - | Repository to enable |
| `disablerepo` | string | No | - | Repository to disable |
| `disable_gpg_check` | bool | No | `false` | Disable GPG signature checking |
| `update_cache` | bool | No | `false` | Force yum cache update |
| `security` | bool | No | `false` | Only install security updates |

**Examples:**

```yaml
# Install a package
- name: Install httpd
  yum:
    name: httpd
    state: present

# Install from specific repo
- name: Install package from EPEL
  yum:
    name: htop
    state: present
    enablerepo: epel

# Update all packages
- name: Update all packages
  yum:
    name: '*'
    state: latest
    security: true
```

---

### dnf Module

Manages packages on Fedora and RHEL 8+ using DNF.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string/list | Yes | - | Package name(s) to manage |
| `state` | string | No | `present` | Desired state: `present`, `absent`, `latest` |
| `enablerepo` | string | No | - | Repository to enable |
| `disablerepo` | string | No | - | Repository to disable |
| `disable_gpg_check` | bool | No | `false` | Disable GPG signature checking |
| `allowerasing` | bool | No | `false` | Allow erasing of installed packages |
| `install_weak_deps` | bool | No | `true` | Install weak dependencies |

**Examples:**

```yaml
# Install a package
- name: Install nginx
  dnf:
    name: nginx
    state: present

# Install package group
- name: Install development tools
  dnf:
    name: '@Development Tools'
    state: present

# Remove package allowing dependency removal
- name: Remove conflicting package
  dnf:
    name: mariadb
    state: absent
    allowerasing: true
```

---

### pip Module

Manages Python packages using pip.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string/list | Yes* | - | Package name(s) to manage |
| `requirements` | string | Yes* | - | Path to requirements.txt file |
| `state` | string | No | `present` | Desired state: `present`, `absent`, `latest`, `forcereinstall` |
| `version` | string | No | - | Package version to install |
| `virtualenv` | string | No | - | Path to virtualenv |
| `virtualenv_command` | string | No | `virtualenv` | Command to create virtualenv |
| `executable` | string | No | - | Path to pip executable |
| `extra_args` | string | No | - | Extra arguments for pip |

**Examples:**

```yaml
# Install a package
- name: Install Flask
  pip:
    name: flask
    state: present

# Install specific version
- name: Install Django 4.2
  pip:
    name: django
    version: "4.2"

# Install in virtualenv
- name: Install requirements in venv
  pip:
    requirements: /app/requirements.txt
    virtualenv: /app/venv
    virtualenv_command: python3 -m venv

# Install multiple packages
- name: Install data science stack
  pip:
    name:
      - numpy
      - pandas
      - scikit-learn
    state: latest
```

---

## File Management

### copy Module

Copies files to remote hosts.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `src` | string | Yes* | - | Local source file path |
| `content` | string | Yes* | - | File content (alternative to src) |
| `dest` | string | Yes | - | Remote destination path |
| `owner` | string | No | - | Owner of the file |
| `group` | string | No | - | Group of the file |
| `mode` | string | No | - | File permissions (e.g., "0644") |
| `backup` | bool | No | `false` | Create backup before overwriting |
| `force` | bool | No | `true` | Overwrite existing files |
| `directory_mode` | string | No | - | Permissions for created directories |
| `validate` | string | No | - | Command to validate file before copying |

**Examples:**

```yaml
# Copy a file
- name: Copy nginx config
  copy:
    src: files/nginx.conf
    dest: /etc/nginx/nginx.conf
    owner: root
    group: root
    mode: '0644'
    backup: true

# Copy content directly
- name: Create motd
  copy:
    content: |
      Welcome to {{ inventory_hostname }}
      Managed by Rustible
    dest: /etc/motd
    mode: '0644'

# Copy with validation
- name: Copy sudoers file
  copy:
    src: sudoers
    dest: /etc/sudoers
    validate: '/usr/sbin/visudo -cf %s'
```

---

### file Module

Manages file and directory properties.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | Yes | - | Path to the file or directory |
| `state` | string | No | `file` | State: `file`, `directory`, `link`, `hard`, `touch`, `absent` |
| `owner` | string | No | - | Owner of the file |
| `group` | string | No | - | Group of the file |
| `mode` | string | No | - | File permissions |
| `src` | string | No | - | Source for symlinks |
| `recurse` | bool | No | `false` | Recursively apply to directory contents |
| `force` | bool | No | `false` | Force link creation |

**Examples:**

```yaml
# Create directory
- name: Create app directory
  file:
    path: /opt/myapp
    state: directory
    owner: app
    group: app
    mode: '0755'

# Create symlink
- name: Create symlink
  file:
    src: /opt/myapp/current
    dest: /opt/myapp/latest
    state: link

# Set permissions recursively
- name: Set web directory permissions
  file:
    path: /var/www/html
    state: directory
    owner: www-data
    group: www-data
    mode: '0755'
    recurse: true

# Remove file
- name: Remove old config
  file:
    path: /etc/myapp/old.conf
    state: absent
```

---

### template Module

Deploys Jinja2 templates to remote hosts.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `src` | string | Yes | - | Local template file path |
| `dest` | string | Yes | - | Remote destination path |
| `owner` | string | No | - | Owner of the file |
| `group` | string | No | - | Group of the file |
| `mode` | string | No | - | File permissions |
| `backup` | bool | No | `false` | Create backup before overwriting |
| `force` | bool | No | `true` | Overwrite existing files |
| `validate` | string | No | - | Command to validate rendered file |

**Examples:**

```yaml
# Deploy nginx config
- name: Configure nginx
  template:
    src: templates/nginx.conf.j2
    dest: /etc/nginx/nginx.conf
    owner: root
    group: root
    mode: '0644'
  notify: Reload nginx

# Deploy with validation
- name: Configure sshd
  template:
    src: sshd_config.j2
    dest: /etc/ssh/sshd_config
    validate: '/usr/sbin/sshd -t -f %s'
    backup: true
```

**Template Example (nginx.conf.j2):**

```jinja2
user {{ nginx_user }};
worker_processes {{ ansible_processor_vcpus }};

events {
    worker_connections {{ nginx_worker_connections | default(1024) }};
}

http {
    {% for vhost in virtual_hosts %}
    server {
        listen {{ vhost.port }};
        server_name {{ vhost.name }};
        root {{ vhost.root }};
    }
    {% endfor %}
}
```

---

### lineinfile Module

Manages lines in text files.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | Yes | - | Path to the file |
| `line` | string | Yes* | - | Line to insert/replace |
| `regexp` | string | No | - | Regular expression to match |
| `state` | string | No | `present` | State: `present` or `absent` |
| `insertafter` | string | No | `EOF` | Insert after matching line |
| `insertbefore` | string | No | - | Insert before matching line |
| `create` | bool | No | `false` | Create file if not exists |
| `backup` | bool | No | `false` | Create backup before modification |
| `backrefs` | bool | No | `false` | Use regexp backreferences in line |

**Examples:**

```yaml
# Ensure line exists
- name: Add JAVA_HOME to environment
  lineinfile:
    path: /etc/environment
    line: 'JAVA_HOME=/usr/lib/jvm/java-11-openjdk'

# Replace matching line
- name: Update SSH PermitRootLogin
  lineinfile:
    path: /etc/ssh/sshd_config
    regexp: '^#?PermitRootLogin'
    line: 'PermitRootLogin no'
    backup: true

# Remove line
- name: Remove deprecated option
  lineinfile:
    path: /etc/myapp/config
    regexp: '^deprecated_option='
    state: absent

# Insert after specific line
- name: Add repo after base
  lineinfile:
    path: /etc/apt/sources.list
    insertafter: '^deb.*main'
    line: 'deb http://ppa.example.com/repo stable main'
```

---

### blockinfile Module

Manages blocks of text in files.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | Yes | - | Path to the file |
| `block` | string | Yes | - | Block of text to insert |
| `marker` | string | No | `# {mark} ANSIBLE MANAGED BLOCK` | Block markers |
| `insertafter` | string | No | `EOF` | Insert after matching line |
| `insertbefore` | string | No | - | Insert before matching line |
| `state` | string | No | `present` | State: `present` or `absent` |
| `create` | bool | No | `false` | Create file if not exists |
| `backup` | bool | No | `false` | Create backup |

**Examples:**

```yaml
# Insert block
- name: Add SSH banner
  blockinfile:
    path: /etc/ssh/sshd_config
    block: |
      Match User sftp
          ChrootDirectory /home/%u
          ForceCommand internal-sftp
          PasswordAuthentication yes

# Custom markers
- name: Add iptables rules
  blockinfile:
    path: /etc/iptables/rules.v4
    marker: "# {mark} MANAGED FIREWALL RULES"
    block: |
      -A INPUT -p tcp --dport 80 -j ACCEPT
      -A INPUT -p tcp --dport 443 -j ACCEPT
```

---

## Command Execution

### command Module

Executes commands on remote hosts without shell processing.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `cmd` | string | Yes | - | Command to execute |
| `chdir` | string | No | - | Directory to run command in |
| `creates` | string | No | - | Skip if file exists |
| `removes` | string | No | - | Only run if file exists |
| `stdin` | string | No | - | Input to pass to command |

**Examples:**

```yaml
# Simple command
- name: Get uptime
  command: uptime

# Command with chdir
- name: Run migration
  command:
    cmd: ./manage.py migrate
    chdir: /opt/myapp

# Idempotent command
- name: Initialize database
  command:
    cmd: /opt/myapp/init-db.sh
    creates: /var/lib/myapp/db.sqlite
```

---

### shell Module

Executes commands through the shell.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `cmd` | string | Yes | - | Command to execute |
| `chdir` | string | No | - | Directory to run command in |
| `creates` | string | No | - | Skip if file exists |
| `removes` | string | No | - | Only run if file exists |
| `executable` | string | No | `/bin/sh` | Shell to use |

**Examples:**

```yaml
# Use shell features
- name: Get disk usage
  shell: df -h | grep '/dev/sda1'

# Pipe commands
- name: Count running processes
  shell: ps aux | wc -l
  register: process_count

# Use bash features
- name: Process array
  shell: |
    for i in {1..5}; do
      echo "Item $i"
    done
  args:
    executable: /bin/bash
```

---

## System Administration

### service Module

Manages system services.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Service name |
| `state` | string | No | - | Service state: `started`, `stopped`, `restarted`, `reloaded` |
| `enabled` | bool | No | - | Enable service on boot |
| `pattern` | string | No | - | Pattern to match for status check |
| `sleep` | int | No | - | Seconds to wait between stop/start on restart |

**Examples:**

```yaml
# Start and enable service
- name: Start nginx
  service:
    name: nginx
    state: started
    enabled: true

# Restart service
- name: Restart sshd
  service:
    name: sshd
    state: restarted

# Reload configuration
- name: Reload haproxy
  service:
    name: haproxy
    state: reloaded
```

---

### user Module

Manages user accounts.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Username |
| `state` | string | No | `present` | State: `present` or `absent` |
| `uid` | int | No | - | User ID |
| `group` | string | No | - | Primary group |
| `groups` | list | No | - | Supplementary groups |
| `append` | bool | No | `false` | Append to existing groups |
| `shell` | string | No | - | Login shell |
| `home` | string | No | - | Home directory path |
| `create_home` | bool | No | `true` | Create home directory |
| `password` | string | No | - | Encrypted password |
| `comment` | string | No | - | User comment (GECOS) |
| `system` | bool | No | `false` | Create system account |
| `remove` | bool | No | `false` | Remove home directory on absent |

**Examples:**

```yaml
# Create user
- name: Create app user
  user:
    name: appuser
    uid: 1001
    group: app
    groups:
      - docker
      - sudo
    shell: /bin/bash
    home: /opt/app
    comment: "Application User"

# Create system user
- name: Create nginx user
  user:
    name: nginx
    system: true
    shell: /usr/sbin/nologin
    create_home: false

# Remove user
- name: Remove old user
  user:
    name: olduser
    state: absent
    remove: true
```

---

### group Module

Manages groups.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Group name |
| `state` | string | No | `present` | State: `present` or `absent` |
| `gid` | int | No | - | Group ID |
| `system` | bool | No | `false` | Create system group |

**Examples:**

```yaml
# Create group
- name: Create app group
  group:
    name: app
    gid: 1001

# Create system group
- name: Create docker group
  group:
    name: docker
    system: true
```

---

## Source Control

### git Module

Manages Git repositories.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `repo` | string | Yes | - | Repository URL |
| `dest` | string | Yes | - | Destination path |
| `version` | string | No | `HEAD` | Branch, tag, or commit to checkout |
| `clone` | bool | No | `true` | Clone if missing |
| `update` | bool | No | `true` | Update if exists |
| `force` | bool | No | `false` | Force checkout |
| `depth` | int | No | - | Create shallow clone |
| `single_branch` | bool | No | `false` | Only clone single branch |
| `recursive` | bool | No | `true` | Initialize submodules |
| `ssh_opts` | string | No | - | SSH command options |
| `accept_hostkey` | bool | No | `false` | Accept unknown host keys |

**Examples:**

```yaml
# Clone repository
- name: Clone application
  git:
    repo: https://github.com/example/myapp.git
    dest: /opt/myapp
    version: v1.2.3

# Shallow clone
- name: Clone with depth
  git:
    repo: git@github.com:example/myapp.git
    dest: /opt/myapp
    depth: 1
    single_branch: true

# Force update
- name: Force latest
  git:
    repo: https://github.com/example/myapp.git
    dest: /opt/myapp
    version: main
    force: true
```

---

## Utility Modules

### debug Module

Prints debug information.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `msg` | string | No | - | Message to print |
| `var` | string | No | - | Variable to print |
| `verbosity` | int | No | 0 | Minimum verbosity level |

**Examples:**

```yaml
# Print message
- name: Debug message
  debug:
    msg: "Current user is {{ ansible_user }}"

# Print variable
- name: Show facts
  debug:
    var: ansible_facts

# Only show with -v
- name: Verbose debug
  debug:
    msg: "Detailed info: {{ detailed_data }}"
    verbosity: 1
```

---

### assert Module

Asserts conditions and fails if not met.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `that` | list | Yes | - | Conditions that must be true |
| `fail_msg` | string | No | - | Message on failure |
| `success_msg` | string | No | - | Message on success |
| `quiet` | bool | No | `false` | Only show failures |

**Examples:**

```yaml
# Simple assertion
- name: Check disk space
  assert:
    that:
      - ansible_facts.mounts | selectattr('mount', 'eq', '/') | map(attribute='size_available') | first > 1000000000
    fail_msg: "Root filesystem has less than 1GB free"
    success_msg: "Disk space check passed"

# Multiple conditions
- name: Validate configuration
  assert:
    that:
      - http_port > 1024
      - http_port < 65535
      - worker_count >= 1
    fail_msg: "Invalid configuration values"
```

---

### fail Module

Fails with a custom message.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `msg` | string | No | - | Failure message |

**Examples:**

```yaml
# Conditional failure
- name: Check OS support
  fail:
    msg: "This playbook only supports Ubuntu"
  when: ansible_distribution != 'Ubuntu'
```

---

### set_fact Module

Sets host facts.

**Parameters:**

Any key-value pairs become facts.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `cacheable` | bool | No | `false` | Cache the fact |

**Examples:**

```yaml
# Set simple fact
- name: Set environment
  set_fact:
    deploy_env: production
    app_port: 8080

# Set complex fact
- name: Build connection string
  set_fact:
    db_connection: "postgres://{{ db_user }}:{{ db_pass }}@{{ db_host }}:{{ db_port }}/{{ db_name }}"
    cacheable: true
```

---

## Module Development

### Creating Custom Modules

```rust
use rustible::traits::{Module, ModuleResult};
use rustible::error::{Error, Result};
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct MyModuleArgs {
    target: String,
    action: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug)]
pub struct MyModule;

#[async_trait]
impl Module for MyModule {
    fn name(&self) -> &'static str {
        "my_module"
    }

    async fn execute(
        &self,
        args: &serde_json::Value,
        context: &ExecutionContext,
    ) -> Result<ModuleResult> {
        // Parse arguments
        let args: MyModuleArgs = serde_json::from_value(args.clone())
            .map_err(|e| Error::module_args("my_module", e.to_string()))?;

        // Check current state
        let current_state = self.check_state(&args, context).await?;

        // If already in desired state, return unchanged
        if current_state == args.action && !args.force {
            return Ok(ModuleResult {
                changed: false,
                msg: Some("Already in desired state".to_string()),
                ..Default::default()
            });
        }

        // Apply changes (if not in check mode)
        if !context.check_mode {
            self.apply_changes(&args, context).await?;
        }

        Ok(ModuleResult {
            changed: true,
            msg: Some(format!("Applied action: {}", args.action)),
            diff: context.diff_mode.then(|| Diff {
                before: current_state,
                after: args.action.clone(),
            }),
            ..Default::default()
        })
    }
}
```

### Registering Custom Modules

```rust
use rustible::modules::ModuleRegistry;

let mut registry = ModuleRegistry::with_builtins();
registry.register(Box::new(MyModule));
```
