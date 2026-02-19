---
summary: Get up and running with Rustible in minutes, covering installation, first playbook, inventory setup, and common patterns.
read_when: You want to quickly install Rustible and run your first playbook.
---

# Rustible Quick Start Guide

Get up and running with Rustible in minutes. This guide covers installation, your first playbook, inventory setup, and common patterns.

## Installation

### Building from Source

```bash
# Clone the repository
git clone https://github.com/rustible/rustible.git
cd rustible

# Build in release mode (pure Rust, no C dependencies)
cargo build --release

# Install to ~/.cargo/bin
cargo install --path .

# Verify installation
rustible --version
```

### From Crates.io (Coming Soon)

```bash
cargo install rustible
```

### Feature Flags

Rustible supports multiple feature configurations:

| Feature | Description |
|---------|-------------|
| `russh` (default) | Pure Rust SSH backend - recommended |
| `ssh2-backend` | Legacy SSH via libssh2 (requires C dependencies) |
| `docker` | Docker container execution support |
| `kubernetes` | Kubernetes pod execution |
| `pure-rust` | Minimal pure Rust build |
| `aws` | AWS cloud modules (EC2, S3, VPC, IAM) |
| `azure` | Azure cloud modules (experimental) |
| `gcp` | GCP cloud modules (experimental) |
| `hpc` | HPC modules (Slurm, GPU, OFED) |
| `slurm` | Slurm workload manager modules |
| `gpu` | GPU management modules (NVIDIA) |
| `pbs` | PBS Pro workload manager modules |
| `lsf` | IBM Spectrum LSF modules |
| `ofed` | InfiniBand/RDMA/OFED support |
| `parallel_fs` | Parallel filesystem clients (Lustre, BeeGFS) |
| `identity` | Kerberos and SSSD identity management |
| `bare_metal` | PXE boot and Warewulf bare-metal provisioning |
| `redfish` | Bare-metal BMC management via Redfish/IPMI |
| `winrm` | Windows Remote Management (Beta) |
| `database` | Database modules (PostgreSQL, MySQL) |
| `full` | All core features enabled |
| `full-cloud` | All features plus all cloud providers |
| `full-hpc` | All features plus full HPC stack |

```bash
# Pure Rust build (default)
cargo build --release

# With Docker support
cargo build --release --features docker

# Legacy ssh2 backend (requires libssh2)
cargo build --release --features ssh2-backend
```

## Your First Playbook

Create `hello.yml`:

```yaml
---
- name: Hello World
  hosts: localhost
  gather_facts: false

  tasks:
    - name: Print greeting
      debug:
        msg: "Hello from Rustible!"

    - name: Show system info
      debug:
        msg: "Running on {{ ansible_hostname | default('localhost') }}"
```

Run it:

```bash
rustible run hello.yml
```

Expected output:

```
================================================================================
PLAYBOOK: hello.yml
================================================================================
Loading playbook...
No inventory specified, using localhost

PLAY [Hello World] ************************************************************

TASK [Print greeting] *********************************************************
localhost: ok
DEBUG: Hello from Rustible!

TASK [Show system info] *******************************************************
localhost: ok
DEBUG: Running on localhost

PLAY RECAP ********************************************************************
localhost                  : ok=2    changed=0    failed=0    skipped=0

Playbook finished in 0.02s
```

## Inventory Setup

### localhost (No Inventory Required)

For local execution, no inventory file is needed:

```bash
rustible run playbook.yml
```

### INI Format

Create `inventory.ini`:

```ini
[webservers]
web1 ansible_host=192.168.1.10
web2 ansible_host=192.168.1.11 ansible_port=2222

[dbservers]
db1 ansible_host=192.168.1.20

[webservers:vars]
http_port=80
ansible_user=deploy

[all:vars]
ansible_ssh_private_key_file=~/.ssh/id_rsa
```

### YAML Format

Create `inventory.yml`:

```yaml
all:
  vars:
    ansible_user: deploy
    ansible_ssh_private_key_file: ~/.ssh/id_rsa

  hosts:
    localhost:
      ansible_connection: local

  children:
    webservers:
      vars:
        http_port: 80
      hosts:
        web1:
          ansible_host: 192.168.1.10
        web2:
          ansible_host: 192.168.1.11
          ansible_port: 2222

    dbservers:
      hosts:
        db1:
          ansible_host: 192.168.1.20
          ansible_user: postgres
```

### Host Variables

