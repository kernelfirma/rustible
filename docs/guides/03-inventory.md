---
summary: Managing hosts, groups, and patterns in Rustible inventory files across YAML, INI, JSON, and dynamic sources.
read_when: You need to define which hosts and groups Rustible manages, or want to use dynamic inventory sources.
---

# Chapter 3: Inventory - Hosts, Groups, and Patterns

The inventory is where you define the hosts Rustible manages and how they are organized into groups. Rustible supports multiple inventory formats and dynamic sources, giving you flexibility in how you describe your infrastructure.

## Inventory Formats

### YAML Format (Recommended)

YAML is the most common inventory format. The top-level key is always `all`, which implicitly contains every host.

```yaml
all:
  children:
    webservers:
      hosts:
        web1:
          ansible_host: 10.0.0.1
          ansible_port: 22
        web2:
          ansible_host: 10.0.0.2
      vars:
        http_port: 80
        document_root: /var/www/html

    databases:
      hosts:
        db1:
          ansible_host: 10.0.1.1
        db2:
          ansible_host: 10.0.1.2
      vars:
        db_port: 5432

    production:
      children:
        webservers:
        databases:
      vars:
        env: production
```

Key concepts:

- **`all`** is the root group containing every host.
- **`children`** defines child groups within a parent group.
- **`hosts`** lists individual hosts with optional per-host variables.
- **`vars`** sets variables shared by all hosts in the group.

### INI Format

The INI format uses section headers for groups:

```ini
[webservers]
web1 ansible_host=10.0.0.1 ansible_port=22
web2 ansible_host=10.0.0.2

[webservers:vars]
http_port=80
document_root=/var/www/html

[databases]
db1 ansible_host=10.0.1.1
db2 ansible_host=10.0.1.2

[databases:vars]
db_port=5432

[production:children]
webservers
databases

[production:vars]
env=production
```

- `[group]` headers define groups with host entries below them.
- `[group:vars]` sections define variables for a group.
- `[group:children]` sections define child groups.

### JSON Format

JSON inventory follows the same structure used by dynamic inventory scripts:

```json
{
  "webservers": {
    "hosts": ["web1", "web2"],
    "vars": {
      "http_port": 80
    }
  },
  "databases": {
    "hosts": ["db1", "db2"],
    "vars": {
      "db_port": 5432
    }
  },
  "_meta": {
    "hostvars": {
      "web1": { "ansible_host": "10.0.0.1" },
      "web2": { "ansible_host": "10.0.0.2" },
      "db1": { "ansible_host": "10.0.1.1" },
      "db2": { "ansible_host": "10.0.1.2" }
    }
  }
}
```

The `_meta.hostvars` key provides per-host variables in a single lookup, avoiding a separate call per host in dynamic inventory scripts.

### Dynamic Inventory Scripts

Any executable script that outputs JSON in the format above can serve as a dynamic inventory source. Rustible detects this automatically when the inventory path points to an executable file.

```bash
#!/usr/bin/env python3
import json
inventory = {
    "webservers": {"hosts": ["web1", "web2"]},
    "_meta": {
        "hostvars": {
            "web1": {"ansible_host": "10.0.0.1"},
            "web2": {"ansible_host": "10.0.0.2"}
        }
    }
}
print(json.dumps(inventory))
```

```bash
chmod +x inventory.py
rustible run playbook.yml -i ./inventory.py
```

## Host Variables

Host variables control how Rustible connects to and interacts with each host.

| Variable | Description | Default |
|----------|-------------|---------|
| `ansible_host` | IP or hostname to connect to | Inventory hostname |
| `ansible_port` | SSH port | `22` |
| `ansible_user` | SSH username | Current user |
| `ansible_connection` | Connection type (`ssh`, `local`, `docker`) | `ssh` |
| `ansible_ssh_private_key_file` | Path to SSH private key | `~/.ssh/id_rsa` |
| `ansible_become` | Enable privilege escalation | `false` |
| `ansible_become_user` | User to become | `root` |
| `ansible_become_method` | Escalation method (`sudo`, `su`) | `sudo` |

## Group Variables

Variables defined at the group level apply to all hosts in that group:

```yaml
all:
  children:
    webservers:
      vars:
        http_port: 80
        ssl_enabled: true
      hosts:
        web1:
        web2:
```

