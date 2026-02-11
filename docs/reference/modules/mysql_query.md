---
summary: Reference for the mysql_query module that executes SQL queries on MySQL.
read_when: You need to run ad-hoc SQL queries against MySQL databases from playbooks.
---

# mysql_query - Execute MySQL Queries

## Synopsis

Executes one or more SQL queries against a MySQL database. Supports single queries, query lists, and transactional execution. Returns structured result data for SELECT-type queries.

## Classification

**Database** - requires the `database` feature flag. Classified as `LocalLogic` with `HostExclusive` parallelization.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| query | yes | | string or list | SQL query string, or list of query strings to execute. |
| db | no | | string | Database to run queries against. |
| single_transaction | no | `false` | boolean | Wrap all queries in a single transaction (START TRANSACTION / COMMIT). |
| login_host | no | `localhost` | string | MySQL server hostname. |
| login_port | no | `3306` | integer | MySQL server port. |
| login_user | no | | string | MySQL login username. |
| login_password | no | | string | MySQL login password. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| query_result | list | Per-query results with `rows_affected`, `row_count`, `columns`, and `rows`. |
| rowcount | integer | Total number of rows returned across all queries. |
| rows_affected | integer | Total number of rows affected by modification queries. |
| rows | list | Result rows from a single SELECT query (maps of column name to value). |

## Examples

```yaml
- name: Run a simple SELECT
  mysql_query:
    db: myapp
    query: "SELECT COUNT(*) AS total FROM users"
  register: result

- name: Execute multiple queries in a transaction
  mysql_query:
    db: myapp
    query:
      - "INSERT INTO audit_log (action) VALUES ('start')"
      - "UPDATE users SET last_seen = NOW()"
      - "INSERT INTO audit_log (action) VALUES ('end')"
    single_transaction: true

- name: Create a table
  mysql_query:
    db: myapp
    query: |
      CREATE TABLE IF NOT EXISTS users (
        id INT PRIMARY KEY AUTO_INCREMENT,
        username VARCHAR(255) NOT NULL,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
      )
```

## Notes

- Requires building with `--features database`.
- All queries are treated as modifications in check mode and are skipped.
- SELECT, SHOW, DESCRIBE, and EXPLAIN queries are detected as read-only and return result rows.
- Binary column data (BLOB types) is returned as base64-encoded strings.
- When `single_transaction` is true and any query fails, the entire transaction is rolled back.
