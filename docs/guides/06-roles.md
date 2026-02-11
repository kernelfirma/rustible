---
summary: Creating and using roles for reusable, shareable automation including directory structure, dependencies, and Galaxy integration.
read_when: You want to organize playbook logic into reusable roles, manage role dependencies, or install roles from Galaxy.
---

# Chapter 6: Roles - Reusable Automation

Roles are the primary mechanism for organizing and reusing automation logic. A role packages tasks, handlers, variables, templates, and files into a self-contained unit that can be shared across playbooks and teams.

## Why Use Roles?

Without roles, large playbooks become difficult to maintain. Roles solve this by:

- **Encapsulating** related tasks, variables, and files together
- **Enabling reuse** across multiple playbooks and projects
- **Establishing conventions** through a standard directory structure
- **Supporting sharing** via Ansible Galaxy or private repositories

## Role Directory Structure

A role follows a standard layout:

```
roles/
  webserver/
    tasks/
      main.yml          # Entry point for tasks
      install.yml       # Optional: broken-out task files
      configure.yml
    handlers/
      main.yml          # Handlers triggered by notify
    templates/
      nginx.conf.j2     # Jinja2 templates
      vhost.conf.j2
    files/
      index.html        # Static files to copy
    vars/
      main.yml          # High-precedence variables
    defaults/
      main.yml          # Low-precedence defaults (easily overridden)
    meta/
      main.yml          # Role metadata and dependencies
```

Each directory is optional. Rustible loads only the directories that exist.

| Directory | Purpose | Loaded When |
|-----------|---------|-------------|
| `tasks/` | Task definitions | Always (entry point: `main.yml`) |
| `handlers/` | Notification handlers | Always |
| `templates/` | Jinja2 template files | Referenced by `template` module |
| `files/` | Static files | Referenced by `copy` module |
| `vars/` | High-precedence variables | Always |
| `defaults/` | Low-precedence default values | Always |
| `meta/` | Role metadata and dependencies | Always |

## Creating a Role

### Using the CLI

```bash
rustible init --template role roles/webserver
```

This scaffolds the full directory structure with placeholder `main.yml` files.

### Manual Creation

```bash
mkdir -p roles/webserver/{tasks,handlers,templates,files,vars,defaults,meta}
```

### Example Role

**`roles/webserver/defaults/main.yml`** -- easily overridden defaults:

```yaml
http_port: 80
document_root: /var/www/html
server_name: localhost
worker_connections: 1024
```

**`roles/webserver/tasks/main.yml`** -- task entry point:

```yaml
---
- name: Install nginx
  package:
    name: nginx
    state: present

- name: Deploy nginx configuration
  template:
    src: nginx.conf.j2
    dest: /etc/nginx/nginx.conf
    owner: root
    group: root
    mode: '0644'
  notify: Restart nginx

- name: Deploy site configuration
  template:
    src: vhost.conf.j2
    dest: /etc/nginx/sites-enabled/default
    mode: '0644'
  notify: Reload nginx

- name: Ensure nginx is running
  service:
    name: nginx
    state: started
    enabled: true
```

**`roles/webserver/handlers/main.yml`**:

```yaml
---
- name: Restart nginx
  service:
    name: nginx
    state: restarted

- name: Reload nginx
  service:
    name: nginx
    state: reloaded
```

**`roles/webserver/meta/main.yml`** -- metadata and dependencies:

```yaml
---
dependencies:
  - common
platforms:
  - Debian
  - Ubuntu
  - RedHat
```

## Using Roles in Playbooks

### The roles Keyword

The simplest way to apply roles:

```yaml
- hosts: webservers
  become: true
  roles:
    - common
    - webserver
    - monitoring
```

### With Parameters

Override role defaults by passing variables:

```yaml
- hosts: webservers
  roles:
    - role: webserver
      vars:
        http_port: 8080
        server_name: app.example.com

    - role: database
      vars:
        db_port: 5432
```

### include_role (Dynamic)

Include a role dynamically at runtime. Supports conditionals and loops:

```yaml
tasks:
  - name: Apply role based on OS
    include_role:
      name: "{{ ansible_os_family | lower }}_setup"

  - name: Apply multiple app roles
    include_role:
      name: "{{ item }}"
    loop:
      - app_frontend
      - app_backend
      - app_worker
```

