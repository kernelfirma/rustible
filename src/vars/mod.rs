//! Variable system for Rustible.
//!
//! This module provides comprehensive variable management including:
//! - Variable precedence (similar to Ansible's 22 levels)
//! - Variable merging
//! - Vault-like secret handling

pub mod scope;
pub mod terraform;

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use indexmap::IndexMap;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Variable precedence levels (from lowest to highest)
/// Based on Ansible's variable precedence
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum VarPrecedence {
    /// Role defaults (lowest priority)
    RoleDefaults = 1,
    /// Dynamic inventory group vars
    InventoryGroupVars = 2,
    /// Inventory file group vars
    InventoryFileGroupVars = 3,
    /// Playbook group_vars/all
    PlaybookGroupVarsAll = 4,
    /// Playbook group_vars/* (specific group)
    PlaybookGroupVars = 5,
    /// Dynamic inventory host vars
    InventoryHostVars = 6,
    /// Inventory file host vars
    InventoryFileHostVars = 7,
    /// Playbook host_vars/*
    PlaybookHostVars = 8,
    /// Host facts / cached set_facts
    HostFacts = 9,
    /// Play vars
    PlayVars = 10,
    /// Play vars_prompt
    PlayVarsPrompt = 11,
    /// Play vars_files
    PlayVarsFiles = 12,
    /// Role vars (from role's vars/main.yml)
    RoleVars = 13,
    /// Block vars
    BlockVars = 14,
    /// Task vars (only for the specific task)
    TaskVars = 15,
    /// Include vars
    IncludeVars = 16,
    /// set_facts / registered vars
    SetFacts = 17,
    /// Role params (when including role)
    RoleParams = 18,
    /// Include params
    IncludeParams = 19,
    /// Extra vars (--extra-vars, -e) - highest priority
    ExtraVars = 20,
}

impl VarPrecedence {
    /// Get all precedence levels in order (lowest to highest)
    pub fn all() -> impl Iterator<Item = VarPrecedence> {
        [
            VarPrecedence::RoleDefaults,
            VarPrecedence::InventoryGroupVars,
            VarPrecedence::InventoryFileGroupVars,
            VarPrecedence::PlaybookGroupVarsAll,
            VarPrecedence::PlaybookGroupVars,
            VarPrecedence::InventoryHostVars,
            VarPrecedence::InventoryFileHostVars,
            VarPrecedence::PlaybookHostVars,
            VarPrecedence::HostFacts,
            VarPrecedence::PlayVars,
            VarPrecedence::PlayVarsPrompt,
            VarPrecedence::PlayVarsFiles,
            VarPrecedence::RoleVars,
            VarPrecedence::BlockVars,
            VarPrecedence::TaskVars,
            VarPrecedence::IncludeVars,
            VarPrecedence::SetFacts,
            VarPrecedence::RoleParams,
            VarPrecedence::IncludeParams,
            VarPrecedence::ExtraVars,
        ]
        .into_iter()
    }

    /// Get the precedence level number
    pub fn level(&self) -> u8 {
        *self as u8
    }
}

impl std::fmt::Display for VarPrecedence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            VarPrecedence::RoleDefaults => "role defaults",
            VarPrecedence::InventoryGroupVars => "inventory group vars",
            VarPrecedence::InventoryFileGroupVars => "inventory file group vars",
            VarPrecedence::PlaybookGroupVarsAll => "playbook group_vars/all",
            VarPrecedence::PlaybookGroupVars => "playbook group_vars/*",
            VarPrecedence::InventoryHostVars => "inventory host vars",
            VarPrecedence::InventoryFileHostVars => "inventory file host vars",
            VarPrecedence::PlaybookHostVars => "playbook host_vars/*",
            VarPrecedence::HostFacts => "host facts",
            VarPrecedence::PlayVars => "play vars",
            VarPrecedence::PlayVarsPrompt => "play vars_prompt",
            VarPrecedence::PlayVarsFiles => "play vars_files",
            VarPrecedence::RoleVars => "role vars",
            VarPrecedence::BlockVars => "block vars",
            VarPrecedence::TaskVars => "task vars",
            VarPrecedence::IncludeVars => "include vars",
            VarPrecedence::SetFacts => "set_facts",
            VarPrecedence::RoleParams => "role params",
            VarPrecedence::IncludeParams => "include params",
            VarPrecedence::ExtraVars => "extra vars",
        };
        write!(f, "{}", name)
    }
}

