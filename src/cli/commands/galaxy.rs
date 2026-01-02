//! Galaxy subcommand for Rustible CLI
//!
//! This module provides CLI commands for interacting with Ansible Galaxy,
//! including installing/managing collections and roles.

use super::CommandContext;
use anyhow::Result;
use clap::{Args, Subcommand};
use rustible::galaxy::{Galaxy, RequirementsFile};
use std::path::PathBuf;

/// Galaxy command arguments
#[derive(Args, Debug, Clone)]
pub struct GalaxyArgs {
    /// Galaxy subcommand to execute
    #[command(subcommand)]
    pub command: GalaxyCommands,
}

/// Available Galaxy subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum GalaxyCommands {
    /// Collection operations
    Collection(CollectionArgs),

    /// Role operations
    Role(RoleArgs),

    /// Search Galaxy for collections or roles
    Search(SearchArgs),

    /// Install from requirements file
    Install(InstallArgs),
}

/// Collection subcommand arguments
#[derive(Args, Debug, Clone)]
pub struct CollectionArgs {
    #[command(subcommand)]
    pub command: CollectionCommands,
}

/// Collection subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum CollectionCommands {
    /// Install a collection from Galaxy
    Install(CollectionInstallArgs),

    /// List installed collections
    List(CollectionListArgs),

    /// Show collection info
    Info(CollectionInfoArgs),

    /// Verify collection integrity
    Verify(CollectionVerifyArgs),
}

/// Arguments for collection install
#[derive(Args, Debug, Clone)]
pub struct CollectionInstallArgs {
    /// Collection name (namespace.name) or path to tarball
    pub name: String,

    /// Version constraint (e.g., ">=1.0.0,<2.0.0")
    #[arg(id = "version_constraint", long = "ver")]
    pub version: Option<String>,

    /// Path to install collections to
    #[arg(short = 'p', long)]
    pub collections_path: Option<PathBuf>,

    /// Force reinstall even if already installed
    #[arg(long)]
    pub force: bool,

    /// Install offline from cache only
    #[arg(long)]
    pub offline: bool,
}

/// Arguments for collection list
#[derive(Args, Debug, Clone)]
pub struct CollectionListArgs {
    /// Path to search for collections
    #[arg(short = 'p', long)]
    pub collections_path: Option<PathBuf>,
}

/// Arguments for collection info
#[derive(Args, Debug, Clone)]
pub struct CollectionInfoArgs {
    /// Collection name (namespace.name)
    pub name: String,
}

/// Arguments for collection verify
#[derive(Args, Debug, Clone)]
pub struct CollectionVerifyArgs {
    /// Collection name (namespace.name) to verify, or verify all if not specified
    pub name: Option<String>,
}

/// Role subcommand arguments
#[derive(Args, Debug, Clone)]
pub struct RoleArgs {
    #[command(subcommand)]
    pub command: RoleCommands,
}

/// Role subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum RoleCommands {
    /// Install a role from Galaxy
    Install(RoleInstallArgs),

    /// List installed roles
    List(RoleListArgs),

    /// Show role info
    Info(RoleInfoArgs),

    /// Remove an installed role
    Remove(RoleRemoveArgs),
}

/// Arguments for role install
#[derive(Args, Debug, Clone)]
pub struct RoleInstallArgs {
    /// Role name (namespace.name or username.role)
    pub name: String,

    /// Version constraint
    #[arg(id = "version_constraint", long = "ver")]
    pub version: Option<String>,

    /// Path to install roles to
    #[arg(short = 'p', long)]
    pub roles_path: Option<PathBuf>,

    /// Force reinstall even if already installed
    #[arg(long)]
    pub force: bool,

    /// Install offline from cache only
    #[arg(long)]
    pub offline: bool,
}

/// Arguments for role list
#[derive(Args, Debug, Clone)]
pub struct RoleListArgs {
    /// Path to search for roles
    #[arg(short = 'p', long)]
    pub roles_path: Option<PathBuf>,
}

