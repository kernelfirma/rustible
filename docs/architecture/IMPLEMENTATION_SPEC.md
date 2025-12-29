# Phase 1 Implementation Specification

**Target Resources**: aws_security_group_rule, aws_iam_role, aws_iam_policy, aws_ebs_volume
**Duration**: 2-3 weeks
**Effort**: ~17-22 days focused development

---

## Resource 1: aws_iam_role

### Overview
Identity and Access Management role for AWS services and EC2 instances.

### File Structure
```
src/provisioning/resources/aws/iam_role.rs
├── Configuration
│   ├── IamRoleConfig (assume_role_policy, description, path, tags)
│   ├── IamRoleAttributes (arn, id, create_date)
│   └── IamRoleAssumePolicy (simplified JSON support)
├── Implementation
│   ├── AwsIamRoleResource struct
│   ├── Resource trait impl (create, read, update, delete, plan)
│   └── Helper functions (policy validation, ARN building)
└── Tests
    ├── Schema validation
    ├── Config parsing
    ├── Assume policy validation
    └── CRUD operations
```

### Key Configuration Fields
```yaml
aws_iam_role:
  my_service_role:
    name: my-service-role
    assume_role_policy: |
      {
        "Version": "2012-10-17",
        "Statement": [{
          "Effect": "Allow",
          "Principal": {"Service": "ec2.amazonaws.com"},
          "Action": "sts:AssumeRole"
        }]
      }
    description: "Service role for EC2 instances"
    max_session_duration: 3600
    path: /service/
    tags:
      Environment: production
```

### Dependencies
- None (foundational)

### AWS SDK Usage
- `iam_client.create_role()`
- `iam_client.update_assume_role_policy()`
- `iam_client.delete_role()`
- `iam_client.get_role()`

### Estimated Lines of Code
- Config: 200 lines
- Implementation: 700 lines
- Tests: 400 lines
- **Total**: ~1,300 lines

---

## Resource 2: aws_iam_policy

### Overview
Reusable permission policies for attachment to roles, users, and groups.

### File Structure
```
src/provisioning/resources/aws/iam_policy.rs
├── Configuration
│   ├── IamPolicyConfig (policy, description, path, tags)
│   ├── IamPolicyAttributes (arn, id, version)
│   └── PolicyDocument (JSON validation)
├── Implementation
│   ├── AwsIamPolicyResource struct
│   ├── Resource trait impl
│   └── Helper functions (policy validation, JSON parsing)
└── Tests
    ├── Schema validation
    ├── Policy document validation
    ├── JSON parsing tests
    └── CRUD operations
```

### Key Configuration Fields
```yaml
aws_iam_policy:
  ec2_full_access:
    name: EC2FullAccess
    policy: |
      {
        "Version": "2012-10-17",
        "Statement": [{
          "Effect": "Allow",
          "Action": "ec2:*",
          "Resource": "*"
        }]
      }
    description: "Full access to EC2"
    path: /policies/
    tags:
      Type: infrastructure
```

### Dependencies
- aws_iam_role (for attachment, but policy can exist standalone)

### AWS SDK Usage
- `iam_client.create_policy()`
- `iam_client.update_policy_version()`
- `iam_client.delete_policy()`
- `iam_client.get_policy()`
- `iam_client.list_policy_versions()`

### Estimated Lines of Code
- Config: 150 lines
- Implementation: 600 lines
- Tests: 350 lines
- **Total**: ~1,100 lines

---

## Resource 3: aws_security_group_rule

### Overview
Granular ingress/egress rules for security groups (alternative to inline rules).

### File Structure
```
src/provisioning/resources/aws/security_group_rule.rs
├── Configuration
│   ├── SecurityGroupRuleConfig (type, protocol, from_port, to_port, cidr_blocks, etc.)
│   ├── SecurityGroupRuleAttributes (id)
│   └── RuleType enum (Ingress, Egress)
├── Implementation
│   ├── AwsSecurityGroupRuleResource struct
│   ├── Resource trait impl
│   └── Helper functions (rule validation, normalization)
└── Tests
    ├── Schema validation
    ├── Rule conflict detection
    ├── Protocol/port validation
    └── CRUD operations
```

### Key Configuration Fields
```yaml
aws_security_group_rule:
  allow_https:
    type: ingress
    from_port: 443
    to_port: 443
    protocol: tcp
    cidr_blocks:
      - 0.0.0.0/0
    security_group_id: "{{ resources.aws_security_group.web.id }}"
    description: "Allow HTTPS from anywhere"

  allow_rds:
    type: egress
    from_port: 5432
    to_port: 5432
    protocol: tcp
    source_security_group_id: "{{ resources.aws_security_group.db.id }}"
    security_group_id: "{{ resources.aws_security_group.app.id }}"
```

### Dependencies
- aws_security_group (HAVE - already implemented)

