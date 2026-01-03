---
summary: Recommendations for writing maintainable, efficient, and secure playbooks covering project organization, variable management, and security.
read_when: You want to improve playbook quality, organization, and security practices.
---

# Best Practices Guide

This guide provides recommendations for writing maintainable, efficient, and secure Rustible playbooks.

## Table of Contents

- [Project Organization](#project-organization)
- [Playbook Design](#playbook-design)
- [Variable Management](#variable-management)
- [Security Best Practices](#security-best-practices)
- [Performance Optimization](#performance-optimization)
- [Testing and Validation](#testing-and-validation)
- [Maintenance and Operations](#maintenance-and-operations)

---

## Project Organization

### Directory Structure

Organize your automation project consistently:

```
my-infrastructure/
  ansible.cfg                    # Configuration (optional)
  rustible.toml                  # Rustible configuration

  inventory/
    production/
      hosts.yml
      group_vars/
        all.yml
        webservers.yml
        databases.yml
      host_vars/
        web1.yml
    staging/
      hosts.yml
      group_vars/
        all.yml

  playbooks/
    site.yml                     # Main entry point
    webservers.yml
    databases.yml
    deploy.yml
    maintenance.yml

  roles/
    common/
      tasks/main.yml
      handlers/main.yml
      defaults/main.yml
      vars/main.yml
      templates/
      files/
      meta/main.yml
    webserver/
    database/

  group_vars/                    # Global group variables
    all.yml

  host_vars/                     # Global host variables

  files/                         # Static files
  templates/                     # Jinja2 templates
  library/                       # Custom modules (if any)
  filter_plugins/                # Custom filters (if any)
```

### Naming Conventions

1. **Use lowercase with hyphens** for file names:
   - `web-servers.yml` not `WebServers.yml`
   - `deploy-app.yml` not `deployApp.yml`

2. **Use underscores** for variable names:
   - `http_port` not `httpPort` or `http-port`
   - `database_config` not `databaseConfig`

3. **Prefix role variables** with role name:
   - `nginx_port` in nginx role
   - `mysql_root_password` in mysql role

4. **Use descriptive names**:
   - `web_app_deploy_path` not `path`
   - `enable_ssl_termination` not `ssl`

---

## Playbook Design

### Keep Playbooks Focused

One playbook should do one thing:

```yaml
# Good: Single-purpose playbook
# deploy.yml - Deploy the application
- name: Deploy application
  hosts: webservers
  tasks:
    - name: Deploy application code
      # ...

# Bad: Kitchen-sink playbook
# everything.yml - Does too many unrelated things
- name: Setup infrastructure, deploy app, configure monitoring
  # Too broad!
```

### Always Name Your Tasks

Names appear in output and make debugging easier:

```yaml
# Good
- name: Install nginx web server
  package:
    name: nginx
    state: present

# Bad - no name, hard to identify in logs
- package:
    name: nginx
    state: present
```

### Use Tags Strategically

Tags enable selective execution:

```yaml
- name: Install dependencies
  package:
    name: "{{ packages }}"
  tags:
    - install
    - dependencies

- name: Configure application
  template:
    src: app.conf.j2
    dest: /etc/app/config
  tags:
    - configure

- name: Verify service is running
  service:
    name: app
    state: started
  tags:
    - always  # Always runs
```

Run specific stages:

```bash
# Only install dependencies
rustible run playbook.yml --tags install

# Skip slow tasks
rustible run playbook.yml --skip-tags slow
```

### Use Blocks for Logical Grouping

Group related tasks with error handling:

```yaml
- name: Database setup
  block:
    - name: Create database
      command: createdb myapp

    - name: Run migrations
      command: /opt/app/migrate

    - name: Seed initial data
      command: /opt/app/seed

  rescue:
    - name: Rollback on failure
      command: dropdb myapp --if-exists

    - name: Notify team
      debug:
        msg: "Database setup failed on {{ inventory_hostname }}"

  always:
    - name: Log completion
      debug:
        msg: "Database setup process completed"

  become: true
  become_user: postgres
```

### Handle Idempotency

Tasks should be safe to run multiple times:

```yaml
# Good - idempotent
- name: Ensure directory exists
  file:
    path: /opt/app
    state: directory
    mode: '0755'

# Bad - not idempotent, fails if directory exists
- name: Create directory
  command: mkdir /opt/app
```

Use `creates` and `removes` for command modules:

```yaml
- name: Extract archive (only if not already done)
  command: tar -xzf /tmp/app.tar.gz
  args:
    chdir: /opt/
    creates: /opt/app/bin/app  # Skip if this file exists
```

---

## Variable Management

### Use Role Defaults for Flexibility

Put overridable settings in `defaults/main.yml`:

```yaml
# roles/nginx/defaults/main.yml
nginx_port: 80
nginx_worker_processes: auto
nginx_worker_connections: 1024
nginx_ssl_enabled: false
```

### Keep Secrets Separate

Never mix secrets with regular variables:

```yaml
# vars/common.yml - regular variables
app_name: myapp
app_version: "2.0.0"
app_port: 8080

# vars/secrets.yml - encrypted with vault
database_password: !vault |
  $RUSTIBLE_VAULT;1.0;AES256-GCM
  [encrypted content]
api_key: !vault |
  [encrypted content]
```

### Document Variables

Comment your variables:

```yaml
# Application configuration
app_name: myapp          # Application identifier
app_version: "2.0.0"     # Version to deploy
app_port: 8080           # HTTP port for the application

# Feature flags
enable_caching: true     # Redis caching
enable_metrics: true     # Prometheus metrics endpoint
```

### Use Variable Files for Environments

```yaml
# vars/production.yml
environment: production
debug_mode: false
log_level: warn
replicas: 3

# vars/staging.yml
environment: staging
debug_mode: true
log_level: debug
replicas: 1
```

Load dynamically:

```yaml
- hosts: all
  vars_files:
    - vars/common.yml
    - "vars/{{ env }}.yml"
```

### Avoid Deep Nesting

Keep variable structures shallow:

```yaml
# Good - shallow structure
database_host: db.example.com
database_port: 5432
database_name: myapp

# Avoid - deeply nested
config:
  database:
    connection:
      primary:
        host: db.example.com
        port: 5432
```

---

## Security Best Practices

### Encrypt Sensitive Data

Always encrypt secrets with vault:

```bash
# Encrypt file
rustible vault encrypt vars/secrets.yml

# Encrypt single string
rustible vault encrypt-string "password123" -p db_password
```

### Use Minimal Privileges

Only use `become` when necessary:

```yaml
# Good - become only where needed
- name: Read app config (no become needed)
  slurp:
    src: /opt/app/config.yml
  register: config

- name: Install system package (needs become)
  package:
    name: nginx
  become: true
```

### Secure SSH Configuration

```toml
# rustible.toml
[ssh]
host_key_checking = true
pipelining = true

[defaults]
private_key_file = "~/.ssh/deploy_key"
```

### Avoid Logging Sensitive Data

Mark sensitive tasks with `no_log`:

```yaml
- name: Set database password
  command: "/opt/app/set-password '{{ db_password }}'"
  no_log: true  # Don't log the command (contains password)
```

### Validate User Input

When accepting extra vars, validate them:

```yaml
- name: Validate version format
  assert:
    that:
      - app_version is defined
      - app_version is match('^v?[0-9]+\.[0-9]+\.[0-9]+$')
    fail_msg: "Invalid version format: {{ app_version }}"
```

---

## Performance Optimization

### Disable Fact Gathering When Not Needed

```yaml
- hosts: all
  gather_facts: false  # Skip if facts aren't used
  tasks:
    - name: Quick operation
      command: echo "hello"
```

### Use Free Strategy for Independent Tasks

```yaml
- hosts: webservers
  strategy: free  # Maximum parallelism
  tasks:
    - name: Independent task
      command: /opt/app/update.sh
```

### Increase Forks for Large Fleets

```bash
# Default is 5, increase for many hosts
rustible run playbook.yml -f 20
```

### Batch File Operations

```yaml
# Bad - many small transfers
- name: Copy files one by one
  copy:
    src: "files/{{ item }}"
    dest: "/opt/app/{{ item }}"
  loop: "{{ file_list }}"

# Good - single archive transfer
- name: Deploy as archive
  unarchive:
    src: app-files.tar.gz
    dest: /opt/app/
```

### Cache Facts Between Runs

Rustible automatically caches facts. Configure caching:

```toml
# rustible.toml
[cache]
facts_ttl = 600  # 10 minutes
```

### Use Asynchronous Tasks for Long Operations

```yaml
- name: Long running backup
  command: /opt/backup-all.sh
  async: 3600   # Max runtime
  poll: 0       # Don't wait

- name: Do other work
  # ... other tasks ...

- name: Check backup completed
  async_status:
    jid: "{{ backup_job.ansible_job_id }}"
  register: result
  until: result.finished
  retries: 60
  delay: 60
```

---

## Testing and Validation

### Use Check Mode First

Always preview changes:

```bash
# Dry run
rustible run playbook.yml --check

# With diff output
rustible run playbook.yml --check --diff
```

### Use Plan Mode

View execution plan:

```bash
rustible run playbook.yml --plan
```

### Validate Playbook Syntax

```bash
rustible validate playbook.yml
```

### Test on Limited Hosts

```bash
# Single host
rustible run playbook.yml --limit test-host

# Subset
rustible run playbook.yml --limit 'webservers[0:2]'
```

### Add Assertions

Verify expected state:

```yaml
- name: Verify application is healthy
  uri:
    url: http://localhost:8080/health
    status_code: 200
  register: health

- name: Assert health check passed
  assert:
    that:
      - health.status == 200
      - "'healthy' in health.json.status"
    fail_msg: "Application health check failed"
```

### Use Serial Deployment for Safety

```yaml
- hosts: webservers
  serial: [1, "25%", "100%"]  # Canary deployment
  max_fail_percentage: 10

  tasks:
    - name: Deploy with safety
      # ...
```

---

## Maintenance and Operations

### Version Control Everything

```bash
# .gitignore
*.retry
*.vault_pass
.vault_password
secrets.yml.decrypted
```

### Document Your Automation

Create README files:

```markdown
# My Infrastructure Automation

## Quick Start
rustible run site.yml -i inventory/production

## Playbooks
- site.yml: Full infrastructure setup
- deploy.yml: Application deployment
- rollback.yml: Rollback to previous version

## Variables
See vars/README.md for variable documentation
```

### Use Meaningful Commit Messages

```bash
git commit -m "Add Redis caching to web tier

- Configure Redis connection pool
- Add cache invalidation handlers
- Update nginx config for cache headers"
```

### Monitor Playbook Runs

Log execution results:

```bash
rustible run playbook.yml 2>&1 | tee "logs/deploy-$(date +%Y%m%d-%H%M%S).log"
```

### Plan for Rollback

Always have a rollback strategy:

```yaml
# deploy.yml
- name: Backup current version
  archive:
    path: /opt/app/current
    dest: /opt/app/backup-{{ ansible_date_time.iso8601 }}.tar.gz
  before: deploy

# rollback.yml
- name: Restore previous version
  unarchive:
    src: "{{ backup_file }}"
    dest: /opt/app/current
    remote_src: true
```

### Regular Cleanup

```yaml
# maintenance.yml
- name: Cleanup old releases
  shell: |
    cd /opt/app/releases
    ls -t | tail -n +6 | xargs rm -rf
  args:
    executable: /bin/bash
```

---

## Quick Reference Checklist

Before committing a playbook:

- [ ] All tasks have descriptive names
- [ ] Variables are documented
- [ ] Secrets are encrypted with vault
- [ ] Idempotent (safe to run multiple times)
- [ ] `become` only used when necessary
- [ ] Sensitive operations use `no_log: true`
- [ ] Error handling with blocks where appropriate
- [ ] Tags added for selective execution
- [ ] Tested with `--check` mode
- [ ] Validated with `rustible validate`

For production deployments:

- [ ] Tested in staging first
- [ ] Serial deployment configured
- [ ] Rollback plan documented
- [ ] Health checks included
- [ ] Logging and monitoring in place
