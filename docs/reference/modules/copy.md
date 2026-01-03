---
summary: Reference for the copy module that copies files or inline content to remote hosts with ownership and permission control.
read_when: You need to copy files to remote hosts or write content directly to remote files.
---

# copy - Copy Files to Remote Locations

## Synopsis

The `copy` module copies files from the local machine to remote locations. It can also copy content directly to a remote file.

## Classification

**NativeTransport** - This module uses native Rust SSH/SFTP operations for file transfer.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `src` | yes* | - | string | Local path to file to copy. Mutually exclusive with `content`. |
| `content` | yes* | - | string | Content to write directly to the destination file. |
| `dest` | yes | - | string | Remote absolute path where the file should be copied. |
| `owner` | no | - | string | Name of the user that should own the file. |
| `group` | no | - | string | Name of the group that should own the file. |
| `mode` | no | - | string | Permissions of the file (e.g., "0644" or "u=rw,g=r,o=r"). |
| `backup` | no | false | boolean | Create a backup file including the timestamp. |
| `force` | no | true | boolean | If false, only transfer if destination does not exist. |
| `validate` | no | - | string | Command to validate the file before use (use %s for file path). |

*Either `src` or `content` must be provided.

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `dest` | string | Destination file path |
| `src` | string | Source file path (if used) |
| `checksum` | string | SHA1 checksum of the file |
| `size` | integer | Size of the file in bytes |
| `owner` | string | Owner of the file |
| `group` | string | Group of the file |
| `mode` | string | Permissions of the file |
| `backup_file` | string | Path to backup file (if backup was created) |

## Examples

### Copy a file with specific permissions

```yaml
- name: Copy configuration file
  copy:
    src: files/app.conf
    dest: /etc/myapp/app.conf
    owner: root
    group: root
    mode: "0644"
```

### Copy content directly to a file

```yaml
- name: Create a configuration file from content
  copy:
    content: |
      [myapp]
      setting1 = value1
      setting2 = value2
    dest: /etc/myapp/settings.ini
    mode: "0640"
```

### Copy with backup

```yaml
- name: Update configuration with backup
  copy:
    src: files/nginx.conf
    dest: /etc/nginx/nginx.conf
    backup: yes
```

### Copy only if destination does not exist

```yaml
- name: Copy default config if not present
  copy:
    src: files/default.conf
    dest: /etc/myapp/config.conf
    force: no
```

### Validate configuration before applying

```yaml
- name: Copy nginx config with validation
  copy:
    src: files/nginx.conf
    dest: /etc/nginx/nginx.conf
    validate: nginx -t -c %s
```

## Notes

- The `copy` module is idempotent; it will not copy files if they are identical
- Checksums are used to determine if a file has changed
- When using `content`, the file is created even if empty
- Symbolic mode notation (like "u=rw,g=r,o=r") is supported
- The module creates parent directories if they do not exist

## Real-World Use Cases

### Deploy Configuration Files

```yaml
- name: Deploy application configuration
  copy:
    src: configs/{{ environment }}/app.conf
    dest: /etc/myapp/app.conf
    owner: myapp
    group: myapp
    mode: "0640"
    backup: yes
  notify: Restart myapp
```

### Create SSL Certificates

```yaml
- name: Deploy SSL certificate
  copy:
    src: ssl/{{ inventory_hostname }}.crt
    dest: /etc/ssl/certs/server.crt
    owner: root
    group: root
    mode: "0644"

- name: Deploy SSL key
  copy:
    src: ssl/{{ inventory_hostname }}.key
    dest: /etc/ssl/private/server.key
    owner: root
    group: ssl-cert
    mode: "0640"
```

### Create Configuration from Content

```yaml
- name: Create database configuration
  copy:
    content: |
      [client]
      host={{ db_host }}
      port={{ db_port }}
      user={{ db_user }}
      password={{ db_password }}
    dest: /etc/myapp/.my.cnf
    owner: myapp
    group: myapp
    mode: "0600"
```

### Deploy with Validation

```yaml
- name: Deploy sudoers configuration
  copy:
    src: sudoers.d/myapp
    dest: /etc/sudoers.d/myapp
    owner: root
    group: root
    mode: "0440"
    validate: visudo -cf %s
```

## Troubleshooting

### File not found

Ensure the source file exists and the path is correct:

```bash
# Check file exists relative to playbook
ls -la files/myfile.conf

# Check file exists relative to role
ls -la roles/myrole/files/myfile.conf
```

Files are searched in this order:
1. `files/` directory relative to the playbook
2. `files/` directory in the role
3. The exact path specified

### Permission denied on destination

Check that you have write permissions or use privilege escalation:

```yaml
- name: Copy to privileged location
  copy:
    src: myfile.conf
    dest: /etc/myfile.conf
  become: yes
```

### File keeps showing as changed

This usually happens when file metadata differs. Check:

1. Mode differences (use consistent format like "0644")
2. Owner/group differences
3. SELinux context differences

```yaml
- name: Copy with explicit attributes
  copy:
    src: myfile.conf
    dest: /etc/myfile.conf
    owner: root
    group: root
    mode: "0644"
```

### Content encoding issues

For binary files or files with special characters, ensure proper handling:

```yaml
# For binary content, base64 encode it
- name: Copy binary content
  copy:
    content: "{{ binary_data | b64decode }}"
    dest: /path/to/binary
```

### Validation command fails

The validation command receives a temporary file path. Ensure the validator supports this:

```yaml
# Use %s placeholder for temp file path
- name: Copy with validation
  copy:
    src: nginx.conf
    dest: /etc/nginx/nginx.conf
    validate: nginx -t -c %s
```

### Large file transfer is slow

For very large files, consider using `synchronize` module or rsync directly. The copy module reads the entire file into memory.

### Backup files accumulating

Backup files are not automatically cleaned up. Manage them with a separate task:

```yaml
- name: Clean old backups
  shell: find /etc -name "*.*.*.bak" -mtime +30 -delete
```

## See Also

- [template](template.md) - Template files with variable substitution
- [file](file.md) - Manage file and directory properties
- [lineinfile](lineinfile.md) - Modify specific lines in files
- [blockinfile](blockinfile.md) - Manage blocks of text in files
- [stat](stat.md) - Check if destination exists before copying
