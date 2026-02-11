---
summary: Reference for the proxmox_vm module that manages Proxmox QEMU/KVM virtual machines.
read_when: You need to create, clone, start, stop, or delete Proxmox VMs from playbooks.
---

# proxmox_vm - Manage Proxmox QEMU/KVM Virtual Machines

## Synopsis

The `proxmox_vm` module manages the full lifecycle of Proxmox VE QEMU/KVM virtual machines
via the Proxmox REST API. Supports creation, cloning, deletion, power state management,
and idempotent configuration updates. This is a core module and requires no feature flags.

## Classification

**LocalLogic** - Executes API calls from the controller to the Proxmox host.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `api_url` | yes | - | string | Proxmox API URL (e.g., https://pve:8006). |
| `api_token_id` | yes | - | string | API token ID (e.g., user@pam!token). |
| `api_token_secret` | yes | - | string | API token secret. |
| `node` | yes | - | string | Proxmox node name. |
| `vmid` | yes | - | int | Virtual machine VMID. |
| `state` | no | status | string | Desired state: status, started, stopped, restarted, present, absent, cloned. |
| `name` | no | - | string | VM name (required for create). |
| `description` | no | - | string | VM description. |
| `tags` | no | - | string | Comma-separated tags. |
| `stop_method` | no | shutdown | string | How to stop: shutdown (graceful) or stop (immediate). |
| `create` | no | - | map | Extra creation parameters passed to the Proxmox API. |
| `clone` | no | - | map | Extra clone parameters passed to the Proxmox API. |
| `delete` | no | - | map | Extra deletion parameters (purge, etc.). |
| `config` | no | - | map | Desired VM configuration for idempotent updates. |
| `clone_from` | no | - | int | Source VMID to clone from (required for state=cloned). |
| `clone_full` | no | true | bool | Full clone (true) or linked clone (false). |
| `clone_target_node` | no | - | string | Target node for clone. |
| `clone_storage` | no | - | string | Target storage for clone. |
| `auto_name` | no | false | bool | Auto-generate name as rustible-vm-{vmid}. |
| `strict_params` | no | true | bool | Validate params against Proxmox API allowlists. |
| `timeout` | no | 30 | int | HTTP request timeout in seconds. |
| `validate_certs` | no | true | bool | Validate TLS certificates. |
| `ca_cert_path` | no | - | string | Path to custom CA certificate (PEM). |
| `wait` | no | false | bool | Wait for API tasks to complete. |
| `wait_timeout` | no | 300 | int | Timeout in seconds for task wait. |
| `wait_interval` | no | 2 | int | Poll interval in seconds for task wait. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `vmid` | int | Virtual machine VMID. |
| `node` | string | Proxmox node name. |
| `present` | bool | Whether the VM exists. |
| `status` | string | VM power state (running, stopped, absent). |
| `upid` | string | Proxmox task UPID (for async operations). |
| `task` | map | Task completion details (when wait=true). |
| `config_changes` | list | Configuration changes applied (when updating). |

## Examples

### Create a VM

```yaml
- name: Create QEMU VM
  proxmox_vm:
    api_url: https://pve.example:8006
    api_token_id: root@pam!automation
    api_token_secret: "{{ vault_proxmox_token }}"
    node: pve
    vmid: 200
    name: web-vm
    state: present
    create:
      memory: 2048
      cores: 2
      sockets: 1
      scsi0: "local-lvm:32,format=qcow2"
      net0: "virtio,bridge=vmbr0"
      ostype: l26
      boot: "order=scsi0;net0"
      cdrom: "local:iso/debian-12.5.0-amd64-netinst.iso"
    wait: true
```

### Update VM configuration idempotently

```yaml
- name: Ensure VM has 4GB memory
  proxmox_vm:
    api_url: https://pve.example:8006
    api_token_id: root@pam!automation
    api_token_secret: "{{ vault_proxmox_token }}"
    node: pve
    vmid: 200
    state: present
    config:
      memory: "4096"
      cores: "4"
```

### Clone a VM

```yaml
- name: Clone VM 200 to 201
  proxmox_vm:
    api_url: https://pve.example:8006
    api_token_id: root@pam!automation
    api_token_secret: "{{ vault_proxmox_token }}"
    node: pve
    vmid: 201
    name: web-vm-clone
    clone_from: 200
    clone_full: true
    state: cloned
    wait: true
```

### Start a VM

```yaml
- name: Start the VM
  proxmox_vm:
    api_url: https://pve.example:8006
    api_token_id: root@pam!automation
    api_token_secret: "{{ vault_proxmox_token }}"
    node: pve
    vmid: 200
    state: started
```

## Notes

- Authentication uses Proxmox API tokens (not username/password).
- The `config` parameter enables idempotent updates: only changed values are sent to the API.
- Set `strict_params: false` to pass custom or newer API parameters.
- The `wait` option polls the Proxmox task system until completion.
- Use `validate_certs: false` with `insecure_skip_tls_verify: true` for self-signed certs (VM module does not require the insecure opt-in).
