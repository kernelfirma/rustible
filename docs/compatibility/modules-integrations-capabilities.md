# Modules, Integrations & Extensibility

> **Last Updated:** 2026-02-05
> **Rustible Version:** 0.1.x
> **HPC Initiative Phase:** 1C - Module Coverage and Extensibility

This document provides a comprehensive capability matrix for Rustible's built-in modules, external integrations, plugin system, and Ansible fallback behavior.

---

## Quick Summary

| Category | Count | Maturity |
|----------|-------|----------|
| **Core Modules** | 42+ | Stable |
| **Cloud Modules** | 10+ | Stable/Experimental |
| **Container Modules** | 10+ | Stable |
| **Database Modules** | 10 | Stable |
| **Network Modules** | 5 | Experimental |
| **Windows Modules** | 5 | Experimental |
| **Callback Plugins** | 25+ | Stable |
| **Lookup Plugins** | 7+ | Stable |
| **Filter Plugins** | 5+ | Stable |
| **Inventory Plugins** | 6 | Stable |
| **Connection Plugins** | 6+ | Stable |
| **Secret Backends** | 2 | Stable |

---

## 1. Core Modules (System Administration)

### 1.1 File Management

| Module | Ansible Equivalent | Status | Idempotent | Evidence |
|--------|-------------------|--------|------------|----------|
| `file` | `ansible.builtin.file` | ✅ Stable | Yes | [`src/modules/file.rs`](../../src/modules/file.rs) |
| `copy` | `ansible.builtin.copy` | ✅ Stable | Yes | [`src/modules/copy.rs`](../../src/modules/copy.rs) |
| `template` | `ansible.builtin.template` | ✅ Stable | Yes | [`src/modules/template.rs`](../../src/modules/template.rs) |
| `lineinfile` | `ansible.builtin.lineinfile` | ✅ Stable | Yes | [`src/modules/lineinfile.rs`](../../src/modules/lineinfile.rs) |
| `blockinfile` | `ansible.builtin.blockinfile` | ✅ Stable | Yes | [`src/modules/blockinfile.rs`](../../src/modules/blockinfile.rs) |
| `stat` | `ansible.builtin.stat` | ✅ Stable | N/A | [`src/modules/stat.rs`](../../src/modules/stat.rs) |
| `archive` | `community.general.archive` | ✅ Stable | Yes | [`src/modules/archive.rs`](../../src/modules/archive.rs) |
| `unarchive` | `ansible.builtin.unarchive` | ✅ Stable | Yes | [`src/modules/unarchive.rs`](../../src/modules/unarchive.rs) |
| `synchronize` | `ansible.posix.synchronize` | ✅ Stable | Yes | [`src/modules/synchronize.rs`](../../src/modules/synchronize.rs) |

### 1.2 Package Management

| Module | Ansible Equivalent | Status | Platforms | Evidence |
|--------|-------------------|--------|-----------|----------|
| `apt` | `ansible.builtin.apt` | ✅ Stable | Debian/Ubuntu | [`src/modules/apt.rs`](../../src/modules/apt.rs) |
| `yum` | `ansible.builtin.yum` | ✅ Stable | RHEL/CentOS 7 | [`src/modules/yum.rs`](../../src/modules/yum.rs) |
| `dnf` | `ansible.builtin.dnf` | ✅ Stable | RHEL/CentOS 8+, Fedora | [`src/modules/dnf.rs`](../../src/modules/dnf.rs) |
| `package` | `ansible.builtin.package` | ✅ Stable | Cross-platform | [`src/modules/package.rs`](../../src/modules/package.rs) |
| `pip` | `ansible.builtin.pip` | ✅ Stable | Python | [`src/modules/pip.rs`](../../src/modules/pip.rs) |

### 1.3 Service Management

| Module | Ansible Equivalent | Status | Init System | Evidence |
|--------|-------------------|--------|-------------|----------|
| `service` | `ansible.builtin.service` | ✅ Stable | systemd, sysvinit | [`src/modules/service.rs`](../../src/modules/service.rs) |
| `systemd_unit` | `community.general.systemd` | ✅ Stable | systemd | [`src/modules/systemd_unit.rs`](../../src/modules/systemd_unit.rs) |

