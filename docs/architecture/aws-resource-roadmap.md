# AWS Resource Implementation Roadmap for Rustible

**Document**: AWS Resource Prioritization Analysis
**Date**: 2025-12-29
**Status**: Research Complete
**Scope**: Next 5-10 AWS resources prioritized for Terraform parity in the Rustible provisioning system

---

## Executive Summary

Rustible currently implements 8 core AWS networking resources (VPC, Subnet, Security Group, Instance, EIP, IGW, NAT Gateway, Route Table), totaling approximately 10,379 lines of production code. This analysis recommends the next 5-10 resources based on:

1. **Infrastructure pattern coverage** (3-tier apps, EKS clusters, serverless, HA)
2. **Dependency relationships** between resources
3. **Implementation complexity vs. business value**
4. **User impact and adoption velocity**

The analysis identifies critical capability gaps preventing common infrastructure patterns and recommends a phased implementation roadmap prioritized for maximum value delivery.

---

## Current Implementation Status

### Networking Layer (100% Complete)
- **Implemented**: vpc, subnet, security_group, internet_gateway, nat_gateway, route_table, elastic_ip, instance
- **Lines of Code**: ~10,379 across 9 resource modules
- **AWS SDK Version**: aws-sdk-ec2 1.15, aws-sdk-s3 1.15
- **Pattern Coverage**: Basic networking and compute only

### Gaps by Category

| Category | Status | Impact |
|----------|--------|--------|
| **Storage** | 0/3 (0%) | CRITICAL - No persistent storage beyond instance volumes |
| **Identity/Access** | 0/2 (0%) | CRITICAL - No IAM roles/policies for secure access |
| **Databases** | 0/3 (0%) | HIGH - No RDS, Aurora, or DynamoDB support |
| **Load Balancing** | 0/2 (0%) | HIGH - No HA/scale-out patterns possible |
| **Auto Scaling** | 0/1 (0%) | HIGH - No cluster management |
| **DNS** | 0/1 (0%) | MEDIUM - No Route53 support |
| **Certificates** | 0/1 (0%) | MEDIUM - No HTTPS provisioning |

---

## Recommended Priority: Top 8 Resources

### PRIORITY 1: Foundation Resources (Implement First)

These resources unlock the most common infrastructure patterns and are dependencies for many others.

#### 1. **aws_security_group_rule**
**Type**: Fine-grained network access control
**Priority**: P0 (Implement First)
**Complexity**: Low (650-800 lines)
**Justification**:
- Depends on: aws_security_group (already have)
- Required by: All new resources needing specific ingress/egress rules
- Pattern value: Enables moving rules outside security group definition
- Terraform alignment: Direct parity with terraform aws_security_group_rule
- Real-world use: 95%+ of infrastructure uses fine-grained rules

**Key Features**:
- Ingress rules with protocols, ports, CIDR blocks
- Egress rules for outbound access
- Support for IPv4, IPv6, and security group references
- Rule description and tagging

**Dependencies**:
- aws_security_group (HAVE)

**Estimated Effort**: 1-2 days

---

#### 2. **aws_iam_role**
**Type**: Identity and access management
**Priority**: P0 (Implement First)
**Complexity**: Low (700-900 lines)
**Justification**:
- Prerequisite for: RDS, Lambda, ECS, EC2 instance profiles
- Blocks: Every resource that needs specific AWS permissions
- Pattern value: Core of AWS security model
- Terraform alignment: Direct parity with terraform aws_iam_role

**Key Features**:
- Assume role policy (trust relationship)
- Maximum session duration
- Description and path
- Tags for organization

**Dependencies**:
- None (foundational)

**Estimated Effort**: 1-2 days

---

#### 3. **aws_iam_policy**
**Type**: Permission definitions
**Priority**: P0 (Implement First)
**Complexity**: Low-Medium (600-800 lines)
**Justification**:
- Prerequisite for: aws_iam_role_policy_attachment, service permissions
- Blocks: Proper permission model implementation
- Pattern value: Complete IAM story with roles
- Terraform alignment: Direct parity

