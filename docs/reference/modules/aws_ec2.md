---
summary: Reference for the aws_ec2 module that manages AWS EC2 instances.
read_when: You need to create or terminate EC2 instances from playbooks.
---

# aws_ec2 - Manage AWS EC2 Instances

## Synopsis

The `aws_ec2` module provisions and manages EC2 instances. It is feature-gated and
requires building with `--features aws`.

## Classification

**Cloud** - Uses the AWS SDK to manage cloud resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `instance_type` | yes | - | string | EC2 instance type (e.g., t3.micro). |
| `image_id` | yes | - | string | AMI ID to launch. |
| `state` | no | running | string | Desired state: running, stopped, terminated. |
| `region` | no | - | string | AWS region (e.g., us-east-1). |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `instance_id` | string | EC2 instance ID. |
| `state` | string | Final instance state. |

## Examples

### Launch an EC2 instance

```yaml
- name: Launch instance
  aws_ec2:
    instance_type: t3.micro
    image_id: ami-12345678
    region: us-east-1
    state: running
```

### Terminate an instance

```yaml
- name: Terminate instance
  aws_ec2:
    instance_type: t3.micro
    image_id: ami-12345678
    region: us-east-1
    state: terminated
```

## Notes

- Requires AWS credentials in the environment or config files.
- Build with `--features aws` to enable this module.
