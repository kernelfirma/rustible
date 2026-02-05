# Provisioning and State Capabilities Matrix

> **Phase**: 1A - HPC Initiative
> **Last Updated**: 2026-02-05
> **Rustible Version**: 0.1.x
> **Status**: Reference Document

This document maps Rustible's provisioning and state-management capabilities relevant to HPC deployments.

---

## Overview

Rustible provides Terraform-like infrastructure provisioning alongside Ansible-compatible configuration management. Provisioning capabilities are feature-gated behind `--features provisioning` or `--features full-provisioning`.

---

## Provisioning CLI Commands

| Command | Status | Description | Evidence |
|---------|--------|-------------|----------|
| `provision plan` | **Stable** | Generate execution plan with diff preview | [`src/cli/commands/provision.rs:376-435`](../../src/cli/commands/provision.rs) |
| `provision apply` | **Stable** | Apply infrastructure changes with state locking | [`src/cli/commands/provision.rs:445-528`](../../src/cli/commands/provision.rs) |
| `provision destroy` | **Stable** | Destroy infrastructure resources | [`src/cli/commands/provision.rs:530-610`](../../src/cli/commands/provision.rs) |
| `provision import` | **Stable** | Import existing cloud resources into state | [`src/cli/commands/provision.rs:612-673`](../../src/cli/commands/provision.rs) |
| `provision show` | **Stable** | Display current state (JSON/human-readable) | [`src/cli/commands/provision.rs:675-734`](../../src/cli/commands/provision.rs) |
| `provision refresh` | **Stable** | Refresh state from cloud providers | [`src/cli/commands/provision.rs:736-781`](../../src/cli/commands/provision.rs) |
| `provision init` | **Stable** | Initialize project with backend configuration | [`src/cli/commands/provision.rs:892-1136`](../../src/cli/commands/provision.rs) |
| `provision migrate` | **Stable** | Migrate state to current schema version | [`src/cli/commands/provision.rs:783-827`](../../src/cli/commands/provision.rs) |
| `provision import-terraform` | **Stable** | Import Terraform state into Rustible format | [`src/cli/commands/provision.rs:829-890`](../../src/cli/commands/provision.rs) |

### CLI Options

| Option | Commands | Status | Notes |
|--------|----------|--------|-------|
| `--config-file` | All | **Stable** | Infrastructure YAML path (default: `infrastructure.rustible.yml`) |
| `--state` | All | **Stable** | Override state file path |
| `--backend-config` | All | **Stable** | External backend configuration file (JSON/YAML) |
| `-t, --target` | plan/apply/destroy/refresh | **Stable** | Target specific resources |
| `--auto-approve` | apply/destroy | **Stable** | Skip interactive confirmation |
| `--parallelism` | apply | **Stable** | Max parallel operations (default: 10) |
| `--no-lock` | apply | **Stable** | Skip state locking |
| `--no-backup` | apply | **Stable** | Skip state backup before changes |
| `--destroy` | plan | **Stable** | Generate destroy plan |
| `--json` | show | **Stable** | Output as JSON |

---

## State Management

### State Commands

| Command | Status | Description | Evidence |
|---------|--------|-------------|----------|
| `state init` | **Stable** | Initialize state with backend configuration | [`src/cli/commands/state.rs:236-379`](../../src/cli/commands/state.rs) |
| `state migrate` | **Stable** | Migrate state between backends | [`src/cli/commands/state.rs:381-444`](../../src/cli/commands/state.rs) |
| `state import-terraform` | **Stable** | Import Terraform state | [`src/cli/commands/state.rs:446-546`](../../src/cli/commands/state.rs) |
| `state list` | **Stable** | List available states | [`src/cli/commands/state.rs:548-603`](../../src/cli/commands/state.rs) |
| `state show` | **Stable** | Show state details | [`src/cli/commands/state.rs:605-623`](../../src/cli/commands/state.rs) |
| `state pull` | **Stable** | Pull remote state to local | [`src/cli/commands/state.rs:625-663`](../../src/cli/commands/state.rs) |
| `state push` | **Stable** | Push local state to remote | [`src/cli/commands/state.rs:665-701`](../../src/cli/commands/state.rs) |
| `state rm` | **Stable** | Remove state entry | [`src/cli/commands/state.rs:703-727`](../../src/cli/commands/state.rs) |
| `state lock list` | **Stable** | List active locks | [`src/cli/commands/state.rs:730-761`](../../src/cli/commands/state.rs) |
| `state lock release` | **Stable** | Force-release a lock | [`src/cli/commands/state.rs:763-776`](../../src/cli/commands/state.rs) |

