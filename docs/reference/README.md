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
- Core (command, shell, debug)
- Files (copy, template, file)
- Packages (apt, yum, dnf, pip)
- System (service, user, group)
- Cloud (AWS, Azure, GCP)

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
