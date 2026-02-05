# Terraform Integration Compatibility

> **Last Updated:** 2026-02-05
> **Rustible Version:** 0.1.x
> **Status:** Experimental (Feature-gated)

This document tracks Rustible's Terraform-like provisioning capabilities and integration scope.

---

## Overview

Rustible includes experimental Terraform-like provisioning capabilities, enabling infrastructure-as-code workflows alongside configuration management. This feature is enabled via the `provisioning` feature flag.

```bash
# Build with provisioning support
cargo build --release --features provisioning

# Or with full AWS support
cargo build --release --features full-provisioning
```

Rustible can also import Terraform outputs via `vars_files` entries (local, HTTP, or S3) and use Terraform state for dynamic inventory with resource mappings and caching.

---

## Feature Status

| Capability | Terraform | Rustible | Status |
|------------|-----------|----------|--------|
| Plan mode preview | Yes | Yes | Stable |
| State management | Yes | Yes | Stable |
| Drift detection | Yes | Yes | Stable |
| Remote state backends | Yes | Yes | Stable |
| State locking | Yes | Yes | Stable |
| Terraform state import | Yes | Yes | Stable |
| Lockfiles | Yes | Planned | v1.0 |
| Checkpoints/rollback | No | Planned | v1.0 |

---

## Plan Mode

Rustible's plan mode provides Terraform-style execution previews:

```bash
rustible plan playbook.yml -i inventory.yml
```

Output format:
```
Execution Plan:
  web1.example.com:
    + [package] Install nginx (will install)
    ~ [template] Configure nginx.conf (will modify)
    - [file] Remove old config (will delete)

  web2.example.com:
    . [package] Install nginx (already installed)
    ~ [template] Configure nginx.conf (will modify)

Apply this plan? [y/N]
```

| Symbol | Meaning |
|--------|---------|
| `+` | Resource will be created |
| `~` | Resource will be modified |
| `-` | Resource will be deleted |
| `.` | Resource unchanged (no action) |

---

## AWS Resource Support

Requires `--features aws` or `--features provisioning`.

### Implemented Resources (18 total)

| Resource Type | Terraform Equivalent | Status | Notes |
|---------------|---------------------|--------|-------|
| `aws_autoscaling_group` | `aws_autoscaling_group` | Implemented | Auto Scaling Groups with launch templates |
| `aws_db_subnet_group` | `aws_db_subnet_group` | Implemented | RDS DB Subnet Groups |
| `aws_ebs_volume` | `aws_ebs_volume` | Implemented | EBS volumes with encryption support |
| `aws_eip` | `aws_eip` | Implemented | Elastic IPs with VPC association |
| `aws_iam_policy` | `aws_iam_policy` | Implemented | IAM policies with JSON documents |
| `aws_iam_role` | `aws_iam_role` | Implemented | IAM roles with assume role policies |
| `aws_instance` | `aws_instance` | Implemented | EC2 instances with full config |
| `aws_internet_gateway` | `aws_internet_gateway` | Implemented | Internet Gateways |
| `aws_launch_template` | `aws_launch_template` | Implemented | EC2 Launch Templates |
| `aws_lb` | `aws_lb` | Implemented | ALB/NLB/GWLB load balancers |
| `aws_nat_gateway` | `aws_nat_gateway` | Implemented | NAT Gateways |
| `aws_rds_instance` | `aws_db_instance` | Implemented | RDS instances (MySQL, PostgreSQL, etc.) |
| `aws_route_table` | `aws_route_table` | Implemented | Route tables with associations |
| `aws_s3_bucket` | `aws_s3_bucket` | Implemented | S3 buckets with versioning, encryption |
| `aws_security_group` | `aws_security_group` | Implemented | Security groups with inline rules |
| `aws_security_group_rule` | `aws_security_group_rule` | Implemented | Standalone security group rules |
| `aws_subnet` | `aws_subnet` | Implemented | VPC subnets |
| `aws_vpc` | `aws_vpc` | Implemented | Virtual Private Clouds |

### Planned Resources

| Resource Type | Priority | Notes |
|---------------|----------|-------|
| `aws_lambda_function` | High | Lambda functions |
| `aws_sqs_queue` | Medium | SQS queues |
| `aws_sns_topic` | Medium | SNS topics |
| `aws_dynamodb_table` | Medium | DynamoDB tables |
| `aws_ecs_cluster` | Medium | ECS clusters |
| `aws_eks_cluster` | Low | EKS clusters |

---

## State Management

### State Commands