### State Lifecycle

| Capability | Status | Description | Evidence |
|------------|--------|-------------|----------|
| State versioning | **Stable** | Schema versioning with serial increments | [`src/provisioning/state.rs:1-100`](../../src/provisioning/state.rs) |
| State diff | **Stable** | Compare states (added/removed/modified) | [`src/provisioning/state.rs:194-332`](../../src/provisioning/state.rs) |
| State migration | **Stable** | Upgrade state from older versions | [`src/provisioning/mod.rs:153-155`](../../src/provisioning/mod.rs) |
| Change history | **Stable** | Track historical changes with timestamps | [`src/provisioning/state.rs:368-400`](../../src/provisioning/state.rs) |
| Terraform import | **Stable** | Convert Terraform state to Rustible format | [`src/cli/commands/state.rs:786-906`](../../src/cli/commands/state.rs) |
| Resource tainting | **Stable** | Mark resources for replacement | [`src/provisioning/state.rs:150-159`](../../src/provisioning/state.rs) |
| Atomic writes | **Stable** | Temp file + rename for safety | [`src/provisioning/state_backends.rs:161-174`](../../src/provisioning/state_backends.rs) |

---

## State Backends

| Backend | Status | Locking | Feature Flag | Evidence |
|---------|--------|---------|--------------|----------|
| **Local** | **Stable** | File-based | None (default) | [`src/provisioning/state_backends.rs:89-200`](../../src/provisioning/state_backends.rs) |
| **S3** | **Stable** | DynamoDB | `aws` | [`src/provisioning/state_backends.rs:207-400`](../../src/provisioning/state_backends.rs) |
| **GCS** | **Stable** | None | `gcs` | [`src/cli/commands/state.rs:299-314`](../../src/cli/commands/state.rs) |
| **Azure Blob** | **Stable** | Lease-based | `azure` | [`src/cli/commands/state.rs:315-335`](../../src/cli/commands/state.rs) |
| **Consul** | **Stable** | Session-based | None | [`src/cli/commands/state.rs:336-351`](../../src/cli/commands/state.rs) |
| **HTTP** | **Stable** | HTTP Lock/Unlock | None | [`src/cli/commands/state.rs:352-366`](../../src/cli/commands/state.rs) |

### Backend Configuration

```yaml
# Local backend (default)
backend: local
path: .rustible/provisioning.state.json

# S3 backend with DynamoDB locking
backend: s3
bucket: my-terraform-state
key: prod/terraform.tfstate
region: us-east-1
dynamodb_table: terraform-locks

# Azure Blob backend
backend: azurerm
storage_account_name: mystorageaccount
container_name: rustible-state
key: terraform.tfstate

# Consul backend
backend: consul
address: http://127.0.0.1:8500
path: rustible/state

# HTTP backend (Terraform Cloud compatible)
backend: http
address: https://app.terraform.io/api/v2/...
```

---

## State Locking

