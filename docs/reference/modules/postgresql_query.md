---
summary: Reference for the postgresql_query module that executes SQL queries on PostgreSQL.
read_when: You need to run ad-hoc SQL queries or scripts against PostgreSQL from playbooks.
---

# postgresql_query - Execute PostgreSQL Queries

## Synopsis

Executes SQL queries or script files against a PostgreSQL database. Supports positional and named parameter substitution.

## Classification

**Database** - requires the `database` feature flag. Classified as `RemoteCommand` with `FullyParallel` parallelization (read queries can run concurrently).

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| query | no | | string | SQL query to execute. Mutually exclusive with `path_to_script`. |
| path_to_script | no | | string | Path to a SQL script file on the remote host. Mutually exclusive with `query`. |
| db | yes | | string | Target database name. |
| positional_args | no | | list | List of positional arguments substituted as `$1`, `$2`, etc. |
| named_args | no | | object | Dictionary of named arguments substituted as `%(name)` or `:name`. |
| encoding | no | | string | Character encoding for the script file. |
| autocommit | no | `false` | boolean | Use autocommit mode for the query. |
| as_single_query | no | `false` | boolean | Run a script as a single transaction (`psql -1`). |
| search_path | no | | string | Schema search path (`SET search_path TO ...`). |
| login_host | no | `localhost` | string | PostgreSQL server hostname. |
| login_port | no | `5432` | integer | PostgreSQL server port. |
| login_user | no | `postgres` | string | Login username. |
| login_password | no | | string | Login password. |
| login_unix_socket | no | | string | Unix socket path for local connections. |
| ssl_mode | no | `prefer` | string | SSL mode: `disable`, `allow`, `prefer`, `require`, `verify-ca`, `verify-full`. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| rowcount | integer | Number of rows returned or affected. |
| status | string | Execution status (`SUCCESS` or `FAILED`). |
| rows | list | Query result rows (only for SELECT; capped at 1000 rows). |
| columns | list | Column names from the result set. |

## Examples

```yaml
- name: Run a simple SELECT
  postgresql_query:
    db: myapp
    query: "SELECT COUNT(*) FROM users"
  register: result

- name: Insert with positional args
  postgresql_query:
    db: myapp
    query: "INSERT INTO users (name, email) VALUES ($1, $2)"
    positional_args:
      - "Alice"
      - "alice@example.com"

- name: Query with named args
  postgresql_query:
    db: myapp
    query: "SELECT * FROM orders WHERE status = :status"
    named_args:
      status: "pending"

- name: Execute a script file
  postgresql_query:
    db: myapp
    path_to_script: /opt/migrations/001_schema.sql
    as_single_query: true
```

## Notes

- Requires building with `--features database`.
- Exactly one of `query` or `path_to_script` must be provided.
- Modification queries (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP, TRUNCATE, GRANT, REVOKE) are skipped in check mode.
- Parameter substitution uses simple string replacement with SQL escaping, not true prepared statements.
