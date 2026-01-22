//! Common network device utilities and types
//!
//! This module provides shared functionality for network device modules including:
//! - Configuration diff generation
//! - Configuration backup management
//! - Transport abstraction (SSH, NETCONF)
//! - Device connection management

use crate::connection::{Connection, ConnectionError, ConnectionResult};
use crate::modules::{Diff, ModuleError, ModuleResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Transport Types
// ============================================================================

/// Supported transport protocols for network devices
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NetworkTransport {
    /// SSH-based CLI transport (default)
    #[default]
    Ssh,
    /// NETCONF over SSH (RFC 6241)
    Netconf,
    /// gRPC Network Management Interface
    Gnmi,
    /// REST API transport
    RestApi,
}

impl std::fmt::Display for NetworkTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkTransport::Ssh => write!(f, "ssh"),
            NetworkTransport::Netconf => write!(f, "netconf"),
            NetworkTransport::Gnmi => write!(f, "gnmi"),
            NetworkTransport::RestApi => write!(f, "restapi"),
        }
    }
}

impl std::str::FromStr for NetworkTransport {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ssh" | "cli" => Ok(NetworkTransport::Ssh),
            "netconf" | "nc" => Ok(NetworkTransport::Netconf),
            "gnmi" | "grpc" => Ok(NetworkTransport::Gnmi),
            "restapi" | "rest" | "api" => Ok(NetworkTransport::RestApi),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Unknown transport type: {}. Valid options: ssh, netconf, gnmi, restapi",
                s
            ))),
        }
    }
}

// ============================================================================
// Device Platform Types
// ============================================================================

/// Supported network device platforms
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPlatform {
    /// Cisco IOS/IOS-XE
    CiscoIos,
    /// Cisco IOS-XR
    CiscoIosXr,
    /// Cisco NX-OS
    CiscoNxos,
    /// Cisco ASA
    CiscoAsa,
    /// Arista EOS
    AristaEos,
    /// Juniper Junos
    JuniperJunos,
    /// Generic platform (best-effort detection)
    Generic,
}

impl std::fmt::Display for NetworkPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkPlatform::CiscoIos => write!(f, "cisco_ios"),
            NetworkPlatform::CiscoIosXr => write!(f, "cisco_iosxr"),
            NetworkPlatform::CiscoNxos => write!(f, "cisco_nxos"),
            NetworkPlatform::CiscoAsa => write!(f, "cisco_asa"),
            NetworkPlatform::AristaEos => write!(f, "arista_eos"),
            NetworkPlatform::JuniperJunos => write!(f, "juniper_junos"),
            NetworkPlatform::Generic => write!(f, "generic"),
        }
    }
}

impl std::str::FromStr for NetworkPlatform {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().replace('-', "_").as_str() {
            "cisco_ios" | "ios" | "ios_xe" | "iosxe" => Ok(NetworkPlatform::CiscoIos),
            "cisco_iosxr" | "iosxr" | "ios_xr" => Ok(NetworkPlatform::CiscoIosXr),
            "cisco_nxos" | "nxos" | "nexus" => Ok(NetworkPlatform::CiscoNxos),
            "cisco_asa" | "asa" => Ok(NetworkPlatform::CiscoAsa),
            "arista_eos" | "eos" | "arista" => Ok(NetworkPlatform::AristaEos),
            "juniper_junos" | "junos" | "juniper" => Ok(NetworkPlatform::JuniperJunos),
            "generic" | "auto" => Ok(NetworkPlatform::Generic),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Unknown platform: {}. Valid options: cisco_ios, cisco_iosxr, cisco_nxos, cisco_asa, arista_eos, juniper_junos, generic",
                s
            ))),
        }
    }
}

// ============================================================================
// Configuration Structures
// ============================================================================

/// Represents a network device configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// The raw configuration text
    pub content: String,
    /// Platform this configuration is for
    pub platform: NetworkPlatform,
    /// When this configuration was captured
    pub captured_at: DateTime<Utc>,
    /// Configuration source (running, startup, candidate)
    pub source: ConfigSource,
}

