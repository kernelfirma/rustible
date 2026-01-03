---
summary: Reference for the service module that manages systemd, sysvinit, OpenRC, and Upstart services.
read_when: You need to start, stop, restart, reload, or enable/disable services on remote hosts.
---

# service - Manage System Services

## Synopsis

The `service` module manages system services using systemd, sysvinit, OpenRC, Upstart, or other init systems. It can start, stop, restart, reload, and enable/disable services.

## Classification

**RemoteCommand** - This module executes service management commands on remote hosts via SSH.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `name` | yes | - | string | Name of the service to manage. Supports wildcards (`*`, `?`, `[...]`) for systemd. |
| `state` | no | - | string | Desired state: started, stopped, restarted, reloaded. |
| `enabled` | no | - | boolean | Whether the service should start on boot. |
| `pattern` | no | - | string | Pattern to look for in process table (for services without proper status). |
| `runlevel` | no | - | string | Runlevel(s) for sysvinit/OpenRC enable/disable (e.g., "2345" or "default"). |
| `sleep` | no | - | integer | Seconds to sleep between stop and start for restart. |
| `use_systemctl` | no | - | boolean | Force use of systemctl even if service command is available. |
| `daemon_reload` | no | false | boolean | Reload systemd daemon before performing action. |
| `daemon_reexec` | no | false | boolean | Re-execute systemd manager before performing action. |
| `arguments` | no | - | string | Additional arguments passed to the service command. |

## State Values

| State | Description |
|-------|-------------|
| `started` | Ensure the service is running |
| `stopped` | Ensure the service is stopped |
| `restarted` | Stop and then start the service |
| `reloaded` | Reload the service configuration without restart |

## Supported Init Systems

| Init System | Detection Method | Enable/Disable Support |
|-------------|------------------|------------------------|
| **systemd** | `/run/systemd/system` directory or `systemctl` binary | Yes |
| **SysV** | `/etc/init.d` directory | Yes (via chkconfig or update-rc.d) |
| **OpenRC** | `rc-service` binary | Yes (via rc-update) |
| **Upstart** | `/etc/init/*.conf` files | Yes (via override files) |
| **Launchd** | `launchctl` binary (macOS) | Partial |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `status.active` | boolean | Whether the service is currently running |
| `status.enabled` | boolean | Whether the service is enabled at boot |
| `status.init_system` | string | Detected init system (systemd, sysv, openrc, etc.) |

## Examples

### Start a service

```yaml
- name: Start nginx
  service:
    name: nginx
    state: started
```

### Stop a service

```yaml
- name: Stop apache
  service:
    name: httpd
    state: stopped
```

### Restart a service

```yaml
- name: Restart nginx after config change
  service:
    name: nginx
    state: restarted
```

### Restart with delay between stop and start

Some services need time to release resources before starting again:

```yaml
- name: Restart database with cleanup time
  service:
    name: postgresql
    state: restarted
    sleep: 5  # Wait 5 seconds between stop and start
```

### Reload service configuration

```yaml
- name: Reload nginx configuration
  service:
    name: nginx
    state: reloaded
```

### Enable service at boot

```yaml
- name: Enable nginx at boot
  service:
    name: nginx
    enabled: yes
```

### Start and enable service

```yaml
- name: Ensure nginx is running and enabled
  service:
    name: nginx
    state: started
    enabled: yes
```

### Enable service at specific runlevels (SysV/OpenRC)

```yaml
- name: Enable httpd at runlevels 3 and 5
  service:
    name: httpd
    enabled: yes
    runlevel: "35"
```

### Disable service at boot

```yaml
- name: Disable unused service
  service:
    name: cups
    enabled: no
```

### Stop and disable service

```yaml
- name: Completely disable service
  service:
    name: postfix
    state: stopped
    enabled: no
```

### Restart service using handlers

Handlers are useful for restarting services only when configuration changes:

```yaml
- hosts: webservers
  tasks:
    - name: Update nginx configuration
      template:
        src: nginx.conf.j2
        dest: /etc/nginx/nginx.conf
      notify: Restart nginx

  handlers:
    - name: Restart nginx
      service:
        name: nginx
        state: restarted
```

### Reload systemd daemon before starting service

When you've installed a new unit file:

```yaml
- name: Copy new service unit file
  copy:
    src: myapp.service
    dest: /etc/systemd/system/myapp.service

- name: Start new service
  service:
    name: myapp
    state: started
    enabled: yes
    daemon_reload: yes
```

### Check service state without modifying

Use check mode to see what would change:

```yaml
- name: Check nginx status
  service:
    name: nginx
    state: started
  check_mode: yes
  register: nginx_status
```

### Handle services without proper status command

Some legacy services don't have a working status command. Use `pattern` to check the process table:

```yaml
- name: Start legacy daemon
  service:
    name: old_daemon
    state: started
    pattern: "/usr/local/bin/old_daemon"
```

### Manage multiple services with wildcards (systemd only)

