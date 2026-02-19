---
summary: Curated reading paths for different audiences including new users, module developers, contributors, and operations teams.
read_when: You're new to Rustible documentation and want a guided learning path for your specific role.
---

# Rustible Documentation Index

Welcome to the Rustible documentation. This index provides curated reading paths tailored to your role and goals.

## Reading Paths

### New Users

Start here if you're new to Rustible or configuration management.

| Order | Document | Purpose |
|-------|----------|---------|
| 1 | [Quick Start Guide](guides/quick-start.md) | Install Rustible and run your first playbook |
| 2 | [Introduction](guides/01-introduction.md) | Understand Rustible's architecture and advantages |
| 3 | [Playbooks](guides/02-playbooks.md) | Learn playbook structure, tasks, and handlers |
| 4 | [CLI Reference](guides/cli-reference.md) | Master command-line options and workflows |
| 5 | [Best Practices](guides/best-practices.md) | Write maintainable, secure automation |

**Coming from Ansible?** See the [Migration Guide](guides/migration-from-ansible.md) for compatibility details and performance comparisons.

---

### Module Developers

For those extending Rustible with custom modules.

| Order | Document | Purpose |
|-------|----------|---------|
| 1 | [Module Reference](reference/modules.md) | Understand built-in module patterns |
| 2 | [ADR-0002: Module System Design](architecture/0002-module-system-design.md) | Learn the Module trait and execution tiers |
| 3 | [Creating Custom Modules](development/custom-modules.md) | Implement your own modules |
| 4 | [Variables Reference](reference/variables.md) | Handle variable precedence and scoping |
| 5 | [Inventory Reference](reference/inventory.md) | Work with hosts and dynamic inventory |

**Individual Module References:**
- [file](reference/modules/file.md) - File and directory management
- [copy](reference/modules/copy.md) - Copy files to remote hosts
- [template](reference/modules/template.md) - Jinja2 template deployment
- [command](reference/modules/command.md) - Command execution
- [service](reference/modules/service.md) - Service management
- [package](reference/modules/package.md) - Package management
- [docker_container](reference/modules/docker_container.md) - Docker container management
- [k8s_deployment](reference/modules/k8s_deployment.md) - Kubernetes deployments
- [aws_ec2](reference/modules/aws_ec2.md) - AWS EC2 instances
- [user](reference/modules/user.md) - User account management
- [group](reference/modules/group.md) - Group management
- [cron](reference/modules/cron.md) - Cron job management

---

### Contributors

For those contributing to the Rustible codebase.

| Order | Document | Purpose |
|-------|----------|---------|
| 1 | [Contributing Guide](development/CONTRIBUTING.md) | Setup, standards, and PR process |
| 2 | [Architecture Overview](architecture/ARCHITECTURE.md) | Understand internal design |
| 3 | [ADR-0001: Architecture Overview](architecture/0001-architecture-overview.md) | Design rationale and decisions |
| 4 | [ADR-0002: Module System Design](architecture/0002-module-system-design.md) | Module implementation patterns |
| 5 | [Security Audit Report](security/SECURITY_AUDIT_REPORT.md) | Security considerations |

**Plugin Development:**
- [Creating Custom Modules](development/custom-modules.md)
- [Creating Connection Plugins](development/connection-plugins.md)
- [Creating Callback Plugins](development/callback-plugins.md)

---

### Operations & Troubleshooting

For operators running Rustible in production environments.

| Order | Document | Purpose |
|-------|----------|---------|
| 1 | [Troubleshooting Guide](guides/troubleshooting.md) | Diagnose and fix common issues |
| 2 | [Performance Tuning](guides/performance-tuning.md) | Optimize execution speed and resources |
| 3 | [CLI Reference](guides/cli-reference.md) | Command options and environment variables |
| 4 | [Container Guide](guides/container-guide.md) | Docker and Kubernetes deployment |
| 5 | [Callbacks Reference](reference/callbacks.md) | Output formatting and integrations |

**Advanced Execution:**
- [Plan Mode](guides/plan_mode.md) - Dry-run execution planning
- [Serial Execution](guides/serial-execution.md) - Rolling updates and canary deployments
- [Delegation](guides/delegation.md) - Running tasks on different hosts
- [Task Inclusion](guides/include_tasks.md) - Modular playbook organization

---

### HPC & Infrastructure

For teams managing HPC clusters and high-performance infrastructure.

| Order | Document | Purpose |
|-------|----------|---------|
| 1 | [HPC Quick Start](hpc/) | Getting started with HPC modules |
| 2 | [Quick Start Guide](guides/quick-start.md) | Feature flags for HPC features |
| 3 | [Performance Tuning](guides/performance-tuning.md) | Optimize for large-scale deployments |

---

## Quick Reference

| Topic | Document |
|-------|----------|
| All modules | [Module Reference](reference/modules.md) |
| Variable precedence | [Variables Reference](reference/variables.md) |
| Inventory formats | [Inventory Reference](reference/inventory.md) |
| Inventory & Variables | [Inventory & Variables Guide](guides/03-inventory.md), [Variables Deep Dive](guides/04-variables.md) |
| Callback plugins | [Callbacks Reference](reference/callbacks.md) |
| CLI options | [CLI Reference](guides/cli-reference.md) |
| Changelog | [CHANGELOG](CHANGELOG.md) |
| Roadmap | [ROADMAP](ROADMAP.md) |

---

## Documentation Structure

```
docs/
├── guides/           # User tutorials and how-to guides
├── reference/        # API and module documentation
│   └── modules/      # Individual module references
├── architecture/     # Design decisions and ADRs
├── development/      # Contributor guides
├── security/         # Security documentation
├── hpc/              # HPC cluster management guides
├── compatibility/    # Ansible compatibility matrices
├── logging/          # Logging and output configuration
└── performance/      # Performance tuning and benchmarks
```

---

## Getting Help

- **Issues:** Report bugs or request features on GitHub
- **Troubleshooting:** See the [Troubleshooting Guide](guides/troubleshooting.md)
- **Security:** See [Security Audit Report](security/SECURITY_AUDIT_REPORT.md)