### 1.4 User & Group Management

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `user` | `ansible.builtin.user` | ✅ Stable | [`src/modules/user.rs`](../../src/modules/user.rs) |
| `group` | `ansible.builtin.group` | ✅ Stable | [`src/modules/group.rs`](../../src/modules/group.rs) |
| `authorized_key` | `ansible.posix.authorized_key` | ✅ Stable | [`src/modules/authorized_key.rs`](../../src/modules/authorized_key.rs) |

### 1.5 System Configuration

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `hostname` | `ansible.builtin.hostname` | ✅ Stable | [`src/modules/hostname.rs`](../../src/modules/hostname.rs) |
| `timezone` | `community.general.timezone` | ✅ Stable | [`src/modules/timezone.rs`](../../src/modules/timezone.rs) |
| `sysctl` | `ansible.posix.sysctl` | ✅ Stable | [`src/modules/sysctl.rs`](../../src/modules/sysctl.rs) |
| `mount` | `ansible.posix.mount` | ✅ Stable | [`src/modules/mount.rs`](../../src/modules/mount.rs) |
| `cron` | `ansible.builtin.cron` | ✅ Stable | [`src/modules/cron.rs`](../../src/modules/cron.rs) |
| `selinux` | `ansible.posix.selinux` | ✅ Stable | [`src/modules/selinux.rs`](../../src/modules/selinux.rs) |

### 1.6 Security

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `firewalld` | `ansible.posix.firewalld` | ✅ Stable | [`src/modules/firewalld.rs`](../../src/modules/firewalld.rs) |
| `ufw` | `community.general.ufw` | ✅ Stable | [`src/modules/ufw.rs`](../../src/modules/ufw.rs) |
| `known_hosts` | `ansible.builtin.known_hosts` | ✅ Stable | [`src/modules/known_hosts.rs`](../../src/modules/known_hosts.rs) |

### 1.7 Command Execution

| Module | Ansible Equivalent | Status | Check Mode | Evidence |
|--------|-------------------|--------|------------|----------|
| `command` | `ansible.builtin.command` | ✅ Stable | `creates`/`removes` | [`src/modules/command.rs`](../../src/modules/command.rs) |
| `shell` | `ansible.builtin.shell` | ✅ Stable | `creates`/`removes` | [`src/modules/shell.rs`](../../src/modules/shell.rs) |
| `raw` | `ansible.builtin.raw` | ✅ Stable | No | [`src/modules/raw.rs`](../../src/modules/raw.rs) |
| `script` | `ansible.builtin.script` | ✅ Stable | `creates`/`removes` | [`src/modules/script.rs`](../../src/modules/script.rs) |

### 1.8 Control Flow & Facts

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `debug` | `ansible.builtin.debug` | ✅ Stable | [`src/modules/debug.rs`](../../src/modules/debug.rs) |
| `fail` | `ansible.builtin.fail` | ✅ Stable | [`src/modules/fail.rs`](../../src/modules/fail.rs) |
| `assert` | `ansible.builtin.assert` | ✅ Stable | [`src/modules/assert.rs`](../../src/modules/assert.rs) |
| `set_fact` | `ansible.builtin.set_fact` | ✅ Stable | [`src/modules/set_fact.rs`](../../src/modules/set_fact.rs) |
| `include_vars` | `ansible.builtin.include_vars` | ✅ Stable | [`src/modules/include_vars.rs`](../../src/modules/include_vars.rs) |
| `meta` | `ansible.builtin.meta` | ✅ Stable | [`src/modules/meta.rs`](../../src/modules/meta.rs) |
| `pause` | `ansible.builtin.pause` | ✅ Stable | [`src/modules/pause.rs`](../../src/modules/pause.rs) |
| `wait_for` | `ansible.builtin.wait_for` | ✅ Stable | [`src/modules/wait_for.rs`](../../src/modules/wait_for.rs) |
| `facts` | `ansible.builtin.setup` | ✅ Stable | [`src/modules/facts.rs`](../../src/modules/facts.rs) |

