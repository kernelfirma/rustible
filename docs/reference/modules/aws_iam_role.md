---
summary: Reference for the aws_iam_role module that manages AWS IAM roles.
read_when: You need to create or delete IAM roles from playbooks.
---

# aws_iam_role - Manage AWS IAM Roles

## Synopsis

The `aws_iam_role` module creates, updates, and deletes AWS IAM roles. It supports
assume-role policy documents, managed policy attachments, and tagging. Feature-gated
and requires building with `--features aws`.

## Classification

**Cloud** - Uses the AWS SDK to manage IAM resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | IAM role name. |
| `state` | no | present | string | Desired state: present, absent. |
| `assume_role_policy_document` | no* | - | string | JSON trust policy. *Required when creating. |
| `description` | no | - | string | Role description. |
| `path` | no | / | string | IAM path for the role. |
| `managed_policy_arns` | no | - | list | List of managed policy ARNs to attach. |
| `tags` | no | - | map | Key-value resource tags. |
| `region` | no | - | string | AWS region override. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `role_name` | string | The IAM role name. |
| `arn` | string | The IAM role ARN. |
| `role_id` | string | The unique role ID. |

## Examples

### Create an IAM role for EC2

```yaml
- name: Create EC2 instance role
  aws_iam_role:
    name: ec2-web-role
    assume_role_policy_document: |
      {
        "Version": "2012-10-17",
        "Statement": [
          {
            "Effect": "Allow",
            "Principal": { "Service": "ec2.amazonaws.com" },
            "Action": "sts:AssumeRole"
          }
        ]
      }
    managed_policy_arns:
      - arn:aws:iam::aws:policy/AmazonS3ReadOnlyAccess
    description: Role for web server EC2 instances
    tags:
      Environment: production
    state: present
```

### Delete an IAM role

```yaml
- name: Remove old role
  aws_iam_role:
    name: deprecated-role
    state: absent
```

## Notes

- Requires AWS credentials in the environment or config files.
- Build with `--features aws` to enable this module.
- This module is planned and may have limited implementation.
- Managed policies are attached after role creation.
- Deleting a role detaches all policies first.