impl NetworkConfig {
    /// Create a new configuration from raw text
    pub fn new(content: String, platform: NetworkPlatform, source: ConfigSource) -> Self {
        Self {
            content,
            platform,
            captured_at: Utc::now(),
            source,
        }
    }

    /// Parse configuration into sections
    pub fn sections(&self) -> Vec<ConfigSection> {
        parse_config_sections(&self.content, self.platform)
    }

    /// Get a specific section by path (e.g., "interface GigabitEthernet0/0")
    pub fn get_section(&self, path: &str) -> Option<ConfigSection> {
        self.sections().into_iter().find(|s| s.path == path)
    }

    /// Check if a line or pattern exists in the configuration
    pub fn contains(&self, pattern: &str) -> bool {
        self.content.lines().any(|line| line.contains(pattern))
    }

    /// Count lines in the configuration
    pub fn line_count(&self) -> usize {
        self.content.lines().count()
    }
}

/// Configuration source type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigSource {
    /// Running configuration (in memory)
    Running,
    /// Startup configuration (persistent)
    Startup,
    /// Candidate configuration (pending commit)
    Candidate,
    /// Backup file
    Backup,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::Running => write!(f, "running"),
            ConfigSource::Startup => write!(f, "startup"),
            ConfigSource::Candidate => write!(f, "candidate"),
            ConfigSource::Backup => write!(f, "backup"),
        }
    }
}

/// A section of configuration (e.g., an interface block)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSection {
    /// The section path/header (e.g., "interface GigabitEthernet0/0")
    pub path: String,
    /// The section content (including header)
    pub content: String,
    /// Child sections if hierarchical
    pub children: Vec<ConfigSection>,
    /// Indentation level
    pub indent_level: usize,
    /// Starting line number in the original config
    pub start_line: usize,
    /// Ending line number in the original config
    pub end_line: usize,
}

/// Parse configuration into hierarchical sections
fn parse_config_sections(content: &str, platform: NetworkPlatform) -> Vec<ConfigSection> {
    let mut sections = Vec::new();
    let mut current_section: Option<ConfigSection> = None;
    let indent_char = match platform {
        NetworkPlatform::JuniperJunos => "    ", // Junos uses 4 spaces
        _ => " ",                                // IOS uses single space
    };

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('!') || trimmed.starts_with('#') {
            // Comment or empty line
            if let Some(ref mut section) = current_section {
                section.content.push('\n');
                section.content.push_str(line);
            }
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        let indent_level = if !indent_char.is_empty() {
            indent / indent_char.len()
        } else {
            0
        };

        // Detect section headers based on platform
        let is_section_header = match platform {
            NetworkPlatform::CiscoIos | NetworkPlatform::CiscoNxos | NetworkPlatform::CiscoAsa => {
                indent_level == 0
                    && (trimmed.starts_with("interface ")
                        || trimmed.starts_with("router ")
                        || trimmed.starts_with("ip ")
                        || trimmed.starts_with("line ")
                        || trimmed.starts_with("vlan ")
                        || trimmed.starts_with("class-map ")
                        || trimmed.starts_with("policy-map ")
                        || trimmed.starts_with("access-list ")
                        || trimmed.starts_with("route-map ")
                        || trimmed.starts_with("crypto ")
                        || trimmed.starts_with("aaa "))
            }
            NetworkPlatform::JuniperJunos => trimmed.ends_with('{'),
            NetworkPlatform::AristaEos => {
                indent_level == 0
                    && (trimmed.starts_with("interface ")
                        || trimmed.starts_with("router ")
                        || trimmed.starts_with("vlan ")
                        || trimmed.starts_with("ip "))
            }
            _ => indent_level == 0 && !trimmed.is_empty(),
        };

        if is_section_header {
            // Save previous section
            if let Some(section) = current_section.take() {
                sections.push(section);
            }
            // Start new section
            current_section = Some(ConfigSection {
                path: trimmed.to_string(),
                content: line.to_string(),
                children: Vec::new(),
                indent_level,
                start_line: line_num + 1,
                end_line: line_num + 1,
            });
        } else if let Some(ref mut section) = current_section {
            section.content.push('\n');
            section.content.push_str(line);
            section.end_line = line_num + 1;
        }
    }

    // Save final section
    if let Some(section) = current_section {
        sections.push(section);
    }

    sections
}

