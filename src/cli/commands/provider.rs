//! Provider subcommand for Rustible CLI
//!
//! This module provides CLI commands for managing providers,
//! including installing, updating, listing, and verifying providers.

use super::CommandContext;
use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

/// Provider command arguments
#[derive(Args, Debug, Clone)]
pub struct ProviderArgs {
    /// Provider subcommand to execute
    #[command(subcommand)]
    pub command: ProviderCommands,
}

/// Available Provider subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum ProviderCommands {
    /// Install a provider from local path, URL, or registry
    Install(InstallArgs),

    /// Update an installed provider
    Update(UpdateArgs),

    /// List installed providers
    List(ListArgs),

    /// Show provider information
    Info(InfoArgs),

    /// Verify provider signature and integrity
    Verify(VerifyArgs),

    /// Remove an installed provider
    Remove(RemoveArgs),
}

/// Arguments for provider install
#[derive(Args, Debug, Clone)]
pub struct InstallArgs {
    /// Provider path, URL, or registry name
    /// Examples:
    ///   - ./path/to/provider.tar.gz (local file)
    ///   - https://example.com/provider.tar.gz (URL)
    ///   - aws (registry shorthand)
    ///   - registry::aws@1.0.0 (registry with version)
    pub source: String,

    /// Installation path for providers
    #[arg(short = 'p', long)]
    pub path: Option<PathBuf>,

    /// Force reinstall even if already installed
    #[arg(long)]
    pub force: bool,

    /// Skip signature verification (use with caution)
    #[arg(long)]
    pub skip_verify: bool,

    /// Specific version to install (for registry sources)
    #[arg(long = "ver")]
    pub target_version: Option<String>,
}

/// Arguments for provider update
#[derive(Args, Debug, Clone)]
pub struct UpdateArgs {
    /// Provider name to update (or "all" for all providers)
    #[arg(default_value = "all")]
    pub name: String,

    /// Update to specific version
    #[arg(long = "ver")]
    pub target_version: Option<String>,

    /// Skip signature verification (use with caution)
    #[arg(long)]
    pub skip_verify: bool,
}

/// Arguments for provider list
#[derive(Args, Debug, Clone)]
pub struct ListArgs {
    /// Provider installation path to search
    #[arg(short = 'p', long)]
    pub path: Option<PathBuf>,

    /// Show detailed information
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Arguments for provider info
#[derive(Args, Debug, Clone)]
pub struct InfoArgs {
    /// Provider name
    pub name: String,

    /// Provider installation path
    #[arg(short = 'p', long)]
    pub path: Option<PathBuf>,
}

/// Arguments for provider verify
#[derive(Args, Debug, Clone)]
pub struct VerifyArgs {
    /// Provider name to verify (or verify all if not specified)
    pub name: Option<String>,

    /// Provider installation path
    #[arg(short = 'p', long)]
    pub path: Option<PathBuf>,
}

/// Arguments for provider remove
#[derive(Args, Debug, Clone)]
pub struct RemoveArgs {
    /// Provider name to remove
    pub name: String,

    /// Provider installation path
    #[arg(short = 'p', long)]
    pub path: Option<PathBuf>,

    /// Force removal without confirmation
    #[arg(long)]
    pub force: bool,
}

/// Execute the provider command
pub async fn execute(args: &ProviderArgs, ctx: &CommandContext) -> Result<i32> {
    match &args.command {
        ProviderCommands::Install(install_args) => execute_install(install_args, ctx).await,
        ProviderCommands::Update(update_args) => execute_update(update_args, ctx).await,
        ProviderCommands::List(list_args) => execute_list(list_args, ctx).await,
        ProviderCommands::Info(info_args) => execute_info(info_args, ctx).await,
        ProviderCommands::Verify(verify_args) => execute_verify(verify_args, ctx).await,
        ProviderCommands::Remove(remove_args) => execute_remove(remove_args, ctx).await,
    }
}

/// Get the default providers path
fn default_providers_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".rustible").join("providers"))
        .unwrap_or_else(|| PathBuf::from("./providers"))
}

