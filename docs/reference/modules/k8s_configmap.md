---
summary: Reference for the k8s_configmap module that manages Kubernetes ConfigMaps.
read_when: You need to create, update, or delete Kubernetes ConfigMaps from playbooks.
---

# k8s_configmap - Manage Kubernetes ConfigMaps

## Synopsis

The `k8s_configmap` module manages Kubernetes ConfigMap resources for decoupling
configuration data from container images. It is feature-gated and requires building
with `--features kubernetes`.

## Classification

**RemoteCommand** - Kubernetes API operations via kube-rs.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | ConfigMap name. |
| `namespace` | no | default | string | Kubernetes namespace. |
| `state` | no | present | string | Desired state: present, absent. |
| `data` | no | - | map | Key-value pairs for the ConfigMap data. |
| `binary_data` | no | - | map | Binary data as base64-encoded strings. |
| `labels` | no | - | map | Labels for the ConfigMap. |
| `annotations` | no | - | map | Annotations for the ConfigMap. |
| `immutable` | no | - | bool | If true, the ConfigMap cannot be updated after creation. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `keys` | list | List of data keys in the ConfigMap. |

## Examples

### Create a ConfigMap

```yaml
- name: Create application config
  k8s_configmap:
    name: app-config
    namespace: default
    data:
      app.properties: |
        database.url=jdbc:postgresql://db:5432/app
        cache.enabled=true
      log.level: INFO
    labels:
      app: myapp
    state: present
```

### Delete a ConfigMap

```yaml
- name: Remove config
  k8s_configmap:
    name: app-config
    namespace: default
    state: absent
```

## Notes

- Build with `--features kubernetes` to enable this module.
- Uses kube-rs to communicate with the Kubernetes API server.
- Immutable ConfigMaps cannot be updated; attempting to do so will return an error.
- Binary data values must be valid base64-encoded strings.
