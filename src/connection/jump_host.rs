//! Jump host (bastion) support for SSH connections.
//!
//! This module provides ProxyJump functionality for connecting to hosts
//! through one or more intermediate bastion/jump hosts. It supports:
//!
//! - Single jump host connections
//! - Multi-hop chains (A -> B -> C -> Target)
//! - SSH config file ProxyJump directive parsing
//! - Recursive connection resolution
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::connection::jump_host::{JumpHostConfig, JumpHostChain};
//!
//! // Single jump host
//! let jump = JumpHostConfig::new("bastion.example.com")
//!     .user("admin")
//!     .port(22);
//!
//! // Multi-hop chain
//! let chain = JumpHostChain::new()
//!     .add_jump(JumpHostConfig::new("jump1.example.com"))
//!     .add_jump(JumpHostConfig::new("jump2.example.com"));
//! # Ok(())
//! # }
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

use super::config::{ConnectionConfig, HostConfig};
use super::{ConnectionError, ConnectionResult};

/// Maximum number of jumps allowed in a chain (prevents infinite loops)
pub const MAX_JUMP_DEPTH: usize = 10;

/// Configuration for a single jump/bastion host.
///
/// This represents a single hop in a ProxyJump chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JumpHostConfig {
    /// Hostname or IP address of the jump host
    pub host: String,
    /// SSH port (default: 22)
    pub port: u16,
    /// Username for authentication (if different from target)
    pub user: Option<String>,
    /// Path to private key file (if different from default)
    pub identity_file: Option<String>,
}

impl JumpHostConfig {
    /// Create a new jump host configuration.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: 22,
            user: None,
            identity_file: None,
        }
    }

    /// Set the SSH port.
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the username.
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Set the identity file path.
    pub fn identity_file(mut self, path: impl Into<String>) -> Self {
        self.identity_file = Some(path.into());
        self
    }

    /// Parse a jump host specification string.
    ///
    /// Supports formats:
    /// - `host`
    /// - `host:port`
    /// - `user@host`
    /// - `user@host:port`
    pub fn parse(spec: &str) -> ConnectionResult<Self> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err(ConnectionError::InvalidConfig(
                "Empty jump host specification".to_string(),
            ));
        }

        let (user, host_port) = if let Some(at_pos) = spec.find('@') {
            let user = &spec[..at_pos];
            let rest = &spec[at_pos + 1..];
            (Some(user.to_string()), rest)
        } else {
            (None, spec)
        };

        let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
            // Check if this is IPv6 (contains multiple colons or is bracketed)
            if host_port.contains('[') {
                // IPv6: [::1]:port or [::1]
                if let Some(bracket_end) = host_port.find(']') {
                    let host = &host_port[1..bracket_end];
                    let port = if bracket_end + 1 < host_port.len()
                        && host_port.chars().nth(bracket_end + 1) == Some(':')
                    {
                        host_port[bracket_end + 2..].parse().map_err(|_| {
                            ConnectionError::InvalidConfig(format!(
                                "Invalid port in jump host: {}",
                                spec
                            ))
                        })?
                    } else {
                        22
                    };
                    (host.to_string(), port)
                } else {
                    return Err(ConnectionError::InvalidConfig(format!(
                        "Invalid IPv6 address in jump host: {}",
                        spec
                    )));
                }
            } else if host_port.matches(':').count() > 1 {
                // Unbracketed IPv6 without port
                (host_port.to_string(), 22)
            } else {
                // Regular host:port
                let host = &host_port[..colon_pos];
                let port = host_port[colon_pos + 1..].parse().map_err(|_| {
                    ConnectionError::InvalidConfig(format!("Invalid port in jump host: {}", spec))
                })?;
                (host.to_string(), port)
            }
        } else {
            (host_port.to_string(), 22)
        };

        Ok(Self {
            host,
            port,
            user,
            identity_file: None,
        })
    }

    /// Convert to a HostConfig for connection purposes.
    pub fn to_host_config(&self) -> HostConfig {
        HostConfig {
            hostname: Some(self.host.clone()),
            port: Some(self.port),
            user: self.user.clone(),
            identity_file: self.identity_file.clone(),
            ..Default::default()
        }
    }

    /// Get the effective user, falling back to a default.
    pub fn effective_user(&self, default_user: &str) -> String {
        self.user
            .clone()
            .unwrap_or_else(|| default_user.to_string())
    }
}

impl fmt::Display for JumpHostConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(user) = &self.user {
            write!(f, "{}@", user)?;
        }
        write!(f, "{}", self.host)?;
        if self.port != 22 {
            write!(f, ":{}", self.port)?;
        }
        Ok(())
    }
}

