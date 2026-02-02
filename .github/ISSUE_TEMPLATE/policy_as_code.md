#180 [MEDIUM] Policy-as-Code Framework (Sentinel-like)

## Problem Statement
Rustible lacks a policy-as-code framework like Terraform Sentinel or Open Policy Agent (OPA) integration. This prevents organizations from enforcing security, compliance, and governance policies on playbooks before execution.

## Current State
| Feature | Status | Notes |
|---------|--------|-------|
| Policy framework | ❌ Not implemented | No policy engine |
| Policy enforcement | ❌ Not implemented | No runtime checks |
| Compliance reporting | ❌ Not implemented | No audit trails |
| Policy testing | ❌ Not implemented | No policy validation |

## Comparison to Alternatives
| Feature | Rustible | Terraform Sentinel | OPA |
|---------|----------|-------------------|-----|
| Policy language | ❌ None | Sentinel HCL | Rego |
| Pre-execution validation | ❌ None | ✅ `sentinel apply` | ✅ `opa test` |
| Runtime enforcement | ❌ None | ✅ Enterprise | ✅ kube-mgmt |
| Compliance reporting | ❌ None | ✅ Enterprise | ✅ OPA Gatekeeper |
| Policy library | ❌ None | ✅ HashiCorp registry | ✅ OPA ecosystem |

## Use Cases
1. **Security policies**: Block playbooks that open port 22 to 0.0.0.0/0
2. **Compliance**: Ensure all resources have required tags
3. **Cost control**: Prevent provisioning of expensive instance types
4. **Change management**: Require approval for production changes
5. **Best practices**: Enforce idempotent module usage

## Proposed Implementation

### Phase 1: Policy Engine Core
```rust
// src/policy/engine.rs
pub struct PolicyEngine {
    policies: Vec<Policy>,
    enforcement_mode: EnforcementMode,  // Advisory, Mandatory
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub name: String,
    pub description: String,
    pub severity: PolicySeverity,  // Critical, High, Medium, Low
    pub rule: Box<dyn PolicyRule>,
}

#[async_trait]
pub trait PolicyRule: Send + Sync {
    async fn evaluate(&self, context: &PolicyContext) -> Result<PolicyResult, PolicyError>;
}

pub struct PolicyResult {
    pub passed: bool,
    pub violations: Vec<PolicyViolation>,
    pub message: String,
}
```

- [ ] Policy engine core implementation
- [ ] Policy loading from files
- [ ] Policy evaluation framework
- [ ] Enforcement mode support (advisory vs mandatory)

### Phase 2: Policy Languages Support

#### Option A: Rego (OPA) Integration
```rust
// src/policy/rego.rs
pub struct RegoPolicy {
    module: RegoModule,
    query: String,
}

impl PolicyRule for RegoPolicy {
    async fn evaluate(&self, context: &PolicyContext) -> Result<PolicyResult, PolicyError> {
        // Use OPA WASM or embedded engine
    }
}
```

```rego
# policies/required_tags.rego
package rustible.required_tags

import future.keywords.if
import future.keywords.in

deny[msg] if {
    resource := input.resources[_]
    resource.type == "aws_instance"
    not resource.config.tags["Environment"]
    msg := sprintf("Resource %s missing required Environment tag", [resource.name])
}

deny[msg] if {
    resource := input.resources[_]
    resource.type == "aws_instance"
    not resource.config.tags["Owner"]
    msg := sprintf("Resource %s missing required Owner tag", [resource.name])
}
```

#### Option B: Rustible Policy DSL
```yaml
# policies/required_tags.yml
name: required-tags
description: Ensure all AWS resources have required tags
severity: high
enforcement: mandatory

rules:
  - type: resource-attribute
    resource_types:
      - aws_instance
      - aws_s3_bucket
    condition: tags.Environment exists
    message: "Resource {{ resource.name }} missing Environment tag"
    
  - type: resource-attribute
    resource_types:
      - aws_instance
      - aws_s3_bucket  
    condition: tags.Owner exists
    message: "Resource {{ resource.name }} missing Owner tag"
```

