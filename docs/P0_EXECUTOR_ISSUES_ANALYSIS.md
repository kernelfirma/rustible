# P0 Executor Issues Analysis

## Executive Summary

This document analyzes four critical (P0) issues related to the executor module in rustible. The analysis covers the current implementation state, identifies the specific problems, and provides detailed implementation recommendations for each issue.

---

## Issue #48: Make 'rustible run' use executor as single runtime

### Current State

**Location:** `src/cli/commands/run.rs` and `src/executor/mod.rs`

**Problem:** The `rustible run` command currently creates its own ad-hoc execution logic instead of using the centralized `Executor` from `src/executor/mod.rs`. This creates code duplication and inconsistency.

#### Current Implementation Analysis

1. **Run command (lines 82-186):**
   - Creates its own playbook execution loop (line 151-164)
   - Manually handles stats tracking with `Arc<Mutex<RecapStats>>` (line 148)
   - Implements its own execution flow for plays (line 697-931)
   - Has duplicate task execution logic (line 979-1083)

2. **Executor module (src/executor/mod.rs):**
   - Properly implements playbook execution (lines 422-501)
   - Handles recovery manager integration (lines 429-437, 479-498)
   - Has comprehensive result tracking with `HostResult` (lines 348-357)
   - Already has all the execution strategies (Linear, Free, HostPinned)

### Recommended Changes

**Step 1: Modify `RunArgs::execute()` to use `Executor`**

```rust
// In src/cli/commands/run.rs, replace execute() method:
pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
    let start_time = Instant::now();

    // Initialize progress bars
    ctx.output.init_progress();

    // Load playbook (existing code lines 88-112)
    let playbook_content = std::fs::read_to_string(&self.playbook)?;
    let playbook: serde_yaml::Value = serde_yaml::from_str(&playbook_content)?;

    // Parse extra vars (existing code line 132)
    let extra_vars_yaml = ctx.parse_extra_vars()?;

    // Convert extra_vars from serde_yaml::Value to serde_json::Value
    let mut extra_vars = HashMap::new();
    for (key, value) in extra_vars_yaml {
        if let Ok(json_value) = serde_yaml::to_value(&value)
            .and_then(|v| serde_json::from_str(&serde_json::to_string(&v).unwrap()))
        {
            extra_vars.insert(key, json_value);
        }
    }

    // Create executor config
    let executor_config = rustible::executor::ExecutorConfig {
        forks: ctx.forks,
        check_mode: ctx.check_mode,
        diff_mode: ctx.diff_mode,
        verbosity: ctx.verbosity,
        strategy: rustible::executor::ExecutionStrategy::Linear, // or from config
        task_timeout: ctx.timeout,
        gather_facts: true,
        extra_vars,
    };

    // Create RuntimeContext from inventory
    let runtime_ctx = if let Some(inv_path) = ctx.inventory() {
        let inventory = rustible::inventory::Inventory::from_file(inv_path)?;
        rustible::executor::runtime::RuntimeContext::from_inventory(&inventory)
    } else {
        rustible::executor::runtime::RuntimeContext::new()
    };

    // Create executor with runtime context
    let executor = rustible::executor::Executor::with_runtime(executor_config, runtime_ctx);

    // TODO: Convert parsed playbook YAML to Playbook struct
    // This requires creating a parser for the Playbook type
    // let playbook = parse_playbook(playbook_content)?;

    // Execute playbook
    let results = executor.run_playbook(&playbook).await?;

    // Close connections
    ctx.close_connections().await;

    // Print recap from executor results
    let stats = rustible::executor::Executor::summarize_results(&results);
    ctx.output.recap(&convert_stats(&stats));

    // Print timing
    let duration = start_time.elapsed();
    ctx.output.info(&format!("Playbook finished in {:.2}s", duration.as_secs_f64()));

    // Return exit code
    if stats.failed > 0 {
        Ok(2)
    } else {
        Ok(0)
    }
}
```

**Step 2: Remove duplicate execution logic**

Lines to remove from `src/cli/commands/run.rs`:
- `execute_play()` method (lines 697-931)
- `execute_task()` method (lines 979-1083)
- `run_linear()`, `run_free()`, etc. logic (if present)

**Step 3: Create playbook parser**