### 1.9 Source Control

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `git` | `ansible.builtin.git` | ✅ Stable | [`src/modules/git.rs`](../../src/modules/git.rs) |

### 1.10 Network Utilities

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `uri` | `ansible.builtin.uri` | ✅ Stable | [`src/modules/uri.rs`](../../src/modules/uri.rs) |

---

## 2. Cloud Modules

### 2.1 AWS Modules

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `aws_ec2` | `amazon.aws.ec2_instance` | ✅ Stable | [`src/modules/cloud/aws/ec2.rs`](../../src/modules/cloud/aws/ec2.rs) |
| `aws_s3` | `amazon.aws.s3_object` | ✅ Stable | [`src/modules/cloud/aws/s3.rs`](../../src/modules/cloud/aws/s3.rs) |

**Note:** See [provisioning-state-capabilities.md](./provisioning-state-capabilities.md) for 18 AWS provisioning resources.

### 2.2 Azure Modules

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `azure_vm` | `azure.azcollection.azure_rm_virtualmachine` | 🔶 Experimental | [`src/modules/cloud/azure/vm.rs`](../../src/modules/cloud/azure/vm.rs) |

### 2.3 GCP Modules

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `gcp_compute` | `google.cloud.gcp_compute_instance` | 🔶 Experimental | [`src/modules/cloud/gcp/compute.rs`](../../src/modules/cloud/gcp/compute.rs) |

### 2.4 Proxmox Modules

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `proxmox_vm` | `community.general.proxmox_kvm` | ✅ Stable | [`src/modules/proxmox_vm.rs`](../../src/modules/proxmox_vm.rs) |
| `proxmox_lxc` | `community.general.proxmox` | ✅ Stable | [`src/modules/proxmox_lxc.rs`](../../src/modules/proxmox_lxc.rs) |

---

## 3. Container Modules

### 3.1 Docker Modules

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `docker_container` | `community.docker.docker_container` | ✅ Stable | [`src/modules/docker/docker_container.rs`](../../src/modules/docker/docker_container.rs) |
| `docker_image` | `community.docker.docker_image` | ✅ Stable | [`src/modules/docker/docker_image.rs`](../../src/modules/docker/docker_image.rs) |
| `docker_network` | `community.docker.docker_network` | ✅ Stable | [`src/modules/docker/docker_network.rs`](../../src/modules/docker/docker_network.rs) |
| `docker_volume` | `community.docker.docker_volume` | ✅ Stable | [`src/modules/docker/docker_volume.rs`](../../src/modules/docker/docker_volume.rs) |
| `docker_compose` | `community.docker.docker_compose` | ✅ Stable | [`src/modules/docker/docker_compose.rs`](../../src/modules/docker/docker_compose.rs) |

### 3.2 Kubernetes Modules

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `k8s_deployment` | `kubernetes.core.k8s` | ✅ Stable | [`src/modules/k8s/k8s_deployment.rs`](../../src/modules/k8s/k8s_deployment.rs) |
| `k8s_service` | `kubernetes.core.k8s` | ✅ Stable | [`src/modules/k8s/k8s_service.rs`](../../src/modules/k8s/k8s_service.rs) |
| `k8s_configmap` | `kubernetes.core.k8s` | ✅ Stable | [`src/modules/k8s/k8s_configmap.rs`](../../src/modules/k8s/k8s_configmap.rs) |
| `k8s_secret` | `kubernetes.core.k8s` | ✅ Stable | [`src/modules/k8s/k8s_secret.rs`](../../src/modules/k8s/k8s_secret.rs) |
| `k8s_namespace` | `kubernetes.core.k8s` | ✅ Stable | [`src/modules/k8s/k8s_namespace.rs`](../../src/modules/k8s/k8s_namespace.rs) |

---

## 4. Database Modules

