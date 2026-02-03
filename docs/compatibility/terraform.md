# Terraform Integration Compatibility

> **Last Updated:** 2026-02-03
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

---

## Feature Status

| Capability | Terraform | Rustible | Status |
|------------|-----------|----------|--------|
| Plan mode preview | Yes | Yes | Stable |
| State management | Yes | Yes | Beta |
| Drift detection | Yes | Planned | v0.3 |
| Remote state backends | Yes | Yes | Beta |
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

### Implemented Resources

| Resource Type | Terraform | Rustible | Notes |
|---------------|-----------|----------|-------|
| `aws_instance` | Yes | Yes | EC2 instances |
| `aws_s3_bucket` | Yes | Partial | Basic bucket operations |
| `aws_vpc` | Yes | Partial | Basic VPC |
| `aws_subnet` | Yes | Partial | Basic subnet |
| `aws_security_group` | Yes | Partial | Basic SG |

### Planned Resources (v0.2-v1.0)

| Resource Type | Priority | Target |
|---------------|----------|--------|
| `aws_security_group_rule` | High | v0.2 |
| `aws_iam_role` | High | v0.2 |
| `aws_iam_policy` | High | v0.2 |
| `aws_ebs_volume` | Medium | v0.2 |
| `aws_db_subnet_group` | Medium | v0.2 |
| `aws_rds_instance` | Medium | v0.2 |
| `aws_lb` | Medium | v0.2 |
| `aws_launch_template` | Low | v0.3 |
| `aws_autoscaling_group` | Low | v0.3 |

---

## State Management

### Current State (v0.1)

- Local or remote provisioning state backends
- Optional state locking for collaborative workflows
- Terraform state import into Rustible state

State lifecycle commands:

```bash
rustible provision init
rustible provision migrate
rustible provision import-terraform --tfstate terraform.tfstate
```

### Planned State Features

| Feature | Status | Target |
|---------|--------|--------|
| State manifests | In Progress | v0.2 |
| Drift detection | Planned | v0.3 |
| S3 remote backend | Implemented | v0.2 |
| GCS remote backend | Implemented | v0.2 |
| Azure Blob backend | Implemented | v0.2 |
| Consul backend | Implemented | v0.2 |
| State locking | Implemented | v0.2 |
| Lockfile support | Planned | v1.0 |

### State File Location

```
./.rustible/provisioning.state.json
./.rustible/provisioning.backend.json
```

---

## Provider Ecosystem

Rustible is building a provider ecosystem similar to Terraform's model.

### Current Status

| Provider | Status | Notes |
|----------|--------|-------|
| AWS | Partial | Core EC2/S3 resources |
| Azure | Stub | Experimental |
| GCP | Stub | Experimental |
| Kubernetes | Stable | Full support |
| Docker | Stable | Full support |

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
| Configuration drift | Full support | Planned |
| Provider ecosystem | Extensive (1000+) | Growing (~10) |
| Execution model | Graph-based | Task-based with DAG support |
| Secret management | External (Vault) | Built-in vault |
| Learning curve | Moderate | Low (Ansible knowledge transfers) |

---

## Migration from Terraform

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

1. **Provider Coverage**: Far fewer providers than Terraform
2. **State Maturity**: State management is still evolving
3. **HCL Support**: No HCL parsing (YAML only)
4. **Import**: Cannot import existing Terraform state
5. **Modules**: No Terraform module compatibility

---

## Roadmap

| Version | Features |
|---------|----------|
| v0.1 | Plan mode, basic AWS resources |
| v0.2 | Remote state backends, more AWS resources |
| v0.3 | Drift detection, state locking |
| v1.0 | Lockfiles, checkpoints, provider registry |

---

*For detailed architecture, see [architecture/terraform-integration.md](../architecture/terraform-integration.md)*
