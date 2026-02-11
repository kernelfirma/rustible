---
summary: Reference for the proxmox_lxc module that manages Proxmox LXC containers.
read_when: You need to create, clone, start, stop, or delete Proxmox LXC containers from playbooks.
---

# proxmox_lxc - Manage Proxmox LXC Containers

## Synopsis

The `proxmox_lxc` module manages the full lifecycle of Proxmox VE LXC containers via the
Proxmox REST API. Supports creation, cloning, deletion, and power state management. This
is a core module and requires no feature flags.

## Classification

**LocalLogic** - Executes API calls from the controller to the Proxmox host.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `api_url` | yes | - | string | Proxmox API URL (e.g., https://pve:8006). |
| `api_token_id` | yes | - | string | API token ID (e.g., user@pam!token). |
| `api_token_secret` | yes | - | string | API token secret. |
| `node` | yes | - | string | Proxmox node name. |
| `vmid` | yes | - | int | Container VMID. |
| `state` | no | status | string | Desired state: status, started, stopped, restarted, present, absent, cloned. |
| `hostname` | no | - | string | Container hostname (required for create). |
| `description` | no | - | string | Container description. |
| `tags` | no | - | string | Comma-separated tags. |
| `stop_method` | no | shutdown | string | How to stop: shutdown (graceful) or stop (immediate). |
| `create` | no | - | map | Extra creation parameters passed to the Proxmox API. |
| `clone` | no | - | map | Extra clone parameters passed to the Proxmox API. |
| `delete` | no | - | map | Extra deletion parameters (purge, force, etc.). |
| `clone_from` | no | - | int | Source VMID to clone from (required for state=cloned). |
| `clone_full` | no | true | bool | Full clone (true) or linked clone (false). |
| `clone_target_node` | no | - | string | Target node for clone. |
| `clone_storage` | no | - | string | Target storage for clone. |
| `auto_name` | no | false | bool | Auto-generate hostname as rustible-ct-{vmid}. |
| `strict_params` | no | true | bool | Validate params against Proxmox API allowlists. |
| `timeout` | no | 30 | int | HTTP request timeout in seconds. |
| `validate_certs` | no | true | bool | Validate TLS certificates. |
| `insecure_skip_tls_verify` | no | false | bool | Must be true to disable cert validation. |
| `ca_cert_path` | no | - | string | Path to custom CA certificate (PEM). |
| `wait` | no | false | bool | Wait for API tasks to complete. |
| `wait_timeout` | no | 300 | int | Timeout in seconds for task wait. |
| `wait_interval` | no | 2 | int | Poll interval in seconds for task wait. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `vmid` | int | Container VMID. |
| `node` | string | Proxmox node name. |
| `present` | bool | Whether the container exists. |
| `status` | string | Container power state (running, stopped, absent). |
| `upid` | string | Proxmox task UPID (for async operations). |
| `task` | map | Task completion details (when wait=true). |

## Examples

### Create a container

```yaml
- name: Create LXC container
  proxmox_lxc:
    api_url: https://pve.example:8006
    api_token_id: root@pam!automation
    api_token_secret: "{{ vault_proxmox_token }}"
    node: pve
    vmid: 110
    hostname: web-ct
    state: present
    create:
      ostemplate: local:vztmpl/debian-12-standard_12.2-1_amd64.tar.zst
      storage: local-lvm
      memory: 1024
      cores: 2
      net0: name=eth0,bridge=vmbr0,ip=dhcp
      unprivileged: true
    wait: true
```

### Start a container

```yaml
- name: Start the container
  proxmox_lxc:
    api_url: https://pve.example:8006
    api_token_id: root@pam!automation
    api_token_secret: "{{ vault_proxmox_token }}"
    node: pve
    vmid: 110
    state: started
```

### Clone a container

```yaml
- name: Clone container 110 to 120
  proxmox_lxc:
    api_url: https://pve.example:8006
    api_token_id: root@pam!automation
    api_token_secret: "{{ vault_proxmox_token }}"
    node: pve
    vmid: 120
    hostname: web-ct-clone
    clone_from: 110
    clone_full: true
    state: cloned
    wait: true
```

## Notes

- Authentication uses Proxmox API tokens (not username/password).
- Set `strict_params: false` to pass custom or newer API parameters.
- The `wait` option polls the Proxmox task system until completion.
- Use `insecure_skip_tls_verify: true` alongside `validate_certs: false` for self-signed certs.
