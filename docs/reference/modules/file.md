---
summary: Reference for the file module that manages file/directory properties including state, permissions, ownership, and symbolic links.
read_when: You need to create directories, set permissions, manage symlinks, or delete files on remote hosts.
---

# file - Manage File and Directory Properties

## Synopsis

The `file` module manages file and directory properties including state, permissions, ownership, and symbolic links. It can create, delete, and modify files and directories.

## Classification

**NativeTransport** - This module uses native Rust SSH/SFTP operations.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `path` | yes | - | string | Path to the file or directory to manage. |
| `state` | no | file | string | Desired state: file, directory, link, hard, touch, absent. |
| `owner` | no | - | string | Name of the user that should own the file/directory. |
| `group` | no | - | string | Name of the group that should own the file/directory. |
| `mode` | no | - | string | Permissions of the file/directory. |
| `src` | no | - | string | Path to the file to link to (for state=link or state=hard). |
| `force` | no | false | boolean | Force creation of symlinks even if source does not exist. |
| `recurse` | no | false | boolean | Apply owner, group, mode recursively to directories. |

## State Values

| State | Description |
|-------|-------------|
| `file` | Ensure file exists and has specified properties |
| `directory` | Ensure directory exists and has specified properties |
| `link` | Create a symbolic link |
| `hard` | Create a hard link |
| `touch` | Create empty file if not exists, update mtime if exists |
| `absent` | Remove the file or directory |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `path` | string | Path that was managed |
| `state` | string | State of the file/directory |
| `mode` | string | Permissions of the file/directory |
| `owner` | string | Owner of the file/directory |
| `group` | string | Group of the file/directory |
| `size` | integer | Size of the file in bytes |

## Examples

### Create a directory with specific permissions

```yaml
- name: Ensure app directory exists
  file:
    path: /var/lib/myapp
    state: directory
    owner: appuser
    group: appgroup
    mode: "0755"
```

### Create a symbolic link

```yaml
- name: Create a symlink
  file:
    path: /usr/local/bin/myapp
    src: /opt/myapp/bin/myapp
    state: link
```

### Remove a file

```yaml
- name: Remove temporary file
  file:
    path: /tmp/myapp.tmp
    state: absent
```

### Set file permissions

```yaml
- name: Set permissions on a file
  file:
    path: /etc/myapp/secret.conf
    mode: "0600"
    owner: root
    group: root
```

### Create an empty file (touch)

```yaml
- name: Touch a file to update timestamp
  file:
    path: /var/log/myapp/last_run
    state: touch
```

### Recursively set permissions on a directory

```yaml
- name: Set permissions recursively
  file:
    path: /var/www/html
    owner: www-data
    group: www-data
    mode: "0755"
    recurse: yes
```

## Notes

- When `state=file`, the file must already exist; it will not create a new file
- Use `state=touch` to create an empty file if it does not exist
- The `recurse` option only works with `state=directory`
- Hard links cannot span filesystems
- Symbolic links can point to non-existent targets when `force=yes`

## Real-World Use Cases

### Application Directory Structure

```yaml
- name: Create application directories
  file:
    path: "{{ item }}"
    state: directory
    owner: appuser
    group: appgroup
    mode: "0755"
  loop:
    - /opt/myapp
    - /opt/myapp/bin
    - /opt/myapp/config
    - /opt/myapp/logs
    - /opt/myapp/data

- name: Create log directory with sticky bit
  file:
    path: /opt/myapp/logs
    state: directory
    mode: "1775"
```

### Secure File Permissions

```yaml
- name: Secure SSH directory
  file:
    path: /home/{{ user }}/.ssh
    state: directory
    owner: "{{ user }}"
    group: "{{ user }}"
    mode: "0700"

- name: Secure private key
  file:
    path: /home/{{ user }}/.ssh/id_rsa
    owner: "{{ user }}"
    group: "{{ user }}"
    mode: "0600"
```

### Symbolic Link Management

```yaml
- name: Create current version symlink
  file:
    path: /opt/myapp/current
    src: /opt/myapp/releases/{{ version }}
    state: link

- name: Update alternatives
  file:
    path: /usr/local/bin/python
    src: /usr/bin/python3.11
    state: link
```

### Cleanup Operations

```yaml
- name: Remove old release directories
  file:
    path: "/opt/myapp/releases/{{ item }}"
    state: absent
  loop: "{{ old_releases }}"

- name: Remove temporary files
  file:
    path: /tmp/myapp_cache
    state: absent
```

## Troubleshooting

### Cannot create directory - Permission denied

Ensure you have appropriate permissions:

```yaml
- name: Create system directory
  file:
    path: /etc/myapp
    state: directory
  become: yes
```

### Symlink points to wrong target

Check that you have `state: link` and `src` is the target:

```yaml
# CORRECT: src is what the link points TO
- file:
    path: /usr/local/bin/myapp       # The symlink
    src: /opt/myapp/bin/myapp        # The target
    state: link
```

### Mode not being set correctly

Use quotes and leading zero for octal modes:

```yaml
# CORRECT
- file:
    path: /path/to/file
    mode: "0644"

# INCORRECT - may be interpreted as decimal
- file:
    path: /path/to/file
    mode: 644
```

### Owner/group changes not taking effect

The user and group must exist on the target system:

```yaml
- name: Create user first
  user:
    name: appuser

- name: Then set ownership
  file:
    path: /opt/myapp
    owner: appuser
    group: appuser
```

### Recurse not working as expected

The `recurse` option only works with `state: directory`:

```yaml
- name: Set permissions recursively
  file:
    path: /var/www/html
    owner: www-data
    group: www-data
    mode: "0755"
    recurse: yes
    state: directory  # Required for recurse
```

### Hard link fails - Invalid cross-device link

Hard links cannot span filesystems. Use symbolic links instead or ensure both paths are on the same filesystem.

### Symlink to non-existent target fails

Use `force: yes` to create symlinks to targets that do not exist yet:

```yaml
- name: Create symlink before target exists
  file:
    path: /opt/myapp/current
    src: /opt/myapp/releases/pending
    state: link
    force: yes
```

### SELinux context issues

On SELinux-enabled systems, file context may need to be set:

```bash
# Check SELinux context
ls -lZ /path/to/file

# Restore default context
restorecon -v /path/to/file
```

## See Also

- [copy](copy.md) - Copy files to remote locations
- [template](template.md) - Template files with variable substitution
- [stat](stat.md) - Retrieve file information
- [lineinfile](lineinfile.md) - Manage individual lines in files
- [user](user.md) - Create users for file ownership
- [group](group.md) - Create groups for file ownership