| Lock Type | Backend | Status | Evidence |
|-----------|---------|--------|----------|
| File lock | Local | **Stable** | [`src/provisioning/state_lock.rs:241-428`](../../src/provisioning/state_lock.rs) |
| DynamoDB lock | S3 | **Stable** | [`src/provisioning/state_lock.rs:437-656`](../../src/provisioning/state_lock.rs) |
| In-memory lock | Testing | **Stable** | [`src/provisioning/state_lock.rs:662-741`](../../src/provisioning/state_lock.rs) |
| Lock timeout | All | **Stable** | Default 30s, configurable | [`src/provisioning/state_lock.rs:776-780`](../../src/provisioning/state_lock.rs) |
| Lock expiration | All | **Stable** | Default 1 hour, configurable | [`src/provisioning/state_lock.rs:783-793`](../../src/provisioning/state_lock.rs) |
| Force unlock | All | **Stable** | Manual lock release | [`src/provisioning/state_lock.rs:851-866`](../../src/provisioning/state_lock.rs) |
| RAII guards | All | **Stable** | Auto-release on drop | [`src/provisioning/state_lock.rs:906-958`](../../src/provisioning/state_lock.rs) |

---

## Inventory Sources

| Source | Status | Description | Evidence |
|--------|--------|-------------|----------|
| **YAML** | **Stable** | Ansible-compatible YAML inventory | [`src/inventory/mod.rs:38-49`](../../src/inventory/mod.rs) |
| **INI** | **Stable** | Ansible-compatible INI inventory | [`src/inventory/mod.rs:24-36`](../../src/inventory/mod.rs) |
| **JSON** | **Stable** | Dynamic inventory JSON format | [`src/inventory/mod.rs:51-63`](../../src/inventory/mod.rs) |
| **Script** | **Stable** | Executable dynamic inventory | [`src/inventory/plugin.rs`](../../src/inventory/plugin.rs) |
| **AWS EC2** | **Stable** | EC2 instance discovery | [`src/inventory/plugins/aws_ec2.rs`](../../src/inventory/plugins/aws_ec2.rs) |
| **Azure** | **Stable** | Azure VM discovery | [`src/inventory/plugins/azure.rs`](../../src/inventory/plugins/azure.rs) |
| **GCP** | **Stable** | GCP instance discovery | [`src/inventory/plugins/gcp.rs`](../../src/inventory/plugins/gcp.rs) |
| **Terraform** | **Stable** | Dynamic inventory from Terraform state | [`src/inventory/plugins/terraform.rs`](../../src/inventory/plugins/terraform.rs) |
| **Proxmox** | **Stable** | Proxmox VE discovery | [`src/inventory/plugins/proxmox.rs`](../../src/inventory/plugins/proxmox.rs) |
| **Constructed** | **Stable** | Compose hosts/groups from other sources | [`src/inventory/constructed.rs`](../../src/inventory/constructed.rs) |

### Terraform Inventory Plugin Features

| Feature | Status | Description | Evidence |
|---------|--------|-------------|----------|
| Local state | **Stable** | Read from local `.tfstate` file | [`src/inventory/plugins/terraform.rs:70-87`](../../src/inventory/plugins/terraform.rs) |
| S3 backend | **Stable** | Read state from S3 bucket | [`src/inventory/plugins/terraform.rs:72-78`](../../src/inventory/plugins/terraform.rs) |
| HTTP backend | **Stable** | Read state from HTTP endpoint | [`src/inventory/plugins/terraform.rs:79`](../../src/inventory/plugins/terraform.rs) |
| Resource mappings | **Stable** | Map TF resources to inventory hosts | [`src/inventory/plugins/terraform.rs:207-229`](../../src/inventory/plugins/terraform.rs) |
| Output export | **Stable** | Export TF outputs as group vars | [`src/inventory/plugins/terraform.rs:176-177`](../../src/inventory/plugins/terraform.rs) |
| State caching | **Stable** | TTL-based caching (default 300s) | [`src/inventory/plugins/terraform.rs:242-256`](../../src/inventory/plugins/terraform.rs) |

---

## Terraform Variable Import

