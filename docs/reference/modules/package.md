---
summary: Reference for the package module that auto-detects and uses apt, yum, or dnf based on the target system.
read_when: You need to install, remove, or update packages across different Linux distributions.
---

# package - Generic Package Manager

## Synopsis

The `package` module is a generic abstraction that automatically selects the appropriate package manager based on the target system. It supports apt (Debian/Ubuntu), yum (RHEL/CentOS 7), and dnf (Fedora/RHEL 8+).

## Classification

**RemoteCommand** - This module executes package management commands on remote hosts via SSH.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string/list | Package name(s) to manage. Can be a single package or a list. |
| `state` | no | present | string | Desired state: present, absent, latest. |
| `use` | no | auto | string | Force a specific package manager: apt, yum, dnf, auto. |
| `update_cache` | no | false | boolean | Update package cache before installing. |

## State Values

| State | Description |
|-------|-------------|
| `present` | Ensure the package is installed |
| `absent` | Ensure the package is removed |
| `latest` | Ensure the package is at the latest version |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `name` | string/list | Package name(s) managed |
| `state` | string | Desired state |
| `changed` | boolean | Whether changes were made |
| `msg` | string | Summary of actions taken |

## Examples

### Install a single package

```yaml
- name: Install nginx
  package:
    name: nginx
    state: present
```

### Install multiple packages

```yaml
- name: Install web server packages
  package:
    name:
      - nginx
      - php-fpm
      - mariadb-server
    state: present
```

### Remove a package

```yaml
- name: Remove unused package
  package:
    name: telnet
    state: absent
```

### Upgrade a package to latest version

```yaml
- name: Upgrade nginx to latest
  package:
    name: nginx
    state: latest
```

### Update cache before installing

```yaml
- name: Install package with cache update
  package:
    name: htop
    state: present
    update_cache: yes
```

### Force a specific package manager

```yaml
- name: Use apt explicitly
  package:
    name: nginx
    state: present
    use: apt
```

## Notes

- The module automatically detects the appropriate package manager
- Package names may differ between distributions (e.g., `httpd` vs `apache2`)
- The `update_cache` option is translated to the appropriate command for each package manager
- This module provides host-exclusive parallelization to prevent package manager lock conflicts

## See Also

- [apt](apt.md) - Debian/Ubuntu package management
- [yum](yum.md) - RHEL/CentOS package management
- [dnf](dnf.md) - Fedora/RHEL 8+ package management
- [pip](pip.md) - Python package management
