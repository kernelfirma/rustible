---
summary: Reference for the aws_ec2_security_group module that manages AWS Security Groups.
read_when: You need to create security groups or manage firewall rules from playbooks.
---

# aws_ec2_security_group - Manage AWS Security Groups

## Synopsis

The `aws_ec2_security_group` module creates, updates, and deletes EC2 security groups
and their ingress/egress rules. Feature-gated and requires building with `--features aws`.

## Classification

**Cloud** - Uses the AWS SDK to manage cloud security resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Security group name. |
| `description` | no | - | string | Group description (set at creation). |
| `vpc_id` | no | - | string | VPC ID. Required for VPC security groups. |
| `state` | no | present | string | Desired state: present, absent. |
| `rules` | no | - | list | Ingress rules (see Rule format below). |
| `rules_egress` | no | - | list | Egress rules (see Rule format below). |
| `purge_rules` | no | false | bool | Remove ingress rules not in the list. |
| `purge_rules_egress` | no | false | bool | Remove egress rules not in the list. |
| `tags` | no | - | map | Key-value resource tags. |
| `region` | no | - | string | AWS region override. |

### Rule format

Each rule in `rules` or `rules_egress` is a map with these keys:

| Key | Required | Type | Description |
|-----|----------|------|-------------|
| `protocol` | yes | string | IP protocol: tcp, udp, icmp, or -1 (all). |
| `from_port` | yes | int | Start of port range. |
| `to_port` | yes | int | End of port range. |
| `cidr_ip` | no | string | IPv4 CIDR (e.g., 0.0.0.0/0). |
| `cidr_ipv6` | no | string | IPv6 CIDR. |
| `source_security_group_id` | no | string | Source security group ID. |
| `description` | no | string | Rule description. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `group_id` | string | The security group ID. |
| `group_name` | string | The security group name. |
| `vpc_id` | string | The VPC ID the group belongs to. |

## Examples

### Create a security group with rules

```yaml
- name: Create web security group
  aws_ec2_security_group:
    name: web-sg
    description: Allow HTTP and HTTPS
    vpc_id: vpc-12345678
    rules:
      - protocol: tcp
        from_port: 80
        to_port: 80
        cidr_ip: 0.0.0.0/0
      - protocol: tcp
        from_port: 443
        to_port: 443
        cidr_ip: 0.0.0.0/0
    rules_egress:
      - protocol: "-1"
        from_port: 0
        to_port: 0
        cidr_ip: 0.0.0.0/0
    tags:
      Environment: production
    state: present
```

### Delete a security group

```yaml
- name: Remove old security group
  aws_ec2_security_group:
    name: old-sg
    vpc_id: vpc-12345678
    region: us-east-1
    state: absent
```

## Notes

- Requires AWS credentials in the environment or config files.
- Build with `--features aws` to enable this module.
- Security groups are looked up by name (and optionally vpc_id).
- Set `purge_rules: true` to ensure only the declared rules exist.
