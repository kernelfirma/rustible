---
summary: Reference for the unarchive module that extracts archives on remote hosts.
read_when: You need to extract tar/zip archives onto remote systems.
---

# unarchive - Extract Archive Files

## Synopsis

The `unarchive` module extracts archive files (tar, tar.gz, zip) on remote hosts.
It can pull from local or remote sources and unpack into a target directory.

## Classification

**RemoteCommand** - Executes extraction commands on the remote host.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| `src` | yes | - | string | Path to the archive file. |
| `dest` | yes | - | string | Destination directory to extract into. |
| `remote_src` | no | false | boolean | Treat `src` as a path on the remote host. |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| `changed` | boolean | Whether files were extracted. |
| `dest` | string | Destination path. |

## Examples

### Extract a local archive to /opt

```yaml
- name: Extract app bundle
  unarchive:
    src: /tmp/app.tar.gz
    dest: /opt/app
```

### Extract a remote archive

```yaml
- name: Extract remote archive
  unarchive:
    src: /var/tmp/app.zip
    dest: /opt/app
    remote_src: true
```

## Notes

- Ensure the destination directory exists or the module can create it.
- Large archives may take time depending on disk performance.
