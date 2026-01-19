//! Authorized Key module - Manage SSH authorized_keys file
//!
//! This module manages SSH authorized keys for user accounts. It supports:
//! - Adding and removing SSH public keys
//! - Key options (command, from, environment, etc.)
//! - Exclusive mode for complete key management
//! - Key validation and format checking
//! - Both local and remote execution

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions, TransferOptions};
use crate::utils::shell_escape;
use once_cell::sync::Lazy;
use regex::Regex;
use std::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Regex pattern for validating SSH public key format
/// Matches: ssh-rsa, ssh-ed25519, ssh-dss, ecdsa-sha2-nistp*, sk-ssh-ed25519@, sk-ecdsa-sha2-*, etc.
static SSH_KEY_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(ssh-(rsa|ed25519|dss)|ecdsa-sha2-nistp(256|384|521)|sk-(ssh-ed25519|ecdsa-sha2-nistp(256|384|521))@openssh\.com)\s+[A-Za-z0-9+/=]+(\s+.*)?$"
    ).expect("Invalid SSH key regex")
});

/// Regex pattern for extracting key type and data from a public key
static KEY_PARTS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^([a-z0-9\-@.]+)\s+([A-Za-z0-9+/=]+)(?:\s+(.*))?$")
        .expect("Invalid key parts regex")
});

/// Desired state for an authorized key
#[derive(Debug, Clone, PartialEq)]
pub enum KeyState {
    /// Key should be present in authorized_keys
    Present,
    /// Key should be absent from authorized_keys
    Absent,
}

impl KeyState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(KeyState::Present),
            "absent" => Ok(KeyState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

impl std::str::FromStr for KeyState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        KeyState::from_str(s)
    }
}

impl fmt::Display for KeyState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyState::Present => write!(f, "present"),
            KeyState::Absent => write!(f, "absent"),
        }
    }
}

/// Represents a parsed SSH authorized key entry
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizedKey {
    /// Key options (e.g., command="...", from="...", no-pty)
    pub options: Option<String>,
    /// Key type (ssh-rsa, ssh-ed25519, etc.)
    pub key_type: String,
    /// Base64-encoded key data
    pub key_data: String,
    /// Key comment (typically user@host)
    pub comment: Option<String>,
}

impl AuthorizedKey {
    /// Parse an SSH public key from a string
    pub fn parse(key: &str) -> ModuleResult<Self> {
        let key = key.trim();

        // Check if the key starts with options
        if key.starts_with("ssh-")
            || key.starts_with("ecdsa-")
            || key.starts_with("sk-ssh-")
            || key.starts_with("sk-ecdsa-")
        {
            // No options, parse directly
            Self::parse_key_only(key, None)
        } else {
            // Has options - need to extract them
            Self::parse_with_options(key)
        }
    }

    /// Parse a key without options
    fn parse_key_only(key: &str, options: Option<String>) -> ModuleResult<Self> {
        let captures = KEY_PARTS_REGEX.captures(key).ok_or_else(|| {
            ModuleError::InvalidParameter(format!("Invalid SSH key format: {}", key))
        })?;

        let key_type = captures
            .get(1)
            .map(|m| m.as_str().to_string())
            .ok_or_else(|| {
                ModuleError::InvalidParameter("Missing key type in SSH key".to_string())
            })?;

        let key_data = captures
            .get(2)
            .map(|m| m.as_str().to_string())
            .ok_or_else(|| {
                ModuleError::InvalidParameter("Missing key data in SSH key".to_string())
            })?;

        let comment = captures.get(3).map(|m| m.as_str().to_string());

        Ok(Self {
            options,
            key_type,
            key_data,
            comment,
        })
    }

