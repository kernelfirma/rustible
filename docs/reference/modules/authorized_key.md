---
summary: Reference for the authorized_key module that manages SSH authorized_keys file entries.
read_when: You need to add, remove, or manage SSH public keys for user accounts from playbooks.
---

# authorized_key - Manage SSH Authorized Keys

## Synopsis
Manages SSH public keys in user authorized_keys files. Supports adding and removing keys, setting key options, exclusive key management, and key validation. Works both locally and via remote connections.

## Classification
**NativeTransport** - manages files directly via the connection transport layer.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| user | yes | - | string | Username whose authorized_keys file to manage |
| key | yes | - | string | SSH public key string (e.g., `ssh-ed25519 AAAA... user@host`) |
| state | no | present | string | `present` to add the key, `absent` to remove it |
| path | no | ~/.ssh/authorized_keys | string | Custom path to the authorized_keys file |
| exclusive | no | false | bool | Replace all keys with only this key |
| key_options | no | - | string | SSH key options (e.g., `command="/bin/date",no-pty`) |
| comment | no | - | string | Override the key comment |
| manage_dir | no | true | bool | Create the .ssh directory if it does not exist |
| validate_certs | no | true | bool | Validate the SSH key format before writing |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| changed | bool | Whether the authorized_keys file was modified |
| msg | string | Description of the action taken |

## Examples
```yaml
- name: Add SSH key for deploy user
  authorized_key:
    user: deploy
    key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI... deploy@ci"
    state: present

- name: Remove old SSH key
  authorized_key:
    user: admin
    key: "ssh-rsa AAAAB3NzaC1yc2E... old@host"
    state: absent

- name: Add key with options
  authorized_key:
    user: backup
    key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI... backup@server"
    key_options: 'command="/usr/bin/rsync --server",no-pty,no-port-forwarding'

- name: Set exclusive key (removes all others)
  authorized_key:
    user: locked
    key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI... admin@secure"
    exclusive: true
```

## Notes
- The `user` parameter must contain only alphanumeric characters, underscores, and hyphens.
- Supported key types: ssh-rsa, ssh-ed25519, ssh-dss, ecdsa-sha2-nistp256/384/521, sk-ssh-ed25519, sk-ecdsa.
- The .ssh directory is created with mode 0700 and the authorized_keys file with mode 0600.
- Key matching is based on key type and key data; comments and options are ignored for comparison.
