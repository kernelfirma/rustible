#181 [HIGH] Complete Cloud Provider Modules (AWS/Azure/GCP)

## Problem Statement
Rustible's cloud provider support is severely limited: AWS has partial implementation (~10 resources), while Azure and GCP are stubs only. This prevents multi-cloud and full AWS infrastructure management.

## Current State

### AWS
| Resource | Status | Priority |
|----------|--------|----------|
| EC2 instances | ⚠️ Partial | Critical |
| S3 buckets | ⚠️ Basic | Critical |
| VPC/Subnets | ❌ Not implemented | Critical |
| Security Groups | ❌ Not implemented | Critical |
| IAM roles/policies | ❌ Stubs only | High |
| RDS databases | ❌ Not implemented | High |
| ELB/ALB | ❌ Not implemented | High |
| Auto Scaling Groups | ❌ Not implemented | High |
| Lambda functions | ❌ Not implemented | Medium |
| ECS/EKS | ❌ Not implemented | Medium |
| Route53 | ❌ Not implemented | High |
| CloudWatch | ❌ Not implemented | Medium |
| SQS/SNS | ❌ Not implemented | Medium |
| DynamoDB | ❌ Not implemented | Medium |
| KMS | ❌ Not implemented | High |

### Azure
| Resource | Status | Notes |
|----------|--------|-------|
| Virtual Machines | ❌ Stub | Interface only |
| Resource Groups | ❌ Not implemented | - |
| VNet/Subnets | ❌ Not implemented | - |
| NSG | ❌ Not implemented | - |
| Storage Accounts | ❌ Not implemented | - |
| Azure SQL | ❌ Not implemented | - |
| App Service | ❌ Not implemented | - |
| AKS | ❌ Not implemented | - |
| Key Vault | ❌ Not implemented | - |

### GCP
| Resource | Status | Notes |
|----------|--------|-------|
| Compute Engine | ❌ Stub | Interface only |
| VPC Networks | ❌ Not implemented | - |
| Cloud Storage | ❌ Not implemented | - |
| Cloud SQL | ❌ Not implemented | - |
| GKE | ❌ Not implemented | - |
| Cloud Run | ❌ Not implemented | - |
| Secret Manager | ❌ Not implemented | - |

## Comparison to Terraform
| Provider | Resources | Rustible | Terraform |
|----------|-----------|----------|-----------|
| AWS | ~1000+ | ~10 | ✅ Full |
| Azure | ~500+ | 0 | ✅ Full |
| GCP | ~400+ | 0 | ✅ Full |

## Proposed Implementation

### Phase 1: AWS Core Resources
```rust
// src/modules/cloud/aws/
pub mod ec2;
pub mod vpc;
pub mod iam;
pub mod rds;
pub mod elb;
pub mod route53;
pub mod kms;

// Each module implements Module trait with CRUD operations
pub struct AwsEc2Instance;
impl Module for AwsEc2Instance {
    fn name(&self) -> &'static str { "aws_ec2_instance" }
    
    async fn execute(&self, params: &ModuleParams, ctx: &ModuleContext) -> ModuleResult {
        // Create/Update/Delete EC2 instances using AWS SDK
    }
}
```

- [ ] Complete `aws_ec2_instance` (start, stop, terminate, modify)
- [ ] Implement `aws_vpc` (create, delete, tagging)
- [ ] Implement `aws_subnet` (CIDR management, AZ placement)
- [ ] Implement `aws_security_group` (ingress/egress rules)
- [ ] Implement `aws_internet_gateway` (VPC attachment)
- [ ] Implement `aws_nat_gateway` (EIP association)
- [ ] Implement `aws_route_table` (route management)

### Phase 2: AWS IAM
```rust
// src/modules/cloud/aws/iam.rs
pub struct AwsIamRole;
pub struct AwsIamPolicy;
pub struct AwsIamInstanceProfile;
pub struct AwsIamUser;
pub struct AwsIamGroup;
```

- [ ] `aws_iam_role` - Role creation with trust policies
- [ ] `aws_iam_policy` - Policy document management
- [ ] `aws_iam_role_policy_attachment` - Attach policies to roles
- [ ] `aws_iam_instance_profile` - EC2 instance profiles
- [ ] `aws_iam_user` - User management
- [ ] `aws_iam_access_key` - Access key rotation
- [ ] `aws_iam_group` - Group management