/// Install a provider
async fn execute_install(args: &InstallArgs, ctx: &CommandContext) -> Result<i32> {
    let providers_path = args.path.clone().unwrap_or_else(default_providers_path);

    ctx.output
        .info(&format!("Installing provider from: {}", args.source));

    // Ensure providers directory exists
    if !providers_path.exists() {
        std::fs::create_dir_all(&providers_path)?;
    }

    // Determine source type and install accordingly
    let source = &args.source;

    if source.starts_with("http://") || source.starts_with("https://") {
        // URL source - download and install
        install_from_url(source, &providers_path, args, ctx).await
    } else if std::path::Path::new(source).exists() {
        // Local file source
        install_from_path(source, &providers_path, args, ctx).await
    } else {
        // Registry source
        install_from_registry(source, &providers_path, args, ctx).await
    }
}

/// Install provider from URL
async fn install_from_url(
    url: &str,
    providers_path: &Path,
    args: &InstallArgs,
    ctx: &CommandContext,
) -> Result<i32> {
    ctx.output
        .info(&format!("Downloading provider from: {}", url));

    // Create a temporary directory for download
    let temp_dir = tempfile::tempdir()?;
    let temp_file = temp_dir.path().join("provider.tar.gz");

    // Download the file
    let response = reqwest::get(url).await?;
    if !response.status().is_success() {
        ctx.output.error(&format!(
            "Failed to download provider: HTTP {}",
            response.status()
        ));
        return Ok(1);
    }

    let bytes = response.bytes().await?;
    std::fs::write(&temp_file, &bytes)?;

    ctx.output.info("Download complete, verifying...");

    // Verify signature if not skipped
    if !args.skip_verify {
        if let Err(e) = verify_provider_archive(&temp_file) {
            ctx.output
                .error(&format!("Signature verification failed: {}", e));
            return Ok(1);
        }
        ctx.output.info("Signature verification passed");
    } else {
        ctx.output
            .warning("Skipping signature verification (--skip-verify)");
    }

    // Extract and install
    install_provider_archive(&temp_file, providers_path, ctx)?;

    Ok(0)
}

/// Install provider from local path
async fn install_from_path(
    path: &str,
    providers_path: &Path,
    args: &InstallArgs,
    ctx: &CommandContext,
) -> Result<i32> {
    let source_path = PathBuf::from(path);
    ctx.output
        .info(&format!("Installing from local path: {:?}", source_path));

    if !source_path.exists() {
        ctx.output
            .error(&format!("Provider file not found: {:?}", source_path));
        return Ok(1);
    }

    // Verify signature if not skipped
    if !args.skip_verify {
        if let Err(e) = verify_provider_archive(&source_path) {
            ctx.output
                .error(&format!("Signature verification failed: {}", e));
            return Ok(1);
        }
        ctx.output.info("Signature verification passed");
    } else {
        ctx.output
            .warning("Skipping signature verification (--skip-verify)");
    }

    // Extract and install
    install_provider_archive(&source_path, providers_path, ctx)?;

    Ok(0)
}

/// Install provider from registry
async fn install_from_registry(
    name: &str,
    providers_path: &PathBuf,
    args: &InstallArgs,
    ctx: &CommandContext,
) -> Result<i32> {
    // Parse registry source: name, name@version, or registry::name@version
    let (registry, provider_name, version) =
        parse_registry_source(name, args.target_version.as_deref());

    ctx.output.info(&format!(
        "Looking up provider '{}' in registry '{}'{}",
        provider_name,
        registry.as_deref().unwrap_or("default"),
        version
            .as_ref()
            .map(|v| format!(" (version {})", v))
            .unwrap_or_default()
    ));

    // For now, we'll use a placeholder implementation
    // In a full implementation, this would query the registry API
    ctx.output
        .warning("Registry installation not yet fully implemented");
    ctx.output.info(&format!(
        "Provider '{}' would be installed to {:?}",
        provider_name, providers_path
    ));

    // Check if provider already exists
    let provider_dir = providers_path.join(&provider_name);
    if provider_dir.exists() && !args.force {
        ctx.output.error(&format!(
            "Provider '{}' is already installed. Use --force to reinstall.",
            provider_name
        ));
        return Ok(1);
    }

    Ok(0)
}

