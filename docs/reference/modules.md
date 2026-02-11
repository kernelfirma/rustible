---
summary: Complete reference for all 60+ built-in modules covering package management, file operations, commands, services, users, groups, git, Docker, Kubernetes, cloud, database, HPC, and more.
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
   - [package](#package-module)
2. [File Management](#file-management)
   - [copy](#copy-module)
   - [file](#file-module)
   - [template](#template-module)
   - [lineinfile](#lineinfile-module)
   - [blockinfile](#blockinfile-module)
   - [archive](#archive-module)
   - [unarchive](#unarchive-module)
   - [stat](#stat-module)
   - [synchronize](#synchronize-module)
3. [Command Execution](#command-execution)
   - [command](#command-module)
   - [shell](#shell-module)
   - [raw](#raw-module)
   - [script](#script-module)
4. [System Administration](#system-administration)
   - [service](#service-module)
   - [user](#user-module)
   - [group](#group-module)
   - [systemd_unit](#systemd_unit-module)
   - [cron](#cron-module)
   - [hostname](#hostname-module)
   - [sysctl](#sysctl-module)
   - [mount](#mount-module)
   - [timezone](#timezone-module)
   - [pause](#pause-module)
   - [wait_for](#wait_for-module)
5. [Source Control](#source-control)
   - [git](#git-module)
6. [Fact Gathering](#fact-gathering)
   - [facts](#facts-module)
7. [Network & Security](#network--security)
   - [uri](#uri-module)
   - [authorized_key](#authorized_key-module)
   - [known_hosts](#known_hosts-module)
   - [ufw](#ufw-module)
   - [firewalld](#firewalld-module)
   - [selinux](#selinux-module)
8. [Utility Modules](#utility-modules)
   - [debug](#debug-module)
   - [assert](#assert-module)
   - [fail](#fail-module)
   - [set_fact](#set_fact-module)
   - [include_vars](#include_vars-module)
   - [meta](#meta-module)
9. [Docker Modules](#docker-modules)
   - [docker_container](#docker_container-module)
   - [docker_image](#docker_image-module)
   - [docker_network](#docker_network-module)
   - [docker_volume](#docker_volume-module)
   - [docker_compose](#docker_compose-module)
10. [Kubernetes Modules](#kubernetes-modules)
    - [k8s_namespace](#k8s_namespace-module)
    - [k8s_deployment](#k8s_deployment-module)
    - [k8s_service](#k8s_service-module)
    - [k8s_configmap](#k8s_configmap-module)
    - [k8s_secret](#k8s_secret-module)
11. [Cloud - AWS](#cloud---aws)
    - [aws_ec2](#aws_ec2-module)
    - [aws_s3](#aws_s3-module)
    - [aws_vpc](#aws_vpc-module)
    - [aws_security_group](#aws_security_group-module)
    - [aws_iam_role](#aws_iam_role-module)
    - [aws_iam_policy](#aws_iam_policy-module)
12. [Cloud - Azure](#cloud---azure)
    - [azure_vm](#azure_vm-module)
    - [azure_resource_group](#azure_resource_group-module)
    - [azure_network_interface](#azure_network_interface-module)
13. [Cloud - GCP](#cloud---gcp)
    - [gcp_compute_instance](#gcp_compute_instance-module)
    - [gcp_compute_firewall](#gcp_compute_firewall-module)
    - [gcp_compute_network](#gcp_compute_network-module)
    - [gcp_service_account](#gcp_service_account-module)
14. [Cloud - Proxmox](#cloud---proxmox)
    - [proxmox_lxc](#proxmox_lxc-module)
    - [proxmox_vm](#proxmox_vm-module)
15. [Network Devices](#network-devices)
    - [ios_config](#ios_config-module)
    - [eos_config](#eos_config-module)
    - [junos_config](#junos_config-module)
    - [nxos_config](#nxos_config-module)
16. [Database Modules](#database-modules)
    - [postgresql_db](#postgresql_db-module)
    - [postgresql_user](#postgresql_user-module)
    - [postgresql_query](#postgresql_query-module)
    - [postgresql_privs](#postgresql_privs-module)
    - [mysql_db](#mysql_db-module)
    - [mysql_user](#mysql_user-module)
    - [mysql_query](#mysql_query-module)
    - [mysql_privs](#mysql_privs-module)
17. [Windows Modules](#windows-modules)
    - [win_copy](#win_copy-module)
    - [win_feature](#win_feature-module)
    - [win_service](#win_service-module)
    - [win_package](#win_package-module)
    - [win_user](#win_user-module)
18. [HPC Modules](#hpc-modules)
    - [hpc_baseline](#hpc_baseline-module)
    - [lmod](#lmod-module)
    - [mpi](#mpi-module)
    - [slurm_config](#slurm_config-module)
    - [slurm_ops](#slurm_ops-module)
    - [nvidia_gpu](#nvidia_gpu-module)
    - [rdma_stack](#rdma_stack-module)
    - [lustre_client](#lustre_client-module)
    - [beegfs_client](#beegfs_client-module)

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

### package Module

Generic OS package manager that auto-detects apt, yum, or dnf. See also [package module docs](modules/package.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string/list | Yes | - | Package name(s) |
| `state` | string | No | `present` | `present`, `absent`, `latest` |
| `use` | string | No | auto | Force backend: `apt`, `yum`, `dnf` |

**Example:**

```yaml
- name: Install nginx on any distro
  package:
    name: nginx
    state: present
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

### archive Module

Creates compressed archives from files or directories.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string/list | Yes | - | File(s) or directory to archive |
| `dest` | string | Yes | - | Destination archive path |
| `format` | string | No | `gz` | Archive format: `gz`, `bz2`, `xz`, `zip`, `tar` |
| `remove` | bool | No | `false` | Remove source after archiving |

**Example:**

```yaml
- name: Archive logs
  archive:
    path: /var/log/myapp/
    dest: /backup/myapp-logs.tar.gz
    format: gz
```

---

### unarchive Module

Extracts archives to a destination directory.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `src` | string | Yes | - | Archive source path or URL |
| `dest` | string | Yes | - | Destination directory |
| `remote_src` | bool | No | `false` | Source is already on remote |
| `creates` | string | No | - | Skip if path exists |

**Example:**

```yaml
- name: Extract application
  unarchive:
    src: files/app-v1.2.tar.gz
    dest: /opt/app
    creates: /opt/app/bin/server
```

---

### stat Module

Retrieves file or filesystem status information.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | Yes | - | Path to stat |
| `follow` | bool | No | `false` | Follow symlinks |
| `checksum_algorithm` | string | No | `sha1` | Hash algorithm |

**Example:**

```yaml
- name: Check if config exists
  stat:
    path: /etc/myapp/config.yml
  register: config_stat

- name: Create default config
  copy:
    content: "defaults: true"
    dest: /etc/myapp/config.yml
  when: not config_stat.stat.exists
```

---

### synchronize Module

Wraps rsync to efficiently sync files between hosts.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `src` | string | Yes | - | Source path |
| `dest` | string | Yes | - | Destination path |
| `delete` | bool | No | `false` | Delete extra files at dest |
| `recursive` | bool | No | `true` | Recurse into directories |
| `compress` | bool | No | `true` | Compress during transfer |

**Example:**

```yaml
- name: Sync web content
  synchronize:
    src: /opt/build/dist/
    dest: /var/www/html/
    delete: true
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

### raw Module

Executes a low-level command over SSH without module wrapping. Useful for bootstrapping hosts without Python.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `free_form` | string | Yes | - | Command to execute (inline) |

**Example:**

```yaml
- name: Bootstrap Python on bare host
  raw: apt-get install -y python3
```

---

### script Module

Transfers and executes a local script on the remote host.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `free_form` | string | Yes | - | Path to local script |
| `creates` | string | No | - | Skip if path exists |
| `removes` | string | No | - | Only run if path exists |
| `chdir` | string | No | - | Directory to run in |

**Example:**

```yaml
- name: Run setup script
  script: scripts/setup.sh
  args:
    creates: /opt/app/.initialized
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

### systemd_unit Module

Manages systemd unit files and services with fine-grained control.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Unit name (e.g., `myapp.service`) |
| `state` | string | No | - | `started`, `stopped`, `restarted`, `reloaded` |
| `enabled` | bool | No | - | Enable on boot |
| `daemon_reload` | bool | No | `false` | Run daemon-reload before action |
| `scope` | string | No | `system` | `system` or `user` |

**Example:**

```yaml
- name: Enable and start custom service
  systemd_unit:
    name: myapp.service
    state: started
    enabled: true
    daemon_reload: true
```

---

### cron Module

Manages cron jobs. See also [cron module docs](modules/cron.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Description of the cron job |
| `job` | string | Yes* | - | Command to execute |
| `state` | string | No | `present` | `present` or `absent` |
| `minute` | string | No | `*` | Minute (0-59) |
| `hour` | string | No | `*` | Hour (0-23) |
| `day` | string | No | `*` | Day of month (1-31) |
| `month` | string | No | `*` | Month (1-12) |
| `weekday` | string | No | `*` | Day of week (0-7) |
| `user` | string | No | `root` | Cron user |

**Example:**

```yaml
- name: Schedule daily backup
  cron:
    name: "Daily backup"
    job: "/opt/scripts/backup.sh"
    hour: "2"
    minute: "30"
```

---

### hostname Module

Sets the system hostname.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Desired hostname |

**Example:**

```yaml
- name: Set hostname
  hostname:
    name: web01.example.com
```

---

### sysctl Module

Manages sysctl kernel parameters.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Sysctl parameter name |
| `value` | string | Yes | - | Parameter value |
| `state` | string | No | `present` | `present` or `absent` |
| `reload` | bool | No | `true` | Reload sysctl after change |
| `sysctl_file` | string | No | `/etc/sysctl.conf` | Config file path |

**Example:**

```yaml
- name: Increase max open files
  sysctl:
    name: fs.file-max
    value: "65536"
    reload: true
```

---

### mount Module

Manages filesystem mount points.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | Yes | - | Mount point |
| `src` | string | Yes* | - | Device or remote filesystem |
| `fstype` | string | Yes* | - | Filesystem type |
| `state` | string | No | `mounted` | `mounted`, `unmounted`, `present`, `absent` |
| `opts` | string | No | `defaults` | Mount options |

**Example:**

```yaml
- name: Mount NFS share
  mount:
    path: /mnt/data
    src: nfs-server:/export/data
    fstype: nfs
    opts: "rw,sync"
    state: mounted
```

---

### timezone Module

Sets the system timezone.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Timezone (e.g., `America/New_York`) |

**Example:**

```yaml
- name: Set timezone
  timezone:
    name: UTC
```

---

### pause Module

Pauses playbook execution for a given duration or until user input.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `seconds` | int | No | - | Seconds to pause |
| `minutes` | int | No | - | Minutes to pause |
| `prompt` | string | No | - | Message to display |

**Example:**

```yaml
- name: Wait for service to stabilize
  pause:
    seconds: 10
```

---

### wait_for Module

Waits for a condition before continuing.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `port` | int | No | - | TCP port to wait for |
| `host` | string | No | `127.0.0.1` | Host to check |
| `path` | string | No | - | File path to wait for |
| `state` | string | No | `started` | `started`, `stopped`, `present`, `absent` |
| `timeout` | int | No | `300` | Timeout in seconds |
| `delay` | int | No | `0` | Seconds to wait before first check |

**Example:**

```yaml
- name: Wait for app to start
  wait_for:
    port: 8080
    delay: 5
    timeout: 60
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

### include_vars Module

Loads variables from YAML/JSON files at runtime.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file` | string | Yes* | - | Path to variables file |
| `dir` | string | Yes* | - | Directory of variable files |
| `name` | string | No | - | Assign loaded vars to this variable |

**Example:**

```yaml
- name: Load environment-specific vars
  include_vars:
    file: "vars/{{ environment }}.yml"
```

---

### meta Module

Executes internal Rustible operations (flush handlers, end play, etc.).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `free_form` | string | Yes | - | Action: `flush_handlers`, `end_play`, `clear_facts`, `refresh_inventory` |

**Example:**

```yaml
- name: Flush handlers now
  meta: flush_handlers
```

---

## Fact Gathering

### facts Module

Gathers system facts (OS, network, hardware) from the remote host. Equivalent to Ansible's `setup` module.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `gather_subset` | list | No | `all` | Subsets to gather: `hardware`, `network`, `os`, `virtual` |
| `filter` | string | No | - | Filter returned facts by glob pattern |

**Example:**

```yaml
- name: Gather only network facts
  facts:
    gather_subset:
      - network

- name: Show IP address
  debug:
    var: ansible_default_ipv4.address
```

---

## Network & Security

### uri Module

Interacts with HTTP/HTTPS endpoints.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `url` | string | Yes | - | Target URL |
| `method` | string | No | `GET` | HTTP method |
| `body` | string | No | - | Request body |
| `body_format` | string | No | `raw` | `json`, `form-urlencoded`, `raw` |
| `headers` | dict | No | - | HTTP headers |
| `status_code` | int/list | No | `200` | Expected status code(s) |
| `return_content` | bool | No | `false` | Include body in result |

**Example:**

```yaml
- name: Check API health
  uri:
    url: http://localhost:8080/health
    method: GET
    status_code: 200
  register: health
```

---

### authorized_key Module

Manages SSH authorized keys for user accounts.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `user` | string | Yes | - | Username |
| `key` | string | Yes | - | SSH public key |
| `state` | string | No | `present` | `present` or `absent` |
| `exclusive` | bool | No | `false` | Remove all other keys |

**Example:**

```yaml
- name: Add deploy key
  authorized_key:
    user: deploy
    key: "{{ lookup('file', 'deploy_key.pub') }}"
    state: present
```

---

### known_hosts Module

Manages SSH known hosts entries.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Hostname or IP |
| `key` | string | Yes* | - | Host key |
| `state` | string | No | `present` | `present` or `absent` |
| `path` | string | No | `~/.ssh/known_hosts` | Known hosts file |

**Example:**

```yaml
- name: Add GitHub to known hosts
  known_hosts:
    name: github.com
    key: "github.com ssh-ed25519 AAAA..."
```

---

### ufw Module

Manages the Uncomplicated Firewall on Debian/Ubuntu.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `rule` | string | No | - | `allow`, `deny`, `reject`, `limit` |
| `port` | string | No | - | Port or port range |
| `proto` | string | No | `any` | Protocol: `tcp`, `udp`, `any` |
| `state` | string | No | - | `enabled`, `disabled`, `reset` |
| `from_ip` | string | No | `any` | Source address |

**Example:**

```yaml
- name: Allow SSH and HTTP
  ufw:
    rule: allow
    port: "{{ item }}"
    proto: tcp
  loop:
    - "22"
    - "80"
    - "443"
```

---

### firewalld Module

Manages firewalld rules on RHEL/CentOS/Fedora.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `service` | string | No | - | Service name to allow |
| `port` | string | No | - | Port/protocol (e.g., `8080/tcp`) |
| `zone` | string | No | `public` | Firewall zone |
| `permanent` | bool | No | `false` | Persist across reboots |
| `state` | string | No | `enabled` | `enabled` or `disabled` |
| `immediate` | bool | No | `false` | Apply immediately |

**Example:**

```yaml
- name: Allow HTTPS in firewalld
  firewalld:
    service: https
    zone: public
    permanent: true
    immediate: true
    state: enabled
```

---

### selinux Module

Manages SELinux mode and policy.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `state` | string | Yes | - | `enforcing`, `permissive`, `disabled` |
| `policy` | string | No | - | SELinux policy name |

**Example:**

```yaml
- name: Set SELinux to permissive
  selinux:
    state: permissive
    policy: targeted
```

---

## Docker Modules

> Requires the `docker` feature flag.

### docker_container Module

Manages Docker containers. See also [docker_container module docs](modules/docker_container.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Container name |
| `image` | string | Yes* | - | Docker image |
| `state` | string | No | `started` | `started`, `stopped`, `absent`, `present` |
| `ports` | list | No | - | Port mappings (`"8080:80"`) |
| `volumes` | list | No | - | Volume mounts |
| `env` | dict | No | - | Environment variables |
| `restart_policy` | string | No | `no` | `no`, `always`, `on-failure`, `unless-stopped` |

**Example:**

```yaml
- name: Run nginx container
  docker_container:
    name: web
    image: nginx:latest
    state: started
    ports:
      - "80:80"
    volumes:
      - "/data/html:/usr/share/nginx/html:ro"
    restart_policy: unless-stopped
```

---

### docker_image Module

Manages Docker images (pull, build, remove).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Image name with optional tag |
| `state` | string | No | `present` | `present`, `absent` |
| `source` | string | No | `pull` | `pull`, `build`, `load` |
| `build` | dict | No | - | Build options (`path`, `dockerfile`) |
| `force_source` | bool | No | `false` | Force re-pull or rebuild |

**Example:**

```yaml
- name: Pull Redis image
  docker_image:
    name: redis:7-alpine
    source: pull
```

---

### docker_network Module

Manages Docker networks.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Network name |
| `state` | string | No | `present` | `present` or `absent` |
| `driver` | string | No | `bridge` | Network driver |
| `ipam_config` | list | No | - | IPAM configuration |

**Example:**

```yaml
- name: Create app network
  docker_network:
    name: app-net
    driver: bridge
```

---

### docker_volume Module

Manages Docker volumes.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Volume name |
| `state` | string | No | `present` | `present` or `absent` |
| `driver` | string | No | `local` | Volume driver |
| `driver_options` | dict | No | - | Driver-specific options |

**Example:**

```yaml
- name: Create data volume
  docker_volume:
    name: app-data
    state: present
```

---

### docker_compose Module

Manages multi-container applications with Docker Compose.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `project_src` | string | Yes | - | Path to compose file directory |
| `state` | string | No | `present` | `present`, `absent` |
| `pull` | bool | No | `false` | Pull images before starting |
| `build` | bool | No | `false` | Build images before starting |
| `services` | list | No | - | Limit to specific services |

**Example:**

```yaml
- name: Deploy application stack
  docker_compose:
    project_src: /opt/myapp
    state: present
    pull: true
```

---

## Kubernetes Modules

> Requires the `kubernetes` feature flag.

### k8s_namespace Module

Manages Kubernetes namespaces.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Namespace name |
| `state` | string | No | `present` | `present` or `absent` |
| `labels` | dict | No | - | Namespace labels |

**Example:**

```yaml
- name: Create namespace
  k8s_namespace:
    name: production
    labels:
      env: production
```

---

### k8s_deployment Module

Manages Kubernetes deployments. See also [k8s_deployment module docs](modules/k8s_deployment.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Deployment name |
| `namespace` | string | No | `default` | Target namespace |
| `state` | string | No | `present` | `present` or `absent` |
| `replicas` | int | No | `1` | Number of replicas |
| `image` | string | Yes* | - | Container image |
| `labels` | dict | No | - | Pod labels |

**Example:**

```yaml
- name: Deploy web app
  k8s_deployment:
    name: web
    namespace: production
    image: myapp:latest
    replicas: 3
```

---

### k8s_service Module

Manages Kubernetes services.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Service name |
| `namespace` | string | No | `default` | Target namespace |
| `state` | string | No | `present` | `present` or `absent` |
| `type` | string | No | `ClusterIP` | `ClusterIP`, `NodePort`, `LoadBalancer` |
| `ports` | list | Yes | - | Port definitions |
| `selector` | dict | Yes | - | Pod selector labels |

**Example:**

```yaml
- name: Expose web deployment
  k8s_service:
    name: web-svc
    namespace: production
    type: LoadBalancer
    ports:
      - port: 80
        target_port: 8080
    selector:
      app: web
```

---

### k8s_configmap Module

Manages Kubernetes ConfigMaps.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | ConfigMap name |
| `namespace` | string | No | `default` | Target namespace |
| `state` | string | No | `present` | `present` or `absent` |
| `data` | dict | No | - | Key-value data |

**Example:**

```yaml
- name: Create app config
  k8s_configmap:
    name: app-config
    namespace: production
    data:
      DATABASE_URL: "postgres://db:5432/app"
      LOG_LEVEL: "info"
```

---

### k8s_secret Module

Manages Kubernetes Secrets.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Secret name |
| `namespace` | string | No | `default` | Target namespace |
| `state` | string | No | `present` | `present` or `absent` |
| `type` | string | No | `Opaque` | Secret type |
| `data` | dict | No | - | Base64-encoded data |
| `string_data` | dict | No | - | Plain-text data (auto-encoded) |

**Example:**

```yaml
- name: Create DB credentials
  k8s_secret:
    name: db-creds
    namespace: production
    string_data:
      username: admin
      password: "{{ vault_db_password }}"
```

---

## Cloud - AWS

> Requires the `aws` feature flag.

### aws_ec2 Module

Manages AWS EC2 instances. See also [aws_ec2 module docs](modules/aws_ec2.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Instance name tag |
| `state` | string | No | `present` | `present`, `absent`, `running`, `stopped` |
| `instance_type` | string | No | `t3.micro` | EC2 instance type |
| `image_id` | string | Yes* | - | AMI ID |
| `key_name` | string | No | - | SSH key pair name |
| `security_groups` | list | No | - | Security group IDs |
| `subnet_id` | string | No | - | VPC subnet ID |
| `region` | string | No | env | AWS region |

**Example:**

```yaml
- name: Launch web server
  aws_ec2:
    name: web-01
    state: running
    instance_type: t3.medium
    image_id: ami-0abcdef1234567890
    key_name: deploy-key
    region: us-east-1
```

---

### aws_s3 Module

Manages AWS S3 buckets and objects.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `bucket` | string | Yes | - | S3 bucket name |
| `object` | string | No | - | Object key |
| `src` | string | No | - | Local file to upload |
| `dest` | string | No | - | Local download path |
| `mode` | string | No | `get` | `get`, `put`, `delete`, `create`, `list` |
| `region` | string | No | env | AWS region |

**Example:**

```yaml
- name: Upload config to S3
  aws_s3:
    bucket: my-configs
    object: app/config.yml
    src: /opt/app/config.yml
    mode: put
```

---

### aws_vpc Module

Manages AWS VPCs.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | VPC name tag |
| `cidr_block` | string | Yes | - | CIDR block (e.g., `10.0.0.0/16`) |
| `state` | string | No | `present` | `present` or `absent` |
| `region` | string | No | env | AWS region |

**Example:**

```yaml
- name: Create application VPC
  aws_vpc:
    name: app-vpc
    cidr_block: 10.0.0.0/16
    state: present
    region: us-east-1
```

---

### aws_security_group Module

Manages AWS security groups.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Security group name |
| `description` | string | Yes | - | Group description |
| `vpc_id` | string | Yes | - | VPC ID |
| `rules` | list | No | - | Inbound rules |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Create web security group
  aws_security_group:
    name: web-sg
    description: Allow HTTP/HTTPS
    vpc_id: vpc-12345
    rules:
      - proto: tcp
        from_port: 80
        to_port: 80
        cidr_ip: 0.0.0.0/0
      - proto: tcp
        from_port: 443
        to_port: 443
        cidr_ip: 0.0.0.0/0
```

---

### aws_iam_role Module

Manages AWS IAM roles.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Role name |
| `state` | string | No | `present` | `present` or `absent` |
| `assume_role_policy_document` | string | Yes* | - | Trust policy JSON |
| `managed_policies` | list | No | - | Attached policy ARNs |

**Example:**

```yaml
- name: Create EC2 role
  aws_iam_role:
    name: ec2-app-role
    assume_role_policy_document: "{{ lookup('file', 'trust-policy.json') }}"
    managed_policies:
      - arn:aws:iam::aws:policy/AmazonS3ReadOnlyAccess
```

---

### aws_iam_policy Module

Manages AWS IAM policies.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Policy name |
| `state` | string | No | `present` | `present` or `absent` |
| `policy_document` | string | Yes* | - | Policy JSON document |

**Example:**

```yaml
- name: Create S3 access policy
  aws_iam_policy:
    name: app-s3-access
    policy_document: "{{ lookup('file', 'policy.json') }}"
```

---

## Cloud - Azure

> Requires the `azure` feature flag (experimental).

### azure_vm Module

Manages Azure virtual machines.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | VM name |
| `resource_group` | string | Yes | - | Resource group name |
| `state` | string | No | `present` | `present`, `absent` |
| `vm_size` | string | No | `Standard_B1s` | VM size |
| `image` | dict | No | - | Image reference |

**Example:**

```yaml
- name: Create Azure VM
  azure_vm:
    name: web-vm
    resource_group: myapp-rg
    vm_size: Standard_B2s
    image:
      publisher: Canonical
      offer: UbuntuServer
      sku: "20.04-LTS"
```

---

### azure_resource_group Module

Manages Azure resource groups.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Resource group name |
| `location` | string | Yes | - | Azure region |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Create resource group
  azure_resource_group:
    name: myapp-rg
    location: eastus
```

---

### azure_network_interface Module

Manages Azure network interfaces.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | NIC name |
| `resource_group` | string | Yes | - | Resource group name |
| `virtual_network` | string | Yes | - | Virtual network name |
| `subnet` | string | Yes | - | Subnet name |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Create network interface
  azure_network_interface:
    name: web-nic
    resource_group: myapp-rg
    virtual_network: myapp-vnet
    subnet: default
```

---

## Cloud - GCP

> Requires the `gcp` feature flag (experimental).

### gcp_compute_instance Module

Manages Google Compute Engine instances.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Instance name |
| `project` | string | Yes | - | GCP project ID |
| `zone` | string | Yes | - | Compute zone |
| `machine_type` | string | No | `e2-micro` | Machine type |
| `disks` | list | No | - | Disk configuration |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Create GCP instance
  gcp_compute_instance:
    name: web-01
    project: my-project
    zone: us-central1-a
    machine_type: e2-standard-2
```

---

### gcp_compute_firewall Module

Manages GCP firewall rules.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Firewall rule name |
| `project` | string | Yes | - | GCP project ID |
| `network` | string | No | `default` | VPC network |
| `allowed` | list | Yes | - | Allowed protocols/ports |
| `source_ranges` | list | No | - | Source CIDR ranges |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Allow HTTP traffic
  gcp_compute_firewall:
    name: allow-http
    project: my-project
    allowed:
      - protocol: tcp
        ports: ["80", "443"]
    source_ranges: ["0.0.0.0/0"]
```

---

### gcp_compute_network Module

Manages GCP VPC networks.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Network name |
| `project` | string | Yes | - | GCP project ID |
| `auto_create_subnetworks` | bool | No | `true` | Auto-create subnets |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Create VPC network
  gcp_compute_network:
    name: app-network
    project: my-project
    auto_create_subnetworks: false
```

---

### gcp_service_account Module

Manages GCP service accounts.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Service account ID |
| `project` | string | Yes | - | GCP project ID |
| `display_name` | string | No | - | Human-readable name |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Create service account
  gcp_service_account:
    name: app-sa
    project: my-project
    display_name: Application Service Account
```

---

## Cloud - Proxmox

### proxmox_lxc Module

Manages Proxmox LXC containers.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `vmid` | int | Yes | - | Container VMID |
| `hostname` | string | Yes | - | Container hostname |
| `node` | string | Yes | - | Proxmox node name |
| `ostemplate` | string | Yes* | - | Container template |
| `state` | string | No | `present` | `present`, `absent`, `started`, `stopped` |
| `cores` | int | No | `1` | CPU cores |
| `memory` | int | No | `512` | Memory in MB |

**Example:**

```yaml
- name: Create LXC container
  proxmox_lxc:
    vmid: 200
    hostname: web-ct
    node: pve1
    ostemplate: local:vztmpl/debian-12-standard_12.0-1_amd64.tar.zst
    cores: 2
    memory: 1024
    state: present
```

---

### proxmox_vm Module

Manages Proxmox QEMU/KVM virtual machines.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `vmid` | int | Yes | - | VM ID |
| `name` | string | Yes | - | VM name |
| `node` | string | Yes | - | Proxmox node name |
| `state` | string | No | `present` | `present`, `absent`, `started`, `stopped` |
| `cores` | int | No | `1` | CPU cores |
| `memory` | int | No | `1024` | Memory in MB |
| `clone` | string | No | - | Template to clone from |

**Example:**

```yaml
- name: Clone VM from template
  proxmox_vm:
    vmid: 300
    name: app-vm
    node: pve1
    clone: ubuntu-template
    cores: 4
    memory: 4096
    state: present
```

---

## Network Devices

> Requires the `network_devices` feature flag.

### ios_config Module

Manages Cisco IOS device configuration.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `lines` | list | Yes* | - | Configuration lines to apply |
| `parents` | list | No | - | Parent context lines |
| `src` | string | Yes* | - | Configuration file to load |
| `save_when` | string | No | `never` | `never`, `always`, `modified`, `changed` |

**Example:**

```yaml
- name: Configure interface
  ios_config:
    lines:
      - description Uplink
      - ip address 10.0.0.1 255.255.255.0
    parents: interface GigabitEthernet0/1
    save_when: modified
```

---

### eos_config Module

Manages Arista EOS device configuration.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `lines` | list | Yes* | - | Configuration lines |
| `parents` | list | No | - | Parent context |
| `src` | string | Yes* | - | Configuration file |
| `save_when` | string | No | `never` | `never`, `always`, `modified`, `changed` |

**Example:**

```yaml
- name: Configure Arista VLAN
  eos_config:
    lines:
      - name Production
    parents: vlan 100
```

---

### junos_config Module

Manages Juniper Junos device configuration.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `lines` | list | Yes* | - | Configuration lines (set commands) |
| `src` | string | Yes* | - | Configuration file |
| `confirm` | int | No | - | Confirm timeout in minutes |
| `comment` | string | No | - | Commit comment |

**Example:**

```yaml
- name: Configure Junos firewall
  junos_config:
    lines:
      - set firewall filter PROTECT term ALLOW-SSH from protocol tcp
      - set firewall filter PROTECT term ALLOW-SSH from port 22
      - set firewall filter PROTECT term ALLOW-SSH then accept
    comment: "Allow SSH access"
```

---

### nxos_config Module

Manages Cisco NX-OS device configuration.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `lines` | list | Yes* | - | Configuration lines |
| `parents` | list | No | - | Parent context |
| `src` | string | Yes* | - | Configuration file |
| `save_when` | string | No | `never` | `never`, `always`, `modified`, `changed` |

**Example:**

```yaml
- name: Configure NX-OS VLAN
  nxos_config:
    lines:
      - name ServerVLAN
    parents: vlan 200
    save_when: modified
```

---

## Database Modules

> Requires the `database` feature flag (experimental).

### postgresql_db Module

Manages PostgreSQL databases.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Database name |
| `state` | string | No | `present` | `present`, `absent` |
| `owner` | string | No | - | Database owner |
| `encoding` | string | No | `UTF8` | Character encoding |
| `login_host` | string | No | `localhost` | Server hostname |
| `login_user` | string | No | `postgres` | Login user |

**Example:**

```yaml
- name: Create application database
  postgresql_db:
    name: myapp
    owner: appuser
    encoding: UTF8
```

---

### postgresql_user Module

Manages PostgreSQL users/roles.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Username |
| `password` | string | No | - | User password |
| `state` | string | No | `present` | `present` or `absent` |
| `role_attr_flags` | string | No | - | Role attributes (e.g., `CREATEDB,LOGIN`) |
| `db` | string | No | - | Database to connect to |

**Example:**

```yaml
- name: Create app user
  postgresql_user:
    name: appuser
    password: "{{ vault_db_password }}"
    role_attr_flags: CREATEDB,LOGIN
```

---

### postgresql_query Module

Executes arbitrary PostgreSQL queries.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | Yes | - | SQL query to execute |
| `db` | string | Yes | - | Target database |
| `login_host` | string | No | `localhost` | Server hostname |
| `login_user` | string | No | `postgres` | Login user |
| `positional_args` | list | No | - | Query parameters |

**Example:**

```yaml
- name: Run migration
  postgresql_query:
    db: myapp
    query: "CREATE TABLE IF NOT EXISTS users (id SERIAL PRIMARY KEY, name TEXT)"
```

---

### postgresql_privs Module

Manages PostgreSQL privileges.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `database` | string | Yes | - | Target database |
| `roles` | string | Yes | - | Role(s) to grant/revoke |
| `privs` | string | No | - | Privileges to grant |
| `type` | string | No | `table` | Object type |
| `objs` | string | No | - | Object names |
| `state` | string | No | `present` | `present` (grant) or `absent` (revoke) |

**Example:**

```yaml
- name: Grant table access
  postgresql_privs:
    database: myapp
    roles: appuser
    privs: SELECT,INSERT,UPDATE
    type: table
    objs: users,orders
```

---

### mysql_db Module

Manages MySQL/MariaDB databases.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Database name |
| `state` | string | No | `present` | `present`, `absent`, `import`, `dump` |
| `collation` | string | No | `utf8mb4_general_ci` | Database collation |
| `encoding` | string | No | `utf8mb4` | Character set |
| `login_host` | string | No | `localhost` | Server hostname |

**Example:**

```yaml
- name: Create MySQL database
  mysql_db:
    name: myapp
    encoding: utf8mb4
    state: present
```

---

### mysql_user Module

Manages MySQL/MariaDB users.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Username |
| `password` | string | No | - | User password |
| `host` | string | No | `localhost` | Host the user connects from |
| `priv` | string | No | - | Privileges (e.g., `myapp.*:ALL`) |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Create MySQL user
  mysql_user:
    name: appuser
    password: "{{ vault_mysql_password }}"
    priv: "myapp.*:ALL"
    host: "%"
```

---

### mysql_query Module

Executes arbitrary MySQL queries.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | Yes | - | SQL query |
| `login_db` | string | No | - | Target database |
| `login_host` | string | No | `localhost` | Server hostname |
| `login_user` | string | No | `root` | Login user |

**Example:**

```yaml
- name: Run MySQL query
  mysql_query:
    login_db: myapp
    query: "SELECT COUNT(*) FROM users"
  register: user_count
```

---

### mysql_privs Module

Manages MySQL privileges.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `user` | string | Yes | - | Username |
| `host` | string | No | `localhost` | User host |
| `priv` | string | Yes | - | Privilege string |
| `state` | string | No | `present` | `present` (grant) or `absent` (revoke) |

**Example:**

```yaml
- name: Grant read-only access
  mysql_privs:
    user: readonly
    priv: "myapp.*:SELECT"
    state: present
```

---

## Windows Modules

> Requires the `winrm` feature flag (experimental).

### win_copy Module

Copies files to Windows hosts.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `src` | string | Yes* | - | Local source path |
| `content` | string | Yes* | - | File content |
| `dest` | string | Yes | - | Remote destination path |
| `force` | bool | No | `true` | Overwrite existing files |

**Example:**

```yaml
- name: Copy config to Windows
  win_copy:
    src: files/app.config
    dest: C:\Program Files\MyApp\app.config
```

---

### win_feature Module

Manages Windows Server features and roles.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string/list | Yes | - | Feature name(s) |
| `state` | string | No | `present` | `present` or `absent` |
| `include_sub_features` | bool | No | `false` | Include sub-features |
| `include_management_tools` | bool | No | `false` | Include management tools |

**Example:**

```yaml
- name: Install IIS
  win_feature:
    name: Web-Server
    state: present
    include_management_tools: true
```

---

### win_service Module

Manages Windows services.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Service name |
| `state` | string | No | - | `started`, `stopped`, `restarted`, `absent` |
| `start_mode` | string | No | - | `auto`, `manual`, `disabled` |

**Example:**

```yaml
- name: Start Windows service
  win_service:
    name: MyAppService
    state: started
    start_mode: auto
```

---

### win_package Module

Manages Windows software packages (MSI, EXE).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | Yes | - | Package path or URL |
| `state` | string | No | `present` | `present` or `absent` |
| `product_id` | string | No | - | Product ID for idempotency |
| `arguments` | string | No | - | Install arguments |

**Example:**

```yaml
- name: Install application
  win_package:
    path: C:\Installers\app-setup.msi
    state: present
    arguments: /quiet /norestart
```

---

### win_user Module

Manages Windows local user accounts.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Username |
| `password` | string | No | - | User password |
| `state` | string | No | `present` | `present` or `absent` |
| `groups` | list | No | - | Group memberships |
| `password_never_expires` | bool | No | `false` | Password policy |

**Example:**

```yaml
- name: Create Windows user
  win_user:
    name: deploy
    password: "{{ vault_win_password }}"
    groups:
      - Administrators
    state: present
```

---

## HPC Modules

> Requires the `hpc` feature flag. See [HPC reference blueprints](../architecture/hpc/) for full deployment examples.

### hpc_baseline Module

Validates and configures HPC cluster baseline settings (kernel params, limits, packages).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `profile` | string | No | `compute` | Profile: `compute`, `login`, `storage` |
| `kernel_params` | dict | No | - | Sysctl overrides |
| `validate_only` | bool | No | `false` | Only check, don't apply |

**Example:**

```yaml
- name: Apply HPC baseline
  hpc_baseline:
    profile: compute
    kernel_params:
      vm.zone_reclaim_mode: "1"
```

---

### lmod Module

Manages Lmod environment modules. See also [lmod module docs](modules/lmod.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | Yes | - | Module name (e.g., `gcc/12.2`) |
| `state` | string | No | `loaded` | `loaded`, `unloaded`, `default` |
| `modulepath` | string | No | - | Additional module search path |

**Example:**

```yaml
- name: Load compiler module
  lmod:
    name: gcc/12.2
    state: loaded
```

---

### mpi Module

Manages MPI stack installation and configuration. See also [mpi module docs](modules/mpi.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `implementation` | string | Yes | - | `openmpi`, `mpich`, `intel-mpi` |
| `version` | string | No | - | Version to install |
| `state` | string | No | `present` | `present` or `absent` |
| `fabric` | string | No | `auto` | Network fabric: `tcp`, `verbs`, `ucx`, `auto` |

**Example:**

```yaml
- name: Install OpenMPI with UCX
  mpi:
    implementation: openmpi
    version: "4.1"
    fabric: ucx
```

---

### slurm_config Module

Manages Slurm workload manager configuration. See also [slurm_config module docs](modules/slurm_config.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `role` | string | Yes | - | `controller`, `compute`, `login` |
| `cluster_name` | string | Yes | - | Slurm cluster name |
| `partitions` | list | No | - | Partition definitions |
| `state` | string | No | `present` | `present` or `absent` |

**Example:**

```yaml
- name: Configure Slurm controller
  slurm_config:
    role: controller
    cluster_name: hpc-cluster
    partitions:
      - name: gpu
        nodes: "gpu[001-008]"
        default: true
```

---

### slurm_ops Module

Performs Slurm operational tasks (drain, resume, reconfig).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `action` | string | Yes | - | `drain`, `resume`, `reconfig`, `reconfigure` |
| `nodes` | string | No | - | Node list for drain/resume |
| `reason` | string | No | - | Reason for drain |

**Example:**

```yaml
- name: Drain node for maintenance
  slurm_ops:
    action: drain
    nodes: "gpu003"
    reason: "GPU replacement"
```

---

### nvidia_gpu Module

Manages NVIDIA GPU driver and toolkit configuration. See also [nvidia_gpu module docs](modules/nvidia_gpu.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `driver_version` | string | No | - | Target driver version |
| `state` | string | No | `present` | `present`, `absent`, `latest` |
| `persistence_mode` | bool | No | `true` | Enable persistence mode |
| `compute_mode` | string | No | `default` | `default`, `exclusive_thread`, `exclusive_process`, `prohibited` |

**Example:**

```yaml
- name: Configure NVIDIA GPUs
  nvidia_gpu:
    driver_version: "535"
    persistence_mode: true
    compute_mode: exclusive_process
```

---

### rdma_stack Module

Manages InfiniBand/RDMA stack (OFED, drivers, subnet manager).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `state` | string | No | `present` | `present` or `absent` |
| `packages` | list | No | - | Additional RDMA packages |
| `subnet_manager` | bool | No | `false` | Enable OpenSM |
| `interfaces` | list | No | - | IB interface configuration |

**Example:**

```yaml
- name: Configure RDMA stack
  rdma_stack:
    state: present
    subnet_manager: false
    interfaces:
      - name: ib0
        mode: datagram
```

---

### lustre_client Module

Manages Lustre parallel filesystem client. See also [lustre_client module docs](modules/lustre_client.md).

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `mount_point` | string | Yes | - | Local mount path |
| `mgs_nids` | string | Yes | - | MGS NIDs (e.g., `10.0.0.1@tcp`) |
| `filesystem` | string | Yes | - | Filesystem name |
| `state` | string | No | `mounted` | `mounted`, `unmounted`, `present`, `absent` |

**Example:**

```yaml
- name: Mount Lustre filesystem
  lustre_client:
    mount_point: /scratch
    mgs_nids: "10.0.0.1@o2ib"
    filesystem: scratch
    state: mounted
```

---

### beegfs_client Module

Manages BeeGFS parallel filesystem client.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `mount_point` | string | Yes | - | Local mount path |
| `mgmtd_host` | string | Yes | - | Management server hostname |
| `state` | string | No | `mounted` | `mounted`, `unmounted`, `present`, `absent` |
| `client_config` | dict | No | - | Client tuning options |

**Example:**

```yaml
- name: Mount BeeGFS filesystem
  beegfs_client:
    mount_point: /data
    mgmtd_host: beegfs-mgmt.local
    state: mounted
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
