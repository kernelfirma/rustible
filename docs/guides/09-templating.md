---
summary: Jinja2 templating with MiniJinja including variable interpolation, filters, tests, control structures, and common patterns.
read_when: You need to render dynamic configuration files, use filters or conditionals in templates, or understand Rustible's Jinja2 support.
---

# Chapter 9: Templating - Jinja2 Syntax

Rustible uses **MiniJinja** as its template engine, providing Jinja2-compatible syntax for rendering dynamic content. Templates are used in configuration files (via the `template` module), inline variable expressions, conditionals (`when`), and many other places throughout playbooks.

## Template Syntax Basics

Jinja2 uses three types of delimiters:

| Delimiter | Purpose | Example |
|-----------|---------|---------|
| `{{ ... }}` | Variable output | `{{ hostname }}` |
| `{% ... %}` | Control structures | `{% if ssl_enabled %}` |
| `{# ... #}` | Comments (not rendered) | `{# This is a comment #}` |

### Performance Note

Rustible includes a fast-path optimization: if a string contains no template syntax (`{{` or `{%`), rendering is bypassed entirely. Plain strings pass through at zero cost.

## Variable Interpolation

### Basic Access

```jinja
Server name: {{ server_name }}
Port: {{ http_port }}
```

### Attribute and Key Access

```jinja
{# Dot notation #}
Database host: {{ database.host }}
Database port: {{ database.port }}

{# Bracket notation #}
Database host: {{ database['host'] }}

{# Nested access #}
Primary DNS: {{ network.dns.primary }}
```

### Undefined Variables

By default, Rustible uses chainable undefined behavior -- accessing an attribute on an undefined variable returns another undefined value rather than raising an error. Use the `default` filter or `defined` test to handle missing values explicitly.

## Filters

Filters transform variable values. They are applied with the pipe (`|`) operator.

### String Filters

| Filter | Description | Example |
|--------|-------------|---------|
| `upper` | Convert to uppercase | `{{ name \| upper }}` |
| `lower` | Convert to lowercase | `{{ name \| lower }}` |
| `capitalize` | Capitalize first letter | `{{ name \| capitalize }}` |
| `title` | Title case | `{{ name \| title }}` |
| `trim` | Strip whitespace | `{{ name \| trim }}` |
| `replace` | Replace substring | `{{ name \| replace("old", "new") }}` |
| `regex_replace` | Regex replacement | `{{ name \| regex_replace("\\d+", "N") }}` |
| `regex_search` | Regex search | `{{ name \| regex_search("v(\\d+)") }}` |
| `split` | Split into list | `{{ csv \| split(",") }}` |
| `join` | Join list to string | `{{ items \| join(", ") }}` |

### Type Conversion Filters

| Filter | Description | Example |
|--------|-------------|---------|
| `int` | Convert to integer | `{{ port_str \| int }}` |
| `float` | Convert to float | `{{ value \| float }}` |
| `string` | Convert to string | `{{ number \| string }}` |
| `bool` | Convert to boolean | `{{ flag \| bool }}` |
| `list` | Convert to list | `{{ value \| list }}` |

### Collection Filters

| Filter | Description | Example |
|--------|-------------|---------|
| `first` | First element | `{{ items \| first }}` |
| `last` | Last element | `{{ items \| last }}` |
| `length` | Length / count | `{{ items \| length }}` |
| `unique` | Remove duplicates | `{{ items \| unique }}` |
| `sort` | Sort elements | `{{ items \| sort }}` |
| `reverse` | Reverse order | `{{ items \| reverse }}` |
| `flatten` | Flatten nested lists | `{{ nested \| flatten }}` |

### Path Filters

| Filter | Description | Example |
|--------|-------------|---------|
| `basename` | File name from path | `{{ path \| basename }}` |
| `dirname` | Directory from path | `{{ path \| dirname }}` |
| `expanduser` | Expand `~` in path | `{{ path \| expanduser }}` |
| `realpath` | Resolve to absolute path | `{{ path \| realpath }}` |

### Encoding Filters

