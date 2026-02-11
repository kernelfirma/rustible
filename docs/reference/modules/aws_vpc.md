---
summary: Reference for the aws_ec2_vpc module that manages AWS VPCs.
read_when: You need to create, configure, or delete AWS VPCs from playbooks.
---

# aws_ec2_vpc - Manage AWS VPCs

## Synopsis

The `aws_ec2_vpc` module creates, updates, and deletes Amazon Virtual Private Clouds. It
supports DNS settings, instance tenancy configuration, and tagging. Feature-gated and
requires building with `--features aws`.

## Classification

**Cloud** - Uses the AWS SDK to manage cloud networking resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | VPC name (set as the Name tag). |
| `cidr_block` | yes* | - | string | IPv4 CIDR block (e.g., 10.0.0.0/16). *Required when creating. |
| `state` | no | present | string | Desired state: present, absent. |
| `region` | no | - | string | AWS region override. |
| `enable_dns_support` | no | true | bool | Enable DNS resolution in the VPC. |
| `enable_dns_hostnames` | no | false | bool | Enable DNS hostnames for instances. |
| `tenancy` | no | default | string | Instance tenancy: default, dedicated, host. |
| `tags` | no | - | map | Key-value resource tags. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `vpc_id` | string | The ID of the VPC. |
| `cidr_block` | string | The primary CIDR block. |
| `state` | string | VPC state (e.g., available). |
| `enable_dns_support` | bool | Whether DNS resolution is enabled. |
| `enable_dns_hostnames` | bool | Whether DNS hostnames are enabled. |

## Examples

### Create a VPC

```yaml
- name: Create production VPC
  aws_ec2_vpc:
    name: prod-vpc
    cidr_block: 10.0.0.0/16
    enable_dns_support: true
    enable_dns_hostnames: true
    region: us-east-1
    tags:
      Environment: production
    state: present
```

### Delete a VPC

```yaml
- name: Remove staging VPC
  aws_ec2_vpc:
    name: staging-vpc
    region: us-east-1
    state: absent
```

## Notes

- Requires AWS credentials in the environment or config files.
- Build with `--features aws` to enable this module.
- VPC lookup is performed by the Name tag, not by VPC ID.
- Deleting a VPC will fail if it still contains dependent resources.
