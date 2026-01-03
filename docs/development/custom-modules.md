---
summary: Guide to implementing custom modules using the Module trait, including parameter handling, check mode, diff output, and registration.
read_when: You need to extend Rustible with new modules for custom automation tasks.
---

# Creating Custom Modules

This guide explains how to create custom modules for Rustible. Modules are the workhorses of Rustible, performing actual work on target systems like package management, file operations, and command execution.

## Overview

A module in Rustible is a struct that implements the `Module` trait. Each module:
- Has a unique name (e.g., `"debug"`, `"copy"`, `"apt"`)
- Defines what parameters it accepts
- Executes logic based on those parameters
- Returns a `ModuleOutput` indicating success/failure and whether changes were made

## Module Trait

The core `Module` trait is defined in `src/modules/mod.rs`:

```rust
pub trait Module: Send + Sync {
    /// Returns the name of the module (e.g., "copy", "template")
    fn name(&self) -> &'static str;

    /// Returns a description of what the module does
    fn description(&self) -> &'static str;

    /// Returns the classification of this module for execution optimization
    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    /// Returns parallelization hints for the executor
    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
    }

    /// Execute the module with the given parameters
    fn execute(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput>;

    /// Check what would change without making changes (check mode/dry run)
    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        // Default: calls execute with check_mode=true
        let check_context = ModuleContext { check_mode: true, ..context.clone() };
        self.execute(params, &check_context)
    }

    /// Generate a diff of what would change
    fn diff(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        Ok(None) // Default: no diff
    }

    /// Validate the parameters before execution
    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        Ok(()) // Default: no validation
    }

    /// Returns the list of required parameters
    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    /// Returns optional parameters with their default values
    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        HashMap::new()
    }
}
```

## Module Classifications

Modules are classified by their execution characteristics to enable intelligent parallelization:

| Classification | Description | Examples |
|----------------|-------------|----------|
| `LocalLogic` | Runs entirely on the control node, no remote execution | `debug`, `set_fact`, `assert` |
| `NativeTransport` | Uses native Rust SSH/SFTP operations | `copy`, `template`, `file` |
| `RemoteCommand` | Executes commands on remote host (default) | `command`, `shell`, `service` |
| `PythonFallback` | Falls back to Ansible Python modules | Ansible module compatibility |

## Parallelization Hints

Hints help the executor determine safe concurrency levels:

| Hint | Description | Use Case |
|------|-------------|----------|
| `FullyParallel` | Safe to run on all hosts simultaneously (default) | Most modules |
| `HostExclusive` | Only one task per host | Package managers (apt, yum) |
| `RateLimited { requests_per_second }` | Rate-limited operations | Cloud API calls |
| `GlobalExclusive` | Only one instance across entire inventory | Cluster-wide changes |

## Creating a Simple Module

Here's a complete example of a custom module:

```rust
// src/modules/my_module.rs

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput,
    ModuleParams, ModuleResult, ParallelizationHint, ParamExt,
};
use std::collections::HashMap;

/// My custom module that echoes a message
pub struct MyEchoModule;

impl Module for MyEchoModule {
    fn name(&self) -> &'static str {
        "my_echo"
    }

    fn description(&self) -> &'static str {
        "Echoes a message back to the user"
    }

    fn classification(&self) -> ModuleClassification {
        // This module runs locally, no remote execution needed
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Can run in parallel across all hosts
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        &["msg"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate that msg is a non-empty string
        let msg = params.get_string_required("msg")?;
        if msg.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "msg cannot be empty".to_string()
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // In check mode, just report what we would do
        if context.check_mode {
            return Ok(ModuleOutput::ok("Would echo message"));
        }

        // Get the message parameter
        let msg = params.get_string_required("msg")?;

        // Get optional prefix
        let prefix = params.get_string("prefix")?.unwrap_or_default();

        // Format the message
        let formatted = if prefix.is_empty() {
            msg
        } else {
            format!("{}: {}", prefix, msg)
        };

        // Return success (this module never "changes" anything)
        let mut output = ModuleOutput::ok(formatted.clone());
        output.data.insert("message".to_string(), serde_json::json!(formatted));

        Ok(output)
    }
}
```

## Module Context

The `ModuleContext` provides execution context to modules:

```rust
pub struct ModuleContext {
    /// Whether to run in check mode (dry run)
    pub check_mode: bool,

    /// Whether to show diffs
    pub diff_mode: bool,

    /// Variables available to the module
    pub vars: HashMap<String, serde_json::Value>,

    /// Facts about the target system
    pub facts: HashMap<String, serde_json::Value>,

    /// Working directory for the module
    pub work_dir: Option<String>,

    /// Whether running with elevated privileges
    pub become: bool,

    /// Method for privilege escalation
    pub become_method: Option<String>,

    /// User to become
    pub become_user: Option<String>,

    /// Connection to use for remote operations
    pub connection: Option<Arc<dyn Connection + Send + Sync>>,
}
```

## Module Output

The `ModuleOutput` struct communicates results back to the executor:

```rust
// Success, no changes made
ModuleOutput::ok("Task completed successfully")

// Success, changes were made
ModuleOutput::changed("File was modified")

// Failure
ModuleOutput::failed("Could not connect to server")

// Skipped (e.g., condition not met)
ModuleOutput::skipped("Skipped due to when condition")

// With additional data
let mut output = ModuleOutput::changed("Package installed");
output.data.insert("version".to_string(), serde_json::json!("1.2.3"));

// With diff information
output = output.with_diff(Diff::new("old content", "new content"));

// With command output (for shell/command modules)
output = output.with_command_output(
    Some(stdout),
    Some(stderr),
    Some(exit_code),
);
```