| Filter | Description | Example |
|--------|-------------|---------|
| `b64encode` | Base64 encode | `{{ secret \| b64encode }}` |
| `b64decode` | Base64 decode | `{{ encoded \| b64decode }}` |
| `to_json` | Convert to JSON | `{{ data \| to_json }}` |
| `to_nice_json` | Pretty JSON | `{{ data \| to_nice_json }}` |
| `from_json` | Parse JSON string | `{{ json_str \| from_json }}` |
| `to_yaml` | Convert to YAML | `{{ data \| to_yaml }}` |
| `to_nice_yaml` | Pretty YAML | `{{ data \| to_nice_yaml }}` |
| `from_yaml` | Parse YAML string | `{{ yaml_str \| from_yaml }}` |

### Ansible-Specific Filters

| Filter | Description | Example |
|--------|-------------|---------|
| `default` (or `d`) | Fallback value | `{{ var \| default("none") }}` |
| `mandatory` | Fail if undefined | `{{ var \| mandatory }}` |
| `ternary` | Conditional value | `{{ flag \| ternary("yes", "no") }}` |
| `combine` | Merge dictionaries | `{{ dict1 \| combine(dict2) }}` |
| `dict2items` | Dict to list of pairs | `{{ mydict \| dict2items }}` |
| `items2dict` | List of pairs to dict | `{{ mylist \| items2dict }}` |
| `selectattr` | Filter by attribute | `{{ users \| selectattr("active") }}` |
| `rejectattr` | Reject by attribute | `{{ users \| rejectattr("disabled") }}` |
| `map` | Map attribute | `{{ users \| map(attribute="name") }}` |

### Chaining Filters

Filters can be chained:

```jinja
{{ raw_input | trim | lower | replace(" ", "-") }}
{{ packages | unique | sort | join(", ") }}
```

## Tests

Tests check conditions about a value. They are used with `is` in `when` clauses and `{% if %}` blocks.

### Type Tests

| Test | Description |
|------|-------------|
| `defined` | Variable is defined |
| `undefined` | Variable is not defined |
| `none` (or `null`) | Value is None/null |
| `boolean` | Value is a boolean |
| `integer` | Value is an integer |
| `float` | Value is a float |
| `number` | Value is numeric |
| `string` | Value is a string |
| `mapping` (or `dict`) | Value is a dictionary |
| `iterable` | Value is iterable |
| `sequence` (or `list`) | Value is a list |

### Truthiness Tests

| Test | Description |
|------|-------------|
| `truthy` | Value evaluates as true |
| `falsy` | Value evaluates as false |

### String Tests

| Test | Description |
|------|-------------|
| `match` | Regex match from start |
| `search` | Regex search anywhere |
| `startswith` | Starts with substring |
| `endswith` | Ends with substring |

### Numeric Tests

| Test | Description |
|------|-------------|
| `odd` | Value is odd |
| `even` | Value is even |
| `divisibleby` | Divisible by N |

### Collection Tests

| Test | Description |
|------|-------------|
| `in` | Value is in collection |
| `contains` | Collection contains value |
| `subset` | Is a subset of |
| `superset` | Is a superset of |

### Task Result Tests

| Test | Description |
|------|-------------|
| `success` | Task succeeded |
| `failed` | Task failed |
| `changed` | Task made changes |
| `skipped` | Task was skipped |

### File Tests

| Test | Description |
|------|-------------|
| `file` | Path is a file |
| `directory` | Path is a directory |
| `link` | Path is a symlink |
| `exists` | Path exists |
| `abs` | Path is absolute |

### Usage Examples

```yaml
when: my_var is defined
when: my_var is not none
when: result is changed
when: port_number is integer
when: users is sequence
when: version is match("^2\\.")
```

## Control Structures

### Conditionals

```jinja
{% if ssl_enabled %}
listen 443 ssl;
ssl_certificate {{ ssl_cert_path }};
ssl_certificate_key {{ ssl_key_path }};
{% elif http2_enabled %}
listen 80 http2;
{% else %}
listen 80;
{% endif %}
```

### For Loops

```jinja
{% for server in upstream_servers %}
server {{ server.host }}:{{ server.port }} weight={{ server.weight | default(1) }};
{% endfor %}
```

### Loop Variables

Inside a `for` loop, these variables are available:

| Variable | Description |
|----------|-------------|
| `loop.index` | Current iteration (1-indexed) |
| `loop.index0` | Current iteration (0-indexed) |
| `loop.first` | `true` on first iteration |
| `loop.last` | `true` on last iteration |
| `loop.length` | Total number of items |
| `loop.revindex` | Iterations remaining (1-indexed) |

```jinja
{% for user in users %}
{{ loop.index }}. {{ user.name }}{% if not loop.last %},{% endif %}
{% endfor %}
```