/// Errors that can occur in the variable system
#[derive(Debug, Error)]
pub enum VarsError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("undefined variable: {0}")]
    UndefinedVariable(String),

    #[error("vault error: {0}")]
    VaultError(String),

    #[error("invalid vault format")]
    InvalidVaultFormat,

    #[error("encryption error: {0}")]
    EncryptionError(String),

    #[error("decryption error: {0}")]
    DecryptionError(String),

    #[error("merge error: {0}")]
    MergeError(String),

    #[error("import error: {0}")]
    ImportError(String),

    #[error("type error: expected {expected}, got {actual}")]
    TypeError { expected: String, actual: String },
}

/// Result type for variable operations
pub type VarsResult<T> = Result<T, VarsError>;

/// A variable with its source/precedence information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    /// The variable value
    pub value: serde_yaml::Value,

    /// Source precedence level
    pub precedence: VarPrecedence,

    /// Source file (if applicable)
    pub source: Option<String>,

    /// Whether this is an encrypted vault value
    pub encrypted: bool,
}

impl Variable {
    /// Create a new variable
    pub fn new(value: serde_yaml::Value, precedence: VarPrecedence) -> Self {
        Self {
            value,
            precedence,
            source: None,
            encrypted: false,
        }
    }

    /// Create a variable with source information
    pub fn with_source(
        value: serde_yaml::Value,
        precedence: VarPrecedence,
        source: impl Into<String>,
    ) -> Self {
        Self {
            value,
            precedence,
            source: Some(source.into()),
            encrypted: false,
        }
    }

    /// Mark as encrypted
    pub fn encrypted(mut self) -> Self {
        self.encrypted = true;
        self
    }
}

/// Hash strategy for merging variables
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HashBehaviour {
    /// Replace hash entirely (Ansible default)
    #[default]
    Replace,
    /// Merge hashes recursively
    Merge,
}

/// The main variable store
#[derive(Debug, Clone, Default)]
pub struct VarStore {
    /// Variables organized by precedence level
    layers: HashMap<VarPrecedence, IndexMap<String, Variable>>,

    /// Cached merged variables (invalidated on changes)
    merged_cache: Option<IndexMap<String, serde_yaml::Value>>,

    /// Hash merge behavior
    hash_behaviour: HashBehaviour,

    /// Vault password for encrypted values
    vault_password: Option<String>,
}

impl VarStore {
    /// Create a new empty variable store
    pub fn new() -> Self {
        Self {
            layers: HashMap::new(),
            merged_cache: None,
            hash_behaviour: HashBehaviour::Replace,
            vault_password: None,
        }
    }

    /// Create a variable store with merge behavior
    pub fn with_hash_behaviour(hash_behaviour: HashBehaviour) -> Self {
        Self {
            hash_behaviour,
            ..Default::default()
        }
    }

    /// Set the vault password
    pub fn set_vault_password(&mut self, password: impl Into<String>) {
        self.vault_password = Some(password.into());
    }

    /// Set a variable at a specific precedence level
    pub fn set(
        &mut self,
        key: impl Into<String>,
        value: serde_yaml::Value,
        precedence: VarPrecedence,
    ) {
        self.merged_cache = None; // Invalidate cache

        let layer = self.layers.entry(precedence).or_default();
        layer.insert(key.into(), Variable::new(value, precedence));
    }

    /// Set a variable with full metadata
    pub fn set_variable(&mut self, key: impl Into<String>, variable: Variable) {
        self.merged_cache = None;

        let layer = self.layers.entry(variable.precedence).or_default();
        layer.insert(key.into(), variable);
    }

    /// Set multiple variables at a precedence level
    pub fn set_many(
        &mut self,
        vars: IndexMap<String, serde_yaml::Value>,
        precedence: VarPrecedence,
    ) {
        self.merged_cache = None;

        let layer = self.layers.entry(precedence).or_default();
        for (key, value) in vars {
            layer.insert(key, Variable::new(value, precedence));
        }
    }

