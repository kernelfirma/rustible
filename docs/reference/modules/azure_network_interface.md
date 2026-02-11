---
summary: Reference for the azure_network_interface module that manages Azure Network Interfaces.
read_when: You need to create or delete Azure NICs from playbooks.
---

# azure_network_interface - Manage Azure Network Interfaces

## Synopsis

The `azure_network_interface` module creates, updates, and deletes Azure Network Interfaces.
NICs are required for connecting VMs to virtual networks. Feature-gated and requires
building with `--features azure`. This module is experimental.

## Classification

**Cloud** - Uses the Azure SDK to manage cloud networking resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Network interface name. |
| `resource_group` | yes | - | string | Resource group name. |
| `location` | no* | - | string | Azure region. *Required when creating. |
| `subnet_id` | no* | - | string | Subnet resource ID. *Required when creating. |
| `state` | no | present | string | Desired state: present, absent. |
| `private_ip_address` | no | - | string | Static private IP address. |
| `private_ip_allocation` | no | Dynamic | string | IP allocation method: Dynamic, Static. |
| `public_ip_address_id` | no | - | string | Public IP address resource ID to associate. |
| `nsg_id` | no | - | string | Network security group resource ID. |
| `enable_accelerated_networking` | no | false | bool | Enable accelerated networking. |
| `enable_ip_forwarding` | no | false | bool | Enable IP forwarding on the NIC. |
| `tags` | no | - | map | Key-value resource tags. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `network_interface` | map | Full NIC information object. |
| `network_interface.id` | string | Azure resource ID. |
| `network_interface.private_ip_address` | string | Assigned private IP. |
| `network_interface.mac_address` | string | MAC address. |

## Examples

### Create a network interface

```yaml
- name: Create NIC for web server
  azure_network_interface:
    name: web-nic-01
    resource_group: prod-rg
    location: eastus
    subnet_id: /subscriptions/.../subnets/default
    private_ip_allocation: Dynamic
    enable_accelerated_networking: true
    tags:
      Role: webserver
    state: present
```

### Create a NIC with static IP

```yaml
- name: Create NIC with static IP
  azure_network_interface:
    name: db-nic-01
    resource_group: prod-rg
    location: eastus
    subnet_id: /subscriptions/.../subnets/data
    private_ip_address: 10.0.2.10
    private_ip_allocation: Static
    state: present
```

## Notes

- Requires Azure credentials (AZURE_CLIENT_ID, AZURE_CLIENT_SECRET, AZURE_TENANT_ID) or Azure CLI login.
- Build with `--features azure` to enable this module. This feature is experimental.
- A NIC cannot be deleted while attached to a running VM.
- Accelerated networking requires a supported VM size.
