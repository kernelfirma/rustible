---
summary: Overview of all built-in modules, their classification tiers, categories, usage patterns, and return values.
read_when: You want to know which modules are available, how to use them, or how module execution works internally.
---

# Chapter 5: Built-in Modules

Modules are the building blocks that perform actual work on target systems. Rustible ships with a comprehensive set of native Rust modules and provides a Python fallback path for Ansible module compatibility.

## Module Classification

Every module in Rustible belongs to one of four execution tiers. This classification drives intelligent parallelization and determines how each module interacts with target hosts.

| Tier | Name | Runs On | Examples |
|------|------|---------|----------|
| 1 | **LocalLogic** | Control node only | debug, set_fact, assert, fail, meta, include_vars, pause, uri |
| 2 | **NativeTransport** | Control node via SSH/SFTP | copy, template, file, lineinfile, blockinfile, stat, synchronize |
| 3 | **RemoteCommand** | Remote host via SSH | command, shell, service, package, user, group, apt, yum, raw |
| 4 | **PythonFallback** | Remote host via Python | Any Ansible module without a native Rust implementation |

**Tier 1 (LocalLogic)** modules never touch the remote host and execute in nanoseconds. **Tier 2 (NativeTransport)** modules use direct SSH/SFTP operations without remote Python. **Tier 3 (RemoteCommand)** modules execute commands on the remote host. **Tier 4 (PythonFallback)** provides backwards compatibility by running Ansible Python modules via an AnsiballZ-compatible wrapper.

## Module Categories

### Package Management (5 modules)

| Module | Description |
|--------|-------------|
| `apt` | Manage Debian/Ubuntu packages |
| `yum` | Manage RHEL/CentOS packages |
| `dnf` | Manage Fedora/RHEL 8+ packages |
| `pip` | Manage Python packages |
| `package` | Generic package manager (auto-detects apt/yum/dnf) |

```yaml
- name: Install nginx
  package:
    name: nginx
    state: present
```

### File Management (9 modules)

| Module | Description |
|--------|-------------|
| `copy` | Copy files from control node to remote |
| `file` | Manage file/directory properties |
| `template` | Render Jinja2 templates to remote |
| `lineinfile` | Ensure a line exists in a file |
| `blockinfile` | Insert/update/remove a block of text |
| `archive` | Create compressed archives |
| `unarchive` | Extract compressed archives |
| `stat` | Retrieve file status information |
| `synchronize` | rsync-based file synchronization |

```yaml
- name: Deploy configuration
  template:
    src: nginx.conf.j2
    dest: /etc/nginx/nginx.conf
    owner: root
    group: root
    mode: '0644'
  notify: Restart nginx
```

### Command Execution (4 modules)

| Module | Description |
|--------|-------------|
| `command` | Execute a command (no shell processing) |
| `shell` | Execute via shell (supports pipes, redirects) |
| `raw` | Execute raw command over SSH (no module wrapper) |
| `script` | Transfer and execute a local script on remote |

```yaml
- name: Run health check
  command: /opt/app/healthcheck.sh
  register: health
  failed_when: health.rc not in [0, 1]
```

### System Administration (10+ modules)

| Module | Description |
|--------|-------------|
| `service` | Manage system services |
| `systemd_unit` | Manage systemd units directly |
| `user` | Manage user accounts |
| `group` | Manage groups |
| `cron` | Manage cron jobs |
| `hostname` | Set system hostname |
| `sysctl` | Manage sysctl parameters |
| `mount` | Manage filesystem mounts |
| `timezone` | Set system timezone |
| `pause` | Pause playbook execution |
| `wait_for` | Wait for a condition (port, file, etc.) |

```yaml
- name: Ensure nginx is running and enabled
  service:
    name: nginx
    state: started
    enabled: true
```

### Source Control

| Module | Description |
|--------|-------------|
| `git` | Clone and manage Git repositories |

### Utility and Logic

| Module | Description |
|--------|-------------|
| `debug` | Print messages or variable values |
| `assert` | Assert conditions are true |
| `fail` | Fail with a custom message |
| `set_fact` | Set host-scoped variables |
| `include_vars` | Load variables from files |
| `meta` | Control playbook execution flow |

```yaml
- name: Validate configuration
  assert:
    that:
      - http_port > 0
      - http_port < 65536
    fail_msg: "Invalid HTTP port: {{ http_port }}"
```

### Fact Gathering

| Module | Description |
|--------|-------------|
| `facts` | Gather system facts from target hosts |

