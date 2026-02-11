---
summary: Reference for the postgresql_db module that manages PostgreSQL databases.
read_when: You need to create, drop, dump, or restore PostgreSQL databases from playbooks.
---

# postgresql_db - Manage PostgreSQL Databases

## Synopsis

Creates, drops, dumps, and restores PostgreSQL databases on remote hosts via `psql`, `pg_dump`, and `pg_restore`.

## Classification

**Database** - requires the `database` feature flag. Classified as `RemoteCommand` with `HostExclusive` parallelization (operations are serialized per host).

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | | string | Name of the database to manage. |
| state | no | `present` | string | Desired state: `present`, `absent`, `dump`, or `restore`. |
| owner | no | | string | Database owner role. |
| encoding | no | `UTF8` | string | Character encoding for the database. |
| lc_collate | no | | string | Collation order (LC_COLLATE) setting. |
| lc_ctype | no | | string | Character classification (LC_CTYPE) setting. |
| template | no | `template0` | string | Template database used for creation. |
| tablespace | no | | string | Default tablespace for the database. |
| conn_limit | no | | integer | Maximum concurrent connections (-1 for unlimited). |
| target | no | | string | File path for dump/restore operations. Required when state is `dump` or `restore`. |
| target_opts | no | | string | Extra options passed to pg_dump or pg_restore. |
| dump_extra_args | no | | string | Additional arguments for pg_dump. |
| maintenance_db | no | `postgres` | string | Admin database used for connection during management operations. |
| force | no | `false` | boolean | Terminate active connections before dropping. |
| login_host | no | `localhost` | string | PostgreSQL server hostname. |
| login_port | no | `5432` | integer | PostgreSQL server port. |
| login_user | no | `postgres` | string | Login username. |
| login_password | no | | string | Login password. |
| login_unix_socket | no | | string | Unix socket path for local connections. |
| ssl_mode | no | `prefer` | string | SSL mode: `disable`, `allow`, `prefer`, `require`, `verify-ca`, `verify-full`. |
| ca_cert | no | | string | Path to CA certificate for SSL verification. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| name | string | Name of the managed database. |
| owner | string | Database owner (on creation). |
| encoding | string | Database encoding (on creation). |
| target | string | Dump/restore file path (on dump/restore). |

## Examples

```yaml
- name: Create a database
  postgresql_db:
    name: myapp
    owner: myapp_user
    encoding: UTF8

- name: Drop a database with force
  postgresql_db:
    name: old_db
    state: absent
    force: true

- name: Dump a database
  postgresql_db:
    name: myapp
    state: dump
    target: /backups/myapp.dump

- name: Restore a database from dump
  postgresql_db:
    name: myapp
    state: restore
    target: /backups/myapp.dump
```

## Notes

- Requires building with `--features database`.
- Dump format is auto-detected from the target file extension (`.sql` = plain, `.dump`/`.backup` = custom, `.tar` = tar, trailing `/` = directory).
- The `force` parameter terminates active connections via `pg_terminate_backend` before dropping.
- Encoding, collation, and template can only be set at creation time and cannot be altered later.
