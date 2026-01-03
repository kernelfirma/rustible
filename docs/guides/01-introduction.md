---
summary: Overview of Rustible, a high-performance Ansible alternative written in Rust, covering architecture, key features, and performance benefits.
read_when: You're new to Rustible and want to understand what it is and why to use it.
---

# Chapter 1: Introduction to Rustible

## What is Rustible?

Rustible is a modern, high-performance configuration management and automation tool written in Rust. It is designed as a drop-in replacement for Ansible, offering significant performance improvements while maintaining full compatibility with Ansible playbook syntax.

### Why Rustible?

Traditional configuration management tools like Ansible are powerful but can be slow, especially when managing large fleets of servers. Rustible addresses these performance challenges while keeping the familiar YAML playbook syntax that teams already know.

**Key Benefits:**

| Benefit | Description |
|---------|-------------|
| **5-11x Faster** | Compiled Rust binary with async execution |
| **Same Syntax** | Drop-in compatible with Ansible playbooks |
| **Lower Memory** | 3.7x less memory usage than Ansible |
| **No Python Required** | Pure Rust binary, no interpreter overhead |
| **Connection Pooling** | 11x faster SSH operations |
| **Type Safe** | Catch errors at parse time, not runtime |

### Performance Comparison

```
Benchmark: 50-host deployment, 20 tasks each

Ansible 2.15:    2m 45s  |  890 MB memory
Rustible v0.1:   15s     |  240 MB memory
                 -----
Improvement:     11x faster
```

## Architecture Overview

Rustible is built on modern Rust architecture principles:

```
+-------------------------------------------------------------------+
|                         CLI Layer                                   |
|  +---------------+  +---------------+  +------------------------+  |
|  | Argument      |  | Configuration |  | Output                 |  |
|  | Parser (clap) |  | Loader        |  | Formatter              |  |
|  +---------------+  +---------------+  +------------------------+  |
+-------------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------------+
|                      Execution Engine                               |
|  +---------------+  +---------------+  +------------------------+  |
|  | Playbook      |  | Task          |  | Strategy               |  |
|  | Executor      |  | Executor      |  | Manager                |  |
|  +---------------+  +---------------+  +------------------------+  |
+-------------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------------+
|                     Connection Layer                                |
|  +-----------+  +-----------+  +-----------+  +---------------+    |
|  | SSH       |  | Local     |  | Docker    |  | Kubernetes    |    |
|  | (russh)   |  |           |  | (bollard) |  | (planned)     |    |
|  +-----------+  +-----------+  +-----------+  +---------------+    |
+-------------------------------------------------------------------+
```

### Core Components

1. **Playbook Parser**: Parses YAML playbooks into strongly-typed Rust structures
2. **Inventory System**: Manages hosts and groups from multiple sources
3. **Module System**: Extensible module architecture for task execution
4. **Connection Layer**: SSH, local, and Docker connection handlers
5. **Template Engine**: Jinja2-compatible templating with MiniJinja
6. **Vault**: AES-256-GCM encryption for sensitive data

## Key Features

### Ansible Compatibility

Rustible supports the core Ansible functionality you use daily:

- **Playbook syntax**: Plays, tasks, handlers, blocks
- **Inventory formats**: YAML, INI, JSON, dynamic scripts
- **Variables**: Full precedence support, facts, registered variables
- **Conditionals**: `when`, `failed_when`, `changed_when`
- **Loops**: `loop`, `with_items`, `with_dict`
- **Error handling**: `ignore_errors`, `block/rescue/always`
- **Vault encryption**: AES-256-GCM with Argon2 key derivation
- **Privilege escalation**: `become`, `become_user`, `become_method`

### Native Modules

Rustible includes 21+ native Rust modules:

| Category | Modules |
|----------|---------|
| Package Management | `package`, `apt`, `yum`, `dnf`, `pip` |
| File Operations | `file`, `copy`, `template`, `lineinfile`, `blockinfile`, `stat` |
| System | `service`, `user`, `group` |
| Commands | `command`, `shell` |
| Utilities | `debug`, `set_fact`, `assert`, `include_vars`, `git` |
| Flow Control | `include_tasks`, `pause`, `wait_for`, `fail` |

### Execution Strategies

Control how tasks execute across hosts:

| Strategy | Behavior |
|----------|----------|
| `linear` | Execute task on all hosts before next task |
| `free` | Hosts run independently, no synchronization |
| `host_pinned` | Dedicated worker per host |

### Connection Types

Multiple connection backends:

| Type | Use Case |
|------|----------|
| SSH (russh) | Remote Linux/Unix hosts (default) |
| Local | Localhost execution |
| Docker | Container execution |
| Kubernetes | Pod execution (planned) |

## Getting Started

Ready to get started? Here's your path:

1. **Install Rustible**: See the [Quick Start Guide](../quick-start.md)
2. **Write Your First Playbook**: [Chapter 2: Playbooks](02-playbooks.md)
3. **Set Up Inventory**: [Chapter 3: Inventory](03-inventory.md)
4. **Learn Best Practices**: [Best Practices Guide](best-practices.md)

## Comparison with Ansible

### What's the Same

- YAML playbook syntax
- Inventory formats
- Variable precedence
- Module parameters
- Jinja2 templating
- Vault encryption

### What's Different

| Aspect | Ansible | Rustible |
|--------|---------|----------|
| Language | Python | Rust |
| Startup time | ~1-2 seconds | ~50ms |
| Module execution | Python scripts | Native Rust |
| Connection reuse | ControlMaster | Built-in pooling |
| Memory usage | Higher | 3.7x lower |
| Plan mode | N/A | `--plan` flag |

### Migration Path

For teams currently using Ansible:

1. **Test compatibility**: Run existing playbooks with `--check`
2. **Verify modules**: Ensure all used modules are supported
3. **Gradual rollout**: Start with non-critical systems
4. **Performance testing**: Benchmark improvement
5. **Full migration**: Switch production workloads

See the [Migration Guide](../migration-from-ansible.md) for detailed steps.

## Next Steps

Continue to [Chapter 2: Playbooks](02-playbooks.md) to learn about playbook structure and execution.
