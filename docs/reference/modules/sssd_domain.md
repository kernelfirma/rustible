---
summary: Reference for the sssd_domain module that manages per-domain SSSD configuration sections in sssd.conf.
read_when: You need to add, update, or remove individual SSSD domain configurations from playbooks.
---

# sssd_domain - Manage SSSD Domain Configuration

## Synopsis

Manage per-domain configuration sections within sssd.conf. Each domain section configures an identity provider, optional LDAP URI, and optional Kerberos realm. Requires that sssd.conf already exists (use `sssd_config` first).

## Classification

**Default** - HPC module. Requires `hpc` and `identity` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | - | string | Domain name (used as the section header `[domain/NAME]`) |
| provider | yes | - | string | Identity provider type (e.g., "ldap", "ad", "ipa", "krb5") |
| ldap_uri | no | null | string | LDAP URI for the domain (e.g., "ldap://ldap.example.com") |
| krb5_realm | no | null | string | Kerberos realm for the domain (e.g., "EXAMPLE.COM") |
| state | no | "present" | string | Desired state: "present" or "absent" |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.domain | string | The domain name that was configured |

## Examples

```yaml
- name: Add LDAP domain to SSSD
  sssd_domain:
    name: "hpc.example.com"
    provider: ldap
    ldap_uri: "ldap://ldap.hpc.example.com"
    state: present

- name: Add Active Directory domain with Kerberos
  sssd_domain:
    name: "corp.example.com"
    provider: ad
    ldap_uri: "ldap://dc.corp.example.com"
    krb5_realm: "CORP.EXAMPLE.COM"

- name: Remove a domain from SSSD
  sssd_domain:
    name: "old.example.com"
    provider: ldap
    state: absent
```

## Notes

- Requires building with `--features hpc,identity` or `--features full-hpc`.
- The `/etc/sssd/sssd.conf` file must already exist before using this module. Run `sssd_config` first.
- Domain sections are appended to the end of sssd.conf when added.
- If the domain section already exists in sssd.conf, the module reports no change (idempotent).
- When `state` is "absent", the entire `[domain/NAME]` section is removed from sssd.conf.
- This module uses `HostExclusive` parallelization, meaning only one instance runs per host at a time.
