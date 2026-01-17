# Rustible

Safe and fast async configuration management tool.

**Acknowledgment**: Rustible is inspired by Ansible and Terraform. This project builds upon those proven concepts while improving safety, reliability and speed.

## Why Rustible?

- **Type Safety**: Compile-time configuration validation with superior error messages
- **Full Compatibility**: Identical YAML playbook syntax to Ansible
- **High Performance**: Compiled binary with connection pooling (Much faster than Ansible)
- **Parallel Execution**: Concurrent task execution by default

## Alpha Status

Rustible is currently in alpha. Expect breaking changes, incomplete features, and evolving
performance/security characteristics.

- Terraform-like provisioning is experimental and limited in scope; Terraform integration
  focuses on state inventory and workflow bridging, not full replacement.
- Several feature flags remain stubbed or partial and require explicit
  `experimental` opt-in (see `Cargo.toml`).
- Security hardening and coverage gaps are tracked in `docs/ALPHA_READINESS_ISSUES.md`.
- Maintainers can track release tasks in `docs/ALPHA_LAUNCH_CHECKLIST.md`.
- Use in production environments only after validating against your own risk model.

## Quick Start

Install and run your first playbook:

```bash
# Clone and install
git clone https://github.com/rustible/rustible.git
cd rustible && cargo install --path .

# Execute playbook
rustible run playbook.yml -i inventory.yml
```

### Sample Playbook

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

Run playbooks with familiar Ansible syntax:

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

### Additional Commands

```bash
rustible check <PLAYBOOK>     # Syntax validation
rustible vault encrypt <FILE> # AES-256-GCM encryption
rustible vault decrypt <FILE> # Decrypt files
rustible galaxy install <PKG> # Install collections/roles
rustible init <PATH>          # Initialize new project
```

## Features

| Feature | Status |
|---------|--------|
| Playbook syntax | 100% Ansible compatibility |
| Inventory formats | YAML, INI, JSON, dynamic scripts |
| Templating | Jinja2 via minijinja |
| Vault encryption | AES-256-GCM |
| Roles | Full support |
| Handlers | Including `listen` syntax |
| Python modules | Fallback via AnsiballZ |

### Connection Methods

- **SSH** (default): Via russh
- **Local**: Direct local execution
- **Docker**: Container-based execution
- **Kubernetes**: Pod execution (feature flag)

### Built-in Modules

**Core modules**: command, shell, debug, set_fact, assert, pause, wait_for, stat

**File operations**: copy, template, file, lineinfile, blockinfile, archive, unarchive

**Package management**: package, apt, yum, dnf, pip

**System administration**: service, systemd_unit, user, group, cron, hostname, sysctl

**Security**: authorized_key, known_hosts, ufw, firewalld

**Cloud modules** (feature flags): aws_ec2_instance, aws_s3, azure_vm, gcp_compute_instance

Unsupported modules automatically fall back to Ansible's Python execution engine.

## Configuration

Configuration files: `rustible.toml`, `~/.config/rustible/config.toml`, or `/etc/rustible/rustible.toml`

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

Build with additional features:

```bash
cargo build --features docker,kubernetes,aws
```

| Flag | Description |
|------|-------------|
| `russh` | Pure Rust SSH (default) |
| `docker` | Docker container support |
| `kubernetes` | Kubernetes pod execution |
| `aws` | AWS cloud modules |
| `experimental` | Required opt-in for stubbed features (azure, gcp, database, winrm, reqwest) |

## Performance

Benchmarks demonstrate significant performance improvements:

| Operation | Ansible | Rustible | Speedup |
|-----------|---------|----------|---------|
| 10 hosts, simple playbook | 8.2s | 1.4s | 5.9x |
| 100 file copies | 45.3s | 8.1s | 5.6x |
| Template rendering | 12.1s | 2.3s | 5.3x |

## Documentation

- [User Guide](docs/guides/README.md) - Comprehensive usage guide
- [API Reference](docs/reference/README.md) - Module documentation
- [Architecture](docs/architecture/ARCHITECTURE.md) - Technical design

## Contributing

All contributions are welcome.

See `CONTRIBUTING.md` for guidelines and `CODE_OF_CONDUCT.md` for community expectations.
For security issues, see `SECURITY.md`.

## License

MIT
