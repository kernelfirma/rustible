//! Sign command - Artifact signing and verification
//!
//! This module implements the `sign` subcommand for signing artifacts,
//! verifying signatures, and managing signing keys.

use super::CommandContext;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use rustible::security::signing::{
    ArtifactSigner, ArtifactVerifier, SignatureBundle, SigningKeyPair,
};

/// Arguments for the sign command
#[derive(Parser, Debug, Clone)]
pub struct SignArgs {
    #[command(subcommand)]
    pub action: SignAction,
}

/// Sign subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum SignAction {
    /// Sign an artifact file
    #[command(name = "artifact")]
    SignArtifact(SignArtifactArgs),

    /// Verify an artifact signature
    Verify(VerifyArgs),

    /// Generate a new signing key pair
    Keygen(KeygenArgs),

    /// List known signing keys
    #[command(name = "list-keys")]
    ListKeys,
}

/// Arguments for signing an artifact
#[derive(Parser, Debug, Clone)]
pub struct SignArtifactArgs {
    /// Path to the file to sign
    pub file: PathBuf,

    /// Path to the signing key file (raw 32-byte key)
    #[arg(long, short = 'k')]
    pub key: PathBuf,

    /// Key identifier
    #[arg(long, default_value = "default")]
    pub key_id: String,

    /// Output path for the signature bundle (default: <file>.sig.json)
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,
}

/// Arguments for verifying a signature
#[derive(Parser, Debug, Clone)]
pub struct VerifyArgs {
    /// Path to the artifact file
    pub file: PathBuf,

    /// Path to the signature bundle (.sig.json)
    #[arg(long, short = 's')]
    pub signature: PathBuf,

    /// Path to the verification key file
    #[arg(long, short = 'k')]
    pub key: PathBuf,

    /// Key identifier
    #[arg(long, default_value = "default")]
    pub key_id: String,
}

/// Arguments for key generation
#[derive(Parser, Debug, Clone)]
pub struct KeygenArgs {
    /// Output path for the generated key
    #[arg(default_value = "signing.key")]
    pub output: PathBuf,

    /// Key identifier
    #[arg(long, default_value = "default")]
    pub key_id: String,
}

impl SignArgs {
    /// Execute the sign command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.action {
            SignAction::SignArtifact(args) => sign_artifact(args, ctx).await,
            SignAction::Verify(args) => verify_artifact(args, ctx).await,
            SignAction::Keygen(args) => generate_key(args, ctx).await,
            SignAction::ListKeys => list_keys(ctx).await,
        }
    }
}

async fn sign_artifact(args: &SignArtifactArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("ARTIFACT SIGNING");

    if !args.file.exists() {
        ctx.output
            .error(&format!("File not found: {}", args.file.display()));
        return Ok(1);
    }

    let key_bytes = std::fs::read(&args.key)
        .with_context(|| format!("Failed to read key file: {}", args.key.display()))?;

    let key = SigningKeyPair::from_bytes(&args.key_id, &key_bytes).ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid key file: expected exactly 32 bytes, got {}",
            key_bytes.len()
        )
    })?;

    let signer = ArtifactSigner::new();
    let bundle = signer
        .sign_file(&args.file, &key)
        .with_context(|| format!("Failed to sign file: {}", args.file.display()))?;

    let output_path = args.output.clone().unwrap_or_else(|| {
        let mut p = args.file.clone();
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        p.set_file_name(format!("{}.sig.json", name));
        p
    });

    let json = serde_json::to_string_pretty(&bundle)?;
    std::fs::write(&output_path, &json)
        .with_context(|| format!("Failed to write signature: {}", output_path.display()))?;

    ctx.output.success(&format!(
        "Signed {} -> {}",
        args.file.display(),
        output_path.display()
    ));
    ctx.output
        .info(&format!("Key: {} ({})", bundle.key_id, bundle.algorithm));
    ctx.output.info(&format!("Hash: {}", bundle.artifact_hash));

    Ok(0)
}

async fn verify_artifact(args: &VerifyArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("SIGNATURE VERIFICATION");

    if !args.file.exists() {
        ctx.output
            .error(&format!("File not found: {}", args.file.display()));
        return Ok(1);
    }

    let sig_json = std::fs::read_to_string(&args.signature)
        .with_context(|| format!("Failed to read signature: {}", args.signature.display()))?;

    let bundle: SignatureBundle =
        serde_json::from_str(&sig_json).with_context(|| "Failed to parse signature bundle")?;

    let key_bytes = std::fs::read(&args.key)
        .with_context(|| format!("Failed to read key file: {}", args.key.display()))?;

    let key = SigningKeyPair::from_bytes(&args.key_id, &key_bytes).ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid key file: expected exactly 32 bytes, got {}",
            key_bytes.len()
        )
    })?;

    let data = std::fs::read(&args.file)
        .with_context(|| format!("Failed to read artifact: {}", args.file.display()))?;

    let verifier = ArtifactVerifier::new();
    let result = verifier.verify(&data, &bundle, &key);

    if result.valid {
        ctx.output.success(&result.message);
        ctx.output.info(&format!("Key: {}", result.key_id));
        Ok(0)
    } else {
        ctx.output.error(&result.message);
        Ok(1)
    }
}

async fn generate_key(args: &KeygenArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("KEY GENERATION");

    let kp = SigningKeyPair::generate(&args.key_id);
    std::fs::write(&args.output, kp.secret_bytes())
        .with_context(|| format!("Failed to write key: {}", args.output.display()))?;

    ctx.output.success(&format!(
        "Generated signing key '{}' -> {}",
        args.key_id,
        args.output.display()
    ));
    ctx.output
        .info("Keep this key file secret. It is required for both signing and verification.");

    Ok(0)
}

async fn list_keys(ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("SIGNING KEYS");

    // Look for .key files in the current directory.
    let mut found = 0u32;
    for entry in std::fs::read_dir(".")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("key") {
            if let Ok(bytes) = std::fs::read(&path) {
                if bytes.len() == 32 {
                    let name = path.file_stem().unwrap_or_default().to_string_lossy();
                    ctx.output.info(&format!("  {} ({})", name, path.display()));
                    found += 1;
                }
            }
        }
    }

    if found == 0 {
        ctx.output
            .info("No signing keys found in current directory.");
        ctx.output
            .hint("Run 'rustible sign keygen' to generate a new key.");
    } else {
        ctx.output.info(&format!("Found {} signing key(s)", found));
    }

    Ok(0)
}
