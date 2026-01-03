---
summary: Rolling updates with serial execution using fixed batches, percentages, and progressive deployment with failure thresholds.
read_when: Implementing rolling updates, canary deployments, or controlling batch sizes for safe production changes.
---

# Serial Execution Parameter Implementation

## Overview

This document describes the implementation of the `serial` execution parameter for Rustible plays, which enables rolling updates and batched host execution.

## Implementation Summary

The serial execution parameter allows Ansible-like batched execution of plays across hosts, supporting:
- Fixed batch sizes (e.g., `serial: 2`)
- Percentage-based batches (e.g., `serial: "50%"`)
- Progressive batches (e.g., `serial: [1, 5, 10]`)
- Safety controls with `max_fail_percentage`

## Key Files Modified

### 1. `/home/artur/Repositories/rustible/src/playbook.rs`

**Added `SerialSpec` enum with implementation:**
```rust
pub enum SerialSpec {
    Fixed(usize),           // Fixed batch size
    Percentage(String),      // Percentage of hosts
    Progressive(Vec<SerialSpec>), // Progressive batch sizes
}
```

**Key methods:**
- `calculate_batches(&self, total_hosts: usize) -> Vec<usize>` - Calculates batch sizes for given host count
- `batch_hosts<'a>(&self, hosts: &'a [String]) -> Vec<&'a [String]>` - Splits hosts into batches

### 2. `/home/artur/Repositories/rustible/src/executor/playbook.rs`

**Updated Play struct:**
- Changed `serial: Option<usize>` to `serial: Option<crate::playbook::SerialSpec>`
- Added `convert_serial_value_to_spec()` function to convert YAML `SerialValue` to `SerialSpec`

### 3. `/home/artur/Repositories/rustible/src/executor/mod.rs`

**Modified `run_play()` method:**
- Added check for serial specification
- Routes to `run_serial()` if serial is specified
- Falls back to standard strategy execution if no serial

**Added `run_serial()` method:**
- Batches hosts according to serial specification
- Executes each batch sequentially
- Applies the configured strategy (linear/free/host_pinned) within each batch
- Tracks failures across all batches
- Implements `max_fail_percentage` safety control
- Aborts remaining batches if failure threshold exceeded

### 4. `/home/artur/Repositories/rustible/src/cli/commands/check.rs`

**Fixed missing field:**
- Added `plan: false` to `RunArgs` initialization in check mode

### 5. `/home/artur/Repositories/rustible/src/include.rs`

**Fixed Task API usage:**
- Updated `extract_import_tasks()` to use `task.module_name()` and `task.module_args()`
- Updated `extract_include_tasks()` to use correct Task API
- Updated `extract_include_vars()` to use correct Task API
- Fixed unused variable warnings

## Features Implemented

### 1. Fixed Batch Sizes
```yaml
serial: 2  # Execute on 2 hosts at a time
```

### 2. Percentage-Based Batches
```yaml
serial: "50%"  # Execute on 50% of hosts at a time
```
- Percentage values round up (e.g., 30% of 5 hosts = 2 hosts)
- Minimum batch size is always 1

### 3. Progressive Batches
```yaml
serial: [1, 5, 10]  # First batch: 1 host, next: 5 hosts, then: 10 hosts
```
- Can mix fixed and percentage values
- Cycles through batch sizes for remaining hosts

### 4. Max Fail Percentage
```yaml
max_fail_percentage: 25  # Abort if >25% of hosts fail
```
- Calculated across all batches
- Remaining hosts marked as skipped if threshold exceeded
- Works seamlessly with serial execution

### 5. Strategy Integration
Serial execution works with all strategies:
- **Linear**: All hosts in batch complete task before moving to next task
- **Free**: Each host in batch proceeds independently through tasks
- **Host-Pinned**: Dedicated worker per host within batch

## Test Coverage

Created comprehensive test suite in `/home/artur/Repositories/rustible/tests/serial_execution_tests.rs` with **29 tests**:

### Unit Tests (7 tests)
- `test_serial_spec_calculate_batches_fixed`
- `test_serial_spec_calculate_batches_percentage`
- `test_serial_spec_calculate_batches_percentage_rounds_up`
- `test_serial_spec_calculate_batches_progressive`
- `test_serial_spec_batch_hosts_fixed`
- `test_serial_spec_batch_hosts_uneven`
- `test_serial_spec_batch_hosts_progressive`

### Integration Tests

**Fixed Batch Size (3 tests):**
- `test_serial_fixed_one_host_at_a_time`
- `test_serial_fixed_batch_size_two`
- `test_serial_fixed_batch_size_larger_than_hosts`

