---
summary: Reference for the template module that renders Jinja2 templates with variables and deploys them to remote hosts.
read_when: You need to deploy configuration files with dynamic content based on variables and facts.
---

# template - Template Files with Jinja2

## Synopsis

The `template` module templates a file to a remote location using Jinja2 templating. Variables and facts are available for substitution in the template.

## Classification

**NativeTransport** - This module uses native Rust operations for template rendering and SSH/SFTP for file transfer.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `src` | yes | - | string | Path to the template file on the local machine. |
| `dest` | yes | - | string | Remote absolute path where the file should be created. |
| `owner` | no | - | string | Name of the user that should own the file. |
| `group` | no | - | string | Name of the group that should own the file. |
| `mode` | no | - | string | Permissions of the file (e.g., "0644"). |
| `backup` | no | false | boolean | Create a backup file including the timestamp. |
| `force` | no | true | boolean | If false, only transfer if destination does not exist. |
| `validate` | no | - | string | Command to validate the file before use (use %s for file path). |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `dest` | string | Destination file path |
| `src` | string | Source template path |
| `checksum` | string | SHA1 checksum of the rendered file |
| `size` | integer | Size of the rendered file in bytes |
| `owner` | string | Owner of the file |
| `group` | string | Group of the file |
| `mode` | string | Permissions of the file |
| `backup_file` | string | Path to backup file (if backup was created) |

## Examples

### Template a configuration file

```yaml
- name: Template nginx configuration
  template:
    src: templates/nginx.conf.j2
    dest: /etc/nginx/nginx.conf
    owner: root
    group: root
    mode: "0644"
```

### Template with validation

```yaml
- name: Template Apache config with validation
  template:
    src: templates/httpd.conf.j2
    dest: /etc/httpd/conf/httpd.conf
    validate: httpd -t -f %s
```

### Template with backup

```yaml
- name: Update configuration with backup
  template:
    src: templates/app.conf.j2
    dest: /etc/myapp/app.conf
    backup: yes
```

### Example Template File (nginx.conf.j2)

```jinja2
# Managed by Rustible
worker_processes {{ worker_processes | default(4) }};

events {
    worker_connections {{ worker_connections | default(1024) }};
}

http {
    server {
        listen {{ http_port | default(80) }};
        server_name {{ server_name }};

        location / {
            root {{ document_root }};
        }

        {% if enable_ssl %}
        listen {{ https_port | default(443) }} ssl;
        ssl_certificate {{ ssl_cert_path }};
        ssl_certificate_key {{ ssl_key_path }};
        {% endif %}
    }
}
```

## Template Syntax

Rustible uses Jinja2-compatible syntax for templates:

| Syntax | Description |
|--------|-------------|
| `{{ variable }}` | Output a variable value |
| `{% if condition %}...{% endif %}` | Conditional blocks |
| `{% for item in list %}...{% endfor %}` | Loop over items |
| `{{ value \| filter }}` | Apply a filter to a value |
| `{# comment #}` | Template comments (not rendered) |

### Common Filters

| Filter | Description |
|--------|-------------|
| `default(value)` | Provide a default if variable is undefined |
| `upper` | Convert to uppercase |
| `lower` | Convert to lowercase |
| `trim` | Remove leading/trailing whitespace |
| `join(sep)` | Join list elements with separator |

## Notes

- Templates are rendered on the control node before being transferred
- All variables and facts are available in templates
- The module is idempotent; it will not update files if rendered content is identical
- Template files typically use the `.j2` extension by convention
- Invalid template syntax will cause the task to fail

## Real-World Use Cases

### Application Configuration

```yaml
- name: Deploy application config
  template:
    src: app.conf.j2
    dest: /etc/myapp/app.conf
    owner: myapp
    group: myapp
    mode: "0640"
  notify: Restart myapp
```

### Dynamic Nginx Virtual Host

```yaml
- name: Create Nginx virtual host
  template:
    src: vhost.conf.j2
    dest: /etc/nginx/sites-available/{{ domain }}.conf
    validate: nginx -t -c /etc/nginx/nginx.conf
  notify: Reload nginx
```

### Systemd Service File

```yaml
- name: Create systemd service
  template:
    src: myapp.service.j2
    dest: /etc/systemd/system/myapp.service
    owner: root
    group: root
    mode: "0644"
  notify:
    - Reload systemd
    - Restart myapp
```

### Environment File Generation

```yaml
- name: Generate environment file
  template:
    src: env.j2
    dest: /opt/myapp/.env
    owner: myapp
    group: myapp
    mode: "0600"
```

## Troubleshooting

### Template not found

Templates are searched in this order:
1. `templates/` directory relative to the playbook
2. `templates/` directory in the role
3. The exact path specified

```bash
# Verify template exists
ls -la templates/mytemplate.j2
ls -la roles/myrole/templates/mytemplate.j2
```

### Undefined variable error

Ensure all variables used in the template are defined:

```yaml
# Use default filter for optional variables
{{ optional_var | default('default_value') }}

# Check if variable is defined
{% if my_var is defined %}
setting = {{ my_var }}
{% endif %}
```

### Syntax error in template

Test templates locally with Jinja2:

```python
from jinja2 import Template
with open('template.j2') as f:
    t = Template(f.read())
    print(t.render(my_var='test'))
```

Common syntax issues:
- Missing `{% endif %}` or `{% endfor %}`
- Unbalanced braces `{{ }}`
- Using Python syntax instead of Jinja2

### Whitespace/newline issues

Control whitespace with `-` in template tags:

```jinja2
{# Remove newline after block #}
{% for item in items -%}
{{ item }}
{% endfor %}

{# Remove newline before block #}
{%- if condition %}
content
{% endif %}
```

### Special characters being escaped

By default, Jinja2 auto-escapes HTML. For config files, this is usually disabled, but if not:

```jinja2
{{ my_var | safe }}
```

### Validation fails

Ensure the validation command works with a temporary file:

```yaml
# The %s is replaced with temp file path
- template:
    src: nginx.conf.j2
    dest: /etc/nginx/nginx.conf
    validate: nginx -t -c %s
```

### File permissions wrong after template

Always specify mode explicitly:

```yaml
- template:
    src: script.sh.j2
    dest: /usr/local/bin/script.sh
    mode: "0755"  # Executable
```

### Template renders but application fails

Check for:
1. Trailing whitespace or newlines
2. Wrong line endings (DOS vs Unix)
3. Missing required sections

Use `diff` mode to see what changed:

```bash
rustible-playbook playbook.yml --diff
```

## See Also

- [copy](copy.md) - Copy files without templating
- [lineinfile](lineinfile.md) - Manage specific lines in files
- [blockinfile](blockinfile.md) - Manage blocks of text
- [file](file.md) - Set file permissions after templating
- [set_fact](set_fact.md) - Define variables for templates