    /// Set multiple variables with source information
    pub fn set_many_from_file<P: AsRef<Path>>(
        &mut self,
        vars: IndexMap<String, serde_yaml::Value>,
        precedence: VarPrecedence,
        source: P,
    ) {
        self.merged_cache = None;

        let source_str = source.as_ref().display().to_string();
        let layer = self.layers.entry(precedence).or_default();

        for (key, value) in vars {
            layer.insert(key, Variable::with_source(value, precedence, &source_str));
        }
    }

    /// Get a variable (considering precedence)
    pub fn get(&mut self, key: &str) -> Option<&serde_yaml::Value> {
        self.ensure_merged();
        self.merged_cache.as_ref().and_then(|cache| cache.get(key))
    }

    /// Get the raw Variable with metadata
    pub fn get_variable(&self, key: &str) -> Option<&Variable> {
        // Find the variable at the highest precedence level
        for precedence in VarPrecedence::all().collect::<Vec<_>>().into_iter().rev() {
            if let Some(layer) = self.layers.get(&precedence) {
                if let Some(var) = layer.get(key) {
                    return Some(var);
                }
            }
        }
        None
    }

    /// Check if a variable exists
    pub fn contains(&mut self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Remove a variable from a specific precedence level
    pub fn remove(&mut self, key: &str, precedence: VarPrecedence) -> Option<Variable> {
        self.merged_cache = None;

        self.layers
            .get_mut(&precedence)
            .and_then(|layer| layer.swap_remove(key))
    }

    /// Clear all variables at a specific precedence level
    pub fn clear_precedence(&mut self, precedence: VarPrecedence) {
        self.merged_cache = None;
        self.layers.remove(&precedence);
    }

    /// Clear all variables
    pub fn clear(&mut self) {
        self.merged_cache = None;
        self.layers.clear();
    }

    /// Get the current hash behaviour
    pub fn hash_behaviour(&self) -> HashBehaviour {
        self.hash_behaviour
    }

    /// Check if the merged cache is valid
    pub fn is_cache_valid(&self) -> bool {
        self.merged_cache.is_some()
    }

    /// Get all merged variables
    pub fn all(&mut self) -> &IndexMap<String, serde_yaml::Value> {
        self.ensure_merged();
        self.merged_cache.as_ref().unwrap()
    }

    /// Get all variable names
    pub fn keys(&mut self) -> impl Iterator<Item = &String> {
        self.ensure_merged();
        self.merged_cache.as_ref().unwrap().keys()
    }

    /// Ensure the merged cache is up to date
    fn ensure_merged(&mut self) {
        if self.merged_cache.is_some() {
            return;
        }

        let mut merged = IndexMap::new();

        // Apply variables in precedence order (lowest to highest)
        for precedence in VarPrecedence::all() {
            if let Some(layer) = self.layers.get(&precedence) {
                for (key, var) in layer {
                    self.merge_value(&mut merged, key, &var.value);
                }
            }
        }

        self.merged_cache = Some(merged);
    }

    /// Merge a value into the merged map
    fn merge_value(
        &self,
        merged: &mut IndexMap<String, serde_yaml::Value>,
        key: &str,
        value: &serde_yaml::Value,
    ) {
        match self.hash_behaviour {
            HashBehaviour::Replace => {
                merged.insert(key.to_string(), value.clone());
            }
            HashBehaviour::Merge => {
                if let Some(existing) = merged.get_mut(key) {
                    deep_merge_in_place(existing, value);
                } else {
                    merged.insert(key.to_string(), value.clone());
                }
            }
        }
    }

    /// Load variables from a YAML file
    pub fn load_file<P: AsRef<Path>>(
        &mut self,
        path: P,
        precedence: VarPrecedence,
    ) -> VarsResult<()> {
        let content = std::fs::read_to_string(&path)?;

        // Check if it's vault encrypted
        if content.starts_with("$ANSIBLE_VAULT;") {
            let decrypted = self.decrypt_vault(&content)?;
            let vars: IndexMap<String, serde_yaml::Value> = serde_yaml::from_str(&decrypted)?;
            self.set_many_from_file(vars, precedence, &path);
        } else {
            let vars: IndexMap<String, serde_yaml::Value> = serde_yaml::from_str(&content)?;
            self.set_many_from_file(vars, precedence, &path);
        }

        Ok(())
    }

    /// Decrypt a vault-encrypted string
    fn decrypt_vault(&self, content: &str) -> VarsResult<String> {
        let password = self
            .vault_password
            .as_ref()
            .ok_or_else(|| VarsError::VaultError("No vault password set".to_string()))?;

        Vault::decrypt(content, password)
    }

    /// Create a child scope with additional variables
    #[allow(mismatched_lifetime_syntaxes)]
    pub fn scope(&self) -> VarScope {
        VarScope::new(self)
    }

    /// Get variable count
    pub fn len(&mut self) -> usize {
        self.ensure_merged();
        self.merged_cache.as_ref().map(|c| c.len()).unwrap_or(0)
    }

    /// Check if empty
    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }
}

