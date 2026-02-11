---
summary: Reference for the k8s_deployment module that manages Kubernetes deployments.
read_when: You need to create, update, or delete Kubernetes deployments from playbooks.
---

# k8s_deployment - Manage Kubernetes Deployments

## Synopsis

The `k8s_deployment` module manages Kubernetes Deployment resources with full
control over pod specifications, replicas, and update strategies. It is
feature-gated and requires building with `--features kubernetes`.

## Classification

**RemoteCommand** - Kubernetes API operations via kube-rs.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Deployment name. |
| `namespace` | no | default | string | Kubernetes namespace. |
| `state` | no | present | string | Desired state: present, absent. |
| `replicas` | no | 1 | integer | Number of desired pod replicas. |
| `image` | yes* | - | string | Container image to deploy. Required for state=present. |
| `container_name` | no | (deployment name) | string | Name of the container. |
| `container_port` | no | - | integer | Container port to expose. |
| `labels` | no | - | map | Labels for the Deployment and pods. An `app` label is auto-added if absent. |
| `annotations` | no | - | map | Annotations for the Deployment. |
| `env` | no | - | map | Environment variables as key-value pairs. |
| `strategy` | no | RollingUpdate | string | Update strategy: RollingUpdate, Recreate. |
| `max_surge` | no | - | string | Maximum pods above desired count during rolling update. |
| `max_unavailable` | no | - | string | Maximum unavailable pods during rolling update. |
| `wait` | no | false | bool | Wait for deployment to be ready. |
| `wait_timeout` | no | 300 | integer | Timeout in seconds for wait. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `replicas` | integer | Configured replica count. |

## Examples

### Deploy an application

```yaml
- name: Deploy nginx
  k8s_deployment:
    name: nginx-deployment
    namespace: default
    replicas: 3
    image: nginx:1.21
    container_port: 80
    labels:
      app: nginx
      tier: frontend
    strategy: RollingUpdate
    wait: true
    state: present
```

### Remove a deployment

```yaml
- name: Remove deployment
  k8s_deployment:
    name: nginx-deployment
    namespace: default
    state: absent
```

## Notes

- Build with `--features kubernetes` to enable this module.
- Uses kube-rs to communicate with the Kubernetes API server.
- An `app` label is automatically added using the deployment name if not provided.
- The `rolling_update` and `rolling` aliases are accepted for the `RollingUpdate` strategy.
