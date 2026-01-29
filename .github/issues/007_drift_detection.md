# Feature: Implement Configuration Drift Detection

## Problem Statement
Neither Ansible nor Terraform have built-in continuous drift detection. Configuration drift occurs when actual infrastructure state differs from desired IaC definitions, often without visibility. This leads to unexpected failures and security risks.

## Current State
- No drift detection capability
- No state tracking for configuration
- No comparison between desired and actual state
- No monitoring of manual changes

## Proposed Solution

### Phase 1: State Snapshots (v0.1.x)
1. **State snapshot system**
   ```rust
   // src/state/snapshot.rs
   use serde::{Deserialize, Serialize};
   
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct StateSnapshot {
       pub timestamp: DateTime<Utc>,
       pub host: String,
       pub playbook: String,
       pub resources: HashMap<ResourceKey, ResourceState>,
       pub checksum: String,
   }
   
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct ResourceState {
       pub resource_type: String,
       pub resource_id: String,
       pub attributes: serde_json::Value,
       pub checksum: String,
   }
   ```

2. **Automatic state capture**
   - Capture state after successful playbook runs
   - Store in `~/.rustible/state/` directory
   - Versioned by timestamp

### Phase 2: Drift Detection (v0.2.x)
1. **Drift detector implementation**
   ```rust
   // src/state/drift_detector.rs
   pub struct DriftDetector {
       state_backend: Box<dyn StateBackend>,
   }
   
   impl DriftDetector {
       pub async fn detect_drift(&self, host: &Host) -> Result<DriftReport> {
           let last_state = self.state_backend.get_last_state(host).await?;
           let current_state = self.gather_current_state(host).await?;
           
           let drift = DriftReport {
               host: host.name.clone(),
               timestamp: Utc::now(),
               added: current_state.diff_added(&last_state),
               removed: current_state.diff_removed(&last_state),
               modified: current_state.diff_modified(&last_state),
               drift_score: self.calculate_drift_score(&current_state, &last_state),
           };
           
           Ok(drift)
       }
   }
   ```

2. **CLI integration**
   ```bash
   # Check for drift
   rustible drift check -i inventory.yml
   
   # Show drift report
   rustible drift report --format json
   
   # Check specific hosts
   rustible drift check -i inventory.yml -l web*
   ```

### Phase 3: Continuous Monitoring (v0.3.x)
1. **Background drift monitor daemon**
   ```rust
   // src/state/drift_monitor.rs
   pub struct DriftMonitor {
       check_interval: Duration,
       notification_channels: Vec<Box<dyn NotificationChannel>>,
       drift_threshold: f64,
   }
   
   impl DriftMonitor {
       pub async fn start(&self, state: Arc<RwLock<State>>) -> Result<()> {
           let mut ticker = interval(self.check_interval);
           
           loop {
               ticker.tick().await;
               
               let current_state = state.read().await;
               let drift_report = self.check_all_resources(&current_state).await?;
               
               if drift_report.drift_score > self.drift_threshold {
                   tracing::warn!(
                       drift_score = %drift_report.drift_score,
                       resources_drifted = %drift_report.drifted_resources.len(),
                       "Significant drift detected"
                   );
                   
                   for channel in &self.notification_channels {
                       channel.send(DriftAlert {
                           severity: Severity::Warning,
                           report: drift_report.clone(),
                           timestamp: Utc::now(),
                       }).await?;
                   }
               }
               
               self.record_drift_metrics(&drift_report).await?;
           }
       }
   }
   ```

2. **Notification channels**
   - Slack webhook
   - Email notifications
   - PagerDuty integration
   - Webhook support

3. **Drift history and trends**
   - Track drift over time
   - Identify drift-prone resources
   - Generate trend reports

## Expected Outcomes
- Visibility into configuration drift
- Early detection of manual changes
- Improved security posture
- Better compliance reporting
- Reduced troubleshooting time

## Success Criteria
- [ ] State snapshot system implemented
- [ ] Drift detection for file changes
- [ ] Drift detection for package changes
- [ ] Drift detection for service changes
- [ ] Drift detection for configuration changes
- [ ] CLI commands for drift checking
- [ ] Background drift monitor daemon
- [ ] Notification channels implemented
- [ ] Drift history and trend reporting
- [ ] Integration with monitoring systems

## Implementation Details

### Drift Report
```rust
pub struct DriftReport {
    pub host: String,
    pub timestamp: DateTime<Utc>,
    pub added: Vec<ResourceDiff>,
    pub removed: Vec<ResourceDiff>,
    pub modified: Vec<ResourceDiff>,
    pub unchanged: Vec<ResourceKey>,
    pub drift_score: f64,
}

pub struct ResourceDiff {
    pub resource_type: String,
    pub resource_id: String,
    pub expected: serde_json::Value,
    pub actual: serde_json::Value,
    pub diff: Diff,
}

pub enum Diff {
    Added,
    Removed,
    Modified { changes: Vec<Change> },
}
```

### CLI Output
```
rustible drift check -i inventory.yml

Checking for configuration drift...

✓ web1.example.com: No drift detected
✗ web2.example.com: Drift detected
  - nginx.conf modified
    Expected: worker_processes 4;
    Actual: worker_processes 8;
  
  - Package vim installed
    Expected: not present
    Actual: present

✓ db1.example.com: No drift detected

Drift Summary: 1/3 hosts have drift
Severity: Medium
Action Required: Review and remediate
```

### Configuration
```toml
[state]
backend = "local"
path = "~/.rustible/state"

[drift]
enable = true
check_interval = "1h"
drift_threshold = 0.3
notification_channels = ["slack", "email"]

[drift.slack]
webhook_url = "https://hooks.slack.com/services/..."

[drift.email]
to = "ops@example.com"
subject = "Configuration Drift Detected"
```

## Related Issues
- #008: State Management
- #009: Remote State Backends
- #010: State Locking

## Additional Notes
This is a **P1 (High)** feature that addresses a significant gap in both Ansible and Terraform. Should be targeted for v0.2.x release with v0.3.x continuous monitoring.
