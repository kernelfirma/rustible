---
summary: Reference for the win_user module that manages Windows local user accounts.
read_when: You need to create, modify, query, or remove local user accounts on Windows from playbooks.
---

# win_user - Manage Windows Local User Accounts

## Synopsis

Creates, modifies, queries, and removes local Windows user accounts. Supports setting
passwords, configuring account policies, and managing local group membership with
add, remove, or set semantics.

## Classification

**RemoteCommand** - Windows module (experimental). Requires `winrm` feature flag.

## Parameters

| Parameter              | Required | Default    | Type        | Description                                                       |
|------------------------|----------|------------|-------------|-------------------------------------------------------------------|
| name                   | yes      | -          | string      | The username of the local account.                                |
| state                  | no       | `present`  | string      | Desired state: `present`, `absent`, `query`.                      |
| password               | no       | -          | string      | Password in plaintext (securely converted via PowerShell).        |
| fullname               | no       | -          | string      | Full display name of the user.                                    |
| description            | no       | -          | string      | User account description / comment.                               |
| groups                 | no       | -          | list        | List of local groups for the user.                                |
| groups_action          | no       | `add`      | string      | How to handle groups: `add`, `remove`, `set`.                     |
| password_expired       | no       | -          | bool        | Force password change on next login.                              |
| password_never_expires | no       | -          | bool        | Set the password to never expire.                                 |
| account_disabled       | no       | -          | bool        | Disable the user account.                                         |

## Return Values

| Key  | Type   | Description                                                  |
|------|--------|--------------------------------------------------------------|
| user | object | Full user details including SID, groups, and account status. |

## Examples

```yaml
- name: Create a service account with group membership
  win_user:
    name: svc_app
    fullname: Application Service Account
    description: Runs the backend application
    password: "{{ vault_svc_password }}"
    groups:
      - Users
      - Remote Desktop Users
    state: present

- name: Query an existing user
  win_user:
    name: Administrator
    state: query

- name: Remove an obsolete user
  win_user:
    name: old_contractor
    state: absent

- name: Disable an account and force password reset
  win_user:
    name: temp_user
    account_disabled: true
    password_expired: true
```

## Notes

- Requires building with `--features winrm`.
- Uses `Get-LocalUser`, `New-LocalUser`, `Set-LocalUser`, and `Remove-LocalUser` cmdlets.
- The `groups_action: set` mode replaces the entire group membership list.
- When `state: query`, no changes are made; user details are returned as data.
- The `password` value is converted to a `SecureString` on the target and never stored in plaintext.
