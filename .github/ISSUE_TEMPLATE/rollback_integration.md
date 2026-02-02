#173 [CRITICAL] Integrate Rollback Framework into Executor

## Problem Statement
Rustible has a `RollbackManager` framework (`src/recovery/rollback.rs`) and `RecoveryManager` (`src/recovery/`) that can track state changes and create rollback plans, but these are **not integrated into the actual playbook execution flow**. This means failed playbooks cannot automatically recover, leaving systems in potentially broken states.

## Current State
```rust
// RollbackManager exists but is NOT connected to Executor
pub struct RollbackManager {
    checkpoints: Vec<Checkpoint>,
    actions: Vec<Box<dyn RollbackAction>>,
}

// RecoveryManager exists but unused in main execution path
pub struct RecoveryManager {
    recovery_points: HashMap<String, RecoveryPoint>,
}
```

## Safety Impact
| Risk | Severity | Description |
|------|----------|-------------|
| Partial configuration | **High** | Failed playbooks leave systems partially configured |
| Database corruption | **Critical** | Database schema changes without rollback capability |
| Service downtime | **High** | Service restarts that fail leave services stopped |
| Security exposure | **Medium** | Firewall rules partially applied |

## Comparison to Alternatives
| Tool | Rollback Capability |
|------|---------------------|
| **Terraform** | `terraform destroy` + state versioning - complete resource rollback |
| **Ansible** | No native rollback; manual recovery required |
| **Rustible (current)** | Framework exists but **not connected** |

## Proposed Implementation

### Phase 1: Checkpoint Integration
- [ ] Create checkpoint before each task that supports rollback
- [ ] Store checkpoint in memory during execution
- [ ] Add checkpoint persistence option (JSON/state file)
- [ ] Implement checkpoint cleanup on successful completion

### Phase 2: Rollback Action Completion
Currently stubbed actions that need full implementation:
```rust
// src/recovery/rollback.rs - Currently warn-only implementations
impl RollbackAction for ServiceRollback { 
    // TODO: Actually restore service state
}
impl RollbackAction for PackageRollback {
    // TODO: Actually uninstall packages
}
impl RollbackAction for UserRollback {
    // TODO: Actually remove created users
}
```

- [ ] Complete `FileRollback` - Restore from backup files
- [ ] Complete `ServiceRollback` - Restore original service state
- [ ] Complete `PackageRollback` - Uninstall installed packages
- [ ] Complete `UserRollback` / `GroupRollback` - Remove created users/groups
- [ ] Add `ConfigurationRollback` - Restore configuration files
- [ ] Add `NetworkRollback` - Revert network changes

### Phase 3: Executor Integration
```rust
// src/executor/mod.rs
pub struct Executor {
    // Add rollback manager
    rollback_manager: Option<RollbackManager>,
    auto_rollback: bool,
}

impl Executor {
    async fn execute_task_with_rollback(&self, task: &Task, host: &Host) -> Result<()> {
        let checkpoint = self.create_checkpoint(task, host).await?;
        
        match self.execute_task(task, host).await {
            Ok(result) => {
                self.commit_checkpoint(checkpoint).await?;
                Ok(result)
            }
            Err(e) if self.auto_rollback => {
                self.rollback_manager.rollback_to(checkpoint.id).await?;
                Err(e)
            }
            Err(e) => Err(e),
        }
    }
}
```

- [ ] Add `--auto-rollback` CLI flag
- [ ] Add `auto_rollback: true/false` config option
- [ ] Integrate checkpoint creation into task execution
- [ ] Trigger rollback on task failure (when enabled)
- [ ] Add rollback confirmation prompts for destructive operations

### Phase 4: CLI Commands
- [ ] `rustible rollback <playbook-id>` - Rollback a specific execution
- [ ] `rustible checkpoints list` - List available checkpoints
- [ ] `rustible checkpoints show <id>` - Show checkpoint details
- [ ] `rustible checkpoints delete <id>` - Clean up old checkpoints

## Acceptance Criteria
- [ ] Failed playbook can be automatically rolled back to pre-execution state
- [ ] Checkpoints do not significantly impact performance (<5% overhead)
- [ ] Rollback is idempotent (can rollback same checkpoint multiple times safely)
- [ ] Partial rollbacks supported (rollback to specific checkpoint)
- [ ] Integration tests demonstrate rollback functionality
- [ ] Documentation covers rollback usage and limitations

## Priority
**CRITICAL** - Essential for production safety; prevents partial configuration states

## Related
- Existing framework: `src/recovery/rollback.rs`, `src/recovery/manager.rs`
- Issue #170: Password material not zeroized (related to secure checkpoint storage)

## Labels
`critical`, `safety`, `recovery`, `production-readiness`
