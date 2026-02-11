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
| [raw](raw.md) | Execute raw commands via SSH |
| [script](script.md) | Execute local scripts on remote hosts |
| [shell](shell.md) | Execute shell commands with full shell features |

### File Operations
| Module | Description |
|--------|-------------|
| [archive](archive.md) | Create archive files |
| [blockinfile](blockinfile.md) | Insert/update/remove text blocks in files |
| [copy](copy.md) | Copy files to remote locations |
| [file](file.md) | Manage file and directory properties |
| [lineinfile](lineinfile.md) | Manage lines in text files |
| [stat](stat.md) | Retrieve file or directory information |
| [synchronize](synchronize.md) | Synchronize files using rsync |
| [template](template.md) | Template files with Jinja2 |
| [unarchive](unarchive.md) | Extract archive files on remote hosts |

### System Administration
| Module | Description |
|--------|-------------|
| [cron](cron.md) | Manage cron jobs |
| [group](group.md) | Manage system groups |
| [hostname](hostname.md) | Manage system hostname |
| [mount](mount.md) | Manage mount points |
| [service](service.md) | Manage system services |
| [sysctl](sysctl.md) | Manage sysctl settings |
| [systemd_unit](systemd_unit.md) | Manage systemd unit files |
| [timezone](timezone.md) | Manage system timezone |
| [user](user.md) | Manage user accounts |

### Source Control
| Module | Description |
|--------|-------------|
| [git](git.md) | Clone and manage git repositories |

### Network and HTTP
| Module | Description |
|--------|-------------|
| [uri](uri.md) | Perform HTTP requests |

### Security
| Module | Description |
|--------|-------------|
| [authorized_key](authorized_key.md) | Manage SSH authorized keys |
| [firewalld](firewalld.md) | Manage firewalld rules |
| [known_hosts](known_hosts.md) | Manage SSH known_hosts |
| [selinux](selinux.md) | Manage SELinux configuration |
| [ufw](ufw.md) | Manage UFW firewall rules |

### Fact Gathering
| Module | Description |
|--------|-------------|
| [facts](facts.md) | Gather system facts |

### Docker (feature flag: `docker`)
| Module | Description |
|--------|-------------|
| [docker_compose](docker_compose.md) | Manage Docker Compose projects |
| [docker_container](docker_container.md) | Manage Docker containers |
| [docker_image](docker_image.md) | Manage Docker images |
| [docker_network](docker_network.md) | Manage Docker networks |
| [docker_volume](docker_volume.md) | Manage Docker volumes |

### Kubernetes (feature flag: `kubernetes`)
| Module | Description |
|--------|-------------|
| [k8s_configmap](k8s_configmap.md) | Manage Kubernetes ConfigMaps |
| [k8s_deployment](k8s_deployment.md) | Manage Kubernetes deployments |
| [k8s_namespace](k8s_namespace.md) | Manage Kubernetes namespaces |
| [k8s_secret](k8s_secret.md) | Manage Kubernetes Secrets |
| [k8s_service](k8s_service.md) | Manage Kubernetes services |

### Cloud - AWS (feature flag: `aws`)
| Module | Description |
|--------|-------------|
| [aws_ec2](aws_ec2.md) | Manage EC2 instances |
| [aws_iam_policy](aws_iam_policy.md) | Manage IAM policies |
| [aws_iam_role](aws_iam_role.md) | Manage IAM roles |
| [aws_s3](aws_s3.md) | Manage S3 objects |
| [aws_security_group](aws_security_group.md) | Manage security groups |
| [aws_vpc](aws_vpc.md) | Manage VPCs |

### Cloud - Azure (feature flag: `azure`, experimental)
| Module | Description |
|--------|-------------|
| [azure_network_interface](azure_network_interface.md) | Manage network interfaces |
| [azure_resource_group](azure_resource_group.md) | Manage resource groups |
| [azure_vm](azure_vm.md) | Manage Azure VMs |

### Cloud - GCP (feature flag: `gcp`, experimental)
| Module | Description |
|--------|-------------|
| [gcp_compute_firewall](gcp_compute_firewall.md) | Manage firewall rules |
| [gcp_compute_instance](gcp_compute_instance.md) | Manage compute instances |
| [gcp_compute_network](gcp_compute_network.md) | Manage VPC networks |
| [gcp_service_account](gcp_service_account.md) | Manage service accounts |

### Cloud - Proxmox
| Module | Description |
|--------|-------------|
| [proxmox_lxc](proxmox_lxc.md) | Manage Proxmox LXC containers |
| [proxmox_vm](proxmox_vm.md) | Manage Proxmox VMs |

### Network Devices
| Module | Description |
|--------|-------------|
| [eos_config](eos_config.md) | Manage Arista EOS configuration |
| [ios_config](ios_config.md) | Manage Cisco IOS configuration |
| [junos_config](junos_config.md) | Manage Juniper JunOS configuration |
| [nxos_config](nxos_config.md) | Manage Cisco NX-OS configuration |

### Database (feature flag: `database`)
| Module | Description |
|--------|-------------|
| [mysql_db](mysql_db.md) | Manage MySQL databases |
| [mysql_privs](mysql_privs.md) | Manage MySQL privileges |
| [mysql_query](mysql_query.md) | Execute MySQL queries |
| [mysql_user](mysql_user.md) | Manage MySQL users |
| [postgresql_db](postgresql_db.md) | Manage PostgreSQL databases |
| [postgresql_privs](postgresql_privs.md) | Manage PostgreSQL privileges |
| [postgresql_query](postgresql_query.md) | Execute PostgreSQL queries |
| [postgresql_user](postgresql_user.md) | Manage PostgreSQL users |

### Windows (feature flag: `winrm`, experimental)
| Module | Description |
|--------|-------------|
| [win_copy](win_copy.md) | Copy files to Windows hosts |
| [win_feature](win_feature.md) | Manage Windows features |
| [win_package](win_package.md) | Manage Windows packages |
| [win_service](win_service.md) | Manage Windows services |
| [win_user](win_user.md) | Manage Windows user accounts |

### HPC (feature flag: `hpc`)
| Module | Description |
|--------|-------------|
| [beegfs_client](beegfs_client.md) | Manage BeeGFS filesystem client |
| [hpc_baseline](hpc_baseline.md) | Apply HPC baseline configuration |
| [lmod](lmod.md) | Manage Lmod environment modules |
| [lustre_client](lustre_client.md) | Manage Lustre filesystem client |
| [mpi](mpi.md) | Manage MPI installations |
| [nvidia_gpu](nvidia_gpu.md) | Manage NVIDIA GPU configuration |
| [rdma_stack](rdma_stack.md) | Manage RDMA/InfiniBand stack |
| [slurm_config](slurm_config.md) | Manage Slurm configuration |
| [slurm_ops](slurm_ops.md) | Manage Slurm operations |

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
| [meta](meta.md) | Execute internal meta tasks |
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
