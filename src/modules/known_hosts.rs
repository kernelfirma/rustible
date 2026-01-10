//! Known Hosts module - SSH known_hosts file management
//!
//! This module manages SSH known_hosts files for host key verification.
//! It supports adding, removing, and updating host keys with optional hashing.
//!
//! Features:
//! - Host key management (add, remove, update)
//! - Key scanning via ssh-keyscan or native implementation
//! - Hash support for hashed known_hosts entries (HashKnownHosts)
//! - Key rotation for updating existing host keys
//! - Support for multiple key types (rsa, ecdsa, ed25519)

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use once_cell::sync::Lazy;
use regex::Regex;
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Default known_hosts file path
const DEFAULT_KNOWN_HOSTS: &str = "~/.ssh/known_hosts";

/// Default SSH port
const DEFAULT_SSH_PORT: u16 = 22;

/// Default timeout for key scanning in seconds
const DEFAULT_SCAN_TIMEOUT: u64 = 5;

/// Regex for parsing known_hosts entries
static KNOWN_HOSTS_ENTRY_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Format: [markers] hostnames keytype base64key [comment]
    // Markers are optional: @cert-authority or @revoked
    // Hostnames can be: hostname, [hostname]:port, or hashed |1|salt|hash
    Regex::new(
        r"^(?:(@cert-authority|@revoked)\s+)?([^\s]+)\s+(ssh-rsa|ssh-ed25519|ssh-dss|ecdsa-sha2-nistp256|ecdsa-sha2-nistp384|ecdsa-sha2-nistp521|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com)\s+([A-Za-z0-9+/=]+)(?:\s+(.*))?$"
    ).expect("Invalid known_hosts regex")
});

/// Regex for detecting hashed hostnames
static HASHED_HOSTNAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\|1\|([A-Za-z0-9+/=]+)\|([A-Za-z0-9+/=]+)$")
        .expect("Invalid hashed hostname regex")
});

/// Desired state for a known_hosts entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownHostsState {
    /// Host key should be present
    Present,
    /// Host key should be absent
    Absent,
}

impl KnownHostsState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(KnownHostsState::Present),
            "absent" => Ok(KnownHostsState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Supported SSH key types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyType {
    Rsa,
    Ed25519,
    Dss,
    EcdsaNistp256,
    EcdsaNistp384,
    EcdsaNistp521,
    SkEd25519,
    SkEcdsa,
}

impl KeyType {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "rsa" | "ssh-rsa" => Ok(KeyType::Rsa),
            "ed25519" | "ssh-ed25519" => Ok(KeyType::Ed25519),
            "dss" | "ssh-dss" | "dsa" => Ok(KeyType::Dss),
            "ecdsa" | "ecdsa-sha2-nistp256" => Ok(KeyType::EcdsaNistp256),
            "ecdsa-sha2-nistp384" => Ok(KeyType::EcdsaNistp384),
            "ecdsa-sha2-nistp521" => Ok(KeyType::EcdsaNistp521),
            "sk-ssh-ed25519" | "sk-ed25519" | "sk-ssh-ed25519@openssh.com" => Ok(KeyType::SkEd25519),
            "sk-ecdsa-sha2-nistp256" | "sk-ecdsa" | "sk-ecdsa-sha2-nistp256@openssh.com" => Ok(KeyType::SkEcdsa),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid key type '{}'. Valid types: rsa, ed25519, dss, ecdsa, ecdsa-sha2-nistp256, ecdsa-sha2-nistp384, ecdsa-sha2-nistp521",
                s
            ))),
        }
    }

    pub fn as_ssh_keyscan_type(&self) -> &'static str {
        match self {
            KeyType::Rsa => "rsa",
            KeyType::Ed25519 => "ed25519",
            KeyType::Dss => "dsa",
            KeyType::EcdsaNistp256 | KeyType::EcdsaNistp384 | KeyType::EcdsaNistp521 => "ecdsa",
            KeyType::SkEd25519 => "ed25519-sk",
            KeyType::SkEcdsa => "ecdsa-sk",
        }
    }

    pub fn as_openssh_str(&self) -> &'static str {
        match self {
            KeyType::Rsa => "ssh-rsa",
            KeyType::Ed25519 => "ssh-ed25519",
            KeyType::Dss => "ssh-dss",
            KeyType::EcdsaNistp256 => "ecdsa-sha2-nistp256",
            KeyType::EcdsaNistp384 => "ecdsa-sha2-nistp384",
            KeyType::EcdsaNistp521 => "ecdsa-sha2-nistp521",
            KeyType::SkEd25519 => "sk-ssh-ed25519@openssh.com",
            KeyType::SkEcdsa => "sk-ecdsa-sha2-nistp256@openssh.com",
        }
    }

    pub fn from_openssh_str(s: &str) -> Option<Self> {
        match s {
            "ssh-rsa" => Some(KeyType::Rsa),
            "ssh-ed25519" => Some(KeyType::Ed25519),
            "ssh-dss" => Some(KeyType::Dss),
            "ecdsa-sha2-nistp256" => Some(KeyType::EcdsaNistp256),
            "ecdsa-sha2-nistp384" => Some(KeyType::EcdsaNistp384),
            "ecdsa-sha2-nistp521" => Some(KeyType::EcdsaNistp521),
            "sk-ssh-ed25519@openssh.com" => Some(KeyType::SkEd25519),
            "sk-ecdsa-sha2-nistp256@openssh.com" => Some(KeyType::SkEcdsa),
            _ => None,
        }
    }
}

