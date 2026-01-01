# P0 Features Implementation Summary

**Quick Reference for Issues #52 and #53**

## Overview

Two critical P0 features blocking the M1 milestone:

1. **Issue #52:** Implement `become` end-to-end (privilege escalation)
2. **Issue #53:** Remove simulated module execution

**Full Design:** [p0-features-design.md](./p0-features-design.md)
**Diagrams:** [p0-features-diagrams.md](./p0-features-diagrams.md)

---

## Current Problems

### 1. Become (Privilege Escalation)

**Broken Flow:**
```
CLI --become flag → RunArgs → ❌ DISCARDED
Task.become field → ❌ HARDCODED to false in ModuleContext
Connection has escalation → ❌ NEVER USED
```

**Impact:** Users expect `become: true` to execute as sudo, but it doesn't work.

### 2. Simulated Execution

**Problematic Code Paths:**
```
execute_command()     → Returns changed=true with empty stdout/stderr ❌
execute_package()     → Returns changed=true without installing anything ❌
execute_service()     → Returns changed=true without managing service ❌
Python module fallback → Returns changed=true with "(simulated)" message ❌
```

**Impact:** Breaks trust, makes `register` unreliable, confuses `changed_when`.

---

## Solution Architecture

### Feature 1: Become Implementation

**New Component:**
```rust
// src/executor/become.rs
pub struct BecomeConfig {
    pub enabled: bool,
    pub method: String,   // "sudo", "su", "doas", etc.
    pub user: String,     // "root", "admin", etc.
    pub password: Option<String>,
    pub flags: Option<Vec<String>>,
}

impl BecomeConfig {
    // Resolve with correct precedence: Task > Play > CLI > Default
    pub fn resolve(task, play, cli) -> Self { /* ... */ }

    // Convert to connection ExecuteOptions
    pub fn to_execute_options(&self) -> Option<String> { /* ... */ }
}
```

**Data Flow:**
```
CLI args → Play config → Task config
     ↓
BecomeConfig::resolve()
     ↓
ExecutionContext.become
     ↓
ModuleContext (become fields populated)
     ↓
ExecuteOptions (escalation field set)
     ↓
Connection::execute() wraps with "sudo -u <user>"
```

**Key Changes:**
1. Add `become: BecomeConfig` to `ExecutionContext`
2. Resolve become config in `Task::execute()` using precedence rules
3. Pass resolved config to `ModuleContext` (replace hardcoded `false`)
4. Convert to `ExecuteOptions` when calling `connection.execute()`
5. CLI remote exec uses `ExecuteOptions::from_become()`

### Feature 2: Remove Simulated Execution

**Execution Strategy:**
```
Module requested
    ↓
Is it native (debug, copy, file, template)? ──YES──> Execute natively
    ↓ NO
Is it in Ansible paths (package, service)? ──YES──┐
    ↓ NO                                          ↓
ERROR: ModuleNotFound                    Has connection? ──YES──> Execute via Python
    "Module 'xyz' not found"                     ↓ NO
    "Install Ansible or set ANSIBLE_LIBRARY"     ERROR: ConnectionRequired
    "Searched: /usr/share/..., ~/.ansible/..."      "Module 'xyz' needs connection"
```

**Key Changes:**
1. **command/shell:** Call `connection.execute()` for real execution
2. **Remove stub methods:** Delete `execute_package`, `execute_service`, `execute_user`, `execute_group`, `execute_lineinfile`, `execute_blockinfile`
3. **Python fallback hardening:** Require connection, fail with clear errors if unavailable
4. **Error types:** Add `ModuleNotFound`, `ConnectionRequired` variants

**No More Simulation:**
- ❌ `TaskResult::changed()` without real work
- ❌ "Would execute..." debug messages as results
- ❌ `(simulated - no connection)` messages
- ✅ Real execution or hard errors with fix instructions

---

## Implementation Phases

### Phase 1: Foundation (Week 1)
- Create `BecomeConfig` type
- Add to `ExecutionContext`
- Implement precedence resolution
- Unit tests for precedence

**Deliverable:** `BecomeConfig::resolve()` works correctly

### Phase 2: Connection Integration (Week 1-2)
- Update `ModuleContext` creation (use resolved become)
- CLI integration (`ExecuteOptions::from_become()`)
- Test with SSH and local connections

**Deliverable:** `--become` flag works end-to-end

### Phase 3: Real Command Execution (Week 2)
- Rewrite `execute_command()` to use real `connection.execute()`
- Add error handling for missing connection
- Test check mode behavior

**Deliverable:** `command` module executes real commands

### Phase 4: Python Fallback Hardening (Week 3)
- Remove all stub methods
- Require connection for Python modules
- Improve error messages

