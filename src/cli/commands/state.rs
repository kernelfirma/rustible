//! State management command
//!
//! This module implements the `state` subcommand for managing state.
//!
//! ## Commands
//!
//! - `init` - Initialize state with backend configuration
//! - `migrate` - Migrate state between backends
//! - `import-terraform` - Import Terraform state into Rustible format
//! - `list` - List available states
//! - `show` - Show state details
//! - `pull` - Pull remote state to local
//! - `push` - Push local state to remote
//! - `rm` - Remove a state entry
//! - `lock` - Manage state locks

use super::CommandContext;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Arguments for the state command
#[derive(Parser, Debug, Clone)]
pub struct StateArgs {
    /// State subcommand
    #[command(subcommand)]
    pub command: StateCommand,
}

/// Backend type for state storage
#[derive(Debug, Clone, ValueEnum)]
pub enum BackendType {
    /// Local file-based storage
    Local,
    /// AWS S3 with optional DynamoDB locking
    S3,
    /// Google Cloud Storage
    Gcs,
    /// Azure Blob Storage
    Azure,
    /// HashiCorp Consul KV
    Consul,
    /// HTTP backend (Terraform Cloud compatible)
    Http,
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendType::Local => write!(f, "local"),
            BackendType::S3 => write!(f, "s3"),
            BackendType::Gcs => write!(f, "gcs"),
            BackendType::Azure => write!(f, "azure"),
            BackendType::Consul => write!(f, "consul"),
            BackendType::Http => write!(f, "http"),
        }
    }
}

/// State subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum StateCommand {
    /// Initialize state with backend configuration
    Init {
        /// Backend type
        #[arg(long, value_enum, default_value = "local")]
        backend: BackendType,

        /// State file path (for local backend)
        #[arg(long, default_value = ".rustible/provisioning.state.json")]
        path: PathBuf,

        /// S3 bucket name (for s3 backend)
        #[arg(long)]
        bucket: Option<String>,

        /// Object key/path within bucket (for s3/gcs/azure backends)
        #[arg(long)]
        key: Option<String>,

        /// AWS region (for s3 backend)
        #[arg(long)]
        region: Option<String>,

        /// DynamoDB table for locking (for s3 backend)
        #[arg(long)]
        dynamodb_table: Option<String>,

        /// Storage account name (for azure backend)
        #[arg(long)]
        storage_account: Option<String>,

        /// Container name (for azure backend)
        #[arg(long)]
        container: Option<String>,

        /// Consul/HTTP address
        #[arg(long)]
        address: Option<String>,

        /// Force reconfiguration if already initialized
        #[arg(long)]
        reconfigure: bool,
    },

    /// Migrate state from one backend to another
    Migrate {
        /// Source backend type
        #[arg(long, value_enum)]
        from: BackendType,

        /// Destination backend type
        #[arg(long, value_enum)]
        to: BackendType,

        /// Source path/URL
        #[arg(long)]
        from_path: String,

        /// Destination path/URL
        #[arg(long)]
        to_path: String,

        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },

    /// Import Terraform state into Rustible format
    #[command(name = "import-terraform")]
    ImportTerraform {
        /// Path to Terraform state file (terraform.tfstate)
        #[arg(long)]
        tfstate: PathBuf,

        /// Output path for Rustible state
        #[arg(long, default_value = ".rustible/provisioning.state.json")]
        output: PathBuf,

        /// Overwrite existing state
        #[arg(long)]
        force: bool,
    },

    /// List available states
    List {
        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,

        /// Output format (table, json)
        #[arg(long, default_value = "table")]
        format: String,
    },

    /// Show details of a state
    Show {
        /// State name/key
        name: String,

        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,
    },

    /// Pull remote state to local
    Pull {
        /// Remote backend URL
        #[arg(long)]
        backend: String,

        /// Local output path
        #[arg(long, default_value = ".rustible/state")]
        output: PathBuf,
    },

    /// Push local state to remote
    Push {
        /// Local state directory
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,

        /// Remote backend URL
        #[arg(long)]
        backend: String,
    },

    /// Remove a state entry
    #[command(name = "rm")]
    Remove {
        /// State name/key to remove
        name: String,

        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,

        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },

    /// Move a resource to a new address in state
    Mv {
        /// Source resource address (e.g., aws_vpc.old_name)
        source: String,

        /// Destination resource address (e.g., aws_vpc.new_name)
        destination: String,

        /// Path to state file
        #[arg(long, default_value = ".rustible/provisioning.state.json")]
        state: PathBuf,

        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },

    /// Replace provider for resources in state
    ReplaceProvider {
        /// Old provider name (e.g., registry.terraform.io/hashicorp/aws)
        from_provider: String,

        /// New provider name (e.g., aws)
        to_provider: String,

        /// Path to state file
        #[arg(long, default_value = ".rustible/provisioning.state.json")]
        state: PathBuf,

        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },

    /// Manage state locks
    Lock {
        /// Lock subcommand
        #[command(subcommand)]
        command: LockCommand,
    },
}

