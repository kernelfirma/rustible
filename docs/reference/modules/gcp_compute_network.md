---
summary: Reference for the gcp_compute_network module that manages GCP VPC networks.
read_when: You need to create or delete GCP VPC networks from playbooks.
---

# gcp_compute_network - Manage GCP VPC Networks

## Synopsis

The `gcp_compute_network` module creates and deletes Google Cloud VPC networks. Supports
auto-mode and custom-mode subnet creation, and routing mode configuration. Feature-gated
and requires building with `--features gcp`. This module is experimental.

## Classification

**Cloud** - Uses the Google Cloud SDK to manage networking resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | VPC network name. |
| `state` | no | present | string | Desired state: present, absent. |
| `auto_create_subnetworks` | no | true | bool | Auto-create subnets in each region. |
| `routing_mode` | no | REGIONAL | string | Routing mode: REGIONAL, GLOBAL. |
| `mtu` | no | 1460 | int | Maximum transmission unit (1460-8896). |
| `description` | no | - | string | Network description. |
| `project` | no | from env | string | GCP project ID. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `network` | map | VPC network information object. |
| `network.id` | string | Network numeric ID. |
| `network.name` | string | Network name. |
| `network.auto_create_subnetworks` | bool | Whether subnets are auto-created. |
| `network.routing_mode` | string | Routing mode. |
| `network.self_link` | string | Network self-link URL. |

## Examples

### Create a custom-mode VPC network

```yaml
- name: Create custom VPC
  gcp_compute_network:
    name: my-custom-vpc
    auto_create_subnetworks: false
    routing_mode: GLOBAL
    description: Custom VPC for production
    state: present
```

### Create an auto-mode VPC network

```yaml
- name: Create auto-mode VPC
  gcp_compute_network:
    name: my-auto-vpc
    auto_create_subnetworks: true
    state: present
```

### Delete a VPC network

```yaml
- name: Remove old VPC
  gcp_compute_network:
    name: deprecated-vpc
    state: absent
```

## Notes

- Requires GCP credentials via GOOGLE_APPLICATION_CREDENTIALS or Application Default Credentials.
- Build with `--features gcp` to enable this module. This feature is experimental.
- Set `auto_create_subnetworks: false` for custom-mode networks where you manage subnets manually.
- A network cannot be deleted while it has active firewall rules or instances.