### import_role (Static)

Import a role at parse time. Tags and conditions from the importing task are applied to all tasks in the role:

```yaml
tasks:
  - name: Import webserver role
    import_role:
      name: webserver
    tags: web
```

## Role Variables

### defaults/main.yml (Low Precedence)

Variables in `defaults/` have the **lowest precedence** (level 1). They serve as documented default values that consumers of the role are expected to override:

```yaml
# roles/webserver/defaults/main.yml
http_port: 80
ssl_enabled: false
ssl_cert_path: /etc/ssl/certs/server.crt
max_connections: 512
```

### vars/main.yml (High Precedence)

Variables in `vars/` have **high precedence** (level 13). Use these for values that should not typically be overridden by inventory or play variables:

```yaml
# roles/webserver/vars/main.yml
nginx_user: www-data
nginx_pid: /run/nginx.pid
nginx_conf_dir: /etc/nginx
```

### When to Use Which

| Use Case | Location | Precedence |
|----------|----------|------------|
| User-configurable settings | `defaults/main.yml` | Level 1 (lowest) |
| Internal role constants | `vars/main.yml` | Level 13 (high) |
| Per-host overrides | Inventory `host_vars/` | Level 7-8 |
| Per-invocation overrides | Play `vars:` or `-e` | Level 10-20 |

## Role Dependencies

Define dependencies in `meta/main.yml`. Dependencies are executed before the role itself:

```yaml
# roles/webserver/meta/main.yml
dependencies:
  - common
  - role: firewall
    vars:
      allowed_ports:
        - 80
        - 443
```

Dependencies are deduplicated by default -- a role is only applied once even if multiple roles depend on it. To allow a dependency to run multiple times, set `allow_duplicates: true` in the dependent role's `meta/main.yml`.

## Galaxy Integration

### Installing Roles

Install roles from Ansible Galaxy or Git repositories:

```bash
# From Galaxy
rustible galaxy install geerlingguy.nginx

# From Git
rustible galaxy install git+https://github.com/user/role.git

# With version
rustible galaxy install geerlingguy.nginx,3.1.0
```

### Requirements File

Define all role dependencies in a `requirements.yml`:

```yaml
---
roles:
  - name: geerlingguy.nginx
    version: "3.1.0"
  - name: geerlingguy.certbot
    version: "5.0.0"
  - src: https://github.com/user/custom-role.git
    version: main
    name: custom_role
```

```bash
rustible galaxy install -r requirements.yml
```

### Collections

Roles can also be distributed as part of collections using the `namespace.collection` format:

```yaml
roles:
  - role: community.general.some_role
```

Collections bundle roles, modules, and plugins into a single distributable package.

## Role Tags

Apply tags to an entire role:

```yaml
roles:
  - role: webserver
    tags:
      - web
      - deploy

  - role: monitoring
    tags:
      - monitoring
```

Run only specific role tasks:

```bash
rustible run site.yml --tags web
```

## Role Search Path

Rustible looks for roles in the following locations (in order):

1. `roles/` directory relative to the playbook
2. Paths defined in `roles_path` configuration
3. `~/.rustible/roles/` (user-level installed roles)
4. `/etc/rustible/roles/` (system-level installed roles)

## Best Practices

1. **Keep roles focused**. Each role should do one thing well. A "webserver" role should not also configure the database.
2. **Document defaults**. Every variable in `defaults/main.yml` should have a comment explaining its purpose and valid values.
3. **Use defaults for configuration, vars for constants**. If users should be able to change a value, put it in `defaults/`. If it is an implementation detail, put it in `vars/`.
4. **Test roles independently**. Each role should be testable on its own with a simple test playbook.
5. **Version your roles**. Use Git tags for role versions when distributing via Git.
6. **Minimize dependencies**. Too many dependencies make roles fragile and hard to debug.
7. **Use `meta/main.yml`** to declare supported platforms and dependencies.

## Next Steps

- Learn about [Execution Strategies](07-execution-strategies.md)
- Understand [Security and Vault](08-security.md)
- Explore [Templating](09-templating.md) for role templates
