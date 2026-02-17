---
summary: Reference for the munge module that manages MUNGE authentication service installation, key distribution, and service lifecycle.
read_when: You need to install, configure, or remove MUNGE authentication from playbooks.
---

# munge - Manage MUNGE Authentication Service

## Synopsis

Manages MUNGE (MUNGE Uid 'N' Gid Emporium) authentication service for HPC clusters. Handles package installation, key generation or distribution, directory permissions, and systemd service management. MUNGE is a prerequisite for Slurm workload manager authentication.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| state | no | `"present"` | string | Desired state: `"present"` to install and configure, `"absent"` to remove. |
| key_source | no | `null` | string | Path to an existing `munge.key` file on the target node to copy into `/etc/munge/`. |
| key_content | no | `null` | string | Base64-encoded munge key content. Decoded and written to `/etc/munge/munge.key`. |
| munge_user | no | `"munge"` | string | User that owns munge files and directories. |
| munge_group | no | `"munge"` | string | Group that owns munge files and directories. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.changes | array | List of changes applied (e.g. "Installed munge packages", "Distributed munge key") |

## Examples

```yaml
- name: Install and configure munge with a shared key
  munge:
    state: present
    key_content: "{{ munge_key_b64 }}"

- name: Install munge and auto-generate a key
  munge:
    state: present

- name: Distribute munge key from a local file
  munge:
    state: present
    key_source: /shared/secrets/munge.key

- name: Remove munge
  munge:
    state: absent
```

## Notes

- Requires building with `--features hpc`.
- Supports RHEL-family (dnf) and Debian-family (apt) distributions.
- When neither `key_source` nor `key_content` is provided and no key exists, a new key is generated with `mungekey --create --force`.
- Key file permissions are set to `0400` owned by the configured munge user/group.
- The `munge.service` systemd unit is enabled and started automatically.
- Parallelization hint: `HostExclusive` (one invocation per host at a time).