    /// Parse a key with options prefix
    fn parse_with_options(key: &str) -> ModuleResult<Self> {
        // Options can contain quoted strings with spaces, so we need careful parsing
        let mut chars = key.chars().peekable();
        let mut options = String::new();
        let mut in_quotes = false;

        // Parse until we hit a key type
        while let Some(&c) = chars.peek() {
            if !in_quotes {
                // Check if we're at the start of a key type
                let remaining: String = chars.clone().collect();
                if remaining.starts_with("ssh-")
                    || remaining.starts_with("ecdsa-")
                    || remaining.starts_with("sk-ssh-")
                    || remaining.starts_with("sk-ecdsa-")
                {
                    break;
                }

                if c == '"' {
                    in_quotes = true;
                }
            } else if c == '"' {
                in_quotes = false;
            }

            options.push(chars.next().unwrap());
        }

        let options = options.trim().to_string();
        let remaining: String = chars.collect();

        if remaining.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "No key data found after options".to_string(),
            ));
        }

        let options = if options.is_empty() {
            None
        } else {
            Some(options)
        };

        Self::parse_key_only(&remaining, options)
    }

    /// Format the key as a line for authorized_keys
    pub fn to_line(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref opts) = self.options {
            parts.push(opts.clone());
        }

        parts.push(self.key_type.clone());
        parts.push(self.key_data.clone());

        if let Some(ref comment) = self.comment {
            parts.push(comment.clone());
        }

        parts.join(" ")
    }

    /// Check if two keys are the same (ignoring options and comment)
    pub fn same_key(&self, other: &AuthorizedKey) -> bool {
        self.key_type == other.key_type && self.key_data == other.key_data
    }

    /// Update options for this key
    pub fn with_options(mut self, options: Option<String>) -> Self {
        self.options = options;
        self
    }

    /// Update comment for this key
    pub fn with_comment(mut self, comment: Option<String>) -> Self {
        self.comment = comment;
        self
    }
}

/// Valid SSH key type prefixes
const VALID_KEY_TYPES: &[&str] = &[
    "ssh-rsa",
    "ssh-ed25519",
    "ssh-dss",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "sk-ssh-ed25519@openssh.com",
    "sk-ecdsa-sha2-nistp256@openssh.com",
    "sk-ecdsa-sha2-nistp384@openssh.com",
    "sk-ecdsa-sha2-nistp521@openssh.com",
];

/// Validate an SSH public key format
pub fn validate_ssh_key(key: &str) -> ModuleResult<()> {
    let key = key.trim();

    if key.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "SSH key cannot be empty".to_string(),
        ));
    }

    // Try to parse the key
    let parsed_key = AuthorizedKey::parse(key)?;

    // Validate that the key type is a known valid SSH key type
    if !VALID_KEY_TYPES.contains(&parsed_key.key_type.as_str()) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid SSH key type '{}'. Valid types: {}",
            parsed_key.key_type,
            VALID_KEY_TYPES.join(", ")
        )));
    }

    Ok(())
}

/// Parse key options from a string
/// Supports: command="...", from="...", environment="...", no-pty, etc.
pub fn parse_key_options(options: &str) -> ModuleResult<String> {
    // Validate option format - options are comma-separated
    let options = options.trim();

    if options.is_empty() {
        return Ok(String::new());
    }

    // Check for common invalid characters that might indicate injection
    if options.contains('\n') || options.contains('\r') {
        return Err(ModuleError::InvalidParameter(
            "Key options cannot contain newlines".to_string(),
        ));
    }

    // Validate balanced quotes
    let quote_count = options.chars().filter(|&c| c == '"').count();
    if quote_count % 2 != 0 {
        return Err(ModuleError::InvalidParameter(
            "Unbalanced quotes in key options".to_string(),
        ));
    }

    Ok(options.to_string())
}

/// Module for managing SSH authorized keys
pub struct AuthorizedKeyModule;