// ============================================================================
// Configuration Diff
// ============================================================================

/// Generate a unified diff between two configurations
pub fn generate_config_diff(before: &str, after: &str) -> Diff {
    let text_diff = TextDiff::from_lines(before, after);

    let mut details = String::new();
    let mut additions = 0;
    let mut deletions = 0;

    for change in text_diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => {
                deletions += 1;
                "-"
            }
            ChangeTag::Insert => {
                additions += 1;
                "+"
            }
            ChangeTag::Equal => " ",
        };
        details.push_str(&format!("{}{}", sign, change));
    }

    Diff {
        before: format!("{} lines", before.lines().count()),
        after: format!(
            "{} lines ({} additions, {} deletions)",
            after.lines().count(),
            additions,
            deletions
        ),
        details: Some(details),
    }
}

/// Generate a context-aware diff that understands configuration sections
pub fn generate_section_diff(before: &NetworkConfig, after: &NetworkConfig) -> Vec<SectionChange> {
    let before_sections_vec = before.sections();
    let before_sections: HashMap<String, &ConfigSection> = before_sections_vec
        .iter()
        .map(|s| (s.path.clone(), s))
        .collect();

    let after_sections = after.sections();
    let mut changes = Vec::new();

    for section in &after_sections {
        if let Some(old_section) = before_sections.get(&section.path) {
            if old_section.content != section.content {
                changes.push(SectionChange {
                    section_path: section.path.clone(),
                    change_type: SectionChangeType::Modified,
                    before: Some(old_section.content.clone()),
                    after: Some(section.content.clone()),
                });
            }
        } else {
            changes.push(SectionChange {
                section_path: section.path.clone(),
                change_type: SectionChangeType::Added,
                before: None,
                after: Some(section.content.clone()),
            });
        }
    }

    // Find removed sections
    let after_paths: std::collections::HashSet<_> =
        after_sections.iter().map(|s| &s.path).collect();

    for (path, section) in before_sections {
        if !after_paths.contains(&path) {
            changes.push(SectionChange {
                section_path: path,
                change_type: SectionChangeType::Removed,
                before: Some(section.content.clone()),
                after: None,
            });
        }
    }

    changes
}

/// Represents a change to a configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionChange {
    /// The section path that changed
    pub section_path: String,
    /// Type of change
    pub change_type: SectionChangeType,
    /// Configuration before the change
    pub before: Option<String>,
    /// Configuration after the change
    pub after: Option<String>,
}

/// Type of section change
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionChangeType {
    Added,
    Removed,
    Modified,
}

// ============================================================================
// Configuration Backup
// ============================================================================

/// Configuration backup metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigBackup {
    /// Unique backup identifier
    pub id: String,
    /// Device hostname
    pub hostname: String,
    /// Platform type
    pub platform: NetworkPlatform,
    /// When the backup was created
    pub created_at: DateTime<Utc>,
    /// Configuration source (running, startup)
    pub source: ConfigSource,
    /// Path to the backup file
    pub file_path: String,
    /// SHA256 checksum of the configuration
    pub checksum: String,
    /// Size in bytes
    pub size: u64,
    /// Optional description/reason for backup
    pub description: Option<String>,
}

/// Generate a backup filename based on device and timestamp
pub fn generate_backup_filename(
    hostname: &str,
    platform: NetworkPlatform,
    source: ConfigSource,
) -> String {
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    format!("{}_{}_{}_{}.cfg", hostname, platform, source, timestamp)
}