### 4.1 PostgreSQL

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `postgresql_db` | `community.postgresql.postgresql_db` | ✅ Stable | [`src/modules/database/postgresql_db.rs`](../../src/modules/database/postgresql_db.rs) |
| `postgresql_user` | `community.postgresql.postgresql_user` | ✅ Stable | [`src/modules/database/postgresql_user.rs`](../../src/modules/database/postgresql_user.rs) |
| `postgresql_privs` | `community.postgresql.postgresql_privs` | ✅ Stable | [`src/modules/database/postgresql_privs.rs`](../../src/modules/database/postgresql_privs.rs) |
| `postgresql_query` | `community.postgresql.postgresql_query` | ✅ Stable | [`src/modules/database/postgresql_query.rs`](../../src/modules/database/postgresql_query.rs) |

### 4.2 MySQL/MariaDB

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `mysql_db` | `community.mysql.mysql_db` | ✅ Stable | [`src/modules/database/mysql_db.rs`](../../src/modules/database/mysql_db.rs) |
| `mysql_user` | `community.mysql.mysql_user` | ✅ Stable | [`src/modules/database/mysql_user.rs`](../../src/modules/database/mysql_user.rs) |
| `mysql_privs` | `community.mysql.mysql_priv` | ✅ Stable | [`src/modules/database/mysql_privs.rs`](../../src/modules/database/mysql_privs.rs) |
| `mysql_query` | `community.mysql.mysql_query` | ✅ Stable | [`src/modules/database/mysql_query.rs`](../../src/modules/database/mysql_query.rs) |

---

## 5. Network Device Modules

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `ios_config` | `cisco.ios.ios_config` | 🔶 Experimental | [`src/modules/network/ios_config.rs`](../../src/modules/network/ios_config.rs) |
| `nxos_config` | `cisco.nxos.nxos_config` | 🔶 Experimental | [`src/modules/network/nxos_config.rs`](../../src/modules/network/nxos_config.rs) |
| `eos_config` | `arista.eos.eos_config` | 🔶 Experimental | [`src/modules/network/eos_config.rs`](../../src/modules/network/eos_config.rs) |
| `junos_config` | `junipernetworks.junos.junos_config` | 🔶 Experimental | [`src/modules/network/junos_config.rs`](../../src/modules/network/junos_config.rs) |

---

## 6. Windows Modules

| Module | Ansible Equivalent | Status | Evidence |
|--------|-------------------|--------|----------|
| `win_copy` | `ansible.windows.win_copy` | 🔶 Experimental | [`src/modules/windows/win_copy.rs`](../../src/modules/windows/win_copy.rs) |
| `win_user` | `ansible.windows.win_user` | 🔶 Experimental | [`src/modules/windows/win_user.rs`](../../src/modules/windows/win_user.rs) |
| `win_service` | `ansible.windows.win_service` | 🔶 Experimental | [`src/modules/windows/win_service.rs`](../../src/modules/windows/win_service.rs) |
| `win_package` | `ansible.windows.win_package` | 🔶 Experimental | [`src/modules/windows/win_package.rs`](../../src/modules/windows/win_package.rs) |
| `win_feature` | `ansible.windows.win_feature` | 🔶 Experimental | [`src/modules/windows/win_feature.rs`](../../src/modules/windows/win_feature.rs) |

---

## 7. Plugin System

### 7.1 Callback Plugins

Output and notification plugins for playbook execution:

