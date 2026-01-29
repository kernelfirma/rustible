---
summary: Reference for the aws_s3 module that manages AWS S3 objects.
read_when: You need to upload or download objects in S3 from playbooks.
---

# aws_s3 - Manage AWS S3 Objects

## Synopsis

The `aws_s3` module uploads and downloads objects in S3 buckets. It is feature-gated
and requires building with `--features aws`.

## Classification

**Cloud** - Uses the AWS SDK to manage cloud resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `bucket` | yes | - | string | S3 bucket name. |
| `object` | yes | - | string | Object key in the bucket. |
| `src` | no | - | string | Local path to upload. |
| `dest` | no | - | string | Local path to download to. |
| `mode` | no | put | string | Operation mode: put or get. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `etag` | string | ETag of the uploaded object. |
| `mode` | string | Operation performed. |

## Examples

### Upload a file to S3

```yaml
- name: Upload artifact
  aws_s3:
    bucket: my-bucket
    object: releases/app.tar.gz
    src: /tmp/app.tar.gz
    mode: put
```

### Download a file from S3

```yaml
- name: Download artifact
  aws_s3:
    bucket: my-bucket
    object: releases/app.tar.gz
    dest: /tmp/app.tar.gz
    mode: get
```

## Notes

- Requires AWS credentials in the environment or config files.
- Build with `--features aws` to enable this module.