**Percentage-Based (5 tests):**
- `test_serial_percentage_50_percent`
- `test_serial_percentage_25_percent`
- `test_serial_percentage_100_percent`
- `test_serial_percentage_rounds_up`
- `test_serial_percentage_zero`

**Progressive Batches (2 tests):**
- `test_serial_progressive_batches`
- `test_serial_progressive_with_percentages`

**Strategy Integration (3 tests):**
- `test_serial_with_linear_strategy`
- `test_serial_with_free_strategy`
- `test_serial_with_host_pinned_strategy`

**Max Fail Percentage (3 tests):**
- `test_serial_with_max_fail_percentage_not_exceeded`
- `test_serial_with_max_fail_percentage_exceeded`
- `test_serial_max_fail_percentage_zero`

**Edge Cases (4 tests):**
- `test_serial_with_zero_hosts`
- `test_serial_with_single_host`
- `test_serial_batch_size_zero`

**Complex Scenarios (3 tests):**
- `test_serial_rolling_update_with_handlers`
- `test_serial_with_conditional_tasks`
- `test_serial_multiple_plays`

### Test Results
```
running 29 tests
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Usage Examples

### Example 1: Rolling Update with Fixed Batch
```yaml
- name: Rolling Update Web Servers
  hosts: webservers
  serial: 2
  max_fail_percentage: 25
  tasks:
    - name: Update application
      copy:
        src: app.jar
        dest: /opt/app/
      notify: restart service

  handlers:
    - name: restart service
      service:
        name: myapp
        state: restarted
```

### Example 2: Canary Deployment with Progressive Batches
```yaml
- name: Canary Deployment
  hosts: production
  serial: [1, "10%", "50%", "100%"]
  max_fail_percentage: 10
  tasks:
    - name: Deploy new version
      docker_container:
        name: app
        image: "myapp:{{ version }}"
        state: started
```

### Example 3: Percentage-Based Rolling Update
```yaml
- name: Database Migration
  hosts: db_cluster
  serial: "25%"  # Update 25% of databases at a time
  tasks:
    - name: Run migration
      command: /opt/db/migrate.sh
```

## Implementation Details

### Batch Calculation Algorithm

1. **Fixed**: Returns the specified size directly
2. **Percentage**:
   - Parses percentage string (e.g., "50%" → 50.0)
   - Calculates `(total_hosts * percentage / 100).ceil()`
   - Ensures minimum batch size of 1
3. **Progressive**: Flattens all specs into a vector of batch sizes

### Host Batching Algorithm

1. Get batch sizes from `calculate_batches()`
2. Split hosts into slices:
   - For each batch, take `batch_size` hosts
   - Cycle through progressive batch sizes if needed
   - Handle remainder hosts in final batch
3. Return vector of host slices

### Execution Flow

```
run_play()
  ↓
  [Check serial?]
  ↓ Yes
  run_serial()
    ↓
    batch_hosts()
    ↓
    For each batch:
      ↓
      run_strategy() ← [linear/free/host_pinned]
      ↓
      Track failures
      ↓
      Check max_fail_percentage
      ↓
      [Continue or abort?]
  ↓
  Return aggregated results
```

### Failure Handling

- Failures tracked across all batches
- Current failure percentage = `(total_failed / total_hosts * 100)`
- If `current_fail_pct > max_fail_percentage`:
  - Log error message
  - Mark remaining hosts as skipped
  - Abort remaining batches
  - Return results for completed + skipped hosts

## Compatibility

- **Ansible-compatible**: Supports same serial syntax as Ansible
- **YAML parsing**: Handles number, string (percentage), and list formats
- **Backward compatible**: Plays without `serial` execute normally
- **Strategy agnostic**: Works with all execution strategies

## Performance Considerations

- Batches execute sequentially (by design for safety)
- Within each batch, hosts execute according to selected strategy
- Memory efficient: Uses slices instead of cloning host vectors
- Minimal overhead: Batch calculation is O(n) where n = number of hosts

## Future Enhancements

Potential areas for future development:
1. Parallel batch execution with `serial_batches` option
2. Dynamic batch sizing based on success rate
3. Batch-level timeouts
4. Custom batch selection strategies (e.g., by host attributes)
5. Batch pause/resume capability

## References

- Ansible serial documentation: https://docs.ansible.com/ansible/latest/playbook_guide/playbooks_strategies.html#setting-the-batch-size-with-serial
- Related issue/PR: Command injection fix (#14)
- Test file: `/home/artur/Repositories/rustible/tests/serial_execution_tests.rs`