### Phase 3: AWS Data & Messaging
- [ ] `aws_rds_instance` - Database provisioning
- [ ] `aws_dynamodb_table` - NoSQL tables
- [ ] `aws_elasticache_cluster` - Redis/Memcached
- [ ] `aws_sqs_queue` - Message queues
- [ ] `aws_sns_topic` - Notifications
- [ ] `aws_kinesis_stream` - Streaming data

### Phase 4: AWS Load Balancing & Auto Scaling
- [ ] `aws_lb` / `aws_alb` - Application/Network Load Balancers
- [ ] `aws_lb_target_group` - Target group management
- [ ] `aws_lb_listener` - Listener rules
- [ ] `aws_autoscaling_group` - ASG with launch templates
- [ ] `aws_launch_template` - Instance templates

### Phase 5: Azure Core
```rust
// src/modules/cloud/azure/
pub mod vm;
pub mod resource_group;
pub mod network;
pub mod storage;
pub mod aks;
pub mod keyvault;
```

- [ ] `azure_resource_group` - RG management
- [ ] `azure_virtual_machine` - VM provisioning
- [ ] `azure_virtual_network` - VNet creation
- [ ] `azure_subnet` - Subnet management
- [ ] `azure_network_security_group` - NSG rules
- [ ] `azure_storage_account` - Blob/Queue/Table
- [ ] `azure_sql_server` / `azure_sql_database` - SQL
- [ ] `azure_aks_cluster` - Kubernetes
- [ ] `azure_key_vault` - Secret management

### Phase 6: GCP Core
```rust
// src/modules/cloud/gcp/
pub mod compute;
pub mod storage;
pub mod sql;
pub mod gke;
pub mod run;
pub mod secretmanager;
```

- [ ] `gcp_compute_instance` - VM provisioning
- [ ] `gcp_compute_network` - VPC networks
- [ ] `gcp_compute_subnetwork` - Subnets
- [ ] `gcp_compute_firewall` - Firewall rules
- [ ] `gcp_storage_bucket` - Cloud Storage
- [ ] `gcp_sql_database_instance` - Cloud SQL
- [ ] `gcp_container_cluster` - GKE
- [ ] `gcp_cloud_run_service` - Cloud Run
- [ ] `gcp_secret_manager_secret` - Secrets

### Phase 7: Resource Dependencies
```rust
// src/provisioning/resource_graph.rs
pub struct ResourceGraph {
    nodes: Vec<ResourceNode>,
    edges: Vec<ResourceEdge>,
}

impl ResourceGraph {
    pub fn build(resources: &[Resource]) -> Result<Self, GraphError> {
        // Parse depends_on and implicit dependencies
        // Build DAG for parallel execution
    }
    
    pub fn execution_order(&self) -> Vec<Vec<ResourceId>> {
        // Return parallelizable groups
    }
}
```

- [ ] Resource graph construction
- [ ] Dependency resolution
- [ ] Parallel resource creation
- [ ] Cycle detection and error handling

## Example Usage
```yaml
# AWS infrastructure
- name: Create VPC
  aws_vpc:
    cidr_block: 10.0.0.0/16
    tags:
      Name: production
  register: vpc

- name: Create subnet
  aws_subnet:
    vpc_id: "{{ vpc.id }}"
    cidr_block: 10.0.1.0/24
    availability_zone: us-east-1a

- name: Create security group
  aws_security_group:
    name: web-sg
    vpc_id: "{{ vpc.id }}"
    rules:
      - protocol: tcp
        port: 80
        cidr: 0.0.0.0/0
      - protocol: tcp
        port: 443
        cidr: 0.0.0.0/0

- name: Launch EC2 instance
  aws_ec2_instance:
    name: web-01
    instance_type: t3.medium
    ami: ami-12345678
    subnet_id: "{{ subnet.id }}"
    security_group_ids:
      - "{{ security_group.id }}"
    tags:
      Environment: production
```

## Acceptance Criteria
- [ ] AWS VPC + EC2 + IAM resources work in production
- [ ] Azure Resource Group + VM + VNet resources work
- [ ] GCP Compute + Network resources work
- [ ] Resource dependencies are resolved correctly
- [ ] Idempotent operations (re-run produces no changes)
- [ ] Error handling for API failures
- [ ] Integration tests against real cloud accounts
- [ ] Documentation with examples for each resource

## Priority
**HIGH** - Required for infrastructure provisioning use cases

## Related
- Issue #175: Terraform-like state management
- Issue #177: Secret management (Cloud KMS integration)
- Feature flags: `aws`, `azure`, `gcp` in `Cargo.toml`

## Labels
`high`, `cloud`, `aws`, `azure`, `gcp`, `infrastructure`, `terraform-parity`
