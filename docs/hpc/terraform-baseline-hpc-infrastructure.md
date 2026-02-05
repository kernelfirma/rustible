# Terraform Baseline for HPC Infrastructure

Phase 3B of the HPC Initiative - Establishing the Terraform baseline for HPC infrastructure provisioning, including capability mapping, state management, and limitations.

## Table of Contents

1. [Terraform HPC Ecosystem Overview](#1-terraform-hpc-ecosystem-overview)
2. [Capability Map by Platform](#2-capability-map-by-platform)
3. [Common Providers and Modules](#3-common-providers-and-modules)
4. [State Management Patterns](#4-state-management-patterns)
5. [Plan/Apply Workflows](#5-planapply-workflows)
6. [Strengths and Best Practices](#6-strengths-and-best-practices)
7. [Limitations and Pain Points](#7-limitations-and-pain-points)
8. [Rustible Integration Opportunities](#8-rustible-integration-opportunities)

---

## 1. Terraform HPC Ecosystem Overview

### 1.1 Terraform's Role in HPC

| Layer | Terraform Responsibility | Ansible/Rustible Responsibility |
|-------|-------------------------|--------------------------------|
| **Cloud infrastructure** | VPCs, subnets, security groups | N/A |
| **Compute instances** | Provisioning VMs, bare metal | OS configuration |
| **Storage** | Block volumes, object storage | Filesystem mounting |
| **Network** | Load balancers, DNS | Service configuration |
| **HPC services** | Managed services (ParallelCluster) | Scheduler tuning |
| **Bare metal** | Limited (via APIs) | Primary tool |

### 1.2 HPC Infrastructure Patterns

```
┌─────────────────────────────────────────────────────────────────────┐
│                    HPC Infrastructure Layers                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                    Terraform Managed                         │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │   │
│  │  │ Networking  │  │   Compute   │  │   Storage   │         │   │
│  │  │ VPC, Subnet │  │ Instances   │  │ Block, S3   │         │   │
│  │  │ Security    │  │ Auto-scale  │  │ FSx, EFS    │         │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘         │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              ↓                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                  Ansible/Rustible Managed                    │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │   │
│  │  │  Scheduler  │  │   Fabric    │  │  Software   │         │   │
│  │  │ Slurm, PBS  │  │ InfiniBand  │  │  CUDA, MPI  │         │   │
│  │  │ Config      │  │ OpenSM      │  │  Modules    │         │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘         │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.3 Deployment Models

| Model | Description | Terraform Role |
|-------|-------------|----------------|
| **Cloud-native** | Full cloud HPC (AWS, Azure, GCP) | Primary IaC tool |
| **Hybrid** | Cloud burst from on-prem | Cloud portion only |
| **On-premises** | Traditional datacenter | Limited (via MAAS, etc.) |
| **Multi-cloud** | Workloads across clouds | Unified provisioning |

---

## 2. Capability Map by Platform

### 2.1 AWS HPC Capabilities

| Capability | Terraform Support | Module/Resource |
|------------|-------------------|-----------------|
| **ParallelCluster** | ✅ Strong | `aws-tf/parallelcluster/aws` |
| **EC2 instances** | ✅ Strong | `aws_instance` |
| **EC2 placement groups** | ✅ Strong | `aws_placement_group` |
| **EFA (Elastic Fabric Adapter)** | ✅ Good | Instance attribute |
| **FSx for Lustre** | ✅ Strong | `aws_fsx_lustre_file_system` |
| **EFS** | ✅ Strong | `aws_efs_file_system` |
| **Auto Scaling** | ✅ Strong | `aws_autoscaling_group` |
| **Spot instances** | ✅ Strong | `aws_spot_instance_request` |
| **GPU instances** | ✅ Strong | Instance type selection |

**AWS ParallelCluster Terraform Module:**
```hcl
module "parallelcluster" {
  source  = "aws-tf/parallelcluster/aws"
  version = "~> 1.0"

  region       = "us-east-1"
  cluster_name = "hpc-prod"

  api_stack_name = "parallelcluster-api"

  cluster_config = {
    HeadNode = {
      InstanceType = "c5.2xlarge"
      Networking = {
        SubnetId = module.vpc.private_subnets[0]
      }
    }
    Scheduling = {
      Scheduler = "slurm"
      SlurmQueues = [{
        Name         = "compute"
        ComputeResources = [{
          Name         = "c5n-18xlarge"
          InstanceType = "c5n.18xlarge"
          MinCount     = 0
          MaxCount     = 100
        }]
      }]
    }
    SharedStorage = [{
      MountDir      = "/shared"
      Name          = "fsx"
      StorageType   = "FsxLustre"
      FsxLustreSettings = {
        StorageCapacity = 1200
      }
    }]
  }
}
```

### 2.2 Azure HPC Capabilities

| Capability | Terraform Support | Module/Resource |
|------------|-------------------|-----------------|
| **CycleCloud** | ⚠️ Limited | ARM template deployment |
| **HPC VMs (HB/HC/ND)** | ✅ Strong | `azurerm_linux_virtual_machine` |
| **VMSS (Scale Sets)** | ✅ Strong | `azurerm_linux_virtual_machine_scale_set` |
| **InfiniBand** | ⚠️ Moderate | SR-IOV enabled VMs |
| **Azure NetApp Files** | ✅ Good | `azurerm_netapp_volume` |
| **Azure Files** | ✅ Strong | `azurerm_storage_share` |
| **Proximity placement** | ✅ Good | `azurerm_proximity_placement_group` |
| **Spot VMs** | ✅ Good | Priority attribute |

**Azure HPC Example:**
```hcl
resource "azurerm_proximity_placement_group" "hpc" {
  name                = "hpc-ppg"
  location            = azurerm_resource_group.hpc.location
  resource_group_name = azurerm_resource_group.hpc.name
}

resource "azurerm_linux_virtual_machine_scale_set" "compute" {
  name                = "hpc-compute"
  resource_group_name = azurerm_resource_group.hpc.name
  location            = azurerm_resource_group.hpc.location
  sku                 = "Standard_HB120rs_v3"  # AMD EPYC, InfiniBand
  instances           = 10

  proximity_placement_group_id = azurerm_proximity_placement_group.hpc.id

  network_interface {
    name    = "nic"
    primary = true
    enable_accelerated_networking = true

    ip_configuration {
      name      = "internal"
      primary   = true
      subnet_id = azurerm_subnet.compute.id
    }
  }
}
```

### 2.3 GCP HPC Capabilities

| Capability | Terraform Support | Module/Resource |
|------------|-------------------|-----------------|
| **HPC Toolkit** | ✅ Good | Terraform blueprints |
| **Compute Engine** | ✅ Strong | `google_compute_instance` |
| **Managed Instance Groups** | ✅ Strong | `google_compute_instance_group_manager` |
| **Filestore** | ✅ Strong | `google_filestore_instance` |
| **Compact placement** | ✅ Good | Resource policy |
| **Spot/Preemptible** | ✅ Good | Scheduling option |
| **GPUs** | ✅ Strong | Guest accelerator |

**GCP HPC Toolkit Pattern:**
```hcl
module "hpc_cluster" {
  source = "github.com/GoogleCloudPlatform/hpc-toolkit//modules/compute/vm-instance"

  project_id   = var.project_id
  zone         = var.zone
  machine_type = "c2-standard-60"

  instance_count = var.compute_node_count

  metadata = {
    startup-script = file("${path.module}/scripts/compute-startup.sh")
  }

  network_interfaces = [{
    network    = module.vpc.network_name
    subnetwork = module.vpc.subnets["compute"].name
  }]
}
```

### 2.4 On-Premises/Bare Metal Capabilities

| Capability | Terraform Support | Provider/Method |
|------------|-------------------|-----------------|
| **MAAS provisioning** | ⚠️ Limited | Community provider |
| **Ironic (OpenStack)** | ⚠️ Moderate | OpenStack provider |
| **Packet/Equinix Metal** | ✅ Good | Official provider |
| **vSphere VMs** | ✅ Strong | VMware provider |
| **Physical servers** | ❌ Poor | No direct support |
| **Network switches** | ⚠️ Limited | Vendor-specific |
| **Storage arrays** | ⚠️ Limited | Vendor-specific |

**MAAS Example (Community Provider):**
```hcl
provider "maas" {
  api_url = "http://maas.example.com:5240/MAAS"
  api_key = var.maas_api_key
}

resource "maas_instance" "compute" {
  count = 10

  hostname     = "compute${format("%03d", count.index + 1)}"
  pool         = "hpc-pool"
  zone         = "default"

  deploy_params {
    distro_series = "jammy"
  }
}
```

---

## 3. Common Providers and Modules

### 3.1 Cloud Providers

| Provider | HPC Relevance | Maturity |
|----------|---------------|----------|
| **hashicorp/aws** | ParallelCluster, FSx, EFA | Production |
| **hashicorp/azurerm** | CycleCloud, HB-series | Production |
| **hashicorp/google** | HPC Toolkit | Production |
| **IBM-Cloud/ibm** | Bare metal, Spectrum Scale | Production |
| **oracle/oci** | BM instances, RDMA | Production |

### 3.2 HPC-Specific Modules

| Module | Provider | Purpose |
|--------|----------|---------|
| **aws-tf/parallelcluster/aws** | AWS | Complete HPC cluster |
| **terraform-google-modules/vm** | GCP | Compute instances |
| **Azure/avm-res-compute-virtualmachinescaleset** | Azure | VMSS for HPC |

### 3.3 Infrastructure Modules

| Module | Purpose | HPC Use Case |
|--------|---------|--------------|
| **terraform-aws-modules/vpc** | VPC creation | HPC networking |
| **terraform-aws-modules/security-group** | Security groups | Node access control |
| **terraform-google-modules/network** | GCP networking | Cluster networking |

### 3.4 On-Premises Providers

| Provider | Maturity | Notes |
|----------|----------|-------|
| **hashicorp/vsphere** | Production | VM provisioning |
| **terraform-provider-openstack** | Production | Includes Ironic |
| **maas/maas** | Community | Bare metal, unofficial |
| **equinix/metal** | Production | Bare metal cloud |

---

## 4. State Management Patterns

### 4.1 Remote State Backends

| Backend | Locking | Use Case |
|---------|---------|----------|
| **S3 + DynamoDB** | ✅ Yes | AWS environments |
| **Azure Blob** | ✅ Yes | Azure environments |
| **GCS** | ✅ Yes | GCP environments |
| **Terraform Cloud** | ✅ Yes | Multi-cloud, enterprise |
| **Consul** | ✅ Yes | On-premises |
| **PostgreSQL** | ✅ Yes | On-premises, multi-cloud |

### 4.2 S3 Backend Configuration

```hcl
terraform {
  backend "s3" {
    bucket         = "hpc-terraform-state"
    key            = "clusters/prod/terraform.tfstate"
    region         = "us-east-1"
    encrypt        = true
    dynamodb_table = "terraform-locks"
  }
}

# DynamoDB table for locking
resource "aws_dynamodb_table" "terraform_locks" {
  name         = "terraform-locks"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "LockID"

  attribute {
    name = "LockID"
    type = "S"
  }
}
```

### 4.3 State Isolation Patterns

**Workspace-based:**
```hcl
# Different workspaces for environments
terraform workspace new prod
terraform workspace new staging
terraform workspace new dev

# Access workspace in config
locals {
  environment = terraform.workspace
  node_count  = {
    prod    = 100
    staging = 10
    dev     = 2
  }[local.environment]
}
```

**Directory-based:**
```
infrastructure/
├── modules/
│   ├── vpc/
│   ├── compute/
│   └── storage/
├── environments/
│   ├── prod/
│   │   ├── main.tf
│   │   ├── terraform.tfvars
│   │   └── backend.tf
│   ├── staging/
│   └── dev/
```

### 4.4 State Locking Behavior

| Operation | Lock Acquired | Lock Duration |
|-----------|---------------|---------------|
| `terraform plan` | Read lock | Duration of plan |
| `terraform apply` | Write lock | Duration of apply |
| `terraform destroy` | Write lock | Duration of destroy |
| `terraform refresh` | Write lock | Duration of refresh |

**Force Unlock (emergency):**
```bash
terraform force-unlock <LOCK_ID>
```

### 4.5 Drift Detection

```bash
# Detect configuration drift
terraform plan -detailed-exitcode
# Exit code 0: No changes
# Exit code 1: Error
# Exit code 2: Changes detected (drift)

# Refresh state from actual infrastructure
terraform refresh

# Import existing resources
terraform import aws_instance.compute i-1234567890abcdef0
```

---

## 5. Plan/Apply Workflows

### 5.1 Standard Workflow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Terraform HPC Workflow                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  1. terraform init                                                  │
│     └── Initialize providers, modules, backend                      │
│                                                                     │
│  2. terraform validate                                              │
│     └── Syntax and configuration validation                         │
│                                                                     │
│  3. terraform plan -out=tfplan                                      │
│     └── Generate execution plan, save for review                    │
│                                                                     │
│  4. Review plan (manual or automated)                               │
│     └── Check resources to create/modify/destroy                    │
│                                                                     │
│  5. terraform apply tfplan                                          │
│     └── Execute the saved plan                                      │
│                                                                     │
│  6. Post-apply: Trigger Ansible/Rustible                           │
│     └── Configure OS, scheduler, software                           │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 5.2 CI/CD Pipeline Pattern

```yaml
# .github/workflows/terraform.yml
name: Terraform HPC Infrastructure

on:
  push:
    branches: [main]
    paths: ['infrastructure/**']
  pull_request:
    branches: [main]

jobs:
  plan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Terraform
        uses: hashicorp/setup-terraform@v3

      - name: Terraform Init
        run: terraform init
        working-directory: infrastructure/prod

      - name: Terraform Plan
        run: terraform plan -out=tfplan
        working-directory: infrastructure/prod

      - name: Upload Plan
        uses: actions/upload-artifact@v4
        with:
          name: tfplan
          path: infrastructure/prod/tfplan

  apply:
    needs: plan
    runs-on: ubuntu-latest
    if: github.ref == 'refs/heads/main'
    environment: production

    steps:
      - uses: actions/checkout@v4

      - name: Download Plan
        uses: actions/download-artifact@v4
        with:
          name: tfplan
          path: infrastructure/prod

      - name: Terraform Apply
        run: terraform apply -auto-approve tfplan
        working-directory: infrastructure/prod

      - name: Trigger Ansible
        run: |
          ansible-playbook -i inventory/dynamic site.yml
```

### 5.3 Terraform + Ansible Integration

**Output for Ansible inventory:**
```hcl
output "compute_nodes" {
  value = {
    for instance in aws_instance.compute :
    instance.tags["Name"] => {
      ansible_host       = instance.private_ip
      ansible_user       = "ec2-user"
      instance_type      = instance.instance_type
      availability_zone  = instance.availability_zone
    }
  }
}

output "ansible_inventory" {
  value = templatefile("${path.module}/templates/inventory.tpl", {
    head_nodes    = aws_instance.head
    compute_nodes = aws_instance.compute
    gpu_nodes     = aws_instance.gpu
  })
}
```

**Dynamic inventory script:**
```python
#!/usr/bin/env python3
import json
import subprocess

def get_terraform_output():
    result = subprocess.run(
        ["terraform", "output", "-json"],
        capture_output=True,
        text=True,
        cwd="/path/to/terraform"
    )
    return json.loads(result.stdout)

def main():
    outputs = get_terraform_output()
    inventory = {
        "compute": {
            "hosts": list(outputs["compute_nodes"]["value"].keys()),
            "vars": {}
        },
        "_meta": {
            "hostvars": outputs["compute_nodes"]["value"]
        }
    }
    print(json.dumps(inventory))

if __name__ == "__main__":
    main()
```

---

## 6. Strengths and Best Practices

### 6.1 Terraform Strengths for HPC

| Strength | Description | HPC Benefit |
|----------|-------------|-------------|
| **Declarative** | Desired state definition | Reproducible clusters |
| **Plan/Apply** | Preview before changes | Safe infrastructure changes |
| **State tracking** | Resource lifecycle | Drift detection |
| **Multi-cloud** | Unified language | Hybrid/multi-cloud HPC |
| **Ecosystem** | Large provider library | Cloud HPC support |
| **Modules** | Reusable components | Standardized deployments |

### 6.2 Best Practices

**Resource Organization:**
```hcl
# Use consistent naming
resource "aws_instance" "compute" {
  count = var.compute_node_count

  tags = {
    Name        = "${var.cluster_name}-compute-${format("%03d", count.index + 1)}"
    Environment = var.environment
    Role        = "compute"
    Scheduler   = "slurm"
  }
}

# Use locals for computed values
locals {
  compute_nodes = [
    for i in range(var.compute_node_count) :
    "${var.cluster_name}-compute-${format("%03d", i + 1)}"
  ]
}
```

**Variable Validation:**
```hcl
variable "compute_node_count" {
  type        = number
  description = "Number of compute nodes"

  validation {
    condition     = var.compute_node_count >= 1 && var.compute_node_count <= 1000
    error_message = "Compute node count must be between 1 and 1000."
  }
}

variable "instance_type" {
  type        = string
  description = "EC2 instance type for compute nodes"

  validation {
    condition     = can(regex("^(c5n|c6i|hpc6a|p4d|p5)\\.", var.instance_type))
    error_message = "Instance type must be HPC-optimized (c5n, c6i, hpc6a, p4d, p5)."
  }
}
```

**Lifecycle Management:**
```hcl
resource "aws_instance" "compute" {
  # ...

  lifecycle {
    create_before_destroy = true
    prevent_destroy       = false

    ignore_changes = [
      ami,  # Don't recreate on AMI updates
      tags["LastModified"],
    ]
  }
}
```

---

## 7. Limitations and Pain Points

### 7.1 General Limitations

| Limitation | Description | Impact on HPC |
|------------|-------------|---------------|
| **Destroy is destructive** | No soft delete | Accidental cluster destruction |
| **State dependency** | Operations need state | State corruption = problems |
| **Provider limitations** | API-dependent | Missing HPC features |
| **No native rollback** | Must re-apply old config | Recovery complexity |
| **Sequential by default** | Resource-level parallelism only | Slow large deployments |

### 7.2 HPC-Specific Pain Points

| Pain Point | Description | Workaround |
|------------|-------------|------------|
| **Bare metal** | No native support | Use MAAS/Ironic providers |
| **InfiniBand** | Cloud-only, limited | Vendor-specific settings |
| **Scheduler config** | Infrastructure only | Hand off to Ansible |
| **Software stack** | Not designed for CM | Use provisioners (anti-pattern) |
| **Node replacement** | Recreates, not repairs | Manual intervention |
| **License servers** | No native support | Custom resources |

### 7.3 On-Premises Challenges

| Challenge | Description | Mitigation |
|-----------|-------------|------------|
| **No physical control** | Can't power cycle hardware | Out-of-band management external |
| **Network equipment** | Limited switch support | Vendor-specific providers |
| **Storage arrays** | Few providers | API wrappers |
| **MAAS unofficial** | Community maintained | Risk of breakage |
| **Long provisioning** | Bare metal is slow | Acceptance of long apply times |

### 7.4 State Management Issues

| Issue | Description | Mitigation |
|-------|-------------|------------|
| **State file size** | Large clusters = large state | State file splitting |
| **Lock contention** | Multiple operators | Workspace separation |
| **Corruption** | Rare but catastrophic | State backups |
| **Import complexity** | Manual for existing resources | Terraformer, import blocks |
| **Drift accumulation** | Manual changes not tracked | Regular plan checks |

---

## 8. Rustible Integration Opportunities

### 8.1 Complementary Workflows

```
┌─────────────────────────────────────────────────────────────────────┐
│              Terraform + Rustible Integration                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Terraform                          Rustible                        │
│  ─────────                          ────────                        │
│  ┌─────────────────┐               ┌─────────────────┐             │
│  │ Infrastructure  │               │ Configuration   │             │
│  │ - VPCs, Subnets │     ──►       │ - Slurm config  │             │
│  │ - Instances     │   Outputs     │ - MPI stacks    │             │
│  │ - Storage       │   Inventory   │ - User software │             │
│  │ - Security      │               │ - Identity      │             │
│  └─────────────────┘               └─────────────────┘             │
│                                                                     │
│  Lifecycle: Create/Destroy         Lifecycle: Configure/Update     │
│  State: terraform.tfstate          State: Idempotent convergence   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 8.2 Rustible Advantages Over Terraform

| Area | Terraform Limitation | Rustible Opportunity |
|------|---------------------|---------------------|
| **Bare metal** | Poor support | Native IPMI/Redfish modules |
| **Scheduler** | Infrastructure only | Full Slurm/PBS management |
| **InfiniBand** | Cloud abstractions | Direct OpenSM configuration |
| **Software** | Not designed for CM | Full package/module lifecycle |
| **Rollback** | Re-apply required | Automatic state-based rollback |
| **Check mode** | Plan only | Full dry-run with validation |

### 8.3 Integration Points

| Integration | Method | Benefit |
|-------------|--------|---------|
| **Terraform outputs → Rustible inventory** | JSON/YAML export | Dynamic inventory |
| **Terraform triggers Rustible** | `local-exec` provisioner | Automated configuration |
| **Shared state** | Consul, S3 | Coordination |
| **CI/CD pipeline** | Sequential stages | End-to-end automation |

### 8.4 Terraform Bridge in Rustible

Rustible's Terraform bridge (Phase 1) enables:

```yaml
# Rustible playbook with Terraform state reading
- name: Configure HPC cluster from Terraform
  hosts: localhost
  tasks:
    - name: Read Terraform outputs
      rustible.terraform.tf_output:
        state_file: /path/to/terraform.tfstate
        # Or remote backend
        backend: s3
        backend_config:
          bucket: hpc-terraform-state
          key: clusters/prod/terraform.tfstate
      register: tf_outputs

    - name: Add compute nodes to inventory
      add_host:
        name: "{{ item.key }}"
        ansible_host: "{{ item.value.private_ip }}"
        groups: compute
      loop: "{{ tf_outputs.compute_nodes | dict2items }}"
```

### 8.5 Future Integration Roadmap

| Phase | Capability | Status |
|-------|------------|--------|
| **1** | Read Terraform state | Implemented |
| **2** | Execute Terraform from Rustible | Planned |
| **3** | Unified plan (Terraform + Rustible) | Future |
| **4** | Shared resource management | Future |

---

## References

- [AWS ParallelCluster Terraform](https://docs.aws.amazon.com/parallelcluster/latest/ug/terraform-what-is.html)
- [Terraform AWS ParallelCluster Module](https://registry.terraform.io/modules/aws-tf/parallelcluster/aws/latest)
- [Terraform State Backends](https://developer.hashicorp.com/terraform/language/state/backends)
- [Terraform State Locking](https://developer.hashicorp.com/terraform/language/state/locking)
- [GCP HPC Toolkit](https://cloud.google.com/hpc-toolkit)
- [Azure HPC VMs](https://learn.microsoft.com/en-us/azure/virtual-machines/sizes-hpc)
- [Terraform On-Premises](https://spacelift.io/blog/terraform-on-premise)
- [MAAS Terraform Provider](https://registry.terraform.io/providers/maas/maas/latest)
