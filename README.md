# Rustible

A fast, async configuration management tool written in Rust. Drop-in replacement for Ansible with 5x+ performance improvement.

## Why Rustible?

- **Fast**: Compiled Rust binary with connection pooling (5-11x faster than Ansible)
- **Compatible**: Same YAML playbook syntax as Ansible
- **Safe**: Type-checked configuration, better error messages
- **Parallel**: Concurrent execution by default

## Quick Start

```bash
# Install
git clone https://github.com/rustible/rustible.git
cd rustible && cargo install --path .

# Run a playbook
rustible run playbook.yml -i inventory.yml
```

### Example Playbook

```yaml
- name: Configure web servers
  hosts: webservers
  become: true

  tasks:
    - name: Install nginx
      package:
        name: nginx
        state: present

    - name: Start nginx
      service:
        name: nginx
        state: started
        enabled: true
```

## CLI Usage

```bash
rustible run <PLAYBOOK> [OPTIONS]

Options:
  -i, --inventory <FILE>   Inventory file
  -l, --limit <PATTERN>    Limit to specific hosts
  -e, --extra-vars <VARS>  Extra variables
  -c, --check              Dry run
  -v, --verbose            Increase verbosity
  -f, --forks <N>          Parallel processes [default: 10]
```

### Other Commands

```bash
rustible check <PLAYBOOK>     # Syntax check
rustible vault encrypt <FILE> # Encrypt file
rustible vault decrypt <FILE> # Decrypt file
rustible galaxy install <PKG> # Install collection/role
rustible init <PATH>          # Create new project
```

## Features

| Feature | Status |
|---------|--------|
| Playbook syntax | Full Ansible compatibility |
| Inventory | YAML, INI, JSON, dynamic scripts |
| Templating | Jinja2 via minijinja |
| Vault | AES-256-GCM encryption |
| Roles | Full support |
| Handlers | Including `listen` |
| Python modules | Fallback via AnsiballZ |

### Connections

- **SSH** (default): Pure Rust via russh
- **Local**: Direct execution
- **Docker**: Container execution
- **Kubernetes**: Pod execution (feature flag)

### Built-in Modules

**Core**: command, shell, debug, set_fact, assert, pause, wait_for, stat

**Files**: copy, template, file, lineinfile, blockinfile, archive, unarchive

**Packages**: package, apt, yum, dnf, pip

**System**: service, systemd_unit, user, group, cron, hostname, sysctl

**Security**: authorized_key, known_hosts, ufw, firewalld

**Cloud** (feature flags): aws_ec2_instance, aws_s3, azure_vm, gcp_compute_instance

Any module not listed falls back to Ansible's Python execution.

## Configuration

Config file: `rustible.toml`, `~/.config/rustible/config.toml`, or `/etc/rustible/rustible.toml`

```toml
[defaults]
inventory = "inventory.yml"
forks = 10
timeout = 30

[ssh]
host_key_checking = true
pipelining = true
```

## Feature Flags

```bash
cargo build --features docker,kubernetes,aws
```

| Flag | Description |
|------|-------------|
| `russh` | Pure Rust SSH (default) |
| `docker` | Docker modules |
| `kubernetes` | K8s modules |
| `aws` | AWS cloud modules |

## Performance

| Operation | Ansible | Rustible |
|-----------|---------|----------|
| 10 hosts, simple playbook | 8.2s | 1.4s |
| 100 file copies | 45.3s | 8.1s |
| Template rendering | 12.1s | 2.3s |

## Documentation

- [User Guide](docs/guides/README.md)
- [API Reference](docs/reference/README.md)
- [Architecture](docs/architecture/ARCHITECTURE.md)

## Contributing

```bash
cargo build
cargo test
cargo clippy --all-features
```

## License

MIT
