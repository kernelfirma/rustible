---
summary: Reference for the postgresql_privs module that manages PostgreSQL privileges.
read_when: You need to grant or revoke privileges on PostgreSQL database objects from playbooks.
---

# postgresql_privs - Manage PostgreSQL Privileges

## Synopsis

Grants and revokes privileges on PostgreSQL database objects including databases, schemas, tables, sequences, functions, types, and default privileges.

## Classification

**Database** - requires the `database` feature flag. Classified as `RemoteCommand` with `HostExclusive` parallelization.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| role | yes | | string | Role/user to grant or revoke privileges for. |
| database | yes | | string | Target database name (used for connection and database-level privileges). |
| state | no | `present` | string | `present` or `grant` to grant, `absent` or `revoke` to revoke. |
| type | no | `table` | string | Object type: `database`, `schema`, `table`, `sequence`, `function`, `type`, `default_privs`. |
| objs | no | | string | Comma-separated object names, or `ALL_IN_SCHEMA` for all objects. |
| schema | no | `public` | string | Schema containing the target objects. |
| privs | yes | | string | Comma-separated privileges to manage (validated per object type). |
| grant_option | no | `false` | boolean | Grant WITH GRANT OPTION. |
| target_roles | no | | list | For `default_privs`, the roles whose default privileges to alter. |
| login_host | no | `localhost` | string | PostgreSQL server hostname. |
| login_port | no | `5432` | integer | PostgreSQL server port. |
| login_user | no | `postgres` | string | Login username. |
| login_password | no | | string | Login password. |
| login_unix_socket | no | | string | Unix socket path for local connections. |
| ssl_mode | no | `prefer` | string | SSL mode: `disable`, `allow`, `prefer`, `require`, `verify-ca`, `verify-full`. |

### Valid Privileges by Object Type

| Object Type | Valid Privileges |
|-------------|-----------------|
| database | CREATE, CONNECT, TEMP, TEMPORARY, ALL |
| schema | CREATE, USAGE, ALL |
| table | SELECT, INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, ALL |
| sequence | USAGE, SELECT, UPDATE, ALL |
| function | EXECUTE, ALL |
| type | USAGE, ALL |
| default_privs | SELECT, INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, USAGE, EXECUTE, ALL |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| role | string | The role that was granted/revoked privileges. |
| privileges | list | The privileges that were applied. |
| object_type | string | The object type that was targeted. |

## Examples

```yaml
- name: Grant SELECT on all tables in a schema
  postgresql_privs:
    role: myapp
    database: mydb
    type: table
    objs: ALL_IN_SCHEMA
    schema: public
    privs: SELECT

- name: Grant all privileges on specific tables
  postgresql_privs:
    role: admin
    database: mydb
    type: table
    objs: users,orders
    privs: ALL
    grant_option: true

- name: Revoke database CONNECT
  postgresql_privs:
    role: readonly
    database: mydb
    type: database
    privs: CONNECT
    state: absent

- name: Set default privileges for future tables
  postgresql_privs:
    role: app_user
    database: mydb
    type: default_privs
    objs: TABLES
    schema: public
    privs: SELECT,INSERT
    target_roles:
      - owner_role
```

## Notes

- Requires building with `--features database`.
- The role must exist before privileges can be granted or revoked.
- Using `ALL_IN_SCHEMA` as `objs` enumerates and applies privileges to every matching object in the schema.
- For `default_privs`, the `target_roles` parameter controls whose default privileges are altered; if omitted, the login user is used.
