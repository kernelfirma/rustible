---
summary: Inventory system reference covering INI/YAML/JSON formats, dynamic scripts, pattern matching, AWS EC2 plugin, caching, and custom plugin development.
read_when: You need to configure inventory, use dynamic sources, write inventory plugins, or understand pattern matching syntax.
---

# Rustible Inventory System

The Rustible inventory system provides comprehensive host and group management with support for multiple formats, dynamic inventory sources, and a plugin architecture for cloud providers.

## Table of Contents

- [Overview](#overview)
- [Inventory Formats](#inventory-formats)
  - [INI Format](#ini-format)
  - [YAML Format](#yaml-format)
  - [JSON Format](#json-format)
  - [Dynamic Inventory Scripts](#dynamic-inventory-scripts)
- [Host Management](#host-management)
  - [Host Variables](#host-variables)
  - [Connection Parameters](#connection-parameters)
- [Group Management](#group-management)
  - [Group Variables](#group-variables)
  - [Group Children](#group-children)
  - [Variable Precedence](#variable-precedence)
- [Pattern Matching](#pattern-matching)
- [Plugin System](#plugin-system)
  - [Built-in Plugins](#built-in-plugins)
  - [AWS EC2 Plugin](#aws-ec2-plugin)
  - [Creating Custom Plugins](#creating-custom-plugins)
- [Inventory Caching](#inventory-caching)
- [API Reference](#api-reference)

## Overview

The inventory system is the foundation for managing target hosts in Rustible. It supports:

- **Multiple formats**: INI, YAML, JSON
- **Dynamic inventory**: Executable scripts that generate inventory
- **Plugin architecture**: Extensible sources (AWS EC2, etc.)
- **Caching**: Performance optimization for cloud inventories
- **Pattern matching**: Flexible host selection with unions, intersections, and regex

## Inventory Formats

### INI Format

The traditional Ansible-compatible INI format:

```ini
# Simple host list
[webservers]
web1 ansible_host=10.0.0.1 ansible_port=22
web2 ansible_host=10.0.0.2

[databases]
db1 ansible_host=10.0.0.10

# Group variables
[webservers:vars]
http_port=80
https_port=443
deploy_user=www-data

# Group children (hierarchy)
[production:children]
webservers
databases

# Production-wide variables
[production:vars]
environment=production
monitoring_enabled=true
```

**Key features:**
- `[groupname]` - Define a group
- `[groupname:vars]` - Group-level variables
- `[groupname:children]` - Create group hierarchy
- Host variables inline with `key=value` pairs

### YAML Format

Structured YAML inventory with full hierarchy support:

```yaml
all:
  children:
    production:
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
            https_port: 443
            deploy_user: www-data
        databases:
          hosts:
            db1:
              ansible_host: 10.0.0.10
              db_port: 5432
          vars:
            backup_enabled: true
      vars:
        environment: production
        monitoring_enabled: true
    staging:
      children:
        webservers_staging:
          hosts:
            staging-web1:
              ansible_host: 192.168.1.10
```

**Key features:**
- Hierarchical structure with `children`
- `hosts` section for host definitions
- `vars` section for variables at any level
- Full YAML data types support

### JSON Format

Compatible with Ansible dynamic inventory output:

```json
{
  "webservers": {
    "hosts": ["web1", "web2"],
    "vars": {
      "http_port": 80
    }
  },
  "databases": {
    "hosts": ["db1"],
    "children": ["primary", "replicas"]
  },
  "_meta": {
    "hostvars": {
      "web1": {
        "ansible_host": "10.0.0.1",
        "custom_var": "value1"
      },
      "web2": {
        "ansible_host": "10.0.0.2"
      },
      "db1": {
        "ansible_host": "10.0.0.10",
        "db_port": 5432
      }
    }
  }
}
```

**Key features:**
- `hosts` array for group membership
- `children` array for group hierarchy
- `vars` object for group variables
- `_meta.hostvars` for efficient host variable lookup

### Dynamic Inventory Scripts

Executable scripts that return JSON inventory:

```bash
#!/bin/bash
# Example: custom_inventory.sh

if [ "$1" == "--list" ]; then
    cat <<EOF
{
    "webservers": {
        "hosts": ["web1.example.com", "web2.example.com"]
    },
    "_meta": {
        "hostvars": {
            "web1.example.com": {"ansible_host": "10.0.0.1"},
            "web2.example.com": {"ansible_host": "10.0.0.2"}
        }
    }
}
EOF
elif [ "$1" == "--host" ]; then
    # Return host-specific variables
    echo "{}"
fi
```

Make the script executable:
```bash
chmod +x custom_inventory.sh
```

## Host Management

### Host Variables

Common Ansible-compatible host variables:

| Variable | Description | Example |
|----------|-------------|---------|
| `ansible_host` | IP/hostname to connect to | `10.0.0.1` |
| `ansible_port` | SSH port | `22` |
| `ansible_user` | SSH username | `admin` |
| `ansible_ssh_private_key_file` | Path to SSH key | `~/.ssh/id_rsa` |
| `ansible_connection` | Connection type | `ssh`, `local`, `docker` |
| `ansible_become` | Enable privilege escalation | `true` |
| `ansible_become_method` | Escalation method | `sudo`, `su` |
| `ansible_become_user` | Target user | `root` |
| `ansible_python_interpreter` | Python path on remote | `/usr/bin/python3` |

### Connection Parameters

```rust
use rustible::inventory::{Host, ConnectionType};

let mut host = Host::new("webserver");
host.ansible_host = Some("10.0.0.1".to_string());
host.set_port(22);
host.set_user("admin");
host.set_private_key("/path/to/key");
host.set_connection(ConnectionType::Ssh);

// Enable privilege escalation
host.enable_become();
host.set_become_method("sudo");
host.set_become_user("root");
```

## Group Management

### Group Variables

Variables can be set at any group level and are inherited by child groups and hosts:

```yaml
all:
  vars:
    # Applied to all hosts
    ntp_server: time.example.com
  children:
    production:
      vars:
        # Applied to production hosts
        log_level: warn
      children:
        webservers:
          vars:
            # Applied to webservers
            http_port: 80
```

### Group Children

Create hierarchical group structures:

```rust
use rustible::inventory::{Group, GroupBuilder};

// Using GroupBuilder
let production = GroupBuilder::new("production")
    .child("webservers")
    .child("databases")
    .var("environment", serde_yaml::Value::String("production".into()))
    .build();

// Or directly
let mut group = Group::new("production");
group.add_child("webservers");
group.add_child("databases");
```

### Variable Precedence

Variables are applied in order (later overrides earlier):

1. **all group vars** - Lowest precedence
2. **Parent group vars**
3. **Child group vars**
4. **Host vars** - Highest precedence

## Pattern Matching

Rustible supports powerful pattern matching for host selection:

| Pattern | Description | Example |
|---------|-------------|---------|
| `all` | All hosts | `all` |
| `*` | All hosts (alias) | `*` |
| `hostname` | Specific host | `web1` |
| `groupname` | All hosts in group | `webservers` |
| `pattern1:pattern2` | Union | `webservers:databases` |
| `group1:&group2` | Intersection | `webservers:&production` |
| `group1:!group2` | Exclusion | `all:!staging` |
| `~regex` | Regex match | `~web\d+` |
| `web*` | Wildcard match | `web*` |
| `web?` | Single char wildcard | `web?` |

**Examples:**

```rust
use rustible::inventory::Inventory;

let inventory = Inventory::load("inventory.yml")?;

// All webservers in production
let hosts = inventory.get_hosts_for_pattern("webservers:&production")?;

// All hosts except staging
let hosts = inventory.get_hosts_for_pattern("all:!staging")?;

// Hosts matching regex
let hosts = inventory.get_hosts_for_pattern("~web-[a-z]+-\\d+")?;

// Wildcard matching
let hosts = inventory.get_hosts_for_pattern("db-*")?;
```

## Plugin System

The inventory plugin system provides extensible inventory sources.

### Built-in Plugins

| Plugin | Description | Type |
|--------|-------------|------|
| `file` | File-based inventory (INI, YAML, JSON) | File |
| `script` | Dynamic inventory scripts | Script |
| `aws_ec2` | AWS EC2 instances | Cloud |

### AWS EC2 Plugin

The AWS EC2 plugin discovers instances from AWS:

```rust
use rustible::inventory::{
    InventoryPluginFactory,
    InventoryPluginConfig,
    InventoryCache,
    KeyedGroup,
};
use std::time::Duration;
use std::sync::Arc;

// Configure the plugin
let config = InventoryPluginConfig::new()
    // AWS region (uses AWS_REGION env var if not set)
    .with_option("region", "us-east-1")
    // Or multiple regions
    .with_option("regions", "us-east-1,us-west-2")
    // Include stopped instances
    .with_option("include_stopped", false)
    // Hostname preference
    .with_option("hostnames", serde_json::json!(["tag:Name", "private-dns-name"]))
    // Instance filters
    .with_option("filters", serde_json::json!({
        "Environment": "production",
        "Service": "web"
    }))
    // Use private IP for connection
    .with_option("use_private_ip", true)
    // Enable caching
    .with_cache_ttl(Duration::from_secs(300))
    // Add keyed groups for dynamic grouping
    .with_keyed_group(
        KeyedGroup::new("tags.Environment")
            .with_prefix("env")
            .with_separator("_")
    )
    .with_keyed_group(
        KeyedGroup::new("instance_type")
            .with_prefix("type")
    );

// Create the plugin
let plugin = InventoryPluginFactory::create("aws_ec2", config)?;

// Parse inventory
let inventory = plugin.parse().await?;
```

**AWS EC2 Plugin Options:**

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `region` | string | `AWS_REGION` env | AWS region |
| `regions` | string | - | Comma-separated regions |
| `include_stopped` | bool | `false` | Include stopped instances |
| `hostnames` | list | `["tag:Name", "private-dns-name"]` | Hostname preferences |
| `filters` | dict | `{}` | EC2 tag filters |
| `use_private_ip` | bool | `true` | Use private IP for ansible_host |

### Creating Custom Plugins

Implement the `InventoryPlugin` trait:

```rust
use async_trait::async_trait;
use rustible::inventory::{
    Inventory,
    InventoryPlugin,
    InventoryPluginConfig,
    InventoryResult,
    PluginOptionInfo,
};

#[derive(Debug)]
pub struct MyCloudPlugin {
    config: InventoryPluginConfig,
}

impl MyCloudPlugin {
    pub fn new(config: InventoryPluginConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl InventoryPlugin for MyCloudPlugin {
    fn name(&self) -> &str {
        "my_cloud"
    }

    fn description(&self) -> &str {
        "My custom cloud inventory plugin"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn verify(&self) -> InventoryResult<()> {
        // Validate configuration
        if self.config.get_string("api_key").is_none() {
            return Err(rustible::inventory::InventoryError::DynamicInventoryFailed(
                "Missing api_key configuration".to_string()
            ));
        }
        Ok(())
    }

    async fn parse(&self) -> InventoryResult<Inventory> {
        let mut inventory = Inventory::new();

        // Query your cloud API
        // let instances = my_cloud_api.list_instances().await?;

        // Create hosts from instances
        // for instance in instances {
        //     let mut host = Host::new(&instance.name);
        //     host.ansible_host = Some(instance.ip);
        //     inventory.add_host(host)?;
        // }

        Ok(inventory)
    }

    fn supported_options(&self) -> Vec<PluginOptionInfo> {
        vec![
            PluginOptionInfo::required_string("api_key", "API key for authentication"),
            PluginOptionInfo::optional_string("endpoint", "API endpoint URL", "https://api.mycloud.com"),
            PluginOptionInfo::optional_bool("include_terminated", "Include terminated instances", false),
        ]
    }
}

// Register the plugin
use rustible::inventory::InventoryPluginRegistry;

let mut registry = InventoryPluginRegistry::with_builtins();
registry.register("my_cloud", |config| {
    Ok(Arc::new(MyCloudPlugin::new(config)) as Arc<dyn InventoryPlugin>)
});

// Use the plugin
let config = InventoryPluginConfig::new()
    .with_option("api_key", "secret123");
let plugin = registry.create("my_cloud", config)?;
```

## Inventory Caching

The caching system improves performance for cloud inventories:

```rust
use rustible::inventory::{
    InventoryCache,
    InventoryPluginFactory,
    InventoryPluginConfig,
    CachedInventoryPlugin,
};
use std::time::Duration;
use std::sync::Arc;
use std::path::PathBuf;

// Create a cache with 5-minute TTL
let cache = Arc::new(InventoryCache::new(Duration::from_secs(300)));

// Or with persistent storage
let cache = Arc::new(InventoryCache::with_persistence(
    Duration::from_secs(300),
    PathBuf::from("/var/cache/rustible/inventory"),
));

// Create a plugin
let config = InventoryPluginConfig::new()
    .with_option("region", "us-east-1");
let plugin = InventoryPluginFactory::create("aws_ec2", config)?;

// Wrap with caching
let cached_plugin = CachedInventoryPlugin::new(
    plugin,
    cache.clone(),
    "aws_ec2_us_east_1",  // Cache key
    Duration::from_secs(300),
);

// Parse (cached after first call)
let inventory = cached_plugin.parse().await?;

// Force refresh
cached_plugin.refresh().await?;

// Get cache statistics
let stats = cache.stats().await;
println!("Cache entries: {}, Valid: {}, Expired: {}",
    stats.total_entries,
    stats.valid_entries,
    stats.expired_entries
);

// Cleanup expired entries
cache.cleanup().await;

// Clear all cache
cache.clear().await;
```

## API Reference

### Inventory

```rust
impl Inventory {
    /// Create a new empty inventory
    pub fn new() -> Self;

    /// Load from file or directory
    pub fn load<P: AsRef<Path>>(path: P) -> InventoryResult<Self>;

    /// Add a host
    pub fn add_host(&mut self, host: Host) -> InventoryResult<()>;

    /// Add a group
    pub fn add_group(&mut self, group: Group) -> InventoryResult<()>;

    /// Get host by name
    pub fn get_host(&self, name: &str) -> Option<&Host>;

    /// Get mutable host
    pub fn get_host_mut(&mut self, name: &str) -> Option<&mut Host>;

    /// Get group by name
    pub fn get_group(&self, name: &str) -> Option<&Group>;

    /// Get hosts matching pattern
    pub fn get_hosts_for_pattern(&self, pattern: &str) -> InventoryResult<Vec<&Host>>;

    /// Get merged variables for a host
    pub fn get_host_vars(&self, host: &Host) -> IndexMap<String, serde_yaml::Value>;

    /// Count hosts
    pub fn host_count(&self) -> usize;

    /// Count groups
    pub fn group_count(&self) -> usize;
}
```

### Host

```rust
impl Host {
    /// Create a new host
    pub fn new(name: impl Into<String>) -> Self;

    /// Create with address
    pub fn with_address(name: impl Into<String>, address: impl Into<String>) -> Self;

    /// Get the connection address
    pub fn address(&self) -> &str;

    /// Set a variable
    pub fn set_var(&mut self, key: impl Into<String>, value: serde_yaml::Value);

    /// Get a variable
    pub fn get_var(&self, key: &str) -> Option<&serde_yaml::Value>;

    /// Add to a group
    pub fn add_to_group(&mut self, group: impl Into<String>);

    /// Check group membership
    pub fn in_group(&self, group: &str) -> bool;
}
```

### Group

```rust
impl Group {
    /// Create a new group
    pub fn new(name: impl Into<String>) -> Self;

    /// Add a host
    pub fn add_host(&mut self, host: impl Into<String>);

    /// Add a child group
    pub fn add_child(&mut self, child: impl Into<String>);

    /// Set a variable
    pub fn set_var(&mut self, key: impl Into<String>, value: serde_yaml::Value);

    /// Check if host belongs to group
    pub fn has_host(&self, host: &str) -> bool;

    /// Check if group is a child
    pub fn has_child(&self, child: &str) -> bool;
}
```

### InventoryPluginFactory

```rust
impl InventoryPluginFactory {
    /// Create a plugin by name
    pub fn create(
        name: &str,
        config: InventoryPluginConfig,
    ) -> PluginResult<Arc<dyn InventoryPlugin>>;

    /// Get available plugin names
    pub fn available_plugin_names() -> Vec<&'static str>;

    /// Get plugin information
    pub fn available_plugins() -> Vec<PluginInfo>;

    /// Check if plugin exists
    pub fn plugin_exists(name: &str) -> bool;
}
```

### InventoryCache

```rust
impl InventoryCache {
    /// Create with TTL
    pub fn new(default_ttl: Duration) -> Self;

    /// Create with persistent storage
    pub fn with_persistence(default_ttl: Duration, cache_dir: PathBuf) -> Self;

    /// Get from cache
    pub async fn get(&self, key: &str) -> Option<Inventory>;

    /// Store in cache
    pub async fn set(&self, key: &str, inventory: Inventory, ttl: Option<Duration>);

    /// Get or compute
    pub async fn get_or_compute<F, Fut>(
        &self,
        key: &str,
        compute: F,
        ttl: Option<Duration>,
    ) -> InventoryResult<Inventory>;

    /// Invalidate entry
    pub async fn invalidate(&self, key: &str);

    /// Clear all
    pub async fn clear(&self);

    /// Get statistics
    pub async fn stats(&self) -> CacheStats;
}
```
