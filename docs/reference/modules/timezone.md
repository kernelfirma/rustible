---
summary: Reference for the timezone module that manages system timezone and hardware clock settings.
read_when: You need to set the system timezone, configure NTP, or manage the hardware clock from playbooks.
---

# timezone - Manage System Timezone

## Synopsis
Manages the system timezone, NTP synchronization, and hardware clock (RTC) settings. Supports both timedatectl (systemd) and traditional file-based methods, with automatic detection of the best strategy.

## Classification
**RemoteCommand** - executes timedatectl or file operations on the remote host.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | - | string | Timezone name (e.g., `America/New_York`, `UTC`, `Europe/London`) |
| hwclock | no | - | string | Hardware clock mode: `UTC` or `local` |
| ntp | no | - | bool | Enable or disable NTP synchronization |
| use | no | auto | string | Strategy: `timedatectl`, `file`, or `auto` |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| timezone | string | The timezone that was set |
| previous_timezone | string | The timezone before the change |
| strategy | string | The strategy used (timedatectl or file) |
| ntp_enabled | bool | NTP status after change (when ntp parameter is set) |
| hwclock | string | Hardware clock mode after change (when hwclock parameter is set) |
| time_status | object | Raw timedatectl status output |

## Examples
```yaml
- name: Set timezone to UTC
  timezone:
    name: UTC

- name: Set timezone with NTP enabled
  timezone:
    name: America/New_York
    ntp: true

- name: Configure for Windows dual-boot
  timezone:
    name: Europe/London
    hwclock: local

- name: Force file-based method
  timezone:
    name: Asia/Tokyo
    use: file
```

## Notes
- Timezone names must follow the Area/Location format (e.g., `America/New_York`) or be `UTC`/`GMT` variants.
- The `auto` strategy checks for timedatectl and a running systemd, falling back to file-based methods.
- File-based method creates a symlink at /etc/localtime and updates /etc/timezone or /etc/sysconfig/clock.
- NTP control tries timedatectl first, then falls back to enabling/disabling chronyd, ntpd, or systemd-timesyncd.
- The module verifies the timezone exists in /usr/share/zoneinfo before applying.
