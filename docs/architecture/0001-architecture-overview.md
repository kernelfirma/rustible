---
summary: Architecture Decision Record documenting the core design layers, async execution model, and Ansible compatibility strategy.
read_when: You want to understand the rationale behind Rustible's architectural decisions.
---

# ADR-0001: Architecture Overview

## Status

Accepted

## Context

Rustible is designed as a modern, Rust-native configuration management and automation tool that serves as an alternative to Ansible. The architecture must support:

1. High-performance parallel execution across multiple hosts
2. Async-first design for efficient I/O operations
3. Type-safe operations with compile-time guarantees
4. Compatibility with existing Ansible playbooks and inventory formats
5. Extensibility through modules, callbacks, and connection backends

## Decision

### Core Architecture Layers

```
+------------------------------------------------------------------+
|                         CLI Interface                             |
|              (clap-based, async-compatible main)                  |
+------------------------------------------------------------------+
                                |
                                v
+------------------------------------------------------------------+
|                      Playbook Engine                              |
|        (Async executor, strategy pattern, handler mgmt)           |
+------------------------------------------------------------------+
                                |
        +---------------+-------+-------+----------------+
        |               |               |                |
        v               v               v                v
+-------------+  +-------------+  +-------------+  +-------------+
|  Inventory  |  |   Module    |  |  Template   |  |  Callback   |
|   Manager   |  |  Registry   |  |   Engine    |  |   System    |
+-------------+  +-------------+  +-------------+  +-------------+
        |               |               |
        +---------------+---------------+
                        |
                        v
+------------------------------------------------------------------+
|                    Connection Manager                             |
|         (SSH, Local, Docker connection backends)                  |
+------------------------------------------------------------------+
                        |
                        v
+------------------------------------------------------------------+
|                      Target Hosts                                 |
+------------------------------------------------------------------+
```

### Key Architectural Components

#### 1. Executor Engine

The executor implements multiple execution strategies:

- **Linear Strategy**: All hosts complete a task before moving to the next
- **Free Strategy**: Each host proceeds independently at maximum speed
- **Host-Pinned Strategy**: Dedicated workers per host for optimal cache locality

```rust
pub trait ExecutionStrategy: Send + Sync {
    async fn execute(&self, tasks: Vec<Task>, hosts: Vec<Host>) -> Vec<ExecutionResult>;
}
```

#### 2. Module System

Modules are the units of work that perform actions on target hosts:

```rust
#[async_trait]
pub trait Module: Send + Sync + Debug {
    fn name(&self) -> &'static str;
    async fn execute(&self, args: &Value, ctx: &ExecutionContext) -> Result<ModuleResult>;
    fn parallelization_hint(&self) -> ParallelizationHint { ParallelizationHint::Safe }
}
```

Parallelization hints allow modules to declare their concurrency safety:
- `Safe`: Can run in parallel on any host
- `HostLocal`: Can run in parallel, but only one instance per host
- `Serial`: Must run serially across all hosts

#### 3. Connection Layer

Abstract connection interface supporting multiple backends:

```rust
#[async_trait]
pub trait Connection: Send + Sync {
    async fn execute(&self, command: &str) -> Result<CommandResult>;
    async fn upload(&self, local: &Path, remote: &Path) -> Result<()>;
    async fn download(&self, remote: &Path, local: &Path) -> Result<()>;
    async fn stat(&self, path: &Path) -> Result<FileStat>;
}
```

Supported backends:
- **russh**: Pure Rust SSH implementation (default)
- **ssh2**: LibSSH2-based SSH (optional, via feature flag)
- **local**: Direct local execution
- **docker**: Container-based execution

#### 4. Callback System

Event-driven callback system for output and integrations:

```rust
#[async_trait]
pub trait ExecutionCallback: Send + Sync + Debug {
    async fn on_playbook_start(&self, playbook: &str);
    async fn on_task_complete(&self, result: &ExecutionResult);
    async fn on_playbook_complete(&self);
    // ... additional event handlers
}
```

#### 5. Inventory System

Flexible inventory with multiple source formats:

- YAML inventory files
- INI inventory files (Ansible-compatible)
- JSON inventory
- Dynamic inventory scripts
- Programmatic construction

Features:
- Host pattern matching with glob, regex, and set operations
- Group hierarchy with inheritance
- Variable precedence handling

### Async Runtime

Rustible uses Tokio as its async runtime:

- All I/O operations are async
- Connection pooling for SSH connections
- Bounded concurrency with semaphores
- Graceful shutdown handling

### Error Handling

Comprehensive error types with:

- Structured error variants for each subsystem
- Rich error context (file, line, task, host)
- Actionable error hints and suggestions
- Exit codes aligned with Ansible conventions

## Consequences

### Positive

1. **Performance**: Parallel execution and async I/O provide significant speedups
2. **Type Safety**: Compile-time guarantees prevent many runtime errors
3. **Memory Safety**: Rust's ownership model prevents memory-related bugs
4. **Extensibility**: Trait-based design allows custom modules, callbacks, and connections
5. **Compatibility**: YAML/INI parsing maintains Ansible compatibility

### Negative

1. **Learning Curve**: Rust requires developers to understand ownership and borrowing
2. **Compilation Time**: Rust compilation is slower than interpreted languages
3. **Binary Size**: Static linking produces larger binaries
4. **Limited Python Module Support**: Cannot directly run Ansible Python modules

### Mitigations

- Provide comprehensive documentation and examples
- Use incremental compilation and workspace caching
- Consider dynamic linking for reduced binary size
- Implement command/shell modules as bridge to external scripts

## References

- Ansible Architecture: https://docs.ansible.com/ansible/latest/dev_guide/overview_architecture.html
- Tokio Runtime: https://tokio.rs/
- Rust Async Book: https://rust-lang.github.io/async-book/