/// Represents a single known_hosts entry
#[derive(Debug, Clone)]
pub struct KnownHostsEntry {
    /// Optional marker (@cert-authority or @revoked)
    pub marker: Option<String>,
    /// Hostnames/addresses (may be hashed)
    pub hostnames: String,
    /// Key type
    pub key_type: String,
    /// Base64-encoded public key
    pub key: String,
    /// Optional comment
    pub comment: Option<String>,
    /// Whether the hostname is hashed
    pub is_hashed: bool,
    /// Original line number (for tracking)
    pub line_number: Option<usize>,
}

impl KnownHostsEntry {
    /// Parse a known_hosts line into an entry
    pub fn parse(line: &str, line_number: Option<usize>) -> Option<Self> {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        if let Some(caps) = KNOWN_HOSTS_ENTRY_REGEX.captures(line) {
            let marker = caps.get(1).map(|m| m.as_str().to_string());
            let hostnames = caps.get(2).map(|m| m.as_str().to_string())?;
            let key_type = caps.get(3).map(|m| m.as_str().to_string())?;
            let key = caps.get(4).map(|m| m.as_str().to_string())?;
            let comment = caps.get(5).map(|m| m.as_str().to_string());

            let is_hashed = HASHED_HOSTNAME_REGEX.is_match(&hostnames);

            Some(KnownHostsEntry {
                marker,
                hostnames,
                key_type,
                key,
                comment,
                is_hashed,
                line_number,
            })
        } else {
            None
        }
    }

    /// Format the entry as a known_hosts line
    pub fn to_line(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref marker) = self.marker {
            parts.push(marker.clone());
        }

        parts.push(self.hostnames.clone());
        parts.push(self.key_type.clone());
        parts.push(self.key.clone());

        if let Some(ref comment) = self.comment {
            parts.push(comment.clone());
        }

        parts.join(" ")
    }

    /// Check if this entry matches a given hostname
    pub fn matches_hostname(&self, hostname: &str, port: Option<u16>) -> bool {
        let target = format_hostname_for_lookup(hostname, port);

        if self.is_hashed {
            // For hashed entries, we need to hash the target and compare
            match_hashed_hostname(&self.hostnames, &target)
        } else {
            // For plain entries, check against comma-separated hostnames
            self.hostnames
                .split(',')
                .any(|h| h.trim() == target || h.trim() == hostname)
        }
    }

    /// Check if this entry matches a given key type
    pub fn matches_key_type(&self, key_type: &KeyType) -> bool {
        self.key_type == key_type.as_openssh_str()
    }
}

