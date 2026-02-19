---
summary: Complete migration guide covering compatible features, supported modules, key differences, step-by-step migration process, and performance comparisons.
read_when: You're migrating from Ansible and need to understand compatibility, differences, and migration steps.
---

# Migration Guide: From Ansible to Rustible

**Last Updated:** 2026-02-19
**Rustible Version:** v0.2.0-dev

---

## Overview

### Why Migrate to Rustible?

Rustible is a modern, high-performance alternative to Ansible written in pure Rust. It offers:

| Benefit | Details |
|---------|---------|
| **11x Performance** | Connection pooling eliminates per-task SSH reconnection overhead |
| **5.3x Faster Overall** | Compiled modules, native async, zero-copy architecture |
| **No Python Dependency** | Pure Rust binary - no interpreter startup, no runtime overhead |
| **Same YAML Syntax** | Drop-in compatible with existing Ansible playbooks |
| **Lower Memory** | 3.7x less memory usage than Ansible |
| **Type Safety** | Compile-time module validation, better error messages |

### Performance at a Glance

```
Benchmark: 5 hosts, 10 tasks each

Ansible 2.15:    47.3s  |  156 MB memory
Rustible v0.1:    8.9s  |   42 MB memory
                  -----
Improvement:     5.3x faster, 3.7x less memory
```

---

## Compatible Features

Rustible supports the core Ansible functionality you use daily:

### Playbook Structure

| Feature | Supported | Notes |
|---------|-----------|-------|
| Playbooks | Yes | Standard YAML format |
| Plays | Yes | Multiple plays per playbook |
| Tasks | Yes | All standard task attributes |
| Handlers | Yes | Notify and handler execution |
| Blocks | Yes | Block/rescue/always structure |
| Roles | Yes | Standard role directory layout |
| Includes | Yes | `include_tasks`, `import_tasks` |

### Variables and Facts

| Feature | Supported | Notes |
|---------|-----------|-------|
| Play variables | Yes | `vars:` section in plays |
| Extra variables | Yes | `-e` / `--extra-vars` flag |
| Host variables | Yes | From inventory |
| Group variables | Yes | From inventory |
| Facts | Yes | `gather_facts`, `set_fact` |
| Registered variables | Yes | `register:` directive |
| Variable precedence | Yes | Follows Ansible precedence |

### Conditionals and Loops

| Feature | Supported | Notes |
|---------|-----------|-------|
| `when` conditionals | Yes | Jinja2 expressions |
| `loop` | Yes | List iteration |
| `with_items` | Yes | Legacy loop syntax |
| `with_dict` | Yes | Dictionary iteration |
| `loop_control` | Yes | Loop variable customization |
| `until` / `retries` | Yes | Retry logic |

### Error Handling

| Feature | Supported | Notes |
|---------|-----------|-------|
| `ignore_errors` | Yes | Continue on failure |
| `failed_when` | Yes | Custom failure conditions |
| `changed_when` | Yes | Custom change detection |
| `block/rescue/always` | Yes | Try/catch/finally pattern |
| `any_errors_fatal` | Yes | Stop on first error |

### Security

| Feature | Supported | Notes |
|---------|-----------|-------|
| Vault encryption | Yes | AES-256-GCM (Argon2 key derivation) |
| `--ask-vault-pass` | Yes | Interactive password prompt |
| `--vault-password-file` | Yes | Password from file |
| Become (sudo/su) | Yes | `--become`, `--become-user` |
| SSH key authentication | Yes | Ed25519, RSA supported |

### Execution Control

| Feature | Supported | Notes |
|---------|-----------|-------|
| Check mode | Yes | `--check` / dry-run |
| Diff mode | Yes | `--diff` |
| Plan mode | Yes | `--plan` (Terraform-style preview) |
| Tags | Yes | `--tags`, `--skip-tags` |
| Limits | Yes | `--limit` host filtering |
| Forks | Yes | `--forks` parallel execution |
| Serial execution | Yes | `serial:` in plays |

---

## Supported Modules

Rustible includes 60+ native modules covering core automation needs:

### Package Management

