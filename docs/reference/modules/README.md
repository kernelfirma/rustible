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
| [aws_ebs_volume](aws_ebs_volume.md) | Manage EBS volumes |
| [aws_iam_policy](aws_iam_policy.md) | Manage IAM policies |
| [aws_iam_role](aws_iam_role.md) | Manage IAM roles |
| [aws_s3](aws_s3.md) | Manage S3 objects |
| [aws_security_group](aws_security_group.md) | Manage security groups |
| [aws_security_group_rule](aws_security_group_rule.md) | Manage standalone security group rules |
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

### Windows (feature flag: `winrm`, Beta / Partial)
| Module | Description |
|--------|-------------|
| [win_copy](win_copy.md) | Copy files to Windows hosts |
| [win_feature](win_feature.md) | Manage Windows features |
| [win_package](win_package.md) | Manage Windows packages |
| [win_service](win_service.md) | Manage Windows services |
| [win_user](win_user.md) | Manage Windows user accounts |

### HPC Core (feature flag: `hpc`)
| Module | Description |
|--------|-------------|
| [hpc_baseline](hpc_baseline.md) | Apply HPC baseline configuration (limits, sysctl, directories) |
| [munge](munge.md) | Manage MUNGE authentication service |
| [hpc_nfs](hpc_nfs.md) | Manage NFS server and client for HPC shared storage |
| [hpc_facts](hpc_facts.md) | Gather HPC-specific facts (CPU, NUMA, GPU, InfiniBand) |
| [hpc_healthcheck](hpc_healthcheck.md) | Validate HPC node health and readiness |
| [lmod](lmod.md) | Manage Lmod environment modules |
| [mpi](mpi.md) | Manage MPI library installations (OpenMPI, Intel MPI) |
| [hpc_toolchain](hpc_toolchain.md) | Manage HPC compiler and toolchain installations |
| [hpc_discovery](hpc_discovery.md) | Discover and inventory HPC cluster resources |
| [hpc_power](hpc_power.md) | Manage HPC node power states |
| [boot_profile](boot_profile.md) | Manage HPC node boot profiles and configurations |
| [image_pipeline](image_pipeline.md) | Manage HPC node image build pipelines |
| [ipmi_power](ipmi_power.md) | IPMI-based power control for bare-metal nodes |
| [ipmi_boot](ipmi_boot.md) | IPMI-based boot device management |
| [hpc_scheduler](hpc_scheduler.md) | Abstract scheduler interface (scheduler-agnostic) |
| [hpc_job](hpc_job.md) | Abstract job management (scheduler-agnostic) |
| [hpc_queue](hpc_queue.md) | Abstract queue management (scheduler-agnostic) |
| [hpc_server](hpc_server.md) | Abstract scheduler server management |

### HPC Slurm (feature flag: `slurm`)
| Module | Description |
|--------|-------------|
| [slurm_config](slurm_config.md) | Manage Slurm configuration files |
| [slurm_ops](slurm_ops.md) | Manage Slurm daemon operations |
| [slurm_node](slurm_node.md) | Manage Slurm compute node state |
| [slurm_partition](slurm_partition.md) | Manage Slurm partition definitions |
| [slurm_account](slurm_account.md) | Manage Slurm accounts via sacctmgr |
| [slurm_qos](slurm_qos.md) | Manage Slurm QoS definitions |
| [slurm_job](slurm_job.md) | Manage Slurm job submission and control |
| [slurm_queue](slurm_queue.md) | Query and manage Slurm job queues |
| [slurm_info](slurm_info.md) | Gather Slurm cluster information |
| [slurmrestd](slurmrestd.md) | Manage Slurm REST API daemon |
| [scheduler_orchestration](scheduler_orchestration.md) | Orchestrate complex Slurm scheduling workflows |
| [partition_policy](partition_policy.md) | Manage Slurm partition access policies |

### HPC PBS (feature flag: `pbs`)
| Module | Description |
|--------|-------------|
| [pbs_job](pbs_job.md) | Manage PBS Pro job submission and control |
| [pbs_queue](pbs_queue.md) | Manage PBS Pro queue definitions |
| [pbs_server](pbs_server.md) | Manage PBS Pro server configuration |

### HPC GPU (feature flag: `gpu`)
| Module | Description |
|--------|-------------|
| [nvidia_gpu](nvidia_gpu.md) | Manage NVIDIA GPU configuration and monitoring |
| [nvidia_driver](nvidia_driver.md) | Manage NVIDIA driver installation lifecycle |
| [cuda](cuda.md) | Manage CUDA toolkit installation |

### HPC OFED / InfiniBand (feature flag: `ofed`)
| Module | Description |
|--------|-------------|
| [rdma_stack](rdma_stack.md) | Manage RDMA/InfiniBand/OFED stack |
| [opensm](opensm.md) | Manage OpenSM subnet manager configuration |
| [ib_partition](ib_partition.md) | Manage InfiniBand partition keys |
| [ib_diagnostics](ib_diagnostics.md) | Run InfiniBand fabric diagnostics |
| [ipoib](ipoib.md) | Manage IPoIB (IP over InfiniBand) interfaces |

### HPC Parallel Filesystems (feature flag: `parallel_fs`)
| Module | Description |
|--------|-------------|
| [lustre_client](lustre_client.md) | Manage Lustre filesystem client |
| [lustre_mount](lustre_mount.md) | Manage Lustre mounts with LNet awareness |
| [lustre_ost](lustre_ost.md) | Manage Lustre OST lifecycle |
| [beegfs_client](beegfs_client.md) | Manage BeeGFS filesystem client |

### HPC Identity (feature flag: `identity`)
| Module | Description |
|--------|-------------|
| [kerberos](kerberos.md) | Manage Kerberos client configuration |
| [sssd_config](sssd_config.md) | Manage SSSD service configuration |
| [sssd_domain](sssd_domain.md) | Manage SSSD domain definitions |

### HPC Bare-Metal (feature flag: `bare_metal`)
| Module | Description |
|--------|-------------|
| [pxe_profile](pxe_profile.md) | Manage PXE boot profiles |
| [pxe_host](pxe_host.md) | Manage PXE host registrations |
| [warewulf_node](warewulf_node.md) | Manage Warewulf node definitions |
| [warewulf_image](warewulf_image.md) | Manage Warewulf VNFS images |

### HPC Redfish (feature flag: `redfish`)
| Module | Description |
|--------|-------------|
| [redfish_power](redfish_power.md) | Manage server power via Redfish BMC |
| [redfish_info](redfish_info.md) | Gather server hardware info via Redfish |

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
