---
summary: Reference for the gcp_compute_instance module that manages GCP Compute Engine instances.
read_when: You need to create, start, stop, or delete GCP VM instances from playbooks.
---

# gcp_compute_instance - Manage GCP Compute Engine Instances

## Synopsis

The `gcp_compute_instance` module creates, deletes, starts, stops, and resets Google Cloud
Compute Engine instances. Feature-gated and requires building with `--features gcp`.
This module is experimental.

## Classification

**Cloud** - Uses the Google Cloud SDK to manage compute resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Instance name (unique within project/zone). |
| `zone` | yes | - | string | GCP zone (e.g., us-central1-a). |
| `project` | no | from env | string | GCP project ID. |
| `state` | no | running | string | Desired state: running, stopped, terminated, absent, reset. |
| `machine_type` | no | e2-medium | string | Machine type (e.g., e2-standard-2). |
| `image` | no | - | string | Boot disk image URL. |
| `image_family` | no | - | string | Image family (e.g., debian-11). |
| `image_project` | no | debian-cloud | string | Project containing the image family. |
| `disk_size_gb` | no | 10 | int | Boot disk size in GB. |
| `disk_type` | no | pd-standard | string | Disk type: pd-standard, pd-ssd, pd-balanced, pd-extreme. |
| `network` | no | default | string | VPC network name. |
| `subnet` | no | - | string | Subnet name. |
| `network_ip` | no | - | string | Internal IP address. |
| `external_ip` | no | - | string | External IP: auto, none, or a static address. |
| `preemptible` | no | false | bool | Use a preemptible VM. |
| `spot` | no | false | bool | Use a Spot VM. |
| `service_account` | no | - | string | Service account email. |
| `scopes` | no | devstorage.read_only | list | OAuth scopes for the service account. |
| `metadata` | no | - | map | Instance metadata key-value pairs. |
| `startup_script` | no | - | string | Startup script content. |
| `labels` | no | - | map | Instance labels as key-value pairs. |
| `tags` | no | - | list | Network tags (e.g., http-server). |
| `can_ip_forward` | no | false | bool | Enable IP forwarding. |
| `deletion_protection` | no | false | bool | Enable deletion protection. |
| `wait` | no | true | bool | Wait for operation to complete. |
| `wait_timeout` | no | 300 | int | Timeout in seconds for wait operations. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `instance` | map | Full instance information object. |
| `instance.id` | string | Instance numeric ID. |
| `instance.status` | string | Instance status (RUNNING, TERMINATED, etc.). |
| `instance.network_interfaces` | list | Network interfaces with IP addresses. |
| `instance.self_link` | string | Instance self-link URL. |

## Examples

### Create a Compute Engine instance

```yaml
- name: Launch a web server
  gcp_compute_instance:
    name: web-server-01
    zone: us-central1-a
    machine_type: e2-standard-2
    image_family: debian-11
    image_project: debian-cloud
    disk_size_gb: 20
    network: my-vpc
    subnet: my-subnet
    tags:
      - http-server
      - https-server
    labels:
      environment: production
    service_account: my-sa@my-project.iam.gserviceaccount.com
    scopes:
      - https://www.googleapis.com/auth/cloud-platform
    state: running
```

### Stop an instance

```yaml
- name: Stop the instance
  gcp_compute_instance:
    name: web-server-01
    zone: us-central1-a
    state: stopped
```

## Notes

- Requires GCP credentials via GOOGLE_APPLICATION_CREDENTIALS or Application Default Credentials.
- Build with `--features gcp` to enable this module. This feature is experimental.
- Project is resolved from parameter, then GOOGLE_CLOUD_PROJECT or GCLOUD_PROJECT env vars.
- The `terminated` and `absent` states both delete the instance.