### Network and Security

| Module | Description |
|--------|-------------|
| `uri` | Interact with HTTP/HTTPS endpoints |
| `authorized_key` | Manage SSH authorized keys |
| `known_hosts` | Manage SSH known hosts |
| `ufw` | Manage Ubuntu firewall rules |
| `firewalld` | Manage firewalld rules |
| `selinux` | Manage SELinux state and policy |

### Docker (feature flag: `docker`)

| Module | Description |
|--------|-------------|
| `docker_container` | Manage Docker containers |
| `docker_image` | Manage Docker images |
| `docker_network` | Manage Docker networks |
| `docker_volume` | Manage Docker volumes |
| `docker_compose` | Manage Docker Compose stacks |

### Kubernetes (feature flag: `k8s`)

| Module | Description |
|--------|-------------|
| `k8s_namespace` | Manage Kubernetes namespaces |
| `k8s_deployment` | Manage deployments |
| `k8s_service` | Manage services |
| `k8s_configmap` | Manage ConfigMaps |
| `k8s_secret` | Manage Secrets |

### Cloud (feature flags)

Cloud modules are available through feature flags at compile time:

- **AWS** (`cloud-aws`): EC2, S3, IAM, RDS, ELB, VPC, Route53, Lambda, and more
- **Azure** (`cloud-azure`): VM, Storage, Network, AKS, and more
- **GCP** (`cloud-gcp`): Compute, Storage, GKE, and more
- **Proxmox** (built-in): `proxmox_vm`, `proxmox_lxc`

### Network Devices

| Module | Description |
|--------|-------------|
| `ios_config` | Cisco IOS configuration |
| `eos_config` | Arista EOS configuration |
| `junos_config` | Juniper Junos configuration |
| `nxos_config` | Cisco NX-OS configuration |

### Database (feature flag: `database`)

| Module | Description |
|--------|-------------|
| `postgresql_*` | PostgreSQL database management |
| `mysql_*` | MySQL/MariaDB database management |

### Windows

| Module | Description |
|--------|-------------|
| `win_copy` | Copy files to Windows hosts |
| `win_feature` | Manage Windows features |
| `win_service` | Manage Windows services |
| `win_package` | Manage Windows packages |
| `win_user` | Manage Windows user accounts |

### HPC (feature flag: `hpc`)

| Module | Description |
|--------|-------------|
| `slurm_config` | Manage SLURM configuration |
| `nvidia_gpu` | Manage NVIDIA GPU settings |
| `lmod` | Manage Lmod environment modules |
| `mpi` | Manage MPI configurations |

## Using Modules

### Basic Syntax

Every task invokes exactly one module:

```yaml
- name: Descriptive task name
  module_name:
    param1: value1
    param2: value2
  when: condition
  register: result_var
```

### Check Mode

Most modules support check mode (`--check`), which reports what would change without making actual changes. Modules that support check mode will return accurate `changed` status during dry runs.

```bash
rustible run playbook.yml --check --diff
```

### Diff Mode

When `--diff` is enabled, modules that modify files will show before/after differences in the output.

## Module Return Values

Every module execution produces a `ModuleOutput` with these fields:

| Field | Type | Description |
|-------|------|-------------|
| `changed` | `bool` | Whether the module made changes to the system |
| `msg` | `string` | Human-readable message about what happened |
| `status` | `string` | Execution status: `ok`, `changed`, `failed`, `skipped` |
| `diff` | `object` | Before/after diff (when available and `--diff` is set) |
| `data` | `map` | Additional module-specific structured data |
| `stdout` | `string` | Standard output (command modules) |
| `stderr` | `string` | Standard error (command modules) |
| `rc` | `int` | Return code (command modules) |

Access these through registered variables:

```yaml
- command: cat /etc/hostname
  register: result

- debug:
    msg: "Host: {{ result.stdout }}, RC: {{ result.rc }}"
```

## Python Fallback

For Ansible modules that do not have a native Rust implementation, Rustible provides a Python fallback path. When a module name is not recognized as a built-in, the executor searches for a matching Ansible Python module and runs it via an AnsiballZ-compatible wrapper.

This requires Python to be installed on the target host, but it ensures broad compatibility with the existing Ansible module ecosystem. Native modules are always preferred for performance.

## Next Steps

- Learn about [Roles](06-roles.md) for organizing modules into reusable units
- See the [full module reference](../reference/modules.md) for detailed parameter documentation
- Understand [Templating](09-templating.md) for the `template` module
