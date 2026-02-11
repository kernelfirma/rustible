---
summary: Reference for the azure_resource_group module that manages Azure Resource Groups.
read_when: You need to create or delete Azure Resource Groups from playbooks.
---

# azure_resource_group - Manage Azure Resource Groups

## Synopsis

The `azure_resource_group` module creates and deletes Azure Resource Groups. Resource groups
are the fundamental organizational unit for Azure resources. Feature-gated and requires
building with `--features azure`. This module is experimental.

## Classification

**Cloud** - Uses the Azure SDK to manage cloud resources.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Resource group name. |
| `location` | no* | - | string | Azure region (e.g., eastus). *Required when creating. |
| `state` | no | present | string | Desired state: present, absent. |
| `tags` | no | - | map | Key-value resource tags. |
| `force_delete` | no | false | bool | Force delete even if the group contains resources. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `resource_group` | map | Resource group information object. |
| `resource_group.id` | string | Azure resource ID. |
| `resource_group.name` | string | Resource group name. |
| `resource_group.location` | string | Azure region. |
| `resource_group.provisioning_state` | string | Provisioning state (e.g., Succeeded). |

## Examples

### Create a resource group

```yaml
- name: Create production resource group
  azure_resource_group:
    name: prod-rg
    location: eastus
    tags:
      Environment: production
      CostCenter: engineering
    state: present
```

### Delete a resource group

```yaml
- name: Remove staging resource group
  azure_resource_group:
    name: staging-rg
    force_delete: true
    state: absent
```

## Notes

- Requires Azure credentials (AZURE_CLIENT_ID, AZURE_CLIENT_SECRET, AZURE_TENANT_ID) or Azure CLI login.
- Build with `--features azure` to enable this module. This feature is experimental.
- Deleting a resource group removes all resources within it.
- Use `force_delete: true` to delete groups that still contain resources.