| Capability | Status | Description | Evidence |
|------------|--------|-------------|----------|
| Local state vars | **Stable** | Import from local `.tfstate` | [`src/vars/terraform.rs:54-80`](../../src/vars/terraform.rs) |
| S3 state vars | **Stable** | Import from S3 bucket | [`src/vars/terraform.rs:134-152`](../../src/vars/terraform.rs) |
| HTTP state vars | **Stable** | Import from HTTP endpoint | [`src/vars/terraform.rs:153-158`](../../src/vars/terraform.rs) |
| Output filtering | **Stable** | Select specific outputs | [`src/vars/terraform.rs:67-72`](../../src/vars/terraform.rs) |
| Sensitive handling | **Stable** | Opt-in sensitive value import | [`src/vars/terraform.rs:73-76`](../../src/vars/terraform.rs) |
| Namespace prefix | **Stable** | Variables prefixed with `terraform_` | [`src/vars/terraform.rs:76`](../../src/vars/terraform.rs) |

```yaml
# vars_files example with Terraform outputs
vars_files:
  - terraform: ./terraform.tfstate
  - terraform:
      backend: s3
      bucket: my-state-bucket
      key: prod/terraform.tfstate
      region: us-east-1
      outputs: [vpc_id, subnet_ids]
      include_sensitive: false
```

---

## AWS Resources (Provisioning)

| Resource Type | Status | Notes | Evidence |
|---------------|--------|-------|----------|
| `aws_vpc` | **Stable** | Virtual Private Cloud | [`src/provisioning/resources/aws/vpc.rs`](../../src/provisioning/resources/aws/vpc.rs) |
| `aws_subnet` | **Stable** | VPC Subnets | [`src/provisioning/resources/aws/subnet.rs`](../../src/provisioning/resources/aws/subnet.rs) |
| `aws_security_group` | **Stable** | Security Groups with inline rules | [`src/provisioning/resources/aws/security_group.rs`](../../src/provisioning/resources/aws/security_group.rs) |
| `aws_security_group_rule` | **Stable** | Standalone SG rules | [`src/provisioning/resources/aws/security_group_rule.rs`](../../src/provisioning/resources/aws/security_group_rule.rs) |
| `aws_instance` | **Stable** | EC2 Instances | [`src/provisioning/resources/aws/instance.rs`](../../src/provisioning/resources/aws/instance.rs) |
| `aws_internet_gateway` | **Stable** | Internet Gateways | [`src/provisioning/resources/aws/internet_gateway.rs`](../../src/provisioning/resources/aws/internet_gateway.rs) |
| `aws_nat_gateway` | **Stable** | NAT Gateways | [`src/provisioning/resources/aws/nat_gateway.rs`](../../src/provisioning/resources/aws/nat_gateway.rs) |
| `aws_route_table` | **Stable** | Route Tables | [`src/provisioning/resources/aws/route_table.rs`](../../src/provisioning/resources/aws/route_table.rs) |
| `aws_eip` | **Stable** | Elastic IPs | [`src/provisioning/resources/aws/elastic_ip.rs`](../../src/provisioning/resources/aws/elastic_ip.rs) |
| `aws_ebs_volume` | **Stable** | EBS Volumes with encryption | [`src/provisioning/resources/aws/ebs_volume.rs`](../../src/provisioning/resources/aws/ebs_volume.rs) |
| `aws_s3_bucket` | **Stable** | S3 Buckets | [`src/provisioning/resources/aws/s3_bucket.rs`](../../src/provisioning/resources/aws/s3_bucket.rs) |
| `aws_iam_role` | **Stable** | IAM Roles | [`src/provisioning/resources/aws/iam_role.rs`](../../src/provisioning/resources/aws/iam_role.rs) |
| `aws_iam_policy` | **Stable** | IAM Policies | [`src/provisioning/resources/aws/iam_policy.rs`](../../src/provisioning/resources/aws/iam_policy.rs) |
| `aws_rds_instance` | **Stable** | RDS Instances | [`src/provisioning/resources/aws/rds_instance.rs`](../../src/provisioning/resources/aws/rds_instance.rs) |
| `aws_db_subnet_group` | **Stable** | RDS Subnet Groups | [`src/provisioning/resources/aws/db_subnet_group.rs`](../../src/provisioning/resources/aws/db_subnet_group.rs) |
| `aws_lb` | **Stable** | Load Balancers (ALB/NLB) | [`src/provisioning/resources/aws/load_balancer.rs`](../../src/provisioning/resources/aws/load_balancer.rs) |
| `aws_launch_template` | **Stable** | EC2 Launch Templates | [`src/provisioning/resources/aws/launch_template.rs`](../../src/provisioning/resources/aws/launch_template.rs) |
| `aws_autoscaling_group` | **Stable** | Auto Scaling Groups | [`src/provisioning/resources/aws/autoscaling_group.rs`](../../src/provisioning/resources/aws/autoscaling_group.rs) |

