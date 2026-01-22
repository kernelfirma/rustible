//! Security Analysis
//!
//! This module provides security-focused static analysis for playbooks,
//! detecting potential vulnerabilities, hardcoded secrets, and unsafe patterns.

use super::{
    helpers, AnalysisCategory, AnalysisConfig, AnalysisFinding, AnalysisResult, Severity,
    SourceLocation,
};
use crate::playbook::{Play, Playbook, Task};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Type of security vulnerability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VulnerabilityType {
    /// Hardcoded password or secret
    HardcodedSecret,
    /// Insecure shell command
    CommandInjection,
    /// Unencrypted sensitive data
    UnencryptedSecret,
    /// Use of sudo/become without consideration
    PrivilegeEscalation,
    /// Insecure HTTP instead of HTTPS
    InsecureProtocol,
    /// World-readable/writable file permissions
    InsecurePermissions,
    /// Use of deprecated/insecure algorithms
    WeakCrypto,
    /// Missing input validation
    InputValidation,
    /// Plaintext credentials in variables
    PlaintextCredentials,
    /// No verification of checksums/signatures
    MissingVerification,
}

impl std::fmt::Display for VulnerabilityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VulnerabilityType::HardcodedSecret => write!(f, "hardcoded-secret"),
            VulnerabilityType::CommandInjection => write!(f, "command-injection"),
            VulnerabilityType::UnencryptedSecret => write!(f, "unencrypted-secret"),
            VulnerabilityType::PrivilegeEscalation => write!(f, "privilege-escalation"),
            VulnerabilityType::InsecureProtocol => write!(f, "insecure-protocol"),
            VulnerabilityType::InsecurePermissions => write!(f, "insecure-permissions"),
            VulnerabilityType::WeakCrypto => write!(f, "weak-crypto"),
            VulnerabilityType::InputValidation => write!(f, "input-validation"),
            VulnerabilityType::PlaintextCredentials => write!(f, "plaintext-credentials"),
            VulnerabilityType::MissingVerification => write!(f, "missing-verification"),
        }
    }
}

/// A security rule definition
#[derive(Debug, Clone)]
pub struct SecurityRule {
    /// Rule identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Vulnerability type
    pub vulnerability_type: VulnerabilityType,
    /// Severity level
    pub severity: Severity,
    /// Description of the rule
    pub description: String,
    /// Pattern to match (if regex-based)
    pub pattern: Option<Regex>,
}

/// A security finding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    /// Rule that was violated
    pub rule_id: String,
    /// Vulnerability type
    pub vulnerability_type: VulnerabilityType,
    /// Severity
    pub severity: Severity,
    /// Message
    pub message: String,
    /// Location
    pub location: SourceLocation,
    /// Evidence (the problematic content)
    pub evidence: Option<String>,
}

impl SecurityFinding {
    /// Convert to a standard AnalysisFinding
    pub fn to_finding(&self) -> AnalysisFinding {
        let mut finding = AnalysisFinding::new(
            &self.rule_id,
            AnalysisCategory::Security,
            self.severity,
            &self.message,
        )
        .with_location(self.location.clone());

        if let Some(evidence) = &self.evidence {
            finding = finding.with_metadata("evidence", serde_json::json!(evidence));
        }

        finding
    }
}

/// Security analyzer
pub struct SecurityAnalyzer {
    /// Patterns for detecting secrets
    secret_patterns: Vec<Regex>,
    /// Patterns for detecting URLs
    url_pattern: Regex,
    /// Sensitive variable name patterns
    sensitive_var_patterns: Vec<Regex>,
}

