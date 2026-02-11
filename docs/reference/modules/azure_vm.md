---
summary: Reference for the azure_vm module that manages Azure Virtual Machines.
read_when: You need to create, start, stop, or delete Azure VMs from playbooks.
---

# azure_vm - Manage Azure Virtual Machines

## Synopsis

The `azure_vm` module creates, updates, deletes, starts, stops, deallocates, and restarts
Azure Virtual Machines. It is feature-gated and requires building with `--features azure`.
This module is experimental.

## Classification

**Cloud** - Uses the Azure SDK to manage cloud compute resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Virtual machine name. |
| `resource_group` | yes | - | string | Resource group name. |
| `state` | no | present | string | Desired state: present, absent, running, stopped, deallocated, restarted. |
| `location` | no* | - | string | Azure region. *Required when creating. |
| `vm_size` | no | Standard_B1s | string | VM size (e.g., Standard_B2s). |
| `image` | no* | - | map/string | VM image reference or custom image ID. *Required when creating. |
| `admin_username` | no | - | string | Admin username for the VM. |
| `admin_password` | no | - | string | Admin password (prefer ssh_public_keys for Linux). |
| `ssh_public_keys` | no | - | list | SSH public keys (path and key_data). |
| `os_disk` | no | - | map | OS disk config (disk_size_gb, storage_account_type, caching). |
| `data_disks` | no | - | list | Data disk configurations. |
| `network_interfaces` | no | - | list | Network interface IDs to attach. |
| `subnet_id` | no | - | string | Subnet ID for auto-created NIC. |
| `public_ip` | no | false | bool | Create a public IP address. |
| `nsg_id` | no | - | string | Network security group ID. |
| `availability_set_id` | no | - | string | Availability set ID. |
| `zones` | no | - | list | Availability zones. |
| `managed_identity` | no | - | map/string | Managed identity config (type, user_assigned_identities). |
| `tags` | no | - | map | Key-value resource tags. |
| `wait` | no | true | bool | Wait for operation to complete. |
| `wait_timeout` | no | 600 | int | Timeout in seconds for wait operations. |
| `custom_data` | no | - | string | Custom data script for cloud-init. |
| `priority` | no | - | string | VM priority: Regular, Low, Spot. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `vm` | map | Full VM information object. |
| `id` | string | Azure resource ID of the VM. |

## Examples

### Create an Ubuntu VM

```yaml
- name: Create web server
  azure_vm:
    name: web-server-01
    resource_group: my-rg
    location: eastus
    vm_size: Standard_B2s
    image:
      publisher: Canonical
      offer: 0001-com-ubuntu-server-jammy
      sku: 22_04-lts-gen2
      version: latest
    admin_username: azureuser
    ssh_public_keys:
      - path: /home/azureuser/.ssh/authorized_keys
        key_data: ssh-rsa AAAAB3...
    managed_identity:
      type: SystemAssigned
    tags:
      Environment: production
    state: present
```

### Stop and deallocate a VM

```yaml
- name: Deallocate VM to save costs
  azure_vm:
    name: web-server-01
    resource_group: my-rg
    state: deallocated
```

## Notes

- Requires Azure credentials (AZURE_CLIENT_ID, AZURE_CLIENT_SECRET, AZURE_TENANT_ID) or Azure CLI login.
- Build with `--features azure` to enable this module. This feature is experimental.
- The `image` parameter accepts an object with publisher/offer/sku/version or a custom image ID string.
- The `stopped` state keeps the allocation (compute charges apply); use `deallocated` to release resources.