```yaml
- name: Stop all docker-* services
  service:
    name: "docker-*"
    state: stopped
```

### Force use of systemctl

On systems with both `service` and `systemctl` commands:

```yaml
- name: Start nginx using systemctl explicitly
  service:
    name: nginx
    state: started
    use_systemctl: yes
```

### Pass additional arguments

```yaml
- name: Restart nginx without blocking
  service:
    name: nginx
    state: restarted
    arguments: "--no-block"
```

## Init System Specific Notes

### Systemd

- Uses `systemctl` for all operations
- Supports `daemon_reload` and `daemon_reexec` options
- Supports wildcard patterns in service names
- Falls back to `reload-or-restart` if `reload` fails
- Returns both `active` and `enabled` status

### SysV Init

- Uses `service` command for start/stop/restart/reload
- Uses `chkconfig` (RHEL/CentOS) or `update-rc.d` (Debian/Ubuntu) for enable/disable
- The `runlevel` parameter controls which runlevels the service is enabled in
- The `pattern` parameter is useful for services without proper status scripts

### OpenRC

- Uses `rc-service` for service operations
- Uses `rc-update` for enable/disable
- The `runlevel` parameter defaults to "default"

### Upstart

- Uses `initctl` for service operations
- Enable/disable uses override files in `/etc/init/`
- Services are enabled by default if their `.conf` file exists

## Best Practices

1. **Use handlers for restarts**: Instead of always restarting, use handlers to only restart when configuration changes.

2. **Always enable important services**: Combine `state: started` with `enabled: yes` to ensure services start on boot.

3. **Use check mode first**: Run with `check_mode: yes` to preview changes before applying.

4. **Handle reload failures**: The module automatically falls back to restart if reload fails on systemd and SysV.

5. **Use patterns for legacy services**: If a service doesn't have a proper status command, use the `pattern` parameter.

6. **Reload daemon for new units**: Always use `daemon_reload: yes` when installing new systemd unit files.

## Real-World Use Cases

### Web Application Deployment

```yaml
- name: Stop application for maintenance
  service:
    name: myapp
    state: stopped

- name: Deploy new version
  copy:
    src: myapp-{{ version }}.jar
    dest: /opt/myapp/myapp.jar

- name: Start application
  service:
    name: myapp
    state: started
    enabled: yes
```

### Database Cluster Management

```yaml
- name: Ensure PostgreSQL is running on all nodes
  service:
    name: postgresql
    state: started
    enabled: yes

- name: Graceful restart with sleep
  service:
    name: postgresql
    state: restarted
    sleep: 10
```

### Docker Host Setup

```yaml
- name: Ensure Docker is running
  service:
    name: docker
    state: started
    enabled: yes

- name: Ensure containerd is running
  service:
    name: containerd
    state: started
    enabled: yes
```

## Troubleshooting

### Service not found

Verify the service name matches the init system:

```bash
# For systemd
systemctl list-units --type=service | grep myservice

# For SysV
ls /etc/init.d/

# Check service file exists
systemctl cat myservice
```

### Service fails to start

Check service logs:

```bash
# Systemd
journalctl -u myservice -n 50 --no-pager
systemctl status myservice

# SysV
cat /var/log/myservice.log
```

Common causes:
- Configuration file errors
- Missing dependencies
- Port already in use
- Permission issues

### "Service is masked" error

Unmask the service before enabling:

```bash
systemctl unmask myservice
```

```yaml
- name: Unmask and enable service
  shell: systemctl unmask myservice

- name: Now enable it
  service:
    name: myservice
    enabled: yes
    state: started
```

### Enable/disable not working

Check the init system being used:

```yaml
- name: Debug init system
  debug:
    var: ansible_service_mgr
```

For non-systemd systems, `chkconfig` or `update-rc.d` must be available.

### Reload fails

Not all services support reload. The module falls back to restart:

```yaml
- name: Try reload, fall back to restart
  service:
    name: myservice
    state: reloaded
  register: reload_result
  failed_when: false

- name: Force restart if reload failed
  service:
    name: myservice
    state: restarted
  when: reload_result.failed
```

### daemon_reload required for new unit files

After adding or modifying systemd unit files:

```yaml
- name: Copy new service file
  copy:
    src: myservice.service
    dest: /etc/systemd/system/myservice.service

- name: Start with daemon reload
  service:
    name: myservice
    state: started
    daemon_reload: yes
```

### Service shows enabled but does not start on boot

Check the service's WantedBy target:

```bash
systemctl cat myservice | grep WantedBy
systemctl get-default  # Check default target
```

Ensure the service is wanted by the right target:

```ini
[Install]
WantedBy=multi-user.target
```

## See Also

- [command](command.md) - For custom service management commands
- [systemd](systemd.md) - For advanced systemd-specific operations
- [file](file.md) - Manage service configuration files
- [template](template.md) - Generate systemd unit files
- [sysctl](sysctl.md) - Set kernel parameters for services