/// Lock management subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum LockCommand {
    /// List active locks
    List {
        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,
    },

    /// Force-release a lock
    Release {
        /// Lock ID to release
        lock_id: String,

        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,
    },
}

impl StateArgs {
    /// Execute the state command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            StateCommand::Init {
                backend,
                path,
                bucket,
                key,
                region,
                dynamodb_table,
                storage_account,
                container,
                address,
                reconfigure,
            } => {
                ctx.output.banner("STATE INIT");

                // Check if state config already exists
                let config_path = PathBuf::from(".rustible/backend.json");
                if config_path.exists() && !reconfigure {
                    ctx.output
                        .warning("Backend already configured. Use --reconfigure to overwrite.");
                    return Ok(1);
                }

                // Create config directory
                std::fs::create_dir_all(".rustible")?;

                // Build backend configuration
                let backend_config = match backend {
                    BackendType::Local => {
                        ctx.output
                            .info(&format!("Initializing local backend at {:?}", path));
                        serde_json::json!({
                            "type": "local",
                            "path": path.to_string_lossy()
                        })
                    }
                    BackendType::S3 => {
                        let bucket = bucket.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("--bucket is required for S3 backend")
                        })?;
                        let key = key
                            .as_ref()
                            .map(|k| k.as_str())
                            .unwrap_or("terraform.tfstate");
                        let region = region.as_ref().map(|r| r.as_str()).unwrap_or("us-east-1");

                        ctx.output
                            .info(&format!("Initializing S3 backend: s3://{}/{}", bucket, key));

                        let mut config = serde_json::json!({
                            "type": "s3",
                            "bucket": bucket,
                            "key": key,
                            "region": region,
                            "encrypt": true
                        });

                        if let Some(table) = dynamodb_table {
                            config["dynamodb_table"] = serde_json::json!(table);
                            ctx.output
                                .info(&format!("  DynamoDB locking enabled: {}", table));
                        }

                        config
                    }
                    BackendType::Gcs => {
                        let bucket = bucket.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("--bucket is required for GCS backend")
                        })?;
                        let key = key
                            .as_ref()
                            .map(|k| k.as_str())
                            .unwrap_or("terraform.tfstate");

                        ctx.output.info(&format!(
                            "Initializing GCS backend: gs://{}/{}",
                            bucket, key
                        ));

                        serde_json::json!({
                            "type": "gcs",
                            "bucket": bucket,
                            "key": key
                        })
                    }
                    BackendType::Azure => {
                        let storage_account = storage_account.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("--storage-account is required for Azure backend")
                        })?;
                        let container = container.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("--container is required for Azure backend")
                        })?;
                        let blob_name = key
                            .as_ref()
                            .map(|k| k.as_str())
                            .unwrap_or("terraform.tfstate");

                        ctx.output.info(&format!(
                            "Initializing Azure backend: [redacted]/{}/{}",
                            container, blob_name
                        ));

                        serde_json::json!({
                            "type": "azurerm",
                            "storage_account_name": storage_account,
                            "container_name": container,
                            "key": blob_name
                        })
                    }
                    BackendType::Consul => {
                        let addr = address
                            .as_ref()
                            .map(|a| a.as_str())
                            .unwrap_or("http://127.0.0.1:8500");
                        let path = key.as_ref().map(|k| k.as_str()).unwrap_or("rustible/state");

                        ctx.output.info(&format!(
                            "Initializing Consul backend: {}/v1/kv/{}",
                            addr, path
                        ));

                        serde_json::json!({
                            "type": "consul",
                            "address": addr,
                            "path": path
                        })
                    }
                    BackendType::Http => {
                        let addr = address.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("--address is required for HTTP backend")
                        })?;

                        ctx.output
                            .info(&format!("Initializing HTTP backend: {}", addr));

                        serde_json::json!({
                            "type": "http",
                            "address": addr
                        })
                    }
                };

                // Write config
                let config_content = serde_json::to_string_pretty(&backend_config)?;
                std::fs::write(&config_path, &config_content)?;

                ctx.output
                    .info(&format!("Backend configuration saved to {:?}", config_path));
                ctx.output.info("");
                ctx.output.info("Successfully configured the backend!");
                ctx.output
                    .info("You may now begin working with Rustible provisioning.");

                Ok(0)
            }

            StateCommand::Migrate {
                from,
                to,
                from_path,
                to_path,
                force,
            } => {
                ctx.output.banner("STATE MIGRATE");
                ctx.output
                    .info(&format!("Migrating state from {} to {}", from, to));
                ctx.output.info(&format!("  Source: {}", from_path));
                ctx.output.info(&format!("  Destination: {}", to_path));

                if !force {
                    ctx.output
                        .warning("This will copy state data. Use --force to confirm.");
                    return Ok(1);
                }

                // Load source state
                let source_content = match from {
                    BackendType::Local => {
                        let path = PathBuf::from(from_path);
                        if !path.exists() {
                            ctx.output
                                .error(&format!("Source state not found: {:?}", path));
                            return Ok(1);
                        }
                        std::fs::read_to_string(&path)?
                    }
                    _ => {
                        ctx.output.error(&format!(
                            "Migration from {} backend requires async support. Use pull/push commands.",
                            from
                        ));
                        return Ok(1);
                    }
                };

                // Parse state to validate
                let state: serde_json::Value = serde_json::from_str(&source_content)?;

                // Save to destination
                match to {
                    BackendType::Local => {
                        let path = PathBuf::from(to_path);
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let content = serde_json::to_string_pretty(&state)?;
                        std::fs::write(&path, content)?;
                        ctx.output.info(&format!("State migrated to {:?}", path));
                    }
                    _ => {
                        ctx.output.error(&format!(
                            "Migration to {} backend requires async support. Use pull/push commands.",
                            to
                        ));
                        return Ok(1);
                    }
                }

                ctx.output.info("Migration completed successfully.");
                Ok(0)
            }

            StateCommand::ImportTerraform {
                tfstate,
                output,
                force,
            } => {
                ctx.output.banner("IMPORT TERRAFORM STATE");
                ctx.output.info(&format!("Importing from: {:?}", tfstate));
                ctx.output.info(&format!("Output to: {:?}", output));

                // Check source exists
                if !tfstate.exists() {
                    ctx.output
                        .error(&format!("Terraform state file not found: {:?}", tfstate));
                    return Ok(1);
                }

                // Check destination doesn't exist (unless force)
                if output.exists() && !force {
                    ctx.output
                        .warning("Output file already exists. Use --force to overwrite.");
                    return Ok(1);
                }

                // Read Terraform state
                let tf_content = std::fs::read_to_string(tfstate)?;
                let tf_state: serde_json::Value = serde_json::from_str(&tf_content)?;

                // Validate it's a Terraform state file
                if tf_state.get("version").is_none() {
                    ctx.output
                        .error("Invalid Terraform state file: missing version field");
                    return Ok(1);
                }

                let tf_version = tf_state
                    .get("terraform_version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let serial = tf_state.get("serial").and_then(|v| v.as_u64()).unwrap_or(0);

                ctx.output
                    .info(&format!("  Terraform version: {}", tf_version));
                ctx.output.info(&format!("  State serial: {}", serial));

                // Import using the provisioning module (feature-gated)
                #[cfg(feature = "provisioning")]
                {
                    use rustible::provisioning::state::ProvisioningState;

                    let rustible_state = ProvisioningState::import_from_terraform(&tf_state)
                        .map_err(|e| anyhow::anyhow!("Failed to import state: {}", e))?;

                    // Report what was imported
                    ctx.output.info(&format!(
                        "  Resources imported: {}",
                        rustible_state.resource_count()
                    ));
                    ctx.output.info(&format!(
                        "  Outputs imported: {}",
                        rustible_state.outputs.len()
                    ));

                    // Create output directory if needed
                    if let Some(parent) = output.parent() {
                        std::fs::create_dir_all(parent)?;
                    }

                    // Save Rustible state
                    let content = serde_json::to_string_pretty(&rustible_state)?;
                    std::fs::write(output, content)?;

                    ctx.output.info("");
                    ctx.output.info("Successfully imported Terraform state!");
                    ctx.output.info(
                        "You can now use 'rustible provision plan' to see the current state.",
                    );
                }

                #[cfg(not(feature = "provisioning"))]
                {
                    // Fallback: simple JSON-to-JSON conversion for basic import
                    let resources = tf_state
                        .get("resources")
                        .and_then(|r| r.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let outputs = tf_state
                        .get("outputs")
                        .and_then(|o| o.as_object())
                        .map(|o| o.len())
                        .unwrap_or(0);

                    ctx.output
                        .info(&format!("  Resources found: {}", resources));
                    ctx.output.info(&format!("  Outputs found: {}", outputs));

                    // Create output directory if needed
                    if let Some(parent) = output.parent() {
                        std::fs::create_dir_all(parent)?;
                    }

                    // Convert to Rustible state format
                    let rustible_state = convert_terraform_state(&tf_state);
                    let content = serde_json::to_string_pretty(&rustible_state)?;
                    std::fs::write(output, content)?;

                    ctx.output.info("");
                    ctx.output.info("Successfully imported Terraform state!");
                    ctx.output
                        .info("Note: Enable 'provisioning' feature for full state management.");
                }

                Ok(0)
            }

            StateCommand::List { state_dir, format } => {
                ctx.output.banner("STATE LIST");
                let state_path = state_dir
                    .canonicalize()
                    .unwrap_or_else(|_| state_dir.clone());

                if !state_path.exists() {
                    ctx.output
                        .info("No state directory found. Run a playbook first to generate state.");
                    return Ok(0);
                }

                let mut count = 0;
                if let Ok(entries) = std::fs::read_dir(&state_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(false, |ext| ext == "json") {
                            let name = path.file_stem().unwrap_or_default().to_string_lossy();
                            let metadata = std::fs::metadata(&path).ok();
                            let size = metadata.as_ref().map_or(0, |m| m.len());
                            let modified = metadata
                                .and_then(|m| m.modified().ok())
                                .map(|t| {
                                    let duration = t.elapsed().unwrap_or_default();
                                    if duration.as_secs() < 60 {
                                        "just now".to_string()
                                    } else if duration.as_secs() < 3600 {
                                        format!("{}m ago", duration.as_secs() / 60)
                                    } else if duration.as_secs() < 86400 {
                                        format!("{}h ago", duration.as_secs() / 3600)
                                    } else {
                                        format!("{}d ago", duration.as_secs() / 86400)
                                    }
                                })
                                .unwrap_or_else(|| "unknown".to_string());

                            if format == "json" {
                                println!(
                                    r#"{{"name":"{}","size":{},"modified":"{}"}}"#,
                                    name, size, modified
                                );
                            } else {
                                println!("  {} ({} bytes, {})", name, size, modified);
                            }
                            count += 1;
                        }
                    }
                }

                if count == 0 {
                    ctx.output.info("No state files found.");
                } else {
                    ctx.output.info(&format!("{} state file(s) found.", count));
                }
                Ok(0)
            }

            StateCommand::Show { name, state_dir } => {
                ctx.output.banner("STATE SHOW");
                let state_file = state_dir.join(format!("{}.json", name));

                if !state_file.exists() {
                    ctx.output
                        .error(&format!("State '{}' not found at {:?}", name, state_file));
                    return Ok(1);
                }

                let content = std::fs::read_to_string(&state_file)?;
                // Pretty-print JSON
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                    println!("{}", serde_json::to_string_pretty(&value)?);
                } else {
                    println!("{}", content);
                }
                Ok(0)
            }

            StateCommand::Pull { backend, output } => {
                ctx.output.banner("STATE PULL");
                ctx.output.info(&format!("Pulling state from: {}", backend));
                ctx.output.info(&format!("Output directory: {:?}", output));

                // For now, support file:// backends
                if backend.starts_with("file://") {
                    let source = backend.strip_prefix("file://").unwrap();
                    let source_path = std::path::Path::new(source);

                    if !source_path.exists() {
                        ctx.output
                            .error(&format!("Source path does not exist: {}", source));
                        return Ok(1);
                    }

                    std::fs::create_dir_all(output)?;

                    let mut count = 0;
                    if let Ok(entries) = std::fs::read_dir(source_path) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().map_or(false, |ext| ext == "json") {
                                let dest = output.join(entry.file_name());
                                std::fs::copy(&path, &dest)?;
                                count += 1;
                            }
                        }
                    }

                    ctx.output.info(&format!("Pulled {} state file(s).", count));
                } else {
                    ctx.output
                        .error("Unsupported backend. Supported: file://, s3://, http://");
                    return Ok(1);
                }

                Ok(0)
            }

            StateCommand::Push { state_dir, backend } => {
                ctx.output.banner("STATE PUSH");
                ctx.output
                    .info(&format!("Pushing state from: {:?}", state_dir));
                ctx.output.info(&format!("Target backend: {}", backend));

                if !state_dir.exists() {
                    ctx.output.error("State directory does not exist.");
                    return Ok(1);
                }

                if backend.starts_with("file://") {
                    let dest = backend.strip_prefix("file://").unwrap();
                    let dest_path = std::path::Path::new(dest);
                    std::fs::create_dir_all(dest_path)?;

                    let mut count = 0;
                    if let Ok(entries) = std::fs::read_dir(state_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().map_or(false, |ext| ext == "json") {
                                let target = dest_path.join(entry.file_name());
                                std::fs::copy(&path, &target)?;
                                count += 1;
                            }
                        }
                    }

                    ctx.output.info(&format!("Pushed {} state file(s).", count));
                } else {
                    ctx.output
                        .error("Unsupported backend. Supported: file://, s3://, http://");
                    return Ok(1);
                }

                Ok(0)
            }

            StateCommand::Remove {
                name,
                state_dir,
                force,
            } => {
                ctx.output.banner("STATE REMOVE");
                let state_file = state_dir.join(format!("{}.json", name));

                if !state_file.exists() {
                    ctx.output.error(&format!("State '{}' not found.", name));
                    return Ok(1);
                }

                if !force {
                    ctx.output.warning(&format!(
                        "This will permanently delete state '{}'. Use --force to confirm.",
                        name
                    ));
                    return Ok(1);
                }

                std::fs::remove_file(&state_file)?;
                ctx.output.info(&format!("State '{}' removed.", name));
                Ok(0)
            }

            StateCommand::Mv {
                source,
                destination,
                state,
                force,
            } => {
                ctx.output.banner("STATE MV");
                ctx.output.info(&format!("Moving {} -> {}", source, destination));

                if !force {
                    ctx.output.warning(
                        "This will rename a resource in state. Use --force to confirm.",
                    );
                    return Ok(1);
                }

                #[cfg(feature = "provisioning")]
                {
                    use rustible::provisioning::state::ProvisioningState;

                    if !state.exists() {
                        ctx.output.error("State file not found.");
                        return Ok(1);
                    }

                    let mut prov_state = ProvisioningState::load(&state).await
                        .map_err(|e| anyhow::anyhow!("Failed to load state: {}", e))?;

                    rustible::provisioning::state_ops::state_mv(&mut prov_state, source, destination)
                        .map_err(|e| anyhow::anyhow!("State mv failed: {}", e))?;

                    prov_state.save(&state).await
                        .map_err(|e| anyhow::anyhow!("Failed to save state: {}", e))?;

                    ctx.output.info("Successfully moved resource in state.");
                    return Ok(0);
                }

                #[cfg(not(feature = "provisioning"))]
                {
                    ctx.output.error("Provisioning feature not enabled. Rebuild with --features provisioning");
                    return Ok(1);
                }

                #[allow(unreachable_code)]
                Ok(0)
            }

            StateCommand::ReplaceProvider {
                from_provider,
                to_provider,
                state,
                force,
            } => {
                ctx.output.banner("STATE REPLACE-PROVIDER");
                ctx.output.info(&format!("Replacing provider {} -> {}", from_provider, to_provider));

                if !force {
                    ctx.output.warning(
                        "This will replace provider names in state. Use --force to confirm.",
                    );
                    return Ok(1);
                }

                #[cfg(feature = "provisioning")]
                {
                    use rustible::provisioning::state::ProvisioningState;

                    if !state.exists() {
                        ctx.output.error("State file not found.");
                        return Ok(1);
                    }

                    let mut prov_state = ProvisioningState::load(&state).await
                        .map_err(|e| anyhow::anyhow!("Failed to load state: {}", e))?;

                    let count = rustible::provisioning::state_ops::state_replace_provider(
                        &mut prov_state,
                        from_provider,
                        to_provider,
                    ).map_err(|e| anyhow::anyhow!("Replace provider failed: {}", e))?;

                    prov_state.save(&state).await
                        .map_err(|e| anyhow::anyhow!("Failed to save state: {}", e))?;

                    ctx.output.info(&format!("Replaced provider in {} resource(s).", count));
                    return Ok(0);
                }

                #[cfg(not(feature = "provisioning"))]
                {
                    ctx.output.error("Provisioning feature not enabled. Rebuild with --features provisioning");
                    return Ok(1);
                }

                #[allow(unreachable_code)]
                Ok(0)
            }

            StateCommand::Lock { command } => match command {
                LockCommand::List { state_dir } => {
                    ctx.output.banner("STATE LOCKS");
                    let lock_dir = state_dir.join("locks");

                    if !lock_dir.exists() {
                        ctx.output.info("No active locks.");
                        return Ok(0);
                    }

                    let mut count = 0;
                    if let Ok(entries) = std::fs::read_dir(&lock_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().map_or(false, |ext| ext == "lock") {
                                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                                if let Ok(content) = std::fs::read_to_string(&path) {
                                    println!("  Lock: {} - {}", name, content.trim());
                                } else {
                                    println!("  Lock: {}", name);
                                }
                                count += 1;
                            }
                        }
                    }

                    if count == 0 {
                        ctx.output.info("No active locks.");
                    } else {
                        ctx.output.info(&format!("{} active lock(s).", count));
                    }
                    Ok(0)
                }

                LockCommand::Release { lock_id, state_dir } => {
                    ctx.output.banner("STATE LOCK RELEASE");
                    let lock_file = state_dir.join("locks").join(format!("{}.lock", lock_id));

                    if !lock_file.exists() {
                        ctx.output.error(&format!("Lock '{}' not found.", lock_id));
                        return Ok(1);
                    }

                    std::fs::remove_file(&lock_file)?;
                    ctx.output.info(&format!("Lock '{}' released.", lock_id));
                    Ok(0)
                }
            },
        }
    }
}