## Parameter Extraction Helpers

The `ParamExt` trait provides convenient parameter extraction:

```rust
// Get optional string parameter
let msg = params.get_string("msg")?; // Returns Option<String>

// Get required string parameter (returns error if missing)
let path = params.get_string_required("path")?;

// Get boolean parameter
let force = params.get_bool("force")?; // Returns Option<bool>

// Get boolean with default
let backup = params.get_bool_or("backup", true);

// Get integer parameter
let count = params.get_i64("count")?; // Returns Option<i64>

// Get unsigned integer (supports octal for modes like "0755")
let mode = params.get_u32("mode")?; // Returns Option<u32>

// Get array parameter
let packages = params.get_vec_string("name")?; // Returns Option<Vec<String>>
```

## Remote Command Execution

For modules that need to execute commands on remote hosts:

```rust
fn execute(
    &self,
    params: &ModuleParams,
    context: &ModuleContext,
) -> ModuleResult<ModuleOutput> {
    // Get connection from context
    let connection = context.connection.as_ref()
        .ok_or_else(|| ModuleError::ExecutionFailed(
            "No connection available".to_string()
        ))?;

    // Execute a command
    let result = tokio::runtime::Handle::current()
        .block_on(async {
            connection.execute("ls -la /tmp", None).await
        })
        .map_err(|e| ModuleError::ExecutionFailed(e.to_string()))?;

    if result.success {
        Ok(ModuleOutput::ok(result.stdout))
    } else {
        Err(ModuleError::CommandFailed {
            code: result.exit_code,
            message: result.stderr,
        })
    }
}
```

## File Transfer Operations

For modules that transfer files:

```rust
use std::path::Path;

// Upload a file
connection.upload(
    Path::new("/local/source.txt"),
    Path::new("/remote/dest.txt"),
    Some(TransferOptions::new().with_mode(0o644)),
).await?;

// Upload content directly
connection.upload_content(
    b"file contents",
    Path::new("/remote/file.txt"),
    Some(TransferOptions::new().with_mode(0o600)),
).await?;

// Download a file
connection.download(
    Path::new("/remote/file.txt"),
    Path::new("/local/file.txt"),
).await?;

// Check if path exists
let exists = connection.path_exists(Path::new("/remote/file")).await?;

// Get file stats
let stat = connection.stat(Path::new("/remote/file")).await?;
```

## Registering Custom Modules

Register your module with the `ModuleRegistry`:

```rust
use std::sync::Arc;
use rustible::modules::ModuleRegistry;

// Create registry with built-in modules
let mut registry = ModuleRegistry::with_builtins();

// Register your custom module
registry.register(Arc::new(MyEchoModule));

// Use the module
let result = registry.execute(
    "my_echo",
    &params,
    &context,
)?;
```

## Validation Best Practices

### Security Validation

Always validate user input, especially for parameters that affect commands:

```rust
use rustible::modules::{validate_package_name, validate_path_param};

fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
    // Validate package names (prevents command injection)
    if let Some(name) = params.get_string("name")? {
        validate_package_name(&name)?;
    }

    // Validate paths
    if let Some(path) = params.get_string("creates")? {
        validate_path_param(&path, "creates")?;
    }

    Ok(())
}
```

### Check Mode Support

Always support check mode for idempotent behavior:

```rust
fn execute(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
    // Determine current state
    let current_state = get_current_state()?;
    let desired_state = get_desired_state(params)?;

    // Check if changes are needed
    if current_state == desired_state {
        return Ok(ModuleOutput::ok("Already in desired state"));
    }

    // In check mode, report what would change
    if context.check_mode {
        return Ok(ModuleOutput::changed("Would update state")
            .with_diff(Diff::new(
                format!("{:?}", current_state),
                format!("{:?}", desired_state),
            )));
    }

    // Make the actual change
    apply_desired_state(&desired_state)?;

    Ok(ModuleOutput::changed("State updated"))
}
```

## Testing Modules

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_name() {
        let module = MyEchoModule;
        assert_eq!(module.name(), "my_echo");
    }

    #[test]
    fn test_execute_with_msg() {
        let module = MyEchoModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("msg".to_string(), serde_json::json!("Hello!"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Hello!"));
    }

    #[test]
    fn test_validation_rejects_empty_msg() {
        let module = MyEchoModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("msg".to_string(), serde_json::json!(""));

        let result = module.validate_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_mode() {
        let module = MyEchoModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("msg".to_string(), serde_json::json!("Test"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Would"));
    }
}
```

## Built-in Module Examples

Study these modules in the `src/modules/` directory for reference:

| Module | Type | Description |
|--------|------|-------------|
| `debug.rs` | LocalLogic | Simple module for printing debug messages |
| `copy.rs` | NativeTransport | File copy operations using SFTP |
| `command.rs` | RemoteCommand | Execute commands on remote hosts |
| `apt.rs` | RemoteCommand | Package management with host-exclusive locking |
| `file.rs` | NativeTransport | File and directory management |
| `template.rs` | NativeTransport | Jinja2-compatible template rendering |

## Summary

1. Implement the `Module` trait with at least `name()`, `description()`, and `execute()`
2. Choose appropriate `classification()` and `parallelization_hint()` for your module
3. Validate parameters properly in `validate_params()`
4. Support check mode by respecting `context.check_mode`
5. Return appropriate `ModuleOutput` variants (ok/changed/failed/skipped)
6. Register your module with `ModuleRegistry`
7. Write comprehensive tests
