---
summary: Using delegate_to and delegate_facts to execute tasks on different hosts while controlling where facts are stored.
read_when: Running tasks on localhost or different targets, gathering facts from remote hosts, or implementing jump-host patterns.
---

# Delegation Implementation for Rustible

## Overview
This document describes the implementation of `delegate_to` and `delegate_facts` directives for Rustible, providing Ansible-compatible task delegation functionality.

## Implementation Summary

### 1. Task Struct Changes

#### `/home/artur/Repositories/rustible/src/playbook.rs`
Added `delegate_facts` field to the Task struct:
```rust
/// Whether facts should be set on the delegated host instead of the original host
#[serde(skip_serializing_if = "Option::is_none")]
pub delegate_facts: Option<bool>,
```

Updated:
- Task struct initialization to include `delegate_facts: None`
- TaskModule deserializer to recognize `delegate_facts` as a known task field

#### `/home/artur/Repositories/rustible/src/executor/task.rs`
Added `delegate_facts` field to the executor Task struct:
```rust
/// Whether facts should be set on the delegated host instead of the original host
#[serde(default)]
pub delegate_facts: Option<bool>,
```

### 2. Task Execution Logic

Modified the `Task::execute()` method to handle delegation:

```rust
// Handle delegation - create a new context for the delegate host if needed
let (execution_ctx, fact_target_host) = if let Some(ref delegate_host) = self.delegate_to {
    debug!("Delegating task to host: {}", delegate_host);

    // Create new context for delegate host
    let mut delegate_ctx = ctx.clone();
    delegate_ctx.host = delegate_host.clone();

    // Determine where facts should be stored
    // If delegate_facts is true, store on delegate host; otherwise on original host
    let fact_host = if self.delegate_facts.unwrap_or(false) {
        delegate_host.clone()
    } else {
        ctx.host.clone()
    };

    (delegate_ctx, fact_host)
} else {
    (ctx.clone(), ctx.host.clone())
};
```

### 3. Behavior Specification

#### delegate_to
- When set, the task executes on the specified delegate host instead of the original target host
- The execution context (`ExecutionContext`) is updated to use the delegate host
- Connection and variables are resolved for the delegate host

#### delegate_facts
- Default: `false` (facts are stored on the original host)
- When `true`: Facts are stored on the delegate host
- When `false` or unset: Facts are stored on the original host (Ansible default behavior)

#### Registered Variables
- Registered variables (via `register:`) ALWAYS go to the original host, regardless of delegation
- This matches Ansible's behavior where the controlling host tracks task results

### 4. Test Coverage

Created comprehensive tests in `/home/artur/Repositories/rustible/tests/delegation_tests.rs`:

- `test_delegate_to_basic`: Basic delegation functionality
- `test_delegate_facts_false`: Facts stored on original host when delegate_facts=false
- `test_delegate_facts_true`: Facts stored on delegate host when delegate_facts=true
- `test_delegate_facts_default_false`: Default behavior (facts on original host)
- `test_delegate_with_register`: Registered vars always go to original host
- `test_no_delegation`: Normal execution without delegation

### 5. Key Design Decisions

1. **Execution Context**: Tasks execute with the delegate host's context, ensuring proper variable resolution and module execution
2. **Fact Storage**: Controlled by `delegate_facts`, defaulting to storing on original host (matching Ansible)
3. **Register Variables**: Always stored on original host for consistent playbook flow control
4. **Backward Compatibility**: `delegate_to` field already existed; `delegate_facts` is optional and defaults to false

## Usage Examples

### Basic Delegation
```yaml
- name: Run command on localhost
  command: echo "hello"
  delegate_to: localhost
```

### Delegation with Facts on Original Host (Default)
```yaml
- name: Gather facts from delegate but store on original
  setup:
  delegate_to: monitoring_server
  # delegate_facts defaults to false - facts go to inventory_hostname
```

### Delegation with Facts on Delegate Host
```yaml
- name: Gather facts and store on delegate
  setup:
  delegate_to: monitoring_server
  delegate_facts: true
  # Facts stored on monitoring_server instead of original host
```

### Delegation with Register
```yaml
- name: Check disk space on localhost
  command: df -h
  delegate_to: localhost
  register: disk_info
  # disk_info registered on original host, not localhost
```

## Compatibility

This implementation follows Ansible's delegation semantics:
- Tasks execute on delegate host with that host's connection and variables
- Facts storage controlled by `delegate_facts` (default: original host)
- Registered variables always go to original host
- Compatible with all existing Rustible modules

## Files Modified

1. `/home/artur/Repositories/rustible/src/playbook.rs`
   - Added `delegate_facts` field to Task struct
   - Updated Task::new() to initialize delegate_facts
   - Updated TaskModule deserializer

2. `/home/artur/Repositories/rustible/src/executor/task.rs`
   - Added `delegate_facts` field
   - Modified `execute()` method to handle delegation
   - Updated Default implementation

3. `/home/artur/Repositories/rustible/src/executor/mod.rs`
   - Updated handler Task creation to include delegate_facts

4. `/home/artur/Repositories/rustible/src/executor/playbook.rs`
   - Updated Task creation in playbook parsing

5. `/home/artur/Repositories/rustible/tests/delegation_tests.rs` (NEW)
   - Comprehensive test suite for delegation functionality

## Future Enhancements

1. Add `delegate_facts` to TaskDefinition parser for YAML playbook support
2. Implement connection pooling for frequently-used delegate hosts
3. Add metrics/logging for delegation operations
4. Support for `run_once` with delegation
5. Delegation loops (delegate_to with with_items)