Need to add a function to convert `serde_yaml::Value` to `executor::playbook::Playbook`:

```rust
// New file: src/cli/commands/playbook_parser.rs
use rustible::executor::playbook::{Playbook, Play, Task};
use anyhow::Result;

pub fn parse_playbook(yaml: serde_yaml::Value) -> Result<Playbook> {
    // Parse YAML structure into Playbook
    // This would read the plays array and construct the proper types
    todo!("Implement YAML to Playbook conversion")
}
```

---

## Issue #49: Fix inventory→RuntimeContext wiring for host/group resolution

### Current State

**Location:** `src/executor/runtime.rs` and `src/cli/commands/run.rs`

**Problem:** The RuntimeContext doesn't properly populate inventory data from the inventory file, leading to incorrect host and group resolution.

#### Current Implementation Analysis

1. **RuntimeContext::from_inventory() (lines 328-342):**
   ```rust
   pub fn from_inventory(inventory: &crate::inventory::Inventory) -> Self {
       let mut ctx = Self::new();

       // Only adds host variables, doesn't add hosts to all_hosts!
       for host in inventory.hosts() {
           for (key, value) in &host.vars {
               if let Ok(json_value) = serde_json::to_value(value) {
                   ctx.set_host_var(host.name(), key.clone(), json_value);
               }
           }
       }

       ctx
   }
   ```

   **Missing:**
   - Not calling `ctx.add_host()` to populate `all_hosts` vector
   - Not adding groups from inventory
   - Not populating group variables

2. **Inventory structure (in `src/inventory`):**
   - Has `Inventory::hosts()` method
   - Has `Inventory::groups()` method (likely)
   - Has host and group data that needs to be transferred

3. **Run command resolves hosts incorrectly (lines 934-977):**
   - Has its own `resolve_hosts()` method
   - Doesn't use RuntimeContext's host resolution
   - Manually parses inventory file again (lines 944-966)

### Recommended Changes

**Step 1: Fix RuntimeContext::from_inventory()**

```rust
// In src/executor/runtime.rs, replace from_inventory():
pub fn from_inventory(inventory: &crate::inventory::Inventory) -> Self {
    let mut ctx = Self::new();

    // Add all hosts to the context
    for host in inventory.hosts() {
        // Add host to all_hosts and create HostVars entry
        ctx.add_host(host.name().to_string(), None);

        // Set host variables
        for (key, value) in &host.vars {
            if let Ok(json_value) = serde_json::to_value(value) {
                ctx.set_host_var(host.name(), key.clone(), json_value);
            }
        }
    }

    // Add all groups
    for group in inventory.groups() {
        let mut group_data = InventoryGroup {
            hosts: vec![],
            vars: IndexMap::new(),
            children: vec![],
        };

        // Add hosts to this group
        for host_name in group.hosts() {
            group_data.hosts.push(host_name.to_string());
            // Also add the host to the group in the context
            if !ctx.all_hosts.contains(&host_name.to_string()) {
                ctx.add_host(host_name.to_string(), Some(group.name()));
            }
        }

        // Add group variables
        for (key, value) in &group.vars {
            if let Ok(json_value) = serde_json::to_value(value) {
                group_data.vars.insert(key.clone(), json_value);
            }
        }

        // Add child groups
        for child in group.children() {
            group_data.children.push(child.to_string());
        }

        ctx.add_group(group.name().to_string(), group_data);
    }

    ctx
}
```

**Step 2: Use RuntimeContext's resolve_hosts in executor**

The executor already has `resolve_hosts()` at lines 1374-1406. This should work correctly once RuntimeContext is properly populated.

**Step 3: Remove duplicate resolution from run.rs**

After Issue #48 is fixed (using executor), the `resolve_hosts()` method in `run.rs` (lines 933-977) can be removed entirely.

---

## Issue #50: Wire real connections into executor ExecutionContext + ModuleContext

### Current State

**Location:** `src/executor/runtime.rs`, `src/executor/task.rs`, and `src/modules/mod.rs`

**Problem:** Connections are available in CommandContext but not being passed through to ExecutionContext and ModuleContext where modules actually execute.

#### Current Implementation Analysis

1. **CommandContext has connection pool (src/cli/commands/mod.rs:49-50):**
   ```rust
   pub connections: Arc<RwLock<HashMap<String, Arc<dyn Connection + Send + Sync>>>>,
   ```