```bash
# Initialize state with backend configuration
rustible state init --backend s3 --bucket my-bucket --key state.json --region us-east-1

# Migrate state between backends
rustible state migrate --from local --to s3 --from-path ./state.json --to-path s3://bucket/key

# Import Terraform state
rustible state import-terraform --tfstate terraform.tfstate --output .rustible/state.json

# List states
rustible state list

# Show state details
rustible state show <name>

# Manage locks
rustible state lock list
rustible state lock release <lock-id>
```

### Remote Backends

| Backend | Status | Locking | Notes |
|---------|--------|---------|-------|
| Local | Stable | File-based | Default backend |
| S3 | Stable | DynamoDB | Full AWS integration |
| GCS | Stable | None | Google Cloud Storage |
| Azure Blob | Stable | Lease-based | Azure Blob Storage |
| Consul | Stable | Session-based | HashiCorp Consul KV |
| HTTP | Stable | HTTP Lock/Unlock | Terraform Cloud compatible |

### State File Location

```
./.rustible/provisioning.state.json
./.rustible/backend.json
```

---

## Drift Detection

Rustible supports drift detection to compare desired state against actual cloud resources:

```bash
rustible drift --playbook site.yml --inventory production.yml
```

Output:
```
╭─────────────────────────────────────────────────────────────────────────╮
│                            DRIFT DETECTION                              │
╰─────────────────────────────────────────────────────────────────────────╯

Host: web1.example.com
  ~ /etc/nginx/nginx.conf
      worker_connections: 1024 → 2048

  + /etc/nginx/conf.d/site.conf (missing)

  - /etc/nginx/conf.d/old.conf (extra file)

Summary: 1 modified, 1 missing, 1 extra
```

---

## Provider Ecosystem

### Current Status

| Provider | Resources | Status | Notes |
|----------|-----------|--------|-------|
| AWS | 18 | Stable | Core EC2, S3, IAM, RDS, ELB, ASG |
| Azure | 0 | Stub | Provisioning not yet implemented |
| GCP | 0 | Stub | Provisioning not yet implemented |
| Kubernetes | N/A | Stable | Module-based (not provisioning) |
| Docker | N/A | Stable | Module-based (not provisioning) |

### Provider SDK (Planned)

The provider ecosystem architecture includes:

1. **Provider SDK** - Rust SDK for writing providers
2. **Provider CLI** - Packaging and publishing tools
3. **Provider Registry** - Discovery and versioning

See [architecture/provider-ecosystem.md](../architecture/provider-ecosystem.md) for details.

---

## Comparison with Terraform

| Aspect | Terraform | Rustible |
|--------|-----------|----------|
| Primary use case | Infrastructure provisioning | Config management + provisioning |
| Language | HCL | YAML (Ansible-compatible) |
| State tracking | Central to design | Optional feature |
| Configuration drift | Full support | Supported |
| Provider ecosystem | Extensive (1000+) | Growing (~18 AWS resources) |
| Execution model | Graph-based | Task-based with DAG support |
| Secret management | External (Vault) | Built-in vault integration |
| Learning curve | Moderate | Low (Ansible knowledge transfers) |

---

## Migration from Terraform

### Importing Terraform State

```bash
# Import existing Terraform state into Rustible
rustible state import-terraform --tfstate terraform.tfstate

# The import preserves:
# - Resource attributes
# - Dependencies
# - Outputs
# - Lineage and serial numbers
```

### When to Use Rustible vs Terraform

**Use Rustible when:**
- You have existing Ansible playbooks
- Configuration management is primary need
- Want unified tool for provisioning + config
- Need SSH-based management

**Use Terraform when:**
- Infrastructure provisioning is primary need
- Need extensive cloud provider coverage
- Complex multi-cloud deployments
- Large team already using HCL

### Hybrid Approach

Rustible can complement Terraform:

```yaml
# Use Terraform for infrastructure
# Use Rustible for configuration

- name: Configure instances provisioned by Terraform
  hosts: "{{ lookup('file', 'terraform.tfstate') | from_json | json_query('resources[?type==`aws_instance`].instances[*].attributes.public_ip') | flatten }}"
  tasks:
    - name: Install application
      package:
        name: myapp
        state: present
```

---

## Limitations

1. **Provider Coverage**: Fewer providers than Terraform (AWS only for provisioning)
2. **HCL Support**: No HCL parsing (YAML only)
3. **Modules**: No Terraform module compatibility
4. **Azure/GCP Provisioning**: Not yet implemented (planned)

---

## Roadmap

| Version | Features |
|---------|----------|
| v0.1 | Plan mode, basic AWS resources |
| v0.2 | ✅ Remote state backends, 18 AWS resources, drift detection, state locking |
| v0.3 | Azure/GCP provisioning baseline |
| v1.0 | Lockfiles, checkpoints, provider registry |

---

*For detailed architecture, see [architecture/terraform-integration.md](../architecture/terraform-integration.md)*