/// Convert Terraform state JSON to Rustible state format
///
/// This is a simplified conversion that works without the full provisioning feature.
/// For full state management capabilities, enable the 'provisioning' feature.
#[cfg(not(feature = "provisioning"))]
fn convert_terraform_state(tf_state: &serde_json::Value) -> serde_json::Value {
    use chrono::Utc;
    use std::collections::HashMap;

    let mut resources: HashMap<String, serde_json::Value> = HashMap::new();
    let mut outputs: HashMap<String, serde_json::Value> = HashMap::new();

    // Extract lineage and serial
    let lineage = tf_state
        .get("lineage")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let serial = tf_state.get("serial").and_then(|v| v.as_u64()).unwrap_or(0);

    // Convert resources
    if let Some(tf_resources) = tf_state.get("resources").and_then(|r| r.as_array()) {
        for resource in tf_resources {
            let resource_type = resource
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let name = resource
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let mode = resource
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("managed");
            let provider = resource
                .get("provider")
                .and_then(|v| v.as_str())
                .map(|p| {
                    // Extract provider name from full provider path
                    p.split('/')
                        .last()
                        .unwrap_or(p)
                        .trim_start_matches("provider[\"")
                        .trim_end_matches("\"]")
                        .split('.')
                        .last()
                        .unwrap_or("unknown")
                })
                .or_else(|| resource_type.split('_').next())
                .unwrap_or("unknown")
                .to_string();

            // Process instances
            if let Some(instances) = resource.get("instances").and_then(|i| i.as_array()) {
                for (idx, instance) in instances.iter().enumerate() {
                    let attributes = instance
                        .get("attributes")
                        .cloned()
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    let cloud_id = attributes
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Handle index key for count/for_each
                    let index = instance.get("index_key").cloned();

                    let resource_key = format!("{}.{}", resource_type, name);

                    resources.insert(
                        resource_key.clone(),
                        serde_json::json!({
                            "id": {
                                "resource_type": resource_type,
                                "name": name
                            },
                            "cloud_id": cloud_id,
                            "resource_type": resource_type,
                            "provider": provider,
                            "config": {},
                            "attributes": attributes,
                            "dependencies": [],
                            "dependents": [],
                            "created_at": Utc::now().to_rfc3339(),
                            "updated_at": Utc::now().to_rfc3339(),
                            "metadata": {},
                            "tainted": false,
                            "index": index,
                            "mode": mode
                        }),
                    );
                }
            }
        }
    }

    // Convert outputs
    if let Some(tf_outputs) = tf_state.get("outputs").and_then(|o| o.as_object()) {
        for (name, output) in tf_outputs {
            let value = output
                .get("value")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let sensitive = output
                .get("sensitive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let output_type = output.get("type").cloned();

            outputs.insert(
                name.clone(),
                serde_json::json!({
                    "value": value,
                    "sensitive": sensitive,
                    "type": output_type,
                    "description": null
                }),
            );
        }
    }

    // Build Rustible state
    serde_json::json!({
        "version": 1,
        "serial": serial,
        "lineage": lineage,
        "resources": resources,
        "outputs": outputs,
        "history": [{
            "timestamp": Utc::now().to_rfc3339(),
            "change_type": "StateImported",
            "operation": "import-terraform",
            "resources_affected": [],
            "description": "Imported from Terraform state"
        }],
        "metadata": {
            "imported_from": "terraform",
            "imported_at": Utc::now().to_rfc3339()
        }
    })
}
