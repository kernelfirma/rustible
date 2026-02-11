---
summary: Reference for the gather_facts module that collects system information from target hosts.
read_when: You need to gather OS, hardware, network, or environment facts from playbooks.
---

# gather_facts - Gather System Facts

## Synopsis
Gathers facts about the target system including operating system details, hardware specifications, network configuration, date/time information, and environment variables. Facts are collected both locally and remotely via SSH connections.

## Classification
**Standard** - read-only module that does not modify the target system.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| gather_subset | no | ["all"] | list | Subsets of facts to gather: `all`, `os`, `min`, `hardware`, `network`, `date_time`, `env` |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| ansible_facts.hostname | string | Fully qualified hostname |
| ansible_facts.hostname_short | string | Short hostname |
| ansible_facts.system | string | Kernel name (e.g., Linux) |
| ansible_facts.kernel | string | Kernel version |
| ansible_facts.architecture | string | CPU architecture |
| ansible_facts.machine | string | Normalized architecture name |
| ansible_facts.distribution | string | OS distribution ID |
| ansible_facts.distribution_version | string | Distribution version |
| ansible_facts.os_family | string | OS family (debian, redhat, arch, etc.) |
| ansible_facts.user_id | string | Current username |
| ansible_facts.user_uid | integer | Current user UID |
| ansible_facts.user_gid | integer | Current user GID |
| ansible_facts.processor_count | integer | Number of logical CPUs |
| ansible_facts.processor | string | CPU model name |
| ansible_facts.processor_cores | integer | Number of physical cores |
| ansible_facts.memtotal_mb | integer | Total memory in MB |
| ansible_facts.memfree_mb | integer | Free memory in MB |
| ansible_facts.swaptotal_mb | integer | Total swap in MB |
| ansible_facts.interfaces | list | Network interfaces with MAC, MTU, state |
| ansible_facts.default_ipv4 | string | Default IPv4 address |
| ansible_facts.fqdn | string | Fully qualified domain name |
| ansible_facts.date_time | string | Current date/time with timezone |
| ansible_facts.epoch | integer | Current Unix epoch |
| ansible_facts.timezone | string | System timezone |
| ansible_facts.uptime_seconds | integer | System uptime in seconds |
| ansible_facts.env | object | Key environment variables |
| ansible_facts.python_version | string | Python 3 version if available |

## Examples
```yaml
- name: Gather all facts
  gather_facts:

- name: Gather only OS and hardware facts
  gather_facts:
    gather_subset:
      - os
      - hardware

- name: Gather minimal OS facts
  gather_facts:
    gather_subset:
      - min
```

## Notes
- Fact gathering is read-only and behaves identically in check mode.
- Remote facts are gathered by executing commands over the connection (hostname, uname, cat, etc.).
- The `os` and `min` subsets include hostname, kernel, architecture, distribution, and user info.
- The `hardware` subset reads from /proc/cpuinfo, /proc/meminfo, and df.
- The `network` subset reads from /sys/class/net and ip route.