### AWS SDK Usage
- `ec2_client.authorize_security_group_ingress()`
- `ec2_client.authorize_security_group_egress()`
- `ec2_client.revoke_security_group_ingress()`
- `ec2_client.revoke_security_group_egress()`
- `ec2_client.describe_security_group_rules()`

### Estimated Lines of Code
- Config: 150 lines
- Implementation: 550 lines
- Tests: 350 lines
- **Total**: ~1,050 lines

---

## Resource 4: aws_ebs_volume

### Overview
Persistent block storage volumes for EC2 instances.

### File Structure
```
src/provisioning/resources/aws/ebs_volume.rs
├── Configuration
│   ├── EbsVolumeConfig (size, type, iops, throughput, encryption, kms_key_id, tags)
│   ├── EbsVolumeAttributes (arn, id, state, availability_zone)
│   └── VolumeType enum (Gp2, Gp3, Io1, Io2, St1, Sc1, Standard)
├── Implementation
│   ├── AwsEbsVolumeResource struct
│   ├── Resource trait impl
│   └── Helper functions (volume sizing, type validation, encryption)
└── Tests
    ├── Schema validation
    ├── Volume type constraints
    ├── IOPS/throughput validation
    ├── Encryption validation
    └── CRUD operations with attachment tracking
```

### Key Configuration Fields
```yaml
aws_ebs_volume:
  database_volume:
    availability_zone: us-east-1a
    size: 100
    type: gp3
    iops: 3000
    throughput: 125
    encrypted: true
    kms_key_id: "{{ resources.aws_kms_key.ebs.id }}"
    tags:
      Name: db-data-volume
      Environment: production

  snapshot_volume:
    availability_zone: us-east-1a
    snapshot_id: snap-1234567890abcdef0
    type: io2
    iops: 5000
    tags:
      Name: restored-volume
```

### Dependencies
- aws_subnet (for availability_zone determination)
- Optional: aws_kms_key (for encryption)

### AWS SDK Usage
- `ec2_client.create_volume()`
- `ec2_client.modify_volume_attribute()`
- `ec2_client.delete_volume()`
- `ec2_client.describe_volumes()`
- `ec2_client.create_tags()` (for tagging)

### Estimated Lines of Code
- Config: 250 lines
- Implementation: 750 lines
- Tests: 450 lines
- **Total**: ~1,450 lines

---

## Phase 1 Implementation Checklist

### aws_iam_role
- [ ] Create src/provisioning/resources/aws/iam_role.rs
- [ ] Implement IamRoleConfig with validation
- [ ] Implement IamRoleAttributes
- [ ] Implement Resource trait
  - [ ] schema() - return complete schema
  - [ ] create() - create IAM role via SDK
  - [ ] read() - fetch role state
  - [ ] update() - update assume policy
  - [ ] delete() - remove role (fail if attached policies)
  - [ ] plan() - detect changes
- [ ] Implement error handling
  - [ ] InvalidParameterValue for bad names
  - [ ] NoSuchEntity for missing roles
  - [ ] DeleteConflict for attached policies
- [ ] Write tests (>90% coverage)
  - [ ] Valid config parsing
  - [ ] Invalid assume policy JSON handling
  - [ ] Path validation (must start with /)
  - [ ] Name collision detection
  - [ ] Tag handling
  - [ ] Max session duration limits (900-43200 seconds)
- [ ] Register in mod.rs
- [ ] Create examples/iam_role_example.yml

### aws_iam_policy
- [ ] Create src/provisioning/resources/aws/iam_policy.rs
- [ ] Implement IamPolicyConfig with validation
- [ ] Implement IamPolicyAttributes
- [ ] Implement Resource trait
  - [ ] schema() - return complete schema
  - [ ] create() - create policy via SDK
  - [ ] read() - fetch policy
  - [ ] update() - create new version (maintain old versions)
  - [ ] delete() - remove policy (fail if attached)
  - [ ] plan() - detect policy document changes
- [ ] Implement error handling
  - [ ] InvalidPolicyDocument for JSON errors
  - [ ] NoSuchEntity for missing policies
  - [ ] DeleteConflict for attached policies
  - [ ] LimitExceeded for version limit (5 versions per policy)
- [ ] Write tests (>90% coverage)
  - [ ] Valid policy document validation
  - [ ] Invalid JSON handling
  - [ ] Path validation
  - [ ] Version management
  - [ ] Tag handling
- [ ] Register in mod.rs
- [ ] Create examples/iam_policy_example.yml

### aws_security_group_rule
- [ ] Create src/provisioning/resources/aws/security_group_rule.rs
- [ ] Implement SecurityGroupRuleConfig
- [ ] Implement SecurityGroupRuleAttributes (rule_id)
- [ ] Implement Resource trait
  - [ ] schema() - return complete schema
  - [ ] create() - authorize rule via SDK
  - [ ] read() - fetch rule
  - [ ] update() - delete and recreate (force_new for most fields)
  - [ ] delete() - revoke rule via SDK
  - [ ] plan() - detect rule changes