/// A child scope for temporary variable additions
#[derive(Debug)]
pub struct VarScope<'a> {
    /// Parent store
    parent: &'a VarStore,

    /// Local overrides
    local: IndexMap<String, serde_yaml::Value>,
}

impl<'a> VarScope<'a> {
    /// Create a new scope
    fn new(parent: &'a VarStore) -> Self {
        Self {
            parent,
            local: IndexMap::new(),
        }
    }

    /// Set a local variable
    pub fn set(&mut self, key: impl Into<String>, value: serde_yaml::Value) {
        self.local.insert(key.into(), value);
    }

    /// Set multiple local variables
    pub fn set_many(&mut self, vars: IndexMap<String, serde_yaml::Value>) {
        for (key, value) in vars {
            self.local.insert(key, value);
        }
    }

    /// Get a variable (local overrides parent)
    pub fn get(&self, key: &str) -> Option<&serde_yaml::Value> {
        self.local
            .get(key)
            .or_else(|| self.parent.get_variable(key).map(|v| &v.value))
    }

    /// Get all merged variables
    pub fn all(&self) -> IndexMap<String, serde_yaml::Value> {
        let mut merged = IndexMap::new();

        // Get parent variables
        for precedence in VarPrecedence::all() {
            if let Some(layer) = self.parent.layers.get(&precedence) {
                for (key, var) in layer {
                    merged.insert(key.clone(), var.value.clone());
                }
            }
        }

        // Apply local overrides
        for (key, value) in &self.local {
            merged.insert(key.clone(), value.clone());
        }

        merged
    }
}

/// Deep merge two YAML values
pub fn deep_merge(base: &serde_yaml::Value, overlay: &serde_yaml::Value) -> serde_yaml::Value {
    let mut merged = base.clone();
    deep_merge_in_place(&mut merged, overlay);
    merged
}

/// Deep merge two YAML values in place
pub fn deep_merge_in_place(base: &mut serde_yaml::Value, overlay: &serde_yaml::Value) {
    match (base, overlay) {
        (serde_yaml::Value::Mapping(base_map), serde_yaml::Value::Mapping(overlay_map)) => {
            for (key, value) in overlay_map {
                if let Some(base_value) = base_map.get_mut(key) {
                    deep_merge_in_place(base_value, value);
                } else {
                    base_map.insert(key.clone(), value.clone());
                }
            }
        }
        (base_val, overlay_val) => {
            // For non-mappings, overlay wins
            *base_val = overlay_val.clone();
        }
    }
}

/// Vault encryption/decryption utilities
pub struct Vault;

impl Vault {
    /// Vault header format
    const HEADER: &'static str = "$ANSIBLE_VAULT;1.1;AES256";

    /// Encrypt content with a password
    pub fn encrypt(content: &str, password: &str) -> VarsResult<String> {
        // Derive key from password using Argon2
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| VarsError::EncryptionError(e.to_string()))?;

        let key_bytes = password_hash
            .hash
            .ok_or_else(|| VarsError::EncryptionError("Failed to generate key".to_string()))?;

        // Use first 32 bytes as AES-256 key
        let key_slice = &key_bytes.as_bytes()[..32];
        let cipher = Aes256Gcm::new_from_slice(key_slice)
            .map_err(|e| VarsError::EncryptionError(e.to_string()))?;

        // Generate random nonce
        use rand::RngCore;
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = cipher
            .encrypt(nonce, content.as_bytes())
            .map_err(|e| VarsError::EncryptionError(e.to_string()))?;