/// Calculate SHA256 checksum of configuration content
pub fn calculate_config_checksum(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ============================================================================
// Command Generation
// ============================================================================

/// Generate platform-specific commands to apply configuration
pub trait ConfigCommandGenerator {
    /// Generate commands to enter configuration mode
    fn enter_config_mode(&self) -> Vec<String>;

    /// Generate commands to exit configuration mode
    fn exit_config_mode(&self) -> Vec<String>;

    /// Generate commands to save configuration
    fn save_config(&self) -> Vec<String>;

    /// Generate commands to show running configuration
    fn show_running_config(&self) -> String;

    /// Generate commands to show startup configuration
    fn show_startup_config(&self) -> String;

    /// Generate commands to apply configuration lines
    fn apply_config_lines(&self, lines: &[String]) -> Vec<String>;

    /// Generate commands to remove configuration lines
    fn remove_config_lines(&self, lines: &[String]) -> Vec<String>;

    /// Wrap commands with error handling (e.g., terminal length 0)
    fn prepare_session(&self) -> Vec<String>;

    /// Generate rollback commands
    fn rollback_config(&self, checkpoint: &str) -> Vec<String>;
}

/// Cisco IOS command generator
pub struct IosCommandGenerator;

impl ConfigCommandGenerator for IosCommandGenerator {
    fn enter_config_mode(&self) -> Vec<String> {
        vec!["configure terminal".to_string()]
    }

    fn exit_config_mode(&self) -> Vec<String> {
        vec!["end".to_string()]
    }

    fn save_config(&self) -> Vec<String> {
        vec!["write memory".to_string()]
    }

    fn show_running_config(&self) -> String {
        "show running-config".to_string()
    }

    fn show_startup_config(&self) -> String {
        "show startup-config".to_string()
    }

    fn apply_config_lines(&self, lines: &[String]) -> Vec<String> {
        let mut commands = self.enter_config_mode();
        commands.extend(lines.iter().cloned());
        commands.extend(self.exit_config_mode());
        commands
    }

    fn remove_config_lines(&self, lines: &[String]) -> Vec<String> {
        let mut commands = self.enter_config_mode();
        for line in lines {
            // Prefix with 'no' to remove the configuration
            if !line.trim().is_empty() && !line.trim().starts_with("no ") {
                commands.push(format!("no {}", line.trim()));
            }
        }
        commands.extend(self.exit_config_mode());
        commands
    }

    fn prepare_session(&self) -> Vec<String> {
        vec![
            "terminal length 0".to_string(),
            "terminal width 512".to_string(),
        ]
    }

    fn rollback_config(&self, checkpoint: &str) -> Vec<String> {
        vec![format!("configure replace flash:{} force", checkpoint)]
    }
}

// ============================================================================
// Connection Wrapper for Network Devices
// ============================================================================

/// A wrapper around Connection that provides network device-specific operations
pub struct NetworkDeviceConnection {
    /// Underlying connection
    connection: Arc<dyn Connection + Send + Sync>,
    /// Device platform
    platform: NetworkPlatform,
    /// Transport type
    transport: NetworkTransport,
    /// Device hostname (for identification)
    hostname: String,
    /// Enable mode password (if required)
    enable_password: Option<String>,
}

impl NetworkDeviceConnection {
    /// Create a new network device connection wrapper
    pub fn new(
        connection: Arc<dyn Connection + Send + Sync>,
        platform: NetworkPlatform,
        transport: NetworkTransport,
        hostname: String,
    ) -> Self {
        Self {
            connection,
            platform,
            transport,
            hostname,
            enable_password: None,
        }
    }

    /// Set the enable password
    pub fn with_enable_password(mut self, password: String) -> Self {
        self.enable_password = Some(password);
        self
    }

    /// Get the device platform
    pub fn platform(&self) -> NetworkPlatform {
        self.platform
    }

    /// Get the transport type
    pub fn transport(&self) -> NetworkTransport {
        self.transport
    }

    /// Get the hostname/identifier
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    /// Execute a single command and return the output
    pub async fn execute_command(&self, command: &str) -> ConnectionResult<String> {
        let result = self.connection.execute(command, None).await?;
        if result.success {
            Ok(result.stdout)
        } else {
            Err(ConnectionError::ExecutionFailed(format!(
                "Command '{}' failed: {}",
                command, result.stderr
            )))
        }
    }

    /// Execute multiple commands in sequence
    pub async fn execute_commands(&self, commands: &[String]) -> ConnectionResult<Vec<String>> {
        let mut outputs = Vec::with_capacity(commands.len());
        for cmd in commands {
            let output = self.execute_command(cmd).await?;
            outputs.push(output);
        }
        Ok(outputs)
    }

    /// Get the running configuration
    pub async fn get_running_config(&self) -> ConnectionResult<NetworkConfig> {
        let cmd_gen = self.get_command_generator();

        // Prepare session (disable paging)
        let _ = self.execute_commands(&cmd_gen.prepare_session()).await;

        // Get configuration
        let content = self.execute_command(&cmd_gen.show_running_config()).await?;

        Ok(NetworkConfig::new(
            self.clean_config_output(&content),
            self.platform,
            ConfigSource::Running,
        ))
    }

    /// Get the startup configuration
    pub async fn get_startup_config(&self) -> ConnectionResult<NetworkConfig> {
        let cmd_gen = self.get_command_generator();

        // Prepare session
        let _ = self.execute_commands(&cmd_gen.prepare_session()).await;

        // Get configuration
        let content = self.execute_command(&cmd_gen.show_startup_config()).await?;

        Ok(NetworkConfig::new(
            self.clean_config_output(&content),
            self.platform,
            ConfigSource::Startup,
        ))
    }

    /// Apply configuration lines to the device
    pub async fn apply_config(&self, lines: &[String]) -> ConnectionResult<()> {
        let cmd_gen = self.get_command_generator();
        let commands = cmd_gen.apply_config_lines(lines);
        self.execute_commands(&commands).await?;
        Ok(())
    }

    /// Save configuration (write memory)
    pub async fn save_config(&self) -> ConnectionResult<()> {
        let cmd_gen = self.get_command_generator();
        self.execute_commands(&cmd_gen.save_config()).await?;
        Ok(())
    }

    /// Get the appropriate command generator for the platform
    fn get_command_generator(&self) -> Box<dyn ConfigCommandGenerator> {
        match self.platform {
            NetworkPlatform::CiscoIos => Box::new(IosCommandGenerator),
            // For now, use IOS generator as fallback for similar platforms
            NetworkPlatform::CiscoNxos => Box::new(IosCommandGenerator),
            NetworkPlatform::CiscoAsa => Box::new(IosCommandGenerator),
            NetworkPlatform::AristaEos => Box::new(IosCommandGenerator),
            _ => Box::new(IosCommandGenerator),
        }
    }

    /// Clean up configuration output (remove command echo, prompts, etc.)
    fn clean_config_output(&self, output: &str) -> String {
        let mut lines: Vec<&str> = output.lines().collect();

        // Remove first line (command echo) and last lines (prompts)
        if !lines.is_empty() && lines[0].contains("show ") {
            lines.remove(0);
        }

        // Remove trailing prompt lines
        while !lines.is_empty() {
            let last = lines.last().unwrap().trim();
            if last.ends_with('#') || last.ends_with('>') || last.is_empty() {
                lines.pop();
            } else {
                break;
            }
        }

        lines.join("\n")
    }
}

// ============================================================================
// Configuration Line Parser
// ============================================================================

/// Parse configuration lines from various input formats
pub fn parse_config_input(input: &str) -> Vec<String> {
    input
        .lines()
        .map(|line| line.to_string())
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !line.trim().starts_with('!'))
        .filter(|line| !line.trim().starts_with('#'))
        .collect()
}

