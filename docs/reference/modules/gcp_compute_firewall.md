---
summary: Reference for the gcp_compute_firewall module that manages GCP firewall rules.
read_when: You need to create or delete GCP firewall rules from playbooks.
---

# gcp_compute_firewall - Manage GCP Firewall Rules

## Synopsis

The `gcp_compute_firewall` module creates, updates, and deletes Google Cloud Compute Engine
firewall rules. Supports both ingress and egress rules with allow/deny specifications.
Feature-gated and requires building with `--features gcp`. This module is experimental.

## Classification

**Cloud** - Uses the Google Cloud SDK to manage networking resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Firewall rule name. |
| `state` | no | present | string | Desired state: present, absent. |
| `network` | no | default | string | VPC network name to apply the rule to. |
| `direction` | no | INGRESS | string | Traffic direction: INGRESS, EGRESS. |
| `priority` | no | 1000 | int | Rule priority (0-65535, lower is higher). |
| `allowed` | no | - | list | Allowed traffic specs (see format below). |
| `denied` | no | - | list | Denied traffic specs (see format below). |
| `source_ranges` | no | - | list | Source IP CIDR ranges (INGRESS only). |
| `destination_ranges` | no | - | list | Destination IP CIDR ranges (EGRESS only). |
| `source_tags` | no | - | list | Source network tags (INGRESS only). |
| `target_tags` | no | - | list | Target network tags. |
| `source_service_accounts` | no | - | list | Source service account emails. |
| `target_service_accounts` | no | - | list | Target service account emails. |
| `disabled` | no | false | bool | Whether the rule is disabled. |
| `description` | no | - | string | Rule description. |
| `project` | no | from env | string | GCP project ID. |

### Allowed/Denied format

Each entry in `allowed` or `denied` is a map:

| Key | Required | Type | Description |
|-----|----------|------|-------------|
| `IPProtocol` | yes | string | Protocol: tcp, udp, icmp, esp, ah, sctp, all. |
| `ports` | no | list | Port ranges (e.g., ["80", "443", "8000-9000"]). |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `firewall` | map | Firewall rule information object. |
| `firewall.id` | string | Firewall rule numeric ID. |
| `firewall.self_link` | string | Firewall self-link URL. |

## Examples

### Allow HTTP and HTTPS traffic

```yaml
- name: Create web firewall rule
  gcp_compute_firewall:
    name: allow-http-https
    network: my-vpc
    allowed:
      - IPProtocol: tcp
        ports:
          - "80"
          - "443"
    source_ranges:
      - 0.0.0.0/0
    target_tags:
      - http-server
    state: present
```

### Delete a firewall rule

```yaml
- name: Remove old rule
  gcp_compute_firewall:
    name: deprecated-rule
    state: absent
```

## Notes

- Requires GCP credentials via GOOGLE_APPLICATION_CREDENTIALS or Application Default Credentials.
- Build with `--features gcp` to enable this module. This feature is experimental.
- Either `allowed` or `denied` should be specified, not both.
- Firewall rules apply to the entire VPC network specified.