2. **ExecutionContext has connection field (src/executor/runtime.rs:214-215):**
   ```rust
   pub connection: Option<Arc<dyn Connection>>,
   ```

   But the `ExecutionContext::new()` method (lines 237-246) doesn't set it:
   ```rust
   pub fn new(host: impl Into<String>) -> Self {
       Self {
           host: host.into(),
           check_mode: false,
           diff_mode: false,
           verbosity: 0,
           connection: None,  // Always None!
           python_interpreter: "/usr/bin/python3".to_string(),
       }
   }
   ```

3. **ModuleContext has connection field (src/modules/mod.rs:520-521):**
   ```rust
   pub connection: Option<Arc<dyn Connection + Send + Sync>>,
   ```

4. **Task execution creates ExecutionContext (src/executor/mod.rs:877-880, 988-991, etc.):**
   ```rust
   let ctx = ExecutionContext::new(host.clone())
       .with_check_mode(check_mode)
       .with_diff_mode(diff_mode)
       .with_verbosity(verbosity);
   // connection is never set!
   ```

5. **RunArgs gets connections (src/cli/commands/run.rs:1266-1287):**
   ```rust
   let conn = ctx.get_connection(
       host,
       &ansible_host,
       &ansible_user,
       ansible_port,
       ansible_key.as_deref(),
   ).await?;
   ```

   But this is in the run command's own execution, not used by executor!

### Recommended Changes

**Step 1: Pass connection pool to Executor**

```rust
// In src/executor/mod.rs, modify Executor struct:
pub struct Executor {
    config: ExecutorConfig,
    runtime: Arc<RwLock<RuntimeContext>>,
    handlers: Arc<RwLock<HashMap<String, Handler>>>,
    notified_handlers: Arc<Mutex<HashSet<String>>>,
    semaphore: Arc<Semaphore>,
    parallelization_manager: Arc<ParallelizationManager>,
    recovery_manager: Option<Arc<RecoveryManager>>,
    // NEW: Connection pool
    connection_pool: Option<Arc<RwLock<HashMap<String, Arc<dyn Connection + Send + Sync>>>>>,
}

impl Executor {
    // NEW: Constructor with connection pool
    pub fn with_connection_pool(
        mut self,
        pool: Arc<RwLock<HashMap<String, Arc<dyn Connection + Send + Sync>>>>
    ) -> Self {
        self.connection_pool = Some(pool);
        self
    }
}
```

**Step 2: Get connection in executor before task execution**

```rust
// In src/executor/mod.rs, modify run_task_on_hosts() around line 1200:
async fn run_task_on_hosts(...) -> ExecutorResult<HashMap<String, TaskResult>> {
    // ...existing code...

    // NEW: Get connection for this host
    let connection = if let Some(pool) = &self.connection_pool {
        pool.read().await.get(host).cloned()
    } else {
        None
    };

    let ctx = ExecutionContext::new(host.clone())
        .with_check_mode(self.config.check_mode)
        .with_diff_mode(self.config.diff_mode)
        .with_verbosity(self.config.verbosity);

    // NEW: Add connection to context
    let ctx = if let Some(conn) = connection {
        ctx.with_connection(conn)
    } else {
        ctx
    };

    // ...rest of execution...
}
```

**Step 3: Pass connection from ExecutionContext to ModuleContext**

```rust
// In src/executor/task.rs, in Task::execute() method:
// Find where ModuleContext is created and add:

let module_ctx = ModuleContext::new()
    .with_check_mode(exec_ctx.check_mode)
    .with_diff_mode(exec_ctx.diff_mode)
    .with_verbosity(exec_ctx.verbosity)
    .with_vars(merged_vars);

// NEW: Pass connection from ExecutionContext
let module_ctx = if let Some(conn) = &exec_ctx.connection {
    module_ctx.with_connection(Arc::clone(conn))
} else {
    module_ctx
};
```

**Step 4: Use connection in run.rs when creating executor**

```rust
// In src/cli/commands/run.rs, in execute():
let executor = rustible::executor::Executor::with_runtime(executor_config, runtime_ctx)
    .with_connection_pool(Arc::clone(&ctx.connections));
```

---

## Issue #51: Fix --extra-vars precedence (executor stores extra vars incorrectly)