Common host variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `ansible_host` | IP or hostname to connect to | Host name |
| `ansible_port` | SSH port | 22 |
| `ansible_user` | SSH username | Current user |
| `ansible_ssh_private_key_file` | Path to SSH key | ~/.ssh/id_rsa |
| `ansible_connection` | Connection type: `ssh`, `local`, `docker`, `podman`, `kubernetes`, `ssm`, `winrm` | ssh |
| `ansible_become` | Enable privilege escalation | false |
| `ansible_become_user` | User to become | root |
| `ansible_become_method` | Method: `sudo`, `su` | sudo |

## Running Playbooks

### Basic Execution

```bash
# Run with default settings
rustible run playbook.yml -i inventory.yml

# Specify inventory
rustible run playbook.yml -i inventory.yml

# Multiple inventory sources
rustible run playbook.yml -i inventory.yml -i extra_hosts.ini
```

### Common Options

```bash
# Verbose output (-v, -vv, -vvv for more detail)
rustible run playbook.yml -i inventory.yml -v

# Dry run (check mode) - show what would change
rustible run playbook.yml -i inventory.yml --check

# Plan mode - show execution plan without running
rustible run playbook.yml -i inventory.yml --plan

# Limit to specific hosts or groups
rustible run playbook.yml -i inventory.yml --limit webservers
rustible run playbook.yml -i inventory.yml --limit web1,web2
rustible run playbook.yml -i inventory.yml --limit '~web.*'  # regex

# Run only tagged tasks
rustible run playbook.yml -i inventory.yml --tags install,configure
rustible run playbook.yml -i inventory.yml --skip-tags slow

# Extra variables
rustible run playbook.yml -i inventory.yml -e "version=2.0"
rustible run playbook.yml -i inventory.yml -e "@vars.yml"

# Privilege escalation
rustible run playbook.yml -i inventory.yml --become --become-user root

# SSH options
rustible run playbook.yml -i inventory.yml --user deploy --private-key ~/.ssh/deploy_key
```

### Full CLI Reference

```
rustible run [OPTIONS] <PLAYBOOK>

Arguments:
  <PLAYBOOK>  Path to the playbook file

Options:
  -i, --inventory <PATH>       Inventory file or directory
  -l, --limit <PATTERN>        Limit to hosts matching pattern
  -e, --extra-vars <VARS>      Extra variables (key=value or @file.yml)
  -t, --tags <TAGS>            Only run tasks with these tags
      --skip-tags <TAGS>       Skip tasks with these tags
      --start-at-task <NAME>   Start at specific task
      --step                   Step through tasks interactively
  -c, --check                  Dry run without making changes
      --plan                   Show execution plan only
  -v, --verbose                Increase verbosity (-v, -vv, -vvv)
  -b, --become                 Run with privilege escalation
      --become-method <METHOD> Escalation method [default: sudo]
      --become-user <USER>     User to become [default: root]
  -K, --ask-become-pass        Prompt for become password
  -u, --user <USER>            Remote user
      --private-key <PATH>     SSH private key file
      --ask-vault-pass         Prompt for vault password
      --vault-password-file    Vault password file
  -h, --help                   Print help
```

## Common Patterns

### Variables

```yaml
- name: Variable examples
  hosts: localhost
  gather_facts: false

  vars:
    app_name: myapp
    app_version: "1.2.3"
    packages:
      - nginx
      - curl
      - jq
    config:
      port: 8080
      workers: 4

  tasks:
    - name: Use simple variable
      debug:
        msg: "Deploying {{ app_name }} version {{ app_version }}"

    - name: Access nested variable
      debug:
        msg: "Listening on port {{ config.port }}"

    - name: Default values
      debug:
        msg: "Environment: {{ environment | default('development') }}"
```

### Conditionals

```yaml
tasks:
  - name: Run only on Debian systems
    package:
      name: nginx
      state: present
    when: ansible_os_family == "Debian"

  - name: Multiple conditions (AND)
    debug:
      msg: "Production Debian server"
    when:
      - ansible_os_family == "Debian"
      - environment == "production"

  - name: OR condition
    debug:
      msg: "RedHat-based system"
    when: ansible_os_family == "RedHat" or ansible_os_family == "Rocky"

  - name: Check variable is defined
    debug:
      msg: "Version is {{ app_version }}"
    when: app_version is defined
```

### Loops