/// A chain of jump hosts for multi-hop connections.
///
/// Represents a path like: local -> jump1 -> jump2 -> ... -> target
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JumpHostChain {
    /// The ordered list of jump hosts (first is closest to local)
    jumps: Vec<JumpHostConfig>,
}

impl JumpHostChain {
    /// Create a new empty jump host chain.
    pub fn new() -> Self {
        Self { jumps: Vec::new() }
    }

    /// Create a chain from a single jump host.
    pub fn single(jump: JumpHostConfig) -> Self {
        Self { jumps: vec![jump] }
    }

    /// Add a jump host to the chain.
    pub fn add_jump(mut self, jump: JumpHostConfig) -> Self {
        self.jumps.push(jump);
        self
    }

    /// Push a jump host to the chain.
    pub fn push(&mut self, jump: JumpHostConfig) {
        self.jumps.push(jump);
    }

    /// Get the number of hops in the chain.
    pub fn len(&self) -> usize {
        self.jumps.len()
    }

    /// Check if the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.jumps.is_empty()
    }

    /// Get an iterator over the jump hosts.
    pub fn iter(&self) -> impl Iterator<Item = &JumpHostConfig> {
        self.jumps.iter()
    }

    /// Get the jump hosts as a slice.
    pub fn as_slice(&self) -> &[JumpHostConfig] {
        &self.jumps
    }

    /// Parse a ProxyJump specification string.
    ///
    /// Supports comma-separated list of jump hosts:
    /// - `jump1,jump2,jump3` -> chain of 3 hops
    /// - `user@jump1:2222,jump2` -> first hop with user and port
    pub fn parse(spec: &str) -> ConnectionResult<Self> {
        let spec = spec.trim();
        if spec.is_empty() || spec.eq_ignore_ascii_case("none") {
            return Ok(Self::new());
        }

        let mut chain = Self::new();
        for part in spec.split(',') {
            let jump = JumpHostConfig::parse(part.trim())?;
            chain.push(jump);
        }

        if chain.len() > MAX_JUMP_DEPTH {
            return Err(ConnectionError::InvalidConfig(format!(
                "Too many jump hosts ({} > {})",
                chain.len(),
                MAX_JUMP_DEPTH
            )));
        }

        Ok(chain)
    }

    /// Validate the chain (check for loops, depth limits).
    pub fn validate(&self) -> ConnectionResult<()> {
        if self.len() > MAX_JUMP_DEPTH {
            return Err(ConnectionError::InvalidConfig(format!(
                "Jump chain too deep ({} > {})",
                self.len(),
                MAX_JUMP_DEPTH
            )));
        }

        // Check for loops (same host appearing twice)
        let mut seen = std::collections::HashSet::new();
        for jump in &self.jumps {
            let key = format!("{}:{}", jump.host, jump.port);
            if !seen.insert(key.clone()) {
                return Err(ConnectionError::InvalidConfig(format!(
                    "Loop detected in jump chain: {} appears multiple times",
                    jump.host
                )));
            }
        }

        Ok(())
    }
}

impl fmt::Display for JumpHostChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.jumps.iter().map(|j| j.to_string()).collect();
        write!(f, "{}", parts.join(","))
    }
}

impl IntoIterator for JumpHostChain {
    type Item = JumpHostConfig;
    type IntoIter = std::vec::IntoIter<JumpHostConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.jumps.into_iter()
    }
}

impl<'a> IntoIterator for &'a JumpHostChain {
    type Item = &'a JumpHostConfig;
    type IntoIter = std::slice::Iter<'a, JumpHostConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.jumps.iter()
    }
}

/// Resolver for jump host configurations.
///
/// Handles recursive resolution of proxy_jump directives from SSH config.
pub struct JumpHostResolver<'a> {
    config: &'a ConnectionConfig,
    visited: std::collections::HashSet<String>,
}

impl<'a> JumpHostResolver<'a> {
    /// Create a new resolver with the given configuration.
    pub fn new(config: &'a ConnectionConfig) -> Self {
        Self {
            config,
            visited: std::collections::HashSet::new(),
        }
    }

    /// Resolve the full jump host chain for a target host.
    ///
    /// This recursively expands proxy_jump directives from the SSH config.
    pub fn resolve(&mut self, target_host: &str) -> ConnectionResult<JumpHostChain> {
        self.visited.clear();
        self.resolve_recursive(target_host)
    }