        // Encode as hex
        use base64::Engine;
        let encoded_salt = salt.as_str().to_string();
        let encoded_nonce = base64::engine::general_purpose::STANDARD.encode(nonce_bytes);
        let encoded_ciphertext = base64::engine::general_purpose::STANDARD.encode(&ciphertext);

        // Format as vault file
        let vault_content = format!(
            "{}\n{}\n{}\n{}",
            Self::HEADER,
            encoded_salt,
            encoded_nonce,
            encoded_ciphertext
        );

        Ok(vault_content)
    }

    /// Decrypt vault content with a password
    pub fn decrypt(content: &str, password: &str) -> VarsResult<String> {
        let lines: Vec<&str> = content.lines().collect();

        if lines.len() < 4 {
            return Err(VarsError::InvalidVaultFormat);
        }

        // Verify header
        if !lines[0].starts_with("$ANSIBLE_VAULT;") {
            return Err(VarsError::InvalidVaultFormat);
        }

        // Parse components
        let salt_str = lines[1];
        let salt = SaltString::from_b64(salt_str).map_err(|_| VarsError::InvalidVaultFormat)?;

        use base64::Engine;
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(lines[2])
            .map_err(|_| VarsError::InvalidVaultFormat)?;

        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(lines[3..].join(""))
            .map_err(|_| VarsError::InvalidVaultFormat)?;

        // Derive key from password
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| VarsError::DecryptionError(e.to_string()))?;

        let key_bytes = password_hash
            .hash
            .ok_or_else(|| VarsError::DecryptionError("Failed to derive key".to_string()))?;

        // Decrypt
        let key_slice = &key_bytes.as_bytes()[..32];
        let cipher = Aes256Gcm::new_from_slice(key_slice)
            .map_err(|e| VarsError::DecryptionError(e.to_string()))?;

        let nonce = Nonce::from_slice(&nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| VarsError::DecryptionError(e.to_string()))?;

        String::from_utf8(plaintext).map_err(|e| VarsError::DecryptionError(e.to_string()))
    }

    /// Check if content is vault encrypted
    pub fn is_encrypted(content: &str) -> bool {
        content.trim_start().starts_with("$ANSIBLE_VAULT;")
    }

    /// Encrypt a file in place
    pub fn encrypt_file<P: AsRef<Path>>(path: P, password: &str) -> VarsResult<()> {
        let content = std::fs::read_to_string(&path)?;

        if Self::is_encrypted(&content) {
            return Err(VarsError::VaultError(
                "File is already encrypted".to_string(),
            ));
        }

        let encrypted = Self::encrypt(&content, password)?;
        std::fs::write(path, encrypted)?;

        Ok(())
    }

    /// Decrypt a file in place
    pub fn decrypt_file<P: AsRef<Path>>(path: P, password: &str) -> VarsResult<()> {
        let content = std::fs::read_to_string(&path)?;

        if !Self::is_encrypted(&content) {
            return Err(VarsError::VaultError("File is not encrypted".to_string()));
        }

        let decrypted = Self::decrypt(&content, password)?;
        std::fs::write(path, decrypted)?;

        Ok(())
    }

    /// View encrypted file contents without modifying
    pub fn view_file<P: AsRef<Path>>(path: P, password: &str) -> VarsResult<String> {
        let content = std::fs::read_to_string(&path)?;
        Self::decrypt(&content, password)
    }
}

/// Inline vault encryption marker
const VAULT_INLINE_PREFIX: &str = "!vault |";

/// Parse inline vault values from YAML
pub fn parse_inline_vault(
    value: &serde_yaml::Value,
    password: &str,
) -> VarsResult<serde_yaml::Value> {
    match value {
        serde_yaml::Value::String(s) if s.starts_with(VAULT_INLINE_PREFIX) => {
            let encrypted = s.trim_start_matches(VAULT_INLINE_PREFIX).trim();
            let decrypted = Vault::decrypt(encrypted, password)?;
            Ok(serde_yaml::Value::String(decrypted))
        }
        serde_yaml::Value::Mapping(map) => {
            let mut result = serde_yaml::Mapping::new();
            for (k, v) in map {
                result.insert(k.clone(), parse_inline_vault(v, password)?);
            }
            Ok(serde_yaml::Value::Mapping(result))
        }
        serde_yaml::Value::Sequence(seq) => {
            let result: VarsResult<Vec<_>> = seq
                .iter()
                .map(|v| parse_inline_vault(v, password))
                .collect();
            Ok(serde_yaml::Value::Sequence(result?))
        }
        _ => Ok(value.clone()),
    }
}

