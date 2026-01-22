//! STIG (Security Technical Implementation Guide) Scanner
//!
//! Implements security checks based on DISA STIG guidelines for Linux systems.
//! STIGs are configuration standards for DoD-owned systems.

use super::checks::*;
use super::{
    CheckInfo, ComplianceContext, ComplianceError, ComplianceFramework, ComplianceResult,
    ComplianceScanner, Finding, Severity,
};
use async_trait::async_trait;

/// STIG Scanner implementation
pub struct StigScanner {
    /// Version of STIG implemented
    version: String,
    /// Cached check definitions
    checks: Vec<Box<dyn ComplianceCheck>>,
}

impl StigScanner {
    /// Create a new STIG scanner
    pub fn new() -> Self {
        Self {
            version: "1.0.0".to_string(),
            checks: Self::build_checks(),
        }
    }

    /// Build all STIG checks
    fn build_checks() -> Vec<Box<dyn ComplianceCheck>> {
        let mut checks: Vec<Box<dyn ComplianceCheck>> = Vec::new();

        // Add authentication checks
        checks.extend(Self::authentication_checks());

        // Add access control checks
        checks.extend(Self::access_control_checks());

        // Add audit checks
        checks.extend(Self::audit_checks());

        checks
    }

    fn authentication_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // STIG V-230234 - Password complexity
            Box::new(
                CommandCheck::new(
                    "STIG-V-230234",
                    "The system must require passwords contain a minimum of 15 characters",
                    "grep -E '^\\s*minlen\\s*=' /etc/security/pwquality.conf | grep -oP '\\d+' | awk '{if($1>=15) exit 0; else exit 1}'",
                )
                .with_description(
                    "The shorter the password, the easier it is for an attacker to crack.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'minlen = 15' in /etc/security/pwquality.conf",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // STIG V-230235 - Require uppercase
            Box::new(
                CommandCheck::new(
                    "STIG-V-230235",
                    "The system must require uppercase characters in passwords",
                    "grep -E '^\\s*ucredit\\s*=' /etc/security/pwquality.conf | grep -oP '\\-?\\d+' | awk '{if($1<=-1) exit 0; else exit 1}'",
                )
                .with_description(
                    "Passwords must contain at least one uppercase character.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'ucredit = -1' in /etc/security/pwquality.conf",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // STIG V-230236 - Require lowercase
            Box::new(
                CommandCheck::new(
                    "STIG-V-230236",
                    "The system must require lowercase characters in passwords",
                    "grep -E '^\\s*lcredit\\s*=' /etc/security/pwquality.conf | grep -oP '\\-?\\d+' | awk '{if($1<=-1) exit 0; else exit 1}'",
                )
                .with_description(
                    "Passwords must contain at least one lowercase character.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'lcredit = -1' in /etc/security/pwquality.conf",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // STIG V-230237 - Require digits
            Box::new(
                CommandCheck::new(
                    "STIG-V-230237",
                    "The system must require numeric characters in passwords",
                    "grep -E '^\\s*dcredit\\s*=' /etc/security/pwquality.conf | grep -oP '\\-?\\d+' | awk '{if($1<=-1) exit 0; else exit 1}'",
                )
                .with_description(
                    "Passwords must contain at least one numeric character.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'dcredit = -1' in /etc/security/pwquality.conf",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // STIG V-230238 - Require special characters
            Box::new(
                CommandCheck::new(
                    "STIG-V-230238",
                    "The system must require special characters in passwords",
                    "grep -E '^\\s*ocredit\\s*=' /etc/security/pwquality.conf | grep -oP '\\-?\\d+' | awk '{if($1<=-1) exit 0; else exit 1}'",
                )
                .with_description(
                    "Passwords must contain at least one special character.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Authentication)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'ocredit = -1' in /etc/security/pwquality.conf",
                )
                .with_tag("authentication".to_string())
                .with_tag("password".to_string()),
            ),
            // STIG V-230309 - Disable SSH root login
            Box::new(
                CommandCheck::new(
                    "STIG-V-230309",
                    "The system must not permit root logins using SSH",
                    "grep -Ei '^\\s*PermitRootLogin\\s+no' /etc/ssh/sshd_config",
                )
                .with_description(
                    "Direct root logins must be disabled to enforce accountability.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::Ssh)
                .with_expected_exit_code(0)
                .with_remediation(
                    "Set 'PermitRootLogin no' in /etc/ssh/sshd_config",
                )
                .with_tag("ssh".to_string())
                .with_tag("authentication".to_string()),
            ),
        ]
    }

    fn access_control_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // STIG V-230244 - ASLR
            Box::new(
                SysctlCheck::new(
                    "STIG-V-230244",
                    "Address space layout randomization (ASLR) must be enabled",
                    "kernel.randomize_va_space",
                    "2",
                )
                .with_description(
                    "ASLR makes exploitation of memory corruption vulnerabilities more difficult.",
                )
                .with_severity(Severity::Medium)
                .with_remediation("Set 'kernel.randomize_va_space = 2' in /etc/sysctl.conf")
                .with_tag("kernel".to_string())
                .with_tag("memory".to_string()),
            ),
            // STIG V-230269 - /etc/passwd permissions
            Box::new(
                FileCheck::new(
                    "STIG-V-230269",
                    "The system must have correct permissions on /etc/passwd",
                    "/etc/passwd",
                )
                .with_description(
                    "The /etc/passwd file must be protected from unauthorized modification.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_group("root")
                .with_mode("644")
                .with_remediation("chown root:root /etc/passwd && chmod 644 /etc/passwd")
                .with_tag("permissions".to_string())
                .with_tag("user-accounts".to_string()),
            ),
            // STIG V-230270 - /etc/shadow permissions
            Box::new(
                FileCheck::new(
                    "STIG-V-230270",
                    "The system must have correct permissions on /etc/shadow",
                    "/etc/shadow",
                )
                .with_description(
                    "The /etc/shadow file contains password hashes and must be protected.",
                )
                .with_severity(Severity::High)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_mode("000")
                .with_remediation("chown root:root /etc/shadow && chmod 000 /etc/shadow")
                .with_tag("permissions".to_string())
                .with_tag("user-accounts".to_string()),
            ),
            // STIG V-230271 - /etc/group permissions
            Box::new(
                FileCheck::new(
                    "STIG-V-230271",
                    "The system must have correct permissions on /etc/group",
                    "/etc/group",
                )
                .with_description(
                    "The /etc/group file must be protected from unauthorized modification.",
                )
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_group("root")
                .with_mode("644")
                .with_remediation("chown root:root /etc/group && chmod 644 /etc/group")
                .with_tag("permissions".to_string())
                .with_tag("user-accounts".to_string()),
            ),
            // STIG V-230272 - /etc/gshadow permissions
            Box::new(
                FileCheck::new(
                    "STIG-V-230272",
                    "The system must have correct permissions on /etc/gshadow",
                    "/etc/gshadow",
                )
                .with_description("The /etc/gshadow file contains group password information.")
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::AccessControl)
                .with_owner("root")
                .with_mode("000")
                .with_remediation("chown root:root /etc/gshadow && chmod 000 /etc/gshadow")
                .with_tag("permissions".to_string())
                .with_tag("user-accounts".to_string()),
            ),
        ]
    }

    fn audit_checks() -> Vec<Box<dyn ComplianceCheck>> {
        vec![
            // STIG V-230386 - Audit must be installed
            Box::new(
                CommandCheck::new(
                    "STIG-V-230386",
                    "The audit system must be installed",
                    "rpm -q audit 2>/dev/null || dpkg -l auditd 2>/dev/null | grep -q '^ii'",
                )
                .with_description("The audit package is required for security event auditing.")
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Auditing)
                .with_expected_exit_code(0)
                .with_remediation("Install audit: yum install audit or apt install auditd")
                .with_tag("auditing".to_string()),
            ),
            // STIG V-230387 - Audit service must be enabled
            Box::new(
                ServiceCheck::new(
                    "STIG-V-230387",
                    "The audit service must be enabled",
                    "auditd",
                )
                .with_description(
                    "The auditd service must run to capture security-relevant events.",
                )
                .with_severity(Severity::Medium)
                .should_be_enabled(true)
                .should_be_running(true)
                .with_remediation("systemctl enable auditd && systemctl start auditd")
                .with_tag("auditing".to_string()),
            ),
            // STIG V-230388 - Audit log directory permissions
            Box::new(
                FileCheck::new(
                    "STIG-V-230388",
                    "The audit log directory must have mode 0750 or less",
                    "/var/log/audit",
                )
                .with_description("Audit logs contain sensitive security information.")
                .with_severity(Severity::Medium)
                .with_category(CheckCategory::Auditing)
                .with_owner("root")
                .with_mode("750")
                .with_remediation("chmod 0750 /var/log/audit")
                .with_tag("auditing".to_string())
                .with_tag("permissions".to_string()),
            ),
        ]
    }
}

impl Default for StigScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ComplianceScanner for StigScanner {
    fn framework(&self) -> ComplianceFramework {
        ComplianceFramework::Stig
    }

    fn name(&self) -> &str {
        "DISA STIG Scanner"
    }

    fn description(&self) -> &str {
        "Scans for compliance with DISA Security Technical Implementation Guides (STIG)"
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn scan(&self, context: &ComplianceContext) -> ComplianceResult<Vec<Finding>> {
        let mut findings = Vec::new();

        for check in &self.checks {
            let check_tags = check.tags();
            if !context.should_include_tag(&check_tags) {
                continue;
            }

            if check.severity() < context.severity_threshold {
                continue;
            }

            let result = check.execute(context).await?;

            let mut finding = Finding::new(check.id(), check.title(), ComplianceFramework::Stig)
                .with_description(check.description())
                .with_severity(check.severity())
                .with_status(result.status)
                .with_remediation(check.remediation());

            if let Some(observed) = result.observed {
                finding = finding.with_observed(observed);
            }

            for tag in check_tags {
                finding = finding.with_tag(tag);
            }

            findings.push(finding);
        }

        Ok(findings)
    }

    async fn run_check(
        &self,
        check_id: &str,
        context: &ComplianceContext,
    ) -> ComplianceResult<Finding> {
        let check = self
            .checks
            .iter()
            .find(|c| c.id() == check_id)
            .ok_or_else(|| {
                ComplianceError::InvalidConfig(format!("Check {} not found", check_id))
            })?;

        let result = check.execute(context).await?;

        let mut finding = Finding::new(check.id(), check.title(), ComplianceFramework::Stig)
            .with_description(check.description())
            .with_severity(check.severity())
            .with_status(result.status)
            .with_remediation(check.remediation());

        if let Some(observed) = result.observed {
            finding = finding.with_observed(observed);
        }

        Ok(finding)
    }

    fn list_checks(&self) -> Vec<&str> {
        self.checks.iter().map(|c| c.id()).collect()
    }

    fn get_check_info(&self, check_id: &str) -> Option<CheckInfo> {
        self.checks
            .iter()
            .find(|c| c.id() == check_id)
            .map(|c| CheckInfo {
                id: c.id().to_string(),
                title: c.title().to_string(),
                description: c.description().to_string(),
                severity: c.severity(),
                tags: c.tags(),
                auto_remediable: false,
                remediation_time_minutes: Some(5),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stig_scanner_creation() {
        let scanner = StigScanner::new();
        assert_eq!(scanner.framework(), ComplianceFramework::Stig);
        assert!(!scanner.list_checks().is_empty());
    }

    #[test]
    fn test_list_stig_checks() {
        let scanner = StigScanner::new();
        let checks = scanner.list_checks();
        assert!(checks.iter().any(|c| c.starts_with("STIG-")));
    }
}