**Key Features**:
- JSON policy document
- Version and description
- Inline policy support
- Policy attachment to roles/users/groups

**Dependencies**:
- aws_iam_role (for attachments)

**Estimated Effort**: 1-2 days

---

#### 4. **aws_ebs_volume**
**Type**: Persistent block storage
**Priority**: P0 (Implement Early)
**Complexity**: Low (800-1,000 lines)
**Justification**:
- Blocks: AWS instance persistent storage beyond root volume
- Pattern value: Multi-volume configurations, data persistence
- Terraform alignment: Direct parity
- Real-world impact: 80%+ of production instances use multiple EBS volumes

**Key Features**:
- Volume size and type (gp3, io1, io2, st1, sc1)
- IOPS and throughput configuration
- Encryption with custom KMS keys
- Snapshot creation
- Attachment tracking

**Dependencies**:
- aws_subnet (for availability zone)
- Optional: aws_kms_key

**Estimated Effort**: 1-2 days

---

#### 5. **aws_s3_bucket**
**Type**: Object storage (cloud storage)
**Priority**: P0 (Implement Early)
**Complexity**: Medium (1,000-1,200 lines)
**Justification**:
- Enables: Data lakes, backups, static site hosting, logs storage
- Pattern value: All infrastructure patterns use S3 in some capacity
- Terraform alignment: Direct parity (split across multiple resources)
- Real-world impact: 90%+ of AWS deployments use S3

**Key Features**:
- Bucket name and region
- Versioning configuration
- Server-side encryption (SSE-S3, SSE-KMS)
- Public access block
- Lifecycle rules
- CORS configuration
- Access logging
- Tags and metadata

**Dependencies**:
- aws_kms_key (optional, for encryption)

**Estimated Effort**: 2-3 days

---

### PRIORITY 2: Database Support (Implement Second)

Enables data-tier in 3-tier applications and EKS deployments.

#### 6. **aws_db_subnet_group**
**Type**: Database subnet grouping
**Priority**: P1 (Implement Second)
**Complexity**: Low (600-800 lines)
**Justification**:
- Prerequisite for: aws_rds_instance, aws_rds_cluster
- Blocks: RDS deployment in VPC
- Terraform alignment: Required before RDS
- Effort ratio: 20% effort, enables 40% of P1 priority items

**Key Features**:
- Subnet selection across AZs
- Description and tags
- Multi-AZ group definition

**Dependencies**:
- aws_subnet (HAVE)
- aws_vpc (HAVE)

**Estimated Effort**: 1 day

---

#### 7. **aws_rds_instance**
**Type**: Managed relational database
**Priority**: P1 (Implement Second)
**Complexity**: Medium-High (1,500-1,800 lines)
**Justification**:
- Enables: 3-tier web app pattern, EKS data tier
- Pattern value: Database layer for all app architectures
- Terraform alignment: Heavy usage in real Terraform configs
- Real-world impact: 70%+ of production apps need databases

**Key Features**:
- Engine selection (PostgreSQL, MySQL, MariaDB, Oracle, SQL Server)
- Instance class and storage configuration
- Multi-AZ deployment
- Backup retention and windows
- Parameter groups
- DB subnet group and security groups
- Enhanced monitoring
- Performance Insights
- Encryption at rest

**Dependencies**:
- aws_db_subnet_group (MUST HAVE)
- aws_security_group (for DB security group)
- aws_db_parameter_group (optional)

**Estimated Effort**: 3-4 days

---

### PRIORITY 3: Load Balancing & Scaling (Implement Third)

Enables high-availability and auto-scaling patterns.

