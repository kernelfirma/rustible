# Rustible Feature Roadmap

A comprehensive roadmap for Rustible development, outlining current features, planned enhancements, and community-driven priorities.

## Table of Contents

- [Vision Statement](#vision-statement)
- [v0.1 - MVP (Current)](#v01---mvp-current)
- [v0.2 - Planned Features](#v02---planned-features)
- [v1.0 - Production Ready](#v10---production-ready)
- [Community Feature Requests](#community-feature-requests)
- [Contributor Guidelines](#contributor-guidelines)
- [Performance Benchmarks](#performance-benchmarks)

---

## Vision Statement

**Rustible** aims to combine Ansible's accessibility with Nix-like guarantees and Rust's performance. Our goal is to provide a modern, async-first configuration management tool that offers:

- **Drop-in Ansible compatibility** - Existing playbooks work with minimal changes
- **Superior performance** - 5-11x faster than Ansible through native Rust and connection pooling
- **Type safety** - Catch configuration errors at parse time, not runtime
- **Reproducibility** - State tracking, drift detection, and lockfiles for guaranteed outcomes

---

## v0.1 - MVP (Current)

**Status**: Released (December 2025)
**Focus**: Core functionality with Ansible compatibility

### Core Execution Engine

| Feature | Status | Description |
|---------|--------|-------------|
| Playbook parsing | :white_check_mark: Complete | YAML playbooks with plays, tasks, handlers |
| Inventory management | :white_check_mark: Complete | YAML, INI, JSON formats; dynamic scripts |
| Task execution | :white_check_mark: Complete | Sequential and parallel task execution |
| Variable resolution | :white_check_mark: Complete | Full variable precedence chain |
| Template engine | :white_check_mark: Complete | Jinja2-compatible via MiniJinja |
| Handlers | :white_check_mark: Complete | Including `listen` for multiple triggers |
| Blocks | :white_check_mark: Complete | `block`, `rescue`, `always` error handling |
| Roles | :white_check_mark: Complete | Full role structure support |
| Tags | :white_check_mark: Complete | Task filtering with `--tags`/`--skip-tags` |
| Fact gathering | :white_check_mark: Complete | gather_facts/setup module support |

### Connection Types

| Connection | Status | Description |
|------------|--------|-------------|
| SSH (russh) | :white_check_mark: Complete | Pure Rust SSH with connection pooling (11x faster) |
| SSH (ssh2) | :white_check_mark: Complete | libssh2 wrapper (legacy option) |
| Local | :white_check_mark: Complete | Direct localhost execution |
| Docker | :white_check_mark: Complete | Container execution via Bollard |
| Kubernetes | :white_check_mark: Complete | Pod execution via kube-rs (feature-gated) |
| WinRM | :test_tube: Beta | Windows remote management (feature-gated) |
| Podman | :white_check_mark: Complete | Rootless container execution via Podman |
| AWS SSM | :white_check_mark: Complete | EC2 Session Manager connection |
| Jump Host | :white_check_mark: Complete | Bastion/jump host support |
| SSH Agent | :white_check_mark: Complete | SSH agent forwarding |

### Native Modules (60+ total)

**File Operations:**
- :white_check_mark: `file` - File/directory management
- :white_check_mark: `copy` - Copy files to remote
- :white_check_mark: `template` - Jinja2 template rendering
- :white_check_mark: `lineinfile` - Line manipulation in files
- :white_check_mark: `blockinfile` - Block manipulation in files
- :white_check_mark: `stat` - File statistics
- :white_check_mark: `archive` - Archive creation
- :white_check_mark: `unarchive` - Archive extraction
- :white_check_mark: `synchronize` - rsync-based file synchronization

**Command Execution:**
- :white_check_mark: `command` - Execute commands (no shell)
- :white_check_mark: `shell` - Execute shell commands
- :white_check_mark: `raw` - Raw command execution (no Python required)
- :white_check_mark: `script` - Transfer and execute local scripts

**Package Management:**
- :white_check_mark: `package` - Generic package manager abstraction
- :white_check_mark: `apt` - Debian/Ubuntu package management
- :white_check_mark: `yum` - RHEL/CentOS package management
- :white_check_mark: `dnf` - Fedora/RHEL 8+ package management
- :white_check_mark: `pip` - Python package management

**System Management:**
- :white_check_mark: `service` - Service control (systemd/init)
- :white_check_mark: `systemd_unit` - Systemd unit file management
- :white_check_mark: `user` - User account management
- :white_check_mark: `group` - Group management
- :white_check_mark: `hostname` - Hostname configuration
- :white_check_mark: `sysctl` - Kernel parameter management
- :white_check_mark: `mount` - Filesystem mounting
- :white_check_mark: `cron` - Cron job management
- :white_check_mark: `timezone` - Timezone configuration
- :white_check_mark: `selinux` - SELinux configuration

**Security Modules:**
- :white_check_mark: `authorized_key` - SSH authorized keys management
- :white_check_mark: `known_hosts` - SSH known hosts management
- :white_check_mark: `ufw` - UFW firewall management
- :white_check_mark: `firewalld` - Firewalld management

**Utility Modules:**
- :white_check_mark: `debug` - Debug message output
- :white_check_mark: `set_fact` - Set host facts
- :white_check_mark: `assert` - Condition assertions
- :white_check_mark: `include_vars` - Include variable files
- :white_check_mark: `facts` / `gather_facts` - Fact gathering
- :white_check_mark: `uri` - HTTP/HTTPS requests
- :white_check_mark: `get_url` - Download files from HTTP/HTTPS/FTP
- :white_check_mark: `fail` - Fail with custom message
- :white_check_mark: `meta` - Meta actions (flush handlers, end play, etc.)
- :white_check_mark: `git` - Git repository management
- :white_check_mark: `wait_for` - Wait for conditions (port, file, regex)
- :white_check_mark: `pause` - Pause execution with prompt

**Docker Modules:**
- :white_check_mark: `docker_container` - Container management
- :white_check_mark: `docker_image` - Image management
- :white_check_mark: `docker_network` - Network management
- :white_check_mark: `docker_volume` - Volume management
- :white_check_mark: `docker_compose` - Docker Compose support

**Kubernetes Modules:**
- :white_check_mark: `k8s_deployment` - Deployment management
- :white_check_mark: `k8s_service` - Service management
- :white_check_mark: `k8s_configmap` - ConfigMap management
- :white_check_mark: `k8s_secret` - Secret management
- :white_check_mark: `k8s_namespace` - Namespace management

**Cloud Modules:**
- :white_check_mark: `aws_ec2` - AWS EC2 instances
- :white_check_mark: `aws_s3` - AWS S3 storage
- :white_check_mark: `azure_vm` - Azure virtual machines
- :white_check_mark: `gcp_compute` - GCP Compute Engine

**Network Device Modules:**
- :white_check_mark: `ios_config` - Cisco IOS configuration
- :white_check_mark: `eos_config` - Arista EOS configuration
- :white_check_mark: `junos_config` - Juniper Junos configuration
- :white_check_mark: `nxos_config` - Cisco NX-OS configuration

**Windows Modules (feature-gated):**
- :white_check_mark: `win_copy` - Windows file copy
- :white_check_mark: `win_feature` - Windows features
- :white_check_mark: `win_service` - Windows services
- :white_check_mark: `win_package` - Windows packages
- :white_check_mark: `win_user` - Windows user management

**Fallback:**
- :white_check_mark: `python` - Ansible Python module fallback (FQCN support)

### Execution Strategies

| Strategy | Status | Description |
|----------|--------|-------------|
| Linear | :white_check_mark: Complete | Task-by-task across all hosts (Ansible default) |
| Free | :white_check_mark: Complete | Maximum parallelism, hosts run independently |
| HostPinned | :white_check_mark: Complete | Dedicated worker per host (connection affinity) |
| Debug | :white_check_mark: Complete | Interactive step-through debugging (`--step`) |

### Advanced Execution Features

| Feature | Status | Description |
|---------|--------|-------------|
| Parallelization hints | :white_check_mark: Complete | Module-level concurrency control |
| Batch processing | :white_check_mark: Complete | Reduces loop overhead (87x improvement) |
| Work stealing scheduler | :white_check_mark: Complete | Optimal load balancing |
| Async task execution | :white_check_mark: Complete | Timeout and polling support |
| Throttle control | :white_check_mark: Complete | Rate limits and concurrency control |
| Fact pipeline | :white_check_mark: Complete | Optimized fact gathering |
| Condition evaluation | :white_check_mark: Complete | when/changed_when/failed_when |
| Dependency graph | :white_check_mark: Complete | DAG-based task ordering |

### Callback Plugins (30+ total)

**Core Output:**
- :white_check_mark: `default` - Standard Ansible-like output with colors
- :white_check_mark: `minimal` - Shows only failures and final recap
- :white_check_mark: `null` - Silent callback (no output)
- :white_check_mark: `oneline` - Compact single-line output
- :white_check_mark: `summary` - Summary-only output at playbook end

**Visual:**
- :white_check_mark: `progress` - Visual progress bars
- :white_check_mark: `diff` - Before/after diffs for changed files
- :white_check_mark: `dense` - Compact output for large inventories
- :white_check_mark: `tree` - Hierarchical directory structure

**Timing & Analysis:**
- :white_check_mark: `timer` - Execution timing with summary
- :white_check_mark: `context` - Task context with variables/conditions
- :white_check_mark: `stats` - Comprehensive statistics collection
- :white_check_mark: `counter` - Task counting and tracking
- :white_check_mark: `profile_tasks` - Task profiling with recommendations

**Filtering:**
- :white_check_mark: `skippy` - Minimizes skipped task output
- :white_check_mark: `selective` - Filters output by status/host/patterns
- :white_check_mark: `actionable` - Only changed/failed tasks
- :white_check_mark: `full_skip` - Detailed skip analysis

**Logging:**
- :white_check_mark: `json` - JSON-formatted output
- :white_check_mark: `yaml` - YAML-formatted output
- :white_check_mark: `logfile` - File-based logging
- :white_check_mark: `syslog` - System syslog integration
- :white_check_mark: `debug` - Debug output for development

**Integration:**
- :white_check_mark: `notification` - External notifications (Slack, Email, Webhooks)
- :white_check_mark: `junit` - JUnit XML output for CI/CD
- :white_check_mark: `mail` - Email notifications
- :white_check_mark: `forked` - Parallel execution output
- :white_check_mark: `slack` - Slack notifications
- :white_check_mark: `logstash` - Logstash integration
- :white_check_mark: `splunk` - Splunk integration

### Lookup Plugins

| Plugin | Status | Description |
|--------|--------|-------------|
| `file` | :white_check_mark: Complete | Read file contents |
| `env` | :white_check_mark: Complete | Environment variables |
| `password` | :white_check_mark: Complete | Generate random passwords |
| `pipe` | :white_check_mark: Complete | Execute commands and capture output |
| `url` | :white_check_mark: Complete | Fetch content from HTTP/HTTPS URLs |
| `items` | :white_check_mark: Complete | Iterate over lists of items |
| `template` | :white_check_mark: Complete | Render Jinja2 template strings |

### Dynamic Inventory Plugins

| Plugin | Status | Description |
|--------|--------|-------------|
| `aws_ec2` | :white_check_mark: Complete | AWS EC2 instances |
| `azure` | :white_check_mark: Complete | Azure virtual machines |
| `gcp` | :white_check_mark: Complete | GCP Compute Engine |
| Constructed inventory | :white_check_mark: Complete | Dynamic group construction |
| Inventory caching | :white_check_mark: Complete | Cache inventory results |

### Galaxy Support

| Feature | Status | Description |
|---------|--------|-------------|
| Collection installation | :white_check_mark: Complete | Install from Galaxy or tarballs |
| Role installation | :white_check_mark: Complete | Install from Galaxy or Git |
| Requirements parsing | :white_check_mark: Complete | Process requirements.yml |
| Local caching | :white_check_mark: Complete | Cache with integrity verification |
| Offline mode | :white_check_mark: Complete | Fall back to cached artifacts |
| Version constraints | :white_check_mark: Complete | Semantic version matching |

### Security Features

| Feature | Status | Description |
|---------|--------|-------------|
| Vault encryption | :white_check_mark: Complete | AES-256-GCM with Argon2id key derivation |
| Privilege escalation | :white_check_mark: Complete | `become`, `become_user`, `become_method` |
| SSH key authentication | :white_check_mark: Complete | RSA, Ed25519, ECDSA keys |
| Host key checking | :white_check_mark: Complete | Configurable strict/accept modes |
| Circuit breaker | :white_check_mark: Complete | Connection resilience pattern |
| Network security | :white_check_mark: Complete | Host key pinning, TLS validation |
| Audit logging | :white_check_mark: Complete | Encryption audit trail |

### Connection Resilience

| Feature | Status | Description |
|---------|--------|-------------|
| Connection pooling | :white_check_mark: Complete | Reuse SSH connections |
| Retry logic | :white_check_mark: Complete | Exponential backoff |
| Health monitoring | :white_check_mark: Complete | Connection health checks |
| Graceful degradation | :white_check_mark: Complete | Degradation strategies |

### Performance Metrics (v0.1)

- **SSH Connection Pooling**: 11x faster than Ansible
- **Simple playbook (10 hosts)**: 5.8x improvement
- **File copy (100 files)**: 5.6x improvement
- **Template rendering**: 5.3x improvement
- **Loop operations**: 87x improvement with batch processing
- **Test coverage**: ~3,246 tests (99.1% pass rate)

---

## v0.2 - Planned Features

**Target**: Q1 2026
**Focus**: Stability, execution preview, enhanced testing

### Critical Path (Stabilization)

| Task | Priority | Description |
|------|----------|-------------|
| Fix remaining tests | Critical | Achieve 100% test pass rate |
| Ansible boolean compat | High | Done: handle y/n/t/f and string boolean variants consistently |
| Block parsing | High | Done: treat null block/rescue/always as empty lists |
| Python/FQCN edge cases | High | Done: normalize ansible.builtin/ansible.legacy module names |
| CLI edge cases | Medium | Done: support comma-separated tags and richer extra-vars parsing |

### Execution Plan Preview

```bash
rustible plan playbook.yml -i inventory.yml

# Output
Execution Plan:
  web1.example.com:
    + [package] Install nginx (will install)
    ~ [template] Configure nginx.conf (will modify)
    - [file] Remove old config (will delete)

  web2.example.com:
    . [package] Install nginx (already installed)
    ~ [template] Configure nginx.conf (will modify)

Apply this plan? [y/N]
```

### Schema Validation

Parse-time validation of module arguments:

```rust
fn schema(&self) -> JsonSchema;

// Validate before execution
module.schema().validate(&task.args)?;
```

### State Manifest Foundation

```
~/.rustible/state/
  web1.example.com.json
  web2.example.com.json
  db1.example.com.json
```

### New Modules (Completed)

| Module | Status | Description |
|--------|--------|-------------|
| `fail` | :white_check_mark: Complete | Fail with custom message |
| `meta` | :white_check_mark: Complete | Meta actions (flush handlers, etc.) |
| `raw` | :white_check_mark: Complete | Raw command execution (no Python) |
| `script` | :white_check_mark: Complete | Transfer and execute script |
| `synchronize` | :white_check_mark: Complete | rsync wrapper |
| `get_url` | :white_check_mark: Complete | Download files from HTTP/HTTPS/FTP |

---

## v1.0 - Production Ready

**Target**: Q4 2026
**Focus**: Enterprise features, Nix-like guarantees, full Ansible compatibility

### State Management

**Drift Detection:**

```bash
rustible drift-check -i inventory.yml

# Output
web1.example.com: OK (no drift detected)
web2.example.com: DRIFTED
  - /etc/nginx/nginx.conf: modified (expected: abc, actual: xyz)
  - package nginx: version mismatch (expected: 1.18, actual: 1.20)
db1.example.com: OK
```

**State Caching:**

```rust
pub struct StateCache {
    entries: HashMap<StateKey, CachedResult>,
}

pub struct StateKey {
    module: String,
    params_hash: u64,      // Hash of module parameters
    host_facts_hash: u64,  // Relevant facts only
}
```

- Hash `(module_name, params, relevant_host_facts)` before execution
- Store result with timestamp
- On re-run: compare hash, skip if unchanged
- **Target**: "Instant" re-runs for unchanged configurations

**Lockfile Support:**

```yaml
# rustible.lock
version: 1
generated: 2026-01-15T10:30:00Z
modules:
  package: { version: "1.0.0", hash: "abc123" }
templates:
  nginx.conf.j2: { hash: "789ghi" }
variables:
  nginx_port: 80
```

### Advanced Execution

**Dependency Graph Execution (DAG):**

```yaml
- name: Install database
  package: name=postgresql
  provides: database

- name: Configure database
  template: src=pg.conf.j2
  requires: database
  provides: db_config

- name: Install app  # Runs in parallel with db tasks
  package: name=myapp
  provides: app

- name: Configure app
  template: src=app.conf.j2
  requires: [db_config, app]
```

**Transactional Rollback:**

```bash
rustible run playbook.yml --checkpoint

# If something goes wrong
rustible rollback web1.example.com --to checkpoint-20260115

# List checkpoints
rustible checkpoints web1.example.com
```

### Performance Enhancements

**Pipelined SSH:**

```rust
// Current: 1 command = 1 round-trip
ssh.execute("apt update").await?;
ssh.execute("apt install nginx").await?;

// Target: Pipeline multiple commands
ssh.pipeline(&[
    "apt update",
    "apt install -y nginx",
    "systemctl enable nginx",
]).await?;
```

Expected: 2-3x improvement on top of existing 11x (total: 20-30x)

**Native Module Bindings:**

| Module | Current | Target | Expected Gain |
|--------|---------|--------|---------------|
| apt | Shell out | libapt-pkg bindings | 1.5-2x |
| systemctl | Shell out | D-Bus bindings | 2x |
| user/group | Shell out | Native /etc/passwd | 1.5x |

**Binary Agent Mode:**

```bash
# Compile small Rust binary for target
rustible agent-build --target x86_64-unknown-linux-musl

# Deploy and use persistent agent
rustible run --agent-mode playbook.yml
```

### Full Ansible Compatibility

| Feature | Status | Target |
|---------|--------|--------|
| Playbook syntax | :white_check_mark: Complete | 100% |
| Module compatibility | ~90% | 95%+ |
| Ansible Galaxy | :white_check_mark: Complete | Full support |
| Callback plugins | :white_check_mark: Complete | Native + Python support |
| Dynamic inventory | :white_check_mark: Complete | Full plugin system |
| Lookup plugins | :white_check_mark: Complete | Full support |
| Filter plugins | Partial | Full Jinja2 filters |

### Connection Enhancements

| Connection | Status | Target |
|------------|--------|--------|
| WinRM | :test_tube: Beta | Windows remote management |
| Kubernetes | :white_check_mark: Complete | Pod execution |
| Podman | :white_check_mark: Complete | Rootless container execution |
| AWS SSM | :white_check_mark: Complete | EC2 Session Manager |

---

## Community Feature Requests

We track community requests and prioritize based on demand and alignment with project goals.

### How to Submit a Feature Request

1. **GitHub Issues**: Open an issue with the `feature-request` label
2. **Discussions**: Start a discussion in GitHub Discussions
3. **Pull Requests**: Submit a PR with proposed implementation

### Current Requests (Vote with reactions!)

| Request | Votes | Status | Priority |
|---------|-------|--------|----------|
| Podman connection support | - | :white_check_mark: Complete | Medium |
| Web UI for playbook management | - | Under consideration | Low |
| [Terraform integration](architecture/terraform-integration.md) | - | :construction: In Progress ([PR #152](https://github.com/adolago/rustible/pull/152)) | Medium |
| [HashiCorp Vault + AWX/Tower](architecture/awx-vault-integration.md) | - | :construction: In Progress ([PR #153](https://github.com/adolago/rustible/pull/153)) | Medium |
| [Provider ecosystem](architecture/provider-ecosystem.md) | - | :construction: In Progress ([PR #154](https://github.com/adolago/rustible/pull/154)) | Medium |
| [Declarative resource graph](architecture/resource-graph-model.md) | - | :construction: In Progress ([PR #155](https://github.com/adolago/rustible/pull/155)) | Medium |
| [Compatibility gap plan](architecture/ansible-compat-gap.md) | - | Under consideration | Medium |
| YAML anchor/alias support | - | Investigating | Medium |
| Parallel role execution | - | Investigating | Medium |
| Database modules (MySQL/PostgreSQL) | - | :white_check_mark: Complete | High |

### Feature Request Template

```markdown
## Feature Request

**Is your feature request related to a problem?**
A clear description of the problem.

**Describe the solution you'd like**
A clear description of what you want to happen.

**Describe alternatives you've considered**
Other solutions or features you've considered.

**Use Case**
How would you use this feature?

**Additional context**
Any other context, mockups, or examples.
```

---

## Contributor Guidelines

We welcome contributions from the community! Here's how to get started.

### Getting Started

1. **Fork the repository**
   ```bash
   git clone https://github.com/YOUR_USERNAME/rustible.git
   cd rustible
   ```

2. **Set up development environment**
   ```bash
   # Install Rust (1.85+)
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

   # Build the project
   cargo build

   # Run tests
   cargo test
   ```

3. **Create a feature branch**
   ```bash
   git checkout -b feature/your-feature-name
   ```

### Development Workflow

**Code Standards:**

```bash
# Format code
cargo fmt

# Run lints
cargo clippy --all-features

# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run benchmarks
cargo bench
```

**Test Requirements:**
- All new features must have tests
- Maintain >95% code coverage for new code
- Integration tests for module changes
- Property-based tests where applicable

**Documentation:**
- Update relevant docs for API changes
- Add examples for new features
- Include rustdoc comments for public APIs

### Contribution Areas

**Good First Issues:**
- Bug fixes with clear reproduction steps
- Documentation improvements
- Test coverage improvements
- Error message enhancements

**Intermediate:**
- New module implementations
- Performance optimizations
- CLI enhancements
- Inventory plugin development

**Advanced:**
- Connection type implementations
- Execution strategy improvements
- Parser enhancements
- Security features

### Pull Request Process

1. **Create PR** with clear description
2. **Link related issues** using `Fixes #123`
3. **Ensure CI passes** - tests, lints, format
4. **Request review** from maintainers
5. **Address feedback** promptly
6. **Squash commits** before merge

### PR Template

```markdown
## Description
Brief description of changes.

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update

## Testing
- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Manual testing performed

## Checklist
- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Documentation updated
- [ ] No breaking changes (or documented)
```

### Code of Conduct

- Be respectful and inclusive
- Provide constructive feedback
- Focus on the code, not the person
- Help newcomers get started
- Credit others' contributions

### Communication Channels

- **GitHub Issues**: Bug reports, feature requests
- **GitHub Discussions**: Questions, ideas, community
- **Pull Requests**: Code contributions

---

## Performance Benchmarks

### Connection Pooling Results

| Metric | Ansible | Rustible | Improvement |
|--------|---------|----------|-------------|
| 100 tasks / 5 hosts | ~45s | ~4s | **11.25x** |
| Connection setup | Per-task | Pooled | N/A |
| Memory usage | ~200MB | ~50MB | 4x |

### Module Execution Performance

| Module | Ansible | Rustible | Improvement |
|--------|---------|----------|-------------|
| file (stat) | ~80ms | ~8ms | 10x |
| copy (small) | ~120ms | ~15ms | 8x |
| command | ~100ms | ~10ms | 10x |
| template | ~150ms | ~20ms | 7.5x |

### Scalability Testing

- **Tested**: Up to 50 concurrent hosts (Free strategy)
- **Result**: Linear scaling with forks limit
- **Connection Pool**: <10 active connections via reuse
- **Memory**: ~50MB base + ~2MB per active host

### Future Performance Targets

| Enhancement | Current | Target | Expected Gain |
|-------------|---------|--------|---------------|
| SSH pipelining | 11x | 20-30x | 2-3x |
| Native apt | 8x | 12-16x | 1.5-2x |
| State caching | N/A | "Instant" | 10-100x (unchanged) |

---

## Comparison Matrix

| Feature | Ansible | NixOS | Rustible v0.1 | Rustible v1.0 |
|---------|---------|-------|---------------|---------------|
| Speed | Slow | Fast | **11x faster** | 20-30x faster |
| Idempotency | Honor system | Guaranteed | Trait-enforced | + State tracking |
| Reproducibility | Best effort | Perfect | Basic | Lockfile-based |
| Rollback | Manual | Built-in | Not yet | Checkpoints |
| Drift detection | None | Implicit | Not yet | Explicit |
| Learning curve | Low | High | **Low** | Low |
| Existing infra | Works | Needs NixOS | **Works** | Works |

---

## Release Schedule

| Version | Target Date | Theme |
|---------|-------------|-------|
| v0.1.0 | Dec 2025 | MVP - Core functionality |
| v0.1.x | Jan 2026 | Bug fixes, stability |
| v0.2.0 | Q1 2026 | Execution preview, schema validation |
| v0.3.0 | Q2 2026 | State management, drift detection |
| v0.4.0 | Q3 2026 | Native bindings, performance |
| v1.0.0 | Q4 2026 | Production ready, full compatibility |

---

*Last updated: February 2026*

*For the latest updates, see [GitHub Releases](https://github.com/rustible/rustible/releases)*
