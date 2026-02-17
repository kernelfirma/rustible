---
summary: Reference for the kerberos_client module that manages Kerberos authentication client configuration.
read_when: You need to configure Kerberos krb5.conf, deploy keytabs, or manage Kerberos packages from playbooks.
---

# kerberos_client - Manage Kerberos Client Configuration

## Synopsis

Manage Kerberos authentication client setup including krb5.conf generation, keytab deployment, and package installation. Supports both RHEL-family and Debian-family distributions.

## Classification

**Default** - HPC module. Requires `hpc` and `identity` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| realm | yes | - | string | Kerberos realm (e.g., "EXAMPLE.COM") |
| kdc | yes | - | string | KDC server hostname or IP (e.g., "kdc.example.com") |
| admin_server | no | value of `kdc` | string | Admin server hostname (defaults to the KDC value) |
| keytab_src | no | null | string | Path to keytab file on the control node to deploy to /etc/krb5.keytab |
| state | no | "present" | string | Desired state: "present" or "absent" |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of individual changes applied |
| data.realm | string | The Kerberos realm that was configured |

## Examples

```yaml
- name: Configure Kerberos client
  kerberos_client:
    realm: "HPC.EXAMPLE.COM"
    kdc: "kdc.hpc.example.com"
    admin_server: "kadmin.hpc.example.com"
    state: present

- name: Configure Kerberos with keytab deployment
  kerberos_client:
    realm: "HPC.EXAMPLE.COM"
    kdc: "kdc.hpc.example.com"
    keytab_src: "/tmp/host.keytab"

- name: Remove Kerberos client
  kerberos_client:
    realm: "HPC.EXAMPLE.COM"
    kdc: "kdc.hpc.example.com"
    state: absent
```

## Notes

- Requires building with `--features hpc,identity` or `--features full-hpc`.
- Supports RHEL-family (RHEL, CentOS, Rocky, AlmaLinux, Fedora) and Debian-family (Debian, Ubuntu) distributions.
- On RHEL-family systems, installs `krb5-workstation` and `krb5-libs`.
- On Debian-family systems, installs `krb5-user` and `libkrb5-3`.
- The module generates `/etc/krb5.conf` with the specified realm, KDC, and admin server settings.
- Deployed keytabs are written to `/etc/krb5.keytab` with mode 600.
- When `state` is "absent", the `realm` and `kdc` parameters are still formally required but the module only removes packages.
- This module uses `HostExclusive` parallelization, meaning only one instance runs per host at a time.