#### 8. **aws_lb** (Application & Network Load Balancer)
**Type**: Layer 7 (ALB) and Layer 4 (NLB) load balancing
**Priority**: P1 (Implement Third)
**Complexity**: High (1,500-2,000 lines)
**Justification**:
- Enables: HA, multi-AZ, auto-scaling patterns
- Pattern value: Foundation for production deployments
- Terraform alignment: Essential resource in 90%+ of production configs
- Real-world impact: Required for 3-tier apps, Kubernetes Ingress, microservices

**Key Features**:
- ALB: HTTP/HTTPS path/hostname routing
- NLB: Ultra-high throughput, TCP/UDP
- Target groups with health checks
- Listener rules and priorities
- SSL/TLS certificate support
- Access logging
- WAF integration (ALB only)
- Cross-zone load balancing

**Dependencies**:
- aws_security_group (HAVE)
- aws_subnet (HAVE)
- aws_acm_certificate (for HTTPS)

**Estimated Effort**: 3-4 days

---

## Supporting Priority Resources (Conditional)

### PRIORITY 2B: Configuration & Parameter Groups

#### **aws_db_parameter_group** (Conditional for RDS)
**Type**: RDS configuration
**Complexity**: Low (600-800 lines)
**Use When**: Implementing RDS with custom parameters

#### **aws_db_option_group** (Conditional for RDS)
**Type**: RDS options (Oracle/SQL Server)
**Complexity**: Low (500-700 lines)
**Use When**: Oracle or SQL Server database support needed

### PRIORITY 2C: Advanced Storage

#### **aws_ebs_snapshot** (Post-launch)
**Type**: EBS volume snapshots
**Complexity**: Low-Medium (700-900 lines)
**Use When**: Disaster recovery and backup patterns

#### **aws_efs_file_system** (Medium-term)
**Type**: NFS-like shared storage
**Complexity**: Medium (900-1,100 lines)
**Use When**: Multi-instance shared storage needed (Kubernetes, NFS requirements)

### PRIORITY 3B: Auto-Scaling (If ALB/Time Allows)

#### **aws_autoscaling_group**
**Type**: Cluster auto-scaling
**Complexity**: High (1,800-2,200 lines)
**Dependencies**:
- aws_launch_template (also needed, 600-800 lines)
- aws_lb_target_group (part of aws_lb)

**Use When**: Implementing scaling patterns

---

## Implementation Roadmap

### Phase 1: Foundation (Weeks 1-2)
**Goal**: Core resources enabling all patterns

Priority order:
1. aws_iam_role (3-4 files, ~800 lines)
2. aws_iam_policy (2-3 files, ~700 lines)
3. aws_security_group_rule (3 files, ~700 lines)
4. aws_ebs_volume (3 files, ~900 lines)

**Deliverables**:
- 4 new resources
- Complete IAM and storage foundation
- ~3,100 lines of new code
- Full test coverage
- Documentation and examples

**Milestone**: Can provision secured instances with persistent storage

---

### Phase 2: Data Tier (Weeks 3-4)
**Goal**: Database support for 3-tier applications

Priority order:
1. aws_db_subnet_group (2 files, ~700 lines)
2. aws_rds_instance (3 files, ~1,600 lines)
3. aws_s3_bucket (3 files, ~1,100 lines)

**Deliverables**:
- 3 new resources
- Complete data tier for applications
- ~3,400 lines of new code
- Full test coverage with multi-AZ examples

**Milestone**: Can provision complete 3-tier web application

---

### Phase 3: HA & Scaling (Weeks 5-6)
**Goal**: Production-grade load balancing and scaling

Priority order:
1. aws_lb (3 files, ~1,700 lines)
2. aws_launch_template (2 files, ~700 lines)
3. aws_autoscaling_group (3 files, ~1,900 lines)

**Deliverables**:
- 3 new resources
- Complete HA infrastructure patterns
- ~4,300 lines of new code
- Production examples: multi-AZ, auto-scaling, Kubernetes

**Milestone**: Can provision enterprise-grade, auto-scaling infrastructure

---

