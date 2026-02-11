---
summary: Reference for the mysql_privs module that manages MySQL user privileges.
read_when: You need to grant or revoke granular MySQL privileges from playbooks.
---

# mysql_privs - Manage MySQL Privileges

## Synopsis

Provides granular control over MySQL user privileges, independent from user creation. Supports granting, revoking, appending, and the GRANT OPTION.

## Classification

**Database** - requires the `database` feature flag. Classified as `LocalLogic` with `HostExclusive` parallelization.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| user | yes | | string | MySQL username to manage privileges for. |
| priv | yes | | string | Privileges in `db.table:PRIV1,PRIV2` format. Multiple specs separated by `/`. |
| host | no | `localhost` | string | Host pattern the user connects from. |
| state | no | `present` | string | `present`/`grant` to grant, `absent`/`revoke` to revoke. |
| append_privs | no | `false` | boolean | Append privileges without revoking existing ones first. |
| grant_option | no | `false` | boolean | Grant WITH GRANT OPTION on the specified privileges. |
| login_host | no | `localhost` | string | MySQL server hostname. |
| login_port | no | `3306` | integer | MySQL server port. |
| login_user | no | | string | MySQL login username for authentication. |
| login_password | no | | string | MySQL login password for authentication. |

## Valid Privileges

ALL, ALTER, ALTER ROUTINE, CREATE, CREATE ROUTINE, CREATE TABLESPACE, CREATE TEMPORARY TABLES, CREATE USER, CREATE VIEW, DELETE, DROP, EVENT, EXECUTE, FILE, GRANT OPTION, INDEX, INSERT, LOCK TABLES, PROCESS, REFERENCES, RELOAD, REPLICATION CLIENT, REPLICATION SLAVE, SELECT, SHOW DATABASES, SHOW VIEW, SHUTDOWN, SUPER, TRIGGER, UPDATE, USAGE.

## Privilege Format

Privileges follow the pattern `db.table:PRIV1,PRIV2`, with multiple entries separated by `/`:

- `*.*:ALL` - All privileges on everything.
- `mydb.*:SELECT,INSERT` - Specific privileges on all tables in a database.
- `mydb.mytable:SELECT` - Privilege on a specific table.
- `db1.*:SELECT/db2.*:ALL` - Different privileges across databases.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| user | string | The username that was modified. |
| host | string | The host pattern for the user. |
| grants | list | Current GRANT statements after the operation. |

## Examples

```yaml
- name: Grant all privileges on a database
  mysql_privs:
    user: myapp_user
    host: "%"
    priv: "myapp_db.*:ALL"
    state: present

- name: Grant read-only access
  mysql_privs:
    user: readonly_user
    priv: "myapp_db.*:SELECT"

- name: Grant with GRANT OPTION
  mysql_privs:
    user: dba
    priv: "*.*:ALL"
    grant_option: true

- name: Append privileges without revoking
  mysql_privs:
    user: developer
    priv: "staging_db.*:ALL"
    append_privs: true

- name: Revoke privileges
  mysql_privs:
    user: old_user
    priv: "sensitive_db.*:ALL"
    state: absent
```

## Notes

- Requires building with `--features database`.
- The target user must already exist; the module will fail if the user is not found.
- When `append_privs` is `false` (default) and state is `present`, all existing privileges are revoked before granting.
- Privilege names are validated against the full list of MySQL privileges; unknown names cause an immediate error.
- A `FLUSH PRIVILEGES` is issued after every grant or revoke operation.