/// Variable resolution helpers
pub mod resolve {
    #[allow(unused_imports)]
    use super::*;

    /// Resolve a variable path (e.g., "foo.bar.baz")
    pub fn resolve_path<'a>(
        value: &'a serde_yaml::Value,
        path: &str,
    ) -> Option<&'a serde_yaml::Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = value;

        for part in parts {
            match current {
                serde_yaml::Value::Mapping(map) => {
                    current = map.get(serde_yaml::Value::String(part.to_string()))?;
                }
                serde_yaml::Value::Sequence(seq) => {
                    let index: usize = part.parse().ok()?;
                    current = seq.get(index)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }

    /// Set a value at a path (creating intermediate structures)
    pub fn set_path(
        value: &mut serde_yaml::Value,
        path: &str,
        new_value: serde_yaml::Value,
    ) -> bool {
        let parts: Vec<&str> = path.split('.').collect();

        if parts.is_empty() {
            return false;
        }

        let mut current = value;

        for (i, part) in parts.iter().enumerate() {
            let is_last = i == parts.len() - 1;

            if is_last {
                if let serde_yaml::Value::Mapping(map) = current {
                    map.insert(
                        serde_yaml::Value::String(part.to_string()),
                        new_value.clone(),
                    );
                    return true;
                }
                return false;
            }

            // Navigate or create intermediate maps
            if let serde_yaml::Value::Mapping(map) = current {
                let key = serde_yaml::Value::String(part.to_string());
                if !map.contains_key(&key) {
                    map.insert(
                        key.clone(),
                        serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
                    );
                }
                current = map.get_mut(&key).unwrap();
            } else {
                return false;
            }
        }

        false
    }

    /// Convert a value to string
    pub fn to_string(value: &serde_yaml::Value) -> String {
        match value {
            serde_yaml::Value::Null => String::new(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::String(s) => s.clone(),
            _ => serde_yaml::to_string(value).unwrap_or_default(),
        }
    }

    /// Convert a value to boolean
    pub fn to_bool(value: &serde_yaml::Value) -> Option<bool> {
        match value {
            serde_yaml::Value::Bool(b) => Some(*b),
            serde_yaml::Value::String(s) => match s.to_lowercase().as_str() {
                "true" | "yes" | "on" | "1" | "y" | "t" => Some(true),
                "false" | "no" | "off" | "0" | "" | "n" | "f" => Some(false),
                _ => None,
            },
            serde_yaml::Value::Number(n) => n.as_i64().map(|i| i != 0),
            _ => None,
        }
    }

    /// Convert a value to integer
    pub fn to_int(value: &serde_yaml::Value) -> Option<i64> {
        match value {
            serde_yaml::Value::Number(n) => n.as_i64(),
            serde_yaml::Value::String(s) => s.parse().ok(),
            serde_yaml::Value::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    /// Convert a value to float
    pub fn to_float(value: &serde_yaml::Value) -> Option<f64> {
        match value {
            serde_yaml::Value::Number(n) => n.as_f64(),
            serde_yaml::Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }

    /// Convert a value to list
    pub fn to_list(value: &serde_yaml::Value) -> Vec<serde_yaml::Value> {
        match value {
            serde_yaml::Value::Sequence(seq) => seq.clone(),
            serde_yaml::Value::String(s) => s
                .split(',')
                .map(|s| serde_yaml::Value::String(s.trim().to_string()))
                .collect(),
            _ => vec![value.clone()],
        }
    }
}

/// Legacy Variables type for backward compatibility
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Variables {
    data: IndexMap<String, serde_json::Value>,
}

impl Variables {
    /// Create new empty variables
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a variable
    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.data.insert(key.into(), value);
    }

    /// Get a variable
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.data.get(key)
    }

    /// Check if variable exists
    pub fn contains(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// Merge with another variables set (other takes precedence)
    pub fn merge(&mut self, other: &Variables) {
        for (k, v) in &other.data {
            self.data.insert(k.clone(), v.clone());
        }
    }

    /// Get all variables as a map
    pub fn as_map(&self) -> &IndexMap<String, serde_json::Value> {
        &self.data
    }

    /// Check if variables is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get the number of variables
    pub fn len(&self) -> usize {
        self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precedence_order() {
        assert!(VarPrecedence::ExtraVars > VarPrecedence::RoleDefaults);
        assert!(VarPrecedence::PlayVars > VarPrecedence::InventoryGroupVars);
    }

    #[test]
    fn test_var_store_basic() {
        let mut store = VarStore::new();

        store.set(
            "test",
            serde_yaml::Value::String("value".to_string()),
            VarPrecedence::PlayVars,
        );

        assert!(store.contains("test"));
        assert_eq!(
            store.get("test"),
            Some(&serde_yaml::Value::String("value".to_string()))
        );
    }

    #[test]
    fn test_var_store_precedence() {
        let mut store = VarStore::new();

        // Set at lower precedence
        store.set(
            "var",
            serde_yaml::Value::String("low".to_string()),
            VarPrecedence::RoleDefaults,
        );

        // Set at higher precedence
        store.set(
            "var",
            serde_yaml::Value::String("high".to_string()),
            VarPrecedence::ExtraVars,
        );

        // Higher precedence wins
        assert_eq!(
            store.get("var"),
            Some(&serde_yaml::Value::String("high".to_string()))
        );
    }

    #[test]
    fn test_deep_merge() {
        let base = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            a: 1
            b:
              c: 2
              d: 3
            "#,
        )
        .unwrap();

        let overlay = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            b:
              c: 4
              e: 5
            f: 6
            "#,
        )
        .unwrap();

        let merged = deep_merge(&base, &overlay);

        // Check values
        assert_eq!(
            resolve::resolve_path(&merged, "a"),
            Some(&serde_yaml::Value::Number(1.into()))
        );
        assert_eq!(
            resolve::resolve_path(&merged, "b.c"),
            Some(&serde_yaml::Value::Number(4.into())) // Overwritten
        );
        assert_eq!(
            resolve::resolve_path(&merged, "b.d"),
            Some(&serde_yaml::Value::Number(3.into())) // Preserved
        );
        assert_eq!(
            resolve::resolve_path(&merged, "b.e"),
            Some(&serde_yaml::Value::Number(5.into())) // Added
        );
    }

    #[test]
    fn test_deep_merge_in_place() {
        let mut base = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            a: 1
            b:
              c: 2
              d: 3
            "#,
        )
        .unwrap();

        let overlay = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            b:
              c: 4
              e: 5
            f: 6
            "#,
        )
        .unwrap();

        deep_merge_in_place(&mut base, &overlay);

        // Check values
        assert_eq!(
            resolve::resolve_path(&base, "a"),
            Some(&serde_yaml::Value::Number(1.into()))
        );
        assert_eq!(
            resolve::resolve_path(&base, "b.c"),
            Some(&serde_yaml::Value::Number(4.into())) // Overwritten
        );
        assert_eq!(
            resolve::resolve_path(&base, "b.d"),
            Some(&serde_yaml::Value::Number(3.into())) // Preserved
        );
        assert_eq!(
            resolve::resolve_path(&base, "b.e"),
            Some(&serde_yaml::Value::Number(5.into())) // Added
        );
    }

    #[test]
    fn test_resolve_path() {
        let value = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            a:
              b:
                c: "deep"
            list:
              - one
              - two
            "#,
        )
        .unwrap();

        assert_eq!(
            resolve::resolve_path(&value, "a.b.c"),
            Some(&serde_yaml::Value::String("deep".to_string()))
        );
        assert_eq!(
            resolve::resolve_path(&value, "list.0"),
            Some(&serde_yaml::Value::String("one".to_string()))
        );
    }

    #[test]
    fn test_var_scope() {
        let mut store = VarStore::new();
        store.set(
            "a",
            serde_yaml::Value::Number(1.into()),
            VarPrecedence::PlayVars,
        );

        let mut scope = store.scope();
        scope.set("b", serde_yaml::Value::Number(2.into()));

        assert_eq!(scope.get("a"), Some(&serde_yaml::Value::Number(1.into())));
        assert_eq!(scope.get("b"), Some(&serde_yaml::Value::Number(2.into())));

        // Parent doesn't see scope variables
        assert!(!store.contains("b"));
    }
}