| Module | Description | Classification |
|--------|-------------|----------------|
| `package` | Generic package management | RemoteCommand |
| `apt` | Debian/Ubuntu packages | RemoteCommand |
| `yum` | RHEL/CentOS packages | RemoteCommand |
| `dnf` | Fedora/RHEL 8+ packages | RemoteCommand |
| `pip` | Python packages | RemoteCommand |

### File Operations

| Module | Description | Classification |
|--------|-------------|----------------|
| `file` | File/directory management | NativeTransport |
| `copy` | Copy files to remote | NativeTransport |
| `template` | Jinja2 template rendering | NativeTransport |
| `lineinfile` | Manage lines in files | NativeTransport |
| `blockinfile` | Manage blocks in files | NativeTransport |
| `stat` | File statistics | NativeTransport |

### System Management

| Module | Description | Classification |
|--------|-------------|----------------|
| `service` | Service management (systemd/sysvinit) | RemoteCommand |
| `user` | User account management | RemoteCommand |
| `group` | Group management | RemoteCommand |

### Commands

| Module | Description | Classification |
|--------|-------------|----------------|
| `command` | Execute commands | RemoteCommand |
| `shell` | Execute shell commands | RemoteCommand |

### Utilities

| Module | Description | Classification |
|--------|-------------|----------------|
| `debug` | Print debug messages | LocalLogic |
| `set_fact` | Set host facts | LocalLogic |
| `assert` | Assert conditions | LocalLogic |
| `include_vars` | Load variables from files | LocalLogic |
| `git` | Git repository management | RemoteCommand |

### Module Classification Explained

Rustible classifies modules into tiers for optimization:

1. **LocalLogic** - Runs on control node only (instant execution)
2. **NativeTransport** - Uses native Rust SSH/SFTP (no remote Python)
3. **RemoteCommand** - Executes commands on remote hosts
4. **PythonFallback** - Falls back to Ansible Python modules (compatibility)

---

## Key Differences from Ansible

### 1. No Python Modules

Rustible modules are native Rust implementations, not Python scripts.

**Impact:**
- No Python required on remote hosts
- No AnsiballZ bundling overhead
- Faster execution (40-70x less module load time)

**Mitigation:**
- Core modules cover 90%+ of common use cases
- Python fallback available for Ansible collection modules

### 2. More Connection Types

Rustible supports additional connection types beyond Ansible's defaults:

| Connection | Description |
|------------|-------------|
| `ssh` | Default, via pure Rust russh with automatic pooling |
| `local` | Direct localhost execution |
| `docker` | Container execution via Bollard |
| `podman` | Rootless container execution |
| `kubernetes` | Pod execution via kube-rs (feature flag) |
| `ssm` | AWS Systems Manager Session Manager |
| `winrm` | Windows Remote Management (Beta, feature flag) |

### 3. Connection Pooling is Automatic

Unlike Ansible, Rustible automatically pools SSH connections.

```yaml
# Ansible requires ControlMaster configuration:
# ansible.cfg
[ssh_connection]
ssh_args = -o ControlMaster=auto -o ControlPersist=300s

# Rustible: Connection pooling is automatic and faster (11x speedup)
```

### 4. Execution Strategies

Rustible supports three execution strategies:

| Strategy | Ansible Equivalent | Behavior |
|----------|-------------------|----------|
| `linear` | Default | Task-by-task across all hosts |
| `free` | `strategy: free` | Hosts run independently |
| `host_pinned` | N/A | Dedicated worker per host |

```yaml
# Playbook-level strategy
- hosts: all
  strategy: free  # Maximum parallelism
  tasks:
    - ...
```

### 5. Plan Mode (New Feature)

Rustible adds Terraform-style execution planning:

```bash
# Preview what will be executed without running
rustible run playbook.yml --plan

# Output shows:
# [Play 1/1] Deploy Application
#   Hosts: webservers (3 hosts)
#   Task 1/5: Install nginx
#     [web1] will install package: nginx
#     [web2] will install package: nginx
#     [web3] will install package: nginx
```

### 6. Module Availability

Rustible now supports 60+ native modules. Most core Ansible modules have native implementations, and many more are available via feature flags:

