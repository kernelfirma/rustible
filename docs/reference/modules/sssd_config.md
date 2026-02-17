---
summary: Reference for the sssd_config module that manages the main SSSD configuration file with services and domains.
read_when: You need to install SSSD, configure sssd.conf services and domains, or remove SSSD from playbooks.
---

# sssd_config - Manage SSSD Main Configuration

## Synopsis

Manage SSSD (System Security Services Daemon) main configuration including package installation, sssd.conf generation with services and domain lists, and service lifecycle management.

## Classification

**Default** - HPC module. Requires `hpc` and `identity` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| services | yes | - | list(string) | List of SSSD services to enable (e.g., ["nss", "pam", "ssh"]) |
| domains | yes | - | list(string) | List of SSSD domain names to configure (e.g., ["example.com"]) |
| state | no | "present" | string | Desired state: "present" or "absent" |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of individual changes applied |

## Examples

```yaml
- name: Configure SSSD with NSS and PAM services
  sssd_config:
    services:
      - nss
      - pam
    domains:
      - hpc.example.com
    state: present

- name: Configure SSSD with SSH service
  sssd_config:
    services:
      - nss
      - pam
      - ssh
    domains:
      - corp.example.com
      - hpc.example.com

- name: Remove SSSD entirely
  sssd_config:
    services: []
    domains: []
    state: absent
```

## Notes

- Requires building with `--features hpc,identity` or `--features full-hpc`.
- Supports RHEL-family and Debian-family distributions.
- On RHEL-family systems, installs `sssd` and `sssd-tools`.
- On Debian-family systems, installs `sssd` and `sssd-tools`.
- The configuration file `/etc/sssd/sssd.conf` is created with mode 600.
- The module enables and starts the `sssd` systemd service automatically.
- Use the companion `sssd_domain` module to configure individual domain sections.
- When `state` is "absent", the SSSD service is stopped, disabled, and packages are removed.
- This module uses `HostExclusive` parallelization, meaning only one instance runs per host at a time.