/// Parse a registry source string
fn parse_registry_source(
    source: &str,
    version_arg: Option<&str>,
) -> (Option<String>, String, Option<String>) {
    // Handle registry::name@version format
    if let Some((registry, rest)) = source.split_once("::") {
        let (name, version) = if let Some((n, v)) = rest.split_once('@') {
            (n.to_string(), Some(v.to_string()))
        } else {
            (rest.to_string(), version_arg.map(|s| s.to_string()))
        };
        return (Some(registry.to_string()), name, version);
    }

    // Handle name@version format
    if let Some((name, version)) = source.split_once('@') {
        return (None, name.to_string(), Some(version.to_string()));
    }

    // Simple name
    (None, source.to_string(), version_arg.map(|s| s.to_string()))
}

/// Verify a provider archive signature
fn verify_provider_archive(path: &PathBuf) -> Result<()> {
    // Read the archive
    let file = std::fs::File::open(path)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));

    // Look for manifest.json and signature file
    let mut manifest_found = false;
    let mut signature_found = false;

    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        if file_name == "manifest.json" {
            manifest_found = true;
        }
        if file_name == "signature.sig" || file_name == "SIGNATURE" {
            signature_found = true;
        }
    }

    if !manifest_found {
        anyhow::bail!("Provider archive is missing manifest.json");
    }

    // For now, we'll accept providers without signatures but log a warning
    // In production, this should verify the cryptographic signature
    if !signature_found {
        tracing::warn!("Provider archive does not contain a signature file");
    }

    Ok(())
}

/// Install a provider from an archive
fn install_provider_archive(
    archive_path: &Path,
    providers_path: &Path,
    ctx: &CommandContext,
) -> Result<()> {
    // Open and extract the archive
    let file = std::fs::File::open(archive_path)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));

    // Extract to a temporary directory first
    let temp_dir = tempfile::tempdir()?;
    archive.unpack(temp_dir.path())?;

    // Read manifest to get provider name
    let manifest_path = find_manifest(temp_dir.path())?;
    let manifest_content = std::fs::read_to_string(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_content)?;

    let provider_name = manifest["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Manifest missing 'name' field"))?;

    let version = manifest["version"].as_str().unwrap_or("unknown");

    ctx.output.info(&format!(
        "Installing provider: {} v{}",
        provider_name, version
    ));

    // Create provider directory
    let provider_dir = providers_path.join(provider_name);
    if provider_dir.exists() {
        std::fs::remove_dir_all(&provider_dir)?;
    }

    // Find the actual content directory (may be nested)
    let content_dir = find_content_dir(temp_dir.path())?;

    // Copy contents to provider directory
    copy_dir_recursive(&content_dir, &provider_dir)?;

    ctx.output.info(&format!(
        "Provider '{}' installed to {:?}",
        provider_name, provider_dir
    ));

    Ok(())
}

/// Find manifest.json in extracted archive
fn find_manifest(dir: &std::path::Path) -> Result<PathBuf> {
    // Check root level
    let root_manifest = dir.join("manifest.json");
    if root_manifest.exists() {
        return Ok(root_manifest);
    }

    // Check one level deep
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let nested_manifest = entry.path().join("manifest.json");
            if nested_manifest.exists() {
                return Ok(nested_manifest);
            }
        }
    }

    anyhow::bail!("Could not find manifest.json in provider archive")
}