impl SecurityAnalyzer {
    /// Create a new security analyzer
    pub fn new() -> Self {
        Self {
            secret_patterns: Self::default_secret_patterns(),
            url_pattern: Regex::new(r#"https?://[^\s"']+"#).unwrap(),
            sensitive_var_patterns: Self::default_sensitive_var_patterns(),
        }
    }

    /// Default patterns for detecting secrets
    fn default_secret_patterns() -> Vec<Regex> {
        vec![
            // AWS keys
            Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
            // AWS secret (usually 40 chars base64)
            Regex::new(r#"(?i)aws.{0,20}['"][0-9a-zA-Z/+]{40}['"]"#).unwrap(),
            // Private keys
            Regex::new(r"-----BEGIN (?:RSA |DSA |EC |OPENSSH )?PRIVATE KEY-----").unwrap(),
            // Generic API key patterns
            Regex::new(r#"(?i)api[_-]?key\s*[:=]\s*['"][a-zA-Z0-9]{16,}['"]"#).unwrap(),
            // Bearer tokens
            Regex::new(r"(?i)bearer\s+[a-zA-Z0-9\-_=]{20,}").unwrap(),
            // Password in common formats
            Regex::new(r#"(?i)password\s*[:=]\s*['"][^'"]{8,}['"]"#).unwrap(),
            // GitHub tokens
            Regex::new(r"ghp_[a-zA-Z0-9]{36}").unwrap(),
            Regex::new(r"gho_[a-zA-Z0-9]{36}").unwrap(),
            // Generic secret patterns
            Regex::new(r#"(?i)secret\s*[:=]\s*['"][^'"]{8,}['"]"#).unwrap(),
        ]
    }

    /// Default patterns for sensitive variable names
    fn default_sensitive_var_patterns() -> Vec<Regex> {
        vec![
            Regex::new(r"(?i)^password$").unwrap(),
            Regex::new(r"(?i)^passwd$").unwrap(),
            Regex::new(r"(?i)^secret$").unwrap(),
            Regex::new(r"(?i)^api[_-]?key$").unwrap(),
            Regex::new(r"(?i)^token$").unwrap(),
            Regex::new(r"(?i)^private[_-]?key$").unwrap(),
            Regex::new(r"(?i)^access[_-]?key$").unwrap(),
            Regex::new(r"(?i)^auth[_-]?token$").unwrap(),
            Regex::new(r"(?i).*_password$").unwrap(),
            Regex::new(r"(?i).*_secret$").unwrap(),
            Regex::new(r"(?i).*_token$").unwrap(),
            Regex::new(r"(?i).*_key$").unwrap(),
        ]
    }

    /// Analyze a playbook for security issues
    pub fn analyze(
        &self,
        playbook: &Playbook,
        config: &AnalysisConfig,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();
        let source_file = playbook
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            // Check play-level variables for secrets
            findings.extend(self.check_play_variables(play, play_idx, &source_file)?);

            // Check all tasks
            let all_tasks = helpers::get_all_tasks(play);
            for (task_idx, task) in all_tasks.iter().enumerate() {
                findings.extend(self.check_task(task, task_idx, play_idx, &play.name, &source_file)?);
            }
        }

        // Filter by enabled/disabled rules
        if !config.enabled_security_rules.is_empty() {
            findings.retain(|f| config.enabled_security_rules.contains(&f.rule_id));
        }
        if !config.disabled_security_rules.is_empty() {
            findings.retain(|f| !config.disabled_security_rules.contains(&f.rule_id));
        }

        Ok(findings)
    }

    /// Check play-level variables for secrets
    fn check_play_variables(
        &self,
        play: &Play,
        play_idx: usize,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        for (var_name, var_value) in play.vars.as_map() {
            let location = SourceLocation::new().with_play(play_idx, &play.name);
            let location = if let Some(f) = source_file {
                location.with_file(f.clone())
            } else {
                location
            };

            // Check if variable name suggests it's sensitive
            if self.is_sensitive_variable_name(var_name) {
                // Check if the value looks like a hardcoded secret
                if let Some(value_str) = var_value.as_str() {
                    if self.looks_like_secret(value_str) {
                        findings.push(
                            AnalysisFinding::new(
                                "SEC001",
                                AnalysisCategory::Security,
                                Severity::Critical,
                                format!(
                                    "Variable '{}' appears to contain a hardcoded secret",
                                    var_name
                                ),
                            )
                            .with_location(location.clone())
                            .with_description(
                                "Hardcoding secrets in playbooks is a security risk. \
                                 Use Ansible Vault or external secret management."
                            )
                            .with_suggestion(
                                "Use ansible-vault to encrypt sensitive values or reference from a secure source."
                            ),
                        );
                    }
                }
            }

            // Check value for known secret patterns
            let value_str = serde_json::to_string(var_value).unwrap_or_default();
            for pattern in &self.secret_patterns {
                if pattern.is_match(&value_str) {
                    findings.push(
                        AnalysisFinding::new(
                            "SEC002",
                            AnalysisCategory::Security,
                            Severity::Critical,
                            format!("Potential secret detected in variable '{}'", var_name),
                        )
                        .with_location(location.clone())
                        .with_description(
                            "A pattern matching known secret formats was detected."
                        )
                        .with_suggestion(
                            "Remove the secret and use ansible-vault or a secret management tool."
                        ),
                    );
                    break;
                }
            }
        }

        Ok(findings)
    }

    /// Check a task for security issues
    fn check_task(
        &self,
        task: &Task,
        task_idx: usize,
        play_idx: usize,
        play_name: &str,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        let location = SourceLocation::new()
            .with_play(play_idx, play_name)
            .with_task(task_idx, &task.name);
        let location = if let Some(f) = source_file {
            location.with_file(f.clone())
        } else {
            location
        };

        // Check for command injection risks
        if task.module.name == "shell" || task.module.name == "ansible.builtin.shell" {
            findings.extend(self.check_shell_injection(task, &location)?);
        }

        // Check for insecure HTTP URLs
        let args_str = serde_json::to_string(&task.module.args).unwrap_or_default();
        for url_match in self.url_pattern.find_iter(&args_str) {
            let url = url_match.as_str();
            if url.starts_with("http://") && !url.starts_with("http://localhost") && !url.starts_with("http://127.0.0.1") {
                findings.push(
                    AnalysisFinding::new(
                        "SEC003",
                        AnalysisCategory::Security,
                        Severity::Warning,
                        "Insecure HTTP URL detected",
                    )
                    .with_location(location.clone())
                    .with_description(format!("URL '{}' uses unencrypted HTTP.", url))
                    .with_suggestion("Use HTTPS instead of HTTP for secure communication."),
                );
            }
        }

        // Check for insecure file permissions
        if task.module.name == "file" || task.module.name == "ansible.builtin.file"
            || task.module.name == "copy" || task.module.name == "ansible.builtin.copy"
            || task.module.name == "template" || task.module.name == "ansible.builtin.template"
        {
            findings.extend(self.check_file_permissions(task, &location)?);
        }

        // Check for no_log on sensitive tasks
        if self.is_sensitive_task(task) && !self.task_has_no_log(task) {
            findings.push(
                AnalysisFinding::new(
                    "SEC004",
                    AnalysisCategory::Security,
                    Severity::Warning,
                    "Sensitive task without no_log",
                )
                .with_location(location.clone())
                .with_description(
                    "This task appears to handle sensitive data but no_log is not set."
                )
                .with_suggestion("Add 'no_log: true' to prevent sensitive data from being logged."),
            );
        }

        // Check task variables
        for (var_name, var_value) in task.vars.as_map() {
            if self.is_sensitive_variable_name(var_name) {
                if let Some(value_str) = var_value.as_str() {
                    if self.looks_like_secret(value_str) {
                        findings.push(
                            AnalysisFinding::new(
                                "SEC005",
                                AnalysisCategory::Security,
                                Severity::Error,
                                format!(
                                    "Task variable '{}' appears to contain a hardcoded secret",
                                    var_name
                                ),
                            )
                            .with_location(location.clone())
                            .with_description(
                                "Hardcoding secrets in task variables is a security risk."
                            )
                            .with_suggestion(
                                "Use ansible-vault or reference from a secure source."
                            ),
                        );
                    }
                }
            }
        }

        Ok(findings)
    }

    /// Check shell commands for injection risks
    fn check_shell_injection(
        &self,
        task: &Task,
        location: &SourceLocation,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        let cmd = match &task.module.args {
            serde_json::Value::String(s) => Some(s.as_str()),
            serde_json::Value::Object(obj) => {
                obj.get("cmd").and_then(|v| v.as_str())
            }
            _ => None,
        };

        if let Some(cmd) = cmd {
            // Check for unquoted variable expansion
            let var_pattern = Regex::new(r"\$\{\{|\{\{[^}]*\}\}").unwrap();
            if var_pattern.is_match(cmd) {
                // Check if the variable is properly quoted
                let quoted_var = Regex::new(r#"["'][^"']*\{\{[^}]*\}\}[^"']*["']"#).unwrap();
                if !quoted_var.is_match(cmd) {
                    findings.push(
                        AnalysisFinding::new(
                            "SEC006",
                            AnalysisCategory::Security,
                            Severity::Warning,
                            "Potential command injection risk",
                        )
                        .with_location(location.clone())
                        .with_description(
                            "Variable interpolation in shell commands without proper quoting \
                             can lead to command injection."
                        )
                        .with_suggestion(
                            "Quote variable expansions: \"{{ variable }}\" or use the 'command' module."
                        ),
                    );
                }
            }
        }

        Ok(findings)
    }

    /// Check file permissions for security issues
    fn check_file_permissions(
        &self,
        task: &Task,
        location: &SourceLocation,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        if let Some(obj) = task.module.args.as_object() {
            if let Some(mode) = obj.get("mode") {
                let mode_str = mode.as_str().or_else(|| {
                    mode.as_i64().map(|_| "") // We'll handle numeric modes differently
                });

                if let Some(mode_str) = mode_str {
                    // Check for world-writable permissions
                    if mode_str.ends_with("7") || mode_str.ends_with("6") || mode_str.ends_with("2") {
                        let last_char = mode_str.chars().last().unwrap_or('0');
                        if last_char == '7' || last_char == '6' || last_char == '2' {
                            findings.push(
                                AnalysisFinding::new(
                                    "SEC007",
                                    AnalysisCategory::Security,
                                    Severity::Warning,
                                    "World-writable file permissions",
                                )
                                .with_location(location.clone())
                                .with_description(format!(
                                    "Mode '{}' allows world-write access, which may be insecure.",
                                    mode_str
                                ))
                                .with_suggestion("Use more restrictive permissions (e.g., 0644 or 0600)."),
                            );
                        }
                    }
                }
            }
        }

        Ok(findings)
    }

    /// Check if a variable name suggests sensitivity
    fn is_sensitive_variable_name(&self, name: &str) -> bool {
        self.sensitive_var_patterns.iter().any(|p| p.is_match(name))
    }

    /// Check if a value looks like a secret
    fn looks_like_secret(&self, value: &str) -> bool {
        // Skip if it looks like a variable reference
        if value.contains("{{") || value.contains("lookup(") {
            return false;
        }

        // Check against secret patterns
        for pattern in &self.secret_patterns {
            if pattern.is_match(value) {
                return true;
            }
        }

        // Skip if it's a common placeholder
        let placeholders = ["changeme", "CHANGEME", "TODO", "FIXME", "placeholder", "example"];
        for placeholder in &placeholders {
            if value.to_lowercase().contains(&placeholder.to_lowercase()) {
                return false;
            }
        }

        // Check if it's a long random-looking string
        if value.len() >= 16 {
            let has_mixed_case = value.chars().any(|c| c.is_lowercase())
                && value.chars().any(|c| c.is_uppercase());
            let has_numbers = value.chars().any(|c| c.is_numeric());
            let has_special = value.chars().any(|c| !c.is_alphanumeric());

            // High entropy heuristic
            if (has_numbers || has_special) && has_mixed_case {
                return true;
            }
        }

        false
    }

    /// Check if a task handles sensitive data
    fn is_sensitive_task(&self, task: &Task) -> bool {
        // Modules that typically handle sensitive data
        let sensitive_modules = [
            "user",
            "ansible.builtin.user",
            "mysql_user",
            "community.mysql.mysql_user",
            "postgresql_user",
            "community.postgresql.postgresql_user",
            "uri",
            "ansible.builtin.uri",
            "vault_write",
            "community.hashi_vault.vault_write",
        ];

        if sensitive_modules.contains(&task.module.name.as_str()) {
            return true;
        }

        // Check for sensitive parameters
        if let Some(obj) = task.module.args.as_object() {
            let sensitive_params = ["password", "passwd", "secret", "token", "api_key"];
            for param in &sensitive_params {
                if obj.contains_key(*param) {
                    return true;
                }
            }
        }

        false
    }

    fn task_has_no_log(&self, task: &Task) -> bool {
        task.module
            .args
            .as_object()
            .and_then(|obj| obj.get("no_log"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }
}

impl Default for SecurityAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_sensitive_variable_name() {
        let analyzer = SecurityAnalyzer::new();
        assert!(analyzer.is_sensitive_variable_name("password"));
        assert!(analyzer.is_sensitive_variable_name("api_key"));
        assert!(analyzer.is_sensitive_variable_name("db_password"));
        assert!(!analyzer.is_sensitive_variable_name("username"));
        assert!(!analyzer.is_sensitive_variable_name("host"));
    }

    #[test]
    fn test_looks_like_secret() {
        let analyzer = SecurityAnalyzer::new();

        // Should detect AWS-style keys
        assert!(analyzer.looks_like_secret("AKIAIOSFODNN7EXAMPLE"));

        // Should skip variable references
        assert!(!analyzer.looks_like_secret("{{ vault_password }}"));

        // Should skip placeholders
        assert!(!analyzer.looks_like_secret("changeme"));
        assert!(!analyzer.looks_like_secret("CHANGEME"));
    }

    #[test]
    fn test_vulnerability_type_display() {
        assert_eq!(
            format!("{}", VulnerabilityType::HardcodedSecret),
            "hardcoded-secret"
        );
        assert_eq!(
            format!("{}", VulnerabilityType::CommandInjection),
            "command-injection"
        );
    }
}
