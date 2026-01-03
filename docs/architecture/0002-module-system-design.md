---
summary: Architecture Decision Record for the module system covering the Module trait, execution tiers, check mode, diff support, and idempotency.
read_when: You want to understand module design patterns or implement custom modules correctly.
---

# ADR-0002: Module System Design

## Status

Accepted

## Context

Modules are the fundamental units of work in Rustible. Each module performs a specific action on target hosts (installing packages, managing files, executing commands, etc.). The module system must:

1. Support idempotent operations (running twice produces the same result)
2. Enable check mode (dry-run) for all modules
3. Provide diff output showing before/after changes
4. Allow parallel execution when safe
5. Support both built-in and custom modules
6. Handle privilege escalation (become/sudo)

## Decision

### Module Trait Definition

```rust
#[async_trait]
pub trait Module: Send + Sync + Debug {
    /// Returns the module name used in playbooks
    fn name(&self) -> &'static str;

    /// Executes the module with given arguments
    async fn execute(
        &self,
        args: &serde_json::Value,
        context: &ExecutionContext,
    ) -> Result<ModuleResult>;

    /// Returns the parallelization hint for this module
    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::Safe
    }

    /// Validates arguments before execution
    fn validate_args(&self, args: &serde_json::Value) -> Result<()> {
        Ok(())
    }
}
```

### Execution Context

The context provides modules with everything needed for execution:

```rust
pub struct ExecutionContext {
    /// SSH/local connection for executing commands
    pub connection: Arc<dyn Connection>,
    /// Whether this is a check (dry-run) mode
    pub check_mode: bool,
    /// Whether to show diffs
    pub diff_mode: bool,
    /// Variables available to the module
    pub variables: Variables,
    /// Task metadata (name, when, loop, etc.)
    pub task_meta: TaskMeta,
    /// Privilege escalation settings
    pub become: BecomeConfig,
    /// Path to temporary directory on remote
    pub remote_tmp: PathBuf,
}
```

### Module Result

```rust
#[derive(Debug, Clone, Default)]
pub struct ModuleResult {
    /// Whether the module changed the target state
    pub changed: bool,
    /// Whether the execution failed
    pub failed: bool,
    /// Human-readable message
    pub msg: Option<String>,
    /// Standard output from commands
    pub stdout: Option<String>,
    /// Standard error from commands
    pub stderr: Option<String>,
    /// Exit code if applicable
    pub rc: Option<i32>,
    /// Diff showing changes made
    pub diff: Option<Diff>,
    /// Additional structured data
    pub data: serde_json::Value,
    /// Whether to skip this result in output
    pub skipped: bool,
    /// Warnings to display
    pub warnings: Vec<String>,
}
```

### Parallelization Hints

Modules declare their concurrency safety:

```rust
pub enum ParallelizationHint {
    /// Safe to run in parallel on any host
    Safe,
    /// Safe to run in parallel, but only one instance per host
    HostLocal,
    /// Must run serially across all hosts (e.g., cluster operations)
    Serial,
    /// Custom hint with explanation
    Custom(String),
}
```

### Built-in Module Categories

1. **Package Management**
   - `apt`: Debian/Ubuntu packages
   - `yum`: RHEL/CentOS packages
   - `dnf`: Fedora packages
   - `pip`: Python packages

2. **File Management**
   - `copy`: Copy files to remote
   - `file`: Manage file/directory properties
   - `template`: Deploy Jinja2 templates
   - `lineinfile`: Manage lines in files
   - `blockinfile`: Manage blocks in files

3. **Command Execution**
   - `command`: Execute commands (no shell)
   - `shell`: Execute through shell

4. **System Administration**
   - `service`: Manage system services
   - `user`: Manage user accounts
   - `group`: Manage groups

5. **Source Control**
   - `git`: Git repository management

6. **Utility**
   - `debug`: Print debug information
   - `assert`: Assert conditions
   - `fail`: Fail with message
   - `set_fact`: Set host facts

### Module Registry

```rust
pub struct ModuleRegistry {
    modules: HashMap<String, Box<dyn Module>>,
}

impl ModuleRegistry {
    /// Creates registry with all built-in modules
    pub fn with_builtins() -> Self;

    /// Registers a custom module
    pub fn register(&mut self, module: Box<dyn Module>);

    /// Executes a module by name
    pub async fn execute(
        &self,
        module_name: &str,
        args: &serde_json::Value,
        context: &ExecutionContext,
    ) -> Result<ModuleResult>;
}
```

### Idempotency Pattern

Modules follow a consistent pattern for idempotent operations:

```rust
async fn execute(&self, args: &Value, ctx: &ExecutionContext) -> Result<ModuleResult> {
    // 1. Parse and validate arguments
    let args: ModuleArgs = serde_json::from_value(args.clone())?;

    // 2. Gather current state
    let current = self.get_current_state(&args, ctx).await?;

    // 3. Determine desired state
    let desired = self.compute_desired_state(&args)?;

    // 4. Compare states
    if current == desired {
        return Ok(ModuleResult {
            changed: false,
            msg: Some("Already in desired state".to_string()),
            ..Default::default()
        });
    }

    // 5. Generate diff if requested
    let diff = if ctx.diff_mode {
        Some(self.generate_diff(&current, &desired)?)
    } else {
        None
    };

    // 6. Apply changes (skip in check mode)
    if !ctx.check_mode {
        self.apply_state(&args, &desired, ctx).await?;
    }

    Ok(ModuleResult {
        changed: true,
        msg: Some("State updated".to_string()),
        diff,
        ..Default::default()
    })
}
```

### Check Mode Support

All modules must support check mode:

```rust
// In module implementation
if ctx.check_mode {
    // Only check what would change, don't actually change it
    return Ok(ModuleResult {
        changed: would_change,
        msg: Some("Would update configuration".to_string()),
        diff: Some(predicted_diff),
        ..Default::default()
    });
}

// Actually apply changes
self.apply_changes(ctx).await?;
```

### Privilege Escalation

Modules access become configuration through context:

```rust
async fn execute(&self, args: &Value, ctx: &ExecutionContext) -> Result<ModuleResult> {
    // Execute command with privilege escalation if configured
    let command = if ctx.become.enabled {
        format!(
            "{} {} -c '{}'",
            ctx.become.method,
            ctx.become.user.as_deref().unwrap_or("root"),
            actual_command
        )
    } else {
        actual_command
    };

    ctx.connection.execute(&command).await
}
```

## Consequences

### Positive

1. **Type Safety**: Compile-time verification of module interfaces
2. **Testability**: Easy to unit test modules in isolation
3. **Parallelism**: Clear parallelization hints enable safe concurrent execution
4. **Idempotency**: Consistent pattern ensures predictable behavior
5. **Extensibility**: Simple trait implementation for custom modules

### Negative

1. **Boilerplate**: Each module requires implementing the full trait
2. **Learning Curve**: Understanding the execution context and patterns
3. **No Python Modules**: Cannot directly use Ansible Python modules

### Mitigations

- Provide module templates and generators
- Comprehensive documentation with examples
- Bridge modules (command/shell) for external scripts

## References

- Ansible Module Development: https://docs.ansible.com/ansible/latest/dev_guide/developing_modules_general.html
- Rust Async Trait: https://docs.rs/async-trait/