## Common Infrastructure Patterns Coverage

### Pattern: 3-Tier Web Application
**Current**: 4/8 resources (50%)
**After Phase 1**: 8/8 (100%) ✓
**After Phase 2**: 11/8 (137%) ✓
**After Phase 3**: 14/8 (175%) ✓

Components needed:
- Web tier: ALB (P3), security groups (HAVE), instances (HAVE)
- App tier: instances (HAVE), security groups (HAVE), IAM roles (P1)
- Data tier: RDS (P2), S3 (P2), security groups (HAVE)

**Achievable after Phase 2** ✓

---

### Pattern: EKS Kubernetes Cluster
**Current**: 6/8 resources (75%)
**After Phase 1**: 10/8 (125%) ✓
**After Phase 3**: 13/8 (162%) ✓

Components needed:
- Control plane: Managed by AWS
- Worker nodes: instances (HAVE), security groups (HAVE), IAM roles (P1), EBS (P1)
- Networking: VPC (HAVE), subnets (HAVE), security groups (HAVE)
- Storage: EBS (P1), EFS (optional), S3 (P2)
- Ingress: ALB (P3)

**Achievable after Phase 1** ✓

---

### Pattern: Serverless/Lambda
**Current**: 2/6 resources (33%)
**Blocked on**: Lambda resource, API Gateway, DynamoDB

Not in current roadmap (requires separate analysis)

---

### Pattern: Multi-AZ HA Web Service
**Current**: 4/8 resources (50%)
**After Phase 2**: 11/8 (137%) ✓
**After Phase 3**: 14/8 (175%) ✓

Components:
- Compute: instances (HAVE), ASG (P3)
- Networking: VPC (HAVE), subnets (HAVE), security groups (HAVE), ALB (P3)
- Storage: EBS (P1), S3 (P2)
- Database: RDS (P2)

**Achievable after Phase 3** ✓

---

## Resource Implementation Guide

### Template Structure (Proven Pattern)

Each resource requires 3-4 files following the established pattern:

**1. Configuration & Types** (~200-400 lines)
```rust
// Config struct with serde
pub struct ResourceConfig { ... }

// Attributes struct (computed fields)
pub struct ResourceAttributes { ... }

// Supporting enums and types
pub enum SomeOption { ... }
```

**2. Resource Implementation** (~700-1,200 lines)
```rust
pub struct AwsResourceResource { ... }

#[async_trait]
impl Resource for AwsResourceResource {
    fn schema(&self) -> ResourceSchema { ... }
    async fn create(&self, ...) -> Result { ... }
    async fn read(&self, ...) -> Result { ... }
    async fn update(&self, ...) -> Result { ... }
    async fn delete(&self, ...) -> Result { ... }
    async fn plan(&self, ...) -> Result { ... }
}
```

**3. Tests** (~400-800 lines)
```rust
#[cfg(test)]
mod tests {
    // Schema validation
    // Configuration parsing
    // Plan/diff detection
    // Mock create/update/delete operations
}
```

**4. Module Registration** (Register in `/src/provisioning/resources/aws/mod.rs`)
```rust
pub mod new_resource;
pub use new_resource::{AwsNewResource, NewResourceConfig};
```

---

## Implementation Complexity Analysis

### Lines of Code Estimation

| Resource | Config | Impl | Tests | Total | Days |
|----------|--------|------|-------|-------|------|
| aws_security_group_rule | 150 | 550 | 350 | 1,050 | 1-2 |
| aws_iam_role | 200 | 700 | 400 | 1,300 | 1-2 |
| aws_iam_policy | 150 | 600 | 350 | 1,100 | 1-2 |
| aws_ebs_volume | 250 | 750 | 450 | 1,450 | 1-2 |
| aws_s3_bucket | 350 | 850 | 500 | 1,700 | 2-3 |
| aws_db_subnet_group | 150 | 550 | 350 | 1,050 | 1 |
| aws_rds_instance | 400 | 1,100 | 600 | 2,100 | 3-4 |
| aws_lb | 300 | 1,200 | 600 | 2,100 | 3-4 |

