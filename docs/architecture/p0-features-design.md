# Architecture Design: P0 Features Implementation

**Date:** 2026-01-01
**Authors:** System Architecture Designer
**Status:** Design Phase
**Priority:** P0 - Blocking M1 (Remote execution MVP)

## Executive Summary

This document provides a comprehensive architectural design for implementing two critical P0 features required for the M1 milestone:

1. **Issue #52:** End-to-end `become` (privilege escalation) implementation
2. **Issue #53:** Removal of simulated module execution

Both features are fundamental to achieving a production-ready remote execution engine that users can trust.

---

## Table of Contents

1. [Current State Analysis](#current-state-analysis)
2. [Architecture Overview](#architecture-overview)
3. [Feature 1: Become Implementation](#feature-1-become-implementation)
4. [Feature 2: Remove Simulated Execution](#feature-2-remove-simulated-execution)
5. [Integration Points](#integration-points)
6. [Implementation Roadmap](#implementation-roadmap)
7. [Risk Analysis & Mitigations](#risk-analysis--mitigations)
8. [Testing Strategy](#testing-strategy)

---

## Current State Analysis

### 1.1 Become (Privilege Escalation)

**Current Implementation:**

```
CLI Layer (src/cli/commands/run.rs)
  ├─ Parses become args (lines 47-58)
  │  ├─ --become / -b
  │  ├─ --become-method (default: "sudo")
  │  └─ --become-user (default: "root")
  │
  ├─ Stores in RunArgs struct
  └─ NEVER PASSED to connection layer
      └─ Line 1291: conn.execute(cmd, None)  ❌ No ExecuteOptions

Executor Layer (src/executor/task.rs)
  ├─ Task struct has become fields (lines 265-269)
  │  ├─ r#become: bool
  │  └─ become_user: Option<String>
  │
  └─ Creates ModuleContext with HARDCODED become=false
      ├─ Line 1306: r#become: false  ❌
      ├─ Line 1347: r#become: false  ❌
      ├─ Line 1393: r#become: false  ❌
      └─ Line 1446: r#become: false  ❌

Connection Layer (src/connection)
  ├─ Pipelining supports escalation (src/connection/pipelining.rs:121-124)
  │  ├─ escalate: bool
  │  └─ escalate_user: Option<String>
  │
  ├─ ExecuteOptions has escalation field
  └─ Implementation in wrap_command (lines 311-315)
      └─ Wraps with "sudo -u {user}" when escalate=true ✓
```

**Problem:** The plumbing exists but is never connected. Become settings are parsed but discarded.

### 1.2 Simulated Module Execution

**Current Simulation Points:**

```
src/executor/task.rs

Line 1034-1037: Python module fallback without connection
  └─ Returns TaskResult::changed() with "(simulated - no connection)" ❌

Line 1261-1275: execute_command
  └─ Debug log "Would execute command"
  └─ Returns TaskResult::changed() with empty stdout/stderr ❌

Line 1475-1597: Stubbed modules (package, service, user, group, lineinfile, blockinfile)
  └─ All return TaskResult::changed() without real execution ❌
  └─ Only log "Would ensure/manage/modify..." messages
```

**Problem:** Multiple execution paths return `changed=true` without performing any actual work, breaking trust and correctness guarantees.

---

## Architecture Overview

### 2.1 System Component Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLI Layer                                │
│  ┌────────────┐    ┌─────────────┐    ┌──────────────┐         │
│  │  RunArgs   │───▶│ PlaybookExec│───▶│ ConnectionMgr│         │
│  │  (become)  │    │             │    │              │         │
│  └────────────┘    └─────────────┘    └──────────────┘         │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Executor Layer                              │
│  ┌────────────┐    ┌──────────────┐    ┌──────────────┐        │
│  │    Task    │───▶│ ModuleContext│───▶│ Module Exec  │        │
│  │  (become)  │    │   (become)   │    │              │        │
│  └────────────┘    └──────────────┘    └──────────────┘        │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Connection Layer                              │
│  ┌──────────────┐    ┌──────────────┐    ┌─────────────┐       │
│  │ExecuteOptions│───▶│  Connection  │───▶│ SSH/Local   │       │
│  │ (escalation) │    │  .execute()  │    │ (sudo wrap) │       │
│  └──────────────┘    └──────────────┘    └─────────────┘       │
└─────────────────────────────────────────────────────────────────┘

Data Flow:
  CLI become args → Play/Task become → ModuleContext → ExecuteOptions
```

### 2.2 Become Precedence Chain

```
Precedence (highest to lowest):
1. Task.become / Task.become_user
2. Play.become / Play.become_user
3. CLI --become / --become-user
4. Config file defaults

Implementation:
  fn resolve_become(&self, task: &Task, play: &Play, cli: &RunArgs) -> BecomeConfig {
      BecomeConfig {
          enabled: task.become.or(play.become).or(cli.become).unwrap_or(false),
          method: task.become_method.or(play.become_method).or(cli.become_method).unwrap_or("sudo"),
          user: task.become_user.or(play.become_user).or(cli.become_user).unwrap_or("root"),
      }
  }
```

---

## Feature 1: Become Implementation

### 3.1 Design Goals

1. **End-to-end threading:** Become settings flow from CLI → Executor → Connection
2. **Correct precedence:** Task > Play > CLI > Config
3. **Safety:** Check mode prevents privilege escalation side effects
4. **Compatibility:** Works with all connection types (SSH, Local, Docker)

### 3.2 Component Changes

#### 3.2.1 New Types (src/executor/become.rs)

```rust
/// Configuration for privilege escalation (become)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BecomeConfig {
    /// Whether to use privilege escalation
    pub enabled: bool,

    /// Method to use (sudo, su, doas, pbrun, etc.)
    pub method: String,

    /// User to become
    pub user: String,

    /// Password for privilege escalation (if needed)
    pub password: Option<String>,

    /// Additional flags for the become method
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
    /// Resolve become config from task, play, and CLI args
    pub fn resolve(
        task_become: bool,
        task_user: Option<&str>,
        play_become: Option<bool>,
        play_user: Option<&str>,
        cli_become: bool,
        cli_user: &str,
        cli_method: &str,
    ) -> Self {
        let enabled = task_become || play_become.unwrap_or(cli_become);
        let user = task_user
            .or(play_user)
            .map(String::from)
            .unwrap_or_else(|| cli_user.to_string());

        Self {
            enabled,
            method: cli_method.to_string(),
            user,
            password: None,
            flags: None,
        }
    }

    /// Convert to connection ExecuteOptions
    pub fn to_execute_options(&self) -> Option<String> {
        if self.enabled {
            Some(self.user.clone())
        } else {
            None
        }
    }
}
```

#### 3.2.2 ExecutionContext Enhancement (src/executor/runtime.rs)

```rust
/// Context for task execution
#[derive(Clone)]
pub struct ExecutionContext {
    // ... existing fields ...

    /// Become configuration for this execution
    pub become: BecomeConfig,
}

impl ExecutionContext {
    /// Create execution context with become settings
    pub fn with_become(mut self, become: BecomeConfig) -> Self {
        self.become = become;
        self
    }
}
```

#### 3.2.3 Task Execution Changes (src/executor/task.rs)

```rust
impl Task {
    /// Execute the task with become resolution
    pub async fn execute(
        &self,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        handlers: &Arc<RwLock<HashMap<String, Handler>>>,
        notified: &Arc<Mutex<std::collections::HashSet<String>>>,
        parallelization_manager: &Arc<ParallelizationManager>,
    ) -> ExecutorResult<TaskResult> {
        // Resolve become config (task takes precedence over context)
        let become = if self.r#become {
            BecomeConfig {
                enabled: true,
                user: self.become_user.clone().unwrap_or_else(|| ctx.become.user.clone()),
                method: ctx.become.method.clone(),
                password: None,
                flags: None,
            }
        } else {
            ctx.become.clone()
        };

        // Create new context with resolved become
        let mut exec_ctx = ctx.clone();
        exec_ctx.become = become;

        // Continue with existing execution logic...
    }

    /// Execute module with become support
    async fn execute_module(&self, ctx: &ExecutionContext, ...) -> ExecutorResult<TaskResult> {
        // ... existing code ...

        // Create module context with become settings
        let module_ctx = crate::modules::ModuleContext {
            check_mode: ctx.check_mode,
            diff_mode: ctx.diff_mode,
            verbosity: ctx.verbosity,
            vars: vars,
            facts: facts,
            work_dir: None,
            r#become: ctx.become.enabled,              // ✅ Use resolved become
            become_method: Some(ctx.become.method.clone()),  // ✅
            become_user: Some(ctx.become.user.clone()),      // ✅
            connection: ctx.connection.clone(),
        };

        // ... rest of execution ...
    }
}
```

#### 3.2.4 Connection Layer Integration (src/connection/mod.rs)

```rust
impl ExecuteOptions {
    /// Create ExecuteOptions from BecomeConfig
    pub fn from_become(become: &BecomeConfig) -> Self {
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
    pub fn with_become(mut self, become: &BecomeConfig) -> Self {
        if become.enabled {
            self.escalation = Some(become.user.clone());
        }
        self
    }
}
```

#### 3.2.5 CLI Changes (src/cli/commands/run.rs)

```rust
impl RunArgs {
    /// Execute remote command with become support
    async fn execute_remote_command(&self, ctx: &CommandContext, host: &str, cmd: &str) -> Result<bool> {
        // ... existing connection code ...

        // Build ExecuteOptions with become settings ✅
        let execute_opts = if self.r#become {
            Some(ExecuteOptions::new()
                .with_escalation(Some(self.become_user.clone())))
        } else {
            None
        };

        // Execute command with options
        let result = conn
            .execute(cmd, execute_opts)  // ✅ Pass options
            .await
            .map_err(|e| anyhow::anyhow!("Command execution failed: {}", e))?;

        // ... rest of logic ...
    }
}
```

### 3.3 Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. CLI Parsing                                                   │
│    RunArgs { become: true, become_user: "app", become_method }  │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. Play Loading                                                  │
│    Play { become: Some(true), become_user: Some("postgres") }   │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. Task Execution                                                │
│    Task { become: true, become_user: Some("admin") }            │
│                                                                  │
│    BecomeConfig::resolve(                                       │
│      task.become=true,     ← HIGHEST PRECEDENCE                 │
│      task_user="admin",                                         │
│      play.become=true,                                          │
│      play_user="postgres",                                      │
│      cli.become=true,                                           │
│      cli_user="app"                                             │
│    ) → BecomeConfig { enabled: true, user: "admin", ... }       │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 4. Module Execution                                              │
│    ModuleContext {                                               │
│      become: true,                                              │
│      become_user: Some("admin"),                                │
│      become_method: Some("sudo"),                               │
│      connection: Some(conn)                                     │
│    }                                                            │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ 5. Connection Execution                                          │
│    ExecuteOptions {                                              │
│      escalation: Some("admin")                                  │
│    }                                                            │
│                                                                  │
│    Command wrapped: "sudo -u admin <command>"                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## Feature 2: Remove Simulated Execution

### 4.1 Design Goals

1. **Zero simulation:** No path returns `changed=true` without real work
2. **Clear errors:** Unsupported modules fail with actionable messages
3. **Real execution:** All built-in modules execute actual operations
4. **Python fallback:** Clear separation between native and Python modules

### 4.2 Module Execution Strategy

```
┌─────────────────────────────────────────────────────────────────┐
│                    Module Execution Decision Tree                │
└─────────────────────────────────────────────────────────────────┘

Module requested
      │
      ├─ Is it a native Rust module?
      │  ├─ YES → Execute via ModuleRegistry
      │  │         ├─ With connection if remote
      │  │         └─ Return real result
      │  │
      │  └─ NO → Check Python fallback
      │           ├─ Ansible module found?
      │           │  ├─ YES → Execute via PythonModuleExecutor
      │           │  │         ├─ Requires connection
      │           │  │         └─ Return real result
      │           │  │
      │           │  └─ NO → FAIL with ModuleNotFound
      │           │            └─ Error: "Module 'xyz' not found.
      │           │                      Not a native module and not in
      │           │                      Ansible paths. Install Ansible
      │           │                      or set ANSIBLE_LIBRARY."
      │           │
      │           └─ Connection available?
      │              ├─ YES → Execute Python module
      │              └─ NO → FAIL with ConnectionRequired
      │                       └─ Error: "Module 'xyz' requires remote
      │                                 connection but none available"
```

### 4.3 Component Changes

#### 4.3.1 Remove Simulated Command Execution (src/executor/task.rs)

**BEFORE (lines 1243-1276):**
```rust
async fn execute_command(...) -> ExecutorResult<TaskResult> {
    // ...
    debug!("Would execute command: {}", cmd);

    // In a real implementation, this would actually run the command
    // For now, simulate successful execution ❌
    let result = RegisteredResult {
        changed: true,  // ❌ FAKE
        rc: Some(0),    // ❌ FAKE
        stdout: Some(String::new()),
        stderr: Some(String::new()),
        ..Default::default()
    };

    Ok(TaskResult::changed()
        .with_msg(format!("Command executed: {}", cmd))
        .with_result(result.to_json()))
}
```

**AFTER:**
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

    // Get connection or fail
    let connection = ctx.connection.as_ref().ok_or_else(|| {
        ExecutorError::RuntimeError(
            "command module requires a connection but none available".into()
        )
    })?;

    // Build execute options with become support
    let execute_opts = if ctx.become.enabled {
        Some(ExecuteOptions::new()
            .with_escalation(Some(ctx.become.user.clone()))
            .with_cwd(args.get("chdir").and_then(|v| v.as_str()).map(String::from)))
    } else {
        args.get("chdir")
            .and_then(|v| v.as_str())
            .map(|cwd| ExecuteOptions::new().with_cwd(Some(cwd.to_string())))
    };

    // Execute REAL command ✅
    let result = connection
        .execute(cmd, execute_opts)
        .await
        .map_err(|e| ExecutorError::RuntimeError(format!("Command execution failed: {}", e)))?;

    // Build real result ✅
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
            result.stderr
        ))
        .with_result(registered.to_json()))
    }
}
```

#### 4.3.2 Remove Stubbed Modules (src/executor/task.rs)

**BEFORE (lines 1475-1597):**
```rust
async fn execute_package(...) -> ExecutorResult<TaskResult> {
    // ...
    debug!("Would ensure package {:?} is {}", name, state);
    Ok(TaskResult::changed().with_msg(format!("Package {:?} state: {}", name, state)))  // ❌ FAKE
}

async fn execute_service(...) -> ExecutorResult<TaskResult> {
    // ...
    debug!("Would manage service: {} ...", name);
    Ok(TaskResult::changed().with_msg(format!("Service {} managed", name)))  // ❌ FAKE
}

// Similar for: execute_user, execute_group, execute_lineinfile, execute_blockinfile
```

**AFTER:**
```rust
// Remove these stub methods entirely and handle in the fallback path:

async fn execute_module(...) -> ExecutorResult<TaskResult> {
    // ... existing dispatch for implemented modules (debug, set_fact, copy, file, template, etc.) ...

    _ => {
        // Python fallback or error - NO SIMULATION ✅
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

            // Require connection for Python modules ✅
            let connection = ctx.connection.as_ref().ok_or_else(|| {
                ExecutorError::RuntimeError(format!(
                    "Module '{}' requires a connection to execute but none available",
                    self.module
                ))
            })?;

            // Execute REAL Python module ✅
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
                    "Python module {} failed: {}",
                    self.module, e
                ))),
            }
        } else {
            // Module not found - HARD ERROR ✅
            Err(ExecutorError::ModuleNotFound(format!(
                "Module '{}' not found.\n\
                 - Not a native Rust module\n\
                 - Not found in Ansible module paths\n\
                 \n\
                 To fix:\n\
                 1. Install Ansible: pip install ansible\n\
                 2. Set ANSIBLE_LIBRARY environment variable\n\
                 3. Check module name spelling\n\
                 \n\
                 Searched paths: {}",
                self.module,
                executor.search_paths().join(", ")
            )))
        }
    }
}
```

#### 4.3.3 Enhanced Error Types (src/executor/mod.rs)

```rust
#[derive(Error, Debug)]
pub enum ExecutorError {
    // ... existing variants ...

    /// Module not found anywhere (native or Python)
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// Module requires connection but none available
    #[error("Connection required: {0}")]
    ConnectionRequired(String),

    /// Module execution failed
    #[error("Module execution failed: {0}")]
    ModuleExecutionFailed(String),
}
```

#### 4.3.4 Native Module Implementation Priority

**Implement these as native Rust modules (high ROI):**

1. ✅ **command/shell** - Already covered above
2. **stat** - File info gathering (already exists in codebase)
3. **copy** - Already implemented
4. **file** - Already implemented
5. **template** - Already implemented

**Defer to Python fallback (complex, low priority):**

1. **package/apt/yum/dnf** - Complex package manager integration
2. **service/systemd** - Service management
3. **user** - User account management
4. **group** - Group management
5. **lineinfile/blockinfile** - Text manipulation

---

## Integration Points

### 5.1 Cross-Layer Data Flow

```
┌──────────────┐
│ CLI RunArgs  │ become: bool, become_user: String, become_method: String
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ Playbook     │ Play.become: Option<bool>, Play.become_user: Option<String>
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ Task         │ task.become: bool, task.become_user: Option<String>
└──────┬───────┘
       │
       ▼
┌──────────────────┐
│ BecomeConfig     │ resolve(task, play, cli) → BecomeConfig
│ (NEW)            │
└──────┬───────────┘
       │
       ├─────────────────────────────┐
       │                             │
       ▼                             ▼
┌──────────────┐            ┌─────────────────┐
│ModuleContext │            │ ExecuteOptions  │
│  become: bool│            │ escalation: Opt │
└──────┬───────┘            └────────┬────────┘
       │                             │
       │                             │
       ▼                             ▼
┌──────────────┐            ┌─────────────────┐
│Module::exec()│            │Connection::exec │
│ (native mods)│            │ (SSH wrapping)  │
└──────────────┘            └─────────────────┘
```

### 5.2 Module → Connection Integration

```rust
// In native modules (e.g., src/modules/command.rs)
impl Module for CommandModule {
    fn execute(&self, params: &ModuleParams, ctx: &ModuleContext) -> ModuleResult {
        if let Some(ref connection) = ctx.connection {
            // Build execute options from context
            let execute_opts = if ctx.r#become {
                Some(ExecuteOptions::new()
                    .with_escalation(ctx.become_user.clone())
                    .with_cwd(params.get("chdir")))
            } else {
                None
            };

            // Execute via connection
            let result = tokio::runtime::Handle::current()
                .block_on(connection.execute(cmd, execute_opts))?;

            // Return real result
            ModuleOutput {
                changed: true,
                msg: format!("Command executed with rc={}", result.exit_code),
                data: hashmap! {
                    "rc" => result.exit_code,
                    "stdout" => result.stdout,
                    "stderr" => result.stderr,
                },
            }
        } else {
            Err(ModuleError::MissingRequirement("connection required"))
        }
    }
}
```

---

## Implementation Roadmap

### Phase 1: Foundation (Week 1)

**Goal:** Establish core types and wiring

**Tasks:**
1. Create `BecomeConfig` type and precedence resolution logic
2. Add `become: BecomeConfig` field to `ExecutionContext`
3. Update `Task::execute()` to resolve become config
4. Add helper methods to `ExecuteOptions` for become conversion
5. Write unit tests for become precedence resolution

**Acceptance Criteria:**
- [ ] `BecomeConfig::resolve()` correctly prioritizes task > play > CLI
- [ ] `ExecutionContext` carries become config through execution
- [ ] Unit tests cover all precedence combinations

### Phase 2: Connection Integration (Week 1-2)

**Goal:** Make become work end-to-end in connection layer

**Tasks:**
1. Update CLI `execute_remote_command()` to build `ExecuteOptions` with become
2. Update `ModuleContext` creation to use `ctx.become` instead of hardcoded `false`
3. Verify SSH pipelining `wrap_command()` correctly applies `sudo -u`
4. Test with local and SSH connections
5. Add integration tests for become with different connection types

**Acceptance Criteria:**
- [ ] CLI `--become` flag causes remote commands to use `sudo`
- [ ] Task-level `become: true` overrides CLI settings
- [ ] Integration test: playbook with mixed become settings executes correctly
- [ ] Check mode does not execute privileged commands

### Phase 3: Real Command Execution (Week 2)

**Goal:** Remove simulation from command/shell modules

**Tasks:**
1. Rewrite `execute_command()` to use real connection execution
2. Add error handling for missing connection
3. Add error handling for command failures
4. Update tests to use mock connections
5. Test check mode behavior (no execution, correct reporting)

**Acceptance Criteria:**
- [ ] `command` module executes real commands via connection
- [ ] Returns actual stdout/stderr/exit_code
- [ ] Fails gracefully when connection unavailable
- [ ] Check mode skips execution

### Phase 4: Python Fallback Hardening (Week 3)

**Goal:** Remove all remaining simulations

**Tasks:**
1. Remove stubbed `execute_package`, `execute_service`, etc.
2. Update fallback path to require connection for Python modules
3. Improve error messages for missing modules
4. Add `search_paths()` method to `PythonModuleExecutor`
5. Test with and without Ansible installed

**Acceptance Criteria:**
- [ ] No execution path returns `changed=true` without real work
- [ ] `package` module executes via Ansible Python fallback
- [ ] Missing modules fail with actionable error (install Ansible, set ANSIBLE_LIBRARY)
- [ ] Error messages include searched paths

### Phase 5: Testing & Documentation (Week 3-4)

**Goal:** Comprehensive testing and docs

**Tasks:**
1. Integration tests for become (task/play/CLI precedence)
2. Integration tests for module execution (native + Python)
3. Integration tests for error cases (missing module, missing connection)
4. Update user documentation with become examples
5. Update module development guide

**Acceptance Criteria:**
- [ ] Integration test: playbook with become at all levels
- [ ] Integration test: Python module execution with real Ansible
- [ ] Integration test: graceful failures with clear errors
- [ ] Documentation includes become usage examples
- [ ] Code coverage > 80% for new code

---

## Risk Analysis & Mitigations

### Risk 1: Breaking Changes to Existing Tests

**Likelihood:** High
**Impact:** Medium
**Mitigation:**
- Update tests incrementally phase by phase
- Use feature flags if necessary for gradual rollout
- Maintain backward compatibility during transition

### Risk 2: SSH Sudo Password Prompts

**Likelihood:** Medium
**Impact:** High (blocks automation)
**Mitigation:**
- Document passwordless sudo requirement
- Implement `--ask-become-pass` flag (future work)
- Provide clear error messages when sudo requires password
- Support NOPASSWD sudo configurations

### Risk 3: Connection Availability

**Likelihood:** Low
**Impact:** High (execution failures)
**Mitigation:**
- Always check `ctx.connection` before use
- Provide clear error messages: "Module X requires connection but none available"
- Test localhost execution paths separately

### Risk 4: Python Module Compatibility

**Likelihood:** Medium
**Impact:** Medium
**Mitigation:**
- Test with multiple Ansible versions
- Document minimum Ansible version requirements
- Provide clear error when Ansible not installed
- Consider implementing most common modules natively over time

### Risk 5: Performance Regression

**Likelihood:** Low
**Impact:** Low
**Mitigation:**
- Benchmark before/after with real connections
- Become resolution is a simple precedence chain (minimal overhead)
- Connection execution already exists (no new overhead)
- Monitor execution time metrics

---

## Testing Strategy

### 8.1 Unit Tests

```rust
// tests/executor/become_config.rs
#[test]
fn test_become_precedence_task_wins() {
    let cfg = BecomeConfig::resolve(
        true, Some("task-user"),  // Task level
        Some(true), Some("play-user"),  // Play level
        true, "cli-user", "sudo"  // CLI level
    );
    assert_eq!(cfg.enabled, true);
    assert_eq!(cfg.user, "task-user");  // Task wins
}

#[test]
fn test_become_precedence_play_wins() {
    let cfg = BecomeConfig::resolve(
        false, None,  // Task level (disabled)
        Some(true), Some("play-user"),  // Play level
        true, "cli-user", "sudo"  // CLI level
    );
    assert_eq!(cfg.enabled, true);
    assert_eq!(cfg.user, "play-user");  // Play wins
}

// tests/executor/command_module.rs
#[tokio::test]
async fn test_command_real_execution() {
    let mock_conn = Arc::new(MockConnection::new());
    mock_conn.expect_execute()
        .with(eq("echo test"), any())
        .returning(|_, _| Ok(CommandResult {
            exit_code: 0,
            stdout: "test\n".to_string(),
            stderr: String::new(),
            success: true,
        }));

    let ctx = ExecutionContext {
        connection: Some(mock_conn),
        // ...
    };

    let task = Task::new("test", "command").arg("cmd", "echo test");
    let result = task.execute_command(&args, &ctx, &runtime).await.unwrap();

    assert_eq!(result.status, TaskStatus::Changed);
    assert!(result.result.is_some());
    // Verify real stdout returned
    let data = result.result.unwrap();
    assert_eq!(data["stdout"], "test\n");
}
```

### 8.2 Integration Tests

```yaml
# tests/integration/become.yml
---
- name: Test become precedence
  hosts: localhost
  become: true
  become_user: play-user
  tasks:
    - name: CLI become should be overridden by play
      command: whoami
      register: play_level

    - name: Task become should override play
      command: whoami
      become_user: task-user
      register: task_level

    - name: Verify precedence
      assert:
        that:
          - play_level.stdout == "play-user"
          - task_level.stdout == "task-user"
```

```rust
// tests/integration/become_integration.rs
#[tokio::test]
async fn test_become_end_to_end() {
    // Run playbook with become at all levels
    let result = run_playbook("tests/integration/become.yml", &[
        "--become",
        "--become-user", "cli-user"
    ]).await.unwrap();

    assert!(result.success);
    // Verify task-level become took precedence
}
```

### 8.3 Manual Testing Checklist

- [ ] Run playbook with `--become` on SSH host
- [ ] Verify `sudo` is invoked (check process list)
- [ ] Run playbook with task-level become
- [ ] Verify task become overrides CLI become
- [ ] Run with check mode (`--check`)
- [ ] Verify no actual commands executed
- [ ] Run with missing Ansible installation
- [ ] Verify clear error message with fix instructions
- [ ] Run with module not found
- [ ] Verify actionable error with searched paths

---

## Appendix A: File Change Summary

```
Modified Files:
  src/executor/become.rs (NEW)              +150 lines
  src/executor/runtime.rs                   +10 lines
  src/executor/task.rs                      +80 lines, -200 lines (net -120)
  src/executor/mod.rs                       +3 error variants
  src/connection/mod.rs                     +20 lines
  src/cli/commands/run.rs                   +15 lines

Test Files:
  tests/unit/executor/become_config.rs (NEW)       +200 lines
  tests/unit/executor/command_module.rs (NEW)      +150 lines
  tests/integration/become.yml (NEW)               +50 lines
  tests/integration/become_integration.rs (NEW)    +100 lines

Documentation:
  docs/user-guide/become.md (NEW)                  +300 lines
  docs/architecture/p0-features-design.md (THIS)   +800 lines
```

---

## Appendix B: Architecture Decision Records

### ADR 1: Become Config as Separate Type

**Decision:** Create `BecomeConfig` struct instead of adding fields to `ExecutionContext`

**Rationale:**
- Encapsulates all become-related logic in one place
- Easier to test precedence resolution in isolation
- Can be passed around independently
- Future-proof for additional become methods (su, doas, pbrun, etc.)

**Alternatives Considered:**
- Add individual become fields to `ExecutionContext` - rejected (poor cohesion)
- Use tuples/primitives - rejected (not self-documenting)

### ADR 2: Hard Errors for Missing Modules

**Decision:** Fail with `ModuleNotFound` error instead of simulating

**Rationale:**
- Correctness: users must know when modules don't exist
- Safety: prevents false "changed" reporting
- Trust: users can rely on execution results
- Debugging: clear error messages aid troubleshooting

**Alternatives Considered:**
- Warn and skip - rejected (silently fails playbooks)
- Simulate and log warning - rejected (breaks changed_when, register)

### ADR 3: Connection Required for Python Modules

**Decision:** Require connection for all Python module execution

**Rationale:**
- Consistency: all remote modules need connections
- Safety: prevents localhost execution by accident
- Clarity: error messages guide users to fix configuration

**Alternatives Considered:**
- Allow localhost fallback - rejected (confusing, breaks expectations)
- Auto-detect localhost - rejected (too much magic)

---

## Appendix C: Example Playbooks

### Example 1: Multi-Level Become

```yaml
---
- name: Deploy application
  hosts: webservers
  become: true  # Play-level: run as root
  become_user: root
  vars:
    app_user: appuser

  tasks:
    - name: Install system package
      apt:
        name: nginx
        state: present
      # Inherits play become (root)

    - name: Deploy application code
      copy:
        src: app/
        dest: /opt/app/
      become_user: "{{ app_user }}"  # Task-level: run as appuser

    - name: Restart service
      service:
        name: nginx
        state: restarted
      # Inherits play become (root)
```

### Example 2: Check Mode with Become

```yaml
---
- name: System updates
  hosts: all
  become: true

  tasks:
    - name: Update all packages
      apt:
        upgrade: dist
      check_mode: yes  # Will show what would be updated
      register: updates

    - name: Show planned updates
      debug:
        var: updates
```

---

## Conclusion

This design provides a comprehensive, production-ready implementation of become (privilege escalation) and removes all simulated module execution paths. The architecture ensures:

1. **Correctness:** Real execution with accurate results
2. **Safety:** Check mode prevents side effects, clear errors guide users
3. **Maintainability:** Clean separation of concerns, testable components
4. **Future-proof:** Extensible for additional become methods and modules

**Estimated Implementation Time:** 3-4 weeks
**Lines of Code:** ~700 new, ~200 removed (net +500)
**Test Coverage Target:** >80%
**Definition of Done:** All acceptance criteria met, integration tests passing, documentation complete
