---
summary: Reference for the archive module that creates compressed archives from files and directories.
read_when: You need to create tar, tar.gz, or zip archives from playbooks.
---

# archive - Create Compressed Archives

## Synopsis
Creates compressed archives from files and directories on the target host. Supports tar, tar.gz (gzip), and zip formats with configurable compression levels and file exclusion patterns.

## Classification
**NativeTransport** - executes locally using native file operations.

## Parameters
| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| path | yes | - | string | Source file or directory to archive |
| dest | yes | - | string | Destination path for the archive file |
| format | no | (inferred) | string | Archive format: `tar`, `gz`, `tar.gz`, `tgz`, `zip`. Inferred from dest extension if omitted |
| remove | no | false | bool | Remove source files after archiving |
| exclude_path | no | [] | list | List of path patterns to exclude from the archive |
| compression_level | no | 6 | integer | Compression level 0-9 (0=none, 9=best) |
| force | no | true | bool | Overwrite destination if it already exists |
| checksum | no | - | string | Compute checksum of the archive: `sha256`, `md5`, or `sha1` |

## Return Values
| Key | Type | Description |
|-----|------|-------------|
| dest | string | Path to the created archive |
| format | string | Archive format used |
| file_count | integer | Number of files included |
| original_size | integer | Total size of source files in bytes |
| archive_size | integer | Size of the archive in bytes |
| compression_ratio | string | Archive size as percentage of original |
| checksum | object | Checksum algorithm and value (when requested) |
| source_removed | bool | Whether the source was removed |

## Examples
```yaml
- name: Create a gzip-compressed tar archive
  archive:
    path: /var/log/app
    dest: /tmp/app-logs.tar.gz
    format: gz

- name: Create a zip archive excluding build artifacts
  archive:
    path: /srv/project
    dest: /tmp/project.zip
    format: zip
    exclude_path:
      - node_modules
      - .git
      - target

- name: Archive and remove source with checksum
  archive:
    path: /tmp/exports
    dest: /backups/exports.tar.gz
    remove: true
    checksum: sha256
```

## Notes
- Format is inferred from the destination file extension when not explicitly set.
- The `exclude_path` patterns match against relative paths and filenames within the source.
- Check mode reports what would happen without creating the archive.
- Parent directories for the destination are created automatically.