#### Option C: CEL (Common Expression Language)
```yaml
# policies/security.policies
name: no-public-ssh
description: Block SSH access from 0.0.0.0/0
severity: critical
enforcement: mandatory
condition: |
  resource.type == "aws_security_group_rule" &&
  resource.config.from_port == 22 &&
  resource.config.cidr_blocks.exists(cidr, cidr == "0.0.0.0/0")
message: "Security group allows SSH from 0.0.0.0/0"
```

Tasks:
- [ ] Rego/OPA integration
- [ ] Native YAML policy DSL
- [ ] CEL expression support
- [ ] Policy language selection (configurable)

### Phase 3: Built-in Policy Library
```yaml
# policies/builtin/security.yml
policies:
  - name: no-public-admin-ports
    description: Block public access to admin ports
    rules:
      - block_ports: [22, 3389, 5432, 3306]
        from_cidr: ["0.0.0.0/0"]
        
  - name: require-encryption
    description: Require encryption at rest and in transit
    rules:
      - require_attribute: encrypt
        resource_types: [aws_s3_bucket, aws_rds_instance]
        
  - name: cost-controls
    description: Prevent expensive instance types
    rules:
      - restrict_values: instance_type
        resource_type: aws_instance
        allowed: [t3.*, m6.*, c6.*]
        blocked: [x1.*, p3.*, inf1.*]
```

- [ ] Security policies library
- [ ] Compliance policies (SOC2, PCI-DSS, HIPAA)
- [ ] Cost control policies
- [ ] Best practices policies

### Phase 4: Policy Enforcement Points

#### Pre-Execution Validation
```bash
# Validate playbook against policies
rustible policy check playbook.yml

# Fail if policies violated
rustible policy check --enforcement=mandatory playbook.yml

# Show all violations
rustible policy check --verbose playbook.yml
```

#### Runtime Enforcement
```rust
// In executor, before each task
async fn check_policies(&self, task: &Task, context: &ExecutionContext) -> Result<(), PolicyError> {
    let violations = self.policy_engine.evaluate_task(task, context).await?;
    
    if violations.has_critical() {
        return Err(PolicyError::CriticalViolation(violations));
    }
    
    if violations.has_mandatory() && self.policy_mode == PolicyMode::Enforce {
        return Err(PolicyError::MandatoryViolation(violations));
    }
    
    // Log advisory violations
    for v in violations.advisory() {
        warn!("Policy advisory: {}", v.message);
    }
    
    Ok(())
}
```

- [ ] Pre-execution policy check
- [ ] Runtime policy enforcement
- [ ] Policy violation reporting
- [ ] Audit logging of policy decisions

### Phase 5: Compliance Reporting
```bash
# Generate compliance report
rustible policy report --format pdf --output compliance-report.pdf

# Check specific compliance standard
rustible policy check --standard SOC2 playbook.yml
rustible policy check --standard PCI-DSS playbook.yml
```

```json
{
  "report_date": "2026-02-02",
  "policies_evaluated": 25,
  "violations": {
    "critical": 0,
    "high": 2,
    "medium": 5,
    "low": 3
  },
  "compliance": {
    "SOC2": "89%",
    "PCI-DSS": "92%"
  }
}
```

- [ ] Compliance report generation
- [ ] Standard compliance frameworks (SOC2, PCI-DSS, HIPAA, NIST)
- [ ] Violation tracking over time
- [ ] Integration with compliance tools (Vanta, Drata, etc.)

## Configuration
```toml
# rustible.toml
[policy]
enforcement = "advisory"  # advisory, mandatory
default_severity = "medium"

[[policy.repositories]]
name = "builtin"
path = "/usr/share/rustible/policies"

[[policy.repositories]]
name = "company"
path = "./policies"

[policy.opa]
enabled = true
url = "http://localhost:8181"
```

## Acceptance Criteria
- [ ] Policies can block execution of non-compliant playbooks
- [ ] Built-in policy library covers common security requirements
- [ ] Custom policies can be written in Rego or YAML
- [ ] Policy check runs in <5s for typical playbook
- [ ] Compliance reports can be generated
- [ ] Policy violations are logged with full context

## Priority
**MEDIUM** - Important for enterprise governance; can use external tools initially

## Related
- Security audit: `docs/security/SECURITY_AUDIT_REPORT.md`
- Issue #175: Terraform state management (policies often apply to state)

## Labels
`medium`, `security`, `compliance`, `governance`, `enterprise`
