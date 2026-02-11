---
summary: Reference for the known_hosts module that manages SSH known_hosts file entries.
read_when: You need to add, remove, or verify SSH host keys in known_hosts from playbooks.
---

# known_hosts - Manage SSH Known Hosts

## Synopsis
Manages entries in SSH known_hosts files for host key verification. Supports adding and removing host keys, key scanning via ssh-keyscan, hostname hashing, and key rotation.

## Classification
**LocalLogic** - runs on the control node to manage local known_hosts files. Rate-limited parallelization due to potential network key scanning.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| name | yes | - | string | Hostname or IP address of the host |
| key | no | - | string | SSH key type shorthand (e.g., `ed25519`, `rsa`) for filtering |
| state | no | present | string | `present` to add/update, `absent` to remove |
| path | no | ~/.ssh/known_hosts | string | Path to the known_hosts file |
| hash_host | no | false | bool | Hash the hostname in the known_hosts entry |
| key_type | no | - | string | SSH key type: `rsa`, `ed25519`, `ecdsa`, `dss` |
| key_data | no | - | string | Base64-encoded public key data (skips scanning) |
| port | no | 22 | integer | SSH port for the host |
| scan | no | true | bool | Scan the host for keys using ssh-keyscan |
| timeout | no | 5 | integer | Timeout in seconds for key scanning |
| backup | no | - | string | Suffix for backup file before modification |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| added_count | integer | Number of keys added |
| updated_count | integer | Number of keys updated |
| removed_count | integer | Number of keys removed |
| changed | bool | Whether the known_hosts file was modified |

## Examples
```yaml
- name: Ensure GitHub host key is known
  known_hosts:
    name: github.com
    state: present

- name: Add host key with explicit data
  known_hosts:
    name: internal.example.com
    key_type: ed25519
    key_data: "AAAAC3NzaC1lZDI1NTE5AAAAIExampleKeyData..."
    state: present

- name: Remove old host entry
  known_hosts:
    name: decommissioned.example.com
    state: absent

- name: Add host with hashed hostname
  known_hosts:
    name: secure.example.com
    hash_host: true
```

## Notes
- When `key_data` is provided, `key_type` is required and scanning is skipped.
- When `scan` is true and no `key_data` is given, ssh-keyscan fetches the host keys.
- The known_hosts file is saved with mode 0600 and parent directories are created as needed.
- Non-standard ports are stored in `[hostname]:port` format per SSH convention.
- Supports HMAC-SHA1 hostname hashing compatible with OpenSSH HashKnownHosts.
