---
summary: Reference for the postgresql_user module that manages PostgreSQL users and roles.
read_when: You need to create, modify, or remove PostgreSQL users/roles from playbooks.
---

# postgresql_user - Manage PostgreSQL Users/Roles

## Synopsis

Creates, modifies, and removes PostgreSQL roles, including password management, role attribute flags, and group memberships.

## Classification

**Database** - requires the `database` feature flag. Classified as `RemoteCommand` with `HostExclusive` parallelization.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | | string | Name of the role to manage. |
| state | no | `present` | string | Desired state: `present` or `absent`. |
| password | no | | string | User password (plaintext or md5 hash). |
| encrypted | no | `false` | boolean | Whether the password value is already encrypted. |
| expires | no | | string | Account expiration timestamp (`YYYY-MM-DD HH:MM:SS` or `infinity`). |
| conn_limit | no | | integer | Connection limit for the role (-1 for unlimited). |
| role_attr_flags | no | | string | Comma-separated role attributes: `SUPERUSER`, `NOSUPERUSER`, `CREATEDB`, `NOCREATEDB`, `CREATEROLE`, `NOCREATEROLE`, `LOGIN`, `NOLOGIN`, `REPLICATION`, `NOREPLICATION`, `BYPASSRLS`, `NOBYPASSRLS`, `INHERIT`, `NOINHERIT`. |
| groups | no | | list | List of group roles to grant membership in. |
| db | no | | string | Database for privilege operations (also used as maintenance_db). |
| fail_on_user | no | `false` | boolean | Fail if the user already exists. |
| no_password_changes | no | `false` | boolean | Skip password updates if the user already exists. |
| login_host | no | `localhost` | string | PostgreSQL server hostname. |
| login_port | no | `5432` | integer | PostgreSQL server port. |
| login_user | no | `postgres` | string | Login username. |
| login_password | no | | string | Login password. |
| login_unix_socket | no | | string | Unix socket path for local connections. |
| ssl_mode | no | `prefer` | string | SSL mode: `disable`, `allow`, `prefer`, `require`, `verify-ca`, `verify-full`. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| name | string | Name of the managed role. |

## Examples

```yaml
- name: Create an application user with login
  postgresql_user:
    name: myapp_user
    password: "{{ vault_db_password }}"
    role_attr_flags: LOGIN,NOSUPERUSER,CREATEDB
    state: present

- name: Create a read-only user in a group
  postgresql_user:
    name: readonly_user
    password: "{{ vault_ro_password }}"
    groups:
      - readonly_group
    role_attr_flags: LOGIN

- name: Remove a user
  postgresql_user:
    name: old_user
    state: absent
```

## Notes

- Requires building with `--features database`.
- Password changes are always applied when `no_password_changes` is `false`, since encrypted password comparison is not performed.
- Role attribute flags are compared against the current state and only altered if different.
- Group memberships are additive; existing memberships are not revoked when updating.