| Plugin | Description | Evidence |
|--------|-------------|----------|
| `default` | Standard Ansible-like output | [`src/callback/plugins/default.rs`](../../src/callback/plugins/default.rs) |
| `minimal` | Minimal status output | [`src/callback/plugins/minimal.rs`](../../src/callback/plugins/minimal.rs) |
| `dense` | Compact single-line output | [`src/callback/plugins/dense.rs`](../../src/callback/plugins/dense.rs) |
| `oneline` | One line per task | [`src/callback/plugins/oneline.rs`](../../src/callback/plugins/oneline.rs) |
| `json` | JSON output format | [`src/callback/plugins/json.rs`](../../src/callback/plugins/json.rs) |
| `yaml` | YAML output format | [`src/callback/plugins/yaml.rs`](../../src/callback/plugins/yaml.rs) |
| `debug` | Detailed debug output | [`src/callback/plugins/debug.rs`](../../src/callback/plugins/debug.rs) |
| `tree` | Directory tree output | [`src/callback/plugins/tree.rs`](../../src/callback/plugins/tree.rs) |
| `timer` | Task timing information | [`src/callback/plugins/timer.rs`](../../src/callback/plugins/timer.rs) |
| `profile_tasks` | Task profiling | [`src/callback/plugins/profile_tasks.rs`](../../src/callback/plugins/profile_tasks.rs) |
| `stats` | Statistics summary | [`src/callback/plugins/stats.rs`](../../src/callback/plugins/stats.rs) |
| `counter` | Task counter display | [`src/callback/plugins/counter.rs`](../../src/callback/plugins/counter.rs) |
| `progress` | Progress bar display | [`src/callback/plugins/progress.rs`](../../src/callback/plugins/progress.rs) |
| `junit` | JUnit XML output | [`src/callback/plugins/junit.rs`](../../src/callback/plugins/junit.rs) |
| `logfile` | File logging | [`src/callback/plugins/logfile.rs`](../../src/callback/plugins/logfile.rs) |
| `logstash` | Logstash output | [`src/callback/plugins/logstash.rs`](../../src/callback/plugins/logstash.rs) |
| `splunk` | Splunk HEC integration | [`src/callback/plugins/splunk.rs`](../../src/callback/plugins/splunk.rs) |
| `syslog` | Syslog integration | [`src/callback/plugins/syslog.rs`](../../src/callback/plugins/syslog.rs) |
| `slack` | Slack notifications | [`src/callback/plugins/slack.rs`](../../src/callback/plugins/slack.rs) |
| `mail` | Email notifications | [`src/callback/plugins/mail.rs`](../../src/callback/plugins/mail.rs) |
| `notification` | Generic notifications | [`src/callback/plugins/notification.rs`](../../src/callback/plugins/notification.rs) |
| `diff` | Show diffs | [`src/callback/plugins/diff.rs`](../../src/callback/plugins/diff.rs) |
| `selective` | Filter output | [`src/callback/plugins/selective.rs`](../../src/callback/plugins/selective.rs) |
| `skippy` | Skip unchanged | [`src/callback/plugins/skippy.rs`](../../src/callback/plugins/skippy.rs) |
| `actionable` | Show only actionable | [`src/callback/plugins/actionable.rs`](../../src/callback/plugins/actionable.rs) |

### 7.2 Lookup Plugins

| Plugin | Description | Evidence |
|--------|-------------|----------|
| `file` | Read file contents | [`src/plugins/lookup/file.rs`](../../src/plugins/lookup/file.rs) |
| `env` | Environment variables | [`src/plugins/lookup/env.rs`](../../src/plugins/lookup/env.rs) |
| `pipe` | Command output | [`src/plugins/lookup/pipe.rs`](../../src/plugins/lookup/pipe.rs) |
| `template` | Template rendering | [`src/plugins/lookup/template.rs`](../../src/plugins/lookup/template.rs) |
| `password` | Password generation | [`src/plugins/lookup/password.rs`](../../src/plugins/lookup/password.rs) |
| `csvfile` | CSV file lookup | [`src/plugins/lookup/csvfile.rs`](../../src/plugins/lookup/csvfile.rs) |
| `vault` | Vault secret lookup | [`src/lookup/vault.rs`](../../src/lookup/vault.rs) |

### 7.3 Filter Plugins

| Plugin | Description | Evidence |
|--------|-------------|----------|
| `collections` | List/dict manipulation | [`src/plugins/filter/collections.rs`](../../src/plugins/filter/collections.rs) |
| `encoding` | Base64, URL encoding | [`src/plugins/filter/encoding.rs`](../../src/plugins/filter/encoding.rs) |
| `hash` | MD5, SHA hashing | [`src/plugins/filter/hash.rs`](../../src/plugins/filter/hash.rs) |
| `regex` | Regex operations | [`src/plugins/filter/regex.rs`](../../src/plugins/filter/regex.rs) |
| `serialization` | JSON/YAML conversion | [`src/plugins/filter/serialization.rs`](../../src/plugins/filter/serialization.rs) |

### 7.4 Inventory Plugins