```yaml
tasks:
  - name: Install multiple packages
    package:
      name: "{{ item }}"
      state: present
    loop:
      - nginx
      - curl
      - htop

  - name: Loop with variable
    package:
      name: "{{ item }}"
      state: present
    loop: "{{ packages }}"

  - name: Loop with index
    debug:
      msg: "Item {{ ansible_loop.index }}: {{ item }}"
    loop:
      - apple
      - banana
      - cherry
    loop_control:
      index_var: ansible_loop

  - name: Loop over dict
    debug:
      msg: "{{ item.key }} = {{ item.value }}"
    loop: "{{ config | dict2items }}"
```

### Handlers

```yaml
- name: Handler example
  hosts: webservers
  become: true

  tasks:
    - name: Update nginx config
      template:
        src: nginx.conf.j2
        dest: /etc/nginx/nginx.conf
      notify: Restart nginx

    - name: Update site config
      template:
        src: site.conf.j2
        dest: /etc/nginx/sites-enabled/default
      notify: Restart nginx

  handlers:
    - name: Restart nginx
      service:
        name: nginx
        state: restarted

    - name: Reload nginx
      service:
        name: nginx
        state: reloaded
```

Handlers run once at the end of the play, even if notified multiple times.

### Register and Use Results

```yaml
tasks:
  - name: Check if file exists
    stat:
      path: /etc/myapp/config.yml
    register: config_file

  - name: Create config if missing
    copy:
      content: "default: true"
      dest: /etc/myapp/config.yml
    when: not config_file.stat.exists

  - name: Run command and capture output
    command: whoami
    register: current_user

  - name: Display result
    debug:
      msg: "Running as {{ current_user.stdout }}"
```

### Error Handling

```yaml
tasks:
  - name: Try something that might fail
    command: /opt/app/healthcheck.sh
    ignore_errors: true
    register: health_result

  - name: Respond to failure
    debug:
      msg: "Health check failed, taking action..."
    when: health_result.failed

  - name: Block with rescue
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
      - name: Always run cleanup
        command: /opt/cleanup.sh
```

### Tags

```yaml
tasks:
  - name: Install packages
    package:
      name: nginx
      state: present
    tags:
      - install
      - packages

  - name: Configure service
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
      - service
      - always  # 'always' tag runs regardless of --tags filter
```

```bash
# Run only install tasks
rustible run playbook.yml --tags install

# Run install and configure
rustible run playbook.yml --tags install,configure

# Skip slow tasks
rustible run playbook.yml --skip-tags slow
```

## Performance Comparison

Rustible delivers significant performance improvements over Ansible:

| Scenario | Ansible | Rustible | Speedup |
|----------|---------|----------|---------|
| Simple playbook (10 hosts) | 8.2s | 1.4s | **5.9x** |
| File copy (100 files) | 45.3s | 8.1s | **5.6x** |
| Template rendering | 12.1s | 2.3s | **5.3x** |
| Fact gathering (20 hosts) | 15.7s | 3.2s | **4.9x** |
| GPU cluster bootstrap (8 nodes) | 4m 12s | 47s | **5.4x** |
| Large fleet (50 hosts, parallel) | 2m 45s | 15s | **11x** |

### Why Rustible is Faster

1. **Compiled binary**: No Python interpreter startup overhead
2. **Async I/O**: True async execution with Tokio runtime
3. **Connection pooling**: SSH connections reused across tasks
4. **Native modules**: Core modules run as compiled Rust code
5. **Parallel by default**: Concurrent host execution out of the box

### Real-World Impact

For teams managing infrastructure at scale:

```
Example: 50-host deployment, 3 times daily

Ansible:  2m 45s x 3 = 8m 15s/day = 50+ hours/year
Rustible: 15s x 3 = 45s/day = 4.5 hours/year

Time saved: 45+ hours/year on this single workflow
```

For GPU/HPC infrastructure billed by the hour:

```
8-node GPU cluster @ $296/hr:
- Ansible bootstrap: 4m 12s = $20.72 per deployment
- Rustible bootstrap: 47s = $3.87 per deployment
- Savings: $16.85 per deployment
```

## Next Steps

- Read the [Architecture Guide](ARCHITECTURE.md) for deeper understanding
- Explore [example playbooks](../examples/) in the repository
- Check the [module documentation](modules/) for available modules
- See [performance documentation](performance.md) for optimization tips

## Getting Help

```bash
# General help
rustible --help

# Command-specific help
rustible run --help

# Version information
rustible --version
```

For issues and feature requests, visit the [GitHub repository](https://github.com/rustible/rustible).
