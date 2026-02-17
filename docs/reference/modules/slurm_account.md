---
summary: Reference for the slurm_account module that manages Slurm accounts and user associations via sacctmgr.
read_when: You need to create, update, or delete Slurm accounts or manage user-account associations from playbooks.
---

# slurm_account - Manage Slurm Accounts

## Synopsis

Manages Slurm accounts and user associations via `sacctmgr`. Supports creating,
updating, and deleting accounts, as well as adding and removing user associations
with accounts. All operations are idempotent.

## Classification

**Default** - HPC module. Requires `hpc` and `slurm` feature flags.

## Parameters

| Parameter    | Required | Default | Type   | Description                                                                 |
|--------------|----------|---------|--------|-----------------------------------------------------------------------------|
| action       | yes      | -       | string | Action to perform: `create`, `update`, `delete`, `add_user`, or `remove_user`. |
| account      | yes      | -       | string | Account name.                                                               |
| user         | no       | -       | string | User name. Required for `add_user` and `remove_user` actions.               |
| organization | no       | -       | string | Organization name for the account.                                          |
| description  | no       | -       | string | Account description.                                                        |
| parent       | no       | -       | string | Parent account name.                                                        |
| max_jobs     | no       | -       | string | Maximum concurrent jobs allowed.                                            |
| max_submit   | no       | -       | string | Maximum submitted jobs allowed.                                             |
| max_wall     | no       | -       | string | Maximum wall time per job (e.g. `7-00:00:00`).                              |
| fairshare    | no       | -       | string | Fairshare value for scheduling priority.                                    |
| cluster      | no       | -       | string | Cluster name. Defaults to the current cluster.                              |

## Return Values

| Key        | Type    | Description                                     |
|------------|---------|-------------------------------------------------|
| changed    | boolean | Whether changes were made.                      |
| msg        | string  | Status message.                                 |
| data       | object  | Contains `account` name and optional `user`, `properties` fields. |

## Examples

```yaml
- name: Create a Slurm account for the physics department
  slurm_account:
    action: create
    account: physics
    organization: Physics
    description: "Physics department"
    parent: root
    fairshare: "100"
    max_jobs: "50"
    max_submit: "200"
    max_wall: "7-00:00:00"

- name: Add a user to an account
  slurm_account:
    action: add_user
    account: physics
    user: jdoe
    fairshare: "10"
    max_jobs: "5"

- name: Remove a user from an account
  slurm_account:
    action: remove_user
    account: physics
    user: jdoe

- name: Delete a Slurm account
  slurm_account:
    action: delete
    account: physics
```

## Notes

- Requires building with `--features hpc,slurm`.
- All actions are idempotent: creating an existing account or adding an already-associated user will report no change.
- The `user` parameter is required only for `add_user` and `remove_user` actions.
- The `max_jobs`, `max_submit`, `max_wall`, and `fairshare` parameters apply to both account-level and user-level associations.
- Uses `sacctmgr --immediate` to apply changes without confirmation prompts.