When a host belongs to multiple groups, variables are merged. More specific groups take precedence over less specific ones, and child groups override parent groups.

## Host and Group Variable Files

For larger inventories, store variables in separate files organized by directory:

```
inventory/
  hosts.yml
  group_vars/
    all.yml          # Variables for all hosts
    webservers.yml   # Variables for webservers group
    production.yml   # Variables for production group
  host_vars/
    web1.yml         # Variables for web1
    db1.yml          # Variables for db1
```

Rustible automatically loads variables from `group_vars/` and `host_vars/` directories adjacent to your inventory file. You can also place these directories at the playbook level, and both sources will be merged.

## Inventory Patterns

When specifying hosts in a play, you can use patterns to select subsets of your inventory.

| Pattern | Meaning | Example |
|---------|---------|---------|
| `all` | All hosts | `hosts: all` |
| `groupname` | All hosts in group | `hosts: webservers` |
| `host1:host2` | Union of hosts/groups | `hosts: webservers:databases` |
| `group1:&group2` | Intersection | `hosts: production:&webservers` |
| `group1:!group2` | Exclusion | `hosts: all:!databases` |
| `web*` | Wildcard match | `hosts: web*` |
| `~regex` | Regex match on hostname | `hosts: ~web[0-9]+` |

Patterns can be combined for precise targeting:

```yaml
# All production webservers except web3
- hosts: production:&webservers:!web3
  tasks:
    - debug:
        msg: "Deploying to {{ inventory_hostname }}"
```

## Inventory Plugins

Rustible supports inventory plugins for cloud providers and infrastructure tools. These are available via feature flags at compile time.

| Plugin | Feature Flag | Description |
|--------|-------------|-------------|
| `aws_ec2` | `cloud-aws` | AWS EC2 instance discovery |
| `azure` | `cloud-azure` | Azure VM discovery |
| `gcp` | `cloud-gcp` | Google Cloud instance discovery |
| `terraform` | (built-in) | Read hosts from Terraform state |

Plugins are configured with an inventory plugin configuration file:

```yaml
# aws_ec2_inventory.yml
plugin: aws_ec2
region: us-east-1
filters:
  tag:Environment: production
keyed_groups:
  - key: tags.Role
    prefix: role
```

```bash
rustible run playbook.yml -i aws_ec2_inventory.yml
```

The plugin system is extensible. Custom plugins can be created by implementing the `InventoryPlugin` trait and registering them with the `InventoryPluginFactory`.

## Multiple Inventory Sources

You can specify multiple inventory sources by passing the `-i` flag more than once:

```bash
rustible run playbook.yml \
  -i inventory/production.yml \
  -i inventory/staging.yml \
  -i ./dynamic_inventory.py
```

Hosts and groups from all sources are merged into a single inventory. If the same host appears in multiple sources, variables are merged with later sources taking precedence.

## Inventory Caching

For dynamic inventory sources that query external APIs (cloud providers, CMDBs), Rustible provides an inventory cache to avoid repeated expensive lookups.

```yaml
# Plugin configuration with caching
plugin: aws_ec2
region: us-east-1
cache: true
cache_timeout: 300  # seconds
```

The cache stores inventory results locally and reuses them until the TTL expires. Cache statistics (hits, misses, evictions) are available through the `InventoryCache` API for monitoring.

To force a cache refresh:

```bash
rustible run playbook.yml -i aws_ec2_inventory.yml --flush-cache
```

## Best Practices

1. **Use YAML format** for static inventories -- it is the most readable and well-supported.
2. **Organize variables** into `group_vars/` and `host_vars/` directories instead of inlining them in the inventory file.
3. **Use groups for roles**, not just environments. Group by function (webservers, databases) and by environment (production, staging), then combine with patterns.
4. **Keep dynamic inventories fast**. Cache results when querying cloud APIs. Scripts should respond in under a few seconds.
5. **Validate with `--list-hosts`** before running playbooks to confirm pattern matching selects the expected hosts.

## Next Steps

- Learn about [Variables and Precedence](04-variables.md)
- Explore [Available Modules](05-modules.md)
- Understand [Execution Strategies](07-execution-strategies.md)
