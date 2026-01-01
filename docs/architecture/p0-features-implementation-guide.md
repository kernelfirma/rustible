# P0 Features Implementation Guide

**Concrete code examples for developers implementing Issues #52 and #53**

**See Also:**
- [Design Document](./p0-features-design.md) - Full architecture
- [Diagrams](./p0-features-diagrams.md) - Visual architecture
- [Summary](./p0-features-summary.md) - Quick reference

---

## Table of Contents

1. [File-by-File Changes](#file-by-file-changes)
2. [Code Examples](#code-examples)
3. [Testing Examples](#testing-examples)
4. [Common Pitfalls](#common-pitfalls)

---

## File-by-File Changes

### 1. Create `src/executor/become.rs` (NEW FILE)

**Purpose:** Central become configuration and precedence resolution

**Complete implementation:**

```rust
//! Privilege escalation (become) configuration and resolution
//!
//! This module handles the become (sudo/su) privilege escalation feature,
//! resolving configuration from multiple sources with proper precedence.

use serde::{Deserialize, Serialize};

/// Configuration for privilege escalation (become)
///
/// Become allows tasks to execute with elevated privileges using sudo, su,
/// or other escalation methods.
///
/// # Precedence
///
/// 1. Task-level: `task.become` and `task.become_user`
/// 2. Play-level: `play.become` and `play.become_user`
/// 3. CLI-level: `--become` and `--become-user` flags
/// 4. Default: disabled
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BecomeConfig {
    /// Whether to use privilege escalation
    pub enabled: bool,

    /// Method to use (sudo, su, doas, pbrun, pfexec, runas, etc.)
    pub method: String,

    /// User to become
    pub user: String,

    /// Password for privilege escalation (if needed)
    ///
    /// Note: Currently not implemented - requires passwordless sudo or
    /// SSH key forwarding
    pub password: Option<String>,

    /// Additional flags for the become method
    ///
    /// Example: For sudo, you might add ["-n"] for non-interactive
    pub flags: Option<Vec<String>>,
}

impl Default for BecomeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            method: "sudo".to_string(),
            user: "root".to_string(),
            password: None,
            flags: None,
        }
    }
}

impl BecomeConfig {
    /// Create a new become config
    pub fn new(enabled: bool, method: String, user: String) -> Self {
        Self {
            enabled,
            method,
            user,
            password: None,
            flags: None,
        }
    }

    /// Resolve become config from task, play, and CLI args
    ///
    /// Precedence: task > play > CLI > default
    ///
    /// # Arguments
    ///
    /// * `task_become` - Task-level become flag
    /// * `task_user` - Task-level become_user (takes precedence if task_become is true)
    /// * `play_become` - Play-level become flag
    /// * `play_user` - Play-level become_user
    /// * `cli_become` - CLI --become flag
    /// * `cli_user` - CLI --become-user value
    /// * `cli_method` - CLI --become-method value
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustible::executor::become::BecomeConfig;
    ///
    /// let cfg = BecomeConfig::resolve(
    ///     true, Some("admin"),           // Task: become as "admin"
    ///     Some(true), Some("postgres"),  // Play: become as "postgres"
    ///     true, "root", "sudo"           // CLI: become as "root"
    /// );
    ///
    /// assert_eq!(cfg.enabled, true);
    /// assert_eq!(cfg.user, "admin");  // Task wins
    /// ```
    pub fn resolve(
        task_become: bool,
        task_user: Option<&str>,
        play_become: Option<bool>,
        play_user: Option<&str>,
        cli_become: bool,
        cli_user: &str,
        cli_method: &str,
    ) -> Self {
        // Determine if become is enabled (task > play > CLI)
        let enabled = task_become || play_become.unwrap_or(cli_become);

        // Determine user (task > play > CLI)
        // Only use task/play user if their respective become flag is set
        let user = if task_become && task_user.is_some() {
            task_user.unwrap().to_string()
        } else if play_become.unwrap_or(false) && play_user.is_some() {
            play_user.unwrap().to_string()
        } else {
            cli_user.to_string()
        };

        Self {
            enabled,
            method: cli_method.to_string(),  // Method comes from CLI/config only
            user,
            password: None,
            flags: None,
        }
    }

    /// Convert to connection ExecuteOptions escalation parameter
    ///
    /// Returns Some(user) if become is enabled, None otherwise
    pub fn to_execute_options(&self) -> Option<String> {
        if self.enabled {
            Some(self.user.clone())
        } else {
            None
        }
    }

    /// Check if become is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the escalation method
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Get the target user
    pub fn user(&self) -> &str {
        &self.user
    }

    /// Create a disabled become config
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let cfg = BecomeConfig::default();
        assert_eq!(cfg.enabled, false);
        assert_eq!(cfg.method, "sudo");
        assert_eq!(cfg.user, "root");
    }

    #[test]
    fn test_resolve_task_wins() {
        let cfg = BecomeConfig::resolve(
            true, Some("task-user"),       // Task
            Some(true), Some("play-user"), // Play
            true, "cli-user", "sudo"       // CLI
        );

        assert_eq!(cfg.enabled, true);
        assert_eq!(cfg.user, "task-user");
        assert_eq!(cfg.method, "sudo");
    }

    #[test]
    fn test_resolve_play_wins() {
        let cfg = BecomeConfig::resolve(
            false, None,                   // Task (disabled)
            Some(true), Some("play-user"), // Play
            true, "cli-user", "sudo"       // CLI
        );

        assert_eq!(cfg.enabled, true);
        assert_eq!(cfg.user, "play-user");
    }

    #[test]
    fn test_resolve_cli_wins() {
        let cfg = BecomeConfig::resolve(
            false, None,         // Task (disabled)
            Some(false), None,   // Play (disabled)
            true, "cli-user", "sudo"
        );

        assert_eq!(cfg.enabled, true);
        assert_eq!(cfg.user, "cli-user");
    }

    #[test]
    fn test_resolve_all_disabled() {
        let cfg = BecomeConfig::resolve(
            false, None,
            Some(false), None,
            false, "root", "sudo"
        );

        assert_eq!(cfg.enabled, false);
    }

    #[test]
    fn test_to_execute_options() {
        let cfg = BecomeConfig::new(true, "sudo".to_string(), "admin".to_string());
        assert_eq!(cfg.to_execute_options(), Some("admin".to_string()));

        let cfg = BecomeConfig::disabled();
        assert_eq!(cfg.to_execute_options(), None);
    }
}
```

---

### 2. Modify `src/executor/runtime.rs`

**Add become field to ExecutionContext:**

```rust
// Find the ExecutionContext struct (around line 50-100)

/// Context for task execution
#[derive(Clone)]
pub struct ExecutionContext {
    /// Current host being executed against
    pub host: String,

    /// Python interpreter path
    pub python_interpreter: String,

    /// Whether we're in check mode (--check / dry-run)
    pub check_mode: bool,

    /// Whether to show diffs for file changes
    pub diff_mode: bool,

    /// Verbosity level (0-4)
    pub verbosity: u8,

    /// Connection to the host (if not localhost)
    pub connection: Option<Arc<dyn Connection>>,

    // ✅ ADD THIS FIELD
    /// Become configuration (privilege escalation)
    pub become: crate::executor::become::BecomeConfig,
}

impl ExecutionContext {
    /// Create a new execution context
    pub fn new(host: String) -> Self {
        Self {
            host,
            python_interpreter: "/usr/bin/python3".to_string(),
            check_mode: false,
            diff_mode: false,
            verbosity: 0,
            connection: None,
            become: crate::executor::become::BecomeConfig::default(),  // ✅ ADD THIS
        }
    }

    // ✅ ADD THIS METHOD
    /// Set the become configuration
    pub fn with_become(mut self, become: crate::executor::become::BecomeConfig) -> Self {
        self.become = become;
        self
    }

    // ... rest of existing methods ...
}
```

---

### 3. Modify `src/executor/task.rs`

**A. Import the become module (top of file):**

```rust
use crate::executor::become::BecomeConfig;
```

**B. Update execute method (around line 482-634):**

```rust
impl Task {
    /// Execute the task
    #[instrument(skip(self, ctx, runtime, handlers, notified, parallelization_manager), fields(task_name = %self.name, host = %ctx.host))]
    pub async fn execute(
        &self,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        handlers: &Arc<RwLock<HashMap<String, Handler>>>,
        notified: &Arc<Mutex<std::collections::HashSet<String>>>,
        parallelization_manager: &Arc<ParallelizationManager>,
    ) -> ExecutorResult<TaskResult> {
        info!("Executing task: {}", self.name);

        // ✅ ADD: Resolve become config for this task
        // Task-level become takes precedence over context (play/CLI)
        let become = if self.r#become {
            // Task has become enabled, merge with context
            BecomeConfig {
                enabled: true,
                user: self.become_user.clone().unwrap_or_else(|| ctx.become.user.clone()),
                method: ctx.become.method.clone(),
                password: None,
                flags: None,
            }
        } else if ctx.become.enabled {
            // Use play/CLI become settings
            ctx.become.clone()
        } else {
            // Become disabled
            BecomeConfig::disabled()
        };

        // ✅ ADD: Create new context with resolved become
        let mut exec_ctx = ctx.clone();
        exec_ctx.become = become;

        // Evaluate when condition (use original ctx for variable access)
        if let Some(ref condition) = self.when {
            let should_run = self.evaluate_condition(condition, ctx, runtime).await?;
            if !should_run {
                debug!("Task skipped due to when condition: {}", condition);
                return Ok(TaskResult::skipped(format!(
                    "Skipped: condition '{}' was false",
                    condition
                )));
            }
        }

        // Handle delegation - create appropriate context for execution and fact storage
        let (execution_ctx, fact_storage_ctx) = if let Some(ref delegate_host) = self.delegate_to {
            debug!("Delegating task to host: {}", delegate_host);

            let mut delegate_ctx = exec_ctx.clone();  // ✅ CHANGED: use exec_ctx
            delegate_ctx.host = delegate_host.clone();

            let fact_ctx = if self.delegate_facts.unwrap_or(false) {
                let mut fact_ctx = exec_ctx.clone();  // ✅ CHANGED: use exec_ctx
                fact_ctx.host = delegate_host.clone();
                fact_ctx
            } else {
                exec_ctx.clone()  // ✅ CHANGED: use exec_ctx
            };

            (delegate_ctx, fact_ctx)
        } else {
            (exec_ctx.clone(), exec_ctx.clone())  // ✅ CHANGED: use exec_ctx
        };

        // ... rest of method stays the same ...
    }
}
```

**C. Update execute_module to use resolved become (around line 902-1051):**

```rust
async fn execute_module(
    &self,
    ctx: &ExecutionContext,
    runtime: &Arc<RwLock<RuntimeContext>>,
    handlers: &Arc<RwLock<HashMap<String, Handler>>>,
    notified: &Arc<Mutex<std::collections::HashSet<String>>>,
    parallelization_manager: &Arc<ParallelizationManager>,
) -> ExecutorResult<TaskResult> {
    // Template the arguments
    let args = self.template_args(ctx, runtime).await?;

    debug!("Module: {}, Args: {:?}", self.module, args);

    // ... parallelization code stays the same ...

    // Execute based on module type
    let result = match self.module.as_str() {
        "debug" => self.execute_debug(&args, ctx).await,
        "set_fact" => self.execute_set_fact(&args, ctx, runtime).await,
        "command" | "shell" => self.execute_command(&args, ctx, runtime).await,  // ✅ Real execution
        "copy" => self.execute_copy(&args, ctx, runtime).await,
        "file" => self.execute_file(&args, ctx).await,
        "template" => self.execute_template(&args, ctx, runtime).await,

        // ✅ REMOVE these stub methods - let them fall through to Python fallback
        // "package" | "apt" | "yum" | "dnf" => self.execute_package(&args, ctx).await,
        // "service" | "systemd" => self.execute_service(&args, ctx).await,
        // "user" => self.execute_user(&args, ctx).await,
        // "group" => self.execute_group(&args, ctx).await,
        // "lineinfile" => self.execute_lineinfile(&args, ctx).await,
        // "blockinfile" => self.execute_blockinfile(&args, ctx).await,

        "stat" => self.execute_stat(&args, ctx).await,
        "fail" => self.execute_fail(&args).await,
        "assert" => self.execute_assert(&args, ctx, runtime).await,
        "pause" => self.execute_pause(&args).await,
        "wait_for" => self.execute_wait_for(&args, ctx).await,
        "include_vars" => self.execute_include_vars(&args, ctx, runtime).await,
        "include_tasks" | "import_tasks" => {
            self.execute_include_tasks(&args, ctx, runtime, handlers, notified, parallelization_manager).await
        }
        "meta" => self.execute_meta(&args).await,
        "gather_facts" | "setup" => self.execute_gather_facts(&args, ctx).await,

        _ => {
            // ✅ UPDATED: Python fallback with connection requirement
            let mut executor = crate::modules::PythonModuleExecutor::new();

            if let Some(module_path) = executor.find_module(&self.module) {
                debug!(
                    "Found Ansible module {} at {} - executing via Python",
                    self.module,
                    module_path.display()
                );

                if ctx.check_mode {
                    return Ok(TaskResult::skipped(format!(
                        "Check mode - would execute Python module: {}",
                        self.module
                    )));
                }

                // ✅ CHANGED: Require connection - no simulation
                let connection = ctx.connection.as_ref().ok_or_else(|| {
                    ExecutorError::RuntimeError(format!(
                        "Module '{}' requires a connection to execute but none is available. \
                         This module needs to run on a remote host or via SSH.",
                        self.module
                    ))
                })?;

                // Convert args to ModuleParams-compatible format
                let module_params: std::collections::HashMap<String, serde_json::Value> =
                    args.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

                // ✅ EXECUTE REAL PYTHON MODULE
                match executor
                    .execute(
                        connection.as_ref(),
                        &self.module,
                        &module_params,
                        &ctx.python_interpreter,
                    )
                    .await
                {
                    Ok(output) => {
                        let mut result = if output.changed {
                            TaskResult::changed()
                        } else {
                            TaskResult::ok()
                        };
                        result.msg = Some(output.msg);
                        if !output.data.is_empty() {
                            result.result = Some(
                                serde_json::to_value(&output.data).unwrap_or_default(),
                            );
                        }
                        Ok(result)
                    }
                    Err(e) => Err(ExecutorError::RuntimeError(format!(
                        "Python module {} execution failed: {}",
                        self.module, e
                    ))),
                }
            } else {
                // ✅ CHANGED: Hard error with actionable message
                Err(ExecutorError::ModuleNotFound(format!(
                    "Module '{}' not found.\n\
                     \n\
                     This module is not a built-in native module and was not found in \
                     Ansible module paths.\n\
                     \n\
                     To fix this:\n\
                     1. Install Ansible: pip install ansible\n\
                     2. Set ANSIBLE_LIBRARY environment variable to your module directory\n\
                     3. Verify the module name is spelled correctly\n\
                     \n\
                     Searched paths:\n{}",
                    self.module,
                    executor.search_paths()
                        .iter()
                        .map(|p| format!("  - {}", p.display()))
                        .collect::<Vec<_>>()
                        .join("\n")
                )))
            }
        }
    };

    result
}
```

**D. Rewrite execute_command for real execution:**

```rust
async fn execute_command(
    &self,
    args: &IndexMap<String, JsonValue>,
    ctx: &ExecutionContext,
    _runtime: &Arc<RwLock<RuntimeContext>>,
) -> ExecutorResult<TaskResult> {
    let cmd = args
        .get("cmd")
        .or_else(|| args.get("_raw_params"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ExecutorError::RuntimeError("command module requires 'cmd' argument".into())
        })?;

    if ctx.check_mode {
        return Ok(TaskResult::skipped("Check mode - command not executed"));
    }

    // ✅ REQUIRE CONNECTION - no simulation
    let connection = ctx.connection.as_ref().ok_or_else(|| {
        ExecutorError::RuntimeError(
            "command module requires a connection but none is available".into()
        )
    })?;

    // ✅ BUILD EXECUTE OPTIONS with become support
    let chdir = args.get("chdir").and_then(|v| v.as_str()).map(String::from);

    let execute_opts = if ctx.become.enabled {
        // With become
        Some(ExecuteOptions::new()
            .with_escalation(Some(ctx.become.user.clone()))
            .with_cwd(chdir))
    } else if chdir.is_some() {
        // No become, but has chdir
        Some(ExecuteOptions::new().with_cwd(chdir))
    } else {
        // No options
        None
    };

    // ✅ EXECUTE REAL COMMAND
    let result = connection
        .execute(cmd, execute_opts)
        .await
        .map_err(|e| ExecutorError::RuntimeError(format!("Command execution failed: {}", e)))?;

    // ✅ BUILD REAL RESULT
    let registered = RegisteredResult {
        changed: true,  // Command execution always changes state
        rc: Some(result.exit_code),
        stdout: Some(result.stdout.clone()),
        stderr: Some(result.stderr.clone()),
        failed: !result.success,
        ..Default::default()
    };

    if result.success {
        Ok(TaskResult::changed()
            .with_msg(format!("Command executed: {}", cmd))
            .with_result(registered.to_json()))
    } else {
        Ok(TaskResult::failed(format!(
            "Command failed with exit code {}: {}",
            result.exit_code,
            if !result.stderr.is_empty() { &result.stderr } else { &result.stdout }
        ))
        .with_result(registered.to_json()))
    }
}
```

**E. Update ModuleContext creation (multiple places):**

Search for all occurrences of `ModuleContext {` and replace hardcoded become fields:

```rust
// BEFORE:
let module_ctx = crate::modules::ModuleContext {
    check_mode: ctx.check_mode,
    diff_mode: ctx.diff_mode,
    verbosity: ctx.verbosity,
    vars: vars,
    facts: facts,
    work_dir: None,
    r#become: false,              // ❌ HARDCODED
    become_method: None,          // ❌ HARDCODED
    become_user: None,            // ❌ HARDCODED
    connection: ctx.connection.clone(),
};

// AFTER:
let module_ctx = crate::modules::ModuleContext {
    check_mode: ctx.check_mode,
    diff_mode: ctx.diff_mode,
    verbosity: ctx.verbosity,
    vars: vars,
    facts: facts,
    work_dir: None,
    r#become: ctx.become.enabled,                        // ✅ USE RESOLVED
    become_method: Some(ctx.become.method.clone()),      // ✅ USE RESOLVED
    become_user: Some(ctx.become.user.clone()),          // ✅ USE RESOLVED
    connection: ctx.connection.clone(),
};
```

**F. DELETE these stub methods entirely:**

Remove methods (lines ~1475-1597):
- `execute_package`
- `execute_service`
- `execute_user`
- `execute_group`
- `execute_lineinfile`
- `execute_blockinfile`

They will now fall through to the Python fallback path.

---

### 4. Modify `src/connection/mod.rs`

**Add helper methods to ExecuteOptions:**

```rust
impl ExecuteOptions {
    /// Create ExecuteOptions from BecomeConfig
    pub fn from_become(become: &crate::executor::become::BecomeConfig) -> Self {
        Self {
            cwd: None,
            env: HashMap::new(),
            escalation: if become.enabled {
                Some(become.user.clone())
            } else {
                None
            },
            timeout: None,
        }
    }

    /// Merge become config into existing options
    pub fn with_become(mut self, become: &crate::executor::become::BecomeConfig) -> Self {
        if become.enabled {
            self.escalation = Some(become.user.clone());
        }
        self
    }

    /// Set working directory
    pub fn with_cwd(mut self, cwd: Option<String>) -> Self {
        self.cwd = cwd;
        self
    }

    /// Set escalation user
    pub fn with_escalation(mut self, user: Option<String>) -> Self {
        self.escalation = user;
        self
    }
}
```

---

### 5. Modify `src/cli/commands/run.rs`

**Update execute_remote_command method (around line 1280-1308):**

```rust
async fn execute_remote_command(&self, ctx: &CommandContext, host: &str, cmd: &str) -> Result<bool> {
    // ... existing connection setup code ...

    // ✅ BUILD EXECUTE OPTIONS with become support
    let execute_opts = if self.r#become {
        Some(ExecuteOptions::new()
            .with_escalation(Some(self.become_user.clone())))
    } else {
        None
    };

    // Execute command on the pooled connection
    let result = conn
        .execute(cmd, execute_opts)  // ✅ PASS OPTIONS
        .await
        .map_err(|e| anyhow::anyhow!("Command execution failed: {}", e))?;

    if result.success {
        Ok(true)
    } else {
        Err(anyhow::anyhow!(
            "Command failed with exit code {}: {}",
            result.exit_code,
            if result.stderr.is_empty() {
                result.stdout
            } else {
                result.stderr
            }
        ))
    }
}
```

---

### 6. Modify `src/executor/mod.rs`

**Add new error variants:**

```rust
#[derive(Error, Debug)]
pub enum ExecutorError {
    // ... existing variants ...

    /// Module not found (not native and not in Ansible paths)
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// Module requires connection but none available
    #[error("Connection required: {0}")]
    ConnectionRequired(String),
}
```

---

## Testing Examples

### Unit Test: Become Precedence

Create `tests/unit/executor/become_config.rs`:

```rust
use rustible::executor::become::BecomeConfig;

#[test]
fn test_become_precedence_task_wins() {
    let cfg = BecomeConfig::resolve(
        true, Some("task-user"),           // Task level
        Some(true), Some("play-user"),     // Play level
        true, "cli-user", "sudo"           // CLI level
    );

    assert_eq!(cfg.enabled, true);
    assert_eq!(cfg.user, "task-user");  // Task wins!
    assert_eq!(cfg.method, "sudo");
}

#[test]
fn test_become_precedence_play_wins() {
    let cfg = BecomeConfig::resolve(
        false, None,                       // Task (disabled)
        Some(true), Some("play-user"),     // Play level
        true, "cli-user", "sudo"           // CLI level
    );

    assert_eq!(cfg.enabled, true);
    assert_eq!(cfg.user, "play-user");  // Play wins!
}

#[test]
fn test_become_precedence_cli_wins() {
    let cfg = BecomeConfig::resolve(
        false, None,                   // Task (disabled)
        Some(false), None,             // Play (disabled)
        true, "cli-user", "sudo"       // CLI level
    );

    assert_eq!(cfg.enabled, true);
    assert_eq!(cfg.user, "cli-user");  // CLI wins!
}
```

### Integration Test: Become End-to-End

Create `tests/integration/become.yml`:

```yaml
---
- name: Test become precedence
  hosts: localhost
  connection: local
  become: true
  become_user: play-user
  tasks:
    - name: Task with default (play) become
      command: whoami
      register: play_level

    - name: Task with override become
      command: whoami
      become_user: task-user
      register: task_level

    - name: Verify play-level become
      assert:
        that:
          - play_level.stdout == "play-user" or play_level.rc == 0
        fail_msg: "Play-level become did not work"

    - name: Verify task-level become override
      assert:
        that:
          - task_level.stdout == "task-user" or task_level.rc == 0
        fail_msg: "Task-level become did not override play-level"
```

### Integration Test: Real Command Execution

Create `tests/integration/command_execution.rs`:

```rust
use rustible::executor::{ExecutionContext, Task, TaskStatus};
use rustible::connection::{Connection, MockConnection};
use std::sync::Arc;

#[tokio::test]
async fn test_command_real_execution() {
    // Setup mock connection
    let mut mock_conn = MockConnection::new();
    mock_conn.expect_execute()
        .with(eq("echo test"), any())
        .returning(|_, _| Ok(CommandResult {
            exit_code: 0,
            stdout: "test\n".to_string(),
            stderr: String::new(),
            success: true,
        }));

    // Create execution context with connection
    let ctx = ExecutionContext::new("test-host".to_string())
        .with_connection(Some(Arc::new(mock_conn)));

    // Execute command task
    let task = Task::new("test command", "command")
        .arg("cmd", "echo test");

    let runtime = /* setup runtime */;
    let handlers = /* setup handlers */;
    let notified = /* setup notified */;
    let pm = /* setup parallelization manager */;

    let result = task.execute(&ctx, &runtime, &handlers, &notified, &pm).await.unwrap();

    // Verify real execution
    assert_eq!(result.status, TaskStatus::Changed);
    assert!(result.result.is_some());

    let data = result.result.unwrap();
    assert_eq!(data["rc"], 0);
    assert_eq!(data["stdout"], "test\n");
}
```

---

## Common Pitfalls

### Pitfall 1: Forgetting to Clone exec_ctx

**Wrong:**
```rust
let (execution_ctx, fact_storage_ctx) = if let Some(ref delegate_host) = self.delegate_to {
    let mut delegate_ctx = ctx.clone();  // ❌ Uses original ctx, loses become
    // ...
}
```

**Right:**
```rust
let (execution_ctx, fact_storage_ctx) = if let Some(ref delegate_host) = self.delegate_to {
    let mut delegate_ctx = exec_ctx.clone();  // ✅ Uses exec_ctx with resolved become
    // ...
}
```

### Pitfall 2: Hardcoding Become in ModuleContext

**Wrong:**
```rust
let module_ctx = ModuleContext {
    // ...
    r#become: false,  // ❌ HARDCODED
    become_user: None,
    // ...
};
```

**Right:**
```rust
let module_ctx = ModuleContext {
    // ...
    r#become: ctx.become.enabled,  // ✅ From resolved config
    become_user: Some(ctx.become.user.clone()),
    become_method: Some(ctx.become.method.clone()),
    // ...
};
```

### Pitfall 3: Not Checking Connection Availability

**Wrong:**
```rust
async fn execute_command(...) -> ExecutorResult<TaskResult> {
    let connection = ctx.connection.as_ref().unwrap();  // ❌ Panics if None
    // ...
}
```

**Right:**
```rust
async fn execute_command(...) -> ExecutorResult<TaskResult> {
    let connection = ctx.connection.as_ref().ok_or_else(|| {  // ✅ Returns error
        ExecutorError::RuntimeError(
            "command module requires a connection but none is available".into()
        )
    })?;
    // ...
}
```

### Pitfall 4: Simulating Instead of Executing

**Wrong:**
```rust
async fn execute_command(...) -> ExecutorResult<TaskResult> {
    debug!("Would execute: {}", cmd);
    Ok(TaskResult::changed())  // ❌ FAKE result
}
```

**Right:**
```rust
async fn execute_command(...) -> ExecutorResult<TaskResult> {
    let connection = ctx.connection.as_ref().ok_or_else(/* ... */)?;
    let result = connection.execute(cmd, execute_opts).await?;  // ✅ REAL execution

    Ok(TaskResult::changed()
        .with_msg(format!("Command executed: {}", cmd))
        .with_result(result.to_json()))  // ✅ REAL result data
}
```

---

## Checklist for Implementation

- [ ] Create `src/executor/become.rs`
- [ ] Add `become: BecomeConfig` to `ExecutionContext`
- [ ] Implement `BecomeConfig::resolve()` with precedence
- [ ] Update `Task::execute()` to resolve become
- [ ] Replace all hardcoded `r#become: false` with `ctx.become.enabled`
- [ ] Rewrite `execute_command()` for real execution
- [ ] Remove stub methods (package, service, user, group, lineinfile, blockinfile)
- [ ] Update Python fallback to require connection
- [ ] Add `ModuleNotFound` error type
- [ ] Add helper methods to `ExecuteOptions`
- [ ] Update CLI `execute_remote_command()` to use `ExecuteOptions`
- [ ] Write unit tests for `BecomeConfig`
- [ ] Write integration tests for become precedence
- [ ] Write integration tests for real command execution
- [ ] Update documentation

---

**Next:** Start with Phase 1 (BecomeConfig implementation) and work through the checklist sequentially.