impl AuthorizedKeyModule {
    /// Get execution options with become support if needed
    fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
        let mut options = ExecuteOptions::new();
        if context.r#become {
            options = options.with_escalation(context.become_user.clone());
            if let Some(ref method) = context.become_method {
                options.escalate_method = Some(method.clone());
            }
            if let Some(ref password) = context.become_password {
                options.escalate_password = Some(password.clone());
            }
        }
        options
    }

    /// Execute a command via connection
    fn execute_command(
        connection: &Arc<dyn Connection + Send + Sync>,
        command: &str,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String, String)> {
        let options = Self::get_exec_options(context);

        let handle = Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let connection = connection.clone();
        let command = command.to_string();
        let result = std::thread::scope(|s| {
            s.spawn(|| handle.block_on(async { connection.execute(&command, Some(options)).await }))
                .join()
                .unwrap()
        })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Get user info (home directory, uid, gid) via connection
    fn get_user_info(
        connection: &Arc<dyn Connection + Send + Sync>,
        user: &str,
        context: &ModuleContext,
    ) -> ModuleResult<(String, u32, u32)> {
        let command = format!("getent passwd {}", shell_escape(user));
        let (success, stdout, _) = Self::execute_command(connection, &command, context)?;

        if !success || stdout.trim().is_empty() {
            return Err(ModuleError::ExecutionFailed(format!(
                "User '{}' not found",
                user
            )));
        }

        // Parse passwd line: name:x:uid:gid:comment:home:shell
        let parts: Vec<&str> = stdout.trim().split(':').collect();
        if parts.len() < 6 {
            return Err(ModuleError::ExecutionFailed(format!(
                "Invalid passwd entry for user '{}'",
                user
            )));
        }

        let uid = parts[2].parse().unwrap_or(0);
        let gid = parts[3].parse().unwrap_or(0);
        let home = parts[5].to_string();

        Ok((home, uid, gid))
    }

    /// Get the path to authorized_keys file
    fn get_authorized_keys_path(
        connection: &Arc<dyn Connection + Send + Sync>,
        user: &str,
        path: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        if let Some(p) = path {
            return Ok(p.to_string());
        }

        let (home, _, _) = Self::get_user_info(connection, user, context)?;
        Ok(format!("{}/.ssh/authorized_keys", home))
    }

    /// Read authorized_keys file content
    fn read_authorized_keys(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Vec<String>> {
        let handle = Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let conn = connection.clone();
        let path = path.to_string();

        let downloaded_bytes = std::thread::scope(|s| {
            s.spawn(|| {
                handle.block_on(async {
                    let remote_path = Path::new(&path);
                    if conn.path_exists(remote_path).await.unwrap_or(false) {
                        conn.download_content(remote_path).await.ok()
                    } else {
                        Some(Vec::new())
                    }
                })
            })
            .join()
            .unwrap()
        });

        match downloaded_bytes {
            Some(data) => {
                let content_str = String::from_utf8_lossy(&data);
                Ok(content_str.lines().map(|s| s.to_string()).collect())
            }
            None => {
                // Try via command as fallback
                let command = format!("cat {} 2>/dev/null || true", shell_escape(&path));
                let (_, stdout, _) = Self::execute_command(connection, &command, context)?;
                Ok(stdout.lines().map(|s| s.to_string()).collect())
            }
        }
    }

    /// Write authorized_keys file
    fn write_authorized_keys(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        lines: &[String],
        user: &str,
        manage_dir: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let handle = Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let (_, uid, gid) = Self::get_user_info(connection, user, context)?;

        let conn = connection.clone();
        let path_clone = path.to_string();
        let file_content = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };

        std::thread::scope(|s| {
            s.spawn(|| {
                handle.block_on(async {
                    let remote_path = Path::new(&path_clone);

                    // Create .ssh directory if needed
                    if manage_dir {
                        if let Some(parent) = remote_path.parent() {
                            let mkdir_cmd = format!("mkdir -p '{}'", parent.display());
                            let _ = conn.execute(&mkdir_cmd, None).await;
                        }
                    }

                    // Upload content
                    let mut transfer_opts = TransferOptions::new();
                    transfer_opts = transfer_opts.with_mode(0o600);
                    transfer_opts = transfer_opts.with_create_dirs();

                    conn.upload_content(file_content.as_bytes(), remote_path, Some(transfer_opts))
                        .await
                })
            })
            .join()
            .unwrap()
        })
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to write authorized_keys: {}", e))
        })?;

        // Set ownership and permissions
        let dir_path = Path::new(path).parent().unwrap_or(Path::new("/"));
        if manage_dir {
            let chown_dir_cmd = format!(
                "chown {}:{} {} && chmod 700 {}",
                uid,
                gid,
                shell_escape(&dir_path.to_string_lossy()),
                shell_escape(&dir_path.to_string_lossy())
            );
            Self::execute_command(connection, &chown_dir_cmd, context)?;
        }

        let chown_cmd = format!(
            "chown {}:{} {} && chmod 600 {}",
            uid,
            gid,
            shell_escape(path),
            shell_escape(path)
        );
        Self::execute_command(connection, &chown_cmd, context)?;

        Ok(())
    }

    /// Parse keys from authorized_keys file lines
    fn parse_keys(lines: &[String]) -> Vec<AuthorizedKey> {
        lines
            .iter()
            .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
            .filter_map(|line| AuthorizedKey::parse(line).ok())
            .collect()
    }

    /// Add a key to the list, handling options and comments
    fn add_key(existing_keys: &mut Vec<AuthorizedKey>, new_key: &AuthorizedKey) -> bool {
        // Check if key already exists (by key data)
        for key in existing_keys.iter_mut() {
            if key.same_key(new_key) {
                // Key exists - check if options/comment need updating
                if key.options != new_key.options || key.comment != new_key.comment {
                    key.options = new_key.options.clone();
                    key.comment = new_key.comment.clone();
                    return true;
                }
                return false;
            }
        }

        // Key doesn't exist - add it
        existing_keys.push(new_key.clone());
        true
    }

    /// Remove a key from the list
    fn remove_key(existing_keys: &mut Vec<AuthorizedKey>, key_to_remove: &AuthorizedKey) -> bool {
        let original_len = existing_keys.len();
        existing_keys.retain(|k| !k.same_key(key_to_remove));
        existing_keys.len() != original_len
    }

    /// Execute locally using filesystem operations
    #[allow(clippy::too_many_arguments)]
    fn execute_local(
        context: &ModuleContext,
        user: &str,
        key: &str,
        state: KeyState,
        path: Option<&str>,
        manage_dir: bool,
        key_options: Option<String>,
        comment: Option<String>,
        exclusive: bool,
        validate_certs: bool,
    ) -> ModuleResult<ModuleOutput> {
        // Get user info for home directory
        let home = Self::get_local_user_home(user)?;
        let authorized_keys_path = path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("{}/.ssh/authorized_keys", home));

        let auth_keys_path = Path::new(&authorized_keys_path);

        // Read existing keys
        let existing_content = if auth_keys_path.exists() {
            fs::read_to_string(auth_keys_path)?
        } else {
            String::new()
        };

        let existing_lines: Vec<String> = existing_content.lines().map(|s| s.to_string()).collect();
        let mut existing_keys = Self::parse_keys(&existing_lines);

        // Parse the new key
        if validate_certs {
            validate_ssh_key(key)?;
        }
        let mut new_key = AuthorizedKey::parse(key)?;

        // Apply key options if provided
        if let Some(ref opts) = key_options {
            let parsed_opts = parse_key_options(opts)?;
            if !parsed_opts.is_empty() {
                new_key = new_key.with_options(Some(parsed_opts));
            }
        }

        // Apply comment if provided
        if let Some(ref c) = comment {
            new_key = new_key.with_comment(Some(c.clone()));
        }

        let changed = match state {
            KeyState::Present => {
                if exclusive {
                    // Replace all keys with just this one
                    let new_keys = vec![new_key.clone()];
                    if existing_keys != new_keys {
                        existing_keys = new_keys;
                        true
                    } else {
                        false
                    }
                } else {
                    Self::add_key(&mut existing_keys, &new_key)
                }
            }
            KeyState::Absent => Self::remove_key(&mut existing_keys, &new_key),
        };

        if !changed {
            return Ok(ModuleOutput::ok(format!(
                "Key already {} in '{}'",
                state, authorized_keys_path
            )));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would {} key in '{}'",
                if state == KeyState::Present {
                    "add"
                } else {
                    "remove"
                },
                authorized_keys_path
            )));
        }

        // Create .ssh directory if needed
        if manage_dir {
            if let Some(parent) = auth_keys_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
                }
            }
        }

        // Write the file
        let new_content: Vec<String> = existing_keys.iter().map(|k| k.to_line()).collect();
        let file_content = if new_content.is_empty() {
            String::new()
        } else {
            format!("{}\n", new_content.join("\n"))
        };

        fs::write(auth_keys_path, file_content)?;
        fs::set_permissions(auth_keys_path, fs::Permissions::from_mode(0o600))?;

        let action = if state == KeyState::Present {
            if exclusive {
                "Set exclusive key"
            } else {
                "Added key"
            }
        } else {
            "Removed key"
        };

        let mut output = ModuleOutput::changed(format!("{} in '{}'", action, authorized_keys_path));

        if context.diff_mode {
            output = output.with_diff(Diff::new(existing_lines.join("\n"), new_content.join("\n")));
        }

        Ok(output)
    }

    /// Get local user home directory
    fn get_local_user_home(user: &str) -> ModuleResult<String> {
        use std::process::Command;

        let output = Command::new("getent")
            .arg("passwd")
            .arg(user)
            .output()
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to get user info: {}", e)))?;

        if !output.status.success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "User '{}' not found",
                user
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.trim().split(':').collect();
        if parts.len() < 6 {
            return Err(ModuleError::ExecutionFailed(format!(
                "Invalid passwd entry for user '{}'",
                user
            )));
        }

        Ok(parts[5].to_string())
    }

    /// Execute on a remote host via connection
    #[allow(clippy::too_many_arguments)]
    fn execute_remote(
        context: &ModuleContext,
        user: &str,
        key: &str,
        state: KeyState,
        path: Option<&str>,
        manage_dir: bool,
        key_options: Option<String>,
        comment: Option<String>,
        exclusive: bool,
        validate_certs: bool,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed("No connection available for remote execution".to_string())
        })?;

        // Get the authorized_keys path
        let authorized_keys_path = Self::get_authorized_keys_path(connection, user, path, context)?;

        // Read existing keys
        let existing_lines =
            Self::read_authorized_keys(connection, &authorized_keys_path, context)?;
        let mut existing_keys = Self::parse_keys(&existing_lines);

        // Parse and validate the new key
        if validate_certs {
            validate_ssh_key(key)?;
        }
        let mut new_key = AuthorizedKey::parse(key)?;

        // Apply key options if provided
        if let Some(ref opts) = key_options {
            let parsed_opts = parse_key_options(opts)?;
            if !parsed_opts.is_empty() {
                new_key = new_key.with_options(Some(parsed_opts));
            }
        }

        // Apply comment if provided
        if let Some(ref c) = comment {
            new_key = new_key.with_comment(Some(c.clone()));
        }

        let changed = match state {
            KeyState::Present => {
                if exclusive {
                    // Replace all keys with just this one
                    let new_keys = vec![new_key.clone()];
                    if existing_keys != new_keys {
                        existing_keys = new_keys;
                        true
                    } else {
                        false
                    }
                } else {
                    Self::add_key(&mut existing_keys, &new_key)
                }
            }
            KeyState::Absent => Self::remove_key(&mut existing_keys, &new_key),
        };

        if !changed {
            return Ok(ModuleOutput::ok(format!(
                "Key already {} in '{}'",
                state, authorized_keys_path
            )));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would {} key in '{}'",
                if state == KeyState::Present {
                    "add"
                } else {
                    "remove"
                },
                authorized_keys_path
            )));
        }

        // Write the updated keys
        let new_content: Vec<String> = existing_keys.iter().map(|k| k.to_line()).collect();
        Self::write_authorized_keys(
            connection,
            &authorized_keys_path,
            &new_content,
            user,
            manage_dir,
            context,
        )?;

        let action = if state == KeyState::Present {
            if exclusive {
                "Set exclusive key"
            } else {
                "Added key"
            }
        } else {
            "Removed key"
        };

        let mut output = ModuleOutput::changed(format!("{} in '{}'", action, authorized_keys_path));

        if context.diff_mode {
            output = output.with_diff(Diff::new(existing_lines.join("\n"), new_content.join("\n")));
        }

        Ok(output)
    }
}