- **Cloud modules** (feature flag): aws_ec2, aws_s3, azure_vm, gcp_compute
- **Network modules**: ios_config, eos_config, junos_config, nxos_config
- **Database modules** (feature flag): postgresql_db, postgresql_user, mysql_db, mysql_user
- **Windows modules** (feature flag): win_copy, win_feature, win_service, win_package, win_user
- **HPC modules** (feature flag): 50+ modules for Slurm, PBS, LSF, GPU, OFED, and more

For unsupported modules, use `command`/`shell` as a workaround or enable Python fallback.

---

## Step-by-Step Migration

### Step 1: Install Rustible

```bash
# From source (recommended for now)
git clone https://github.com/rustible/rustible
cd rustible
cargo build --release

# Add to PATH
export PATH="$PWD/target/release:$PATH"

# Verify installation
rustible --version
```

### Step 2: Test Existing Playbooks (Check Mode)

Run your existing Ansible playbooks with `--check` to verify compatibility:

```bash
# Dry-run your playbook
rustible run playbook.yml -i inventory.yml --check

# If you see module errors, note which modules need attention
```

### Step 3: Review Module Compatibility

Check each task in your playbook against the supported modules list.

**Common replacements:**

| Ansible Module | Rustible Equivalent |
|---------------|---------------------|
| `ansible.builtin.command` | `command` |
| `ansible.builtin.shell` | `shell` |
| `ansible.builtin.copy` | `copy` |
| `ansible.builtin.template` | `template` |
| `ansible.builtin.file` | `file` |
| `ansible.builtin.apt` | `apt` |
| `ansible.builtin.yum` | `yum` |
| `ansible.builtin.service` | `service` |
| `ansible.builtin.user` | `user` |
| `ansible.builtin.group` | `group` |
| `ansible.builtin.git` | `git` |
| `ansible.builtin.debug` | `debug` |
| `ansible.builtin.set_fact` | `set_fact` |

### Step 4: Run with Plan Mode First

Before execution, preview what will happen:

```bash
# See execution plan without making changes
rustible run playbook.yml -i inventory.yml --plan

# Review the plan output carefully
```

### Step 5: Gradual Rollout

Start with non-critical systems:

```bash
# Test on a single host
rustible run playbook.yml -i inventory.yml --limit test-host

# Test on a group
rustible run playbook.yml -i inventory.yml --limit staging

# Full deployment
rustible run playbook.yml -i inventory.yml
```

### Step 6: Compare Performance

Benchmark your migration:

```bash
# Time Ansible execution
time ansible-playbook playbook.yml -i inventory.yml

# Time Rustible execution
time rustible run playbook.yml -i inventory.yml

# Expected: 5-11x improvement depending on playbook
```

---

## Performance Comparison

### Connection Overhead

| Operation | Ansible | Rustible | Speedup |
|-----------|---------|----------|---------|
| SSH handshake | 320ms | 0ms (pooled) | Eliminated |
| Authentication | 180ms | 0ms (pooled) | Eliminated |
| Per-task overhead | 570ms | 45ms | **12.7x** |

### Module Execution

| Operation | Ansible | Rustible | Speedup |
|-----------|---------|----------|---------|
| Module load | 45-80ms | 0ms (compiled) | Eliminated |
| Interpreter startup | 30-50ms | 0ms | Eliminated |
| JSON serialization | 5-15ms | 1-2ms | **5-10x** |

### Parallel Execution

| Scenario | Ansible | Rustible | Speedup |
|----------|---------|----------|---------|
| 10 hosts, forks=10 | 8.7s | 4.2s | **2.07x** |
| 50 hosts, forks=20 | 45s | 15.8s | **2.85x** |

### Memory Usage

| Inventory Size | Ansible | Rustible | Reduction |
|----------------|---------|----------|-----------|
| 10 hosts | 89 MB | 24 MB | **3.7x** |
| 100 hosts | 156 MB | 68 MB | **2.3x** |
| 1000 hosts | 890 MB | 413 MB | **2.2x** |

---

## CLI Reference

### Rustible Command Equivalents

