# Combined Ansible+Terraform Workflow Patterns for HPC

Phase 3C of the HPC Initiative - Documenting real-world Ansible+Terraform handoff patterns, integration strategies, and pain points in HPC environments.

## Table of Contents

1. [Workflow Overview](#1-workflow-overview)
2. [Integration Patterns](#2-integration-patterns)
3. [Inventory Handoff Strategies](#3-inventory-handoff-strategies)
4. [End-to-End HPC Workflow](#4-end-to-end-hpc-workflow)
5. [Drift Detection and Remediation](#5-drift-detection-and-remediation)
6. [Failure Recovery](#6-failure-recovery)
7. [Pain Points by Stage](#7-pain-points-by-stage)
8. [Tool Boundary Summary](#8-tool-boundary-summary)

---

## 1. Workflow Overview

### 1.1 Tool Responsibilities

```
┌─────────────────────────────────────────────────────────────────────┐
│                    HPC Infrastructure Lifecycle                      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Day 0: Provisioning (Terraform)                                    │
│  ─────────────────────────────────                                  │
│  • Cloud resources (VPC, instances, storage)                        │
│  • Bare metal allocation (via MAAS/Ironic)                          │
│  • Network infrastructure                                           │
│  • Identity (IAM roles, service accounts)                           │
│                                                                     │
│  Day 1: Configuration (Ansible/Rustible)                            │
│  ────────────────────────────────────────                           │
│  • OS configuration and hardening                                   │
│  • Scheduler installation (Slurm, PBS)                              │
│  • Software stack (CUDA, MPI, modules)                              │
│  • Identity integration (LDAP, Kerberos)                            │
│  • Monitoring and logging                                           │
│                                                                     │
│  Day 2+: Operations (Both tools)                                    │
│  ───────────────────────────────                                    │
│  • Terraform: Scale up/down, infrastructure changes                 │
│  • Ansible: Config updates, patching, software updates              │
│  • Both: Drift detection and remediation                            │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.2 Handoff Points

| Stage | From | To | Handoff Data |
|-------|------|----|--------------|
| **Infrastructure ready** | Terraform | Ansible | IP addresses, hostnames, metadata |
| **Inventory update** | Terraform outputs | Ansible inventory | Dynamic inventory JSON |
| **Config complete** | Ansible | Monitoring | Service endpoints, health checks |
| **Scale event** | Terraform | Ansible | New nodes to configure |
| **Decommission** | Ansible | Terraform | Drained nodes to destroy |

---

## 2. Integration Patterns

### 2.1 Pattern 1: Decoupled (CI/CD Orchestrated)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Decoupled Workflow Pattern                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  CI/CD Pipeline (GitHub Actions, GitLab CI, Jenkins)                │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                                                             │   │
│  │  Stage 1: Terraform                                         │   │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐       │   │
│  │  │  init   │─▶│  plan   │─▶│ approve │─▶│  apply  │       │   │
│  │  └─────────┘  └─────────┘  └─────────┘  └────┬────┘       │   │
│  │                                              │              │   │
│  │                                              ▼              │   │
│  │                                    ┌─────────────────┐     │   │
│  │                                    │ Export outputs  │     │   │
│  │                                    │ (JSON/YAML)     │     │   │
│  │                                    └────────┬────────┘     │   │
│  │                                              │              │   │
│  │  Stage 2: Ansible                            ▼              │   │
│  │  ┌─────────────────┐  ┌─────────┐  ┌─────────────────┐    │   │
│  │  │ Generate        │─▶│ verify  │─▶│ ansible-playbook│    │   │
│  │  │ inventory       │  │ access  │  │ site.yml        │    │   │
│  │  └─────────────────┘  └─────────┘  └─────────────────┘    │   │
│  │                                                             │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

**Advantages:**
- Clear separation of concerns
- Independent versioning and testing
- Easy to debug each stage

**Disadvantages:**
- Manual coordination required
- Inventory sync can lag
- Two sets of state to manage

### 2.2 Pattern 2: Terraform-Triggered Ansible

```hcl
# Terraform triggers Ansible via local-exec provisioner
resource "aws_instance" "compute" {
  count         = var.compute_node_count
  ami           = var.ami_id
  instance_type = var.instance_type

  provisioner "local-exec" {
    command = <<-EOT
      ansible-playbook \
        -i '${self.private_ip},' \
        -u ec2-user \
        --private-key ${var.ssh_key_path} \
        playbooks/compute-node.yml
    EOT
  }
}

# Or via null_resource for batch configuration
resource "null_resource" "configure_cluster" {
  depends_on = [aws_instance.compute]

  triggers = {
    instance_ids = join(",", aws_instance.compute[*].id)
  }

  provisioner "local-exec" {
    command = <<-EOT
      # Generate inventory
      terraform output -json compute_nodes > /tmp/inventory.json

      # Run Ansible
      ansible-playbook \
        -i inventory/terraform_inventory.py \
        site.yml
    EOT
  }
}
```

**Advantages:**
- Automated handoff
- Single workflow trigger

**Disadvantages:**
- Provisioners are considered anti-pattern
- Failure handling is complex
- Tight coupling

### 2.3 Pattern 3: Ansible Automation Platform (AAP) Integration

```hcl
# Terraform AAP Provider
terraform {
  required_providers {
    aap = {
      source = "ansible/aap"
    }
  }
}

provider "aap" {
  host     = var.aap_host
  username = var.aap_username
  password = var.aap_password
}

# Sync inventory to AAP
resource "aap_inventory" "hpc_cluster" {
  name         = "hpc-${var.cluster_name}"
  organization = var.aap_organization
}

resource "aap_host" "compute" {
  for_each = aws_instance.compute

  inventory_id = aap_inventory.hpc_cluster.id
  name         = each.value.tags["Name"]
  variables = jsonencode({
    ansible_host      = each.value.private_ip
    instance_type     = each.value.instance_type
    availability_zone = each.value.availability_zone
  })
}

# Trigger job template after provisioning
resource "aap_job" "configure_cluster" {
  depends_on = [aap_host.compute]

  job_template_id = var.configure_template_id
  inventory_id    = aap_inventory.hpc_cluster.id

  extra_vars = jsonencode({
    cluster_name = var.cluster_name
  })
}
```

**Advantages:**
- Enterprise-grade integration
- Automatic inventory sync
- Job orchestration built-in

**Disadvantages:**
- Requires AAP license
- Additional infrastructure
- Learning curve

### 2.4 Pattern 4: Shared State Store

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Shared State Pattern                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────┐                      ┌─────────────┐              │
│  │  Terraform  │                      │   Ansible   │              │
│  └──────┬──────┘                      └──────┬──────┘              │
│         │                                    │                      │
│         │  Write                       Read  │                      │
│         ▼                                    ▼                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                    Shared State Store                        │   │
│  │                  (Consul, etcd, Redis)                       │   │
│  │                                                              │   │
│  │  Keys:                                                       │   │
│  │  - hpc/cluster/nodes/compute/*                               │   │
│  │  - hpc/cluster/config/slurm/*                                │   │
│  │  - hpc/cluster/status/*                                      │   │
│  │                                                              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

**Implementation with Consul:**
```hcl
# Terraform writes to Consul
resource "consul_keys" "compute_nodes" {
  key {
    path  = "hpc/cluster/${var.cluster_name}/nodes"
    value = jsonencode({
      for instance in aws_instance.compute :
      instance.tags["Name"] => {
        ip   = instance.private_ip
        type = instance.instance_type
      }
    })
  }
}
```

```yaml
# Ansible reads from Consul
- name: Get cluster nodes from Consul
  community.general.consul_kv:
    key: "hpc/cluster/{{ cluster_name }}/nodes"
  register: consul_nodes

- name: Add hosts to inventory
  add_host:
    name: "{{ item.key }}"
    ansible_host: "{{ item.value.ip }}"
    groups: compute
  loop: "{{ (consul_nodes.data.Value | from_json) | dict2items }}"
```

---

## 3. Inventory Handoff Strategies

### 3.1 Terraform Output to Ansible Inventory

**Terraform outputs:**
```hcl
output "ansible_inventory" {
  value = {
    all = {
      children = {
        head_nodes = {
          hosts = {
            for instance in aws_instance.head :
            instance.tags["Name"] => {
              ansible_host = instance.private_ip
              ansible_user = "ec2-user"
            }
          }
        }
        compute_nodes = {
          hosts = {
            for instance in aws_instance.compute :
            instance.tags["Name"] => {
              ansible_host       = instance.private_ip
              ansible_user       = "ec2-user"
              instance_type      = instance.instance_type
              availability_zone  = instance.availability_zone
            }
          }
        }
        gpu_nodes = {
          hosts = {
            for instance in aws_instance.gpu :
            instance.tags["Name"] => {
              ansible_host  = instance.private_ip
              ansible_user  = "ec2-user"
              gpu_count     = 4
              gpu_type      = "a100"
            }
          }
        }
      }
    }
  }
}
```

**Generate inventory file:**
```bash
terraform output -json ansible_inventory | \
  python3 -c "import sys,json,yaml; print(yaml.dump(json.load(sys.stdin)))" \
  > inventory/cluster.yml
```

### 3.2 Dynamic Inventory Script

```python
#!/usr/bin/env python3
"""
Terraform state dynamic inventory for Ansible.
Usage: ansible-playbook -i terraform_inventory.py site.yml
"""

import json
import subprocess
import sys
from pathlib import Path

TERRAFORM_DIR = Path(__file__).parent.parent / "terraform"

def get_terraform_state():
    """Read Terraform state."""
    result = subprocess.run(
        ["terraform", "show", "-json"],
        capture_output=True,
        text=True,
        cwd=TERRAFORM_DIR
    )
    if result.returncode != 0:
        sys.exit(1)
    return json.loads(result.stdout)

def build_inventory(state):
    """Build Ansible inventory from Terraform state."""
    inventory = {
        "_meta": {"hostvars": {}},
        "all": {"children": ["head", "compute", "gpu", "storage"]}
    }

    groups = {
        "head": [],
        "compute": [],
        "gpu": [],
        "storage": []
    }

    # Parse resources from state
    for resource in state.get("values", {}).get("root_module", {}).get("resources", []):
        if resource["type"] == "aws_instance":
            attrs = resource["values"]
            name = attrs["tags"].get("Name", resource["name"])
            role = attrs["tags"].get("Role", "compute")

            groups.get(role, groups["compute"]).append(name)

            inventory["_meta"]["hostvars"][name] = {
                "ansible_host": attrs["private_ip"],
                "ansible_user": "ec2-user",
                "instance_type": attrs["instance_type"],
                "instance_id": attrs["id"],
            }

    for group, hosts in groups.items():
        inventory[group] = {"hosts": hosts}

    return inventory

def main():
    if len(sys.argv) == 2 and sys.argv[1] == "--list":
        state = get_terraform_state()
        inventory = build_inventory(state)
        print(json.dumps(inventory, indent=2))
    elif len(sys.argv) == 3 and sys.argv[1] == "--host":
        # Return empty dict for host vars (already in _meta)
        print(json.dumps({}))
    else:
        print("Usage: terraform_inventory.py --list | --host <hostname>")
        sys.exit(1)

if __name__ == "__main__":
    main()
```

### 3.3 Terraform Inventory Provider (cloud.terraform collection)

```yaml
# ansible.cfg
[inventory]
enable_plugins = cloud.terraform.terraform_provider

# inventory/terraform.yml
plugin: cloud.terraform.terraform_provider
project_path: ../terraform
```

```yaml
# Direct state file access
plugin: cloud.terraform.terraform_state
backend_type: s3
backend_config:
  bucket: hpc-terraform-state
  key: clusters/prod/terraform.tfstate
  region: us-east-1
```

---

## 4. End-to-End HPC Workflow

### 4.1 Complete Workflow Map

```
┌─────────────────────────────────────────────────────────────────────┐
│                    HPC Cluster Deployment Workflow                   │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ STAGE 1: Infrastructure Planning                             │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │ • terraform init (providers, modules)                        │   │
│  │ • terraform plan -out=tfplan                                 │   │
│  │ • Review plan (manual or automated)                          │   │
│  │ • Approval gate                                              │   │
│  │                                                              │   │
│  │ Pain points: Long plan times, complex dependencies           │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ STAGE 2: Infrastructure Provisioning                         │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │ • terraform apply tfplan                                     │   │
│  │ • Wait for instances to be running                           │   │
│  │ • Export outputs (IPs, hostnames, metadata)                  │   │
│  │ • Store state (S3, Consul, etc.)                             │   │
│  │                                                              │   │
│  │ Pain points: Long provision times, partial failures          │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ STAGE 3: Inventory Handoff                                   │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │ • Generate Ansible inventory from Terraform                  │   │
│  │ • Verify SSH connectivity to all nodes                       │   │
│  │ • Validate node metadata (IPs, hostnames)                    │   │
│  │                                                              │   │
│  │ Pain points: Timing issues, SSH key distribution             │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ STAGE 4: Base Configuration                                  │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │ • ansible-playbook common.yml (NTP, DNS, users)              │   │
│  │ • ansible-playbook security.yml (firewall, SELinux)          │   │
│  │ • ansible-playbook identity.yml (LDAP, Kerberos)             │   │
│  │                                                              │   │
│  │ Pain points: Idempotency issues, ordering dependencies       │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ STAGE 5: HPC Stack Configuration                             │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │ • ansible-playbook scheduler.yml (Slurm, PBS)                │   │
│  │ • ansible-playbook storage.yml (Lustre, BeeGFS mounts)       │   │
│  │ • ansible-playbook fabric.yml (InfiniBand, IPoIB)            │   │
│  │ • ansible-playbook gpu.yml (NVIDIA driver, CUDA)             │   │
│  │ • ansible-playbook software.yml (Lmod, MPI, modules)         │   │
│  │                                                              │   │
│  │ Pain points: Long execution, reboot requirements             │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ STAGE 6: Validation and Integration                          │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │ • Run health checks (scheduler, storage, network)            │   │
│  │ • Register nodes with scheduler                              │   │
│  │ • Configure monitoring (Prometheus, Grafana)                 │   │
│  │ • Run test jobs                                              │   │
│  │                                                              │   │
│  │ Pain points: Validation coverage, test job failures          │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ STAGE 7: Production Handoff                                  │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │ • Enable nodes for job scheduling                            │   │
│  │ • Update documentation                                       │   │
│  │ • Notify users                                               │   │
│  │                                                              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 4.2 CI/CD Pipeline Implementation

```yaml
# .github/workflows/hpc-deploy.yml
name: HPC Cluster Deployment

on:
  push:
    branches: [main]
    paths:
      - 'terraform/**'
      - 'ansible/**'
  workflow_dispatch:
    inputs:
      action:
        description: 'Action to perform'
        required: true
        default: 'plan'
        type: choice
        options:
          - plan
          - apply
          - configure
          - full-deploy

env:
  TF_VAR_cluster_name: hpc-prod
  ANSIBLE_HOST_KEY_CHECKING: false

jobs:
  terraform-plan:
    runs-on: ubuntu-latest
    outputs:
      plan_exit_code: ${{ steps.plan.outputs.exitcode }}
    steps:
      - uses: actions/checkout@v4

      - name: Setup Terraform
        uses: hashicorp/setup-terraform@v3

      - name: Terraform Init
        working-directory: terraform
        run: terraform init

      - name: Terraform Plan
        id: plan
        working-directory: terraform
        run: |
          terraform plan -detailed-exitcode -out=tfplan
        continue-on-error: true

      - name: Upload Plan
        uses: actions/upload-artifact@v4
        with:
          name: tfplan
          path: terraform/tfplan

  terraform-apply:
    needs: terraform-plan
    if: |
      needs.terraform-plan.outputs.plan_exit_code == '2' &&
      (github.event.inputs.action == 'apply' || github.event.inputs.action == 'full-deploy')
    runs-on: ubuntu-latest
    environment: production
    outputs:
      inventory_json: ${{ steps.output.outputs.inventory }}
    steps:
      - uses: actions/checkout@v4

      - name: Download Plan
        uses: actions/download-artifact@v4
        with:
          name: tfplan
          path: terraform

      - name: Setup Terraform
        uses: hashicorp/setup-terraform@v3

      - name: Terraform Init
        working-directory: terraform
        run: terraform init

      - name: Terraform Apply
        working-directory: terraform
        run: terraform apply -auto-approve tfplan

      - name: Export Inventory
        id: output
        working-directory: terraform
        run: |
          terraform output -json ansible_inventory > inventory.json
          echo "inventory=$(cat inventory.json | base64 -w0)" >> $GITHUB_OUTPUT

      - name: Upload Inventory
        uses: actions/upload-artifact@v4
        with:
          name: ansible-inventory
          path: terraform/inventory.json

  ansible-configure:
    needs: [terraform-plan, terraform-apply]
    if: |
      always() &&
      (github.event.inputs.action == 'configure' || github.event.inputs.action == 'full-deploy')
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Download Inventory
        uses: actions/download-artifact@v4
        with:
          name: ansible-inventory
          path: ansible/inventory

      - name: Setup SSH Key
        run: |
          mkdir -p ~/.ssh
          echo "${{ secrets.SSH_PRIVATE_KEY }}" > ~/.ssh/id_rsa
          chmod 600 ~/.ssh/id_rsa

      - name: Wait for SSH
        run: |
          python3 scripts/wait_for_ssh.py ansible/inventory/inventory.json

      - name: Run Ansible
        working-directory: ansible
        run: |
          ansible-playbook \
            -i inventory/terraform_inventory.py \
            site.yml

      - name: Run Validation
        working-directory: ansible
        run: |
          ansible-playbook \
            -i inventory/terraform_inventory.py \
            validate.yml
```

---

## 5. Drift Detection and Remediation

### 5.1 Drift Detection Strategy

| Layer | Tool | Detection Method | Frequency |
|-------|------|------------------|-----------|
| **Infrastructure** | Terraform | `terraform plan` | Daily/On-demand |
| **Configuration** | Ansible | Check mode | Daily |
| **Combined** | CI/CD | Both tools | Scheduled |

### 5.2 Terraform Drift Detection

```bash
#!/bin/bash
# drift-check.sh - Scheduled drift detection

set -e

cd /opt/hpc-iac/terraform

# Initialize
terraform init -input=false

# Plan and capture exit code
terraform plan -detailed-exitcode -out=drift-plan 2>&1 | tee drift-report.txt
EXIT_CODE=$?

case $EXIT_CODE in
  0)
    echo "No drift detected"
    ;;
  1)
    echo "Error running plan"
    exit 1
    ;;
  2)
    echo "DRIFT DETECTED"
    # Parse and alert
    python3 /opt/scripts/parse_drift.py drift-report.txt | \
      /opt/scripts/send_alert.sh
    ;;
esac
```

### 5.3 Ansible Configuration Drift Check

```yaml
# drift-check.yml
- name: Check configuration drift
  hosts: all
  gather_facts: yes
  check_mode: yes

  tasks:
    - name: Check Slurm configuration
      ansible.builtin.template:
        src: slurm.conf.j2
        dest: /etc/slurm/slurm.conf
      register: slurm_drift

    - name: Check SSSD configuration
      ansible.builtin.template:
        src: sssd.conf.j2
        dest: /etc/sssd/sssd.conf
      register: sssd_drift

    - name: Report drift
      ansible.builtin.set_fact:
        drift_detected: "{{ slurm_drift.changed or sssd_drift.changed }}"

- name: Aggregate drift report
  hosts: localhost
  tasks:
    - name: Collect drift status
      ansible.builtin.set_fact:
        drift_hosts: "{{ groups['all'] | map('extract', hostvars, 'drift_detected') | select('equalto', true) | list }}"

    - name: Alert on drift
      ansible.builtin.debug:
        msg: "Drift detected on: {{ drift_hosts }}"
      when: drift_hosts | length > 0
```

### 5.4 Combined Drift Remediation Workflow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Drift Remediation Workflow                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Drift Detected                                                     │
│       │                                                             │
│       ▼                                                             │
│  ┌─────────────────────┐                                           │
│  │ Classify drift type │                                           │
│  └──────────┬──────────┘                                           │
│             │                                                       │
│    ┌────────┴────────┐                                             │
│    ▼                 ▼                                              │
│  Infrastructure   Configuration                                     │
│    drift             drift                                          │
│    │                 │                                              │
│    ▼                 ▼                                              │
│  ┌─────────┐     ┌─────────────┐                                   │
│  │ Review  │     │ Auto-fix    │                                   │
│  │ Terraform│     │ with Ansible│                                   │
│  │ plan    │     │ (if policy  │                                   │
│  └────┬────┘     │ allows)     │                                   │
│       │          └──────┬──────┘                                   │
│       ▼                 │                                           │
│  ┌─────────┐           │                                           │
│  │ Manual  │           │                                           │
│  │ approval│           │                                           │
│  └────┬────┘           │                                           │
│       │                │                                           │
│       ▼                ▼                                           │
│  ┌─────────────────────────────┐                                   │
│  │      Apply remediation      │                                   │
│  └─────────────────────────────┘                                   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 6. Failure Recovery

### 6.1 Failure Scenarios and Recovery

| Stage | Failure Scenario | Recovery Action |
|-------|------------------|-----------------|
| **Terraform plan** | State lock stuck | `terraform force-unlock` |
| **Terraform apply** | Partial provision | Fix and re-apply (idempotent) |
| **Terraform apply** | Provider error | Check API limits, retry |
| **Inventory handoff** | Missing outputs | Re-run `terraform output` |
| **SSH connectivity** | Timeout | Check security groups, retry |
| **Ansible common** | Package failure | Fix repos, retry |
| **Ansible scheduler** | Service failure | Check logs, fix config, retry |
| **Validation** | Test failure | Diagnose, fix, re-run validation |

### 6.2 Terraform Failure Recovery

```bash
#!/bin/bash
# recover-terraform.sh

# Check for stuck lock
LOCK_INFO=$(terraform force-unlock -force 2>&1 || true)
if [[ $LOCK_INFO == *"Lock"* ]]; then
    echo "Lock released, retrying..."
fi

# Refresh state to sync with reality
terraform refresh

# Re-plan
terraform plan -out=recovery-plan

# Show what needs to be fixed
terraform show recovery-plan
```

### 6.3 Ansible Failure Recovery

```yaml
# recovery-playbook.yml
- name: Recover failed configuration
  hosts: "{{ failed_hosts | default('all') }}"
  serial: 1
  max_fail_percentage: 0

  tasks:
    - name: Check current state
      ansible.builtin.setup:
        gather_subset:
          - min

    - name: Restore from backup if needed
      ansible.builtin.copy:
        src: "/var/backup/ansible/{{ backup_date }}/{{ item }}"
        dest: "{{ item }}"
        remote_src: yes
      loop:
        - /etc/slurm/slurm.conf
        - /etc/sssd/sssd.conf
      when: restore_from_backup | default(false)

    - name: Re-apply configuration
      ansible.builtin.include_role:
        name: "{{ item }}"
      loop:
        - common
        - scheduler
        - identity
```

### 6.4 Partial Failure Handling

```bash
# After Ansible failure, use retry file
ansible-playbook site.yml --limit @site.retry

# Or specify failed hosts manually
ansible-playbook site.yml --limit 'node001,node002,node003'

# Re-run from specific task
ansible-playbook site.yml --start-at-task="Configure Slurm"
```

---

## 7. Pain Points by Stage

### 7.1 Pain Point Summary

| Stage | Pain Point | Severity | Mitigation |
|-------|------------|----------|------------|
| **Planning** | Long plan times (1000+ resources) | Medium | Parallelize, split state |
| **Planning** | Complex module dependencies | Medium | Pin versions, test upgrades |
| **Provisioning** | Partial failures | High | Retry logic, idempotent design |
| **Provisioning** | Long provision times | Medium | Parallel creation, pre-baked AMIs |
| **Handoff** | Inventory sync timing | High | Wait loops, health checks |
| **Handoff** | SSH key distribution | Medium | Cloud-init, user-data |
| **Configuration** | Slow Ansible execution | High | Mitogen, pipelining, parallelism |
| **Configuration** | Reboot coordination | Medium | Serial execution, drain first |
| **Validation** | Incomplete test coverage | Medium | Comprehensive test suite |
| **Drift** | Undetected manual changes | High | Regular drift scans, alerts |
| **Drift** | Tool conflicts | High | Clear ownership boundaries |

### 7.2 Detailed Pain Points

**1. Inventory Timing Issues**
```
Problem: Terraform completes but instances not ready for SSH
         Ansible fails to connect

Solution:
- Add wait loop after Terraform
- Use cloud-init completion signal
- Health check before Ansible
```

```python
# wait_for_ssh.py
import json
import socket
import time
import sys

def wait_for_ssh(hosts, timeout=300, interval=10):
    start = time.time()
    pending = set(hosts)

    while pending and (time.time() - start) < timeout:
        for host in list(pending):
            try:
                sock = socket.create_connection((host, 22), timeout=5)
                sock.close()
                pending.remove(host)
                print(f"✓ {host} ready")
            except (socket.timeout, ConnectionRefusedError):
                pass

        if pending:
            time.sleep(interval)

    if pending:
        print(f"✗ Timeout waiting for: {pending}")
        sys.exit(1)

if __name__ == "__main__":
    with open(sys.argv[1]) as f:
        inventory = json.load(f)
    hosts = [h["ansible_host"] for h in inventory["_meta"]["hostvars"].values()]
    wait_for_ssh(hosts)
```

**2. State Conflicts**
```
Problem: Terraform and Ansible both manage the same resource
         Changes conflict and overwrite each other

Solution:
- Define clear ownership boundaries
- Use Terraform for infrastructure only
- Use Ansible for configuration only
- Document what each tool manages
```

**3. Scale Performance**
```
Problem: Ansible takes 6+ hours for 1000 nodes
         Terraform plan takes 30+ minutes

Solutions:
- Ansible: Use Mitogen, increase forks, free strategy
- Terraform: Split state files, parallelize modules
- Both: Use staged/rolling deployments
```

---

## 8. Tool Boundary Summary

### 8.1 Ownership Matrix

| Resource/Configuration | Terraform | Ansible | Notes |
|------------------------|-----------|---------|-------|
| **VPC/Networking** | ✅ Owner | ❌ | Infrastructure |
| **Security Groups** | ✅ Owner | ❌ | Infrastructure |
| **EC2 Instances** | ✅ Owner | ❌ | Lifecycle only |
| **EBS Volumes** | ✅ Owner | ❌ | Attachment |
| **FSx/EFS** | ✅ Owner | ❌ | Creation |
| **IAM Roles** | ✅ Owner | ❌ | Infrastructure |
| **OS Configuration** | ❌ | ✅ Owner | Packages, services |
| **Slurm Config** | ❌ | ✅ Owner | Full management |
| **User Accounts** | ❌ | ✅ Owner | LDAP/local |
| **Software Stack** | ❌ | ✅ Owner | CUDA, MPI, modules |
| **Mount Points** | ❌ | ✅ Owner | fstab, mount |
| **Monitoring Agents** | ❌ | ✅ Owner | Installation, config |

### 8.2 Handoff Contract

```yaml
# terraform-ansible-contract.yml
# Defines what Terraform provides to Ansible

terraform_outputs:
  required:
    - compute_nodes:        # List of {name, ip, type, az}
    - head_nodes:           # List of {name, ip}
    - storage_endpoints:    # List of {mount_point, endpoint}
    - cluster_name:         # String
    - vpc_cidr:             # String

  optional:
    - gpu_nodes:            # List of {name, ip, gpu_count}
    - license_server:       # String (IP)
    - ldap_server:          # String (IP)

ansible_expectations:
  - SSH access via private key
  - Passwordless sudo for ansible_user
  - DNS resolution for hostnames
  - Network connectivity between nodes
```

### 8.3 Rustible Improvement Opportunities

| Gap | Current State | Rustible Opportunity |
|-----|---------------|---------------------|
| **Handoff latency** | Manual wait loops | Integrated provisioning checks |
| **Inventory sync** | Script-based | Native Terraform state reading |
| **Drift detection** | Separate tools | Unified drift reporting |
| **Failure recovery** | Manual intervention | Automatic rollback |
| **Scale performance** | Limited parallelism | Native async execution |
| **State tracking** | Two separate states | Unified state awareness |

---

## References

- [Terraform + Ansible Integration (HashiCorp)](https://www.hashicorp.com/en/blog/terraform-ansible-unifying-infrastructure-provisioning-configuration-management)
- [Ansible AAP Provider Integration](https://developer.hashicorp.com/validated-patterns/terraform/terraform-integrate-ansible-automation-platform)
- [cloud.terraform Collection](https://galaxy.ansible.com/cloud/terraform)
- [Terraform Drift Detection](https://developer.hashicorp.com/terraform/cloud-docs/workspaces/health#drift-detection)
- [Spacelift Drift Detection](https://docs.spacelift.io/concepts/stack/drift-detection)
- [terraform-inventory](https://github.com/adammck/terraform-inventory)
