---
summary: Reference for the mysql_user module that manages MySQL users and their privileges.
read_when: You need to create, modify, or remove MySQL users from playbooks.
---

# mysql_user - Manage MySQL Users

## Synopsis

Creates, modifies, and removes MySQL users, including password management and privilege grants. Privileges are managed inline as part of user state.

## Classification

**Database** - requires the `database` feature flag. Classified as `LocalLogic` with `HostExclusive` parallelization.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | | string | Username to manage (max 80 characters). |
| host | no | `localhost` | string | Host pattern the user connects from (e.g., `%`, `localhost`, `192.168.1.%`). |
| state | no | `present` | string | Desired state: `present` or `absent`. |
| password | no | | string | User password (plaintext or pre-hashed). |
| encrypted | no | `false` | boolean | Whether the password value is already hashed. |
| priv | no | | string | Privileges in `db.table:PRIV1,PRIV2` format. Multiple specs separated by `/`. |
| append_privs | no | `false` | boolean | Append privileges instead of replacing all existing ones. |
| update_password | no | `always` | string | When to update the password: `always` or `on_create`. |
| login_host | no | `localhost` | string | MySQL server hostname. |
| login_port | no | `3306` | integer | MySQL server port. |
| login_user | no | | string | MySQL login username for authentication. |
| login_password | no | | string | MySQL login password for authentication. |

## Privilege Format

Privileges follow the pattern `db.table:PRIV1,PRIV2`, with multiple entries separated by `/`:

- `*.*:ALL` - All privileges on all databases.
- `mydb.*:ALL` - All privileges on a specific database.
- `mydb.mytable:SELECT,INSERT` - Specific privileges on a specific table.
- `db1.*:SELECT/db2.*:ALL` - Different privileges on different databases.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| user | string | The managed username. |
| host | string | The host pattern for the user. |
| grants | list | Current GRANT statements for the user after the operation. |

## Examples

```yaml
- name: Create a user with full database access
  mysql_user:
    name: myapp_user
    host: "%"
    password: "{{ app_password }}"
    priv: "myapp_db.*:ALL"
    state: present

- name: Create a read-only user
  mysql_user:
    name: readonly_user
    password: "{{ readonly_password }}"
    priv: "myapp_db.*:SELECT"

- name: Remove a user
  mysql_user:
    name: old_user
    state: absent
```

## Notes

- Requires building with `--features database`.
- When `append_privs` is `false` (default), all existing privileges are revoked before granting the specified set.
- Password is always updated when `update_password` is `always` and a `password` value is provided, even if the user already exists.
- Usernames are validated to contain only alphanumeric characters, underscores, hyphens, and dots.
