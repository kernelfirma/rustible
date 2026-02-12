//! Vault command - Encrypt/decrypt secrets
//!
//! This module implements the `vault` subcommand for managing encrypted secrets.

use super::{CommandContext, Runnable};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, Context, Result};
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHasher};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use clap::{Parser, Subcommand};
use dialoguer::{theme::ColorfulTheme, Input};
use rand::rngs::OsRng;
use rand::Rng;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

/// Vault header marker
const VAULT_HEADER: &str = "$RUSTIBLE_VAULT;1.0;AES256-GCM";

/// Arguments for the vault command
#[derive(Parser, Debug, Clone)]
pub struct VaultArgs {
    #[command(subcommand)]
    pub action: VaultAction,
}

/// Vault subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum VaultAction {
    /// Encrypt a file
    Encrypt(EncryptArgs),

    /// Decrypt a file
    Decrypt(DecryptArgs),

    /// Edit an encrypted file
    Edit(EditArgs),

    /// View an encrypted file
    View(ViewArgs),

    /// Create a new encrypted file
    Create(CreateArgs),

    /// Re-encrypt with a new password
    Rekey(RekeyArgs),

    /// Encrypt a string
    EncryptString(EncryptStringArgs),

    /// Decrypt a string
    DecryptString(DecryptStringArgs),

    /// Initialize a new vault password file
    Init(InitArgs),

    /// Test vault connectivity / verify a vault password
    Login(LoginArgs),
}

/// Arguments for encrypt action
#[derive(Parser, Debug, Clone)]
pub struct EncryptArgs {
    /// File to encrypt
    pub file: PathBuf,

    /// Output file (default: overwrite input)
    #[arg(short = 'O', long = "output-file")]
    pub output_file: Option<PathBuf>,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,

    /// Vault ID for multi-vault setups
    #[arg(long)]
    pub vault_id: Option<String>,
}

/// Arguments for decrypt action
#[derive(Parser, Debug, Clone)]
pub struct DecryptArgs {
    /// File to decrypt
    pub file: PathBuf,

    /// Output file (default: overwrite input)
    #[arg(short = 'O', long = "output-file")]
    pub output_file: Option<PathBuf>,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,
}

/// Arguments for edit action
#[derive(Parser, Debug, Clone)]
pub struct EditArgs {
    /// File to edit
    pub file: PathBuf,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,

    /// Editor to use
    #[arg(long, env = "EDITOR", default_value = "vi")]
    pub editor: String,
}

/// Arguments for view action
#[derive(Parser, Debug, Clone)]
pub struct ViewArgs {
    /// File to view
    pub file: PathBuf,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,
}

/// Arguments for create action
#[derive(Parser, Debug, Clone)]
pub struct CreateArgs {
    /// File to create
    pub file: PathBuf,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,

    /// Editor to use
    #[arg(long, env = "EDITOR", default_value = "vi")]
    pub editor: String,
}

/// Arguments for rekey action
#[derive(Parser, Debug, Clone)]
pub struct RekeyArgs {
    /// Files to rekey
    #[arg(required = true)]
    pub files: Vec<PathBuf>,

    /// Current vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,

    /// New vault password file
    #[arg(long)]
    pub new_vault_password_file: Option<PathBuf>,
}

/// Arguments for encrypt-string action
#[derive(Parser, Debug, Clone)]
pub struct EncryptStringArgs {
    /// String to encrypt
    #[arg(short = 'p', long = "stdin-name")]
    pub name: Option<String>,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,

    /// The string to encrypt (if not provided, reads from stdin)
    pub string: Option<String>,
}

/// Arguments for decrypt-string action
#[derive(Parser, Debug, Clone)]
pub struct DecryptStringArgs {
    /// The encrypted string to decrypt
    pub string: Option<String>,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,
}

/// Arguments for init action
#[derive(Parser, Debug, Clone)]
pub struct InitArgs {
    /// Path to write the password file
    #[arg(long, default_value = ".vault_pass")]
    pub password_file: PathBuf,
}