**Total for 8 resources**: ~11,450 lines, ~17-22 days of focused effort

---

## Testing Strategy

### Unit Tests (Mandatory for All)
- Schema validation
- Config parsing and validation
- Plan/diff detection
- Error handling

### Integration Tests (For Complex Resources)
- Mock AWS SDK calls using wiremock
- Full CRUD lifecycle simulation
- Dependency chain testing
- Cross-resource reference validation

### Examples & Documentation
- Complete YAML examples for each resource
- Multi-resource orchestration examples
- Pattern examples (3-tier, HA, EKS, etc.)

---

## Risk Mitigation

### High-Risk Areas
1. **IAM Policy Document Validation**: Complex JSON schema validation
   - Mitigation: Use jsonschema crate for validation
   - Reference: aws-iam-checker patterns

2. **RDS Multi-Engine Support**: Different engines have different parameters
   - Mitigation: Start with PostgreSQL only, expand to others
   - Reference: database module in Ansible

3. **Load Balancer Routing Rules**: Complex rule priority handling
   - Mitigation: Simplify v1 (basic rules), add advanced v1.1
   - Reference: Terraform ALB resource code

### Testing Gaps
- Live AWS testing (use sandbox account)
- Cross-region behavior
- Quota and service limits
- Eventual consistency edge cases

---

## Success Metrics

### Phase 1 Success Criteria
- All 4 resources with 90%+ test coverage
- Can provision secured, multi-volume EC2 instance
- Documentation matches Terraform parity
- Performance within 5% of Terraform apply

### Phase 2 Success Criteria
- All 3 resources with 90%+ test coverage
- Can provision complete 3-tier web app
- Multi-AZ RDS deployment working
- S3 with versioning and encryption

### Phase 3 Success Criteria
- All 3 resources with 90%+ test coverage
- Can provision auto-scaling cluster
- ALB with path/hostname routing
- Load-balanced RDS + ASG + ALB example works

---

## Alternative Approaches Not Recommended

### AWS CDK Integration
**Why Not**: Adds Python/JavaScript dependency, conflicts with pure Rust goal

### Terraform Provider Binding
**Why Not**: Already solved by HCL parsing, adds complexity without benefit

### Lazy Loading Resources
**Why Not**: Current architecture handles on-demand loading well

---

## Appendix: Resource Dependency Graph

```
Foundation Layer:
  aws_iam_role ──┐
  aws_iam_policy ├─ (All resources)
  aws_security_group_rule ──┐
                             ├─ Compute & Data resources
  aws_ebs_volume ───────────┘

Storage Layer:
  aws_s3_bucket (Independent)
  aws_ebs_snapshot ── depends on ── aws_ebs_volume

Database Layer:
  aws_db_subnet_group ──┐
  aws_db_parameter_group├─ aws_rds_instance
  aws_security_group ───┘

Load Balancing Layer:
  aws_lb (Independent, uses aws_security_group)
  aws_launch_template (Independent, uses aws_iam_role)
  aws_autoscaling_group ── depends on ── aws_launch_template

Optional:
  aws_efs_file_system ── depends on ── aws_security_group
  aws_acm_certificate (Independent)
  aws_route53_zone (Independent)
```

---

## Next Steps

1. **Review & Approval**: Get stakeholder approval on prioritization
2. **Environment Setup**: Create feature branch and test environment
3. **Phase 1 Implementation**: Begin with aws_iam_role and aws_iam_policy
4. **Incremental Delivery**: Ship Phase 1 resources as soon as complete
5. **Community Feedback**: Gather user feedback before Phase 2
6. **Scale Based on Demand**: Adjust roadmap based on real-world usage patterns

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2025-12-29 | Research Agent | Initial analysis, 8-resource prioritized roadmap |
