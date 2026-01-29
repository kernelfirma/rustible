# Rustible Module Reference

This directory contains documentation for all Rustible modules. Modules are the building blocks that perform actions on target systems.

## Module Categories

### Package Management
| Module | Description |
|--------|-------------|
| [apt](apt.md) | Manage apt packages on Debian/Ubuntu |
| [dnf](dnf.md) | Manage packages with dnf on Fedora |
| [package](package.md) | Generic package manager abstraction |
| [pip](pip.md) | Manage Python packages with pip |
| [yum](yum.md) | Manage packages with yum on RHEL/CentOS |

### Command Execution
| Module | Description |
|--------|-------------|
| [command](command.md) | Execute commands on remote hosts |
| [shell](shell.md) | Execute shell commands with full shell features |

### File Operations
| Module | Description |
|--------|-------------|
| [blockinfile](blockinfile.md) | Insert/update/remove text blocks in files |
| [copy](copy.md) | Copy files to remote locations |
| [file](file.md) | Manage file and directory properties |
| [lineinfile](lineinfile.md) | Manage lines in text files |
| [stat](stat.md) | Retrieve file or directory information |
| [template](template.md) | Template files with Jinja2 |
| [unarchive](unarchive.md) | Extract archive files on remote hosts |

### System Administration
| Module | Description |
|--------|-------------|
| [group](group.md) | Manage system groups |
| [service](service.md) | Manage system services |
| [systemd_unit](systemd_unit.md) | Manage systemd unit files |
| [user](user.md) | Manage user accounts |

### Source Control
| Module | Description |
|--------|-------------|
| [git](git.md) | Clone and manage git repositories |

### Network and HTTP
| Module | Description |
|--------|-------------|
| [uri](uri.md) | Perform HTTP requests |

### Cloud
| Module | Description |
|--------|-------------|
| [aws_ec2](aws_ec2.md) | Manage AWS EC2 instances |
| [aws_s3](aws_s3.md) | Manage AWS S3 objects |

### Flow Control
| Module | Description |
|--------|-------------|
| [fail](fail.md) | Fail with custom message |
| [include_tasks](include_tasks.md) | Dynamically include task files |
| [pause](pause.md) | Pause playbook execution |
| [wait_for](wait_for.md) | Wait for a condition |

### Logic and Utilities
| Module | Description |
|--------|-------------|
| [assert](assert.md) | Assert conditions are true |
| [debug](debug.md) | Print debug messages |
| [include_vars](include_vars.md) | Load variables from files |
| [set_fact](set_fact.md) | Set host variables dynamically |

## Module Classification

Rustible classifies modules into tiers based on their execution characteristics:

### Tier 1: LocalLogic
Modules that run entirely on the control node. They never touch the remote host and execute in nanoseconds.
- debug, set_fact, assert, include_vars, include_tasks, fail, pause

### Tier 2: NativeTransport
File/transport modules implemented natively in Rust. These use direct SSH/SFTP operations without remote Python.
- copy, template, file, lineinfile, blockinfile, stat

### Tier 3: RemoteCommand
Remote command execution modules. These execute commands on the remote host via SSH.
- command, shell, service, package, user, group, apt, yum, dnf, pip, git, wait_for

### Tier 4: PythonFallback
Python fallback for Ansible module compatibility. Used for any module without a native Rust implementation.

## Common Parameters

Most modules support these common parameters:

| Parameter | Type | Description |
|-----------|------|-------------|
| `become` | boolean | Enable privilege escalation |
| `become_user` | string | User to become for privilege escalation |
| `become_method` | string | Method for privilege escalation (sudo, su) |
| `check_mode` | boolean | Run in check mode (dry run) |
| `diff` | boolean | Show differences when changing files |

## Return Values

All modules return a standard output structure:

| Field | Type | Description |
|-------|------|-------------|
| `changed` | boolean | Whether the module made changes |
| `msg` | string | Human-readable message about what happened |
| `status` | string | Execution status (ok, changed, failed, skipped) |
| `diff` | object | Optional diff showing what changed |
| `data` | object | Additional module-specific data |
| `stdout` | string | Standard output (for command modules) |
| `stderr` | string | Standard error (for command modules) |
| `rc` | integer | Return code (for command modules) |