## Whitespace Control

By default, Jinja2 preserves whitespace around blocks. Use the `-` modifier to strip whitespace:

```jinja
{#- Strip before -#}
{%- if condition -%}
  trimmed output
{%- endif -%}
{{- variable -}}
```

- `{%-` strips whitespace before the tag
- `-%}` strips whitespace after the tag
- `{{-` and `-}}` work the same way for expressions

## Using Templates in Playbooks

The `template` module renders a Jinja2 file and deploys it to the target host:

```yaml
- name: Deploy nginx configuration
  template:
    src: nginx.conf.j2
    dest: /etc/nginx/nginx.conf
    owner: root
    group: root
    mode: '0644'
  notify: Restart nginx
```

Template files are typically stored in `templates/` within a role or relative to the playbook.

## Common Template Patterns

### Nginx Configuration

```jinja
{# templates/nginx.conf.j2 #}
worker_processes {{ worker_processes | default('auto') }};

events {
    worker_connections {{ worker_connections | default(1024) }};
}

http {
    {% for vhost in virtual_hosts %}
    server {
        listen {{ vhost.port | default(80) }};
        server_name {{ vhost.server_name }};
        root {{ vhost.document_root }};

        {% if vhost.ssl | default(false) %}
        ssl_certificate {{ vhost.ssl_cert }};
        ssl_certificate_key {{ vhost.ssl_key }};
        {% endif %}
    }
    {% endfor %}
}
```

### Systemd Unit

```jinja
{# templates/myapp.service.j2 #}
[Unit]
Description={{ app_name }} Service
After=network.target
{% if app_requires is defined %}
Requires={{ app_requires | join(' ') }}
{% endif %}

[Service]
Type=simple
User={{ app_user | default('nobody') }}
WorkingDirectory={{ app_dir }}
ExecStart={{ app_dir }}/bin/{{ app_name }} {{ app_args | default('') }}
Restart=on-failure
RestartSec=5
{% for key, value in app_env | default({}) | dict2items %}
Environment={{ key }}={{ value }}
{% endfor %}

[Install]
WantedBy=multi-user.target
```

### Environment File

```jinja
{# templates/env.j2 #}
# Generated by Rustible - do not edit manually
{% for key, value in env_vars | dict2items | sort(attribute='key') %}
{{ key }}={{ value }}
{% endfor %}
```

## Template Caching

Rustible caches compiled templates in an LRU cache (default size: 1000 entries) to avoid reparsing the same template. The cache size can be configured via the `RUSTIBLE_TEMPLATE_CACHE_SIZE` environment variable:

```bash
# Increase cache for large deployments
export RUSTIBLE_TEMPLATE_CACHE_SIZE=5000

# Disable caching (for debugging)
export RUSTIBLE_TEMPLATE_CACHE_SIZE=0
```

## Differences from Ansible Jinja2

While Rustible aims for full Jinja2 compatibility through MiniJinja, there are some differences to be aware of:

| Feature | Ansible (Python Jinja2) | Rustible (MiniJinja) |
|---------|------------------------|---------------------|
| Engine | Python Jinja2 | MiniJinja (Rust) |
| Performance | Interpreted | Compiled, with LRU cache |
| Undefined behavior | Strict by default | Chainable (lenient) |
| Python expressions | Full Python in templates | Jinja2 expressions only |
| Custom plugins | Python lookup plugins | Rust-native filters/tests |

The most notable difference is that Ansible allows arbitrary Python expressions in templates (e.g., calling Python built-ins). Rustible restricts templates to standard Jinja2 syntax, which is safer and more portable.

## Best Practices

1. **Use the `default` filter** liberally. Every variable reference in a template should have a sensible fallback: `{{ var | default("safe_value") }}`.
2. **Add a "managed by" comment** at the top of generated files so operators know not to edit them by hand.
3. **Keep templates simple**. Complex logic belongs in tasks or roles, not in Jinja2 templates.
4. **Validate rendered output**. Use `--check --diff` to preview template changes before applying them.
5. **Use whitespace control** (`{%-` / `-%}`) to keep generated files clean and readable.
6. **Test templates independently** by rendering them with known variable sets before deploying.

## Next Steps

- Review [Built-in Modules](05-modules.md) for full module documentation
- Learn about [Roles](06-roles.md) for organizing templates within roles
- See [Variables](04-variables.md) for how template variables are resolved
