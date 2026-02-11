---
summary: Reference for the synchronize module that provides rsync-based file and directory synchronization.
read_when: You need to efficiently sync files between the control machine and remote hosts from playbooks.
---

# synchronize - Rsync File Synchronization

## Synopsis
Wraps rsync for efficient file and directory synchronization between the control machine and remote hosts. Supports push and pull modes, archive preservation, compression, deletion of extraneous files, and custom rsync options.

## Classification
**NativeTransport** - executes rsync locally on the control node. Fully parallelizable across hosts.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| src | yes | - | string | Source path |
| dest | yes | - | string | Destination path |
| mode | no | push | string | Sync direction: `push` (local to remote) or `pull` (remote to local) |
| delete | no | false | bool | Delete files in dest that do not exist in src (mirror) |
| recursive | no | true | bool | Recurse into directories |
| archive | no | true | bool | Archive mode (preserves permissions, times, symlinks, etc.) |
| compress | no | true | bool | Compress data during transfer |
| checksum | no | false | bool | Use checksum comparison instead of time/size |
| links | no | true | bool | Copy symlinks as symlinks |
| perms | no | true | bool | Preserve file permissions |
| times | no | true | bool | Preserve modification times |
| owner | no | false | bool | Preserve owner (requires sudo) |
| group | no | false | bool | Preserve group (requires sudo) |
| rsync_path | no | rsync | string | Path to rsync on the remote host |
| rsync_opts | no | [] | list | Additional rsync options |
| partial | no | false | bool | Keep partially transferred files |
| verify_host | no | true | bool | Verify SSH host key |
| ssh_args | no | - | string | Additional SSH arguments |
| set_remote_user | no | true | bool | Set remote user for rsync connection |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| src | string | Source path used |
| dest | string | Destination path used |
| mode | string | Sync direction used |
| rsync_args | string | Full rsync argument string |
| stdout | string | Rsync standard output |
| stderr | string | Rsync standard error |
| rc | integer | Rsync exit code |

## Examples
```yaml
- name: Push local directory to remote
  synchronize:
    src: /opt/app/
    dest: /srv/app/

- name: Pull files from remote host
  synchronize:
    src: /var/log/app/
    dest: /tmp/remote-logs/
    mode: pull

- name: Mirror with deletion
  synchronize:
    src: /opt/release/
    dest: /srv/app/
    delete: true

- name: Sync with exclusions
  synchronize:
    src: /opt/project/
    dest: /srv/project/
    rsync_opts:
      - "--exclude=*.log"
      - "--exclude=.git"
      - "--exclude=node_modules"
```

## Notes
- Requires rsync to be installed on the control machine (and on the remote host for pull mode).
- In check mode, rsync runs with `-n` (dry-run) to report what would change.
- Trailing slashes on `src` follow standard rsync semantics (contents vs. directory itself).
- The module constructs SSH options automatically from the connection context.
- When `archive` is true, individual flags like `recursive`, `links`, `perms`, and `times` are implied.
