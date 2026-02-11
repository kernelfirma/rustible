---
summary: Reference for the k8s_namespace module that manages Kubernetes namespaces.
read_when: You need to create or delete Kubernetes namespaces from playbooks.
---

# k8s_namespace - Manage Kubernetes Namespaces

## Synopsis

The `k8s_namespace` module manages Kubernetes Namespace resources for isolating
groups of resources within a cluster. It is feature-gated and requires building
with `--features kubernetes`.

## Classification

**RemoteCommand** - Kubernetes API operations via kube-rs.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Namespace name. Must be lowercase alphanumeric and hyphens, max 63 characters. |
| `state` | no | present | string | Desired state: present, absent. |
| `labels` | no | - | map | Labels to apply to the Namespace. |
| `annotations` | no | - | map | Annotations to apply to the Namespace. |
| `wait` | no | false | bool | Wait for the namespace to become active (or be deleted). |
| `wait_timeout` | no | 60 | integer | Timeout in seconds for wait. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `status` | string | Namespace phase (e.g., Active, Terminating). |

## Examples

### Create a namespace

```yaml
- name: Create production namespace
  k8s_namespace:
    name: production
    labels:
      environment: production
      team: platform
    annotations:
      description: "Production workloads"
    state: present
```

### Delete a namespace

```yaml
- name: Remove staging namespace
  k8s_namespace:
    name: staging
    state: absent
    wait: true
    wait_timeout: 120
```

## Notes

- Build with `--features kubernetes` to enable this module.
- Uses kube-rs to communicate with the Kubernetes API server.
- Namespace names must follow RFC 1123 label rules (lowercase, alphanumeric, hyphens only).
- Terminating namespaces cannot be updated.