| Plugin | Description | Evidence |
|--------|-------------|----------|
| `ini` | INI format inventory | [`src/inventory/mod.rs`](../../src/inventory/mod.rs) |
| `yaml` | YAML format inventory | [`src/inventory/mod.rs`](../../src/inventory/mod.rs) |
| `aws_ec2` | AWS EC2 dynamic inventory | [`src/inventory/plugins/aws_ec2.rs`](../../src/inventory/plugins/aws_ec2.rs) |
| `azure` | Azure dynamic inventory | [`src/inventory/plugins/azure.rs`](../../src/inventory/plugins/azure.rs) |
| `gcp` | GCP dynamic inventory | [`src/inventory/plugins/gcp.rs`](../../src/inventory/plugins/gcp.rs) |
| `proxmox` | Proxmox dynamic inventory | [`src/inventory/plugins/proxmox.rs`](../../src/inventory/plugins/proxmox.rs) |
| `terraform` | Terraform state inventory | [`src/inventory/plugins/terraform.rs`](../../src/inventory/plugins/terraform.rs) |
| `constructed` | Constructed groups | [`src/inventory/constructed.rs`](../../src/inventory/constructed.rs) |

### 7.5 Connection Plugins

| Plugin | Protocol | Status | Evidence |
|--------|----------|--------|----------|
| `ssh` | SSH (russh native) | ✅ Stable | [`src/connection/russh.rs`](../../src/connection/russh.rs) |
| `local` | Local execution | ✅ Stable | [`src/connection/local.rs`](../../src/connection/local.rs) |
| `docker` | Docker exec | ✅ Stable | [`src/connection/docker.rs`](../../src/connection/docker.rs) |
| `kubernetes` | kubectl exec | ✅ Stable | [`src/connection/kubernetes.rs`](../../src/connection/kubernetes.rs) |
| `winrm` | WinRM (Windows) | 🔶 Experimental | [`src/connection/winrm.rs`](../../src/connection/winrm.rs) |
| `jump_host` | SSH via bastion | ✅ Stable | [`src/connection/jump_host.rs`](../../src/connection/jump_host.rs) |

---

## 8. Secret Management

### 8.1 Secret Backends

| Backend | Features | Status | Evidence |
|---------|----------|--------|----------|
| **HashiCorp Vault** | KV v1/v2, Transit, Dynamic credentials | ✅ Stable | [`src/secrets/hashicorp_vault.rs`](../../src/secrets/hashicorp_vault.rs) |
| **AWS Secrets Manager** | Secret retrieval, rotation | ✅ Stable | [`src/secrets/aws_secrets_manager.rs`](../../src/secrets/aws_secrets_manager.rs) |

### 8.2 Vault Authentication Methods

| Method | Description |
|--------|-------------|
| Token | Direct token authentication |
| AppRole | Machine-to-machine auth |
| Kubernetes | K8s service account auth |
| LDAP | LDAP credentials auth |
| AWS IAM | AWS IAM role auth |

### 8.3 Secret Features

| Feature | Status | Description |
|---------|--------|-------------|
| TTL Caching | ✅ Stable | Reduce API calls |
| Secret Rotation | ✅ Stable | Automatic/manual rotation |
| No-log Enforcement | ✅ Stable | Redact sensitive data |
| Transit Encryption | ✅ Stable | Vault encryption-as-a-service |

---

## 9. Ansible Module Fallback

### 9.1 Python Module Executor

Rustible includes a Python module executor for backward compatibility with the entire Ansible module ecosystem:

```rust
// PythonModuleExecutor automatically:
// 1. Finds Ansible module Python files
// 2. Bundles with AnsiballZ format
// 3. Transfers via SSH
// 4. Executes with Python interpreter
// 5. Parses JSON result
```

**Source:** [`src/modules/python.rs`](../../src/modules/python.rs)

### 9.2 Module Search Paths

```
~/.ansible/collections
~/.ansible/plugins/modules
/usr/share/ansible/plugins/modules
/usr/lib/python3/dist-packages/ansible/modules
$ANSIBLE_LIBRARY
```

### 9.3 Supported Module Name Formats

