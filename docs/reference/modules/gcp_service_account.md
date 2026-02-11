---
summary: Reference for the gcp_service_account module that manages GCP service accounts.
read_when: You need to create or delete GCP service accounts from playbooks.
---

# gcp_service_account - Manage GCP Service Accounts

## Synopsis

The `gcp_service_account` module creates and deletes Google Cloud service accounts.
Optionally generates service account keys. Feature-gated and requires building with
`--features gcp`. This module is experimental.

## Classification

**Cloud** - Uses the Google Cloud SDK to manage IAM resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Service account ID (the part before @). |
| `state` | no | present | string | Desired state: present, absent. |
| `display_name` | no | - | string | Human-readable display name. |
| `description` | no | - | string | Service account description. |
| `create_key` | no | false | bool | Generate a JSON key upon creation. |
| `key_algorithm` | no | KEY_ALG_RSA_2048 | string | Key algorithm when creating keys. |
| `project` | no | from env | string | GCP project ID. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `service_account` | map | Service account information object. |
| `service_account.email` | string | Full service account email. |
| `service_account.unique_id` | string | Unique numeric ID. |
| `service_account.name` | string | Full resource name. |

## Examples

### Create a service account

```yaml
- name: Create application service account
  gcp_service_account:
    name: my-app-sa
    display_name: My Application Service Account
    description: Used by the web application
    project: my-gcp-project
    state: present
```

### Create a service account with a key

```yaml
- name: Create SA with key
  gcp_service_account:
    name: ci-deployer
    display_name: CI/CD Deployer
    create_key: true
    project: my-gcp-project
    state: present
```

### Delete a service account

```yaml
- name: Remove old service account
  gcp_service_account:
    name: deprecated-sa
    project: my-gcp-project
    state: absent
```

## Notes

- Requires GCP credentials via GOOGLE_APPLICATION_CREDENTIALS or Application Default Credentials.
- Build with `--features gcp` to enable this module. This feature is experimental.
- The email will be `{name}@{project}.iam.gserviceaccount.com`.
- Service account keys should be handled carefully and stored securely.
- Project is resolved from parameter, then GOOGLE_CLOUD_PROJECT or GCLOUD_PROJECT env vars.