/// Arguments for role info
#[derive(Args, Debug, Clone)]
pub struct RoleInfoArgs {
    /// Role name
    pub name: String,
}

/// Arguments for role remove
#[derive(Args, Debug, Clone)]
pub struct RoleRemoveArgs {
    /// Role name to remove
    pub name: String,

    /// Path to search for roles
    #[arg(short = 'p', long)]
    pub roles_path: Option<PathBuf>,
}

/// Arguments for search command
#[derive(Args, Debug, Clone)]
pub struct SearchArgs {
    /// Search query
    pub query: String,

    /// Search type (collection or role)
    #[arg(short = 't', long, default_value = "collection")]
    pub search_type: SearchType,

    /// Maximum number of results
    #[arg(long, default_value = "20")]
    pub limit: usize,
}

/// Search type
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SearchType {
    Collection,
    Role,
}

/// Arguments for install from requirements
#[derive(Args, Debug, Clone)]
pub struct InstallArgs {
    /// Path to requirements.yml file
    #[arg(short = 'r', long, default_value = "requirements.yml")]
    pub requirements: PathBuf,

    /// Force reinstall
    #[arg(long)]
    pub force: bool,

    /// Install offline from cache only
    #[arg(long)]
    pub offline: bool,
}

/// Execute the galaxy command
pub async fn execute(args: &GalaxyArgs, ctx: &CommandContext) -> Result<i32> {
    match &args.command {
        GalaxyCommands::Collection(collection_args) => {
            execute_collection(collection_args, ctx).await
        }
        GalaxyCommands::Role(role_args) => execute_role(role_args, ctx).await,
        GalaxyCommands::Search(search_args) => execute_search(search_args, ctx).await,
        GalaxyCommands::Install(install_args) => execute_install(install_args, ctx).await,
    }
}

/// Execute collection subcommand
async fn execute_collection(args: &CollectionArgs, ctx: &CommandContext) -> Result<i32> {
    match &args.command {
        CollectionCommands::Install(install_args) => {
            execute_collection_install(install_args, ctx).await
        }
        CollectionCommands::List(list_args) => execute_collection_list(list_args, ctx).await,
        CollectionCommands::Info(info_args) => execute_collection_info(info_args, ctx).await,
        CollectionCommands::Verify(verify_args) => {
            execute_collection_verify(verify_args, ctx).await
        }
    }
}

/// Execute role subcommand
async fn execute_role(args: &RoleArgs, ctx: &CommandContext) -> Result<i32> {
    match &args.command {
        RoleCommands::Install(install_args) => execute_role_install(install_args, ctx).await,
        RoleCommands::List(list_args) => execute_role_list(list_args, ctx).await,
        RoleCommands::Info(info_args) => execute_role_info(info_args, ctx).await,
        RoleCommands::Remove(remove_args) => execute_role_remove(remove_args, ctx).await,
    }
}

/// Install a collection from Galaxy
async fn execute_collection_install(
    args: &CollectionInstallArgs,
    ctx: &CommandContext,
) -> Result<i32> {
    ctx.output
        .info(&format!("Installing collection: {}", args.name));

    let galaxy_config = build_galaxy_config(ctx, args.collections_path.as_ref(), None);
    let galaxy = if args.offline {
        Galaxy::offline(galaxy_config)?
    } else {
        Galaxy::new(galaxy_config)?
    };

    match galaxy
        .install_collection(
            &args.name,
            args.version.as_deref(),
            args.collections_path.as_ref(),
        )
        .await
    {
        Ok(path) => {
            ctx.output.info(&format!(
                "Collection '{}' installed to {:?}",
                args.name, path
            ));
            Ok(0)
        }
        Err(e) => {
            ctx.output
                .error(&format!("Failed to install collection: {}", e));
            Ok(1)
        }
    }
}