### Current State

**Location:** `src/executor/mod.rs` and `src/executor/runtime.rs`

**Problem:** Extra vars are being stored as global vars instead of in the dedicated `extra_vars` field, which breaks the precedence hierarchy.

#### Current Implementation Analysis

1. **Executor::run_playbook() (lines 443-451):**
   ```rust
   // Set playbook-level variables
   {
       let mut runtime = self.runtime.write().await;
       for (key, value) in &playbook.vars {
           runtime.set_global_var(key.clone(), value.clone());
       }
       // Add extra vars (highest precedence)
       for (key, value) in &self.config.extra_vars {
           runtime.set_global_var(key.clone(), value.clone());  // WRONG!
       }
   }
   ```

   **Problem:** Using `set_global_var()` instead of `set_extra_var()`

2. **RuntimeContext precedence (lines 418-464):**
   - The `get_var()` method correctly checks `extra_vars` first (lines 422-424)
   - The `get_var_with_full_precedence()` method also checks `extra_vars` first (lines 906-909)
   - But extra vars are being set in the wrong place!

3. **Variable precedence hierarchy should be (from RuntimeContext:899-898):**
   ```
   12. Extra vars (highest precedence)
   11. Include params
   10. Role params
   9.  Registered variables and set_fact
   8.  Include vars
   7.  Task variables
   6.  Block vars
   5.  Play variables
   4.  Global (playbook) variables
   3.  Host variables from inventory
   2.  Group variables
   1.  Role defaults (lowest)
   ```

4. **In RunArgs::execute_play() (lines 732-739):**
   ```rust
   // Add extra vars first (lowest precedence in this context)
   if let Ok(extra_vars) = ctx.parse_extra_vars() {
       for (k, v) in extra_vars {
           if let Ok(yaml_val) = serde_yaml::to_value(&v) {
               vars.insert(k, yaml_val);  // Treating as lowest precedence!
           }
       }
   }
   ```

   **Problem:** Comment says "lowest precedence" but should be highest!

### Recommended Changes

**Step 1: Fix Executor::run_playbook()**

```rust
// In src/executor/mod.rs, replace lines 443-451:
// Set playbook-level variables
{
    let mut runtime = self.runtime.write().await;

    // Add playbook vars as global vars (correct)
    for (key, value) in &playbook.vars {
        runtime.set_global_var(key.clone(), value.clone());
    }

    // FIXED: Add extra vars to extra_vars field (highest precedence)
    for (key, value) in &self.config.extra_vars {
        runtime.set_extra_var(key.clone(), value.clone());
    }
}
```

**Step 2: Fix Executor::run_play()**

```rust
// In src/executor/mod.rs, around line 528-539:
// Set play-level variables
{
    let mut runtime = self.runtime.write().await;

    // Play vars
    for (key, value) in &play.vars {
        runtime.set_play_var(key.clone(), value.clone());
    }

    // Role variables
    for role in &play.roles {
        for (key, value) in role.get_all_vars() {
            runtime.set_play_var(key.clone(), value.clone());
        }
    }

    // NOTE: Extra vars are already set at playbook level,
    // don't set them again here or precedence will break
}
```

**Step 3: Remove incorrect extra-vars handling from run.rs**

After Issue #48 is fixed, the `execute_play()` method in run.rs will be removed, so lines 732-739 will no longer exist.

**Step 4: Verify precedence in RuntimeContext**

The existing precedence methods are already correct:
- `get_var()` checks `extra_vars` first (line 422)
- `get_var_with_full_precedence()` checks `extra_vars` first (line 906)
- `get_merged_vars()` adds `extra_vars` last (line 557-560), overwriting lower precedence

No changes needed in RuntimeContext - the bug is only in how Executor calls it.

---

## Implementation Order

**Recommended sequence to minimize conflicts:**

1. **Issue #51 first** - Simple fix, independent of others
   - Fix `set_extra_var()` calls in `Executor::run_playbook()`
   - Update `Executor::run_play()` to not re-set extra vars
   - Test with existing code

2. **Issue #49 second** - Fix inventory wiring
   - Update `RuntimeContext::from_inventory()`
   - Test host/group resolution works
   - This will make Issue #48 easier