/// Format a hostname for known_hosts lookup
fn format_hostname_for_lookup(hostname: &str, port: Option<u16>) -> String {
    match port {
        Some(p) if p != DEFAULT_SSH_PORT => format!("[{}]:{}", hostname, p),
        _ => hostname.to_string(),
    }
}

/// Format a hostname for storage in known_hosts
fn format_hostname_for_storage(hostname: &str, port: Option<u16>) -> String {
    format_hostname_for_lookup(hostname, port)
}

/// Check if a hashed hostname matches a target
fn match_hashed_hostname(hashed: &str, target: &str) -> bool {
    if let Some(caps) = HASHED_HOSTNAME_REGEX.captures(hashed) {
        if let (Some(salt_b64), Some(hash_b64)) = (caps.get(1), caps.get(2)) {
            if let Ok(salt) = BASE64.decode(salt_b64.as_str()) {
                // Compute HMAC-SHA1 of target using salt
                let computed_hash = compute_hostname_hash(target, &salt);
                let expected_hash = hash_b64.as_str();
                return computed_hash == expected_hash;
            }
        }
    }
    false
}

/// Compute HMAC-SHA1 hash of a hostname using a salt
fn compute_hostname_hash(hostname: &str, salt: &[u8]) -> String {
    use hmac::Hmac;
    use sha1::Sha1;

    type HmacSha1 = Hmac<Sha1>;

    let mut mac = HmacSha1::new_from_slice(salt).expect("HMAC can take key of any size");
    mac.update(hostname.as_bytes());
    let result = mac.finalize();
    BASE64.encode(result.into_bytes())
}

/// Generate a hashed hostname entry
fn hash_hostname(hostname: &str, port: Option<u16>) -> String {
    use rand::Rng;

    let target = format_hostname_for_storage(hostname, port);

    // Generate 20 random bytes for salt (SHA1 output size)
    let salt: [u8; 20] = rand::thread_rng().gen();
    let salt_b64 = BASE64.encode(&salt);
    let hash_b64 = compute_hostname_hash(&target, &salt);

    format!("|1|{}|{}", salt_b64, hash_b64)
}

/// HMAC implementation for hostname hashing
mod hmac {
    use sha1::{Digest, Sha1};

    pub struct Hmac<D> {
        inner: D,
        outer: D,
    }

    pub type Mac = Hmac<Sha1>;

    impl Hmac<Sha1> {
        pub fn new_from_slice(key: &[u8]) -> Result<Self, ()> {
            const BLOCK_SIZE: usize = 64;

            let mut key_block = [0u8; BLOCK_SIZE];
            if key.len() > BLOCK_SIZE {
                let mut hasher = Sha1::new();
                hasher.update(key);
                key_block[..20].copy_from_slice(&hasher.finalize());
            } else {
                key_block[..key.len()].copy_from_slice(key);
            }

            let mut inner_key = [0x36u8; BLOCK_SIZE];
            let mut outer_key = [0x5cu8; BLOCK_SIZE];

            for i in 0..BLOCK_SIZE {
                inner_key[i] ^= key_block[i];
                outer_key[i] ^= key_block[i];
            }

            let mut inner = Sha1::new();
            inner.update(&inner_key);

            let mut outer = Sha1::new();
            outer.update(&outer_key);

            Ok(Self { inner, outer })
        }

        pub fn update(&mut self, data: &[u8]) {
            self.inner.update(data);
        }

        pub fn finalize(self) -> HmacOutput {
            let inner_result = self.inner.finalize();
            let mut outer = self.outer;
            outer.update(&inner_result);
            HmacOutput {
                bytes: outer.finalize().into(),
            }
        }
    }

    pub struct HmacOutput {
        bytes: [u8; 20],
    }

    impl HmacOutput {
        pub fn into_bytes(self) -> [u8; 20] {
            self.bytes
        }
    }
}

/// Known hosts file manager
pub struct KnownHostsFile {
    path: PathBuf,
    pub entries: Vec<KnownHostsEntry>,
    comments: Vec<(usize, String)>, // Line number -> comment
}