/// Validate configuration lines for common syntax errors
pub fn validate_config_lines(lines: &[String], platform: NetworkPlatform) -> ModuleResult<()> {
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check for empty lines (already filtered, but double-check)
        if trimmed.is_empty() {
            continue;
        }

        // Check for obviously invalid characters
        if trimmed.contains('\0') {
            return Err(ModuleError::InvalidParameter(format!(
                "Line {} contains null character: {}",
                i + 1,
                line
            )));
        }

        // Platform-specific validation
        match platform {
            NetworkPlatform::CiscoIos | NetworkPlatform::CiscoNxos => {
                // IOS doesn't allow certain characters in configuration
                if trimmed.contains('\t') && !trimmed.starts_with(' ') {
                    // Tabs are generally okay but warn about mixing
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Generate the set of commands needed to transform one config into another
pub fn generate_config_diff_commands(before: &NetworkConfig, after: &NetworkConfig) -> Vec<String> {
    let before_lines: std::collections::HashSet<_> = before
        .content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('!'))
        .collect();

    let after_lines: std::collections::HashSet<_> = after
        .content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('!'))
        .collect();

    let mut commands = Vec::new();

    // Lines to remove (in before but not in after)
    for line in before_lines.difference(&after_lines) {
        if !line.is_empty() && !line.starts_with("!") {
            commands.push(format!("no {}", line));
        }
    }

    // Lines to add (in after but not in before)
    for line in after_lines.difference(&before_lines) {
        if !line.is_empty() && !line.starts_with("!") {
            commands.push(line.to_string());
        }
    }

    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_transport() {
        assert_eq!(
            "ssh".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::Ssh
        );
        assert_eq!(
            "netconf".parse::<NetworkTransport>().unwrap(),
            NetworkTransport::Netconf
        );
        assert!("invalid".parse::<NetworkTransport>().is_err());
    }

    #[test]
    fn test_parse_platform() {
        assert_eq!(
            "cisco_ios".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoIos
        );
        assert_eq!(
            "ios".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::CiscoIos
        );
        assert_eq!(
            "junos".parse::<NetworkPlatform>().unwrap(),
            NetworkPlatform::JuniperJunos
        );
    }

    #[test]
    fn test_config_diff() {
        let before = "interface GigabitEthernet0/0\n ip address 10.0.0.1 255.255.255.0\n!";
        let after = "interface GigabitEthernet0/0\n ip address 10.0.0.2 255.255.255.0\n!";

        let diff = generate_config_diff(before, after);
        assert!(diff.details.unwrap().contains("-"));
        assert!(diff.after.contains("1 additions"));
    }

    #[test]
    fn test_parse_config_input() {
        let input = "interface Gi0/0\n! comment\nip address 10.0.0.1 255.255.255.0\n\n";
        let lines = parse_config_input(input);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "interface Gi0/0");
        assert_eq!(lines[1], "ip address 10.0.0.1 255.255.255.0");
    }

    #[test]
    fn test_ios_command_generator() {
        let gen = IosCommandGenerator;
        assert_eq!(gen.enter_config_mode(), vec!["configure terminal"]);
        assert_eq!(gen.exit_config_mode(), vec!["end"]);
        assert_eq!(gen.save_config(), vec!["write memory"]);
    }

    #[test]
    fn test_backup_filename() {
        let filename =
            generate_backup_filename("router1", NetworkPlatform::CiscoIos, ConfigSource::Running);
        assert!(filename.starts_with("router1_cisco_ios_running_"));
        assert!(filename.ends_with(".cfg"));
    }

    #[test]
    fn test_config_checksum() {
        let content = "hostname router1\ninterface Gi0/0\n";
        let checksum = calculate_config_checksum(content);
        assert_eq!(checksum.len(), 64); // SHA256 hex length
    }
}