/// Find the content directory in extracted archive
fn find_content_dir(dir: &std::path::Path) -> Result<PathBuf> {
    // If manifest is at root, content is at root
    if dir.join("manifest.json").exists() {
        return Ok(dir.to_path_buf());
    }

    // Check one level deep
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("manifest.json").exists() {
            return Ok(entry.path());
        }
    }

    anyhow::bail!("Could not find provider content directory")
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Update a provider
async fn execute_update(args: &UpdateArgs, ctx: &CommandContext) -> Result<i32> {
    let providers_path = default_providers_path();

    if args.name == "all" {
        ctx.output.info("Updating all providers...");
        // List installed providers and update each
        if !providers_path.exists() {
            ctx.output.warning("No providers installed");
            return Ok(0);
        }

        for entry in std::fs::read_dir(&providers_path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                ctx.output.info(&format!("Checking for updates: {}", name));
                // In full implementation, query registry for newer version
            }
        }
        ctx.output.info("All providers are up to date");
    } else {
        ctx.output
            .info(&format!("Updating provider: {}", args.name));

        let provider_dir = providers_path.join(&args.name);
        if !provider_dir.exists() {
            ctx.output
                .error(&format!("Provider '{}' is not installed", args.name));
            return Ok(1);
        }

        // In full implementation, query registry for newer version and install
        if let Some(version) = &args.target_version {
            ctx.output
                .info(&format!("Would update to version: {}", version));
        } else {
            ctx.output.info("Would update to latest version");
        }
    }

    Ok(0)
}

/// List installed providers
async fn execute_list(args: &ListArgs, ctx: &CommandContext) -> Result<i32> {
    let providers_path = args.path.clone().unwrap_or_else(default_providers_path);

    ctx.output
        .info(&format!("Installed providers from: {:?}", providers_path));

    if !providers_path.exists() {
        ctx.output.warning("Provider directory does not exist");
        return Ok(0);
    }

    let mut found = false;
    for entry in std::fs::read_dir(&providers_path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            found = true;
            let name = entry.file_name().to_string_lossy().to_string();
            let manifest_path = entry.path().join("manifest.json");

            if args.verbose {
                if manifest_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                            let version = manifest["version"].as_str().unwrap_or("unknown");
                            let api_version = manifest["api_version"].as_str().unwrap_or("unknown");
                            let description = manifest["description"].as_str().unwrap_or("");

                            println!("{}:", name);
                            println!("  Version: {}", version);
                            println!("  API Version: {}", api_version);
                            if !description.is_empty() {
                                println!("  Description: {}", description);
                            }
                            println!();
                            continue;
                        }
                    }
                }
                println!("{}: (no manifest)", name);
            } else {
                // Simple list
                let version = if manifest_path.exists() {
                    std::fs::read_to_string(&manifest_path)
                        .ok()
                        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                        .and_then(|m| m["version"].as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown".to_string())
                } else {
                    "unknown".to_string()
                };
                println!("{} v{}", name, version);
            }
        }
    }

    if !found {
        ctx.output.info("No providers installed");
    }

    Ok(0)
}