impl KnownHostsFile {
    /// Load a known_hosts file
    pub fn load(path: &Path) -> ModuleResult<Self> {
        let expanded_path = expand_path(path)?;

        if !expanded_path.exists() {
            return Ok(Self {
                path: expanded_path,
                entries: Vec::new(),
                comments: Vec::new(),
            });
        }

        let file = fs::File::open(&expanded_path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        let mut comments = Vec::new();

        for (line_num, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                comments.push((line_num, line.clone()));
                continue;
            }

            if let Some(entry) = KnownHostsEntry::parse(&line, Some(line_num)) {
                entries.push(entry);
            }
        }

        Ok(Self {
            path: expanded_path,
            entries,
            comments,
        })
    }

    /// Save the known_hosts file
    pub fn save(&self) -> ModuleResult<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let lines: Vec<String> = self.entries.iter().map(|e| e.to_line()).collect();
        let content = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };

        fs::write(&self.path, content)?;

        // Set appropriate permissions (0600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Find entries matching a hostname and optional key type
    pub fn find_entries(
        &self,
        hostname: &str,
        port: Option<u16>,
        key_type: Option<&KeyType>,
    ) -> Vec<&KnownHostsEntry> {
        self.entries
            .iter()
            .filter(|e| {
                e.matches_hostname(hostname, port)
                    && key_type.map_or(true, |kt| e.matches_key_type(kt))
            })
            .collect()
    }

    /// Add a new entry
    pub fn add_entry(&mut self, entry: KnownHostsEntry) {
        self.entries.push(entry);
    }

    /// Remove entries matching hostname and optional key type
    pub fn remove_entries(
        &mut self,
        hostname: &str,
        port: Option<u16>,
        key_type: Option<&KeyType>,
    ) -> usize {
        let original_len = self.entries.len();
        self.entries.retain(|e| {
            !(e.matches_hostname(hostname, port)
                && key_type.map_or(true, |kt| e.matches_key_type(kt)))
        });
        original_len - self.entries.len()
    }

    /// Update an existing entry or add if not found
    pub fn update_or_add(
        &mut self,
        hostname: &str,
        port: Option<u16>,
        entry: KnownHostsEntry,
    ) -> bool {
        // Find and update existing entry with same hostname and key type
        for existing in &mut self.entries {
            if existing.matches_hostname(hostname, port) && existing.key_type == entry.key_type {
                if existing.key != entry.key {
                    existing.key = entry.key.clone();
                    existing.comment = entry.comment.clone();
                    return true;
                }
                return false; // Already up to date
            }
        }

        // Not found, add new entry
        self.entries.push(entry);
        true
    }
}

/// Expand ~ and environment variables in path
fn expand_path(path: &Path) -> ModuleResult<PathBuf> {
    let path_str = path.to_string_lossy();
    let expanded = shellexpand::tilde(&path_str);
    Ok(PathBuf::from(expanded.as_ref()))
}

