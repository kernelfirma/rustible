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
- Alpha readiness risks and active ownership are tracked in `docs/ALPHA_READINESS_ISSUES.md`.
- Alpha release execution tasks are tracked in `docs/ALPHA_LAUNCH_CHECKLIST.md`.
- Beta promotion criteria and sign-off requirements are defined in `docs/BETA_ENTRY_CRITERIA.md`.
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
      --step               Step through tasks interactively
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
- **Podman**: Rootless container execution
- **AWS SSM**: EC2 Session Manager connection
- **WinRM**: Windows remote management (Beta, feature flag)

### Built-in Modules

**Core modules**: command, shell, raw, script, debug, set_fact, assert, fail, meta, pause, wait_for, stat

**File operations**: copy, template, file, lineinfile, blockinfile, archive, unarchive, synchronize, get_url

**Package management**: package, apt, yum, dnf, pip

**System administration**: service, systemd_unit, user, group, cron, hostname, sysctl

**Security**: authorized_key, known_hosts, ufw, firewalld

**Cloud modules** (feature flags): aws_ec2_instance, aws_s3, azure_vm, gcp_compute_instance

**Docker**: docker_container, docker_image, docker_network, docker_volume, docker_compose

**Kubernetes** (feature flag): k8s_namespace, k8s_deployment, k8s_service, k8s_configmap, k8s_secret

**Database** (feature flag): postgresql_db, postgresql_user, mysql_db, mysql_user, and more

**Network devices** (feature flag): ios_config, eos_config, junos_config, nxos_config

**HPC** (feature flag): Comprehensive HPC cluster management including:
  - *Scheduler*: slurm_config, slurm_ops, slurm_node, slurm_partition, slurm_account, slurm_qos, slurm_job, slurm_queue, slurm_info, slurmrestd, pbs_job, pbs_queue, pbs_server, scheduler_orchestration, partition_policy, lsf_queue, lsf_host, lsf_policy
  - *GPU*: nvidia_gpu, nvidia_driver, cuda
  - *InfiniBand/OFED*: rdma_stack, opensm, ib_partition, ib_diagnostics, ipoib
  - *Parallel Filesystems*: lustre_client, lustre_mount, lustre_ost, beegfs_client
  - *Identity*: kerberos, sssd_config, sssd_domain
  - *Bare-Metal Provisioning*: pxe_profile, pxe_host, warewulf_node, warewulf_image
  - *BMC/IPMI*: redfish_power, redfish_info, ipmi_power, ipmi_boot
  - *Infrastructure*: hpc_baseline, lmod, mpi, munge, hpc_nfs, hpc_facts, hpc_healthcheck, hpc_toolchain, hpc_discovery, hpc_power, boot_profile, image_pipeline

**Windows** (feature flag): win_copy, win_feature, win_service, win_package, win_user

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
| `hpc` | HPC modules (Slurm, GPU, OFED) |
| `slurm` | Slurm workload manager modules |
| `gpu` | GPU management modules (NVIDIA) |
| `ofed` | InfiniBand/RDMA/OFED support |
| `parallel_fs` | Parallel filesystem clients (Lustre, BeeGFS) |
| `pbs` | PBS Pro workload manager modules |
| `lsf` | IBM Spectrum LSF workload manager modules |
| `identity` | Kerberos and SSSD identity management |
| `bare_metal` | PXE boot and Warewulf bare-metal provisioning |
| `distributed` | Distributed execution support |
| `api` | REST API server |
| `provisioning` | Infrastructure provisioning (requires AWS) |
| `full` | All core features enabled |
| `full-cloud` | All features plus all cloud providers |
| `full-aws` | All features plus AWS |
| `full-hpc` | All features plus full HPC (hpc, pbs, ofed, parallel_fs, redfish, vsphere, identity, bare_metal) |
| `pure-rust` | Minimal pure Rust build (no C deps) |
| `ssh2-backend` | Legacy SSH via libssh2 (C dependency) |
| `startup-warmup` | Background warmup of lazy components |
| `openstack` | OpenStack cloud provider (experimental) |
| `redfish` | Bare-metal BMC management via Redfish/IPMI |
| `database` | Database modules (PostgreSQL, MySQL) |
| `winrm` | Windows Remote Management (Beta) |
| `azure` | Azure cloud modules (experimental) |
| `gcp` | GCP cloud modules (experimental) |
| `reqwest` | HTTP client backend (experimental) |
| `experimental` | Required opt-in for experimental features (azure, gcp, reqwest, openstack) |

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

## Testing

### SSH Integration Tests (Ignored)

Russh integration tests are ignored by default and require real SSH hosts.
You can export the variables manually or source the helper script:

```bash
source scripts/ssh-test-env.sh
cargo test test_russh_ -- --ignored
```

Environment variables:

- `RUSTIBLE_SSH_TEST_HOST` / `RUSTIBLE_SSH_TEST_PORT` / `RUSTIBLE_SSH_TEST_USER` / `RUSTIBLE_SSH_TEST_KEY`
- `RUSTIBLE_SSH_TEST_JUMP_HOST` / `RUSTIBLE_SSH_TEST_JUMP_PORT` / `RUSTIBLE_SSH_TEST_JUMP_USER` / `RUSTIBLE_SSH_TEST_JUMP_KEY`
- `RUSTIBLE_SSH_TEST_JUMP2_HOST` / `RUSTIBLE_SSH_TEST_JUMP2_PORT` / `RUSTIBLE_SSH_TEST_JUMP2_USER` / `RUSTIBLE_SSH_TEST_JUMP2_KEY` (multi-hop test)

### Homelab Playbook Tests (Ignored)

Run the homelab smoke playbook against real hosts:

```bash
export RUSTIBLE_HOMELAB_TESTS=1
export RUSTIBLE_HOMELAB_INVENTORY=tests/fixtures/homelab_inventory.yml
cargo test --test homelab_playbook_tests -- --ignored
```

## Contributing

All contributions are welcome.

See `CONTRIBUTING.md` for guidelines and `CODE_OF_CONDUCT.md` for community expectations.
For security issues, see `SECURITY.md`.

## License

MIT