/// Arguments for login action
#[derive(Parser, Debug, Clone)]
pub struct LoginArgs {
    /// Encrypted file to test against
    pub file: PathBuf,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,
}

/// Vault encryption/decryption engine
pub struct VaultEngine {
    password: String,
}

impl VaultEngine {
    /// Create a new vault engine with the given password
    pub fn new(password: String) -> Self {
        Self { password }
    }

    /// Derive an encryption key from the password
    fn derive_key(&self, salt: &[u8]) -> Result<[u8; 32]> {
        let salt_string = SaltString::encode_b64(salt)
            .map_err(|e| anyhow::anyhow!("Failed to encode salt: {}", e))?;

        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(self.password.as_bytes(), &salt_string)
            .map_err(|e| anyhow::anyhow!("Failed to derive key: {}", e))?;

        let hash_bytes = hash.hash.ok_or_else(|| anyhow::anyhow!("No hash output"))?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash_bytes.as_bytes()[..32]);

        Ok(key)
    }

    /// Encrypt data
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String> {
        // Generate random salt and nonce
        let salt: [u8; 16] = OsRng.gen();
        let nonce_bytes: [u8; 12] = OsRng.gen();

        // Derive key
        let key = self.derive_key(&salt)?;

        // Create cipher
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;

        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        // Combine salt + nonce + ciphertext
        let mut combined = Vec::new();
        combined.extend_from_slice(&salt);
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);

        // Encode as base64
        let encoded = BASE64.encode(&combined);

        // Format as vault file
        let mut output = String::new();
        output.push_str(VAULT_HEADER);
        output.push('\n');

        // Split into 80-character lines
        for chunk in encoded.as_bytes().chunks(80) {
            output.push_str(std::str::from_utf8(chunk)?);
            output.push('\n');
        }

        Ok(output)
    }

    /// Decrypt data
    pub fn decrypt(&self, vault_content: &str) -> Result<Vec<u8>> {
        let lines: Vec<&str> = vault_content.lines().collect();

        // Verify header
        if lines.is_empty() || !lines[0].starts_with("$RUSTIBLE_VAULT") {
            bail!("Invalid vault file format");
        }

        // Parse header
        let _header = lines[0];

        // Combine remaining lines and decode
        let encoded: String = lines[1..].iter().map(|l| l.trim()).collect();
        let combined = BASE64
            .decode(&encoded)
            .context("Failed to decode vault content")?;

        // Extract salt, nonce, and ciphertext
        if combined.len() < 28 {
            bail!("Invalid vault content: too short");
        }

        let salt = &combined[0..16];
        let nonce_bytes = &combined[16..28];
        let ciphertext = &combined[28..];

        // Derive key
        let key = self.derive_key(salt)?;

        // Create cipher
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;

        let nonce = Nonce::from_slice(nonce_bytes);

        // Decrypt
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("Decryption failed - wrong password?"))?;

        Ok(plaintext)
    }

    /// Check if content is encrypted
    pub fn is_encrypted(content: &str) -> bool {
        content.trim_start().starts_with("$RUSTIBLE_VAULT")
    }
}

/// Get password from file or prompt
fn get_password(password_file: Option<&PathBuf>, ctx: &CommandContext) -> Result<String> {
    if let Some(file) = password_file {
        let password = fs::read_to_string(file)
            .with_context(|| format!("Failed to read password file: {}", file.display()))?;
        return Ok(password.trim().to_string());
    }

    // Check environment variable
    if let Ok(password) = std::env::var("RUSTIBLE_VAULT_PASSWORD") {
        return Ok(password);
    }

    // Check for password file in environment
    if let Some(file) = crate::cli::env::vault_password_file() {
        let password = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read password file: {}", file.display()))?;
        return Ok(password.trim().to_string());
    }

    // Prompt for password
    ctx.output.flush();

    let password = dialoguer::Password::with_theme(&ColorfulTheme::default())
        .with_prompt("🔐 Vault password")
        .interact()?;

    Ok(password)
}