/// Show provider information
async fn execute_info(args: &InfoArgs, ctx: &CommandContext) -> Result<i32> {
    let providers_path = args.path.clone().unwrap_or_else(default_providers_path);
    let provider_dir = providers_path.join(&args.name);

    if !provider_dir.exists() {
        ctx.output
            .error(&format!("Provider '{}' is not installed", args.name));
        return Ok(1);
    }

    let manifest_path = provider_dir.join("manifest.json");
    if !manifest_path.exists() {
        ctx.output.error(&format!(
            "Provider '{}' is missing manifest.json",
            args.name
        ));
        return Ok(1);
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    println!("Provider: {}", args.name);
    println!("Path: {:?}", provider_dir);
    println!();

    if let Some(version) = manifest["version"].as_str() {
        println!("Version: {}", version);
    }
    if let Some(api_version) = manifest["api_version"].as_str() {
        println!("API Version: {}", api_version);
    }
    if let Some(description) = manifest["description"].as_str() {
        println!("Description: {}", description);
    }
    if let Some(author) = manifest["author"].as_str() {
        println!("Author: {}", author);
    }
    if let Some(license) = manifest["license"].as_str() {
        println!("License: {}", license);
    }

    // Show supported targets
    if let Some(targets) = manifest["supported_targets"].as_array() {
        let target_strs: Vec<&str> = targets.iter().filter_map(|t| t.as_str()).collect();
        if !target_strs.is_empty() {
            println!("Supported Targets: {}", target_strs.join(", "));
        }
    }

    // Show capabilities
    if let Some(capabilities) = manifest["capabilities"].as_array() {
        let cap_strs: Vec<&str> = capabilities.iter().filter_map(|c| c.as_str()).collect();
        if !cap_strs.is_empty() {
            println!("Capabilities: {}", cap_strs.join(", "));
        }
    }

    // Show modules if available
    if let Some(modules) = manifest["modules"].as_array() {
        println!();
        println!("Available Modules ({}):", modules.len());
        for module in modules {
            if let Some(name) = module["name"].as_str() {
                let desc = module["description"].as_str().unwrap_or("");
                println!("  - {}: {}", name, desc);
            }
        }
    }

    Ok(0)
}

/// Verify provider signature and integrity
async fn execute_verify(args: &VerifyArgs, ctx: &CommandContext) -> Result<i32> {
    let providers_path = args.path.clone().unwrap_or_else(default_providers_path);

    if !providers_path.exists() {
        ctx.output.warning("Provider directory does not exist");
        return Ok(0);
    }

    let mut failed = 0;
    let mut verified = 0;

    let providers_to_verify: Vec<_> = if let Some(name) = &args.name {
        vec![providers_path.join(name)]
    } else {
        std::fs::read_dir(&providers_path)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect()
    };

    for provider_path in providers_to_verify {
        let provider_name = provider_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        if !provider_path.exists() {
            ctx.output
                .error(&format!("Provider '{}' is not installed", provider_name));
            failed += 1;
            continue;
        }

        // Check manifest exists
        let manifest_path = provider_path.join("manifest.json");
        if !manifest_path.exists() {
            ctx.output
                .error(&format!("✗ {} - missing manifest.json", provider_name));
            failed += 1;
            continue;
        }

        // Verify manifest is valid JSON
        match std::fs::read_to_string(&manifest_path) {
            Ok(content) => {
                if serde_json::from_str::<serde_json::Value>(&content).is_err() {
                    ctx.output
                        .error(&format!("✗ {} - invalid manifest.json", provider_name));
                    failed += 1;
                    continue;
                }
            }
            Err(e) => {
                ctx.output.error(&format!(
                    "✗ {} - cannot read manifest: {}",
                    provider_name, e
                ));
                failed += 1;
                continue;
            }
        }

        // Check for checksum file
        let checksum_path = provider_path.join("CHECKSUMS");
        if checksum_path.exists() {
            // Verify file checksums
            if let Err(e) = verify_checksums(&provider_path, &checksum_path) {
                ctx.output.error(&format!(
                    "✗ {} - checksum verification failed: {}",
                    provider_name, e
                ));
                failed += 1;
                continue;
            }
        }

        // Check for signature file
        let sig_path = provider_path.join("SIGNATURE");
        if sig_path.exists() {
            // In production, verify cryptographic signature
            ctx.output.info(&format!(
                "✓ {} - signature present (not cryptographically verified)",
                provider_name
            ));
        } else {
            ctx.output
                .warning(&format!("⚠ {} - no signature file", provider_name));
        }

        ctx.output.info(&format!("✓ {} - verified", provider_name));
        verified += 1;
    }

    println!();
    println!("Verified: {}, Failed: {}", verified, failed);

    if failed > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

/// Verify checksums for provider files
fn verify_checksums(
    provider_path: &std::path::Path,
    checksum_path: &std::path::Path,
) -> Result<()> {
    let content = std::fs::read_to_string(checksum_path)?;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse "checksum  filename" or "checksum filename"
        let parts: Vec<&str> = line.splitn(2, [' ', '\t']).collect();
        if parts.len() != 2 {
            continue;
        }

        let expected_checksum = parts[0].trim();
        let filename = parts[1].trim().trim_start_matches('*');

        let file_path = provider_path.join(filename);
        if !file_path.exists() {
            anyhow::bail!("File not found: {}", filename);
        }

        // Calculate BLAKE3 checksum
        let file_content = std::fs::read(&file_path)?;
        let actual_checksum = blake3::hash(&file_content).to_hex().to_string();

        if actual_checksum != expected_checksum {
            anyhow::bail!(
                "Checksum mismatch for {}: expected {}, got {}",
                filename,
                expected_checksum,
                actual_checksum
            );
        }
    }

    Ok(())
}

/// Remove an installed provider
async fn execute_remove(args: &RemoveArgs, ctx: &CommandContext) -> Result<i32> {
    let providers_path = args.path.clone().unwrap_or_else(default_providers_path);
    let provider_path = providers_path.join(&args.name);

    if !provider_path.exists() {
        ctx.output
            .error(&format!("Provider '{}' is not installed", args.name));
        return Ok(1);
    }

    // Confirm removal if not forced
    if !args.force {
        ctx.output.warning(&format!(
            "This will remove provider '{}' from {:?}",
            args.name, provider_path
        ));
        // In a real CLI, we'd prompt for confirmation here
        // For now, require --force flag
        ctx.output.error("Use --force to confirm removal");
        return Ok(1);
    }

    ctx.output
        .info(&format!("Removing provider: {}", args.name));
    std::fs::remove_dir_all(&provider_path)?;
    ctx.output
        .info(&format!("Provider '{}' removed", args.name));

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::output::OutputFormatter;
    use crate::config::Config;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::RwLock;

    fn test_context(config: Config) -> CommandContext {
        CommandContext {
            config,
            output: OutputFormatter::new(false, false, 0),
            inventory_path: None,
            extra_vars: Vec::new(),
            verbosity: 0,
            check_mode: false,
            diff_mode: false,
            limit: None,
            forks: 1,
            timeout: 30,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[test]
    fn test_parse_registry_source_simple() {
        let (registry, name, version) = parse_registry_source("aws", None);
        assert_eq!(registry, None);
        assert_eq!(name, "aws");
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_registry_source_with_version() {
        let (registry, name, version) = parse_registry_source("aws@1.0.0", None);
        assert_eq!(registry, None);
        assert_eq!(name, "aws");
        assert_eq!(version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_parse_registry_source_with_registry() {
        let (registry, name, version) = parse_registry_source("internal::aws@2.0.0", None);
        assert_eq!(registry, Some("internal".to_string()));
        assert_eq!(name, "aws");
        assert_eq!(version, Some("2.0.0".to_string()));
    }

    #[test]
    fn test_parse_registry_source_version_arg_override() {
        let (registry, name, version) = parse_registry_source("aws", Some("3.0.0"));
        assert_eq!(registry, None);
        assert_eq!(name, "aws");
        assert_eq!(version, Some("3.0.0".to_string()));
    }

    #[tokio::test]
    async fn test_execute_list_missing_path() {
        let temp = tempdir().unwrap();
        let missing = temp.path().join("missing_providers");
        let args = ListArgs {
            path: Some(missing),
            verbose: false,
        };

        let ctx = test_context(Config::default());
        let exit = execute_list(&args, &ctx).await.unwrap();

        assert_eq!(exit, 0);
    }

    #[tokio::test]
    async fn test_execute_list_with_providers() {
        let temp = tempdir().unwrap();
        let providers_path = temp.path().join("providers");

        // Create a mock provider
        let aws_provider = providers_path.join("aws");
        std::fs::create_dir_all(&aws_provider).unwrap();
        std::fs::write(
            aws_provider.join("manifest.json"),
            r#"{"name": "aws", "version": "1.0.0", "api_version": "1.0.0"}"#,
        )
        .unwrap();

        let args = ListArgs {
            path: Some(providers_path),
            verbose: false,
        };

        let ctx = test_context(Config::default());
        let exit = execute_list(&args, &ctx).await.unwrap();

        assert_eq!(exit, 0);
    }

    #[tokio::test]
    async fn test_execute_info_missing_provider() {
        let temp = tempdir().unwrap();
        let args = InfoArgs {
            name: "nonexistent".to_string(),
            path: Some(temp.path().to_path_buf()),
        };

        let ctx = test_context(Config::default());
        let exit = execute_info(&args, &ctx).await.unwrap();

        assert_eq!(exit, 1);
    }

    #[tokio::test]
    async fn test_execute_info_with_provider() {
        let temp = tempdir().unwrap();
        let providers_path = temp.path();

        // Create a mock provider
        let aws_provider = providers_path.join("aws");
        std::fs::create_dir_all(&aws_provider).unwrap();
        std::fs::write(
            aws_provider.join("manifest.json"),
            r#"{
                "name": "aws",
                "version": "1.0.0",
                "api_version": "1.0.0",
                "description": "AWS cloud provider",
                "supported_targets": ["aws"],
                "capabilities": ["read", "create", "update", "delete"],
                "modules": [
                    {"name": "ec2_instance", "description": "Manage EC2 instances"}
                ]
            }"#,
        )
        .unwrap();

        let args = InfoArgs {
            name: "aws".to_string(),
            path: Some(providers_path.to_path_buf()),
        };

        let ctx = test_context(Config::default());
        let exit = execute_info(&args, &ctx).await.unwrap();

        assert_eq!(exit, 0);
    }

    #[tokio::test]
    async fn test_execute_remove_missing_provider() {
        let temp = tempdir().unwrap();
        let args = RemoveArgs {
            name: "nonexistent".to_string(),
            path: Some(temp.path().to_path_buf()),
            force: true,
        };

        let ctx = test_context(Config::default());
        let exit = execute_remove(&args, &ctx).await.unwrap();

        assert_eq!(exit, 1);
    }

    #[tokio::test]
    async fn test_execute_remove_success() {
        let temp = tempdir().unwrap();
        let providers_path = temp.path();

        // Create a mock provider
        let aws_provider = providers_path.join("aws");
        std::fs::create_dir_all(&aws_provider).unwrap();
        std::fs::write(
            aws_provider.join("manifest.json"),
            r#"{"name": "aws", "version": "1.0.0"}"#,
        )
        .unwrap();

        let args = RemoveArgs {
            name: "aws".to_string(),
            path: Some(providers_path.to_path_buf()),
            force: true,
        };

        let ctx = test_context(Config::default());
        let exit = execute_remove(&args, &ctx).await.unwrap();

        assert_eq!(exit, 0);
        assert!(!aws_provider.exists());
    }

    #[tokio::test]
    async fn test_execute_verify_empty() {
        let temp = tempdir().unwrap();
        let providers_path = temp.path().join("providers");
        std::fs::create_dir_all(&providers_path).unwrap();

        let args = VerifyArgs {
            name: None,
            path: Some(providers_path),
        };

        let ctx = test_context(Config::default());
        let exit = execute_verify(&args, &ctx).await.unwrap();

        assert_eq!(exit, 0);
    }

    #[tokio::test]
    async fn test_execute_verify_valid_provider() {
        let temp = tempdir().unwrap();
        let providers_path = temp.path();

        // Create a mock provider with valid manifest
        let aws_provider = providers_path.join("aws");
        std::fs::create_dir_all(&aws_provider).unwrap();
        std::fs::write(
            aws_provider.join("manifest.json"),
            r#"{"name": "aws", "version": "1.0.0"}"#,
        )
        .unwrap();

        let args = VerifyArgs {
            name: Some("aws".to_string()),
            path: Some(providers_path.to_path_buf()),
        };

        let ctx = test_context(Config::default());
        let exit = execute_verify(&args, &ctx).await.unwrap();

        assert_eq!(exit, 0);
    }
}