/// Scan a host for SSH keys using ssh-keyscan
pub fn scan_host_keys(
    hostname: &str,
    port: Option<u16>,
    key_types: Option<&[KeyType]>,
    timeout: Duration,
) -> ModuleResult<Vec<KnownHostsEntry>> {
    let port = port.unwrap_or(DEFAULT_SSH_PORT);

    // Build ssh-keyscan command
    let mut cmd = Command::new("ssh-keyscan");
    cmd.arg("-T").arg(timeout.as_secs().to_string());
    cmd.arg("-p").arg(port.to_string());

    // Add key type filter if specified
    if let Some(types) = key_types {
        let type_str: Vec<&str> = types.iter().map(|t| t.as_ssh_keyscan_type()).collect();
        cmd.arg("-t").arg(type_str.join(","));
    }

    cmd.arg(hostname);

    let output = cmd.output().map_err(|e| {
        ModuleError::ExecutionFailed(format!("Failed to execute ssh-keyscan: {}", e))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // ssh-keyscan returns non-zero even on partial success, check if we got any output
        if output.stdout.is_empty() {
            return Err(ModuleError::ExecutionFailed(format!(
                "ssh-keyscan failed: {}",
                stderr
            )));
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();

    for line in stdout.lines() {
        if let Some(entry) = KnownHostsEntry::parse(line, None) {
            entries.push(entry);
        }
    }

    if entries.is_empty() {
        return Err(ModuleError::ExecutionFailed(format!(
            "No SSH keys found for host {}:{}",
            hostname, port
        )));
    }

    Ok(entries)
}

/// Check if a host is reachable on SSH port
fn check_host_reachable(hostname: &str, port: u16, timeout: Duration) -> bool {
    let addr = format!("{}:{}", hostname, port);
    TcpStream::connect_timeout(
        &addr
            .parse()
            .unwrap_or_else(|_| format!("0.0.0.0:{}", port).parse().unwrap()),
        timeout,
    )
    .is_ok()
}

/// Module for known_hosts management
pub struct KnownHostsModule;

impl KnownHostsModule {
    /// Get the default known_hosts path
    fn default_path() -> PathBuf {
        PathBuf::from(DEFAULT_KNOWN_HOSTS)
    }

    /// Create backup of known_hosts file
    fn create_backup(path: &Path, suffix: &str) -> ModuleResult<Option<String>> {
        let expanded = expand_path(path)?;
        if expanded.exists() {
            let backup_path = format!("{}{}", expanded.display(), suffix);
            fs::copy(&expanded, &backup_path)?;
            Ok(Some(backup_path))
        } else {
            Ok(None)
        }
    }
}

impl Module for KnownHostsModule {
    fn name(&self) -> &'static str {
        "known_hosts"
    }

    fn description(&self) -> &'static str {
        "Manage SSH known_hosts file entries"
    }

    fn classification(&self) -> ModuleClassification {
        // This module runs locally on the control node
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Rate limit due to potential network operations (key scanning)
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Parse parameters
        let hostname = params.get_string_required("name")?;
        let state = params
            .get_string("state")?
            .map(|s| KnownHostsState::from_str(&s))
            .transpose()?
            .unwrap_or(KnownHostsState::Present);

        let path = params
            .get_string("path")?
            .map(PathBuf::from)
            .unwrap_or_else(Self::default_path);

        let port = params.get_u32("port")?.map(|p| p as u16);

        let key_type = params
            .get_string("key_type")?
            .or_else(|| params.get_string("key").ok().flatten())
            .map(|s| KeyType::from_str(&s))
            .transpose()?;

        let key_data = params.get_string("key_data")?;

        let hash_host = params.get_bool_or("hash_host", false);

        let scan = params.get_bool_or("scan", true);

        let timeout = params
            .get_u32("timeout")?
            .map(|t| Duration::from_secs(t as u64))
            .unwrap_or(Duration::from_secs(DEFAULT_SCAN_TIMEOUT));

        let backup = params.get_string("backup")?;

        // Load known_hosts file
        let mut known_hosts = KnownHostsFile::load(&path)?;

        match state {
            KnownHostsState::Absent => {
                // Remove matching entries
                let existing = known_hosts.find_entries(&hostname, port, key_type.as_ref());

                if existing.is_empty() {
                    return Ok(ModuleOutput::ok(format!(
                        "No entries found for {} in {}",
                        hostname,
                        path.display()
                    )));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would remove {} entries for {} from {}",
                        existing.len(),
                        hostname,
                        path.display()
                    )));
                }

                // Create backup if requested
                if let Some(ref suffix) = backup {
                    Self::create_backup(&path, suffix)?;
                }

                let removed = known_hosts.remove_entries(&hostname, port, key_type.as_ref());
                known_hosts.save()?;

                Ok(ModuleOutput::changed(format!(
                    "Removed {} entries for {} from {}",
                    removed,
                    hostname,
                    path.display()
                ))
                .with_data("removed_count", serde_json::json!(removed)))
            }

            KnownHostsState::Present => {
                // Determine the key to add
                let entries_to_add = if let Some(ref key_data) = key_data {
                    // Use provided key data
                    let key_type = key_type.ok_or_else(|| {
                        ModuleError::MissingParameter(
                            "key_type is required when key_data is provided".to_string(),
                        )
                    })?;

                    let formatted_hostname = if hash_host {
                        hash_hostname(&hostname, port)
                    } else {
                        format_hostname_for_storage(&hostname, port)
                    };

                    vec![KnownHostsEntry {
                        marker: None,
                        hostnames: formatted_hostname,
                        key_type: key_type.as_openssh_str().to_string(),
                        key: key_data.clone(),
                        comment: None,
                        is_hashed: hash_host,
                        line_number: None,
                    }]
                } else if scan {
                    // Scan the host for keys
                    let key_types = key_type.as_ref().map(|kt| vec![*kt]);
                    let mut scanned =
                        scan_host_keys(&hostname, port, key_types.as_deref(), timeout)?;

                    // Apply hashing if requested
                    if hash_host {
                        for entry in &mut scanned {
                            entry.hostnames = hash_hostname(&hostname, port);
                            entry.is_hashed = true;
                        }
                    }

                    scanned
                } else {
                    return Err(ModuleError::InvalidParameter(
                        "Either key_data must be provided or scan must be enabled".to_string(),
                    ));
                };

                // Check if entries already exist and are up-to-date
                let mut changes_needed = false;
                for entry in &entries_to_add {
                    let kt = KeyType::from_openssh_str(&entry.key_type);
                    let existing = known_hosts.find_entries(&hostname, port, kt.as_ref());

                    if existing.is_empty() {
                        changes_needed = true;
                        break;
                    }

                    // Check if key matches
                    for ex in existing {
                        if ex.key != entry.key {
                            changes_needed = true;
                            break;
                        }
                    }

                    if changes_needed {
                        break;
                    }
                }

                if !changes_needed {
                    return Ok(ModuleOutput::ok(format!(
                        "Host key for {} already present in {}",
                        hostname,
                        path.display()
                    )));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would add/update {} key(s) for {} in {}",
                        entries_to_add.len(),
                        hostname,
                        path.display()
                    )));
                }

                // Create backup if requested
                if let Some(ref suffix) = backup {
                    Self::create_backup(&path, suffix)?;
                }

                // Add or update entries
                let mut added = 0;
                let mut updated = 0;

                for entry in entries_to_add {
                    if known_hosts.update_or_add(&hostname, port, entry) {
                        // Check if it was an update or add
                        let kt = KeyType::from_openssh_str(
                            &known_hosts.entries.last().unwrap().key_type,
                        );
                        let existing_count =
                            known_hosts.find_entries(&hostname, port, kt.as_ref()).len();
                        if existing_count == 1 {
                            added += 1;
                        } else {
                            updated += 1;
                        }
                    }
                }

                known_hosts.save()?;

                let msg = if updated > 0 {
                    format!(
                        "Updated {} and added {} key(s) for {} in {}",
                        updated,
                        added,
                        hostname,
                        path.display()
                    )
                } else {
                    format!(
                        "Added {} key(s) for {} in {}",
                        added,
                        hostname,
                        path.display()
                    )
                };

                Ok(ModuleOutput::changed(msg)
                    .with_data("added_count", serde_json::json!(added))
                    .with_data("updated_count", serde_json::json!(updated)))
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_parse_known_hosts_entry() {
        let line = "github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
        let entry = KnownHostsEntry::parse(line, Some(0)).unwrap();

        assert_eq!(entry.hostnames, "github.com");
        assert_eq!(entry.key_type, "ssh-ed25519");
        assert!(!entry.is_hashed);
    }

    #[test]
    fn test_parse_known_hosts_entry_with_port() {
        // Use valid base64 key data (the regex requires valid base64 characters only)
        let line = "[example.com]:2222 ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7test";
        let entry = KnownHostsEntry::parse(line, None).unwrap();

        assert_eq!(entry.hostnames, "[example.com]:2222");
        assert_eq!(entry.key_type, "ssh-rsa");
    }

    #[test]
    fn test_parse_hashed_entry() {
        // Use valid base64 key data (the regex requires valid base64 characters only)
        let line = "|1|F3GJvMX9f3ByPm4MQq5R7S7E/wY=|hXGJd+SqtTeGJ8jELEmYvNF0J24= ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBtest";
        let entry = KnownHostsEntry::parse(line, None).unwrap();

        assert!(entry.is_hashed);
        assert_eq!(entry.key_type, "ssh-ed25519");
    }

    #[test]
    fn test_key_type_conversion() {
        assert_eq!(
            KeyType::from_str("rsa").unwrap().as_openssh_str(),
            "ssh-rsa"
        );
        assert_eq!(
            KeyType::from_str("ed25519").unwrap().as_openssh_str(),
            "ssh-ed25519"
        );
        assert_eq!(
            KeyType::from_str("ecdsa").unwrap().as_openssh_str(),
            "ecdsa-sha2-nistp256"
        );
    }

    #[test]
    fn test_format_hostname() {
        assert_eq!(
            format_hostname_for_storage("example.com", None),
            "example.com"
        );
        assert_eq!(
            format_hostname_for_storage("example.com", Some(22)),
            "example.com"
        );
        assert_eq!(
            format_hostname_for_storage("example.com", Some(2222)),
            "[example.com]:2222"
        );
    }

    #[test]
    fn test_known_hosts_file_operations() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("known_hosts");

        // Create initial file
        fs::write(
            &path,
            "github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl\n",
        )
        .unwrap();

        let known_hosts = KnownHostsFile::load(&path).unwrap();
        assert_eq!(known_hosts.entries.len(), 1);

        let entries = known_hosts.find_entries("github.com", None, None);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_module_absent_state() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("known_hosts");

        // Create file with entry
        fs::write(
            &path,
            "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test123\n",
        )
        .unwrap();

        let module = KnownHostsModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("example.com"));
        params.insert("state".to_string(), serde_json::json!("absent"));
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);

        // Verify entry was removed
        let known_hosts = KnownHostsFile::load(&path).unwrap();
        assert!(known_hosts.entries.is_empty());
    }

    #[test]
    fn test_module_present_with_key_data() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("known_hosts");

        let module = KnownHostsModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("example.com"));
        params.insert("state".to_string(), serde_json::json!("present"));
        params.insert("key_type".to_string(), serde_json::json!("ed25519"));
        params.insert(
            "key_data".to_string(),
            serde_json::json!("AAAAC3NzaC1lZDI1NTE5testkey"),
        );
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);

        // Verify entry was added
        let known_hosts = KnownHostsFile::load(&path).unwrap();
        assert_eq!(known_hosts.entries.len(), 1);
        assert_eq!(known_hosts.entries[0].hostnames, "example.com");
    }

    #[test]
    fn test_module_check_mode() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("known_hosts");

        // Create file with entry
        fs::write(
            &path,
            "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5test123\n",
        )
        .unwrap();

        let module = KnownHostsModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("example.com"));
        params.insert("state".to_string(), serde_json::json!("absent"));
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would remove"));

        // Verify entry was NOT removed
        let known_hosts = KnownHostsFile::load(&path).unwrap();
        assert_eq!(known_hosts.entries.len(), 1);
    }

    #[test]
    fn test_hash_hostname() {
        let hashed = hash_hostname("example.com", None);
        assert!(hashed.starts_with("|1|"));

        // Verify it has the right format
        assert!(HASHED_HOSTNAME_REGEX.is_match(&hashed));
    }

    #[test]
    fn test_hostname_matching() {
        let entry = KnownHostsEntry {
            marker: None,
            hostnames: "github.com,192.30.255.113".to_string(),
            key_type: "ssh-ed25519".to_string(),
            key: "testkey".to_string(),
            comment: None,
            is_hashed: false,
            line_number: None,
        };

        assert!(entry.matches_hostname("github.com", None));
        assert!(entry.matches_hostname("192.30.255.113", None));
        assert!(!entry.matches_hostname("gitlab.com", None));
    }
}