**Deliverable:** No simulated execution paths remain

### Phase 5: Testing & Docs (Week 3-4)
- Integration tests for become precedence
- Integration tests for module execution
- Update user documentation

**Deliverable:** Full test coverage, production-ready

---

## Critical Integration Points

### Point 1: CLI → Executor
```rust
// src/cli/commands/run.rs
let execute_opts = if self.r#become {
    Some(ExecuteOptions::new()
        .with_escalation(Some(self.become_user.clone())))
} else {
    None
};

let result = conn.execute(cmd, execute_opts).await?;  // ✅ Pass options
```

### Point 2: Task → ModuleContext
```rust
// src/executor/task.rs
let become = BecomeConfig::resolve(
    self.r#become,              // Task level
    self.become_user.as_deref(),
    play.r#become,              // Play level
    play.become_user.as_deref(),
    cli_args.r#become,          // CLI level
    &cli_args.become_user,
    &cli_args.become_method,
);

let module_ctx = ModuleContext {
    r#become: become.enabled,                      // ✅ Use resolved
    become_user: Some(become.user.clone()),        // ✅ Use resolved
    become_method: Some(become.method.clone()),    // ✅ Use resolved
    connection: ctx.connection.clone(),
    // ... other fields
};
```

### Point 3: Module → Connection
```rust
// In native modules
if let Some(ref connection) = ctx.connection {
    let opts = if ctx.r#become {
        Some(ExecuteOptions::new()
            .with_escalation(ctx.become_user.clone()))
    } else {
        None
    };

    let result = connection.execute(cmd, opts).await?;  // ✅ Real execution
    // Return real result with actual stdout/stderr/rc
} else {
    return Err(ModuleError::ConnectionRequired);  // ✅ Hard error
}
```

---

## Risk Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Breaking existing tests | High | Medium | Update incrementally, use feature flags if needed |
| SSH sudo password prompts | Medium | High | Document NOPASSWD requirement, implement --ask-become-pass later |
| Missing connection in tests | Low | High | Always check `ctx.connection`, clear error messages |
| Python module incompatibility | Medium | Medium | Test with multiple Ansible versions, document requirements |

---

## Success Criteria

### Become Feature
- [ ] CLI `--become` flag causes remote commands to execute as specified user
- [ ] Task-level `become: true` overrides play and CLI settings
- [ ] Play-level become overrides CLI but not task settings
- [ ] Precedence: Task > Play > CLI > Default (documented and tested)
- [ ] Check mode does not execute privileged commands
- [ ] Integration test with 3-level become hierarchy passes

### Remove Simulation Feature
- [ ] Zero execution paths return `changed=true` without real work
- [ ] `command` module returns real stdout/stderr/exit_code
- [ ] Missing modules fail with `ModuleNotFound` error and fix instructions
- [ ] Python modules require connection or fail with `ConnectionRequired`
- [ ] All stub methods (`execute_package`, etc.) removed
- [ ] Error messages include searched paths and actionable fixes

---

## Quick File Reference

**New Files:**
- `src/executor/become.rs` - BecomeConfig type and resolution
- `tests/unit/executor/become_config.rs` - Unit tests
- `tests/integration/become.yml` - Integration test playbook

**Modified Files:**
- `src/executor/runtime.rs` - Add `become` to ExecutionContext
- `src/executor/task.rs` - Resolve become, real execution, remove stubs (~80 new, ~200 removed)
- `src/connection/mod.rs` - ExecuteOptions helpers
- `src/cli/commands/run.rs` - CLI integration (~15 new)

**Lines of Code:**
- ~700 added
- ~200 removed
- Net: +500 LOC

**Time Estimate:** 3-4 weeks

---

## Testing Checklist

**Manual Tests:**
- [ ] Run playbook with `--become` on SSH host
- [ ] Verify `sudo` invoked (check process list)
- [ ] Run with task-level become, verify override
- [ ] Run with check mode, verify no execution
- [ ] Run with missing Ansible, verify error message
- [ ] Run with unknown module, verify actionable error

**Automated Tests:**
- [ ] Become precedence (task > play > CLI)
- [ ] Real command execution (stdout/stderr/rc)
- [ ] Error cases (missing module, missing connection)
- [ ] Check mode behavior (no side effects)
- [ ] Integration: full playbook with become hierarchy

---

## Next Steps

1. **Review this design** with team
2. **Create implementation tasks** in issue tracker
3. **Start Phase 1:** BecomeConfig implementation
4. **Regular check-ins** to ensure alignment
5. **Update design** as issues discovered

---

**Questions or concerns?** See full design document for detailed architecture, diagrams, and ADRs.

**Last Updated:** 2026-01-01
**Status:** Design Complete - Ready for Implementation