impl Module for AuthorizedKeyModule {
    fn name(&self) -> &'static str {
        "authorized_key"
    }

    fn description(&self) -> &'static str {
        "Manage SSH authorized_keys file entries"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::NativeTransport
    }

    fn required_params(&self) -> &[&'static str] {
        &["user", "key"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate user parameter
        let user = params.get_string_required("user")?;
        if user.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "user parameter cannot be empty".to_string(),
            ));
        }

        // Validate user name characters (prevent injection)
        if !user
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid user name '{}': must contain only alphanumeric characters, underscores, and hyphens",
                user
            )));
        }

        // Validate key parameter
        let key = params.get_string_required("key")?;
        if key.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "key parameter cannot be empty".to_string(),
            ));
        }

        // Validate state parameter
        if let Some(state) = params.get_string("state")? {
            KeyState::from_str(&state)?;
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let user = params.get_string_required("user")?;
        let key = params.get_string_required("key")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = KeyState::from_str(&state_str)?;
        let path = params.get_string("path")?;
        let manage_dir = params.get_bool_or("manage_dir", true);
        let key_options = params.get_string("key_options")?;
        let comment = params.get_string("comment")?;
        let exclusive = params.get_bool_or("exclusive", false);
        let validate_certs = params.get_bool_or("validate_certs", true);

        // Route to remote or local execution based on connection availability
        if context.connection.is_some() {
            Self::execute_remote(
                context,
                &user,
                &key,
                state,
                path.as_deref(),
                manage_dir,
                key_options,
                comment,
                exclusive,
                validate_certs,
            )
        } else {
            Self::execute_local(
                context,
                &user,
                &key,
                state,
                path.as_deref(),
                manage_dir,
                key_options,
                comment,
                exclusive,
                validate_certs,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TEST_RSA_KEY: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7 test@example.com";
    const TEST_ED25519_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@example.com";

    #[test]
    fn test_parse_simple_rsa_key() {
        let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
        assert_eq!(key.key_type, "ssh-rsa");
        assert_eq!(key.key_data, "AAAAB3NzaC1yc2EAAAADAQABAAABgQC7");
        assert_eq!(key.comment, Some("test@example.com".to_string()));
        assert!(key.options.is_none());
    }

    #[test]
    fn test_parse_ed25519_key() {
        let key = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();
        assert_eq!(key.key_type, "ssh-ed25519");
        assert_eq!(key.key_data, "AAAAC3NzaC1lZDI1NTE5AAAAI");
        assert_eq!(key.comment, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_parse_key_with_options() {
        let key_str = r#"command="/bin/date",no-pty ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7 test"#;
        let key = AuthorizedKey::parse(key_str).unwrap();
        assert_eq!(
            key.options,
            Some(r#"command="/bin/date",no-pty"#.to_string())
        );
        assert_eq!(key.key_type, "ssh-rsa");
    }

    #[test]
    fn test_parse_key_with_from_option() {
        let key_str = r#"from="192.168.1.0/24" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI admin"#;
        let key = AuthorizedKey::parse(key_str).unwrap();
        assert_eq!(key.options, Some(r#"from="192.168.1.0/24""#.to_string()));
        assert_eq!(key.key_type, "ssh-ed25519");
    }

    #[test]
    fn test_key_to_line() {
        let key = AuthorizedKey {
            options: Some("no-pty".to_string()),
            key_type: "ssh-rsa".to_string(),
            key_data: "AAAAB3NzaC1yc2EAAAADAQABAAABgQC7".to_string(),
            comment: Some("test@example.com".to_string()),
        };
        let line = key.to_line();
        assert_eq!(
            line,
            "no-pty ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7 test@example.com"
        );
    }

    #[test]
    fn test_same_key() {
        let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
        let key2 =
            AuthorizedKey::parse("ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7 other@host").unwrap();
        let key3 = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();

        assert!(key1.same_key(&key2)); // Same key, different comment
        assert!(!key1.same_key(&key3)); // Different keys
    }

    #[test]
    fn test_validate_ssh_key_valid() {
        assert!(validate_ssh_key(TEST_RSA_KEY).is_ok());
        assert!(validate_ssh_key(TEST_ED25519_KEY).is_ok());
        assert!(
            validate_ssh_key("ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY= user").is_ok()
        );
    }

    #[test]
    fn test_validate_ssh_key_invalid() {
        assert!(validate_ssh_key("").is_err());
        assert!(validate_ssh_key("not a valid key").is_err());
        assert!(validate_ssh_key("ssh-invalid AAAA").is_err());
    }

    #[test]
    fn test_parse_key_options_valid() {
        assert_eq!(parse_key_options("no-pty").unwrap(), "no-pty");
        assert_eq!(
            parse_key_options(r#"command="/bin/date""#).unwrap(),
            r#"command="/bin/date""#
        );
        assert_eq!(
            parse_key_options(r#"from="10.0.0.0/8",no-pty"#).unwrap(),
            r#"from="10.0.0.0/8",no-pty"#
        );
    }

    #[test]
    fn test_parse_key_options_invalid() {
        assert!(parse_key_options("option\nwith\nnewlines").is_err());
        assert!(parse_key_options(r#"unbalanced="quote"#).is_err());
    }

    #[test]
    fn test_key_state_from_str() {
        assert_eq!(KeyState::from_str("present").unwrap(), KeyState::Present);
        assert_eq!(KeyState::from_str("absent").unwrap(), KeyState::Absent);
        assert_eq!(KeyState::from_str("PRESENT").unwrap(), KeyState::Present);
        assert!(KeyState::from_str("invalid").is_err());
    }

    #[test]
    fn test_add_key_new() {
        let mut keys = Vec::new();
        let new_key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();

        let changed = AuthorizedKeyModule::add_key(&mut keys, &new_key);

        assert!(changed);
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn test_add_key_existing() {
        let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
        let mut keys = vec![key.clone()];

        let changed = AuthorizedKeyModule::add_key(&mut keys, &key);

        assert!(!changed);
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn test_add_key_update_options() {
        let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
        let mut keys = vec![key.clone()];

        let new_key = key.with_options(Some("no-pty".to_string()));
        let changed = AuthorizedKeyModule::add_key(&mut keys, &new_key);

        assert!(changed);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].options, Some("no-pty".to_string()));
    }

    #[test]
    fn test_remove_key() {
        let key = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
        let mut keys = vec![key.clone()];

        let changed = AuthorizedKeyModule::remove_key(&mut keys, &key);

        assert!(changed);
        assert!(keys.is_empty());
    }

    #[test]
    fn test_remove_key_not_found() {
        let key1 = AuthorizedKey::parse(TEST_RSA_KEY).unwrap();
        let key2 = AuthorizedKey::parse(TEST_ED25519_KEY).unwrap();
        let mut keys = vec![key1];

        let changed = AuthorizedKeyModule::remove_key(&mut keys, &key2);

        assert!(!changed);
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn test_module_name() {
        let module = AuthorizedKeyModule;
        assert_eq!(module.name(), "authorized_key");
    }

    #[test]
    fn test_module_classification() {
        let module = AuthorizedKeyModule;
        assert_eq!(
            module.classification(),
            ModuleClassification::NativeTransport
        );
    }

    #[test]
    fn test_module_required_params() {
        let module = AuthorizedKeyModule;
        assert_eq!(module.required_params(), &["user", "key"]);
    }

    #[test]
    fn test_validate_params_valid() {
        let module = AuthorizedKeyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("user".to_string(), serde_json::json!("testuser"));
        params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_invalid_user() {
        let module = AuthorizedKeyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("user".to_string(), serde_json::json!("user; rm -rf /"));
        params.insert("key".to_string(), serde_json::json!(TEST_RSA_KEY));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_empty_key() {
        let module = AuthorizedKeyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("user".to_string(), serde_json::json!("testuser"));
        params.insert("key".to_string(), serde_json::json!(""));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_parse_keys() {
        let lines = vec![
            "# Comment line".to_string(),
            "".to_string(),
            TEST_RSA_KEY.to_string(),
            TEST_ED25519_KEY.to_string(),
        ];

        let keys = AuthorizedKeyModule::parse_keys(&lines);

        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].key_type, "ssh-rsa");
        assert_eq!(keys[1].key_type, "ssh-ed25519");
    }
}