/// List installed collections
async fn execute_collection_list(args: &CollectionListArgs, ctx: &CommandContext) -> Result<i32> {
    let collections_path = args
        .collections_path
        .clone()
        .or_else(|| ctx.config.galaxy.collections_path.clone())
        .unwrap_or_else(|| PathBuf::from("./collections"));

    ctx.output
        .info(&format!("Listing collections from: {:?}", collections_path));

    if !collections_path.exists() {
        ctx.output.warning("Collections path does not exist");
        return Ok(0);
    }

    // List installed collections by scanning the directory
    let ansible_collections = collections_path.join("ansible_collections");
    if ansible_collections.exists() {
        for namespace_entry in std::fs::read_dir(&ansible_collections)? {
            let namespace_entry = namespace_entry?;
            if namespace_entry.file_type()?.is_dir() {
                let namespace = namespace_entry.file_name();
                for collection_entry in std::fs::read_dir(namespace_entry.path())? {
                    let collection_entry = collection_entry?;
                    if collection_entry.file_type()?.is_dir() {
                        let collection = collection_entry.file_name();
                        let manifest_path = collection_entry.path().join("MANIFEST.json");
                        let version = if manifest_path.exists() {
                            // Try to read version from MANIFEST.json
                            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                                if let Ok(manifest) =
                                    serde_json::from_str::<serde_json::Value>(&content)
                                {
                                    manifest["collection_info"]["version"]
                                        .as_str()
                                        .map(|s| s.to_string())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let version_str = version.unwrap_or_else(|| "unknown".to_string());
                        println!(
                            "{}.{} {}",
                            namespace.to_string_lossy(),
                            collection.to_string_lossy(),
                            version_str
                        );
                    }
                }
            }
        }
    } else {
        ctx.output.warning("No ansible_collections directory found");
    }

    Ok(0)
}

/// Show collection info
async fn execute_collection_info(args: &CollectionInfoArgs, ctx: &CommandContext) -> Result<i32> {
    ctx.output
        .info(&format!("Fetching info for collection: {}", args.name));

    let galaxy_config = build_galaxy_config(ctx, None, None);
    let galaxy = Galaxy::new(galaxy_config)?;

    match galaxy.get_collection_info(&args.name).await {
        Ok(info) => {
            println!("Collection: {}", args.name);
            println!("Namespace: {}", info.namespace);
            println!("Name: {}", info.name);
            if let Some(desc) = &info.description {
                println!("Description: {}", desc);
            }
            if let Some(version_info) = &info.highest_version {
                println!("Latest Version: {}", version_info.version);
            }
            if info.deprecated {
                println!("Status: DEPRECATED");
            }
            Ok(0)
        }
        Err(e) => {
            ctx.output
                .error(&format!("Failed to fetch collection info: {}", e));
            Ok(1)
        }
    }
}

/// Verify collection integrity
async fn execute_collection_verify(
    args: &CollectionVerifyArgs,
    ctx: &CommandContext,
) -> Result<i32> {
    ctx.output.info("Verifying collection cache integrity...");

    let galaxy_config = build_galaxy_config(ctx, None, None);
    let galaxy = Galaxy::new(galaxy_config)?;

    match galaxy.verify_cache_integrity().await {
        Ok(reports) => {
            let mut failed = 0;
            for report in &reports {
                if report.passed {
                    if args.name.is_none() || args.name.as_deref() == Some(&report.artifact) {
                        ctx.output.info(&format!("✓ {} - valid", report.artifact));
                    }
                } else if args.name.is_none() || args.name.as_deref() == Some(&report.artifact) {
                    ctx.output.error(&format!(
                        "✗ {} - invalid: {:?}",
                        report.artifact, report.error
                    ));
                    failed += 1;
                }
            }
            if failed > 0 {
                ctx.output
                    .error(&format!("{} artifacts failed integrity check", failed));
                Ok(1)
            } else {
                ctx.output.info("All artifacts passed integrity check");
                Ok(0)
            }
        }
        Err(e) => {
            ctx.output.error(&format!("Failed to verify cache: {}", e));
            Ok(1)
        }
    }
}

/// Install a role from Galaxy
async fn execute_role_install(args: &RoleInstallArgs, ctx: &CommandContext) -> Result<i32> {
    ctx.output.info(&format!("Installing role: {}", args.name));

    let galaxy_config = build_galaxy_config(ctx, None, args.roles_path.as_ref());
    let galaxy = if args.offline {
        Galaxy::offline(galaxy_config)?
    } else {
        Galaxy::new(galaxy_config)?
    };

    match galaxy
        .install_role(
            &args.name,
            args.version.as_deref(),
            args.roles_path.as_ref(),
        )
        .await
    {
        Ok(path) => {
            ctx.output
                .info(&format!("Role '{}' installed to {:?}", args.name, path));
            Ok(0)
        }
        Err(e) => {
            ctx.output.error(&format!("Failed to install role: {}", e));
            Ok(1)
        }
    }
}

/// List installed roles
async fn execute_role_list(args: &RoleListArgs, ctx: &CommandContext) -> Result<i32> {
    let roles_path = args
        .roles_path
        .clone()
        .or_else(|| ctx.config.galaxy.roles_path.clone())
        .unwrap_or_else(|| PathBuf::from("./roles"));

    ctx.output
        .info(&format!("Listing roles from: {:?}", roles_path));

    if !roles_path.exists() {
        ctx.output.warning("Roles path does not exist");
        return Ok(0);
    }

    // List installed roles by scanning the directory
    for entry in std::fs::read_dir(&roles_path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let role_name = entry.file_name();
            let meta_path = entry.path().join("meta").join("main.yml");

            let version = if meta_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&meta_path) {
                    if let Ok(meta) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                        meta["galaxy_info"]["version"]
                            .as_str()
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let version_str = version.unwrap_or_else(|| "unknown".to_string());
            println!("{} {}", role_name.to_string_lossy(), version_str);
        }
    }

    Ok(0)
}

/// Show role info
async fn execute_role_info(args: &RoleInfoArgs, ctx: &CommandContext) -> Result<i32> {
    ctx.output
        .info(&format!("Fetching info for role: {}", args.name));

    let galaxy_config = build_galaxy_config(ctx, None, None);
    let galaxy = Galaxy::new(galaxy_config)?;

    match galaxy.get_role_info(&args.name).await {
        Ok(info) => {
            println!("Role: {}", args.name);
            if let Some(ns) = &info.namespace {
                println!("Namespace: {}", ns);
            } else if let Some(user) = &info.github_user {
                println!("GitHub User: {}", user);
            }
            println!("Name: {}", info.name);
            if let Some(desc) = &info.description {
                println!("Description: {}", desc);
            }
            if let Some(repo) = &info.github_repo {
                println!("GitHub Repo: {}", repo);
            }
            if info.is_deprecated {
                println!("Status: DEPRECATED");
            }
            Ok(0)
        }
        Err(e) => {
            ctx.output
                .error(&format!("Failed to fetch role info: {}", e));
            Ok(1)
        }
    }
}

/// Remove an installed role
async fn execute_role_remove(args: &RoleRemoveArgs, ctx: &CommandContext) -> Result<i32> {
    let roles_path = args
        .roles_path
        .clone()
        .or_else(|| ctx.config.galaxy.roles_path.clone())
        .unwrap_or_else(|| PathBuf::from("./roles"));

    let role_path = roles_path.join(&args.name);

    if !role_path.exists() {
        ctx.output.error(&format!(
            "Role '{}' not found in {:?}",
            args.name, roles_path
        ));
        return Ok(1);
    }

    ctx.output.info(&format!("Removing role: {}", args.name));

    std::fs::remove_dir_all(&role_path)?;
    ctx.output.info(&format!("Role '{}' removed", args.name));

    Ok(0)
}

/// Search Galaxy
async fn execute_search(args: &SearchArgs, ctx: &CommandContext) -> Result<i32> {
    ctx.output
        .info(&format!("Searching Galaxy for: {}", args.query));

    let galaxy_config = build_galaxy_config(ctx, None, None);
    let galaxy = Galaxy::new(galaxy_config)?;

    match args.search_type {
        SearchType::Collection => match galaxy.search_collections(&args.query).await {
            Ok(results) => {
                if results.is_empty() {
                    println!("No collections found matching '{}'", args.query);
                } else {
                    println!("Found {} collections:", results.len().min(args.limit));
                    for (i, collection) in results.iter().take(args.limit).enumerate() {
                        println!(
                            "{}. {}.{} - {}",
                            i + 1,
                            collection.namespace,
                            collection.name,
                            collection
                                .description
                                .as_deref()
                                .unwrap_or("No description")
                        );
                    }
                }
                Ok(0)
            }
            Err(e) => {
                ctx.output.error(&format!("Search failed: {}", e));
                Ok(1)
            }
        },
        SearchType::Role => match galaxy.search_roles(&args.query).await {
            Ok(results) => {
                if results.is_empty() {
                    println!("No roles found matching '{}'", args.query);
                } else {
                    println!("Found {} roles:", results.len().min(args.limit));
                    for (i, role) in results.iter().take(args.limit).enumerate() {
                        let owner = role
                            .namespace
                            .as_deref()
                            .or(role.github_user.as_deref())
                            .unwrap_or("unknown");
                        println!(
                            "{}. {}.{} - {}",
                            i + 1,
                            owner,
                            role.name,
                            role.description.as_deref().unwrap_or("No description")
                        );
                    }
                }
                Ok(0)
            }
            Err(e) => {
                ctx.output.error(&format!("Search failed: {}", e));
                Ok(1)
            }
        },
    }
}

/// Install from requirements file
async fn execute_install(args: &InstallArgs, ctx: &CommandContext) -> Result<i32> {
    if !args.requirements.exists() {
        ctx.output.error(&format!(
            "Requirements file not found: {:?}",
            args.requirements
        ));
        return Ok(1);
    }

    ctx.output.info(&format!(
        "Installing from requirements file: {:?}",
        args.requirements
    ));

    let galaxy_config = build_galaxy_config(ctx, None, None);
    let galaxy = if args.offline {
        Galaxy::offline(galaxy_config)?
    } else {
        Galaxy::new(galaxy_config)?
    };

    let requirements = RequirementsFile::from_path(&args.requirements).await?;

    let collection_count = requirements.collections.len();
    let role_count = requirements.roles.len();
    ctx.output.info(&format!(
        "Found {} collections and {} roles in requirements",
        collection_count, role_count
    ));

    match galaxy.install_requirements(&requirements).await {
        Ok(paths) => {
            ctx.output
                .info(&format!("Successfully installed {} items", paths.len()));
            for path in &paths {
                println!("  - {:?}", path);
            }
            Ok(0)
        }
        Err(e) => {
            ctx.output
                .error(&format!("Failed to install requirements: {}", e));
            Ok(1)
        }
    }
}

/// Build GalaxyConfig from context and optional overrides
fn build_galaxy_config(
    ctx: &CommandContext,
    collections_path: Option<&PathBuf>,
    roles_path: Option<&PathBuf>,
) -> rustible::config::GalaxyConfig {
    // Convert from binary's config to library's config
    let cli_config = &ctx.config.galaxy;

    let mut config = rustible::config::GalaxyConfig {
        server: cli_config.server.clone(),
        server_list: cli_config
            .server_list
            .iter()
            .map(|s| rustible::config::GalaxyServer {
                name: s.name.clone(),
                url: s.url.clone(),
                token: s.token.clone(),
            })
            .collect(),
        cache_dir: cli_config.cache_dir.clone(),
        collections_path: cli_config.collections_path.clone(),
        roles_path: cli_config.roles_path.clone(),
        ignore_certs: cli_config.ignore_certs,
    };

    if let Some(path) = collections_path {
        config.collections_path = Some(path.clone());
    }
    if let Some(path) = roles_path {
        config.roles_path = Some(path.clone());
    }

    config
}