    fn resolve_recursive(&mut self, host: &str) -> ConnectionResult<JumpHostChain> {
        // Check for loops
        if self.visited.contains(host) {
            return Err(ConnectionError::InvalidConfig(format!(
                "Circular reference in ProxyJump chain: {}",
                host
            )));
        }
        self.visited.insert(host.to_string());

        // Check depth limit
        if self.visited.len() > MAX_JUMP_DEPTH {
            return Err(ConnectionError::InvalidConfig(format!(
                "ProxyJump chain too deep (>{} hops)",
                MAX_JUMP_DEPTH
            )));
        }

        // Get host config
        let host_config = match self.config.get_host(host) {
            Some(hc) => hc,
            None => return Ok(JumpHostChain::new()), // No config = no jumps
        };

        // Check for ProxyJump directive
        let proxy_jump = match &host_config.proxy_jump {
            Some(pj) if !pj.is_empty() && !pj.eq_ignore_ascii_case("none") => pj.clone(),
            _ => return Ok(JumpHostChain::new()), // No proxy = no jumps
        };

        // Parse the ProxyJump specification
        let chain = JumpHostChain::parse(&proxy_jump)?;

        // Recursively resolve each jump host (they might have their own ProxyJump)
        let mut full_chain = JumpHostChain::new();
        for jump in chain.iter() {
            // First, resolve any jumps needed to reach this jump host
            let sub_chain = self.resolve_recursive(&jump.host)?;
            for sub_jump in sub_chain {
                full_chain.push(sub_jump);
            }
            // Then add this jump host itself
            full_chain.push(jump.clone());
        }

        full_chain.validate()?;
        Ok(full_chain)
    }