**Total**: 18 AWS resources implemented

---

## Experimental Flags and Feature Gates

| Feature Flag | Description | Resources Enabled |
|--------------|-------------|-------------------|
| `provisioning` | Core provisioning capabilities | State management, plan/apply, local backend |
| `aws` | AWS provider and resources | S3 backend, DynamoDB locking, 18 AWS resources |
| `gcs` | Google Cloud Storage backend | GCS state backend |
| `azure` | Azure Blob backend | Azure Blob state backend |
| `full-provisioning` | All provisioning features | Combines provisioning + aws |

```bash
# Build with provisioning support
cargo build --release --features provisioning

# Build with full AWS support
cargo build --release --features full-provisioning

# Build with specific backends
cargo build --release --features "provisioning,aws,gcs"
```

---

## Known Limitations

### HPC-Critical Blockers

| Limitation | Impact | Workaround | Priority |
|------------|--------|------------|----------|
| No lockfiles | Cannot pin provider versions | Manual version control | **v1.0 Planned** |
| No checkpoints | Cannot rollback partial applies | State backups before apply | **v1.0 Planned** |
| Azure/GCP provisioning | Cannot provision Azure/GCP resources | Use Terraform for provisioning | **v0.3 Planned** |
| No Terraform module compatibility | Cannot reuse TF modules | Re-implement in Rustible YAML | N/A |
| No workspace support | Single state per project | Multiple config files | **Planned** |

### General Limitations

1. **Provider Coverage**: Only AWS resources for provisioning (18 total). Azure/GCP require Terraform.

2. **HCL Support**: No HCL parsing - YAML only for configuration.

3. **State Encryption**: Local state files are not encrypted at rest. Use S3 with SSE or Azure Blob encryption.

4. **Large State Files**: No state splitting or workspace partitioning. For very large clusters, consider state file segmentation.

5. **Concurrent Operations**: State locking prevents concurrent applies but doesn't queue operations.

---

## HPC-Relevant Capabilities Summary

| Capability | Status | HPC Relevance |
|------------|--------|---------------|
| Parallel provisioning | **Stable** | Faster cluster deployment |
| State locking | **Stable** | Safe multi-user workflows |
| Terraform state import | **Stable** | Migrate existing HPC infra |
| AWS ASG support | **Stable** | Elastic compute scaling |
| Launch templates | **Stable** | Consistent node configuration |
| EBS volumes | **Stable** | Persistent storage for compute |
| S3 remote state | **Stable** | Team collaboration |
| DynamoDB locking | **Stable** | Distributed lock safety |
| Dynamic inventory | **Stable** | Auto-discover cluster nodes |
| Terraform vars import | **Stable** | Bridge TF and CM workflows |

---

## Related Documentation

- [Terraform Integration](./terraform.md) - Detailed Terraform compatibility
- [Architecture: Terraform Integration](../architecture/terraform-integration.md) - Technical design
- [Architecture: Provider Ecosystem](../architecture/provider-ecosystem.md) - Provider SDK roadmap

---

*Document generated for HPC Phase 1A Initiative*