| Format | Example |
|--------|---------|
| Short name | `apt` |
| FQCN | `ansible.builtin.apt` |
| Collection | `community.general.ufw` |

### 9.4 Fallback Behavior

1. Check if native Rustible module exists
2. If not, search for Ansible Python module
3. If found, execute via PythonModuleExecutor
4. Parse JSON result into ModuleOutput

---

## 10. HPC-Specific Module Gaps

### 10.1 Critical Gaps (High Priority)

| Module Category | Gap | HPC Impact | Workaround |
|----------------|-----|------------|------------|
| **Slurm** | No native Slurm modules | Job submission, node management | Python fallback or shell |
| **PBS Pro** | No PBS modules | Job scheduling | Python fallback |
| **InfiniBand** | No IB modules | Network configuration | Shell commands |
| **Lustre** | No Lustre modules | Filesystem mounts | Use `mount` module |
| **GPFS** | No GPFS modules | Filesystem management | Shell commands |
| **MPI** | No MPI modules | Application deployment | Shell scripts |

### 10.2 Medium Priority Gaps

| Module Category | Gap | Workaround |
|----------------|-----|------------|
| **LDAP** | No LDAP modules | Python fallback |
| **SSSD** | No SSSD modules | Template/lineinfile |
| **Kerberos** | No Kerberos modules | Shell commands |
| **NFS** | Limited NFS module | Use `mount` module |
| **BeeGFS** | No BeeGFS modules | Shell commands |

### 10.3 Low Priority Gaps

| Module Category | Gap | Workaround |
|----------------|-----|------------|
| **NVIDIA DCGM** | No GPU monitoring modules | Shell + dcgmi |
| **ROCm** | No AMD GPU modules | Shell commands |
| **Mellanox** | No OFED modules | Shell commands |

---

## 11. Extension Points

### 11.1 Custom Module Development

```rust
use rustible::modules::{Module, ModuleContext, ModuleOutput, ModuleParams, ModuleResult};

#[derive(Debug)]
pub struct MyModule;

impl Module for MyModule {
    fn name(&self) -> &'static str { "my_module" }

    fn execute(&self, params: &ModuleParams, ctx: &ModuleContext) -> ModuleResult<ModuleOutput> {
        // Implementation
    }
}
```

### 11.2 Custom Callback Plugin

```rust
use rustible::callback::{CallbackPlugin, CallbackEvent};

pub struct MyCallback;

impl CallbackPlugin for MyCallback {
    fn name(&self) -> &'static str { "my_callback" }

    fn on_event(&mut self, event: CallbackEvent) {
        // Handle event
    }
}
```

### 11.3 Custom Inventory Plugin

```rust
use rustible::inventory::{InventoryPlugin, InventoryHost};

pub struct MyInventory;

impl InventoryPlugin for MyInventory {
    fn name(&self) -> &'static str { "my_inventory" }

    async fn get_hosts(&self) -> Result<Vec<InventoryHost>, Error> {
        // Fetch hosts
    }
}
```

---

## 12. Summary Statistics

### 12.1 Module Coverage by Category

| Category | Native Modules | Ansible Parity |
|----------|---------------|----------------|
| File Operations | 9 | ~100% |
| Package Management | 5 | ~100% |
| Service Management | 2 | ~100% |
| User/Group | 3 | ~100% |
| System Config | 6 | ~80% |
| Security | 3 | ~60% |
| Command Exec | 4 | ~100% |
| Control Flow | 9 | ~100% |
| Docker | 5 | ~90% |
| Kubernetes | 5 | ~70% |
| Database | 8 | ~80% |
| Cloud AWS | 2 + 18 provisioning | ~30% |
| Network | 4 | ~10% |
| Windows | 5 | ~20% |

### 12.2 Total Module Count

- **Native Modules:** 70+
- **Ansible Fallback:** Unlimited (any Python module)
- **Callback Plugins:** 25+
- **Lookup Plugins:** 7+
- **Inventory Plugins:** 8

---

*For execution capabilities, see [execution-reliability-capabilities.md](./execution-reliability-capabilities.md)*
*For provisioning capabilities, see [provisioning-state-capabilities.md](./provisioning-state-capabilities.md)*