- [ ] Implement error handling
  - [ ] InvalidGroupId.NotFound for missing SG
  - [ ] InvalidParameterValue for invalid ports/protocols
  - [ ] AuthorizeFailed for duplicate rules
- [ ] Write tests (>90% coverage)
  - [ ] Ingress vs Egress handling
  - [ ] CIDR block validation (v4/v6)
  - [ ] Security group reference resolution
  - [ ] Protocol/port combinations (TCP, UDP, ICMP, -1)
  - [ ] Conflict detection (should not error on idempotent update)
- [ ] Register in mod.rs
- [ ] Create examples/security_group_rule_example.yml

### aws_ebs_volume
- [ ] Create src/provisioning/resources/aws/ebs_volume.rs
- [ ] Implement EbsVolumeConfig with volume type enum
- [ ] Implement EbsVolumeAttributes (id, state, arn)
- [ ] Implement Resource trait
  - [ ] schema() - return complete schema
  - [ ] create() - create volume via SDK
  - [ ] read() - fetch volume state
  - [ ] update() - modify size, iops, throughput (must be detached for type change)
  - [ ] delete() - remove volume (must be detached, no snapshots)
  - [ ] plan() - detect changes
- [ ] Implement error handling
  - [ ] InvalidVolume.NotFound for missing volumes
  - [ ] InvalidParameterValue for invalid size/iops/throughput
  - [ ] VolumeInUse for operations on attached volumes
- [ ] Write tests (>90% coverage)
  - [ ] Volume type constraints (gp3 requires iops/throughput, io1/io2 requires iops only)
  - [ ] Size validation (1-16,384 GiB)
  - [ ] Encryption with custom KMS keys
  - [ ] Snapshot restoration
  - [ ] Tag handling
  - [ ] AZ selection
- [ ] Register in mod.rs
- [ ] Create examples/ebs_volume_example.yml

### Integration Tests
- [ ] iam_role + iam_policy attachment test
- [ ] security_group + security_group_rule orchestration
- [ ] instance + ebs_volume attachment test
- [ ] Cross-resource reference resolution

### Documentation
- [ ] Add 4 resources to provider schema (src/provisioning/providers/aws/mod.rs)
- [ ] Create YAML examples for each
- [ ] Update RESOURCE_TYPES constant
- [ ] Create migration guide (from inline SG rules to aws_security_group_rule)

---

## Testing Requirements

### Minimum Coverage: 90% per resource

**Unit Tests**:
- Config parsing from YAML/JSON
- Validation logic
- Error cases
- Plan/diff detection

**Integration Tests** (using wiremock or mock SDK):
- Complete CRUD lifecycle
- Error handling
- State transitions
- Dependency validation

**Documentation Tests**:
- Example YAML parses correctly
- Example shows expected output

---

## Code Quality Standards

- [ ] All public items documented with doc comments
- [ ] Error messages are user-friendly
- [ ] No unwrap() calls without justification
- [ ] All panics properly handled
- [ ] Async/await properly used
- [ ] No hardcoded values
- [ ] Sensitive fields marked in schema

---

## Deliverables Checklist

For each resource:
- [ ] Source code with full docs (1,000-2,000 lines)
- [ ] Unit tests (>90% coverage)
- [ ] Example YAML configuration
- [ ] Error handling test cases
- [ ] Performance characteristics documented (if relevant)

For Phase 1 as a whole:
- [ ] All 4 resources registered and accessible
- [ ] Full test suite runs successfully
- [ ] Documentation updated
- [ ] Examples run against local mocks
- [ ] Code review completed
- [ ] Ready for PR submission

---

## Success Criteria

- All 8 test suites pass with >90% coverage
- Can provision 3 types of infrastructure:
  1. Secured EC2 instance with IAM role and EBS volume
  2. Security group with individual rules
  3. All components tagged and documented
- Example files compile and parse correctly
- Documentation matches Terraform AWS provider docs in scope

---

## Timeline Estimate

**Week 1**:
- Days 1-2: aws_iam_role (implement, test, docs)
- Days 3-4: aws_iam_policy (implement, test, docs)
- Day 5: Code review, integration testing

**Week 2**:
- Days 1-2: aws_security_group_rule (implement, test, docs)
- Days 3-4: aws_ebs_volume (implement, test, docs)
- Day 5: Integration testing, final review

**Week 3**:
- Refinement based on feedback
- Performance optimization if needed
- Documentation refinement
- PR submission and review

---

## References

Existing implementations to reference:
- **VPC** (400 lines) - Simple networking resource
- **Instance** (2,100+ lines) - Complex resource with many fields
- **Security Group** (2,000+ lines) - Complex with nested rules
- **Route Table** (1,500 lines) - Medium complexity with associations

See `/home/artur/Repositories/rustible/src/provisioning/resources/aws/` for reference implementations.

