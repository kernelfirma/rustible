---
summary: Reference for the k8s_secret module that manages Kubernetes Secrets.
read_when: You need to create, update, or delete Kubernetes Secrets from playbooks.
---

# k8s_secret - Manage Kubernetes Secrets

## Synopsis

The `k8s_secret` module manages Kubernetes Secret resources for storing sensitive
information such as passwords, tokens, and keys. It is feature-gated and requires
building with `--features kubernetes`.

## Classification

**RemoteCommand** - Kubernetes API operations via kube-rs.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Secret name. |
| `namespace` | no | default | string | Kubernetes namespace. |
| `state` | no | present | string | Desired state: present, absent. |
| `type` | no | Opaque | string | Secret type (Opaque, kubernetes.io/tls, kubernetes.io/dockerconfigjson, etc.). |
| `data` | no | - | map | Key-value pairs with base64-encoded values. |
| `string_data` | no | - | map | Key-value pairs as plain strings (encoded by Kubernetes). |
| `labels` | no | - | map | Labels for the Secret. |
| `annotations` | no | - | map | Annotations for the Secret. |
| `immutable` | no | - | bool | If true, the Secret cannot be updated after creation. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `type` | string | Secret type. |
| `keys_count` | integer | Total number of data keys in the Secret. |

## Examples

### Create a Secret with string data

```yaml
- name: Create database credentials
  k8s_secret:
    name: db-credentials
    namespace: default
    type: Opaque
    string_data:
      username: admin
      password: "{{ db_password }}"
    labels:
      app: myapp
    state: present
```

### Create a TLS Secret

```yaml
- name: Create TLS secret
  k8s_secret:
    name: tls-cert
    namespace: default
    type: kubernetes.io/tls
    data:
      tls.crt: "{{ tls_cert_b64 }}"
      tls.key: "{{ tls_key_b64 }}"
    state: present
```

### Delete a Secret

```yaml
- name: Remove credentials
  k8s_secret:
    name: db-credentials
    namespace: default
    state: absent
```

## Notes

- Build with `--features kubernetes` to enable this module.
- Uses kube-rs to communicate with the Kubernetes API server.
- Use `string_data` for plain text values; use `data` for pre-encoded base64 values.
- Immutable Secrets cannot be updated; attempting to do so will return an error.
- Supported secret types include Opaque, kubernetes.io/tls, kubernetes.io/dockerconfigjson, kubernetes.io/basic-auth, kubernetes.io/ssh-auth, and others.
