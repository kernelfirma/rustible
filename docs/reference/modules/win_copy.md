---
summary: Reference for the win_copy module that copies files to Windows hosts.
read_when: You need to transfer files or write content to Windows targets from playbooks.
---

# win_copy - Copy Files to Windows Hosts

## Synopsis

Copies files to Windows destinations via WinRM/SSH with support for backup creation,
checksum verification, content generation, and read-only file overwriting. Either a
local source file or inline content can be provided but not both.

## Classification

**NativeTransport** - Windows module (experimental). Requires `winrm` feature flag.

## Parameters

| Parameter  | Required | Default  | Type   | Description                                                    |
|------------|----------|----------|--------|----------------------------------------------------------------|
| src        | no*      | -        | string | Source file path on the control node. Mutually exclusive with `content`. |
| dest       | yes      | -        | string | Destination path on the Windows target. Must be a valid Windows path.    |
| content    | no*      | -        | string | Inline content to write to dest. Mutually exclusive with `src`.          |
| backup     | no       | `false`  | bool   | Create a timestamped `.bak` copy before overwriting.                     |
| force      | no       | `true`   | bool   | Overwrite destination even if the file is read-only.                     |
| checksum   | no       | `SHA256` | string | Hash algorithm for integrity checks (`md5`, `sha1`, `sha256`, `sha512`).|

## Return Values

| Key         | Type   | Description                                      |
|-------------|--------|--------------------------------------------------|
| dest        | string | The destination path that was written.            |
| checksum    | string | Hex checksum of the transferred content.          |
| backup_file | string | Path to the backup file, if `backup` was enabled. |

## Examples

```yaml
- name: Copy a configuration file with backup
  win_copy:
    src: files/app.config
    dest: C:\Program Files\MyApp\app.config
    backup: true

- name: Write inline content to a file
  win_copy:
    content: |
      [Settings]
      Debug=false
    dest: C:\MyApp\settings.ini

- name: Force-overwrite a read-only file using MD5 verification
  win_copy:
    src: files/data.bin
    dest: C:\Data\data.bin
    force: true
    checksum: md5
```

## Notes

- Requires building with `--features winrm`.
- Either `src` or `content` must be supplied, but not both.
- Binary files transferred via `src` are base64-encoded for the PowerShell transport.
- Parent directories on the target are created automatically if they do not exist.
- When `force: false` and the target is read-only, the module returns an error.
