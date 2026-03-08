---
summary: Reference for the aws_security_group_rule module that manages standalone AWS EC2 security group rules.
read_when: You need to add or remove individual ingress or egress rules from playbooks.
---

# aws_security_group_rule - Manage AWS Security Group Rules

## Synopsis

The `aws_security_group_rule` module manages individual AWS EC2 security group
rules. It supports standalone ingress and egress rules, IPv4 and IPv6 CIDRs,
source security groups, check mode, and description updates by recreating the
matching rule when AWS requires it.

## Classification

**Cloud** - Uses the AWS SDK to manage EC2 security group rules.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `group_id` | yes* | - | string | Target security group id. `security_group_id` is accepted as an alias. |
| `type` | yes | - | string | Rule direction: `ingress` or `egress`. |
| `state` | no | present | string | Desired state: `present`, `absent`. |
| `protocol` | no | -1 | string | Protocol name. Supported values: `-1`, `tcp`, `udp`, `icmp`, `icmpv6`. |
| `from_port` | no | -1 | integer | Start port or ICMP type. Must match protocol rules. |
| `to_port` | no | -1 | integer | End port or ICMP code. Must match protocol rules. |
| `cidr_blocks` | no | [] | list | IPv4 CIDR blocks for the rule. |
| `ipv6_cidr_blocks` | no | [] | list | IPv6 CIDR blocks for the rule. |
| `source_security_group_id` | no | - | string | Source or destination security group reference. `source_group_id` is accepted as an alias. |
| `self_referencing` | no | false | boolean | Add the target security group itself as a rule source. |
| `description` | no | - | string | Rule description applied to each source entry. |
| `region` | no | - | string | AWS region override. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `action` | string | Action taken or planned: create, update, or delete. Present only when a change is required. |
| `rule` | object | Resolved rule metadata including `rule_id`, group id, rule direction, ports, sources, and description. |

## Examples

### Allow HTTPS from an internal CIDR

```yaml
- name: Allow HTTPS from the app subnet
  aws_security_group_rule:
    group_id: sg-0123456789abcdef0
    type: ingress
    protocol: tcp
    from_port: 443
    to_port: 443
    cidr_blocks:
      - 10.20.30.0/24
    description: Internal TLS
    state: present
```

### Allow SSH from another security group

```yaml
- name: Allow SSH from bastion hosts
  aws_security_group_rule:
    group_id: sg-0123456789abcdef0
    type: ingress
    protocol: tcp
    from_port: 22
    to_port: 22
    source_security_group_id: sg-0feedface1234567
    description: SSH from bastion
```

### Remove an old egress rule

```yaml
- name: Remove legacy outbound rule
  aws_security_group_rule:
    security_group_id: sg-0123456789abcdef0
    type: egress
    protocol: tcp
    from_port: 8080
    to_port: 8080
    cidr_blocks:
      - 0.0.0.0/0
    state: absent
```

## Notes

- Requires AWS credentials in the environment or config files.
- Build with `--features aws` to enable this module.
- At least one of `cidr_blocks`, `ipv6_cidr_blocks`, `source_security_group_id`, or `self_referencing` must be supplied.
- AWS does not update rule descriptions in place, so description-only changes are applied as revoke plus authorize.
