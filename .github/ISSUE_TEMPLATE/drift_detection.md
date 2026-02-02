#178 [HIGH] Enable and Complete Drift Detection System

## Problem Statement
Rustible has a drift detection module (`src/drift/`) that is currently **disabled/commented out** in `lib.rs`. Without drift detection, users cannot identify when managed resources have been modified outside of Rustible (configuration drift), leading to unpredictable behavior and security gaps.

## Current State
```rust
// src/lib.rs - Drift module is commented out!
// pub mod drift;  // <-- DISABLED
```

| Feature | Status | Notes |
|---------|--------|-------|
| Drift detection framework | ⚠️ Exists but disabled | Module stubbed |
| Drift report generation | ❌ Not implemented | No reporting |
| Automatic drift remediation | ❌ Not implemented | No auto-fix |
| Scheduled drift checks | ❌ Not implemented | No cron equivalent |

## Comparison to Alternatives
| Feature | Rustible | Ansible | Terraform |
|---------|----------|---------|-----------|
| Drift detection | ❌ Disabled | ⚠️ Limited (facts diff) | ✅ `terraform plan` shows drift |
| Drift reporting | ❌ None | ❌ None | ✅ Detailed resource diffs |
| Drift remediation | ❌ None | ❌ None | ✅ Apply to correct |
| Scheduled checks | ❌ None | ❌ None | ✅ Terraform Cloud |

## Safety Implications
- **Security drift**: Firewall rules, user permissions change undetected
- **Compliance violations**: Systems drift out of compliance without notice
- **Configuration rot**: Accumulated undocumented changes
- **Incident response**: Unable to verify expected state during incidents

## Proposed Implementation

### Phase 1: Enable and Complete Drift Module
```rust
// src/lib.rs
pub mod drift;  // Re-enable

// src/drift/detector.rs
pub struct DriftDetector {
    state_manager: Arc<StateManager>,
    modules: Vec<Box<dyn DriftDetectableModule>>,
}

impl DriftDetector {
    pub async fn detect_drift(&self, target: &Target) -> Result<DriftReport, DriftError> {
        // Compare current state with expected state
        // Generate drift report
    }
}
```

- [ ] Re-enable drift module in `lib.rs`
- [ ] Complete `DriftDetector` implementation
- [ ] Add drift detection trait for modules
- [ ] Implement drift detection for core modules (file, package, service)
- [ ] State comparison engine

### Phase 2: Drift Detection for Core Modules
```rust
#[async_trait]
pub trait DriftDetectableModule: Module {
    async fn get_actual_state(&self, context: &ModuleContext) -> Result<ResourceState, DriftError>;
    async fn get_expected_state(&self, params: &ModuleParams) -> Result<ResourceState, DriftError>;
}

// Implement for:
// - file: Compare checksums, permissions, ownership
// - package: Check installed version vs desired
// - service: Check running state, enabled status
// - user/group: Verify existence and attributes
// - authorized_key: Compare key lists
```

- [ ] `file` module drift detection
- [ ] `package` module drift detection
- [ ] `service` module drift detection
- [ ] `user` / `group` module drift detection
- [ ] `authorized_key` module drift detection

### Phase 3: Drift Reporting
```bash
# Check for drift
rustible drift check -i inventory.yml

# Show drift report
rustible drift show --format json

# Check specific hosts
rustible drift check -l webservers

# Generate drift report file
rustible drift check --output drift-report.json
```

```rust
pub struct DriftReport {
    pub timestamp: DateTime<Utc>,
    pub hosts: Vec<HostDrift>,
    pub summary: DriftSummary,
}

pub struct HostDrift {
    pub hostname: String,
    pub resources: Vec<ResourceDrift>,
    pub drift_count: usize,
}

pub struct ResourceDrift {
    pub resource_type: String,
    pub resource_name: String,
    pub expected: Value,
    pub actual: Value,
    pub diff: String,  // Unified diff format
}
```

- [ ] Drift report generation
- [ ] JSON/CSV/YAML output formats
- [ ] Diff visualization (like `terraform show`)
- [ ] Integration with callback system
- [ ] Slack/email notifications for drift

### Phase 4: Drift Remediation
```bash
# Preview remediation
rustible drift remediate --check

# Apply fixes
rustible drift remediate

# Remediate specific hosts
rustible drift remediate -l webservers
```

- [ ] Generate remediation plan
- [ ] Preview changes (`--check` mode)
- [ ] Apply fixes selectively
- [ ] Rollback on remediation failure

### Phase 5: Continuous Drift Monitoring
```bash
# Start drift monitoring daemon
rustible drift monitor --interval 1h

# Schedule drift checks
rustible drift schedule --cron "0 */6 * * *" -i production.yml
```

- [ ] Background drift monitoring
- [ ] Scheduled drift checks
- [ ] Drift detection as callback plugin
- [ ] Integration with monitoring systems (Prometheus metrics)

## Playbook Integration
```yaml
# Detect drift as part of playbook
- name: Check for drift
  drift_check:
    resources:
      - file:/etc/nginx/nginx.conf
      - service:nginx
      - package:nginx
  register: drift_result

- name: Report drift
  debug:
    msg: "Drift detected: {{ drift_result.drifted_resources }}"
  when: drift_result.has_drift

# Remediate drift
- name: Fix drift
  drift_remediate:
    when: drift_result.has_drift
```

## Acceptance Criteria
- [ ] Drift detection works for file, package, and service modules
- [ ] `rustible drift check` produces actionable reports
- [ ] Drift can be remediated via `rustible drift remediate`
- [ ] Report shows unified diff of changes
- [ ] Performance: Drift check on 100 hosts completes in <60s
- [ ] Documentation covers drift detection workflows

## Priority
**HIGH** - Essential for maintaining configuration consistency

## Related
- Issue #167: Resource graph state comparison (related to state tracking)
- Existing code: `src/drift/` (disabled in lib.rs)

## Labels
`high`, `reliability`, `drift-detection`, `configuration-management`