    /// Resolve jump chain from a HostConfig directly.
    pub fn resolve_from_config(
        &mut self,
        host_config: &HostConfig,
    ) -> ConnectionResult<JumpHostChain> {
        match &host_config.proxy_jump {
            Some(pj) if !pj.is_empty() && !pj.eq_ignore_ascii_case("none") => {
                let chain = JumpHostChain::parse(pj)?;

                // Recursively resolve each jump host
                let mut full_chain = JumpHostChain::new();
                for jump in chain.iter() {
                    let sub_chain = self.resolve_recursive(&jump.host)?;
                    for sub_jump in sub_chain {
                        full_chain.push(sub_jump);
                    }
                    full_chain.push(jump.clone());
                }

                full_chain.validate()?;
                Ok(full_chain)
            }
            _ => Ok(JumpHostChain::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jump_host_config_new() {
        let jump = JumpHostConfig::new("bastion.example.com");
        assert_eq!(jump.host, "bastion.example.com");
        assert_eq!(jump.port, 22);
        assert!(jump.user.is_none());
    }

    #[test]
    fn test_jump_host_config_builder() {
        let jump = JumpHostConfig::new("bastion.example.com")
            .port(2222)
            .user("admin")
            .identity_file("~/.ssh/bastion_key");

        assert_eq!(jump.host, "bastion.example.com");
        assert_eq!(jump.port, 2222);
        assert_eq!(jump.user, Some("admin".to_string()));
        assert_eq!(jump.identity_file, Some("~/.ssh/bastion_key".to_string()));
    }

    #[test]
    fn test_jump_host_parse_simple() {
        let jump = JumpHostConfig::parse("bastion.example.com").unwrap();
        assert_eq!(jump.host, "bastion.example.com");
        assert_eq!(jump.port, 22);
        assert!(jump.user.is_none());
    }

    #[test]
    fn test_jump_host_parse_with_port() {
        let jump = JumpHostConfig::parse("bastion.example.com:2222").unwrap();
        assert_eq!(jump.host, "bastion.example.com");
        assert_eq!(jump.port, 2222);
    }

    #[test]
    fn test_jump_host_parse_with_user() {
        let jump = JumpHostConfig::parse("admin@bastion.example.com").unwrap();
        assert_eq!(jump.host, "bastion.example.com");
        assert_eq!(jump.user, Some("admin".to_string()));
        assert_eq!(jump.port, 22);
    }

    #[test]
    fn test_jump_host_parse_full() {
        let jump = JumpHostConfig::parse("admin@bastion.example.com:2222").unwrap();
        assert_eq!(jump.host, "bastion.example.com");
        assert_eq!(jump.user, Some("admin".to_string()));
        assert_eq!(jump.port, 2222);
    }

    #[test]
    fn test_jump_host_parse_ipv6() {
        let jump = JumpHostConfig::parse("[::1]:2222").unwrap();
        assert_eq!(jump.host, "::1");
        assert_eq!(jump.port, 2222);
    }

    #[test]
    fn test_jump_host_parse_ipv6_no_port() {
        let jump = JumpHostConfig::parse("[::1]").unwrap();
        assert_eq!(jump.host, "::1");
        assert_eq!(jump.port, 22);
    }

    #[test]
    fn test_jump_host_display() {
        let jump = JumpHostConfig::new("bastion.example.com")
            .port(2222)
            .user("admin");
        assert_eq!(jump.to_string(), "admin@bastion.example.com:2222");

        let simple = JumpHostConfig::new("bastion.example.com");
        assert_eq!(simple.to_string(), "bastion.example.com");
    }

    #[test]
    fn test_jump_chain_empty() {
        let chain = JumpHostChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn test_jump_chain_single() {
        let chain = JumpHostChain::single(JumpHostConfig::new("bastion"));
        assert_eq!(chain.len(), 1);
        assert!(!chain.is_empty());
    }

    #[test]
    fn test_jump_chain_multi() {
        let chain = JumpHostChain::new()
            .add_jump(JumpHostConfig::new("jump1"))
            .add_jump(JumpHostConfig::new("jump2"))
            .add_jump(JumpHostConfig::new("jump3"));

        assert_eq!(chain.len(), 3);
        let hosts: Vec<_> = chain.iter().map(|j| j.host.as_str()).collect();
        assert_eq!(hosts, vec!["jump1", "jump2", "jump3"]);
    }

    #[test]
    fn test_jump_chain_parse() {
        let chain = JumpHostChain::parse("jump1,user@jump2:2222,jump3").unwrap();
        assert_eq!(chain.len(), 3);

        let jumps: Vec<_> = chain.iter().collect();
        assert_eq!(jumps[0].host, "jump1");
        assert_eq!(jumps[1].host, "jump2");
        assert_eq!(jumps[1].user, Some("user".to_string()));
        assert_eq!(jumps[1].port, 2222);
        assert_eq!(jumps[2].host, "jump3");
    }

    #[test]
    fn test_jump_chain_parse_none() {
        let chain = JumpHostChain::parse("none").unwrap();
        assert!(chain.is_empty());

        let chain2 = JumpHostChain::parse("").unwrap();
        assert!(chain2.is_empty());
    }

    #[test]
    fn test_jump_chain_validate_loop() {
        let mut chain = JumpHostChain::new();
        chain.push(JumpHostConfig::new("jump1"));
        chain.push(JumpHostConfig::new("jump2"));
        chain.push(JumpHostConfig::new("jump1")); // Loop!

        assert!(chain.validate().is_err());
    }

    #[test]
    fn test_jump_chain_display() {
        let chain = JumpHostChain::new()
            .add_jump(JumpHostConfig::new("jump1"))
            .add_jump(JumpHostConfig::new("jump2").user("admin").port(2222));

        assert_eq!(chain.to_string(), "jump1,admin@jump2:2222");
    }

    #[test]
    fn test_resolver_no_config() {
        let config = ConnectionConfig::default();
        let mut resolver = JumpHostResolver::new(&config);
        let chain = resolver.resolve("unknown-host").unwrap();
        assert!(chain.is_empty());
    }

    #[test]
    fn test_resolver_simple() {
        let mut config = ConnectionConfig::default();
        config.hosts.insert(
            "target".to_string(),
            HostConfig {
                proxy_jump: Some("bastion".to_string()),
                ..Default::default()
            },
        );

        let mut resolver = JumpHostResolver::new(&config);
        let chain = resolver.resolve("target").unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.as_slice()[0].host, "bastion");
    }

    #[test]
    fn test_resolver_recursive() {
        let mut config = ConnectionConfig::default();
        config.hosts.insert(
            "target".to_string(),
            HostConfig {
                proxy_jump: Some("jump2".to_string()),
                ..Default::default()
            },
        );
        config.hosts.insert(
            "jump2".to_string(),
            HostConfig {
                proxy_jump: Some("jump1".to_string()),
                ..Default::default()
            },
        );

        let mut resolver = JumpHostResolver::new(&config);
        let chain = resolver.resolve("target").unwrap();

        assert_eq!(chain.len(), 2);
        let hosts: Vec<_> = chain.iter().map(|j| j.host.as_str()).collect();
        assert_eq!(hosts, vec!["jump1", "jump2"]);
    }

    #[test]
    fn test_resolver_circular() {
        let mut config = ConnectionConfig::default();
        config.hosts.insert(
            "host1".to_string(),
            HostConfig {
                proxy_jump: Some("host2".to_string()),
                ..Default::default()
            },
        );
        config.hosts.insert(
            "host2".to_string(),
            HostConfig {
                proxy_jump: Some("host1".to_string()),
                ..Default::default()
            },
        );

        let mut resolver = JumpHostResolver::new(&config);
        let result = resolver.resolve("host1");
        assert!(result.is_err());
    }
}
