#175 [CRITICAL] Production-Ready Terraform-like State Management

## Problem Statement
Rustible's Terraform-like provisioning is marked "experimental and limited in scope." The state management system only supports local files, lacks remote backends, proper state locking, and workspaces. This prevents team-based infrastructure management and creates state corruption risks.

## Current State
| Feature | Status | Notes |
|---------|--------|-------|
| Local state file | ✅ Partial | Basic JSON storage |
| Remote state backends | ❌ Stubs only | S3/GCS/Azure interfaces defined |
| State locking | ⚠️ Incomplete | DynamoDB lock stub present |
| State versioning | ❌ Not implemented | No history/rollback |
| Workspaces | ❌ Not implemented | No environment separation |
| State import | ❌ Not implemented | Cannot adopt existing resources |
| `plan` workflow | ⚠️ Partial | `--check` mode only |

## Comparison to Terraform 1.5+
| Feature | Rustible | Terraform |
|---------|----------|-----------|
| Local state | ⚠️ Basic | ✅ Full with backup |
| S3 backend | ❌ Stub | ✅ Full with locking |
| GCS backend | ❌ Stub | ✅ Full with locking |
| Azure backend | ❌ Stub | ✅ Full with locking |
| HTTP backend | ❌ None | ✅ Generic HTTP backend |
| State locking | ⚠️ DynamoDB stub | ✅ Native per-backend |
| Workspaces | ❌ None | ✅ Full isolation |
| State versioning | ❌ None | ✅ Automatic backup |
| State import | ❌ None | ✅ `terraform import` |
| `plan` output | ⚠️ Basic diff | ✅ Full resource plans |
| Graph visualization | ❌ None | ✅ `terraform graph` |

## Safety Implications
- **State corruption**: No locking = concurrent modifications corrupt state
- **No recovery**: No versioning = state loss is unrecoverable
- **Team blocking**: Local state only = cannot share infrastructure state
- **Drift undetected**: No refresh mechanism = state becomes stale

## Proposed Implementation

### Phase 1: Backend Interface Completion
```rust
// src/provisioning/backend.rs
#[async_trait]
pub trait StateBackend: Send + Sync {
    async fn get_state(&self) -> Result<State, BackendError>;
    async fn put_state(&self, state: &State) -> Result<(), BackendError>;
    async fn delete_state(&self) -> Result<(), BackendError>;
    // Add missing methods:
    async fn lock(&self, info: LockInfo) -> Result<LockId, BackendError>;
    async fn unlock(&self, id: LockId) -> Result<(), BackendError>;
    async fn get_versions(&self) -> Result<Vec<StateVersion>, BackendError>;
}
```

- [ ] Complete `S3Backend` implementation (with DynamoDB locking)
- [ ] Complete `GcsBackend` implementation (with GCS-native locking)
- [ ] Complete `AzureBackend` implementation (with Azure blob leasing)
- [ ] Add `ConsulBackend` for HashiCorp Consul
- [ ] Add `PgBackend` for PostgreSQL (popular in self-hosted)

### Phase 2: State Locking
```rust
// src/provisioning/state_lock.rs
pub struct StateLockManager {
    backend: Arc<dyn StateBackend>,
    lock_ttl: Duration,
    refresh_interval: Duration,
}

impl StateLockManager {
    pub async fn acquire_lock(&self, operation: &str) -> Result<StateLock, LockError> {
        // Implement lock acquisition with retries
        // Implement lock refresh (keepalive)
        // Implement lock release (even on panic)
    }
}
```

- [ ] Complete DynamoDB lock table implementation
- [ ] Add lock acquisition with exponential backoff
- [ ] Implement lock refresh/keepalive
- [ ] Add lock force-unlock capability (`rustible force-unlock`)
- [ ] Implement lock timeout/expiration

### Phase 3: State Versioning
- [ ] Store previous state versions (configurable retention)
- [ ] Add `rustible state list` - Show state versions
- [ ] Add `rustible state show <version>` - View specific version
- [ ] Add `rustible state rollback <version>` - Restore previous state
- [ ] Add `rustible state rm <version>` - Clean up old versions

### Phase 4: Workspaces
```bash
# Equivalent to `terraform workspace`
rustible workspace list          # List workspaces
rustible workspace new prod      # Create workspace
rustible workspace select prod   # Switch workspace
rustible workspace delete prod   # Remove workspace
rustible workspace show          # Current workspace
```

- [ ] Implement workspace isolation (separate state per workspace)
- [ ] Add workspace configuration in `rustible.toml`
- [ ] Support workspace-specific variables

### Phase 5: Enhanced Plan/Apply
```bash
rustible plan                      # Show execution plan
rustible plan -out=plan.out        # Save plan to file
rustible apply                     # Apply changes
rustible apply plan.out            # Apply saved plan
rustible apply -target=aws_instance.web  # Target specific resource
rustible destroy                   # Destroy managed resources
```

- [ ] Enhance plan output with resource graphs
- [ ] Add plan file format (portable, signed)
- [ ] Implement targeted operations
- [ ] Add resource addressing syntax

### Phase 6: State Import
```bash
rustible import aws_instance.web i-abc123  # Import existing resource
rustible import -f imports.txt              # Bulk import
```

- [ ] Implement resource import for AWS resources
- [ ] Generate resource configuration from imported state
- [ ] Support bulk import from CSV/JSON

## Configuration
```toml
# rustible.toml
[provisioning]
backend = "s3"

[provisioning.s3]
bucket = "my-terraform-state"
key = "rustible/terraform.tfstate"
region = "us-east-1"
encrypt = true

[provisioning.s3.lock]
table = "terraform-state-lock"

[workspaces]
default = "default"
```

## Acceptance Criteria
- [ ] S3 backend with DynamoDB locking works in production
- [ ] Concurrent executions are safely serialized via locking
- [ ] State versioning allows recovery from bad changes
- [ ] Workspaces enable environment isolation
- [ ] `rustible plan` shows accurate resource changes
- [ ] State import allows adopting existing infrastructure
- [ ] Backend migration tools (move state between backends)

## Priority
**CRITICAL** - Required for team-based infrastructure management

## Related
- Issue #167: Resource graph state comparison TODO
- Issue #164: DynamoDB state lock operations (marked resolved but incomplete)

## Labels
`critical`, `provisioning`, `terraform-parity`, `enterprise`, `state-management`
