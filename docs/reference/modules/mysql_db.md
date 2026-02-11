---
summary: Reference for the mysql_db module that manages MySQL databases.
read_when: You need to create or drop MySQL databases from playbooks.
---

# mysql_db - Manage MySQL Databases

## Synopsis

Creates and drops MySQL databases, with support for character encoding and collation settings. Connects directly to MySQL from the control node using connection pooling.

## Classification

**Database** - requires the `database` feature flag. Classified as `LocalLogic` with `HostExclusive` parallelization (operations serialized per host).

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | | string | Name of the database. Must be alphanumeric/underscore/dollar, max 64 chars, cannot start with a digit. |
| state | no | `present` | string | Desired state: `present` or `absent`. |
| encoding | no | | string | Character set for the database (e.g., `utf8mb4`). |
| collation | no | | string | Collation for the database (e.g., `utf8mb4_unicode_ci`). |
| login_host | no | `localhost` | string | MySQL server hostname. |
| login_port | no | `3306` | integer | MySQL server port. |
| login_user | no | `root` | string | MySQL login username. |
| login_password | no | | string | MySQL login password. |
| login_unix_socket | no | | string | Unix socket path for local connections. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| encoding | string | Character set of the database after the operation. |
| collation | string | Collation of the database after the operation. |

## Examples

```yaml
- name: Create a database
  mysql_db:
    name: myapp
    state: present
    encoding: utf8mb4
    collation: utf8mb4_unicode_ci
    login_user: root
    login_password: "{{ mysql_root_password }}"

- name: Drop a database
  mysql_db:
    name: old_database
    state: absent

- name: Ensure database exists with defaults
  mysql_db:
    name: staging_app
```

## Notes

- Requires building with `--features database`.
- System databases (`mysql`, `information_schema`, `performance_schema`, `sys`) cannot be managed and will be rejected.
- Database names are validated against MySQL naming rules; invalid characters cause an immediate error.
- Encoding and collation can be modified on an existing database via `ALTER DATABASE`.
- Uses connection pooling internally for efficient repeated operations.
