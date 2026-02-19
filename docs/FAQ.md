# Rustible FAQ & Troubleshooting Knowledge Base

Comprehensive FAQ and troubleshooting guide for Rustible - the high-performance Ansible-compatible automation tool written in Rust.

---

## Table of Contents

1. [Installation](#installation)
2. [Configuration](#configuration)
3. [Modules](#modules)
4. [Connection Issues](#connection-issues)
5. [Performance](#performance)
6. [Migration from Ansible](#migration-from-ansible)
7. [Debugging Decision Trees](#debugging-decision-trees)

---

## Installation

### Q: What are the system requirements for Rustible?

**A:** Rustible requires:
- **Rust 1.85+** (for building from source)
- **OpenSSL development headers** (if using `ssh2-backend` feature)
- **Linux, macOS, or Windows** (with WSL2 recommended for Windows)

For the default pure-Rust build, no additional C dependencies are needed.

### Q: How do I install Rustible from source?

**A:**
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

### Q: What feature flags are available?

**A:**

| Feature | Description |
|---------|-------------|
| `russh` (default) | Pure Rust SSH backend - recommended |
| `ssh2-backend` | Legacy SSH via libssh2 (requires C dependencies) |
| `docker` | Docker container execution support |
| `kubernetes` | Kubernetes pod execution |
| `aws` | AWS cloud modules (EC2, S3, IAM) |
| `hpc` | HPC modules (Slurm, GPU, OFED) |
| `slurm` | Slurm workload manager modules |
| `gpu` | GPU management modules (NVIDIA) |
| `pbs` | PBS Pro workload manager modules |
| `lsf` | IBM Spectrum LSF modules |
| `ofed` | InfiniBand/RDMA/OFED support |
| `parallel_fs` | Parallel filesystem clients (Lustre, BeeGFS) |
| `identity` | Kerberos and SSSD identity management |
| `bare_metal` | PXE boot and Warewulf provisioning |
| `redfish` | Bare-metal BMC management via Redfish/IPMI |
| `winrm` | Windows Remote Management (Beta) |
| `database` | Database modules (PostgreSQL, MySQL) |
| `full-hpc` | All features plus full HPC stack |
| `pure-rust` | Minimal pure Rust build |
| `full` | All features enabled |

Build with specific features:
```bash
# Pure Rust build (default)
cargo build --release

# With Docker support
cargo build --release --features docker

# Legacy ssh2 backend
cargo build --release --features ssh2-backend
```

### Q: I get "error: linker `cc` not found" during build

**A:** Install build essentials for your platform:

```bash
# Debian/Ubuntu
sudo apt install build-essential

# Fedora/RHEL
sudo dnf groupinstall "Development Tools"

# macOS
xcode-select --install

# Arch Linux
sudo pacman -S base-devel
```

### Q: Build fails with OpenSSL errors when using ssh2-backend

**A:** Install OpenSSL development libraries:

```bash
# Debian/Ubuntu
sudo apt install libssl-dev pkg-config

# Fedora/RHEL
sudo dnf install openssl-devel

# macOS
brew install openssl
export OPENSSL_DIR=$(brew --prefix openssl)
```

Consider using the default `russh` backend instead, which has no C dependencies.

### Q: How do I update Rustible?

**A:**
```bash
cd rustible
git pull
cargo build --release
cargo install --path . --force
```

---

## Configuration

### Q: Where does Rustible look for configuration files?

**A:** Rustible searches in this order (first found wins):
1. Path specified by `--config` or `$RUSTIBLE_CONFIG`
2. `./rustible.yml` (current directory)
3. `~/.rustible/config.yml`
4. `/etc/rustible/rustible.yml`

### Q: What environment variables does Rustible support?

**A:**

| Variable | Description |
|----------|-------------|
| `RUSTIBLE_INVENTORY` | Default inventory file path |
| `RUSTIBLE_CONFIG` | Default configuration file path |
| `RUSTIBLE_VAULT_PASSWORD` | Vault password (use with caution) |
| `RUSTIBLE_VAULT_PASSWORD_FILE` | Path to vault password file |
| `RUSTIBLE_HOME` | Rustible home directory |
| `RUSTIBLE_REMOTE_USER` | Default remote SSH user |
| `RUSTIBLE_SSH_KEY` | Default SSH private key path |
| `RUSTIBLE_NO_COLOR` | Disable colored output |
| `NO_COLOR` | Standard no-color environment variable |
| `EDITOR` | Editor for vault edit/create commands |

### Q: How do I create a configuration file?

**A:** Create `rustible.yml` in your project root:

```yaml
defaults:
  inventory: ./inventory/hosts.yml
  forks: 10
  timeout: 30
  remote_user: deploy

ssh:
  private_key: ~/.ssh/id_ed25519
  common_args: "-o StrictHostKeyChecking=accept-new"

privilege_escalation:
  become: true
  become_method: sudo
  become_user: root
```

### Q: How do I disable host key checking?

**A:**

**Warning:** Only do this in trusted networks!

```yaml
# In rustible.toml or rustible.yml
[ssh]
host_key_checking = false
```

Or via SSH config:
```bash
# ~/.ssh/config
Host *
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
```

### Q: How do I configure SSH connection pooling?

**A:** Connection pooling is automatic in Rustible (unlike Ansible). No configuration needed! This provides an 11x speedup on SSH operations.

For advanced tuning:
```toml
[ssh]
control_master = true
control_persist = 300  # Keep connections for 5 minutes
pipelining = true
```

### Q: What inventory formats are supported?

**A:** Rustible supports:
- **INI format** - Traditional Ansible format
- **YAML format** - Structured inventory
- **JSON format** - Compatible with dynamic inventory output
- **Dynamic inventory scripts** - Executable scripts returning JSON

### Q: How do I specify an inventory file?

**A:**
```bash
# Command line
rustible run playbook.yml -i inventory.yml

# Multiple inventories
rustible run playbook.yml -i inventory.yml -i extra_hosts.ini

# Environment variable
export RUSTIBLE_INVENTORY=inventory.yml
rustible run playbook.yml
```

---

## Modules

### Q: What modules are available in Rustible?

**A:** Rustible includes 60+ native modules:

**Package Management:**
- `apt` - Debian/Ubuntu packages
- `yum` - RHEL/CentOS packages
- `dnf` - Fedora/RHEL 8+ packages
- `pip` - Python packages
- `package` - Generic package management

**File Operations:**
- `file` - File/directory management
- `copy` - Copy files to remote hosts
- `template` - Jinja2 template rendering
- `lineinfile` - Manage lines in files
- `blockinfile` - Manage blocks in files
- `stat` - Get file statistics

**Commands:**
- `command` - Execute commands (no shell)
- `shell` - Execute shell commands

**System:**
- `service` - Service management
- `user` - User account management
- `group` - Group management

**Utilities:**
- `debug` - Print debug messages
- `set_fact` - Set host facts
- `assert` - Assert conditions
- `fail` - Fail with message
- `include_vars` - Load variables
- `git` - Git repository management

### Q: Why is my module not found?

**A:** Common causes:

1. **Typo in module name:**
   ```yaml
   # Wrong
   - commnad: echo hello

   # Correct
   - command: echo hello
   ```

2. **Module not yet implemented:**
   Some Ansible modules aren't available. Use `command`/`shell` as workaround:
   ```yaml
   # Instead of uri module
   - command: curl -s https://api.example.com
   ```

3. **FQCN format issues:**
   Rustible supports short names:
   ```yaml
   # Both work
   - ansible.builtin.command: echo hello
   - command: echo hello
   ```

### Q: Template rendering fails with undefined variable

**A:**
```
ERROR: Template rendering failed: undefined variable 'missing_var'
```

**Solutions:**

1. **Provide default value:**
   ```jinja2
   server_name = {{ server_name | default('localhost') }}
   ```

2. **Check if variable exists:**
   ```jinja2
   {% if database is defined %}
   db_host = {{ database.host }}
   {% endif %}
   ```

3. **Debug variable values:**
   ```yaml
   - debug:
       var: database
   ```

### Q: Command module can't find my script

**A:**
```
FAILED! => {"rc": 127, "msg": "command not found: my_script.sh"}
```

**Solutions:**

1. **Use absolute path:**
   ```yaml
   - command: /usr/local/bin/my_script.sh
   ```

2. **Use shell module for PATH resolution:**
   ```yaml
   - shell: my_script.sh
   ```

3. **Set PATH explicitly:**
   ```yaml
   - command: my_script.sh
     environment:
       PATH: "/usr/local/bin:{{ ansible_env.PATH }}"
   ```

### Q: What's the difference between command and shell modules?

**A:**

| Aspect | command | shell |
|--------|---------|-------|
| Speed | Faster | Slower (shell parsing) |
| Pipes | Not supported | Supported |
| Redirects | Not supported | Supported |
| Glob patterns | Not supported | Supported |
| Environment vars | Literal only | Expanded |

Use `command` when possible, `shell` only when you need shell features.

### Q: How do I make modules idempotent?

**A:** Use `creates` or `removes` parameters:

```yaml
# Only runs if file doesn't exist
- command:
    cmd: /opt/app/init-db.sh
    creates: /var/lib/app/db.sqlite

# Only runs if file exists
- command:
    cmd: /opt/app/cleanup.sh
    removes: /tmp/cleanup_needed
```

### Q: File module gives permission denied

**A:**
```
FAILED! => {"msg": "Permission denied: /etc/important.conf"}
```

**Solutions:**

1. **Add become:**
   ```yaml
   - name: Edit system file
     file:
       path: /etc/important.conf
       mode: '0644'
     become: true
   ```

2. **Specify become user:**
   ```yaml
   - name: As specific user
     file:
       path: /opt/app/config
       mode: '0644'
     become: true
     become_user: app_user
   ```

---

## Connection Issues

### Q: SSH connection fails with "Connection refused"

**A:**
```
ERROR: Connection failed to 'host1': Connection refused
```

**Troubleshooting steps:**

1. **Verify SSH service is running:**
   ```bash
   ssh host1  # Test manually
   sudo systemctl status sshd  # On target
   ```

2. **Check port number:**
   ```yaml
   host1:
     ansible_host: 192.168.1.10
     ansible_port: 2222  # If using non-standard port
   ```

3. **Check firewall:**
   ```bash
   # On target
   sudo firewall-cmd --add-service=ssh --permanent
   sudo firewall-cmd --reload
   ```

4. **Verify network connectivity:**
   ```bash
   ping host1
   telnet host1 22
   ```

### Q: Authentication fails with "Permission denied"

**A:**
```
ERROR: Authentication failed: Permission denied (publickey,password)
```

**Troubleshooting steps:**

1. **Check SSH key permissions:**
   ```bash
   chmod 600 ~/.ssh/id_rsa
   chmod 700 ~/.ssh
   ```

2. **Verify correct user:**
   ```yaml
   host1:
     ansible_user: correct_username
   ```

3. **Test SSH manually with verbose:**
   ```bash
   ssh -vvv user@host1
   ```

4. **Copy SSH key to target:**
   ```bash
   ssh-copy-id user@host1
   ```

5. **Check authorized_keys on target:**
   ```bash
   cat ~/.ssh/authorized_keys  # On target
   ```

### Q: Host key verification failed

**A:**
```
ERROR: Host key verification failed
```

**Solutions:**

1. **Accept the host key manually:**
   ```bash
   ssh user@host1  # Accept when prompted
   ```

2. **Clear old host key (if host was reinstalled):**
   ```bash
   ssh-keygen -R host1
   ```

3. **Disable checking (development only!):**
   ```yaml
   [ssh]
   host_key_checking = false
   ```

### Q: Connection times out

**A:**
```
ERROR: Connection timed out to 'host1'
```

**Solutions:**

1. **Increase timeout:**
   ```bash
   rustible run playbook.yml --timeout 60
   ```

2. **Check network:**
   ```bash
   ping host1
   traceroute host1
   ```

3. **Verify DNS resolution:**
   ```bash
   nslookup host1
   dig host1
   ```

4. **Check for network segmentation:**
   - VPN connected?
   - Correct subnet?
   - Jump host needed?

### Q: How do I use a jump host / bastion?

**A:** Configure in inventory:

```yaml
host1:
  ansible_host: 10.0.0.5
  ansible_ssh_common_args: '-o ProxyJump=bastion.example.com'
```

Or in SSH config:
```
Host private-*
    ProxyJump bastion.example.com
```

### Q: SSH tests are skipped in E2E testing

**A:**
```
test_e2e_modules_ssh is skipped
```

Ensure environment variables are set:
```bash
export RUSTIBLE_TEST_SSH_ENABLED=1
export RUSTIBLE_TEST_SSH_USER=testuser
export RUSTIBLE_TEST_SSH_HOSTS="192.168.178.141,192.168.178.142"
export RUSTIBLE_TEST_SSH_KEY=$HOME/.ssh/id_ed25519

cargo test --test modules_e2e_tests test_e2e_modules_ssh
```

---

## Performance

### Q: Why is Rustible faster than Ansible?

**A:** Rustible provides significant performance improvements:

| Optimization | Impact |
|-------------|--------|
| Connection pooling | 11x faster SSH operations |
| Compiled modules | 40-70x faster module load |
| Native async | 2x better parallel scaling |
| Zero-copy architecture | Lower memory, faster parsing |
| No Python interpreter | No startup overhead |

### Q: How do I increase parallelism?

**A:**
```bash
# Increase forks (parallel hosts)
rustible run playbook.yml -f 20

# For large fleets
rustible run playbook.yml -f 50
```

### Q: How do I use the free execution strategy?

**A:** Use when tasks don't depend on each other:

```yaml
- hosts: webservers
  strategy: free
  tasks:
    - name: Independent task 1
      command: /opt/app/update.sh
    - name: Independent task 2
      command: /opt/app/reload-config.sh
```

This can provide 2x speedup over the default linear strategy.

### Q: How do I disable fact gathering for speed?

**A:** Fact gathering adds 3-5 seconds per host:

```yaml
- hosts: all
  gather_facts: false
  tasks:
    - name: Quick operation
      command: echo "hello"
```

Or gather only specific facts:
```yaml
- hosts: all
  gather_facts: true
  gather_subset:
    - network
    - hardware
```

### Q: How do I batch file operations for speed?

**A:**
```yaml
# Slow: Many small transfers
- copy:
    src: "{{ item }}"
    dest: /opt/app/
  loop: "{{ files }}"

# Fast: Single archive transfer
- name: Deploy archive
  unarchive:
    src: files.tar.gz
    dest: /opt/app/
```

### Q: What are typical performance benchmarks?

**A:**

| Scenario | Ansible | Rustible | Speedup |
|----------|---------|----------|---------|
| Simple playbook (10 hosts) | 8.2s | 1.4s | **5.9x** |
| File copy (100 files) | 45.3s | 8.1s | **5.6x** |
| Template rendering | 12.1s | 2.3s | **5.3x** |
| Large fleet (50 hosts) | 2m 45s | 15s | **11x** |

### Q: How do I optimize for high-latency networks?

**A:**
```toml
[ssh]
timeout = 60
pipelining = true
control_persist = 600
```

And reduce forks to avoid congestion:
```bash
rustible run playbook.yml -f 10
```

### Q: How do I reduce memory usage?

**A:**

1. **Limit forks:**
   ```bash
   rustible run playbook.yml -f 10
   ```

2. **Split large playbooks:**
   ```bash
   rustible run part1.yml
   rustible run part2.yml
   ```

3. **Use limits for subset operations:**
   ```bash
   rustible run playbook.yml --limit 'webservers[0:99]'
   ```

4. **Avoid storing large data in variables:**
   ```yaml
   # Bad: Stores entire file in memory
   - slurp:
       src: /var/log/huge.log
     register: log_contents

   # Good: Process on remote
   - shell: tail -1000 /var/log/huge.log | grep ERROR
     register: errors
   ```

---

## Migration from Ansible

### Q: Is Rustible compatible with Ansible playbooks?

**A:** Yes! Rustible uses the same YAML syntax and supports most core Ansible features:
- Playbooks, plays, tasks, handlers
- Variables, facts, registered variables
- Conditionals (`when`), loops (`loop`, `with_items`)
- Error handling (`ignore_errors`, `block/rescue/always`)
- Vault encryption (different format, see below)
- Privilege escalation (`become`)

### Q: What features are available with feature flags?

**A:** Many additional modules are available via feature flags:

| Feature Flag | Modules | Status |
|--------------|---------|--------|
| `docker` | docker_container, docker_image, docker_network, docker_volume, docker_compose | Stable |
| `kubernetes` | k8s_deployment, k8s_service, k8s_configmap, k8s_secret, k8s_namespace | Stable |
| `aws` | aws_ec2, aws_s3, aws_iam_role, aws_iam_policy | Stable |
| `hpc` | slurm_*, pbs_*, nvidia_gpu, nvidia_driver, cuda, rdma_stack, lustre_*, beegfs_*, lmod, mpi, munge | Stable |
| `lsf` | lsf_queue, lsf_host, lsf_policy | Stable |
| `identity` | kerberos, sssd_config, sssd_domain | Stable |
| `bare_metal` | pxe_profile, pxe_host, warewulf_node, warewulf_image | Stable |
| `redfish` | redfish_power, redfish_info, ipmi_power, ipmi_boot | Stable |
| `database` | postgresql_*, mysql_* | Stable |
| `winrm` | win_copy, win_feature, win_service, win_package, win_user | Beta |
| `azure` | azure_vm | Experimental |
| `gcp` | gcp_compute | Experimental |

**Network device modules** (ios_config, eos_config, junos_config, nxos_config) are always available.

Build with features:
```bash
cargo build --release --features "docker,kubernetes,aws"
```

See the [Compatibility Matrix](compatibility/ansible.md) for full details.

### Q: How do I migrate my Ansible playbooks?

**A:** Step-by-step migration:

1. **Test with check mode:**
   ```bash
   rustible run playbook.yml -i inventory.yml --check
   ```

2. **Use plan mode to preview:**
   ```bash
   rustible run playbook.yml -i inventory.yml --plan
   ```

3. **Start with single host:**
   ```bash
   rustible run playbook.yml --limit test-host
   ```

4. **Gradual rollout:**
   ```bash
   rustible run playbook.yml --limit staging
   rustible run playbook.yml  # Full deployment
   ```

### Q: What about my vault-encrypted files?

**A:** Rustible uses a different vault format (`$RUSTIBLE_VAULT`). Re-encrypt:

```bash
# Decrypt with Ansible
ansible-vault decrypt secrets.yml

# Re-encrypt with Rustible
rustible vault encrypt secrets.yml
```

### Q: What are the CLI command equivalents?

**A:**

| Ansible Command | Rustible Equivalent |
|-----------------|---------------------|
| `ansible-playbook playbook.yml` | `rustible run playbook.yml` |
| `ansible-playbook -i inventory.yml` | `rustible run -i inventory.yml` |
| `ansible-playbook --check` | `rustible run --check` |
| `ansible-playbook --diff` | `rustible run --diff` |
| `ansible-playbook --tags deploy` | `rustible run --tags deploy` |
| `ansible-playbook --limit web1` | `rustible run --limit web1` |
| `ansible-playbook -e "var=value"` | `rustible run -e "var=value"` |
| `ansible-playbook --become` | `rustible run --become` |
| `ansible-vault encrypt` | `rustible vault encrypt` |
| `ansible-vault decrypt` | `rustible vault decrypt` |

### Q: What new features does Rustible offer?

**A:**

1. **Plan mode** (Terraform-style preview):
   ```bash
   rustible run playbook.yml --plan
   ```

2. **Automatic connection pooling** - No configuration needed

3. **Native Rust modules** - No Python on remote hosts

4. **Lower memory usage** - 3.7x less than Ansible

### Q: Module not found after migration

**A:**
```
ERROR: Module not found: ansible.builtin.uri
```

Replace with command/shell equivalent:

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

---

## Debugging Decision Trees

### Playbook Won't Run

```
START: Playbook won't run
  |
  v
Is there a YAML syntax error?
  |-- YES --> Check indentation (use spaces, not tabs)
  |           Check for special characters (quote them)
  |           Run: rustible validate playbook.yml
  |
  v
Is the playbook file found?
  |-- NO --> Check file path and permissions
  |          Use absolute path
  |
  v
Is inventory specified?
  |-- NO --> Add: -i inventory.yml
  |          Or set: RUSTIBLE_INVENTORY
  |
  v
Are hosts matched?
  |-- NO --> Check host pattern in play
  |          Run: rustible list-hosts -i inventory.yml
  |
  v
Check verbose output: rustible run playbook.yml -vvv
```

### SSH Connection Failures

```
START: SSH connection fails
  |
  v
Can you SSH manually?
  |-- NO --> Check: ssh user@host
  |          |
  |          v
  |          Is SSH service running on target?
  |          |-- NO --> Start: sudo systemctl start sshd
  |          |
  |          v
  |          Is firewall blocking?
  |          |-- YES --> Open port 22
  |          |
  |          v
  |          Wrong port?
  |          |-- YES --> Set ansible_port in inventory
  |
  v
Is authentication failing?
  |-- YES --> Check SSH key permissions (600)
  |           Check ansible_user in inventory
  |           Copy key: ssh-copy-id user@host
  |
  v
Is host key verification failing?
  |-- YES --> ssh-keygen -R hostname
  |           Or accept key: ssh user@host
  |
  v
Connection timing out?
  |-- YES --> Increase: --timeout 60
  |           Check network: ping host
  |           Check DNS: nslookup host
```

### Task Failures

```
START: Task fails
  |
  v
Is it a module not found error?
  |-- YES --> Check spelling
  |           Check if module is supported
  |           Use command/shell as fallback
  |
  v
Is it a permission error?
  |-- YES --> Add: become: true
  |           Check: become_user
  |
  v
Is it a template error?
  |-- YES --> Check variable is defined
  |           Use default filter: {{ var | default('value') }}
  |           Debug: add debug task to print vars
  |
  v
Is it a command failure?
  |-- YES --> Use absolute paths
  |           Check command exists on target
  |           Use shell module if pipes needed
  |
  v
Run with verbose: rustible run playbook.yml -vvv
Check mode first: rustible run playbook.yml --check
```

### Performance Issues

```
START: Playbook runs slowly
  |
  v
Is fact gathering enabled?
  |-- YES and not needed --> gather_facts: false
  |
  v
Are you using many forks?
  |-- NO --> Increase: -f 20 or -f 50
  |
  v
Using linear strategy?
  |-- YES and tasks independent --> strategy: free
  |
  v
Many small file transfers?
  |-- YES --> Batch into archives
  |
  v
Using shell where command works?
  |-- YES --> Switch to command module
  |
  v
Running unnecessary tasks?
  |-- YES --> Use tags: --tags deploy
  |           Skip: --skip-tags slow
  |
  v
Check module types (native faster than remote command)
Profile: time rustible run playbook.yml
```

### Idempotency Issues

```
START: Second run still shows changes
  |
  v
Is the task inherently non-idempotent?
  |-- YES (command/shell) --> Add creates: or removes:
  |                           Use changed_when: false if no change expected
  |
  v
Is there timestamp-based detection?
  |-- YES --> Mode or owner changing each time?
  |           Check template rendering produces same output
  |
  v
Debug with verbose: rustible run playbook.yml -vvv
Compare diffs: rustible run playbook.yml --diff
```

### Variable Issues

```
START: Variable has wrong value or is undefined
  |
  v
Is variable defined anywhere?
  |-- NO --> Define in play vars, inventory, or extra-vars
  |
  v
Using correct precedence? (highest to lowest)
  1. Extra vars (-e)
  2. Task vars
  3. Role vars
  4. Play vars
  5. Host vars
  6. Group vars
  7. Role defaults
  |
  v
Is it a boolean issue?
  |-- YES --> Use: when: my_var | bool
  |           Check string "false" vs boolean false
  |
  v
Debug:
  - debug:
      var: hostvars[inventory_hostname]
```

---

## Additional Resources

- **Quick Start Guide:** [docs/guides/quick-start.md](guides/quick-start.md)
- **Troubleshooting Guide:** [docs/guides/troubleshooting.md](guides/troubleshooting.md)
- **Migration Guide:** [docs/guides/migration-from-ansible.md](guides/migration-from-ansible.md)
- **Performance Tuning:** [docs/guides/performance-tuning.md](guides/performance-tuning.md)
- **CLI Reference:** [docs/guides/cli-reference.md](guides/cli-reference.md)
- **Module Reference:** [docs/reference/modules.md](reference/modules.md)
- **GitHub Issues:** https://github.com/rustible/rustible/issues

---

## Getting More Help

If you can't resolve your issue:

1. **Check the documentation**: Review the guides above
2. **Run with verbose output**: `rustible run playbook.yml -vvv`
3. **Use check mode**: `rustible run playbook.yml --check`
4. **Use plan mode**: `rustible run playbook.yml --plan`
5. **Search existing issues**: [GitHub Issues](https://github.com/rustible/rustible/issues)
6. **Open a new issue** with:
   - Rustible version (`rustible --version`)
   - OS and version
   - Minimal playbook to reproduce
   - Full error output with `-vvv`
   - Expected vs actual behavior
