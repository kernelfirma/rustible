---
summary: Reference for the k8s_service module that manages Kubernetes services.
read_when: You need to create, update, or delete Kubernetes services from playbooks.
---

# k8s_service - Manage Kubernetes Services

## Synopsis

The `k8s_service` module manages Kubernetes Service resources including ClusterIP,
NodePort, LoadBalancer, and ExternalName types. It is feature-gated and requires
building with `--features kubernetes`.

## Classification

**RemoteCommand** - Kubernetes API operations via kube-rs.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Service name. |
| `namespace` | no | default | string | Kubernetes namespace. |
| `state` | no | present | string | Desired state: present, absent. |
| `type` | no | ClusterIP | string | Service type: ClusterIP, NodePort, LoadBalancer, ExternalName. |
| `selector` | no | - | map | Label selector for target pods. |
| `ports` | no | - | list | Port mappings, each with port, target_port, node_port, name, protocol. |
| `cluster_ip` | no | - | string | Cluster IP address. Use "None" for headless services. |
| `external_ips` | no | - | list | External IP addresses. |
| `external_name` | no | - | string | External DNS name (for ExternalName type). |
| `load_balancer_ip` | no | - | string | Requested load balancer IP. |
| `session_affinity` | no | - | string | Session affinity: None, ClientIP. |
| `labels` | no | - | map | Labels for the Service. |
| `annotations` | no | - | map | Annotations for the Service. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `cluster_ip` | string | Assigned cluster IP address. |

## Examples

### Create a ClusterIP service

```yaml
- name: Create nginx service
  k8s_service:
    name: nginx-service
    namespace: default
    type: ClusterIP
    selector:
      app: nginx
    ports:
      - port: 80
        target_port: 80
        protocol: TCP
    state: present
```

### Create a NodePort service

```yaml
- name: Expose service on node port
  k8s_service:
    name: web-nodeport
    namespace: default
    type: NodePort
    selector:
      app: web
    ports:
      - port: 80
        target_port: 8080
        node_port: 30080
    state: present
```

## Notes

- Build with `--features kubernetes` to enable this module.
- Uses kube-rs to communicate with the Kubernetes API server.
- The existing `clusterIP` is preserved during updates unless explicitly set.
- Selectors are not applied for ExternalName services.
