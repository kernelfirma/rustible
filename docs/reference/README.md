---
summary: API reference for using Rustible as a Rust library, including executor, inventory, and playbook interfaces.
read_when: You want to embed Rustible in your own Rust application or extend its functionality programmatically.
---

# API Reference

## Library Usage

```rust
use rustible::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let inventory = Inventory::from_file("inventory.yml").await?;
    let playbook = Playbook::from_file("playbook.yml").await?;

    let executor = PlaybookExecutor::new()
        .with_inventory(inventory)
        .with_parallelism(10)
        .build()?;

    executor.run(&playbook).await?;
    Ok(())
}
```

## Core Components

| Component | Description |
|-----------|-------------|
| [Inventory](inventory.md) | Host and group management |
| [Variables](variables.md) | Variable system and precedence |
| [Modules](modules.md) | Module API and custom modules |
| [Callbacks](callbacks.md) | Output and event handling |

## Modules

See [modules/](modules/) for full module documentation.

**Categories:**
- Core (command, shell, debug, assert, fail, set_fact, meta, pause, wait_for)
- Files (copy, template, file, lineinfile, blockinfile, archive, unarchive, stat, synchronize)
- Packages (apt, yum, dnf, pip, package)
- System (service, systemd_unit, user, group, cron, hostname, sysctl, mount, timezone)
- Security (authorized_key, known_hosts, ufw, firewalld, selinux)
- Network (uri)
- Docker (docker_container, docker_image, docker_network, docker_volume, docker_compose)
- Kubernetes (k8s_namespace, k8s_deployment, k8s_service, k8s_configmap, k8s_secret)
- Cloud - AWS (aws_ec2, aws_s3, aws_vpc, aws_security_group, aws_iam_role, aws_iam_policy)
- Cloud - Azure (azure_vm, azure_resource_group, azure_network_interface)
- Cloud - GCP (gcp_compute_instance, gcp_compute_firewall, gcp_compute_network, gcp_service_account)
- Cloud - Proxmox (proxmox_lxc, proxmox_vm)
- Network Devices (ios_config, eos_config, junos_config, nxos_config)
- Database (postgresql_db, postgresql_user, mysql_db, mysql_user, and more)
- Windows (win_copy, win_feature, win_service, win_package, win_user)
- HPC (slurm_config, nvidia_gpu, lmod, mpi, lustre_client, and more)

## Connections

```rust
// SSH
let conn = RusshConnection::new("host", 22, "user", key_path).await?;

// Local
let conn = LocalConnection::new();

// Docker
let conn = DockerConnection::new("container_name");
```

## Vault

```rust
let vault = Vault::new("password");
let encrypted = vault.encrypt("secret data")?;
let decrypted = vault.decrypt(&encrypted)?;
```

## Templates

```rust
let engine = TemplateEngine::new();
let result = engine.render("Hello {{ name }}", &vars)?;
```
