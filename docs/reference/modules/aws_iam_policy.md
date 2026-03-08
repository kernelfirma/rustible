---
summary: Reference for the aws_iam_policy module that manages AWS IAM policies.
read_when: You need to create or delete IAM policies from playbooks.
---

# aws_iam_policy - Manage AWS IAM Policies

## Synopsis

The `aws_iam_policy` module creates, updates, and deletes AWS IAM managed policies.
It supports inline policy documents, path configuration, and tagging. Feature-gated
and requires building with `--features aws`.

## Classification

**Cloud** - Uses the AWS SDK to manage IAM resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | IAM policy name. |
| `state` | no | present | string | Desired state: present, absent. |
| `policy_document` | no* | - | string | JSON policy document. *Required when creating. |
| `description` | no | - | string | Policy description. |
| `path` | no | / | string | IAM path for the policy. |
| `tags` | no | - | map | Key-value resource tags. |
| `region` | no | - | string | AWS region override. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `policy_name` | string | The IAM policy name. |
| `arn` | string | The IAM policy ARN. |
| `policy_id` | string | The unique policy ID. |

## Examples

### Create a custom IAM policy

```yaml
- name: Create S3 access policy
  aws_iam_policy:
    name: custom-s3-policy
    description: Allow read/write to specific S3 bucket
    policy_document: |
      {
        "Version": "2012-10-17",
        "Statement": [
          {
            "Effect": "Allow",
            "Action": ["s3:GetObject", "s3:PutObject"],
            "Resource": "arn:aws:s3:::my-bucket/*"
          }
        ]
      }
    tags:
      Team: platform
    state: present
```

### Delete an IAM policy

```yaml
- name: Remove old policy
  aws_iam_policy:
    name: deprecated-policy
    state: absent
```

## Notes

- Requires AWS credentials in the environment or config files.
- Build with `--features aws` to enable this module.
- Updating a policy creates a new version; old versions may need manual cleanup.
- Deleting a policy removes all non-default versions first.