/// Get password with confirmation
fn get_password_with_confirm(
    password_file: Option<&PathBuf>,
    ctx: &CommandContext,
) -> Result<String> {
    if password_file.is_some() {
        return get_password(password_file, ctx);
    }

    ctx.output.flush();

    let password = dialoguer::Password::with_theme(&ColorfulTheme::default())
        .with_prompt("🔐 New Vault password")
        .with_confirmation("🔐 Confirm Vault password", "Passwords do not match")
        .interact()?;

    Ok(password)
}

impl VaultArgs {
    /// Execute the vault command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        // Initialize progress bars support
        ctx.output.init_progress();

        match &self.action {
            VaultAction::Encrypt(args) => {
                let password = get_password_with_confirm(args.vault_password_file.as_ref(), ctx)?;
                let engine = VaultEngine::new(password);

                let content = fs::read(&args.file)
                    .with_context(|| format!("Failed to read file: {}", args.file.display()))?;

                if VaultEngine::is_encrypted(&String::from_utf8_lossy(&content)) {
                    ctx.output.warning("File is already encrypted");
                    return Ok(0);
                }

                let spinner = ctx.output.create_spinner("Encrypting...");
                let encrypted = engine.encrypt(&content);
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                let encrypted = encrypted?;

                let output_path = args.output_file.as_ref().unwrap_or(&args.file);
                fs::write(output_path, &encrypted)
                    .with_context(|| format!("Failed to write file: {}", output_path.display()))?;

                ctx.output
                    .info(&format!("Encryption successful: {}", output_path.display()));
                Ok(0)
            }

            VaultAction::Decrypt(args) => {
                let password = get_password(args.vault_password_file.as_ref(), ctx)?;
                let engine = VaultEngine::new(password);

                let content = fs::read_to_string(&args.file)
                    .with_context(|| format!("Failed to read file: {}", args.file.display()))?;

                if !VaultEngine::is_encrypted(&content) {
                    ctx.output.warning("File is not encrypted");
                    return Ok(0);
                }

                let spinner = ctx.output.create_spinner("Decrypting...");
                let decrypted = engine.decrypt(&content);
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                let decrypted = decrypted?;

                let output_path = args.output_file.as_ref().unwrap_or(&args.file);
                fs::write(output_path, &decrypted)
                    .with_context(|| format!("Failed to write file: {}", output_path.display()))?;

                ctx.output
                    .info(&format!("Decryption successful: {}", output_path.display()));
                Ok(0)
            }

            VaultAction::View(args) => {
                let password = get_password(args.vault_password_file.as_ref(), ctx)?;
                let engine = VaultEngine::new(password);

                let content = fs::read_to_string(&args.file)
                    .with_context(|| format!("Failed to read file: {}", args.file.display()))?;

                if !VaultEngine::is_encrypted(&content) {
                    // Not encrypted, just show content
                    println!("{}", content);
                    return Ok(0);
                }

                let spinner = ctx.output.create_spinner("Decrypting...");
                let decrypted = engine.decrypt(&content);
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                let decrypted = decrypted?;

                println!("{}", String::from_utf8_lossy(&decrypted));
                Ok(0)
            }

            VaultAction::Edit(args) => {
                let password = get_password(args.vault_password_file.as_ref(), ctx)?;
                let engine = VaultEngine::new(password.clone());

                let content = fs::read_to_string(&args.file)
                    .with_context(|| format!("Failed to read file: {}", args.file.display()))?;

                let was_encrypted = VaultEngine::is_encrypted(&content);
                let plaintext = if was_encrypted {
                    let spinner = ctx.output.create_spinner("Decrypting...");
                    let decrypted = engine.decrypt(&content);
                    if let Some(sp) = spinner {
                        sp.finish_and_clear();
                    }
                    decrypted?
                } else {
                    content.into_bytes()
                };

                // Create temporary file
                let temp_dir = std::env::temp_dir();
                let temp_file = temp_dir.join(format!(".rustible_vault_{}", std::process::id()));

                fs::write(&temp_file, &plaintext)?;

                // Open editor
                let status = std::process::Command::new(&args.editor)
                    .arg(&temp_file)
                    .status()
                    .with_context(|| format!("Failed to open editor: {}", args.editor))?;

                if !status.success() {
                    fs::remove_file(&temp_file).ok();
                    bail!("Editor exited with error");
                }

                // Read edited content
                let edited = fs::read(&temp_file)?;
                fs::remove_file(&temp_file)?;

                // Re-encrypt if it was encrypted
                if was_encrypted {
                    let spinner = ctx.output.create_spinner("Encrypting...");
                    let encrypted = engine.encrypt(&edited);
                    if let Some(sp) = spinner {
                        sp.finish_and_clear();
                    }
                    let encrypted = encrypted?;

                    fs::write(&args.file, &encrypted)?;
                } else {
                    fs::write(&args.file, &edited)?;
                }

                ctx.output.info("File saved successfully");
                Ok(0)
            }

            VaultAction::Create(args) => {
                if args.file.exists() {
                    bail!("File already exists: {}", args.file.display());
                }

                let password = get_password_with_confirm(args.vault_password_file.as_ref(), ctx)?;
                let engine = VaultEngine::new(password);

                // Create temporary file
                let temp_dir = std::env::temp_dir();
                let temp_file = temp_dir.join(format!(".rustible_vault_{}", std::process::id()));

                fs::write(&temp_file, "")?;

                // Open editor
                let status = std::process::Command::new(&args.editor)
                    .arg(&temp_file)
                    .status()
                    .with_context(|| format!("Failed to open editor: {}", args.editor))?;

                if !status.success() {
                    fs::remove_file(&temp_file).ok();
                    bail!("Editor exited with error");
                }

                // Read content
                let content = fs::read(&temp_file)?;
                fs::remove_file(&temp_file)?;

                if content.is_empty() {
                    ctx.output.warning("No content entered, file not created");
                    return Ok(0);
                }

                // Encrypt and save
                let spinner = ctx.output.create_spinner("Encrypting...");
                let encrypted = engine.encrypt(&content);
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                let encrypted = encrypted?;

                fs::write(&args.file, &encrypted)?;

                ctx.output
                    .info(&format!("Created encrypted file: {}", args.file.display()));
                Ok(0)
            }

            VaultAction::Rekey(args) => {
                let old_password = get_password(args.vault_password_file.as_ref(), ctx)?;
                let new_password =
                    get_password_with_confirm(args.new_vault_password_file.as_ref(), ctx)?;

                let old_engine = VaultEngine::new(old_password);
                let new_engine = VaultEngine::new(new_password);

                for file in &args.files {
                    let content = fs::read_to_string(file)
                        .with_context(|| format!("Failed to read file: {}", file.display()))?;

                    if !VaultEngine::is_encrypted(&content) {
                        ctx.output
                            .warning(&format!("Skipping unencrypted file: {}", file.display()));
                        continue;
                    }

                    let spinner = ctx
                        .output
                        .create_spinner(&format!("Rekeying {}...", file.display()));

                    // We need to handle the Result inside the spinner block to ensure clear
                    let res = (|| -> Result<()> {
                        let decrypted = old_engine.decrypt(&content)?;
                        let reencrypted = new_engine.encrypt(&decrypted)?;
                        fs::write(file, &reencrypted)?;
                        Ok(())
                    })();

                    if let Some(sp) = spinner {
                        sp.finish_and_clear();
                    }

                    match res {
                        Ok(_) => ctx.output.info(&format!("Rekeyed: {}", file.display())),
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "Failed to rekey {}: {}",
                                file.display(),
                                e
                            ))
                        }
                    }
                }

                Ok(0)
            }

            VaultAction::EncryptString(args) => {
                let password = get_password_with_confirm(args.vault_password_file.as_ref(), ctx)?;
                let engine = VaultEngine::new(password);

                let plaintext = if let Some(ref s) = args.string {
                    s.as_bytes().to_vec()
                } else if std::io::stdin().is_terminal() {
                    let input: String = Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("📝 Enter text to encrypt")
                        .interact_text()?;
                    input.as_bytes().to_vec()
                } else {
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    input.trim().as_bytes().to_vec()
                };

                let spinner = ctx.output.create_spinner("Encrypting...");
                let encrypted = engine.encrypt(&plaintext);
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                let encrypted = encrypted?;

                if let Some(ref name) = args.name {
                    println!("{}: !vault |", name);
                    for line in encrypted.lines() {
                        println!("  {}", line);
                    }
                } else {
                    print!("{}", encrypted);
                }

                Ok(0)
            }

            VaultAction::DecryptString(args) => {
                let password = get_password(args.vault_password_file.as_ref(), ctx)?;
                let engine = VaultEngine::new(password);

                let encrypted_string = if let Some(ref s) = args.string {
                    s.clone()
                } else if std::io::stdin().is_terminal() {
                    Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("📝 Enter encrypted string")
                        .interact_text()?
                } else {
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    input.trim().to_string()
                };

                let spinner = ctx.output.create_spinner("Decrypting...");
                let decrypted = engine.decrypt(&encrypted_string);
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                let decrypted = decrypted?;

                println!("{}", String::from_utf8_lossy(&decrypted));

                Ok(0)
            }

            VaultAction::Init(args) => {
                if args.password_file.exists() {
                    bail!(
                        "Password file already exists: {}",
                        args.password_file.display()
                    );
                }

                // Generate a random 64-character password using hex encoding (32 random bytes = 64 hex chars)
                let random_bytes: [u8; 32] = OsRng.gen();
                let password: String = random_bytes.iter().map(|b| format!("{:02x}", b)).collect();

                // Write the password file
                fs::write(&args.password_file, &password).with_context(|| {
                    format!(
                        "Failed to write password file: {}",
                        args.password_file.display()
                    )
                })?;

                // Set permissions to 0600 (owner read/write only)
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = std::fs::Permissions::from_mode(0o600);
                    fs::set_permissions(&args.password_file, perms).with_context(|| {
                        format!(
                            "Failed to set permissions on: {}",
                            args.password_file.display()
                        )
                    })?;
                }

                ctx.output.info(&format!(
                    "Vault password file initialized: {}",
                    args.password_file.display()
                ));

                Ok(0)
            }

            VaultAction::Login(args) => {
                let password = get_password(args.vault_password_file.as_ref(), ctx)?;
                let engine = VaultEngine::new(password);

                let content = fs::read_to_string(&args.file)
                    .with_context(|| format!("Failed to read file: {}", args.file.display()))?;

                if !VaultEngine::is_encrypted(&content) {
                    bail!("File is not vault-encrypted: {}", args.file.display());
                }

                let spinner = ctx.output.create_spinner("Verifying vault password...");
                let result = engine.decrypt(&content);
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }

                match result {
                    Ok(_) => {
                        ctx.output
                            .info("Vault password is correct. Decryption successful.");
                        Ok(0)
                    }
                    Err(_) => {
                        ctx.output
                            .error("Vault password verification failed. Wrong password?");
                        Ok(1)
                    }
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl Runnable for VaultArgs {
    async fn run(&self, ctx: &mut CommandContext) -> Result<i32> {
        self.execute(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let engine = VaultEngine::new("test_password".to_string());

        let plaintext = b"Hello, World!";
        let encrypted = engine.encrypt(plaintext).unwrap();

        assert!(VaultEngine::is_encrypted(&encrypted));

        let decrypted = engine.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_password() {
        let engine1 = VaultEngine::new("password1".to_string());
        let engine2 = VaultEngine::new("password2".to_string());

        let plaintext = b"Secret data";
        let encrypted = engine1.encrypt(plaintext).unwrap();

        let result = engine2.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_encrypted() {
        assert!(VaultEngine::is_encrypted(
            "$RUSTIBLE_VAULT;1.0;AES256-GCM\ndata"
        ));
        assert!(!VaultEngine::is_encrypted("plain text content"));
    }
}