3. **Issue #50 third** - Wire connections
   - Add connection pool to Executor
   - Pass connections through to ExecutionContext and ModuleContext
   - Test that modules can use connections

4. **Issue #48 last** - Refactor run command
   - Create playbook parser
   - Update RunArgs::execute() to use Executor
   - Remove duplicate code
   - This touches the most code and benefits from the other fixes

---

## Testing Strategy

### For Issue #48:
```bash
# Test that run command still works
rustible run examples/webserver.yml -i inventory/hosts.yml

# Test with extra vars
rustible run examples/webserver.yml -i inventory/hosts.yml -e "port=8080"

# Test check mode
rustible run examples/webserver.yml -i inventory/hosts.yml --check
```

### For Issue #49:
```bash
# Test host resolution
rustible run test_playbook.yml -i inventory/multi_group.yml

# Test group resolution
# Playbook should have: hosts: webservers
rustible run group_test.yml -i inventory/groups.yml

# Test "all" pattern
# Playbook should have: hosts: all
```

### For Issue #50:
```bash
# Test that remote commands work
rustible run examples/command_test.yml -i inventory/hosts.yml

# Test file operations that need connections
rustible run examples/copy_test.yml -i inventory/hosts.yml

# Verify connection pooling works (check logs for "Reusing connection")
rustible run examples/multi_task.yml -i inventory/hosts.yml -vvv
```

### For Issue #51:
```bash
# Test extra vars override playbook vars
rustible run examples/vars_test.yml -e "app_name=override"

# Test extra vars from file
rustible run examples/vars_test.yml -e "@extra_vars.yml"

# Verify precedence: extra > play > global
# Should show extra vars winning
```

---

## Risk Assessment

### High Risk:
- **Issue #48**: Large refactor, touches main execution path
  - Mitigation: Do incrementally, keep old code path temporarily

### Medium Risk:
- **Issue #49**: Could break existing inventory functionality
  - Mitigation: Comprehensive tests with different inventory formats
- **Issue #50**: Connection handling is critical for remote execution
  - Mitigation: Test with both local and remote hosts

### Low Risk:
- **Issue #51**: Simple logic fix
  - Mitigation: Straightforward, easy to verify

---

## Files That Need Changes

### Issue #48:
- `src/cli/commands/run.rs` - Major refactor
- `src/executor/playbook.rs` - May need parser additions
- New file: `src/cli/commands/playbook_parser.rs`

### Issue #49:
- `src/executor/runtime.rs` - Update `from_inventory()`
- `src/inventory/mod.rs` - May need to expose groups()

### Issue #50:
- `src/executor/mod.rs` - Add connection_pool field, pass to tasks
- `src/executor/task.rs` - Pass connection to ModuleContext
- `src/executor/runtime.rs` - Already has connection field (no changes)
- `src/modules/mod.rs` - Already has connection field (no changes)

### Issue #51:
- `src/executor/mod.rs` - Fix `set_extra_var()` calls (2 locations)

---

## Estimated Effort

- **Issue #51**: 1-2 hours (simple fix + testing)
- **Issue #49**: 4-6 hours (inventory integration + testing)
- **Issue #50**: 6-8 hours (connection threading + testing)
- **Issue #48**: 12-16 hours (major refactor + parser + testing)

**Total**: 23-32 hours of development + testing time

---

## Success Criteria

### Issue #48:
- [ ] `rustible run` uses `Executor::run_playbook()`
- [ ] All duplicate execution code removed from run.rs
- [ ] Playbook YAML correctly parsed to Playbook struct
- [ ] All existing tests pass
- [ ] Performance is same or better

### Issue #49:
- [ ] Hosts from inventory correctly populate RuntimeContext
- [ ] Groups from inventory correctly populate RuntimeContext
- [ ] Host resolution works for: specific hosts, groups, "all", patterns
- [ ] Group variables correctly applied to hosts

### Issue #50:
- [ ] Connections from CommandContext reach ExecutionContext
- [ ] Connections from ExecutionContext reach ModuleContext
- [ ] Modules can execute remote commands using connections
- [ ] Connection pooling works (reuses connections)

### Issue #51:
- [ ] Extra vars stored in `extra_vars` field
- [ ] Extra vars have highest precedence (override everything)
- [ ] Variable precedence tests all pass
- [ ] Extra vars from file and command line both work