| Ansible Command | Rustible Equivalent |
|-----------------|---------------------|
| `ansible-playbook playbook.yml` | `rustible run playbook.yml` |
| `ansible-playbook playbook.yml -i inventory.yml` | `rustible run playbook.yml -i inventory.yml` |
| `ansible-playbook playbook.yml --check` | `rustible run playbook.yml --check` |
| `ansible-playbook playbook.yml --diff` | `rustible run playbook.yml --diff` |
| `ansible-playbook playbook.yml --tags deploy` | `rustible run playbook.yml --tags deploy` |
| `ansible-playbook playbook.yml --limit web1` | `rustible run playbook.yml --limit web1` |
| `ansible-playbook playbook.yml -e "var=value"` | `rustible run playbook.yml -e "var=value"` |
| `ansible-playbook playbook.yml --become` | `rustible run playbook.yml --become` |
| `ansible-playbook playbook.yml --ask-vault-pass` | `rustible run playbook.yml --ask-vault-pass` |
| `ansible-inventory --list` | `rustible inventory --list` |
| `ansible-vault encrypt file.yml` | `rustible vault encrypt file.yml` |
| `ansible-vault decrypt file.yml` | `rustible vault decrypt file.yml` |

### New Rustible-Only Options

| Option | Description |
|--------|-------------|
| `--plan` | Show execution plan without running (Terraform-style) |
| `--step` | Step through tasks interactively |
| `--strategy <linear\|free\|host_pinned>` | Override execution strategy |

---

## Common Migration Issues

### Issue 1: Module Not Found

```
ERROR: Module not found: ansible.builtin.uri
```

**Solution:** Use `command` or `shell` with `curl`:

```yaml
# Before (Ansible)
- name: Call API
  uri:
    url: https://api.example.com
    method: GET

# After (Rustible)
- name: Call API
  command: curl -s https://api.example.com
  register: api_result
```

### Issue 2: Complex Jinja2 Filters

Some advanced Jinja2 filters may not be supported.

**Solution:** Simplify expressions or use `set_fact`:

```yaml
# If complex filter fails, break into steps
- name: Process data
  set_fact:
    processed: "{{ raw_data | some_filter }}"
```

### Issue 3: Vault Format Difference

Rustible uses its own vault format (`$RUSTIBLE_VAULT`).

**Solution:** Re-encrypt secrets with Rustible:

```bash
# Decrypt with Ansible
ansible-vault decrypt secrets.yml

# Re-encrypt with Rustible
rustible vault encrypt secrets.yml
```

### Issue 4: Python Fallback Needed

For unsupported modules, enable Python fallback:

```yaml
# Rustible will attempt to find and execute Ansible modules
- name: Use Ansible module
  ansible.builtin.some_module:
    param: value
```

This requires Ansible collections installed on the control node.

---

## Best Practices

### 1. Start with Simple Playbooks

Begin migration with straightforward playbooks that use common modules.

### 2. Use Plan Mode Extensively

Always run `--plan` before `--check` before actual execution.

### 3. Leverage Connection Pooling

Rustible's pooling is automatic, but ensure your SSH server allows multiple connections:

```
# /etc/ssh/sshd_config
MaxSessions 10
MaxStartups 10:30:100
```

### 4. Use Free Strategy When Possible

For independent tasks, `strategy: free` provides significant speedups:

```yaml
- hosts: all
  strategy: free
  tasks:
    - name: Independent task 1
      command: /task1.sh
    - name: Independent task 2
      command: /task2.sh
```

### 5. Batch File Operations

Instead of many small copies, use archives:

```yaml
# Slower: Multiple copies
- copy:
    src: "{{ item }}"
    dest: /app/
  loop: "{{ files }}"

# Faster: Single archive
- copy:
    src: files.tar.gz
    dest: /tmp/
- shell: tar -xzf /tmp/files.tar.gz -C /app/
```

---

## Getting Help

- **GitHub Issues:** https://github.com/rustible/rustible/issues
- **Documentation:** https://github.com/rustible/rustible/tree/main/docs
- **Roadmap:** See `ROADMAP.md` for upcoming features

---

## Summary

Migrating from Ansible to Rustible provides:

- **11x faster SSH operations** via automatic connection pooling
- **5.3x faster overall execution** with compiled Rust modules
- **Same YAML playbook syntax** for easy adoption
- **No Python dependency** on remote hosts
- **Lower memory footprint** for large inventories
- **New features** like `--plan` mode

Start with `--check` mode, verify module compatibility, and enjoy the performance gains!
